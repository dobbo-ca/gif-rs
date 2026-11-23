//! Whole-file tests. Every input is built by `common`: no fixture files.

mod common;

use common::*;
use gif::{Control, Decoder, Error, Info, Limits};
use std::sync::atomic::AtomicBool;

fn no() -> AtomicBool {
    AtomicBool::new(false)
}

fn dec(d: &[u8]) -> Result<(Info, Vec<u8>), Error> {
    gif::decode(d, &Limits::default(), &no())
}

fn info(d: &[u8]) -> Result<Info, Error> {
    gif::read_info(d, &Limits::default(), &no())
}

fn ok(d: &[u8]) -> (Info, Vec<u8>) {
    dec(d).unwrap()
}

fn err(d: &[u8]) -> Error {
    dec(d).unwrap_err()
}

/// A 4-colour global table and one image of the screen's size.
fn simple(w: u16, h: u16, px: &[u8]) -> Vec<u8> {
    Gif::new(w, h, &P4).image(&Img::new(w, h), px).trailer()
}

#[test]
fn one_by_one() {
    let d = Gif::new(1, 1, &P2).image(&Img::new(1, 1), &[1]).trailer();
    let (i, px) = ok(&d);
    assert_eq!(px, [0, 255, 0, 255]);
    assert_eq!((i.width, i.height), (1, 1));
    assert_eq!((i.screen_width, i.screen_height), (1, 1));
    assert_eq!((i.frame_count, i.complete), (1, true));
    assert_eq!(i.version, *b"89a");
    assert_eq!(i.global_table, Some(13..19));
    assert_eq!(i.frame.min_code_size, 2);
    assert_eq!(i.frame.control, None);
    assert!(i.extensions.is_empty() && !i.extensions_truncated);
    assert_eq!(info(&d).unwrap(), i);
}

#[test]
fn odd_sizes() {
    let sizes = [
        (1, 1),
        (2, 1),
        (1, 2),
        (3, 5),
        (7, 3),
        (13, 17),
        (100, 1),
        (1, 100),
        (255, 257),
        (300, 2),
    ];
    for (n, &(w, h)) in sizes.iter().enumerate() {
        let px = pixels(n as u64, w * h, 4);
        let (i, out) = ok(&simple(w as u16, h as u16, &px));
        assert_eq!((i.width, i.height), (w as u32, h as u32));
        assert_eq!(out, rgba(&P4, &px), "{w}x{h}");
    }
}

#[test]
fn sub_block_sizes() {
    let px = pixels(3, 40 * 30, 4);
    let lzw = lzw_encode(&px, 2, &[]);
    for chunk in [1, 2, 3, 7, 255] {
        let d = Gif::new(40, 30, &P4)
            .image_blocks(&Img::new(40, 30), &subblocks(&lzw, chunk))
            .trailer();
        assert_eq!(ok(&d).1, rgba(&P4, &px), "chunk {chunk}");
    }
}

fn big(clears: &[usize]) -> (Vec<u8>, Vec<u8>) {
    let t = table(256);
    let px = pixels(7, 200 * 200, 256);
    let d = Gif::new(200, 200, &t)
        .image_lzw(&Img::new(200, 200).min(8), &lzw_encode(&px, 8, clears))
        .trailer();
    (d, rgba(&t, &px))
}

#[test]
fn code_size_grows_to_twelve_bits_and_the_table_stays_full() {
    // 40000 random pixels make about 25000 codes: far more than 4096 distinct strings.
    let (d, want) = big(&[]);
    assert_eq!(ok(&d).1, want);
}

#[test]
fn deferred_clear() {
    // The table is full after about 3800 codes; the codes go on at 12 bits until the clear.
    for clears in [&[6000][..], &[3000, 3001, 12000][..], &[4096][..]] {
        let (d, want) = big(clears);
        assert_eq!(ok(&d).1, want, "{clears:?}");
    }
}

#[test]
fn clear_codes_mid_stream() {
    let px = pixels(11, 30 * 30, 4);
    let d = Gif::new(30, 30, &P4)
        .image_lzw(
            &Img::new(30, 30),
            &lzw_encode(&px, 2, &[1, 2, 3, 10, 50, 200]),
        )
        .trailer();
    assert_eq!(ok(&d).1, rgba(&P4, &px));
}

