//! `imagers-oracle <in-dir> <out-dir>`: the output format of `giforacle` (docs/design.md,
//! "Parity output format"), made with the image-rs `gif` crate. Information only: run.sh compares
//! `.rgba` and the ok/err split with giflib, nothing else. The crate does not give the screen
//! flags, the aspect byte, the frame flags, the min code size or the raw extensions: those
//! fields are `-` (the extension lines are left out).

use gif::{ColorOutput, DecodeOptions};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
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

/// The colour of `idx` in an RGB table: opaque black past the end.
fn color(table: &[u8], idx: u8) -> [u8; 4] {
    match table.get(idx as usize * 3..idx as usize * 3 + 3) {
        Some(c) => [c[0], c[1], c[2], 255],
        None => [0, 0, 0, 255],
    }
}

/// Returns the .rgba bytes and the .info text, or the reason (always `error`).
fn decode(data: &[u8]) -> Result<(Vec<u8>, String), &'static str> {
    let mut opts = DecodeOptions::new();
    opts.set_color_output(ColorOutput::Indexed);
    let mut dec = opts.read_info(data).map_err(|_| "error")?;
    let (sw, sh) = (u32::from(dec.width()), u32::from(dec.height()));
    let global = dec.global_palette().map(<[u8]>::to_vec);
    let bg = dec.bg_color();
    let frame = dec
        .read_next_frame()
        .map_err(|_| "error")?
        .ok_or("no-image")?;
    let (l, t) = (u32::from(frame.left), u32::from(frame.top));
    let (fw, fh) = (u32::from(frame.width), u32::from(frame.height));
    let table = frame.palette.clone().or(global).ok_or("no-color-table")?;
    let (cw, ch) = (sw.max(l + fw), sh.max(t + fh));
    if u64::from(cw) * u64::from(ch) * 4 > 1 << 32 {
        return Err("limit");
    }
    let mut rgba = vec![0u8; cw as usize * ch as usize * 4];
    // The crate returns the rows in display order, interlaced frames included.
    for y in 0..fh as usize {
        for x in 0..fw as usize {
            let Some(&i) = frame.buffer.get(y * fw as usize + x) else {
                return Err("error");
            };
            if frame.transparent == Some(i) {
                continue;
            }
            let at = ((t as usize + y) * cw as usize + l as usize + x) * 4;
            rgba[at..at + 4].copy_from_slice(&color(&table, i));
        }
    }
    let mut info = String::new();
    let _ = writeln!(info, "canvas {cw} {ch}");
    let bg = bg.map_or("-".to_string(), |b| b.to_string());
    let _ = writeln!(info, "screen {sw} {sh} - {bg} -");
    let _ = writeln!(info, "frame {l} {t} {fw} {fh} - -");
    let transparent = frame.transparent.map_or("-".to_string(), |v| v.to_string());
    let _ = writeln!(
        info,
        "control {} {} {} {transparent}",
        frame.dispose as u8,
        u8::from(frame.needs_user_input),
        frame.delay
    );
    let mut frames = 1u32;
    let complete = loop {
        match dec.next_frame_info() {
            Ok(Some(_)) => frames += 1,
            Ok(None) => break 1,
            Err(_) => break 0,
        }
    };
    let _ = writeln!(info, "frames {frames} {complete}");
    Ok((rgba, info))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [inp, out] = &args[..] else {
        eprintln!("usage: imagers-oracle <in-dir> <out-dir>");
        std::process::exit(2);
    };
    let (inp, out) = (Path::new(inp), Path::new(out));
    let mut files = Vec::new();
    walk(inp, inp, &mut files);
    let mut bad = false;
    for (name, path) in files {
        let Ok(data) = std::fs::read(&path) else {
            eprintln!("cannot read {}", path.display());
            bad = true;
            continue;
        };
        let res = match decode(&data) {
            Ok((rgba, info)) => std::fs::write(out.join(format!("{name}.rgba")), rgba)
                .and_then(|()| std::fs::write(out.join(format!("{name}.info")), info)),
            Err(tok) => std::fs::write(out.join(format!("{name}.err")), format!("{tok}\n")),
        };
        if res.is_err() {
            bad = true;
        }
    }
    std::process::exit(i32::from(bad));
}
