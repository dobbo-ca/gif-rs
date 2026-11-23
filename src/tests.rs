//! Unit tests of the modules. Whole-file tests are in `tests/decode.rs`.

use crate::compose;
use crate::decoder::{Control, Extension, Frame, Info};
use crate::error::Error;
use crate::ledger::Ledger;
use crate::lzw::Lzw;
use crate::parse;
use crate::reader::Reader;
use std::sync::atomic::AtomicBool;

static NO: AtomicBool = AtomicBool::new(false);

#[test]
fn ledger_charges_and_peak() {
    let mut l = Ledger::default();
    l.reset(100, 10).unwrap();
    l.reserve(90).unwrap();
    assert_eq!(l.reserve(1).unwrap_err(), Error::AllocLimit);
    assert_eq!(l.peak(), 100);
    assert_eq!(l.reserve(usize::MAX).unwrap_err(), Error::AllocLimit);
    // A new call starts again; `held` over `max` is a limit error at once.
    assert_eq!(l.reset(5, 6).unwrap_err(), Error::AllocLimit);
    l.reset(5, 5).unwrap();
}

#[test]
fn reader_never_passes_the_end() {
    let data = [1u8, 2, 3];
    let mut r = Reader::new(&data);
    assert_eq!(r.take(2), Some(&data[..2]));
    assert_eq!(r.take(2), None);
    assert_eq!(r.pos(), 2);
    assert_eq!(r.take(usize::MAX), None);
    assert_eq!(r.byte(), Some(3));
    assert_eq!(r.byte(), None);
    assert_eq!(r.take(0), Some(&data[3..]));
    assert_eq!(r.pos(), 3);
}

/// min code size 2, a 2-byte sub-block, one pixel (index 0): clear (4), 0, eof (5) in 3-bit codes.
const ONE_PIXEL: [u8; 5] = [0x02, 0x02, 0x44, 0x01, 0x00];

#[test]
fn lzw_one_pixel_and_flush() {
    let mut r = Reader::new(&ONE_PIXEL);
    assert_eq!(r.byte(), Some(2));
    let mut z = Lzw::new();
    z.setup(2, 1, 1);
    let mut line = [9u8];
    z.get_line(&mut r, &mut line, 1, &NO).unwrap();
    assert_eq!(line, [0]);
    // The call that reached PixelCount 0 read up to the terminator.
    assert_eq!(r.pos(), 5);
}

#[test]
fn lzw_eof_before_the_last_pixel() {
    let mut r = Reader::new(&ONE_PIXEL);
    r.byte();
    let mut z = Lzw::new();
    z.setup(2, 2, 1);
    let mut line = [0u8; 2];
    assert_eq!(
        z.get_line(&mut r, &mut line, 2, &NO).unwrap_err(),
        Error::EofTooSoon
    );
}

#[test]
fn lzw_bad_prefix_and_empty_block() {
    // clear (4), then code 7: Prefix[7] is NO_SUCH_CODE and LastCode is NO_SUCH_CODE.
    let bad = [0x01, 0x3C, 0x00];
    let mut z = Lzw::new();
    z.setup(2, 2, 1);
    let mut line = [0u8; 2];
    assert_eq!(
        z.get_line(&mut Reader::new(&bad), &mut line, 2, &NO)
            .unwrap_err(),
        Error::ImageDefect
    );
    // An empty sub-block where LZW data is needed.
    z.setup(2, 2, 1);
    assert_eq!(
        z.get_line(&mut Reader::new(&[0u8]), &mut line, 2, &NO)
            .unwrap_err(),
        Error::ImageDefect
    );
    // A short sub-block.
    z.setup(2, 2, 1);
    assert_eq!(
        z.get_line(&mut Reader::new(&[5u8, 1]), &mut line, 2, &NO)
            .unwrap_err(),
        Error::ReadFailed
    );
}

#[test]
fn lzw_data_too_big() {
    // Width 3, height 0: LineLen 0 means the width and PixelCount goes below 0.
    let mut z = Lzw::new();
    z.setup(2, 3, 0);
    assert_eq!(
        z.get_line(&mut Reader::new(&[]), &mut [], 0, &NO)
            .unwrap_err(),
        Error::DataTooBig
    );
}