#[test]
fn min_code_sizes_1_to_8() {
    for min in 1..=8u8 {
        let n = 1usize << min;
        let t = table(n);
        let px = pixels(u64::from(min), 20 * 20, n);
        let d = Gif::new(20, 20, &t)
            .image(&Img::new(20, 20).min(min), &px)
            .trailer();
        let (i, out) = ok(&d);
        assert_eq!(i.frame.min_code_size, min);
        assert_eq!(out, rgba(&t, &px), "min {min}");
    }
}

#[test]
fn min_code_size_over_8_is_read_failed() {
    let d = Gif::new(1, 1, &P2)
        .image_blocks(&Img::new(1, 1).min(9), &[0])
        .trailer();
    assert_eq!(err(&d), Error::ReadFailed);
    assert_eq!(info(&d).unwrap_err(), Error::ReadFailed);
    // In a later frame it ends the scan only.
    let d = Gif::new(1, 1, &P2)
        .image(&Img::new(1, 1), &[0])
        .image_blocks(&Img::new(1, 1).min(9), &[0])
        .trailer();
    let (i, _) = ok(&d);
    assert_eq!((i.frame_count, i.complete), (1, false));
}

#[test]
fn eof_code_too_soon() {
    let px = pixels(1, 3, 4);
    let d = Gif::new(2, 2, &P4)
        .image_lzw(&Img::new(2, 2), &lzw_encode(&px, 2, &[]))
        .trailer();
    assert_eq!(err(&d), Error::EofTooSoon);
    // `read_info` skips the LZW data, so it does not see it.
    assert!(info(&d).is_ok());
}

#[test]
fn lzw_defects() {
    let img = Img::new(2, 1);
    // An empty sub-block where LZW data is needed.
    let d = Gif::new(2, 1, &P4).image_blocks(&img, &[0]).trailer();
    assert_eq!(err(&d), Error::ImageDefect);
    // An empty sub-block after one byte (the clear code and one pixel).
    let d = Gif::new(2, 1, &P4)
        .image_blocks(&img, &[1, 0x44, 0])
        .trailer();
    assert_eq!(err(&d), Error::ImageDefect);
    // Clear, then code 7: no prefix and no last code.
    let d = Gif::new(2, 1, &P4)
        .image_blocks(&img, &[1, 0x3C, 0])
        .trailer();
    assert_eq!(err(&d), Error::ImageDefect);
    // A sub-block longer than the input.
    let d = Gif::new(2, 1, &P4)
        .raw(&[0x2C, 0, 0, 0, 0, 2, 0, 1, 0, 0, 2, 9, 1])
        .bytes();
    assert_eq!(err(&d), Error::ReadFailed);
}

#[test]
fn sub_blocks_after_the_last_pixel_are_dropped() {
    let px = pixels(2, 9, 4);
    let mut blocks = subblocks(&lzw_encode(&px, 2, &[]), 255);
    blocks.pop();
    blocks.extend_from_slice(&[2, 9, 9, 1, 7, 0]);
    let d = Gif::new(3, 3, &P4)
        .image_blocks(&Img::new(3, 3), &blocks)
        .trailer();
    let (i, out) = ok(&d);
    assert_eq!(out, rgba(&P4, &px));
    assert!(i.complete);
}

#[test]
fn truncation_at_every_offset() {
    let mut g = Gif::new(3, 3, &P4);
    assert_eq!(g.size(), 25);
    g = g.ext(0xFE, &[b"hi"]).gce(1, 10, 2);
    let img = g.size();
    g = g.image(&Img::new(3, 3), &pixels(5, 9, 4));
    let img_end = g.size();
    g = g.image(&Img::new(1, 1), &[1]);
    let second_end = g.size();
    let d = g.trailer();
    let total = d.len();
    assert!(img > 25 && img_end > img && second_end > img_end && total == second_end + 1);

    for len in 0..=total {
        let cut = &d[..len];
        let (r, ri) = (dec(cut), info(cut));
        let want = if len < 6 {
            Some(Error::ReadFailed)
        } else if len < 25 {
            Some(Error::NoScreenDescriptor)
        } else if len < img_end {
            // Inside an extension, the descriptor or the LZW data: a short read.
            Some(Error::ReadFailed)
        } else {
            None
        };
        match want {
            Some(e) => {
                assert_eq!(r.unwrap_err(), e, "len {len}");
                assert_eq!(ri.unwrap_err(), e, "len {len}");
            }
            None => {
                let (i, _) = r.unwrap();
                let frames = if len >= second_end { 2 } else { 1 };
                assert_eq!(i.frame_count, frames, "len {len}");
                assert_eq!(i.complete, len == total, "len {len}");
                assert_eq!(ri.unwrap(), i, "len {len}");
            }
        }
    }
    // By hand.
    assert_eq!(err(&d[..0]), Error::ReadFailed);
    assert_eq!(err(&d[..5]), Error::ReadFailed);
    assert_eq!(err(&d[..6]), Error::NoScreenDescriptor);
    assert_eq!(err(&d[..24]), Error::NoScreenDescriptor);
    assert_eq!(err(&d[..25]), Error::ReadFailed);
    assert_eq!(err(&d[..img_end - 1]), Error::ReadFailed);
}

