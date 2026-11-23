# gif-rs design (v1)

The build spec. The crate, the C oracle (`tests/parity/c/giforacle.c`) and the second oracle
(`tests/parity/imagers/`) all follow this file. Where it says "giflib" it means giflib 5.2.1
(Debian bookworm `libgif-dev` 5.2.1-2.5), `dgif_lib.c`.

Scope v1: the FIRST frame only, composited on the canvas, as RGBA8. A cheap scan of the rest of
the file gives the frame count and the raw extension blocks. Not in v1: later frames, disposal,
timing (all are kept as data so v2 can add them).

## API

```rust
pub struct Limits {            // Default:
    pub max_alloc: usize,      // 1 GiB. Peak working memory of one call (output Vec of
                               //   `decode` included). Over: Error::AllocLimit.
    pub max_threads: usize,    // available_parallelism(). gif-rs never spawns a thread (LZW is
                               //   serial); the field exists for the shared convention.
    pub max_width: u32,        // 1 << 17 (covers the largest canvas, 2 * 65535). Error::WidthLimit
    pub max_height: u32,       // 1 << 17. Error::HeightLimit
    pub max_pixels: u64,       // 1 << 28 (canvas width * height). Error::PixelLimit
    pub max_extensions: usize, // 4096 extension blocks recorded in Info::extensions. Over: the
                               //   rest are not recorded and Info::extensions_truncated = true
                               //   (no error: metadata never fails a decode).
}

pub fn read_info(data: &[u8], limits: &Limits, cancel: &AtomicBool) -> Result<Info, Error>;
pub fn decode(data: &[u8], limits: &Limits, cancel: &AtomicBool) -> Result<(Info, Vec<u8>), Error>;

#[derive(Default)]
pub struct Decoder { /* Info, LZW tables, line buffer: kept across calls */ }
impl Decoder {
    pub fn new() -> Self;
    pub fn read_info(&mut self, data: &[u8], limits: &Limits, cancel: &AtomicBool) -> Result<&Info, Error>;
    /// `out.len()` must be `info.output_size()` (read_info first), else Error::OutputSize.
    pub fn decode_into(&mut self, data: &[u8], out: &mut [u8], limits: &Limits, cancel: &AtomicBool) -> Result<&Info, Error>;
}
```

- A later `decode_into` (or `read_info`) on a `Decoder` makes ZERO heap allocations if its first
  frame is no wider, and it has no more extension blocks, than an earlier decode on that
  `Decoder` (tests/alloc.rs proves it with a counting global allocator).
- `read_info` parses up to the first image descriptor, skips (does not decode) the first frame's
  LZW sub-blocks, then scans the rest. `decode`/`decode_into` decode the first frame instead of
  skipping it, then scan the rest. Both fill the same `Info`, with one exception: under a tight
  `max_alloc`, `extensions` and `extensions_truncated` can differ between `read_info`, `decode`
  and `decode_into`, because the list is charged against what the output and the tables left.
  A first frame with width 0, height 0 or over `i32::MAX` pixels is `ReadFailed` in every mode,
  checked right after the canvas limits and before any output is sized.
- Cancel: checked per record, per decoded line, per scanned frame, and at every LZW clear code
  (a clear writes no pixel, so a flood of them would otherwise never reach the per-line check).
  Set: Error::Cancelled.

```rust
pub struct Info {
    pub version: [u8; 3],             // bytes 3..6 as read ("87a", "89a", anything: not checked)
    pub screen_width: u16, pub screen_height: u16,
    pub screen_flags: u8,             // packed byte of the logical screen descriptor, raw
    pub background_index: u8,         // kept as data, NOT used to paint (see Composition)
    pub aspect: u8,
    pub global_table: Option<Range<usize>>, // byte range of the global colour table in `data`
    pub width: u32, pub height: u32,  // the canvas (see Composition)
    pub frame: Frame,                 // the first frame
    pub frame_count: u32,             // see Scan
    pub complete: bool,               // the scan reached the trailer (0x3B) with no error
    pub extensions: Vec<Extension>,   // every extension block in the file, GCEs included, in order
    pub extensions_truncated: bool,
}
pub struct Frame {
    pub left: u16, pub top: u16, pub width: u16, pub height: u16,
    pub flags: u8,                    // packed byte of the image descriptor, raw
    pub interlaced: bool,
    pub local_table: Option<Range<usize>>,
    pub min_code_size: u8,
    pub control: Option<Control>,     // the GCE that applies to this frame
}
pub struct Control { pub disposal: u8, pub user_input: bool, pub delay: u16, pub transparent: Option<u8> }
pub struct Extension {
    pub label: u8,                    // 0xF9 GCE, 0xFE comment, 0xFF application, 0x01 plain text, other
    pub frame: u32,                   // number of image descriptors before this block
    pub raw: Range<usize>,            // the sub-blocks in `data`: length bytes and the 0 terminator included
}
impl Extension {
    pub fn bytes<'a>(&self, data: &'a [u8]) -> &'a [u8];
    pub fn sub_blocks<'a>(&self, data: &'a [u8]) -> impl Iterator<Item = &'a [u8]>; // payloads
    pub fn application_id<'a>(&self, data: &'a [u8]) -> Option<&'a [u8]>; // 11 bytes, label 0xFF only
    pub fn xmp<'a>(&self, data: &'a [u8]) -> Option<&'a [u8]>;
}
impl Info { pub fn output_size(&self) -> usize; } // width * height * 4
```

