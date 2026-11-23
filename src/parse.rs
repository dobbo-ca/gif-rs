//! The record parser. It follows giflib's `DGifOpen`, `DGifGetScreenDesc`, `DGifGetRecordType`,
//! `DGifGetImageDesc` (with the `DGifGetImageHeader` checks), `DGifGetExtension(Next)` and
//! `DGifGetCodeNext`, including which error each failure gives.
//!
//! giflib is MIT licensed. This file is derived from `dgif_lib.c`:
//!
//! ```text
//! The GIFLIB distribution is Copyright (c) 1997  Eric S. Raymond
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in
//! all copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.  IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
//! THE SOFTWARE.
//! ```

use crate::decoder::{Control, Frame, Info};
use crate::error::Error;
use crate::reader::Reader;
use std::ops::Range;

/// What the next record is.
pub(crate) enum Record {
    Image,
    Extension,
    Trailer,
}

/// Bytes of a colour table: 3 * 2^(1 + (flags & 7)).
fn table_bytes(flags: u8) -> usize {
    3usize << ((flags & 7) + 1)
}

/// `DGifOpen` and `DGifGetScreenDesc`: the header, the screen descriptor and the global table.
pub(crate) fn header(r: &mut Reader<'_>, info: &mut Info) -> Result<(), Error> {
    let h: [u8; 6] = r
        .take(6)
        .and_then(|s| s.try_into().ok())
        .ok_or(Error::ReadFailed)?;
    if h[0] != b'G' || h[1] != b'I' || h[2] != b'F' {
        return Err(Error::NotGif);
    }
    info.version = [h[3], h[4], h[5]];
    // DGifOpen maps every DGifGetScreenDesc failure to D_GIF_ERR_NO_SCRN_DSCR.
    let s: [u8; 7] = r
        .take(7)
        .and_then(|s| s.try_into().ok())
        .ok_or(Error::NoScreenDescriptor)?;
    info.screen_width = u16::from_le_bytes([s[0], s[1]]);
    info.screen_height = u16::from_le_bytes([s[2], s[3]]);
    info.screen_flags = s[4];
    info.background_index = s[5];
    info.aspect = s[6];
    if s[4] & 0x80 != 0 {
        let start = r.pos();
        let n = table_bytes(s[4]);
        r.take(n).ok_or(Error::NoScreenDescriptor)?;
        info.global_table = Some(start..start + n);
    }
    Ok(())
}

/// `DGifGetRecordType`.
pub(crate) fn record_type(r: &mut Reader<'_>) -> Result<Record, Error> {
    match r.byte().ok_or(Error::ReadFailed)? {
        0x2C => Ok(Record::Image),
        0x21 => Ok(Record::Extension),
        0x3B => Ok(Record::Trailer),
        _ => Err(Error::WrongRecord),
    }
}

/// `DGifGetCodeNext` in a loop: reads and drops the sub-blocks up to the 0 terminator.
pub(crate) fn skip_blocks(r: &mut Reader<'_>) -> Result<(), Error> {
    loop {
        let n = r.byte().ok_or(Error::ReadFailed)?;
        if n == 0 {
            return Ok(());
        }
        r.take(usize::from(n)).ok_or(Error::ReadFailed)?;
    }
}

/// `DGifGetExtension` and `DGifGetExtensionNext`: the label, then the sub-blocks. Gives the label
/// and the range of the sub-blocks (length bytes and the 0 terminator included).
pub(crate) fn extension(r: &mut Reader<'_>) -> Result<(u8, Range<usize>), Error> {
    let label = r.byte().ok_or(Error::ReadFailed)?;
    let start = r.pos();
    skip_blocks(r)?;
    Ok((label, start..r.pos()))
}

/// The GCE in a raw extension block. `None` if the first sub-block has fewer than 4 bytes.
pub(crate) fn gce(data: &[u8], raw: &Range<usize>) -> Option<Control> {
    let b = data.get(raw.clone())?;
    if *b.first()? < 4 {
        return None;
    }
    let g: [u8; 4] = b.get(1..5)?.try_into().ok()?;
    Some(Control {
        disposal: (g[0] >> 2) & 7,
        user_input: g[0] & 2 != 0,
        delay: u16::from_le_bytes([g[1], g[2]]),
        transparent: (g[0] & 1 != 0).then_some(g[3]),
    })
}

/// `DGifGetImageDesc` (with `DGifGetImageHeader` and the code size byte of
/// `DGifSetupDecompress`): the 9 bytes, the local table and the LZW minimum code size.
pub(crate) fn image_desc(r: &mut Reader<'_>, control: Option<Control>) -> Result<Frame, Error> {
    let d: [u8; 9] = r
        .take(9)
        .and_then(|s| s.try_into().ok())
        .ok_or(Error::ReadFailed)?;
    let flags = d[8];
    let local_table = if flags & 0x80 != 0 {
        let start = r.pos();
        let n = table_bytes(flags);
        r.take(n).ok_or(Error::ReadFailed)?;
        Some(start..start + n)
    } else {
        None
    };
    // A missing code size byte: giflib returns an error with no code set.
    let min_code_size = r.byte().ok_or(Error::ReadFailed)?;
    if min_code_size > 8 {
        // giflib: "somewhat bogus error code".
        return Err(Error::ReadFailed);
    }
    Ok(Frame {
        left: u16::from_le_bytes([d[0], d[1]]),
        top: u16::from_le_bytes([d[2], d[3]]),
        width: u16::from_le_bytes([d[4], d[5]]),
        height: u16::from_le_bytes([d[6], d[7]]),
        flags,
        interlaced: flags & 0x40 != 0,
        local_table,
        min_code_size,
        control,
    })
}