#[test]
fn missing_trailer_after_the_first_frame() {
    let d = Gif::new(2, 2, &P4)
        .image(&Img::new(2, 2), &[0, 1, 2, 3])
        .bytes();
    let (i, out) = ok(&d);
    assert_eq!((i.frame_count, i.complete), (1, false));
    assert_eq!(out, rgba(&P4, &[0, 1, 2, 3]));
}

#[test]
fn interlaced_1_to_17_rows() {
    for h in 1..=17u16 {
        for w in [1u16, 3] {
            let px = pixels(u64::from(h), usize::from(w) * usize::from(h), 4);
            let d = Gif::new(w, h, &P4)
                .image(&Img::new(w, h).interlaced(), &px)
                .trailer();
            let (i, out) = ok(&d);
            assert!(i.frame.interlaced && i.frame.flags & 0x40 != 0);
            assert_eq!(out, rgba(&P4, &px), "{w}x{h}");
        }
    }
}

#[test]
fn canvas_grows_to_the_frame() {
    // The frame passes the screen on the right and at the bottom.
    let px = pixels(1, 5 * 2, 4);
    let d = Gif::new(4, 4, &P4)
        .image(&Img::new(5, 2).at(2, 3), &px)
        .trailer();
    let (i, out) = ok(&d);
    assert_eq!((i.width, i.height), (7, 5));
    assert_eq!((i.screen_width, i.screen_height), (4, 4));
    assert_eq!(out, place(7, 5, 2, 3, 5, &rgba(&P4, &px)));
    assert_eq!((i.frame.left, i.frame.top, i.frame.width), (2, 3, 5));

    // The frame is inside the screen: the rest stays transparent black, and the background
    // colour index is not painted.
    let px = pixels(2, 4, 4);
    let mut d = Gif::new(6, 5, &P4)
        .image(&Img::new(2, 2).at(3, 2), &px)
        .trailer();
    d[11] = 1;
    let (i, out) = ok(&d);
    assert_eq!((i.width, i.height, i.background_index), (6, 5, 1));
    assert_eq!(out, place(6, 5, 3, 2, 2, &rgba(&P4, &px)));

    // A screen smaller than the frame, or empty.
    for (sw, sh) in [(1, 1), (0, 0)] {
        let px = pixels(3, 9, 4);
        let d = Gif::new(sw, sh, &P4).image(&Img::new(3, 3), &px).trailer();
        let (i, out) = ok(&d);
        assert_eq!((i.width, i.height), (3, 3));
        assert_eq!(out, rgba(&P4, &px));
    }
}

fn with_transparent(px: &[u8], t: u8) -> Vec<u8> {
    let mut v = rgba(&P4, px);
    for (i, &p) in px.iter().enumerate() {
        if p == t {
            v[i * 4..i * 4 + 4].copy_from_slice(&[0, 0, 0, 0]);
        }
    }
    v
}

