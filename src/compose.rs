//! The composition policy (docs/design.md, "Composition"): the canvas size, the palette and the
//! row painter.

use crate::decoder::Frame;

/// The canvas: `max(screen, left + frame width)` by `max(screen, top + frame height)`.
pub(crate) fn canvas(screen_width: u16, screen_height: u16, f: &Frame) -> (u32, u32) {
    let w = u32::from(screen_width).max(u32::from(f.left) + u32::from(f.width));
    let h = u32::from(screen_height).max(u32::from(f.top) + u32::from(f.height));
    (w, h)
}

/// Bytes of an RGBA canvas, or `None` if the size does not fit `usize`.
pub(crate) fn rgba_size(width: u32, height: u32) -> Option<usize> {
    let n = u64::from(width)
        .checked_mul(u64::from(height))?
        .checked_mul(4)?;
    usize::try_from(n).ok()
}

/// RGBA for each index. An index past the table is opaque black. The transparent index is
/// `[0; 4]`, which is what the canvas holds already. `None` if there is no table.
pub(crate) fn palette(table: Option<&[u8]>, transparent: Option<u8>) -> Option<[[u8; 4]; 256]> {
    let table = table?;
    let mut pal = [[0, 0, 0, 255]; 256];
    for (e, c) in pal.iter_mut().zip(table.chunks_exact(3)) {
        if let [r, g, b] = *c {
            *e = [r, g, b, 255];
        }
    }
    if let Some(t) = transparent {
        pal[usize::from(t)] = [0; 4];
    }
    Some(pal)
}

/// The bytes of frame row `y` on the canvas: `n` pixels from column `left`.
pub(crate) fn row_mut(
    out: &mut [u8],
    canvas_width: u32,
    left: u16,
    y: u32,
    n: u16,
) -> Option<&mut [u8]> {
    let px = usize::try_from(y)
        .ok()?
        .checked_mul(usize::try_from(canvas_width).ok()?)?
        .checked_add(usize::from(left))?;
    let start = px.checked_mul(4)?;
    let end = start.checked_add(usize::from(n) * 4)?;
    out.get_mut(start..end)
}

/// Paints one row of palette indices.
pub(crate) fn paint_row(dst: &mut [u8], idx: &[u8], pal: &[[u8; 4]; 256]) {
    for (px, &i) in dst.chunks_exact_mut(4).zip(idx) {
        px.copy_from_slice(&pal[usize::from(i)]);
    }
}
