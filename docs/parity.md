# Parity

All of this runs in CI only (the `parity` job). Nothing runs on the owner's Mac.

## Oracles

1. `examples/oracle.rs` (gif-rs): `oracle [--threads N] <in-dir> <out-dir>`. Mutations come from
   `giforacle -gen` (`tests/parity/run.sh gen`).
2. `tests/parity/c/giforacle.c`: giflib 5.2.1 (Debian bookworm `libgif-dev` 5.2.1-2.5). It is
   built in CI only and never linked into the crate. It uses `DGifOpen` with a memory input
   function and the low-level calls, mirrors the record loop in [design.md](design.md), decodes
   the first frame with `DGifGetLine` in the call pattern of `DGifSlurp`, composites by the
   policy of the design, then scans the rest. This is the gate.
3. `tests/parity/imagers/`: a separate Cargo crate that decodes with image-rs `gif` 0.13.1. It
   is information only: the job prints how often it agrees, and a difference never fails CI.

## Output format

For every `*.gif` under the input dir (recursive, sorted), `stem` is the relative path with `/`
replaced by `__`. On success: `<stem>.rgba` (the canvas bytes) and `<stem>.info` (text: canvas,
screen, frame, control, frames, one `ext` line per extension). On error: `<stem>.err` with one
token: `read-failed not-gif no-scrn-dscr wrong-record data-too-big image-defect eof-too-soon
no-image no-color-table limit oom`. The exact format is in design.md.

## Corpus

| Set | Files | Licence | Source |
|---|---|---|---|
| giflib-5.2.1 (`pic`, `tests/wedge.gif`) | see `corpus.toml` | MIT | the giflib 5.2.1 tarball |
| image-gif-0.13.1 | see `corpus.toml` | MIT OR Apache-2.0 | image-rs/image-gif, files made for the repo |
| wuffs-a87f0bb | see `corpus.toml` | Apache-2.0 | google/wuffs at a pinned commit |
| libnsgif-release-1.0.0 | see `corpus.toml` | MIT | libnsgif test data |
| Mutations | 13080 | n/a | made by `giforacle -gen`; deterministic |
| Fuzz-grown | 0 at first | n/a | `tests/data/fuzz-grown/` |

The four sets give 329 files in all (CI run 45264352241). The corpus is not in git. Mutations per input: truncations, single bit flips, and edits of
length fields (sub-block lengths, table sizes, code size, descriptor sizes).

## Gate

Exact. For every input, gif-rs and the giflib oracle must give the same `.rgba` bytes and the
same `.info` text, or the same error token. Any difference fails the job. The only allowed
differences are the documented policy choices in [differences.md](differences.md), and the
oracle follows them, so they do not show up as diffs.

## Results

Latest green `parity` job on main: 13409 inputs, no difference against giflib 5.2.1. The ok/err split and the image-rs count are in the job summary (`summary.md`) of each run.

| Item | Count |
|---|---|
| inputs | 13409 |
| equal output (both decode) | 591 |
| equal error token | 12818 |
| differences | 0 |
| image-rs agrees (information) | 13284 of 13409; the 125 others are all mutations (87 giflib errors and image-rs decodes, 35 the reverse, 3 pixels differ); all 329 corpus files agree |

## Second gate and arm64

- `run.sh full` also compares gif-rs with `--threads 1` against gif-rs with the default threads
  (`d-threads`). Any difference fails the job.
- `parity-arm64` runs the aarch64 oracle under qemu on every 4th file of the sorted corpus and
  compares it with the x86_64 output of the same files.