#[test]
fn transparency_and_control() {
    let px = [0, 1, 2, 3, 1, 1];
    // Disposal 2, user input, transparent index 1.
    let d = Gif::new(3, 2, &P4)
        .gce(0b0000_1011, 25, 1)
        .image(&Img::new(3, 2), &px)
        .trailer();
    let (i, out) = ok(&d);
    assert_eq!(
        i.frame.control,
        Some(Control {
            disposal: 2,
            user_input: true,
            delay: 25,
            transparent: Some(1)
        })
    );
    assert_eq!(out, with_transparent(&px, 1));

    // The last GCE wins.
    let d = Gif::new(3, 2, &P4)
        .gce(1, 0, 0)
        .gce(1, 0, 1)
        .image(&Img::new(3, 2), &px)
        .trailer();
    let (i, out) = ok(&d);
    assert_eq!(i.frame.control.and_then(|c| c.transparent), Some(1));
    assert_eq!(out, with_transparent(&px, 1));

    // The last GCE has no transparent flag: nothing is transparent.
    let d = Gif::new(3, 2, &P4)
        .gce(1, 0, 0)
        .gce(0, 4, 0)
        .image(&Img::new(3, 2), &px)
        .trailer();
    let (i, out) = ok(&d);
    assert_eq!(
        i.frame.control.map(|c| (c.delay, c.transparent)),
        Some((4, None))
    );
    assert_eq!(out, rgba(&P4, &px));

    // A GCE after the image is for a later frame.
    let d = Gif::new(3, 2, &P4)
        .image(&Img::new(3, 2), &px)
        .gce(1, 0, 1)
        .trailer();
    let (i, out) = ok(&d);
    assert_eq!(i.frame.control, None);
    assert_eq!(out, rgba(&P4, &px));
}

#[test]
fn short_gce_is_ignored() {
    let px = [0, 1, 2, 3];
    // Alone: no control, but the block is kept.
    let d = Gif::new(2, 2, &P4)
        .ext(0xF9, &[&[1, 2, 3]])
        .image(&Img::new(2, 2), &px)
        .trailer();
    let (i, out) = ok(&d);
    assert_eq!(i.frame.control, None);
    assert_eq!(out, rgba(&P4, &px));
    assert_eq!(i.extensions.len(), 1);
    assert_eq!(i.extensions[0].label, 0xF9);
    // After a valid GCE: the valid one stays. A GCE with no sub-block is ignored too.
    let d = Gif::new(2, 2, &P4)
        .gce(1, 0, 3)
        .ext(0xF9, &[&[1, 2, 3]])
        .ext(0xF9, &[])
        .image(&Img::new(2, 2), &px)
        .trailer();
    let (i, out) = ok(&d);
    assert_eq!(i.frame.control.and_then(|c| c.transparent), Some(3));
    assert_eq!(out, with_transparent(&px, 3));
    assert_eq!(i.extensions.len(), 3);
}

#[test]
fn index_past_the_table_is_opaque_black() {
    // A table of 2 entries and a code size of 2: indices 2 and 3 have no entry.
    let px = [0, 1, 2, 3];
    let d = Gif::new(2, 2, &P2).image(&Img::new(2, 2), &px).trailer();
    assert_eq!(
        ok(&d).1,
        [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]
    );
    // A transparent index past the table is still transparent.
    let d = Gif::new(2, 2, &P2)
        .gce(1, 0, 3)
        .image(&Img::new(2, 2), &px)
        .trailer();
    assert_eq!(
        ok(&d).1,
        [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 0, 255, 0, 0, 0, 0]
    );
}

#[test]
fn colour_tables() {
    let px = [0, 1, 2, 3];
    // No table at all.
    let d = Gif::new(2, 2, &[]).image(&Img::new(2, 2), &px).trailer();
    assert_eq!(err(&d), Error::NoColorTable);
    assert!(info(&d).is_ok());
    // LZW errors come first.
    let lzw = lzw_encode(&px[..3], 2, &[]);
    let d = Gif::new(2, 2, &[])
        .image_lzw(&Img::new(2, 2), &lzw)
        .trailer();
    assert_eq!(err(&d), Error::EofTooSoon);
    // A local table alone is enough, and it wins over the global one.
    let d = Gif::new(2, 2, &[])
        .image(&Img::new(2, 2).local(&P4), &px)
        .trailer();
    let (i, out) = ok(&d);
    assert_eq!(out, rgba(&P4, &px));
    assert!(i.frame.local_table.is_some());
    assert_eq!(i.global_table, None);
    let d = Gif::new(2, 2, &P2)
        .image(&Img::new(2, 2).local(&P4), &px)
        .trailer();
    let (i, out) = ok(&d);
    assert_eq!(out, rgba(&P4, &px));
    let local = i.frame.local_table.unwrap();
    assert_eq!(local.len(), 12);
    assert_eq!(&d[local.start..local.start + 3], &[255, 0, 0]);
}

