# Threading

gif-rs is serial by design. It never spawns a thread.

## Why

- LZW is serial. Each code depends on the table that the codes before it built, and the GIF
  stream has no restart points.
- The first frame is the only frame decoded. The scan of later frames reads only length bytes
  and costs little.
- The palette step is one 4-byte copy per pixel. A thread would cost more than it saves.
- A frame that is later cut into tiles would still need the whole LZW run first.

## `max_threads`

`Limits::max_threads` exists for the shared convention of the Rust ports. Its default is
`std::thread::available_parallelism()`. The crate reads it and ignores it: `max_threads = 1` and
any other value give the same output, the same errors and the same memory use.

- No thread pool, no global state.
- The calling thread is the only thread of a call.
