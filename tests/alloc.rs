//! Allocation and thread behaviour under a counting global allocator. One test fn on purpose:
//! the counters are process-wide, so no other test may run in this binary.

mod common;

use common::*;
use gif::{Decoder, Error, Limits};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};

struct Counting;
static CUR: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);

fn grow(n: usize) {
    let c = CUR.fetch_add(n, SeqCst) + n;
    PEAK.fetch_max(c, SeqCst);
}

// SAFETY: forwards every call to `System` unchanged and only adds bookkeeping.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        // SAFETY: same contract as the caller's.
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            CALLS.fetch_add(1, SeqCst);
            grow(l.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        // SAFETY: same contract as the caller's.
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            CALLS.fetch_add(1, SeqCst);
            grow(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        CUR.fetch_sub(l.size(), SeqCst);
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        // SAFETY: same contract as the caller's.
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            CALLS.fetch_add(1, SeqCst);
            CUR.fetch_sub(l.size(), SeqCst);
            grow(new);
        }
        q
    }
}

#[global_allocator]
static A: Counting = Counting;

/// The threads of this process, from /proc (Linux only).
#[cfg(target_os = "linux")]
fn threads() -> Option<usize> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    s.lines()
        .find_map(|l| l.strip_prefix("Threads:")?.trim().parse().ok())
}

#[cfg(not(target_os = "linux"))]
fn threads() -> Option<usize> {
    None
}

/// Runs `f` and gives its result and the most bytes it held at once above the start.
fn peak_of<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = CUR.load(SeqCst);
    PEAK.store(base, SeqCst);
    let r = f();
    (r, PEAK.load(SeqCst).saturating_sub(base))
}

/// A 200 x 150 image of 256 colours, 20 comments before it and 2 more frames after it.
fn big_file() -> Vec<u8> {
    let t = table(256);
    let mut g = Gif::new(200, 150, &t);
    for _ in 0..20 {
        g = g.ext(0xFE, &[b"comment"]);
    }
    g.image(&Img::new(200, 150).min(8), &pixels(1, 200 * 150, 256))
        .image(&Img::new(10, 10), &pixels(2, 100, 4))
        .image(&Img::new(10, 10), &pixels(3, 100, 4))
        .trailer()
}

/// An interlaced image with a local table.
fn interlaced_file() -> Vec<u8> {
    Gif::new(37, 41, &P4)
        .gce(1, 5, 1)
        .image(
            &Img::new(37, 41).interlaced().local(&table(16)).min(4),
            &pixels(4, 37 * 41, 16),
        )
        .trailer()
}

fn small_file() -> Vec<u8> {
    Gif::new(3, 3, &P4)
        .image(&Img::new(3, 3), &pixels(5, 9, 4))
        .trailer()
}

/// The second `decode_into` and `read_info` on the same decoder allocate nothing.
fn zero_alloc(dec: &mut Decoder, name: &str, data: &[u8]) {
    let (cancel, l) = (AtomicBool::new(false), Limits::default());
    let size = gif::read_info(data, &l, &cancel).unwrap().output_size();
    let mut out = vec![0u8; size];
    let first = dec
        .decode_into(data, &mut out, &l, &cancel)
        .unwrap()
        .clone();
    let first_out = out.clone();

    let before = CALLS.load(SeqCst);
    let r = dec.decode_into(data, &mut out, &l, &cancel).is_ok();
    let after = CALLS.load(SeqCst);
    assert!(r, "{name}");
    assert_eq!(after - before, 0, "{name}: second decode_into allocated");
    assert_eq!(first_out, out, "{name}");

    let before = CALLS.load(SeqCst);
    let r = dec.read_info(data, &l, &cancel).is_ok();
    let after = CALLS.load(SeqCst);
    assert!(r, "{name}");
    assert_eq!(after - before, 0, "{name}: second read_info allocated");
    assert_eq!(dec.read_info(data, &l, &cancel).unwrap(), &first, "{name}");
}

const VALUES: [usize; 17] = [
    0,
    1,
    64,
    1024,
    4096,
    16 << 10,
    17 << 10,
    20 << 10,
    64 << 10,
    100 << 10,
    128 << 10,
    130 << 10,
    137 << 10,
    140 << 10,
    160 << 10,
    1 << 20,
    1 << 30,
];