#[test]
fn record_errors() {
    // Trailer before an image.
    let d = Gif::new(1, 1, &P2).trailer();
    assert_eq!(err(&d), Error::NoImage);
    assert_eq!(info(&d).unwrap_err(), Error::NoImage);
    let d = Gif::new(1, 1, &P2).ext(0xFE, &[b"x"]).trailer();
    assert_eq!(err(&d), Error::NoImage);
    // A wrong record byte.
    let d = Gif::new(1, 1, &P2).raw(&[0x99]).bytes();
    assert_eq!(err(&d), Error::WrongRecord);
    let d = Gif::new(1, 1, &P2).ext(0xFE, &[b"x"]).raw(&[0]).bytes();
    assert_eq!(err(&d), Error::WrongRecord);
    // End of data before any image.
    let d = Gif::new(1, 1, &P2).bytes();
    assert_eq!(err(&d), Error::ReadFailed);
    // After the first frame a wrong record only ends the scan.
    let d = Gif::new(1, 1, &P2)
        .image(&Img::new(1, 1), &[0])
        .raw(&[0x99])
        .bytes();
    let (i, _) = ok(&d);
    assert_eq!((i.frame_count, i.complete), (1, false));
}

#[test]
fn header_checks_only_gif() {
    let good = simple(2, 2, &[0, 1, 2, 3]);
    let mut d = good.clone();
    d[3..6].copy_from_slice(b"xyz");
    let (i, out) = ok(&d);
    assert_eq!(i.version, *b"xyz");
    assert_eq!(out, rgba(&P4, &[0, 1, 2, 3]));
    let mut d = good.clone();
    d[..6].copy_from_slice(b"GIF87a");
    assert_eq!(ok(&d).0.version, *b"87a");
    for bad in [b"gif89a", b"GIX89a", b"PNG\r\n\x1a"] {
        let mut d = good.clone();
        d[..6].copy_from_slice(bad);
        assert_eq!(err(&d), Error::NotGif);
        assert_eq!(info(&d).unwrap_err(), Error::NotGif);
    }
    assert_eq!(err(b""), Error::ReadFailed);
    assert_eq!(err(b"GIF8"), Error::ReadFailed);
    // Fewer than 6 bytes is ReadFailed even if they are not GIF.
    assert_eq!(err(b"abc"), Error::ReadFailed);
}

#[test]
fn extensions_are_kept_raw_in_order() {
    let pt_head = [0u8; 12];
    let d = Gif::new(2, 2, &P4)
        .ext(0xFE, &[b"first comment"])
        .ext(0xFF, &[b"NETSCAPE2.0", &[1, 0, 0]])
        .ext(0x01, &[&pt_head, b"text"])
        .ext(0x77, &[&[1, 2, 3], &[4]])
        .gce(0, 7, 0)
        .image(&Img::new(2, 2), &[0, 1, 2, 3])
        .ext(0xFE, &[b"after"])
        .image(&Img::new(1, 1), &[0])
        .ext(0xFE, &[b"end"])
        .trailer();
    let (i, _) = ok(&d);
    let want: [(u8, u32, Vec<u8>); 7] = [
        (0xFE, 0, raw_blocks(&[b"first comment"])),
        (0xFF, 0, raw_blocks(&[b"NETSCAPE2.0", &[1, 0, 0]])),
        (0x01, 0, raw_blocks(&[&pt_head, b"text"])),
        (0x77, 0, raw_blocks(&[&[1, 2, 3], &[4]])),
        (0xF9, 0, raw_blocks(&[&[0, 7, 0, 0]])),
        (0xFE, 1, raw_blocks(&[b"after"])),
        (0xFE, 2, raw_blocks(&[b"end"])),
    ];
    assert_eq!(i.extensions.len(), want.len());
    for (e, (label, frame, raw)) in i.extensions.iter().zip(&want) {
        assert_eq!((e.label, e.frame), (*label, *frame));
        assert_eq!(e.bytes(&d), &raw[..]);
        assert_eq!(&d[e.raw.clone()], &raw[..]);
    }
    assert_eq!((i.frame_count, i.complete), (2, true));
    assert_eq!(
        i.frame.control,
        Some(Control {
            disposal: 0,
            user_input: false,
            delay: 7,
            transparent: None
        })
    );
    let e = &i.extensions;
    assert_eq!(e[1].application_id(&d), Some(&b"NETSCAPE2.0"[..]));
    assert_eq!(e[0].application_id(&d), None);
    assert_eq!(e[3].application_id(&d), None);
    let blocks: Vec<&[u8]> = e[1].sub_blocks(&d).collect();
    assert_eq!(blocks, [&b"NETSCAPE2.0"[..], &[1u8, 0, 0][..]]);
    let blocks: Vec<&[u8]> = e[3].sub_blocks(&d).collect();
    assert_eq!(blocks, [&[1u8, 2, 3][..], &[4u8][..]]);
    // read_info gives the same list.
    assert_eq!(info(&d).unwrap(), i);
}

