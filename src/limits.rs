//! Caps on one call.

/// Caps on one call. Every entry point takes `&Limits` and a cancel flag. The defaults are finite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Peak working memory of one call, in bytes (default 1 GiB). The output `Vec` of
    /// [`decode`](crate::decode), the LZW tables, the line buffer and the extension list count.
    /// Each is charged before it is allocated. More is
    /// [`Error::AllocLimit`](crate::Error::AllocLimit).
    pub max_alloc: usize,
    /// Most OS threads of one call (default `std::thread::available_parallelism`). This crate
    /// never spawns a thread: the LZW decoder is serial. The field exists for the shared
    /// convention.
    pub max_threads: usize,
    /// Most pixels per canvas row (default `1 << 17`). More is
    /// [`Error::WidthLimit`](crate::Error::WidthLimit).
    pub max_width: u32,
    /// Most canvas rows (default `1 << 17`). More is
    /// [`Error::HeightLimit`](crate::Error::HeightLimit).
    pub max_height: u32,
    /// Most canvas `width * height` (default `1 << 28`). More is
    /// [`Error::PixelLimit`](crate::Error::PixelLimit).
    pub max_pixels: u64,
    /// Most extension blocks recorded in [`Info::extensions`](crate::Info::extensions) (default
    /// 4096). The rest are not recorded and
    /// [`Info::extensions_truncated`](crate::Info::extensions_truncated) is set. This is not an
    /// error: metadata never fails a decode.
    pub max_extensions: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_alloc: 1 << 30,
            max_threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
            max_width: 1 << 17,
            max_height: 1 << 17,
            max_pixels: 1 << 28,
            max_extensions: 4096,
        }
    }
}
