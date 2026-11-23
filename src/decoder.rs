//! `Decoder`, the entry points and the result types.

use crate::compose;
use crate::error::Error;
use crate::ledger::Ledger;
use crate::limits::Limits;
use crate::lzw::Lzw;
use crate::parse::{self, Record};
use crate::reader::Reader;
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

/// What the file says, and what the decoder worked out. `read_info` and `decode` fill it the
/// same way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Info {
    /// Bytes 3..6 of the header as read (`87a`, `89a` or anything: they are not checked).
    pub version: [u8; 3],
    /// Logical screen width.
    pub screen_width: u16,
    /// Logical screen height.
    pub screen_height: u16,
    /// The packed byte of the logical screen descriptor, raw.
    pub screen_flags: u8,
    /// The background colour index. Kept as data; it is not used to paint.
    pub background_index: u8,
    /// The pixel aspect ratio byte.
    pub aspect: u8,
    /// The byte range of the global colour table in the input.
    pub global_table: Option<Range<usize>>,
    /// Canvas width: the larger of the screen width and `frame.left + frame.width`.
    pub width: u32,
    /// Canvas height: the larger of the screen height and `frame.top + frame.height`.
    pub height: u32,
    /// The first frame.
    pub frame: Frame,
    /// 1 plus the number of later image descriptors whose sub-blocks reach the terminator.
    pub frame_count: u32,
    /// The scan after the first frame reached the trailer (0x3B) with no error.
    pub complete: bool,
    /// Every extension block in the file, GCEs included, in order (at most
    /// `Limits::max_extensions`).
    pub extensions: Vec<Extension>,
    /// Some extension blocks are not in `extensions` because of `Limits::max_extensions` (or
    /// `Limits::max_alloc`).
    pub extensions_truncated: bool,
}

impl Info {
    /// Bytes of the RGBA output: `width * height * 4`. `usize::MAX` if that does not fit.
    pub fn output_size(&self) -> usize {
        compose::rgba_size(self.width, self.height).unwrap_or(usize::MAX)
    }
}

/// The first frame's image descriptor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frame {
    /// Column of the frame's left edge on the screen.
    pub left: u16,
    /// Row of the frame's top edge on the screen.
    pub top: u16,
    /// Frame width.
    pub width: u16,
    /// Frame height.
    pub height: u16,
    /// The packed byte of the image descriptor, raw.
    pub flags: u8,
    /// The rows are interlaced.
    pub interlaced: bool,
    /// The byte range of the local colour table in the input.
    pub local_table: Option<Range<usize>>,
    /// The LZW minimum code size (0 to 8).
    pub min_code_size: u8,
    /// The graphic control extension that applies to this frame: the last valid GCE before the
    /// image descriptor.
    pub control: Option<Control>,
}

/// A graphic control extension.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Control {
    /// Disposal method, 0 to 7.
    pub disposal: u8,
    /// The user input flag.
    pub user_input: bool,
    /// Delay in 1/100 s.
    pub delay: u16,
    /// The transparent colour index, if the transparent flag is set.
    pub transparent: Option<u8>,
}

/// One extension block, kept as a byte range into the input (no copy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extension {
    /// The label: 0xF9 graphic control, 0xFE comment, 0xFF application, 0x01 plain text, or
    /// another byte.
    pub label: u8,
    /// The number of image descriptors before this block.
    pub frame: u32,
    /// The sub-blocks in the input: the length bytes and the 0 terminator included.
    pub raw: Range<usize>,
}

/// The XMP application identifier plus authentication code.
const XMP_ID: &[u8; 11] = b"XMP DataXMP";
/// Length of the XMP magic trailer: 0x01, then 0xFF down to 0x00.
const XMP_MAGIC_LEN: usize = 257;

fn is_xmp_magic(t: &[u8]) -> bool {
    t.len() == XMP_MAGIC_LEN
        && t.first() == Some(&1)
        && t.iter()
            .skip(1)
            .zip((0..=255u8).rev())
            .all(|(&a, b)| a == b)
}