#[test]
fn xmp_packet() {
    // The packet bytes are sub-blocks too: their length bytes are part of the text.
    let packet = [4u8, b'<', b'x', b'/', b'>', 2, b'a', b'b'];
    let id = {
        let mut v = vec![11u8];
        v.extend_from_slice(b"XMP DataXMP");
        v
    };
    for with_magic in [true, false] {
        let mut payload = id.clone();
        payload.extend_from_slice(&packet);
        if with_magic {
            payload.extend_from_slice(&magic());
        }
        payload.push(0);
        let d = Gif::new(1, 1, &P2)
            .raw(&[0x21, 0xFF])
            .raw(&payload)
            .image(&Img::new(1, 1), &[0])
            .trailer();
        let (i, _) = ok(&d);
        assert_eq!(i.extensions.len(), 1, "magic {with_magic}");
        let e = &i.extensions[0];
        assert_eq!(e.xmp(&d), Some(&packet[..]), "magic {with_magic}");
        assert_eq!(e.application_id(&d), Some(&b"XMP DataXMP"[..]));
        assert_eq!(e.bytes(&d), &payload[..]);
    }
    // Other blocks are not XMP.
    let d = Gif::new(1, 1, &P2)
        .ext(0xFF, &[b"NETSCAPE2.0", &[1, 0, 0]])
        .ext(0xFE, &[b"XMP DataXMP"])
        .image(&Img::new(1, 1), &[0])
        .trailer();
    let (i, _) = ok(&d);
    assert!(i.extensions.iter().all(|e| e.xmp(&d).is_none()));
}

fn frames(n: usize) -> Gif {
    let mut g = Gif::new(2, 2, &P4);
    for k in 0..n {
        g = g.image(&Img::new(2, 2), &pixels(k as u64, 4, 4));
    }
    g
}

#[test]
fn frame_count() {
    let d = frames(3).trailer();
    let (i, out) = ok(&d);
    assert_eq!((i.frame_count, i.complete), (3, true));
    assert_eq!(out, rgba(&P4, &pixels(0, 4, 4)));
    assert_eq!(info(&d).unwrap(), i);

    // The third frame is cut inside its data: 2 frames, not complete.
    let two = frames(2).size();
    let d = frames(3).trailer();
    for cut in [two + 1, two + 5, two + 10, d.len() - 2] {
        let (i, _) = ok(&d[..cut]);
        assert_eq!((i.frame_count, i.complete), (2, false), "cut {cut}");
        assert_eq!(info(&d[..cut]).unwrap(), i, "cut {cut}");
    }
}

#[test]
fn max_extensions_truncates_the_list_only() {
    let px = [0, 1, 2, 3];
    let g = Gif::new(2, 2, &P4)
        .ext(0xFE, &[b"1"])
        .ext(0xFE, &[b"2"])
        .gce(1, 0, 2)
        .ext(0xFE, &[b"4"])
        .image(&Img::new(2, 2), &px)
        .ext(0xFE, &[b"5"]);
    let d = g.trailer();
    for (max, want_len, truncated) in [
        (0, 0, true),
        (2, 2, true),
        (4, 4, true),
        (5, 5, false),
        (99, 5, false),
    ] {
        let l = Limits {
            max_extensions: max,
            ..Limits::default()
        };
        let (i, out) = gif::decode(&d, &l, &no()).unwrap();
        assert_eq!(i.extensions.len(), want_len, "max {max}");
        assert_eq!(i.extensions_truncated, truncated, "max {max}");
        // The GCE still applies, and the pixels are the same.
        assert_eq!(i.frame.control.and_then(|c| c.transparent), Some(2));
        assert_eq!(out, with_transparent(&px, 2));
        assert!(i.complete);
        assert_eq!(gif::read_info(&d, &l, &no()).unwrap(), i);
    }
    // The list grows past its first allocation.
    let mut g = Gif::new(1, 1, &P2);
    for _ in 0..40 {
        g = g.ext(0xFE, &[b"c"]);
    }
    let d = g.image(&Img::new(1, 1), &[0]).trailer();
    assert_eq!(ok(&d).0.extensions.len(), 40);
}

