//! The LZW decoder: a port of giflib 5.2.1 `DGifSetupDecompress`, `DGifGetLine`,
//! `DGifDecompressLine`, `DGifGetPrefixChar`, `DGifDecompressInput` and `DGifBufferedInput`.
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
//!
//! The names in the comments are giflib's, so a reviewer can compare line by line. Differences:
//! the input is a byte cursor, not a `FILE`; the sub-block buffer `Private->Buf` is not copied
//! (`buf_left` is `Buf[0]`, the bytes left in the sub-block, and `buf_pos` is `Buf[1]`, the
//! offset of the next byte in the input); the tables are fixed arrays; every index is masked to
//! 12 bits, which changes nothing because giflib's own checks keep it in range.

use crate::error::Error;
use crate::parse;
use crate::reader::Reader;

const LZ_MAX_CODE: i32 = 4095; // Biggest code possible in 12 bits.
const LZ_BITS: i32 = 12;
const NO_SUCH_CODE: i32 = 4098; // Impossible code, to signal empty.
const NO_SUCH_U16: u16 = 4098;
const MASK: usize = 0xFFF;

/// Table index, always in range.
fn ix(v: i32) -> usize {
    (v as usize) & MASK
}

/// The `GifFilePrivateType` fields that the decoder uses.
pub(crate) struct Lzw {
    prefix: [u16; 4096], // Prefix[LZ_MAX_CODE + 1]
    suffix: [u8; 4096],  // Suffix[LZ_MAX_CODE + 1]
    stack: [u8; 4096],   // Stack[LZ_MAX_CODE] (one spare entry)
    bits_per_pixel: i32, // BitsPerPixel
    clear_code: i32,     // ClearCode
    eof_code: i32,       // EOFCode
    running_code: i32,   // RunningCode: the next code the algorithm can generate
    running_bits: i32,   // RunningBits
    max_code1: i32,      // MaxCode1
    last_code: i32,      // LastCode
    stack_ptr: i32,      // StackPtr
    crnt_shift_state: i32,
    crnt_shift_dword: u64,
    pixel_count: i64, // PixelCount (giflib: unsigned long, compared as unsigned)
    width: u16,       // GifFile->Image.Width
    buf_left: u8,     // Buf[0]
    buf_pos: usize,   // Buf[1]
}

impl Lzw {
    pub(crate) fn new() -> Self {
        Self {
            prefix: [NO_SUCH_U16; 4096],
            suffix: [0; 4096],
            stack: [0; 4096],
            bits_per_pixel: 0,
            clear_code: 0,
            eof_code: 0,
            running_code: 0,
            running_bits: 0,
            max_code1: 0,
            last_code: NO_SUCH_CODE,
            stack_ptr: 0,
            crnt_shift_state: 0,
            crnt_shift_dword: 0,
            pixel_count: 0,
            width: 0,
            buf_left: 0,
            buf_pos: 0,
        }
    }

    /// `DGifGetImageHeader` sets `PixelCount`, then `DGifSetupDecompress` runs. The code size
    /// byte is read and checked (over 8) by `parse::image_desc`.
    pub(crate) fn setup(&mut self, code_size: u8, width: u16, height: u16) {
        self.pixel_count = i64::from(width) * i64::from(height);
        self.width = width;
        let bits_per_pixel = i32::from(code_size);
        self.buf_left = 0; // Input Buffer empty.
        self.buf_pos = 0;
        self.bits_per_pixel = bits_per_pixel;
        self.clear_code = 1 << bits_per_pixel;
        self.eof_code = self.clear_code + 1;
        self.running_code = self.eof_code + 1;
        self.running_bits = bits_per_pixel + 1; // Number of bits per code.
        self.max_code1 = 1 << self.running_bits; // Max. code + 1.
        self.stack_ptr = 0; // No pixels on the pixel stack.
        self.last_code = NO_SUCH_CODE;
        self.crnt_shift_state = 0; // No information in CrntShiftDWord.
        self.crnt_shift_dword = 0;
        self.prefix.fill(NO_SUCH_U16);
    }

