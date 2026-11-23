//! A GIF builder and an LZW encoder for the tests, so no test needs a binary fixture file.
//! Test code: it may panic on a bad argument.
#![allow(dead_code)]

use std::collections::HashMap;

pub const P2: [[u8; 3]; 2] = [[255, 0, 0], [0, 255, 0]];
pub const P4: [[u8; 3]; 4] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255]];

/// xorshift64, so the tests need no dependency.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() >> 8) as usize % n.max(1)
    }
}

/// `n` pseudo-random indices below `colors`.
pub fn pixels(seed: u64, n: usize, colors: usize) -> Vec<u8> {
    let mut r = Rng::new(seed);
    (0..n).map(|_| r.below(colors) as u8).collect()
}

/// A table of `n` (a power of two) distinct colours.
pub fn table(n: usize) -> Vec<[u8; 3]> {
    (0..n)
        .map(|i| [i as u8, (i * 7) as u8 ^ 0x55, (255 - i) as u8])
        .collect()
}

/// The RGBA of palette indices. An index past the table is opaque black.
pub fn rgba(table: &[[u8; 3]], idx: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    for &i in idx {
        match table.get(usize::from(i)) {
            Some(c) => v.extend_from_slice(&[c[0], c[1], c[2], 255]),
            None => v.extend_from_slice(&[0, 0, 0, 255]),
        }
    }
    v
}

/// A `cw` x `ch` canvas of transparent black with the RGBA `frame` (w x h) at (left, top).
pub fn place(cw: usize, ch: usize, left: usize, top: usize, w: usize, frame: &[u8]) -> Vec<u8> {
    let mut v = vec![0u8; cw * ch * 4];
    for (y, row) in frame.chunks(w * 4).enumerate() {
        let at = ((top + y) * cw + left) * 4;
        v[at..at + row.len()].copy_from_slice(row);
    }
    v
}

/// Rows in the order an interlaced image stores them.
pub fn interlace_order(h: usize) -> Vec<usize> {
    let mut v = Vec::new();
    for (start, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
        let mut y = start;
        while y < h {
            v.push(y);
            y += step;
        }
    }
    v
}

/// The LZW stream encoder. It keeps the decoder's code width rule: the width grows when the
/// running code passes the maximum, and the running code stops at 4097.
struct Enc {
    out: Vec<u8>,
    acc: u32,
    nacc: u32,
    min: u32,
    bits: u32,
    running: u32,
    max1: u32,
}

impl Enc {
    fn put(&mut self, code: u32) {
        self.acc |= code << self.nacc;
        self.nacc += self.bits;
        while self.nacc >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.nacc -= 8;
        }
        if self.running < 4097 {
            self.running += 1;
            if self.running > self.max1 && self.bits < 12 {
                self.max1 <<= 1;
                self.bits += 1;
            }
        }
    }

    fn reset(&mut self) {
        self.running = (1 << self.min) + 2;
        self.bits = self.min + 1;
        self.max1 = 1 << self.bits;
    }

    fn put_clear(&mut self) {
        self.put(1 << self.min);
        self.reset();
    }
}

/// LZW codes for `pix` with minimum code size `min` (1 to 8): a clear code, the data, the end
/// code. A clear code follows the data code number n for each n in `clears` (the table then
/// restarts). The table is never cleared by itself: once full, the codes go on at 12 bits.
pub fn lzw_encode(pix: &[u8], min: u8, clears: &[usize]) -> Vec<u8> {
    let min = u32::from(min);
    let mut e = Enc {
        out: Vec::new(),
        acc: 0,
        nacc: 0,
        min,
        bits: min + 1,
        running: (1 << min) + 2,
        max1: 1 << (min + 1),
    };
    let first_entry = (1u32 << min) + 2;
    let mut next = first_entry;
    let mut dict: HashMap<(u32, u8), u32> = HashMap::new();
    e.put_clear();
    let mut emitted = 0usize;
    let mut cur: Option<u32> = None;
    for &p in pix {
        let Some(w) = cur else {
            cur = Some(u32::from(p));
            continue;
        };
        if let Some(&c) = dict.get(&(w, p)) {
            cur = Some(c);
            continue;
        }
        e.put(w);
        emitted += 1;
        if next < 4096 {
            dict.insert((w, p), next);
            next += 1;
        }
        cur = Some(u32::from(p));
        if clears.contains(&emitted) {
            e.put_clear();
            dict.clear();
            next = first_entry;
        }
    }
    if let Some(w) = cur {
        e.put(w);
    }
    e.put((1 << min) + 1);
    if e.nacc > 0 {
        e.out.push(e.acc as u8);
    }
    e.out
}

/// `data` as sub-blocks of at most `max` bytes, and the 0 terminator.
pub fn subblocks(data: &[u8], max: usize) -> Vec<u8> {
    let mut v = Vec::new();
    for c in data.chunks(max) {
        v.push(c.len() as u8);
        v.extend_from_slice(c);
    }
    v.push(0);
    v
}