#[test]
fn size_limits() {
    let d = simple(10, 2, &pixels(1, 20, 4));
    let lim = |w: u32, h: u32, p: u64| Limits {
        max_width: w,
        max_height: h,
        max_pixels: p,
        ..Limits::default()
    };
    let run = |l: &Limits| gif::decode(&d, l, &no()).map(|_| ());
    let run_info = |l: &Limits| gif::read_info(&d, l, &no()).map(|_| ());
    assert_eq!(run(&lim(9, 99, 99)), Err(Error::WidthLimit));
    assert_eq!(run(&lim(99, 1, 99)), Err(Error::HeightLimit));
    assert_eq!(run(&lim(99, 99, 19)), Err(Error::PixelLimit));
    assert_eq!(run(&lim(9, 1, 19)), Err(Error::WidthLimit));
    assert_eq!(run(&lim(10, 2, 20)), Ok(()));
    assert_eq!(run_info(&lim(9, 99, 99)), Err(Error::WidthLimit));
    assert_eq!(run_info(&lim(99, 1, 99)), Err(Error::HeightLimit));
    assert_eq!(run_info(&lim(99, 99, 19)), Err(Error::PixelLimit));
    assert_eq!(run_info(&lim(10, 2, 20)), Ok(()));

    // The limits apply to the canvas that a frame offset makes.
    let d = Gif::new(10, 2, &P4)
        .image(&Img::new(5, 1).at(100, 0), &[0; 5])
        .trailer();
    let l = Limits {
        max_width: 100,
        ..Limits::default()
    };
    assert_eq!(gif::decode(&d, &l, &no()).unwrap_err(), Error::WidthLimit);
}

#[test]
fn limits_come_before_any_allocation() {
    // A canvas of 65535 x 65535 in a 30-byte file: the limit error comes at once.
    let d = Gif::new(65535, 65535, &P2)
        .image(&Img::new(1, 1), &[0])
        .trailer();
    assert_eq!(err(&d), Error::PixelLimit);
    let l = Limits {
        max_pixels: 1 << 40,
        max_width: 1 << 17,
        ..Limits::default()
    };
    assert_eq!(gif::decode(&d, &l, &no()).unwrap_err(), Error::AllocLimit);
    // The same through the memory limit alone.
    let d = Gif::new(1000, 1000, &P2)
        .image(&Img::new(1, 1), &[0])
        .trailer();
    for max_alloc in [0, 1, 1000, 1 << 20, 3_999_999] {
        let l = Limits {
            max_alloc,
            ..Limits::default()
        };
        assert_eq!(
            gif::decode(&d, &l, &no()).unwrap_err(),
            Error::AllocLimit,
            "max_alloc {max_alloc}"
        );
    }
    // `read_info` allocates no output: it needs no memory but the extension list.
    let l = Limits {
        max_alloc: 0,
        ..Limits::default()
    };
    assert!(gif::read_info(&d, &l, &no()).is_ok());
    let d = Gif::new(1, 1, &P2)
        .ext(0xFE, &[b"x"])
        .image(&Img::new(1, 1), &[0])
        .trailer();
    let i = gif::read_info(&d, &l, &no()).unwrap();
    assert!(i.extensions.is_empty() && i.extensions_truncated);
}

