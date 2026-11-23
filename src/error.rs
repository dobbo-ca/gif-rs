//! The one error enum.

use std::fmt;

/// Why a call failed.
///
/// Most variants map to a giflib 5.2.1 error code. [`Error::token`] gives the short class name
/// that the parity oracles write. The limit variants are distinct so a caller can match on them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The input ends inside a record, or the LZW code size is over 8 (giflib
    /// `D_GIF_ERR_READ_FAILED`, or no code set).
    ReadFailed,
    /// The first 3 bytes are not `GIF` (giflib `D_GIF_ERR_NOT_GIF_FILE`).
    NotGif,
    /// The logical screen descriptor or the global colour table is short (giflib
    /// `D_GIF_ERR_NO_SCRN_DSCR`).
    NoScreenDescriptor,
    /// A record starts with a byte that is not `,`, `!` or `;` (giflib
    /// `D_GIF_ERR_WRONG_RECORD`).
    WrongRecord,
    /// The pixel count of the frame is off (giflib `D_GIF_ERR_DATA_TOO_BIG`).
    DataTooBig,
    /// The LZW data is bad: an empty sub-block, a bad prefix chain or a code that is too wide
    /// (giflib `D_GIF_ERR_IMAGE_DEFECT`).
    ImageDefect,
    /// The LZW end code comes before the last pixel (giflib `D_GIF_ERR_EOF_TOO_SOON`).
    EofTooSoon,
    /// The trailer comes before any image descriptor (policy; giflib has no code for it).
    NoImage,
    /// The frame has no local and no global colour table (policy).
    NoColorTable,
    /// The canvas width is over `Limits::max_width`.
    WidthLimit,
    /// The canvas height is over `Limits::max_height`.
    HeightLimit,
    /// The canvas `width * height` is over `Limits::max_pixels`.
    PixelLimit,
    /// The working memory would pass `Limits::max_alloc`.
    AllocLimit,
    /// The allocator refused a size that is within the limits.
    OutOfMemory,
    /// The output slice is not `Info::output_size` bytes long.
    OutputSize,
    /// The cancel flag was set.
    Cancelled,
}

impl Error {
    /// The parity class token: `read-failed`, `not-gif`, `no-scrn-dscr`, `wrong-record`,
    /// `data-too-big`, `image-defect`, `eof-too-soon`, `no-image`, `no-color-table`, `limit`
    /// (the four limit variants), `oom`, `output-size` or `cancelled`.
    pub fn token(&self) -> &'static str {
        match self {
            Self::ReadFailed => "read-failed",
            Self::NotGif => "not-gif",
            Self::NoScreenDescriptor => "no-scrn-dscr",
            Self::WrongRecord => "wrong-record",
            Self::DataTooBig => "data-too-big",
            Self::ImageDefect => "image-defect",
            Self::EofTooSoon => "eof-too-soon",
            Self::NoImage => "no-image",
            Self::NoColorTable => "no-color-table",
            Self::WidthLimit | Self::HeightLimit | Self::PixelLimit | Self::AllocLimit => "limit",
            Self::OutOfMemory => "oom",
            Self::OutputSize => "output-size",
            Self::Cancelled => "cancelled",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ReadFailed => "gif: failed to read from the input",
            Self::NotGif => "gif: not a GIF file",
            Self::NoScreenDescriptor => "gif: no logical screen descriptor",
            Self::WrongRecord => "gif: wrong record type",
            Self::DataTooBig => "gif: too much pixel data",
            Self::ImageDefect => "gif: the image data is defective",
            Self::EofTooSoon => "gif: end code before the last pixel",
            Self::NoImage => "gif: no image in the file",
            Self::NoColorTable => "gif: the frame has no colour table",
            Self::WidthLimit => "gif: image width over the limit",
            Self::HeightLimit => "gif: image height over the limit",
            Self::PixelLimit => "gif: image pixel count over the limit",
            Self::AllocLimit => "gif: memory limit exceeded",
            Self::OutOfMemory => "gif: out of memory",
            Self::OutputSize => "gif: output buffer has the wrong size",
            Self::Cancelled => "gif: cancelled",
        })
    }
}

impl std::error::Error for Error {}
