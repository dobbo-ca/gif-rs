# Safety

## No `unsafe`

The crate root has `#![forbid(unsafe_code)]`. There is no `unsafe` block anywhere in `src/`.

## No SIMD

v1 has no SIMD. No kernel pays:

- LZW decoding is serial and branch-heavy. A vector unit does not help.
- The palette lookup is one 4-byte copy per pixel.

Without SIMD there is no scalar and SIMD split, no runtime CPU detection and no
`GIF_SIMD` override. SIMD is not planned for v2 unless a benchmark shows a gain.

## Hostile input

- No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or unchecked indexing in non-test
  code (denied by clippy). Slices are read with `get`, iterators and `split_at_checked`.
- Every size from the input uses `checked_*` arithmetic.
- Nothing is allocated from an input size before it passes `Limits`. Allocation uses
  `try_reserve_exact`, never `vec![0; n]` on an unchecked `n`.
- The LZW tables have a fixed size (4096 entries). A prefix chain longer than the stack, a prefix
  over 4095 and a code size over 12 bits are `ImageDefect`.
- Each loop makes progress per input byte, and the cancel flag is checked per record, per line
  and per scanned frame.
- Fuzzing runs with `-rss_limit_mb=2048 -malloc_limit_mb=512` (see [fuzzing.md](fuzzing.md)).
