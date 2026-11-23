# Differences between giflib and browsers

giflib 5.2.1 is a library that gives you the records. Browsers (Firefox, Chromium with Wuffs,
libnsgif) show an image. They differ in several places. This page lists each one and says which
side gif-rs follows. Where the policy is "giflib", gif-rs matches giflib's output and errors
exactly. Where it is "browsers", gif-rs and both oracles share the policy, so parity stays exact.

| Case | giflib | Browsers | gif-rs follows |
|---|---|---|---|
| Canvas size | Composites nothing; screen and frame sizes stay apart | The image grows to fit the first frame | Browsers: the canvas is `max(screen, left + frame width)` by `max(screen, top + frame height)` |
| Background colour | Kept as an index only | Ignored; the canvas starts transparent | Browsers: not painted; kept in `Info::background_index` |
| More than one GCE before an image | `DGifSavedExtensionToGCB` takes the first | The last one applies | Browsers: the last GCE applies |
| GCE first sub-block shorter than 4 bytes | Not a valid GCE; ignored | Ignored | Same: ignored, kept in `extensions` |
| Pixel index past the colour table | Reads past its array (undefined) | The table is padded with black | Browsers: opaque black `(0, 0, 0, 255)` |
| No local and no global colour table | Pixels have no colour | A default or no image | `NoColorTable`, after the LZW decode succeeded |
| EOF code before the last pixel | `EofTooSoon` | Shows the partial image | giflib: `EofTooSoon`, no output |
| Truncated data | Error | Shows what decoded | giflib: the error, no output |
| Sub-block data after the last pixel | Read and dropped to the 0 terminator | Same | giflib: dropped; a short read is `ReadFailed` |
| LZW minimum code size above 8 | `ReadFailed` | Often accepted up to 11 or 12 | giflib: `ReadFailed` |
| Version `87a` or `89a` | Not checked after `GIF` | Not checked | Same: the 3 version bytes are kept, never checked |
| Frame count with a broken later frame | n/a | Counts frames that decoded | A later image counts only if its sub-blocks reach the terminator; the scan then stops, `complete = false`, no error |
| Trailer before any image | No code | Blank or no image | `NoImage` (policy) |

## Notes

- Errors give no output. A lenient mode that returns a partial image is not in v1.
- A scan error after the first frame is never returned. It only sets `complete = false`.
- Only the first frame is composited, so disposal and timing do not matter in v1. They stay in
  `Info::frame.control` and `extensions` for v2.
