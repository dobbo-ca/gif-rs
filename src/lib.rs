//! A GIF decoder with strict resource limits, no C dependency and no panics on hostile input.
//!
//! It decodes the first frame only (v1), composited on the canvas as RGBA8, and scans the rest
//! of the file for the frame count and the extension blocks, which it keeps as byte ranges into
//! the input. The parser and the LZW decoder follow giflib 5.2.1: the same checks in the same
//! order, and the same error at every point. See `docs/design.md` for the policy (canvas size,
//! transparency, missing colours) and `docs/differences.md` for where giflib and browsers differ.
//!
//! # Resource limits
//!
//! Every call takes a [`Limits`] and a cancel flag (`&AtomicBool`, set from another thread).
//!
//! - `max_alloc` (bytes, default 1 GiB): the peak working memory of the call. The output `Vec` of
//!   [`decode`], the LZW tables, the line buffer and the extension list are charged before they
//!   are allocated. More is [`Error::AllocLimit`]; an allocator that refuses a size within the
//!   limit is [`Error::OutOfMemory`]. The extension list is the exception: if it does not fit,
//!   the remaining blocks are not recorded and [`Info::extensions_truncated`] is set, with no
//!   error.
//! - `max_threads` (default `available_parallelism`): the most OS threads of the call. This crate
//!   never spawns a thread, so every value gives the same output.
//! - `max_width`, `max_height` (default `1 << 17`) and `max_pixels` (default `1 << 28`) apply to
//!   the canvas and are checked before any buffer is sized from it: [`Error::WidthLimit`],
//!   [`Error::HeightLimit`], [`Error::PixelLimit`].
//! - `max_extensions` (default 4096): the most extension blocks recorded in
//!   [`Info::extensions`]. The rest are not recorded and [`Info::extensions_truncated`] is set.
//!   This is not an error.
//!
//! The cancel flag is read per record, per decoded line, per scanned frame and at every LZW clear
//! code. A set flag is [`Error::Cancelled`].
//!
//! ```
//! use gif::Limits;
//! use std::sync::atomic::AtomicBool;
//!
//! # fn main() -> Result<(), gif::Error> {
//! // A 1x1 GIF: a two-colour global table (red, green) and the pixel index 0.
//! let data = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\xff\x00\x00\x00\xff\x00\
//!              ,\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02\x44\x01\x00;";
//!
//! // One thread and at most 64 MiB for this call.
//! let limits = Limits {
//!     max_threads: 1,
//!     max_alloc: 64 << 20,
//!     ..Limits::default()
//! };
//! let cancel = AtomicBool::new(false);
//! let (info, rgba) = gif::decode(data, &limits, &cancel)?;
//! assert_eq!((info.width, info.height, info.frame_count), (1, 1, 1));
//! assert_eq!(rgba, [255, 0, 0, 255]);
//!
//! // A `max_alloc` that is too small fails before anything is allocated.
//! let tight = Limits { max_alloc: 1, ..limits };
//! assert_eq!(gif::decode(data, &tight, &cancel).unwrap_err(), gif::Error::AllocLimit);
//! # Ok(())
//! # }
//! ```
//!
//! # Errors
//!
//! Every entry point returns the one [`Error`] enum. Each function documents which variants it
//! can return. A scan error after the first frame is never returned: it sets
//! [`Info::complete`] to `false`.
//!
//! # Entry points
//!
//! [`decode`] decodes in one call and [`read_info`] reads the metadata without decoding the first
//! frame. [`Decoder`] keeps its buffers across calls: [`Decoder::decode_into`] writes into the
//! caller's slice and makes no heap allocation on a later call whose first frame is no wider, and
//! that has no more extension blocks, than an earlier decode on the same decoder.
#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented
    )
)]
#![deny(missing_docs)]

mod compose;
mod decoder;
mod error;
mod ledger;
mod limits;
mod lzw;
mod parse;
mod reader;

pub use decoder::{Control, Decoder, Extension, Frame, Info, decode, read_info};
pub use error::Error;
pub use limits::Limits;

#[cfg(test)]
mod tests;