#[test]
fn cancel() {
    let d = simple(3, 3, &pixels(1, 9, 4));
    let set = AtomicBool::new(true);
    let l = Limits::default();
    assert_eq!(gif::decode(&d, &l, &set).unwrap_err(), Error::Cancelled);
    assert_eq!(gif::read_info(&d, &l, &set).unwrap_err(), Error::Cancelled);
    let mut dec = Decoder::new();
    let mut out = [0u8; 36];
    assert_eq!(
        dec.decode_into(&d, &mut out, &l, &set).unwrap_err(),
        Error::Cancelled
    );
    // The decoder works again with a clear flag.
    assert!(dec.decode_into(&d, &mut out, &l, &no()).is_ok());
}

#[test]
fn decode_into_equals_decode_and_read_info_equals_decode() {
    let files = [
        simple(1, 1, &[2]),
        simple(7, 5, &pixels(1, 35, 4)),
        frames(3).trailer(),
        Gif::new(4, 4, &P4)
            .gce(1, 3, 1)
            .ext(0xFE, &[b"c"])
            .image(&Img::new(5, 2).at(2, 3).interlaced(), &pixels(2, 10, 4))
            .bytes(),
        big(&[5000]).0,
        simple(7, 5, &pixels(1, 35, 4)),
    ];
    let l = Limits::default();
    let mut dec = Decoder::new();
    let mut buf = Vec::new();
    for (n, d) in files.iter().enumerate() {
        let (i, out) = ok(d);
        assert_eq!(info(d).unwrap(), i, "file {n}");
        let size = dec.read_info(d, &l, &no()).unwrap().output_size();
        assert_eq!(size, out.len());
        // Dirty buffer: every byte of the output must be written.
        buf.clear();
        buf.resize(size, 0xA5);
        let j = dec.decode_into(d, &mut buf, &l, &no()).unwrap().clone();
        assert_eq!(j, i, "file {n}");
        assert_eq!(buf, out, "file {n}");
    }
}

#[test]
fn decode_into_checks_the_output_size() {
    let l = Limits::default();
    let mut dec = Decoder::new();
    let d = simple(2, 2, &[0, 1, 2, 3]);
    for n in [0, 15, 17, 64] {
        let mut out = vec![0u8; n];
        assert_eq!(
            dec.decode_into(&d, &mut out, &l, &no()).unwrap_err(),
            Error::OutputSize,
            "len {n}"
        );
    }
    // A bad file gives its own error first, then the limits.
    let mut out = [0u8; 3];
    assert_eq!(
        dec.decode_into(&d[..30], &mut out, &l, &no()).unwrap_err(),
        err(&d[..30])
    );
    assert_eq!(
        dec.decode_into(&d, &mut out, &Limits { max_width: 1, ..l }, &no())
            .unwrap_err(),
        Error::WidthLimit
    );
    // The decoder is fine afterwards.
    let mut out = [0u8; 16];
    dec.decode_into(&d, &mut out, &l, &no()).unwrap();
    assert_eq!(out[..4], [255, 0, 0, 255]);
}

// Frames with no pixels: DGifSlurp rejects them before any DGifGetLine call (`ReadFailed`).

#[test]
fn empty_frame() {
    let lzw = lzw_encode(&[], 2, &[]);
    for (w, h, inter) in [(0, 0, false), (0, 3, true), (3, 0, false), (3, 0, true)] {
        let img = Img::new(w, h);
        let img = if inter { img.interlaced() } else { img };
        let d = Gif::new(2, 2, &P4).image_lzw(&img, &lzw).trailer();
        assert_eq!(err(&d), Error::ReadFailed, "{w}x{h} interlaced={inter}");
    }
}

#[test]
fn error_tokens() {
    let all = [
        (Error::ReadFailed, "read-failed"),
        (Error::NotGif, "not-gif"),
        (Error::NoScreenDescriptor, "no-scrn-dscr"),
        (Error::WrongRecord, "wrong-record"),
        (Error::DataTooBig, "data-too-big"),
        (Error::ImageDefect, "image-defect"),
        (Error::EofTooSoon, "eof-too-soon"),
        (Error::NoImage, "no-image"),
        (Error::NoColorTable, "no-color-table"),
        (Error::WidthLimit, "limit"),
        (Error::HeightLimit, "limit"),
        (Error::PixelLimit, "limit"),
        (Error::AllocLimit, "limit"),
        (Error::OutOfMemory, "oom"),
    ];
    for (e, t) in all {
        assert_eq!(e.token(), t);
        assert!(!e.to_string().is_empty());
        let _: &dyn std::error::Error = &e;
    }
}
