//! `bench <dir> [reps]`: decodes every `*.gif` under dir `reps` times (default 20) with one
//! reused `Decoder` and prints `file<TAB>median ns` for each file, then `TOTAL<TAB>sum`. The
//! crate is serial, so one thread and the default give the same time: it is printed once. Files
//! that fail to decode are skipped.

use gif::{Decoder, Limits};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(root, &p, out);
        } else if p.extension().is_some_and(|e| e == "gif") {
            let rel = p.strip_prefix(root).unwrap();
            let parts: Vec<_> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            out.push((parts.join("__"), p));
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (dir, reps) = match &args[..] {
        [d] => (d, 20),
        [d, n] => (d, n.parse().unwrap_or(20)),
        _ => {
            eprintln!("usage: bench <dir> [reps]");
            std::process::exit(2);
        }
    };
    let reps = usize::max(reps, 1);
    let dir = Path::new(dir);
    let mut files = Vec::new();
    walk(dir, dir, &mut files);
    let limits = Limits {
        max_alloc: 4usize << 30,
        max_threads: 1,
        max_width: 1 << 17,
        max_height: 1 << 17,
        max_pixels: 1 << 32,
        ..Limits::default()
    };
    let cancel = AtomicBool::new(false);
    let mut dec = Decoder::new();
    let mut total = 0u128;
    for (name, path) in files {
        let data = std::fs::read(&path).unwrap();
        let Some(size) = dec
            .read_info(&data, &limits, &cancel)
            .ok()
            .map(|i| i.output_size())
        else {
            continue;
        };
        let mut out = vec![0u8; size];
        if dec.decode_into(&data, &mut out, &limits, &cancel).is_err() {
            continue;
        }
        let mut times: Vec<u128> = (0..reps)
            .map(|_| {
                let t = Instant::now();
                std::hint::black_box(dec.decode_into(&data, &mut out, &limits, &cancel).is_ok());
                t.elapsed().as_nanos()
            })
            .collect();
        times.sort_unstable();
        let median = times[times.len() / 2];
        println!("{name}\t{median}");
        total += median;
    }
    println!("TOTAL\t{total}");
}
