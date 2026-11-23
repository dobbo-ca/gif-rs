# Parity

All of this runs in CI only (the `parity` job). Nothing runs on the owner's Mac.

## Oracles

1. `examples/oracle.rs` (gif-rs): `oracle <in-dir> <out-dir>`, and `oracle gen <in> <out>` for
   mutations.
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
| giflib 5.2.1 `pic/*.gif` | TBD | MIT | the giflib 5.2.1 tarball |
| Other public sets | TBD | per set | listed with SHA-256 in `tests/parity/corpus.toml`, fetched by CI |
| Mutations | TBD | n/a | made by `oracle gen`; deterministic |
| Fuzz-grown | 0 at first | n/a | `tests/data/fuzz-grown/` |

The corpus is not in git. Mutations per input: truncations, single bit flips, and edits of
length fields (sub-block lengths, table sizes, code size, descriptor sizes).

## Gate

Exact. For every input, gif-rs and the giflib oracle must give the same `.rgba` bytes and the
same `.info` text, or the same error token. Any difference fails the job. The only allowed
differences are the documented policy choices in [differences.md](differences.md), and the
oracle follows them, so they do not show up as diffs.

## Results

TBD (numbers come from the `parity` job on main).

| Item | Count |
|---|---|
| inputs | TBD |
| equal output | TBD |
| equal error | TBD |
| differences | TBD |
| image-rs agrees (information) | TBD |
