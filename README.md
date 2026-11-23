# gif-rs

A GIF decoder in Rust. It decodes the first frame only (v1), composited on the canvas as RGBA8,
and reports the frame count and every extension block as byte ranges into your input. The LZW
decoder and the parser follow giflib 5.2.1, so output and errors match it. Pure Rust, no C
dependency, no `unsafe`. Apache-2.0; see `NOTICE`.

Dependency: `gif = { git = "https://github.com/dobbo-ca/gif-rs", rev = "<sha>" }`

## Quick start

```rust
use std::sync::atomic::AtomicBool;
use gif::Limits;

let data = std::fs::read("anim.gif")?;
// One thread and at most 64 MiB for this call.
let limits = Limits {
    max_threads: 1,
    max_alloc: 64 << 20,
    ..Limits::default()
};
let cancel = AtomicBool::new(false);
let (info, rgba) = gif::decode(&data, &limits, &cancel)?;
println!("{}x{}, {} frames, {} bytes", info.width, info.height, info.frame_count, rgba.len());
```

To reuse buffers across images, keep a `gif::Decoder`, call `read_info`, then `decode_into` with
your own slice (`info.output_size()` bytes). A second call with an input of the same or smaller
size makes no heap allocation.

## Resource limits

There is no unlimited entry point. Every call takes a `Limits`.

| Field | Default | Unit | Over it |
|---|---|---|---|
| `max_alloc` | 1 GiB | bytes, peak of one call (output included) | `Error::AllocLimit` (`Error::OutOfMemory` if the allocator fails) |
| `max_threads` | `std::thread::available_parallelism()` | threads | no error; never spawns a thread (see [threading](docs/threading.md)) |
| `max_width` | 131 072 (`1 << 17`) | pixels, canvas | `Error::WidthLimit` |
| `max_height` | 131 072 (`1 << 17`) | pixels, canvas | `Error::HeightLimit` |
| `max_pixels` | 268 435 456 (`1 << 28`) | canvas width x height | `Error::PixelLimit` |
| `max_extensions` | 4096 | extension blocks recorded | none: the rest are not recorded, `Info::extensions_truncated` is set |

- `max_alloc` is the enforced peak working memory of one call. Allocations are charged before
  they are made.
- Cancellation: every call takes `&AtomicBool`. It is checked per record, per decoded line and
  per scanned frame. When set, the call returns `Error::Cancelled`.

## Docs

- [docs/design.md](docs/design.md): the build spec
- [docs/limits.md](docs/limits.md): limits, charging, errors
- [docs/threading.md](docs/threading.md): why the crate is serial
- [docs/safety.md](docs/safety.md): no `unsafe`, hostile input
- [docs/differences.md](docs/differences.md): where giflib and browsers differ
- [docs/dependencies.md](docs/dependencies.md): none, and what CI uses
- [docs/parity.md](docs/parity.md): oracles, corpus, gate
- [docs/fuzzing.md](docs/fuzzing.md): fuzz targets
- [docs/benchmarks.md](docs/benchmarks.md): benchmarks
