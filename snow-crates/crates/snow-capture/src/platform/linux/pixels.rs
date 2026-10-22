//! Packed desktop pixels. Portal and X11 sources both land here before a frame is published.

pub(crate) fn rgba_len(width: u32, height: u32) -> Option<usize> {
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)
}

pub(crate) fn bgrx_to_rgba(src: &[u8], width: u32, height: u32, stride: usize) -> Option<Vec<u8>> {
    let row_bytes = (width as usize).checked_mul(4)?;
    if stride < row_bytes {
        return None;
    }
    let mut dst = vec![0u8; rgba_len(width, height)?];
    for y in 0..height as usize {
        let src_row = src.get(y * stride..y * stride + row_bytes)?;
        let dst_row = &mut dst[y * row_bytes..y * row_bytes + row_bytes];
        for x in 0..width as usize {
            let s = x * 4;
            dst_row[s] = src_row[s + 2];
            dst_row[s + 1] = src_row[s + 1];
            dst_row[s + 2] = src_row[s];
            dst_row[s + 3] = 255;
        }
    }
    Some(dst)
}

pub(crate) fn bgra_to_rgba(src: &[u8], width: u32, height: u32, stride: usize) -> Option<Vec<u8>> {
    let row_bytes = (width as usize).checked_mul(4)?;
    if stride < row_bytes {
        return None;
    }
    let mut dst = vec![0u8; rgba_len(width, height)?];
    for y in 0..height as usize {
        let src_row = src.get(y * stride..y * stride + row_bytes)?;
        let dst_row = &mut dst[y * row_bytes..y * row_bytes + row_bytes];
        for x in 0..width as usize {
            let s = x * 4;
            dst_row[s] = src_row[s + 2];
            dst_row[s + 1] = src_row[s + 1];
            dst_row[s + 2] = src_row[s];
            dst_row[s + 3] = src_row[s + 3];
        }
    }
    Some(dst)
}

pub(crate) fn copy_rgba(src: &[u8], width: u32, height: u32, stride: usize) -> Option<Vec<u8>> {
    let row_bytes = (width as usize).checked_mul(4)?;
    if stride < row_bytes {
        return None;
    }
    let mut dst = vec![0u8; rgba_len(width, height)?];
    for y in 0..height as usize {
        let src_row = src.get(y * stride..y * stride + row_bytes)?;
        dst[y * row_bytes..y * row_bytes + row_bytes].copy_from_slice(src_row);
    }
    Some(dst)
}

/// Crop a tightly packed RGBA image. Coordinates are clamped to the source.
pub(crate) fn crop_rgba(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    if width == 0 || height == 0 || x >= src_width || y >= src_height {
        return None;
    }
    let width = width.min(src_width - x);
    let height = height.min(src_height - y);
    let src_stride = src_width as usize * 4;
    let row_bytes = width as usize * 4;
    if src.len() < src_stride * src_height as usize {
        return None;
    }
    let mut dst = vec![0u8; row_bytes * height as usize];
    for row in 0..height as usize {
        let src_off = (y as usize + row) * src_stride + x as usize * 4;
        let dst_off = row * row_bytes;
        dst[dst_off..dst_off + row_bytes]
            .copy_from_slice(&src[src_off..src_off + row_bytes]);
    }
    Some(dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgrx_swaps_channels_and_forces_opaque_alpha() {
        let src = [10u8, 20, 30, 0];
        let rgba = bgrx_to_rgba(&src, 1, 1, 4).unwrap();
        assert_eq!(rgba, [30, 20, 10, 255]);
    }

    #[test]
    fn crop_keeps_the_requested_rectangle() {
        let mut src = vec![0u8; 4 * 4 * 4];
        src[4 * 4 + 4] = 7;
        let cropped = crop_rgba(&src, 4, 4, 1, 1, 1, 1).unwrap();
        assert_eq!(cropped, [7, 0, 0, 0]);
    }
}