/// The raw bytes of an extension block's sub-blocks, as `Extension::raw` holds them.
pub fn raw_blocks(blocks: &[&[u8]]) -> Vec<u8> {
    let mut v = Vec::new();
    for b in blocks {
        v.push(b.len() as u8);
        v.extend_from_slice(b);
    }
    v.push(0);
    v
}

/// The 257-byte XMP magic trailer.
pub fn magic() -> Vec<u8> {
    let mut m = vec![1u8];
    m.extend((0..=255u8).rev());
    m
}

fn size_code(n: usize) -> u8 {
    assert!(
        n.is_power_of_two() && (2..=256).contains(&n),
        "table of {n}"
    );
    (n.trailing_zeros() - 1) as u8
}

/// An image descriptor.
#[derive(Clone)]
pub struct Img {
    pub left: u16,
    pub top: u16,
    pub w: u16,
    pub h: u16,
    pub local: Vec<[u8; 3]>,
    pub interlaced: bool,
    pub min: u8,
}

impl Img {
    pub fn new(w: u16, h: u16) -> Img {
        Img {
            left: 0,
            top: 0,
            w,
            h,
            local: Vec::new(),
            interlaced: false,
            min: 2,
        }
    }
    pub fn at(mut self, left: u16, top: u16) -> Img {
        self.left = left;
        self.top = top;
        self
    }
    pub fn local(mut self, t: &[[u8; 3]]) -> Img {
        self.local = t.to_vec();
        self
    }
    pub fn interlaced(mut self) -> Img {
        self.interlaced = true;
        self
    }
    pub fn min(mut self, m: u8) -> Img {
        self.min = m;
        self
    }
}

/// A GIF file under construction.
pub struct Gif(pub Vec<u8>);

impl Gif {
    /// The header and the logical screen descriptor, with `global` as the global table (empty:
    /// no table).
    pub fn new(w: u16, h: u16, global: &[[u8; 3]]) -> Gif {
        let mut v = b"GIF89a".to_vec();
        v.extend_from_slice(&w.to_le_bytes());
        v.extend_from_slice(&h.to_le_bytes());
        if global.is_empty() {
            v.extend_from_slice(&[0, 0, 0]);
        } else {
            v.extend_from_slice(&[0x80 | size_code(global.len()), 0, 0]);
            for c in global {
                v.extend_from_slice(c);
            }
        }
        Gif(v)
    }

    pub fn size(&self) -> usize {
        self.0.len()
    }

    pub fn raw(mut self, b: &[u8]) -> Gif {
        self.0.extend_from_slice(b);
        self
    }

    /// An extension block with the given sub-blocks.
    pub fn ext(self, label: u8, blocks: &[&[u8]]) -> Gif {
        let mut b = vec![0x21, label];
        b.extend_from_slice(&raw_blocks(blocks));
        self.raw(&b)
    }

    /// A graphic control extension with the packed byte `packed`.
    pub fn gce(self, packed: u8, delay: u16, transparent: u8) -> Gif {
        let d = delay.to_le_bytes();
        self.ext(0xF9, &[&[packed, d[0], d[1], transparent]])
    }

    /// An image descriptor and `blocks` (sub-blocks with the 0 terminator) as the LZW data.
    pub fn image_blocks(self, img: &Img, blocks: &[u8]) -> Gif {
        let mut b = vec![0x2C];
        for v in [img.left, img.top, img.w, img.h] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        let mut flags = 0u8;
        if img.interlaced {
            flags |= 0x40;
        }
        if !img.local.is_empty() {
            flags |= 0x80 | size_code(img.local.len());
        }
        b.push(flags);
        for c in &img.local {
            b.extend_from_slice(c);
        }
        b.push(img.min);
        b.extend_from_slice(blocks);
        self.raw(&b)
    }

    /// An image whose LZW data is `lzw`, in sub-blocks of 255 bytes.
    pub fn image_lzw(self, img: &Img, lzw: &[u8]) -> Gif {
        self.image_blocks(img, &subblocks(lzw, 255))
    }

    /// An image of `pixels` (row by row, as the picture shows them).
    pub fn image(self, img: &Img, pixels: &[u8]) -> Gif {
        assert_eq!(pixels.len(), usize::from(img.w) * usize::from(img.h));
        let data = if img.interlaced && img.w > 0 {
            let w = usize::from(img.w);
            let mut v = Vec::new();
            for y in interlace_order(usize::from(img.h)) {
                v.extend_from_slice(&pixels[y * w..(y + 1) * w]);
            }
            v
        } else {
            pixels.to_vec()
        };
        self.image_lzw(img, &lzw_encode(&data, img.min, &[]))
    }

    /// The trailer, then the bytes.
    pub fn trailer(mut self) -> Vec<u8> {
        self.0.push(0x3B);
        self.0
    }

    /// The bytes without a trailer.
    pub fn bytes(self) -> Vec<u8> {
        self.0
    }
}