Metadata rule (owner, 2026-11-19): keep, never drop. Every extension block is kept as a byte
range into the input (no copy), GCEs and unknown labels included. `xmp()`: for an application
extension whose first sub-block is the 11 bytes `XMP DataXMP`, the XMP packet is the raw bytes
after that sub-block (sub-block length bytes are part of the packet text, as the XMP spec part 3
says) up to the 257-byte "magic trailer" (0x01, 0xFF, 0xFE, ..., 0x01, 0x00) and the 0
terminator; if the trailer is absent, everything up to the terminator.

## Parsing (giflib semantics)

The parse follows giflib's `DGifOpen`, `DGifGetRecordType`, `DGifGetImageDesc`,
`DGifGetExtension(Next)`, `DGifGetCode(Next)`, step by step, including which error each failure
gives. Highlights (read dgif_lib.c for the rest):

- Header: 6 bytes; fewer = `ReadFailed`. Only the first 3 bytes are checked (`GIF`), else
  `NotGif`. The version is not checked.
- Logical screen descriptor (7 bytes) plus the global table (3 * 2^(1+(flags&7)) bytes if
  flags&0x80): any short read here = `NoScreenDescriptor` (DGifOpen maps every
  DGifGetScreenDesc failure to D_GIF_ERR_NO_SCRN_DSCR).
- Records until the first image descriptor: 0x2C image, 0x21 extension, 0x3B trailer, other byte
  = `WrongRecord`, end of data = `ReadFailed`. A trailer before any image = `NoImage` (policy;
  giflib has no code for it). Extension: label byte, then sub-blocks (length byte, then that many
  bytes) to a 0 byte; a short read = `ReadFailed`.
