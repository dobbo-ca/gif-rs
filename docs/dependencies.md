# Dependencies

The crate has none. `[dependencies]` and `[build-dependencies]` are empty, and there is no C code.

Other crates and tools in the repo, none of them linked into the crate:

| Where | Dependency | Why |
|---|---|---|
| `tests/fuzz/` | `libfuzzer-sys` | the fuzz targets; the only place the port convention allows it |
| `tests/parity/c/` | giflib 5.2.1 (C, CI only) | oracle, built in the CI image from Debian `libgif-dev`; see [parity.md](parity.md) |
| `tests/parity/imagers/` | `gif` 0.13.1 from image-rs (CI only) | second oracle, information only; its own Cargo crate with its own `Cargo.lock` |