    /// `DGifGetLine`: decodes `line_len` pixels into `line` (0 means the frame width). After the
    /// call that brings `PixelCount` to 0, reads and drops the rest of the sub-blocks.
    pub(crate) fn get_line(
        &mut self,
        r: &mut Reader<'_>,
        line: &mut [u8],
        line_len: usize,
    ) -> Result<(), Error> {
        let line_len = if line_len == 0 {
            usize::from(self.width)
        } else {
            line_len
        };
        self.pixel_count = self.pixel_count.wrapping_sub(line_len as i64);
        if (self.pixel_count as u64) > 0xffff_0000 {
            return Err(Error::DataTooBig);
        }
        let line = line.get_mut(..line_len).ok_or(Error::ReadFailed)?;
        self.decompress_line(r, line)?;
        if self.pixel_count == 0 {
            // We probably won't be called any more: flush the rest of the image until an empty
            // block (size 0) is read.
            parse::skip_blocks(r)?;
        }
        Ok(())
    }

    /// `DGifDecompressLine`: decompresses into `line` (length `LineLen`).
    fn decompress_line(&mut self, r: &mut Reader<'_>, line: &mut [u8]) -> Result<(), Error> {
        let line_len = line.len();
        let mut i = 0usize;
        let eof_code = self.eof_code;
        let clear_code = self.clear_code;
        let mut last_code = self.last_code;
        let mut stack_ptr = self.stack_ptr;

        if stack_ptr > LZ_MAX_CODE {
            // giflib returns GIF_ERROR with no code set.
            return Err(Error::ReadFailed);
        }

        if stack_ptr != 0 {
            // Let pop the stack off before continueing to read the GIF file.
            while stack_ptr != 0 && i < line_len {
                stack_ptr -= 1;
                if let Some(p) = line.get_mut(i) {
                    *p = self.stack[ix(stack_ptr)];
                }
                i += 1;
            }
        }

        while i < line_len {
            // Decode LineLen items.
            let crnt_code = self.decompress_input(r)?;

            if crnt_code == eof_code {
                // EOF before the last pixel (5.2.1 has no exception for the last pixel).
                return Err(Error::EofTooSoon);
            } else if crnt_code == clear_code {
                // We need to start over again.
                self.prefix.fill(NO_SUCH_U16);
                self.running_code = self.eof_code + 1;
                self.running_bits = self.bits_per_pixel + 1;
                self.max_code1 = 1 << self.running_bits;
                last_code = NO_SUCH_CODE;
                self.last_code = NO_SUCH_CODE;
            } else {
                // Its regular code: if in pixel range simply add it to the output stream,
                // otherwise trace the codes linked list until the prefix is in pixel range.
                if crnt_code < clear_code {
                    // This is simple: its pixel scalar, so add it to output.
                    if let Some(p) = line.get_mut(i) {
                        *p = crnt_code as u8;
                    }
                    i += 1;
                } else {
                    let mut crnt_prefix;
                    if i32::from(self.prefix[ix(crnt_code)]) == NO_SUCH_CODE {
                        crnt_prefix = last_code;

                        // Only allowed if CrntCode is exactly the running code: in that case
                        // CrntCode = XXXCode, CrntCode or the prefix code is last code and the
                        // suffix char is exactly the prefix of last code!
                        let c = if crnt_code == self.running_code - 2 {
                            prefix_char(&self.prefix, last_code, clear_code)
                        } else {
                            prefix_char(&self.prefix, crnt_code, clear_code)
                        };
                        // giflib stores the int into a byte: NO_SUCH_CODE (4098) becomes 2.
                        self.suffix[ix(self.running_code - 2)] = c as u8;
                        self.stack[ix(stack_ptr)] = c as u8;
                        stack_ptr += 1;
                    } else {
                        crnt_prefix = crnt_code;
                    }

                    // Now (if image is O.K.) we should not get a NO_SUCH_CODE during the
                    // trace. StackPtr is the loop counter: stop before overflowing Stack[].
                    while stack_ptr < LZ_MAX_CODE
                        && crnt_prefix > clear_code
                        && crnt_prefix <= LZ_MAX_CODE
                    {
                        self.stack[ix(stack_ptr)] = self.suffix[ix(crnt_prefix)];
                        stack_ptr += 1;
                        crnt_prefix = i32::from(self.prefix[ix(crnt_prefix)]);
                    }
                    if stack_ptr >= LZ_MAX_CODE || crnt_prefix > LZ_MAX_CODE {
                        return Err(Error::ImageDefect);
                    }
                    // Push the last character on stack.
                    self.stack[ix(stack_ptr)] = crnt_prefix as u8;
                    stack_ptr += 1;

                    // Now lets pop all the stack into output.
                    while stack_ptr != 0 && i < line_len {
                        stack_ptr -= 1;
                        if let Some(p) = line.get_mut(i) {
                            *p = self.stack[ix(stack_ptr)];
                        }
                        i += 1;
                    }
                }
                if last_code != NO_SUCH_CODE
                    && self.running_code - 2 < (LZ_MAX_CODE + 1)
                    && i32::from(self.prefix[ix(self.running_code - 2)]) == NO_SUCH_CODE
                {
                    self.prefix[ix(self.running_code - 2)] = last_code as u16;

                    let c = if crnt_code == self.running_code - 2 {
                        prefix_char(&self.prefix, last_code, clear_code)
                    } else {
                        prefix_char(&self.prefix, crnt_code, clear_code)
                    };
                    self.suffix[ix(self.running_code - 2)] = c as u8;
                }
                last_code = crnt_code;
            }
        }

        self.last_code = last_code;
        self.stack_ptr = stack_ptr;
        Ok(())
    }

