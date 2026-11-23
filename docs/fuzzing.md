# Fuzzing

`tests/fuzz/` is a cargo-fuzz crate with its own `Cargo.lock`. Run with `--fuzz-dir tests/fuzz`.

| Target | What it checks |
|---|---|
| `decode` | `gif::decode` with small `Limits`; no panic; output length equals `Info::output_size()`; every `Extension` helper on every extension (raw range inside the input, `xmp()` a subslice of it) |
| `decoder_reuse` | one `Decoder` decodes a truncated prefix A of the input, A again, then the whole input; each result equals a fresh decode, the two A results are equal, and `read_info` on the warm decoder equals a fresh `read_info` |
| `read_info` | `read_info` (free fn and `Decoder`) agree; on a successful decode its `Info` is equal to the decode's (extension fields excepted when either list is truncated) |

Every target reads its options from the **last** byte of the input. Bit 0 picks `max_extensions`
(0: 4, 1: 4096). Bit 1 picks the tight profile (`max_alloc` 20 KiB, `max_width` and `max_height` `1 << 10`) so `AllocLimit`, the extension-list budget and the size limits are reached; `decoder_reuse` always uses the normal one. `decoder_reuse` takes bits 2..8 as the cut point (0 means half). The normal limits are
`max_alloc` 64 MiB, `max_pixels` `1 << 22`, `max_threads` 1. A raw GIF plus one byte is a valid
input; CI appends a 0x00 byte to each seed.

All targets run with `-rss_limit_mb=2048 -malloc_limit_mb=512`, so an allocation outside the
limits is a crash.

Seeds are the parity corpus. They are fetched at build time, not stored in git. Inputs that a
run grows go to `tests/fuzz/corpus/<target>`; the workflow keeps them in an Actions cache and
uploads them as the `corpus-<target>` artifact.

## Schedule and dispatch

`.github/workflows/fuzz.yml` runs weekly (Friday 12:23 UTC) and on `workflow_dispatch`. It never
runs on push or pull request. To run it by hand:

```
gh workflow run fuzz.yml --ref <branch> -f max_total_time=600
gh run watch
```

`max_total_time` is seconds per target (default 600). Crash inputs are uploaded as artifacts.

Fuzz hours so far: 0.75 (run 45271235369 on main e661753: 3 targets x 900 s; decode 431588 runs, read_info 317701, decoder_reuse 59730; 0 crashes).
