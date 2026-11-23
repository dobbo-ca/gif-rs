# Limits

`Limits` is a plain struct with public fields. Every entry point takes `&Limits` (`decode`,
`read_info`, `Decoder::read_info`, `Decoder::decode_into`) and a `&AtomicBool` cancel flag.

| Field | Default | Unit | Error when exceeded |
|---|---|---|---|
| `max_alloc` | 1 GiB | bytes | `AllocLimit` |
| `max_threads` | `available_parallelism()` | threads | none (never used) |
| `max_width` | `1 << 17` | pixels, canvas | `WidthLimit` |
| `max_height` | `1 << 17` | pixels, canvas | `HeightLimit` |
| `max_pixels` | `1 << 28` | canvas width x height | `PixelLimit` |
| `max_extensions` | 4096 | extension blocks | none: the rest are not recorded |

## What is checked, and when

- The canvas is `max(screen, left + frame width)` by `max(screen, top + frame height)`. The three
  size limits apply to the canvas. They are checked before any buffer is sized from it.
- `max_alloc` is the peak working memory of one call. The output `Vec` of `decode` counts. The
  LZW tables, the line buffer and the `Info::extensions` vector are charged before they are
  allocated, with `try_reserve_exact`. A growing vector is charged its whole new size, as the
  old and the new block are live together during a realloc. The extension list is the exception
  to `AllocLimit`: if it does not fit, the remaining blocks are not recorded and
  `Info::extensions_truncated` is set; no error. So under a tight `max_alloc`, `read_info`,
  `decode` and `decode_into` can record different lists.
- A failed allocation that the limits allowed gives `OutOfMemory`.
- `max_extensions` caps only the metadata vector. Extra blocks are skipped and
  `Info::extensions_truncated` is set. Metadata never fails a decode.
- `Decoder::decode_into` returns `OutputSize` if `out.len() != info.output_size()`.

## Cancellation

The flag is read per record, per decoded line, per scanned frame and at every LZW clear code. A set flag gives
`Error::Cancelled`.

## Reuse

A `Decoder` keeps its tables and buffers. A later call makes no heap allocation if its first frame is no wider, and it has no more extension blocks, than an earlier decode on this `Decoder` (`tests/alloc.rs`).