impl Extension {
    /// The raw sub-blocks of this block in `data` (the input given to the call that made this
    /// `Extension`). Empty if `data` is not that input.
    pub fn bytes<'a>(&self, data: &'a [u8]) -> &'a [u8] {
        data.get(self.raw.clone()).unwrap_or_default()
    }

    /// The payload of each sub-block, without the length byte and the terminator.
    pub fn sub_blocks<'a>(&self, data: &'a [u8]) -> impl Iterator<Item = &'a [u8]> + use<'a> {
        let mut rest = self.bytes(data);
        std::iter::from_fn(move || {
            let (&n, tail) = rest.split_first()?;
            if n == 0 {
                return None;
            }
            let n = usize::from(n);
            let block = tail.get(..n)?;
            rest = tail.get(n..)?;
            Some(block)
        })
    }

    /// The 11-byte application identifier and authentication code, for label 0xFF.
    pub fn application_id<'a>(&self, data: &'a [u8]) -> Option<&'a [u8]> {
        if self.label != 0xFF {
            return None;
        }
        self.sub_blocks(data).next().filter(|b| b.len() == 11)
    }

    /// The XMP packet of an application extension whose first sub-block is `XMP DataXMP`: the
    /// raw bytes after that sub-block (the sub-block length bytes are part of the packet text,
    /// as part 3 of the XMP specification says) up to the 257-byte magic trailer and the 0
    /// terminator. If the trailer is absent, everything up to the terminator.
    pub fn xmp<'a>(&self, data: &'a [u8]) -> Option<&'a [u8]> {
        if self.application_id(data)? != XMP_ID {
            return None;
        }
        let raw = self.bytes(data);
        // 1 length byte and the 11 identifier bytes in front, the 0 terminator behind.
        let body = raw.get(12..raw.len().checked_sub(1)?)?;
        if let Some(cut) = body.len().checked_sub(XMP_MAGIC_LEN)
            && body.get(cut..).is_some_and(is_xmp_magic)
        {
            return body.get(..cut);
        }
        Some(body)
    }
}

