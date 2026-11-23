//! Shared by the fuzz targets: small limits, and the options read from the end of the input.
#![allow(dead_code)]

use gif::{Extension, Info, Limits};
use std::sync::atomic::AtomicBool;

pub static NO: AtomicBool = AtomicBool::new(false);

/// Small, so a hostile image cannot reach the fuzzer's malloc limit. `tight` picks a profile
/// that reaches `AllocLimit`, the extension-list `max_alloc` branch and `WidthLimit` and
/// `HeightLimit`: `max_alloc` 20 KiB, `max_width` and `max_height` `1 << 10`.
pub fn limits(max_extensions: usize, tight: bool) -> Limits {
    Limits {
        max_alloc: if tight { 20 << 10 } else { 64 << 20 },
        max_threads: 1,
        max_pixels: 1 << 22,
        max_extensions,
        max_width: if tight {
            1 << 10
        } else {
            Limits::default().max_width
        },
        max_height: if tight {
            1 << 10
        } else {
            Limits::default().max_height
        },
        ..Limits::default()
    }
}

/// Last byte: bit 0 selects `max_extensions` (0: 4, 1: 4096), bit 1 the tight limits profile.
/// Bits 2..8 are free for the target.
/// The rest is the GIF stream, so a raw GIF plus one byte is a valid seed.
pub fn parse(src: &[u8]) -> Option<(Limits, u8, &[u8])> {
    let (&b, data) = src.split_last()?;
    let max_ext = if b & 1 == 0 { 4 } else { 4096 };
    Some((limits(max_ext, b & 2 != 0), b, data))
}

fn inside(sub: &[u8], data: &[u8]) -> bool {
    let (s, d) = (sub.as_ptr() as usize, data.as_ptr() as usize);
    s >= d && s + sub.len() <= d + data.len()
}

/// Every range is inside `data`, and every `Extension` helper stays inside it too.
pub fn check_info(info: &Info, data: &[u8]) {
    for r in info
        .global_table
        .iter()
        .chain(info.frame.local_table.iter())
    {
        assert!(r.start <= r.end && r.end <= data.len());
    }
    for e in &info.extensions {
        check_ext(e, data);
    }
}

fn check_ext(e: &Extension, data: &[u8]) {
    assert!(e.raw.start <= e.raw.end && e.raw.end <= data.len());
    assert_eq!(e.bytes(data).len(), e.raw.end - e.raw.start);
    for s in e.sub_blocks(data) {
        assert!(inside(s, e.bytes(data)));
    }
    if let Some(id) = e.application_id(data) {
        assert_eq!(e.label, 0xFF);
        assert_eq!(id.len(), 11);
        assert!(inside(id, data));
    }
    if let Some(x) = e.xmp(data) {
        assert!(inside(x, data));
    }
}