    /// `DGifDecompressInput`: the next code from the bit stream.
    fn decompress_input(&mut self, r: &mut Reader<'_>) -> Result<i32, Error> {
        // The image can't contain more than LZ_BITS per code.
        if self.running_bits > LZ_BITS {
            return Err(Error::ImageDefect);
        }

        while self.crnt_shift_state < self.running_bits {
            // Needs to get more bytes from input stream for next code.
            let next_byte = self.buffered_input(r)?;
            self.crnt_shift_dword |= u64::from(next_byte) << self.crnt_shift_state;
            self.crnt_shift_state += 8;
        }
        // CodeMasks[RunningBits] is (1 << RunningBits) - 1.
        let code = (self.crnt_shift_dword & ((1u64 << self.running_bits) - 1)) as i32;

        self.crnt_shift_dword >>= self.running_bits;
        self.crnt_shift_state -= self.running_bits;

        // If code cannot fit into RunningBits bits, must raise its size. Codes above 4095 are
        // used for special signaling. If we're using LZ_BITS bits already and we're at the max
        // code, just keep using the table as it is, don't increment RunningCode.
        if self.running_code < LZ_MAX_CODE + 2 {
            self.running_code += 1;
            if self.running_code > self.max_code1 && self.running_bits < LZ_BITS {
                self.max_code1 <<= 1;
                self.running_bits += 1;
            }
        }
        Ok(code)
    }

    /// `DGifBufferedInput`: the next byte of the LZW data. Reads the next sub-block when the
    /// buffer is empty.
    fn buffered_input(&mut self, r: &mut Reader<'_>) -> Result<u8, Error> {
        if self.buf_left == 0 {
            // Needs to read the next buffer: this one is empty.
            let n = r.byte().ok_or(Error::ReadFailed)?;
            // There shouldn't be any empty data blocks here as the LZW spec says the LZW
            // termination code should come first.
            if n == 0 {
                return Err(Error::ImageDefect);
            }
            let start = r.pos();
            r.take(usize::from(n)).ok_or(Error::ReadFailed)?;
            self.buf_pos = start; // Buf[1] = 1, the first byte of the block
            self.buf_left = n; // Buf[0] = n
        }
        let b = *r.data().get(self.buf_pos).ok_or(Error::ReadFailed)?;
        self.buf_pos += 1;
        self.buf_left -= 1;
        Ok(b)
    }
}

/// `DGifGetPrefixChar`: traces the prefix chain to the pixel value. Gives `NO_SUCH_CODE` on a
/// bad chain.
fn prefix_char(prefix: &[u16; 4096], mut code: i32, clear_code: i32) -> i32 {
    let mut i = 0;
    // while (Code > ClearCode && i++ <= LZ_MAX_CODE)
    while code > clear_code {
        let go = i <= LZ_MAX_CODE;
        i += 1;
        if !go {
            break;
        }
        if code > LZ_MAX_CODE {
            return NO_SUCH_CODE;
        }
        code = i32::from(prefix[ix(code)]);
    }
    code
}
