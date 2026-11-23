//! Parity oracle (docs/design.md, "Parity output format").
//!
//! `oracle [--threads N] <in-dir> <out-dir>` decodes every `*.gif` file under in-dir (recursive,
//! sorted) and writes `<stem>.rgba` and `<stem>.info`, or `<stem>.err` with the error token.
//! `<stem>` is the path relative to in-dir with `/` replaced by `__`. Like `giforacle`, it does
//! not follow symlinks and takes every regular file whose name ends in `.gif`. `--threads` sets
//! `max_threads` (default: `Limits::default()`); it has no effect on the output (the crate is
//! serial).

use gif::{Info, Limits};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

fn walk(root: &Path, dir: &Path, out: &mut Vec<(OsString, PathBuf)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for p in entries {
        // The same rule as giforacle (nftw, FTW_PHYS): no symlinks, byte suffix `.gif`.
        let ft = std::fs::symlink_metadata(&p).unwrap().file_type();
        if ft.is_dir() {
            walk(root, &p, out);
        } else if ft.is_file() && p.as_os_str().as_bytes().ends_with(b".gif") {
            let rel = p.strip_prefix(root).unwrap();
            let parts: Vec<&[u8]> = rel.components().map(|c| c.as_os_str().as_bytes()).collect();
            out.push((OsString::from_vec(parts.join(&b"__"[..])), p));
        }
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().fold(String::new(), |mut s, x| {
        write!(s, "{x:02x}").unwrap();
        s
    })
}

fn info_text(i: &Info, data: &[u8]) -> String {
    let f = &i.frame;
    let mut s = String::new();
    writeln!(s, "canvas {} {}", i.width, i.height).unwrap();
    writeln!(
        s,
        "screen {} {} {} {} {}",
        i.screen_width, i.screen_height, i.screen_flags, i.background_index, i.aspect
    )
    .unwrap();
    writeln!(
        s,
        "frame {} {} {} {} {} {}",
        f.left, f.top, f.width, f.height, f.flags, f.min_code_size
    )
    .unwrap();
    match f.control {
        Some(c) => {
            let t = c.transparent.map_or("-".to_string(), |t| t.to_string());
            writeln!(
                s,
                "control {} {} {} {}",
                c.disposal,
                u8::from(c.user_input),
                c.delay,
                t
            )
            .unwrap();
        }
        None => s.push_str("control -\n"),
    }
    writeln!(s, "frames {} {}", i.frame_count, u8::from(i.complete)).unwrap();
    for e in &i.extensions {
        let raw = e.bytes(data);
        writeln!(
            s,
            "ext {} {:02x} {} {}",
            e.frame,
            e.label,
            raw.len(),
            hex(raw)
        )
        .unwrap();
    }
    s
}

fn usage() -> ! {
    eprintln!("usage: oracle [--threads N] <in-dir> <out-dir>");
    std::process::exit(2)
}

fn main() {
    let mut threads = Limits::default().max_threads;
    let mut pos = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        if a == "--threads" {
            threads = it
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| usage());
        } else {
            pos.push(a);
        }
    }
    let [indir, outdir] = &pos[..] else { usage() };
    let (indir, outdir) = (Path::new(indir), Path::new(outdir));
    std::fs::create_dir_all(outdir).unwrap();
    let mut files = Vec::new();
    walk(indir, indir, &mut files);
    let limits = Limits {
        // The same canvas limit as giforacle (MAX_CANVAS_BYTES: more than 2^30 pixels is
        // `limit`). The extra 1 MiB covers the small buffers (LZW tables, line, extensions).
        max_alloc: (4usize << 30) + (1 << 20),
        max_threads: threads,
        max_width: 1 << 17,
        max_height: 1 << 17,
        max_pixels: 1 << 30,
        ..Limits::default()
    };
    let cancel = AtomicBool::new(false);
    for (name, path) in files {
        let data = std::fs::read(&path).unwrap();
        let out = |ext: &str| {
            let mut n = name.clone();
            n.push(format!(".{ext}"));
            outdir.join(n)
        };
        match gif::decode(&data, &limits, &cancel) {
            Ok((info, rgba)) => {
                std::fs::write(out("rgba"), rgba).unwrap();
                std::fs::write(out("info"), info_text(&info, &data)).unwrap();
            }
            Err(e) => {
                std::fs::write(out("err"), format!("{}\n", e.token())).unwrap();
            }
        }
    }
}
