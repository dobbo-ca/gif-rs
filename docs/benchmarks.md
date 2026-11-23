# Benchmarks

Information only. There is no speed bar and no gate. The owner decided this for gif-rs: GIF
decode time is dominated by serial LZW, and the first frame of a typical file is small.

`examples/bench.rs <dir> <reps>` decodes each file with one `Decoder` and `decode_into` and
prints TSV (`name`, `bytes`, `ns`). `tests/parity/c/giforacle.c` has a `bench <dir> <reps>` mode
with the same TSV for giflib. `bench.yml` prints both per file-size class in the job summary.

Results: TBD (`bench.yml` run on main).

| size class (output) | files | giflib | gif-rs | gif-rs / giflib |
|---|---|---|---|---|
| <= 4 KiB | TBD | TBD | TBD | TBD |
| <= 64 KiB | TBD | TBD | TBD | TBD |
| <= 1 MiB | TBD | TBD | TBD | TBD |
| <= 16 MiB | TBD | TBD | TBD | TBD |
| all | TBD | TBD | TBD | TBD |

The giflib number includes `DGifOpen` and the output `malloc` per call. gif-rs reuses one
`Decoder` and output buffer.