- Image descriptor: 9 bytes, local table (3 * 2^(1+(flags&7)) bytes if flags&0x80), LZW minimum
  code size byte. Short read = `ReadFailed`. Code size byte missing: giflib returns an error with
  no code set; gif-rs maps it to `ReadFailed`. Code size > 8 = `ReadFailed` (giflib's code).
- GCE (label 0xF9): the first sub-block must be at least 4 bytes, else the GCE is ignored (kept
  in `extensions` only). giflib's `DGifExtensionToGCB` is stricter (length must be exactly 4);
  gif-rs and the oracle use the first 4 bytes of a longer block (see docs/differences.md). Bytes: packed (disposal = (p >> 2) & 7, user_input = p & 2, transparent
  flag = p & 1), delay u16 LE, transparent index. The LAST GCE before the first image descriptor
  applies (browsers; giflib's DGifSavedExtensionToGCB takes the first, see docs/differences.md).

## LZW (port of giflib)

`src/lzw.rs` ports giflib 5.2.1 `DGifSetupDecompress`, `DGifGetLine`, `DGifDecompressLine`,
`DGifGetPrefixChar`, `DGifDecompressInput`, `DGifBufferedInput` exactly (giflib is MIT; the
attribution is in the file header and in NOTICE). Keep the same state (Prefix/Suffix/Stack
arrays of LZ_MAX_CODE+1 = 4096, RunningCode, RunningBits, MaxCode1, CrntShiftState/DWord,
LastCode, StackPtr, the sub-block buffer `Buf[0]` count and `Buf[1]` cursor), the same order of
checks and the same error at every point:

- an EOF code before the last pixel = `EofTooSoon` (5.2.1 has no "last pixel" exception);
- an empty sub-block inside the LZW data = `ImageDefect`; RunningBits > 12 = `ImageDefect`;
  a prefix chain longer than the stack or a prefix > 4095 = `ImageDefect`;
- deferred clear: at RunningCode 4097 the table stops growing and decoding goes on with 12-bit
  codes until a clear code;
- `DGifGetPrefixChar` returns NO_SUCH_CODE (4098) on a bad chain, and giflib stores it into a
  byte array: keep the truncation (`as u8`) so the output pixel is the same;
- `DGifGetLine`: PixelCount = width * height (i64), each call subtracts LineLen (LineLen 0 means
  width) and fails with `DataTooBig` if the result as u64 is > 0xffff0000; after the call that
  brings PixelCount to 0, read and drop the remaining sub-blocks to the 0 terminator (a short
  read = `ReadFailed`);
- a short read anywhere in the LZW data = `ReadFailed`.

Call pattern = DGifSlurp's, for the first frame only: non-interlaced, one DGifGetLine call with
LineLen = width * height; interlaced, one call per row of width pixels in the order
(offset 0 step 8), (4, 8), (2, 4), (1, 2). gif-rs may run the non-interlaced call as one
streaming state machine that emits rows; the pixels and the error must be the same as the one
call. A frame with width 0, height 0 or more than i32::MAX pixels is `ReadFailed` before any call
(DGifSlurp's guard, dgif_lib.c:1144-1148). Pixels not written before an error do not matter (an error gives no output).

## Composition (policy shared by gif-rs and both oracles)

- Canvas: width = max(screen_width, left + frame.width), height = max(screen_height,
  top + frame.height) (browser convention: Firefox, Chromium/Wuffs and libnsgif grow the image to
  the first frame; giflib composites nothing). Limits are checked on the canvas before any
  allocation.
- Canvas starts as transparent black (0, 0, 0, 0). The background colour index is NOT painted
  (browsers ignore it; it stays in `Info::background_index`).
- Each frame pixel at (left + x, top + y): if a GCE applies, has the transparent flag set and the
  index equals its transparent index, the canvas pixel stays (0, 0, 0, 0). Else the colour is the
  local table entry if the frame has a local table, else the global table entry, alpha 255. An
  index at or past the table's entry count gives opaque black (0, 0, 0, 255) (Firefox pads the
  table with black; giflib reads past its array).
- No local and no global table: `NoColorTable` (after the LZW decode succeeded, so LZW errors
  come first).
- Output: `width * height * 4` bytes, rows top to bottom, R G B A.

## Scan (frame count)

After the first frame (decoded or skipped), read records the same way as the parse, without LZW
decode: an image descriptor (9 bytes, local table, code size byte with the > 8 check) then its
sub-blocks to the 0 terminator; extension blocks recorded in `extensions`. `frame_count` = 1 +
the number of later image descriptors whose sub-blocks were read to the terminator. The scan
stops at the trailer (`complete = true`) or at the first error of any kind, including end of
data (`complete = false`); a scan error is never returned as an error.

## Errors

One enum, `#[non_exhaustive]`, `Debug + Clone + PartialEq + Eq + Display + std::error::Error`.
The parity class token is in brackets; the oracle writes the same token for the giflib code.

| Variant | Token | giflib |
|---|---|---|
| `ReadFailed` | `read-failed` | D_GIF_ERR_READ_FAILED (102), or no code set |
| `NotGif` | `not-gif` | D_GIF_ERR_NOT_GIF_FILE (103) |
| `NoScreenDescriptor` | `no-scrn-dscr` | D_GIF_ERR_NO_SCRN_DSCR (104) |
| `WrongRecord` | `wrong-record` | D_GIF_ERR_WRONG_RECORD (107) |
| `DataTooBig` | `data-too-big` | D_GIF_ERR_DATA_TOO_BIG (108) |
| `ImageDefect` | `image-defect` | D_GIF_ERR_IMAGE_DEFECT (112) |
| `EofTooSoon` | `eof-too-soon` | D_GIF_ERR_EOF_TOO_SOON (113) |
| `NoImage` | `no-image` | policy: trailer before any image |
| `NoColorTable` | `no-color-table` | policy |
| `WidthLimit`, `HeightLimit`, `PixelLimit`, `AllocLimit` | `limit` | - |
| `OutOfMemory` | `oom` | D_GIF_ERR_NOT_ENOUGH_MEM (109) |
| `OutputSize` | `output-size` | caller error (the oracles never write it) |
| `Cancelled` | `cancelled` | - (the oracles never write it) |

`Error::token(&self) -> &'static str` returns the token.

## Parity output format (`examples/oracle.rs`, `giforacle`, `imagers-oracle`)

`<oracle> <in-dir> <out-dir>`: for every `*.gif` file under `in-dir` (recursive, sorted), with
`stem` = the path relative to `in-dir` with `/` replaced by `__`:

- success: `<stem>.rgba` (the raw canvas bytes) and `<stem>.info` (text, `\n` line ends):
  ```
  canvas <width> <height>
  screen <screen_width> <screen_height> <screen_flags> <background_index> <aspect>
  frame <left> <top> <width> <height> <flags> <min_code_size>
  control <disposal> <user_input 0|1> <delay> <transparent index or ->     (or: control -)
  frames <frame_count> <complete 0|1>
  ext <frame> <label as 2 lower hex> <raw length> <raw bytes as lower hex>  (one line per extension)
  ```
- error: `<stem>.err` holding the token and `\n`.

The giflib oracle reads the file with DGifOpen (memory input function) and the low-level calls,
mirrors the record loop above, decodes the first frame with DGifGetLine in DGifSlurp's call
pattern into a zeroed buffer, composites per the policy, then scans with DGifGetRecordType,
DGifGetImageDesc, DGifGetCode/DGifGetCodeNext and DGifGetExtension/DGifGetExtensionNext.

## Not in v1

Later frames, disposal, timing, a partial image on error, a lenient (browser) mode for truncated
files, SIMD (no kernel pays: LZW is serial and the palette lookup is one 4-byte copy per pixel),
threads.