/// Where the pixels go.
enum Mode<'a> {
    /// Parse and scan; skip the first frame's LZW data.
    Info,
    /// Decode into the caller's slice.
    Into(&'a mut [u8]),
    /// Decode into a new `Vec`, charged to the ledger.
    Alloc(&'a mut Vec<u8>),
}

/// A decoder that keeps its tables and buffers across calls. A second call with an input of the
/// same or smaller size and extension count makes no heap allocation.
///
/// Memory the decoder keeps counts against `Limits::max_alloc` of every call.
#[derive(Default)]
pub struct Decoder {
    info: Info,
    lzw: Option<Box<Lzw>>,
    line: Vec<u8>,
    ledger: Ledger,
}

/// Reads the headers and the metadata of `data` without decoding any pixel.
///
/// Parses up to the first image descriptor, skips the first frame's LZW sub-blocks (so LZW
/// errors and [`Error::NoColorTable`] are not found here), then scans the rest. The result is
/// the [`Info`] that [`decode`] gives for the same input.
///
/// Limits: `max_width`, `max_height` and `max_pixels` apply to the canvas; `max_extensions`
/// caps `Info::extensions`; `max_alloc` caps the extension list. The cancel flag is read per
/// record.
///
/// # Errors
///
/// [`Error::ReadFailed`], [`Error::NotGif`], [`Error::NoScreenDescriptor`],
/// [`Error::WrongRecord`], [`Error::NoImage`] for a bad file; [`Error::WidthLimit`],
/// [`Error::HeightLimit`], [`Error::PixelLimit`], [`Error::AllocLimit`] and
/// [`Error::OutOfMemory`] for the limits; [`Error::Cancelled`]. A bad record after the first
/// frame is not an error: it sets `complete = false`.
pub fn read_info(data: &[u8], limits: &Limits, cancel: &AtomicBool) -> Result<Info, Error> {
    let mut d = Decoder::new();
    d.run(data, Mode::Info, limits, cancel)?;
    Ok(std::mem::take(&mut d.info))
}

/// Decodes the first frame of `data`, composited on the canvas, as RGBA8 (rows top to bottom,
/// `info.width * info.height * 4` bytes).
///
/// Limits: `max_width`, `max_height` and `max_pixels` apply to the canvas and are checked
/// before the output is allocated. `max_alloc` caps the peak memory of the call: the output, the
/// LZW tables, the line buffer and the extension list. `max_extensions` caps `Info::extensions`.
/// The cancel flag is read per record and per decoded line.
///
/// # Errors
///
/// [`Error::ReadFailed`], [`Error::NotGif`], [`Error::NoScreenDescriptor`],
/// [`Error::WrongRecord`], [`Error::DataTooBig`], [`Error::ImageDefect`],
/// [`Error::EofTooSoon`], [`Error::NoImage`] and [`Error::NoColorTable`] for a bad file (the
/// giflib rules); [`Error::WidthLimit`], [`Error::HeightLimit`], [`Error::PixelLimit`],
/// [`Error::AllocLimit`] and [`Error::OutOfMemory`] for the limits; [`Error::Cancelled`]. An
/// error gives no output. A bad record after the first frame is not an error: it sets
/// `complete = false`.
pub fn decode(data: &[u8], limits: &Limits, cancel: &AtomicBool) -> Result<(Info, Vec<u8>), Error> {
    let mut d = Decoder::new();
    let mut out = Vec::new();
    d.run(data, Mode::Alloc(&mut out), limits, cancel)?;
    Ok((std::mem::take(&mut d.info), out))
}

impl Decoder {
    /// A decoder with no buffers yet. The first call allocates them.
    pub fn new() -> Self {
        Self::default()
    }

    /// [`read_info`] with this decoder's buffers. The returned `Info` is valid until the next
    /// call.
    ///
    /// # Errors
    ///
    /// The same as [`read_info`].
    pub fn read_info(
        &mut self,
        data: &[u8],
        limits: &Limits,
        cancel: &AtomicBool,
    ) -> Result<&Info, Error> {
        self.run(data, Mode::Info, limits, cancel)?;
        Ok(&self.info)
    }

    /// Decodes the first frame into `out` as RGBA8. `out.len()` must be
    /// [`Info::output_size`] (call [`Decoder::read_info`] first). Makes no heap allocation if
    /// this decoder already decoded an input of the same or smaller size and extension count.
    ///
    /// Limits: as [`decode`]. The caller's `out` is not charged to `max_alloc`.
    ///
    /// # Errors
    ///
    /// The same as [`decode`]. [`Error::OutputSize`] if `out.len()` is not the output size; it
    /// is found after the limit checks and the LZW decode, so a bad file gives its own error
    /// first. `out` is not written on an error, except that it may hold a partial image.
    pub fn decode_into(
        &mut self,
        data: &[u8],
        out: &mut [u8],
        limits: &Limits,
        cancel: &AtomicBool,
    ) -> Result<&Info, Error> {
        self.run(data, Mode::Into(out), limits, cancel)?;
        Ok(&self.info)
    }

    /// Records an extension block. A block that does not fit the limits is not recorded and
    /// `extensions_truncated` is set: metadata never fails a call.
    fn record(&mut self, label: u8, frame: u32, raw: Range<usize>, limits: &Limits) {
        let exts = &mut self.info.extensions;
        if exts.len() >= limits.max_extensions {
            self.info.extensions_truncated = true;
            return;
        }
        if exts.len() == exts.capacity() {
            let cap = exts.capacity();
            let grown = if cap == 0 { 8 } else { cap.saturating_mul(2) };
            let new_cap = grown.min(limits.max_extensions);
            let extra = new_cap - cap;
            let fits = self
                .ledger
                .reserve(extra.saturating_mul(size_of::<Extension>()))
                .is_ok()
                && exts.try_reserve_exact(extra).is_ok();
            if !fits {
                self.info.extensions_truncated = true;
                return;
            }
        }
        exts.push(Extension { label, frame, raw });
    }

    /// Decodes the first frame with the call pattern of `DGifSlurp`: one `DGifGetLine` call for a
    /// frame that is not interlaced, one call per row (in the interlace order) for one that is.
    /// A frame that is not interlaced and has pixels runs as one call per row, which gives the
    /// same pixels and the same error. Paints each row on `out` if there is one. Gives whether
    /// the frame has a colour table.
    fn decode_frame(
        &mut self,
        r: &mut Reader<'_>,
        data: &[u8],
        f: &Frame,
        mut out: Option<&mut [u8]>,
        canvas_width: u32,
        cancel: &AtomicBool,
    ) -> Result<bool, Error> {
        // DGifSlurp (dgif_lib.c:1144-1148): an empty frame, or one of more than i32::MAX
        // pixels, is GIF_ERROR with no code set, before any DGifGetLine call.
        if f.width == 0
            || f.height == 0
            || u64::from(f.width) * u64::from(f.height) > i32::MAX as u64
        {
            return Err(Error::ReadFailed);
        }
        // The line buffer: one row of the frame.
        let fw = usize::from(f.width);
        if self.line.capacity() < fw {
            self.ledger.reserve(fw - self.line.capacity())?;
        }
        self.line.clear();
        self.line
            .try_reserve_exact(fw)
            .map_err(|_| Error::OutOfMemory)?;
        self.line.resize(fw, 0);
        // The LZW tables, boxed once.
        if self.lzw.is_none() {
            self.ledger.reserve(size_of::<Lzw>())?;
            self.lzw = Some(Box::new(Lzw::new()));
        }
        let Some(lzw) = self.lzw.as_deref_mut() else {
            return Err(Error::OutOfMemory);
        };

        let table = f
            .local_table
            .clone()
            .or_else(|| self.info.global_table.clone());
        let table = table.and_then(|range| data.get(range));
        let pal = compose::palette(table, f.control.and_then(|c| c.transparent));
        lzw.setup(f.min_code_size, f.width, f.height);

        let passes: &[(u32, u32)] = if f.interlaced {
            &[(0, 8), (4, 8), (2, 4), (1, 2)]
        } else {
            &[(0, 1)]
        };
        let height = u32::from(f.height);
        for &(first, step) in passes {
            let mut y = first;
            while y < height {
                if cancel.load(Ordering::Relaxed) {
                    return Err(Error::Cancelled);
                }
                lzw.get_line(r, &mut self.line, fw)?;
                if let (Some(pal), Some(o)) = (&pal, out.as_deref_mut()) {
                    let row =
                        compose::row_mut(o, canvas_width, f.left, u32::from(f.top) + y, f.width)
                            .ok_or(Error::OutputSize)?;
                    compose::paint_row(row, &self.line, pal);
                }
                y += step;
            }
        }
        Ok(pal.is_some())
    }

    fn run(
        &mut self,
        data: &[u8],
        mode: Mode<'_>,
        limits: &Limits,
        cancel: &AtomicBool,
    ) -> Result<(), Error> {
        // A new Info that keeps the extension list's buffer.
        let exts = std::mem::take(&mut self.info.extensions);
        self.info = Info {
            extensions: exts,
            ..Info::default()
        };
        self.info.extensions.clear();
        let held = (if self.lzw.is_some() {
            size_of::<Lzw>()
        } else {
            0
        })
        .saturating_add(
            self.info
                .extensions
                .capacity()
                .saturating_mul(size_of::<Extension>()),
        )
        .saturating_add(self.line.capacity());
        self.ledger.reset(limits.max_alloc, held)?;

        // DGifOpen, then the records up to the first image descriptor.
        let mut r = Reader::new(data);
        parse::header(&mut r, &mut self.info)?;
        let mut control = None;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            match parse::record_type(&mut r)? {
                Record::Image => break,
                Record::Trailer => return Err(Error::NoImage),
                Record::Extension => {
                    let (label, raw) = parse::extension(&mut r)?;
                    // The last valid GCE wins.
                    if label == 0xF9
                        && let Some(c) = parse::gce(data, &raw)
                    {
                        control = Some(c);
                    }
                    self.record(label, 0, raw, limits);
                }
            }
        }
        let frame = parse::image_desc(&mut r, control)?;

        // The canvas and its limits, before any buffer is sized from it.
        let (cw, ch) = compose::canvas(self.info.screen_width, self.info.screen_height, &frame);
        if cw > limits.max_width {
            return Err(Error::WidthLimit);
        }
        if ch > limits.max_height {
            return Err(Error::HeightLimit);
        }
        if u64::from(cw) * u64::from(ch) > limits.max_pixels {
            return Err(Error::PixelLimit);
        }
        self.info.width = cw;
        self.info.height = ch;
        self.info.frame = frame.clone();
        self.info.frame_count = 1;

        // The output. The canvas starts as transparent black.
        let mut bad_size = false;
        let want_pixels = !matches!(mode, Mode::Info);
        let degenerate = frame.width == 0 || frame.height == 0;
        let (out, decode): (Option<&mut [u8]>, bool) = match mode {
            Mode::Info => (None, degenerate),
            Mode::Into(o) => {
                if compose::rgba_size(cw, ch) == Some(o.len()) {
                    o.fill(0);
                    (Some(o), true)
                } else {
                    bad_size = true;
                    (None, true)
                }
            }
            Mode::Alloc(v) => {
                let n = compose::rgba_size(cw, ch).ok_or(Error::AllocLimit)?;
                self.ledger.reserve(n)?;
                v.clear();
                v.try_reserve_exact(n).map_err(|_| Error::OutOfMemory)?;
                v.resize(n, 0);
                (Some(v.as_mut_slice()), true)
            }
        };

        // The first frame. `read_info` skips the LZW data, but a frame with no pixels runs the
        // giflib calls (they decide how much data is read).
        if decode {
            let has_table = self.decode_frame(&mut r, data, &frame, out, cw, cancel)?;
            if want_pixels && !has_table {
                return Err(Error::NoColorTable);
            }
            if bad_size {
                return Err(Error::OutputSize);
            }
        } else {
            parse::skip_blocks(&mut r)?;
        }

        // The scan: records after the first frame. A scan error only ends the scan.
        let mut images = 1u32;
        let mut complete = false;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let Ok(rec) = parse::record_type(&mut r) else {
                break;
            };
            match rec {
                Record::Trailer => {
                    complete = true;
                    break;
                }
                Record::Extension => {
                    let Ok((label, raw)) = parse::extension(&mut r) else {
                        break;
                    };
                    self.record(label, images, raw, limits);
                }
                Record::Image => {
                    if parse::image_desc(&mut r, None).is_err()
                        || parse::skip_blocks(&mut r).is_err()
                    {
                        break;
                    }
                    images = images.saturating_add(1);
                }
            }
        }
        self.info.frame_count = images;
        self.info.complete = complete;
        Ok(())
    }
}