/// Peak bytes held during one `decode` stay within `max_alloc`; a failure is `AllocLimit`.
fn peak_decode(label: &str, data: &[u8], threads: usize, values: &[usize]) {
    let cancel = AtomicBool::new(false);
    for &max_alloc in values {
        let l = Limits {
            max_alloc,
            max_threads: threads,
            ..Limits::default()
        };
        let (r, peak) = peak_of(|| gif::decode(data, &l, &cancel));
        assert!(
            peak <= max_alloc,
            "{label} threads {threads} max_alloc {max_alloc}: peak {peak}"
        );
        let ok = r.is_ok();
        if let Err(e) = r {
            assert_eq!(e, Error::AllocLimit, "{label} max_alloc {max_alloc}");
        }
        if max_alloc == 1 << 30 {
            assert!(ok, "{label} must fit in 1 GiB");
        }
        if max_alloc == 0 {
            assert!(!ok, "{label} cannot decode with max_alloc 0");
        }
    }
}

/// A limit error comes before any allocation: it holds almost nothing.
fn limit_first(label: &str, data: &[u8], l: &Limits, want: Error) {
    let cancel = AtomicBool::new(false);
    let (r, peak) = peak_of(|| gif::decode(data, l, &cancel).map(|_| ()));
    assert_eq!(r, Err(want.clone()), "{label}");
    assert!(peak <= 4096, "{label}: peak {peak}");
    let (r, peak) = peak_of(|| gif::read_info(data, l, &cancel).map(|_| ()));
    assert!(peak <= 4096, "{label}: read_info peak {peak}");
    // `read_info` also checks the canvas, except for a memory limit: it makes no output.
    if want != Error::AllocLimit {
        assert_eq!(r, Err(want), "{label}: read_info");
    }
}

#[test]
fn allocation() {
    let big = big_file();
    let inter = interlaced_file();
    let small = small_file();

    // One decoder, warmed by the biggest input, then smaller ones.
    let mut dec = Decoder::new();
    zero_alloc(&mut dec, "big", &big);
    zero_alloc(&mut dec, "interlaced", &inter);
    zero_alloc(&mut dec, "small", &small);
    // A fresh decoder for each shape.
    zero_alloc(&mut Decoder::new(), "interlaced fresh", &inter);
    zero_alloc(&mut Decoder::new(), "small fresh", &small);

    // The peak stays within max_alloc, for one thread and for the default.
    let many = std::thread::available_parallelism().map_or(1, |n| n.get());
    for (name, data) in [("big", &big), ("interlaced", &inter), ("small", &small)] {
        peak_decode(name, data, 1, &VALUES);
        peak_decode(name, data, many, &VALUES);
    }

    // Limit errors are found before the output is allocated.
    let huge = Gif::new(65535, 65535, &P2)
        .image(&Img::new(1, 1), &[0])
        .trailer();
    let l = Limits::default();
    limit_first("pixels", &huge, &l, Error::PixelLimit);
    let l = Limits {
        max_pixels: 1 << 40,
        ..Limits::default()
    };
    limit_first("alloc", &huge, &l, Error::AllocLimit);
    let l = Limits {
        max_width: 100,
        ..Limits::default()
    };
    limit_first("width", &big, &l, Error::WidthLimit);
    let l = Limits {
        max_height: 100,
        ..Limits::default()
    };
    limit_first("height", &big, &l, Error::HeightLimit);
    let l = Limits {
        max_alloc: 1 << 16,
        ..Limits::default()
    };
    limit_first("small alloc", &big, &l, Error::AllocLimit);

    // No thread is spawned, with max_threads 1 and with the default.
    if let Some(before) = threads() {
        let cancel = AtomicBool::new(false);
        for max_threads in [1, many] {
            let l = Limits {
                max_threads,
                ..Limits::default()
            };
            assert!(gif::decode(&big, &l, &cancel).is_ok());
            assert!(gif::read_info(&big, &l, &cancel).is_ok());
            let mut dec = Decoder::new();
            let mut out = vec![0u8; 9 * 4];
            assert!(dec.decode_into(&small, &mut out, &l, &cancel).is_ok());
            assert_eq!(threads(), Some(before), "max_threads {max_threads}");
        }
    }
}