#[test]
fn gce_needs_four_bytes() {
    let data = [4u8, 0b0000_1001, 5, 0, 7, 0];
    let c = parse::gce(&data, &(0..6)).unwrap();
    assert_eq!(
        c,
        Control {
            disposal: 2,
            user_input: false,
            delay: 5,
            transparent: Some(7)
        }
    );
    assert_eq!(parse::gce(&[3, 1, 2, 3, 0], &(0..5)), None);
    assert_eq!(parse::gce(&[0], &(0..1)), None);
    assert_eq!(parse::gce(&data, &(0..99)), None);
}

#[test]
fn header_rules() {
    let mut info = Info::default();
    assert_eq!(
        parse::header(&mut Reader::new(b"GIF89"), &mut info).unwrap_err(),
        Error::ReadFailed
    );
    assert_eq!(
        parse::header(&mut Reader::new(b"GIX89a"), &mut info).unwrap_err(),
        Error::NotGif
    );
    assert_eq!(
        parse::header(&mut Reader::new(b"GIF89a\x01\x00"), &mut info).unwrap_err(),
        Error::NoScreenDescriptor
    );
    // A global table of 2 entries, one byte short.
    assert_eq!(
        parse::header(
            &mut Reader::new(b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x01\x02\x03\x04\x05"),
            &mut info
        )
        .unwrap_err(),
        Error::NoScreenDescriptor
    );
    parse::header(
        &mut Reader::new(b"GIFxyz\x01\x00\x02\x00\x00\x07\x09"),
        &mut info,
    )
    .unwrap();
    assert_eq!(info.version, *b"xyz");
    assert_eq!((info.screen_width, info.screen_height), (1, 2));
    assert_eq!((info.background_index, info.aspect), (7, 9));
    assert_eq!(info.global_table, None);
}

#[test]
fn image_descriptor_rules() {
    // left 1, top 2, 3x4, flags 0x40 (interlaced), min code size 8.
    let d = [1u8, 0, 2, 0, 3, 0, 4, 0, 0x40, 8];
    let f = parse::image_desc(&mut Reader::new(&d), None).unwrap();
    assert_eq!(
        f,
        Frame {
            left: 1,
            top: 2,
            width: 3,
            height: 4,
            flags: 0x40,
            interlaced: true,
            local_table: None,
            min_code_size: 8,
            control: None,
        }
    );
    // Code size 9 and a missing code size are both ReadFailed.
    let mut big = d;
    big[9] = 9;
    assert_eq!(
        parse::image_desc(&mut Reader::new(&big), None).unwrap_err(),
        Error::ReadFailed
    );
    assert_eq!(
        parse::image_desc(&mut Reader::new(&d[..9]), None).unwrap_err(),
        Error::ReadFailed
    );
    // A local table of 2 entries.
    let mut t = d[..8].to_vec();
    t.extend_from_slice(&[0x80, 1, 2, 3, 4, 5, 6, 2]);
    let f = parse::image_desc(&mut Reader::new(&t), None).unwrap();
    assert_eq!(f.local_table, Some(9..15));
    assert!(!f.interlaced);
}

#[test]
fn canvas_and_palette() {
    let f = Frame {
        left: 3,
        top: 1,
        width: 5,
        height: 2,
        ..Frame::default()
    };
    assert_eq!(compose::canvas(4, 4, &f), (8, 4));
    assert_eq!(compose::canvas(10, 1, &f), (10, 3));
    assert_eq!(compose::rgba_size(3, 5), Some(60));

    assert!(compose::palette(None, None).is_none());
    let pal = compose::palette(Some(&[1, 2, 3, 4, 5, 6]), Some(1)).unwrap();
    assert_eq!(pal[0], [1, 2, 3, 255]);
    assert_eq!(pal[1], [0, 0, 0, 0]);
    assert_eq!(pal[2], [0, 0, 0, 255]);
    assert_eq!(pal[255], [0, 0, 0, 255]);
    // The transparent index can be past the table.
    let pal = compose::palette(Some(&[1, 2, 3]), Some(200)).unwrap();
    assert_eq!(pal[200], [0; 4]);
    assert_eq!(pal[199], [0, 0, 0, 255]);

    let mut out = [0u8; 4 * 6];
    let row = compose::row_mut(&mut out, 3, 1, 1, 2).unwrap();
    compose::paint_row(row, &[0, 1], &pal);
    assert_eq!(out[16..20], [1, 2, 3, 255]);
    assert!(compose::row_mut(&mut out, 3, 1, 1, 3).is_none());
}

fn ext(label: u8, raw: &[u8]) -> (Extension, Vec<u8>) {
    (
        Extension {
            label,
            frame: 0,
            raw: 0..raw.len(),
        },
        raw.to_vec(),
    )
}

fn magic() -> Vec<u8> {
    let mut m = vec![1u8];
    m.extend((0..=255u8).rev());
    m
}

#[test]
fn xmp_with_and_without_the_trailer() {
    let mut raw = vec![11u8];
    raw.extend_from_slice(b"XMP DataXMP");
    raw.extend_from_slice(b"<x/>");
    raw.extend_from_slice(&magic());
    raw.push(0);
    let (e, d) = ext(0xFF, &raw);
    assert_eq!(e.xmp(&d), Some(&b"<x/>"[..]));
    assert_eq!(e.application_id(&d), Some(&b"XMP DataXMP"[..]));

    // No trailer: everything up to the terminator, length bytes included.
    let mut raw = vec![11u8];
    raw.extend_from_slice(b"XMP DataXMP");
    raw.extend_from_slice(&[3, b'a', b'b', b'c', 0]);
    let (e, d) = ext(0xFF, &raw);
    assert_eq!(e.xmp(&d), Some(&[3, b'a', b'b', b'c'][..]));

    // Not XMP: another id, another label, a short block.
    let mut raw = vec![11u8];
    raw.extend_from_slice(b"NETSCAPE2.0");
    raw.push(0);
    let (e, d) = ext(0xFF, &raw);
    assert_eq!(e.xmp(&d), None);
    let (e, d) = ext(0xFE, &raw);
    assert_eq!(e.application_id(&d), None);
    assert_eq!(e.xmp(&d), None);
    let (e, d) = ext(0xFF, &[11, b'X']);
    assert_eq!(e.xmp(&d), None);
    // The range is not in the data.
    let e = Extension {
        label: 0xFF,
        frame: 0,
        raw: 5..50,
    };
    assert!(e.bytes(&[1, 2, 3]).is_empty());
    assert_eq!(e.sub_blocks(&[1, 2, 3]).count(), 0);
}

#[test]
fn sub_blocks_give_the_payloads() {
    let (e, d) = ext(0x01, &[2, 7, 8, 1, 9, 0]);
    let v: Vec<&[u8]> = e.sub_blocks(&d).collect();
    assert_eq!(v, [&[7u8, 8][..], &[9u8][..]]);
    assert_eq!(e.bytes(&d), &d[..]);
}

#[test]
fn lzw_clear_code_flood_honours_cancel() {
    // Min code size 0: clear is code 1 and every code is 1 bit, so each 0xFF byte is eight clear
    // codes that write no pixel. Without a cancel check inside the line this runs to the end of
    // the input.
    let mut data = Vec::new();
    for _ in 0..20 {
        data.push(255);
        data.extend_from_slice(&[0xFF; 255]);
    }
    data.push(0);
    let mut z = Lzw::new();
    z.setup(0, 1, 1);
    let cancel = AtomicBool::new(true);
    let mut line = [0u8; 1];
    assert_eq!(
        z.get_line(&mut Reader::new(&data), &mut line, 1, &cancel)
            .unwrap_err(),
        Error::Cancelled
    );
    // Not set: the flood runs to the end of the input.
    z.setup(0, 1, 1);
    assert_eq!(
        z.get_line(&mut Reader::new(&data), &mut line, 1, &NO)
            .unwrap_err(),
        Error::ImageDefect
    );
}
