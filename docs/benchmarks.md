# Benchmarks

Information only. There is no speed bar and no gate. The owner decided this for gif-rs: GIF
decode time is dominated by serial LZW, and the first frame of a typical file is small.

`examples/bench.rs <dir> <reps>` decodes each file with one `Decoder` and `decode_into` and
prints TSV (`name`, `bytes`, `ns`). `tests/parity/c/giforacle.c` has a `bench <dir> <reps>` mode
with the same TSV for giflib. `bench.yml` prints both per file-size class in the job summary.

Results (bench.yml, main 4a9ebe5, run 45266266164; 37 files that both tools decode, median of 5 runs):

| input size class | files | giflib ms | gif-rs ms | gif-rs / giflib |
|---|---|---|---|---|
| <= 1 KiB | 17 | 0.048 | 0.029 | 0.60 |
| <= 16 KiB | 12 | 1.197 | 0.977 | 0.82 |
| <= 256 KiB | 6 | 2.575 | 2.118 | 0.82 |
| > 256 KiB | 2 | 6.269 | 5.094 | 0.81 |
| all | 37 | 10.089 | 8.218 | 0.81 |

The giflib number includes `DGifOpen` and the output `malloc` per call. gif-rs reuses one
`Decoder` and output buffer.
