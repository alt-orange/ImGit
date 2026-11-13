//! Image files to pixels (the `image` crate), on every platform.

use std::fs;
use std::path::Path;

/// Decoded pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Straight (not premultiplied) alpha, rows top to bottom.
    pub rgba: Vec<u8>,
}

/// PNG / JPEG / GIF (first frame) / BMP to RGBA8. With `scale_to`, resampled to that many pixels square with a
/// box filter on alpha-weighted pixels (what WIC's Fant scaler did, and close to Chromium's `<img>` downscale).
/// `None` when the bytes are not an image of one of those formats.
#[must_use]
pub fn decode_image(bytes: &[u8], scale_to: Option<u32>) -> Option<Image> {
    if bytes.is_empty() {
        return None;
    }
    let decoded = image::load_from_memory(bytes).ok()?.into_rgba8();
    let image = Image {
        width: decoded.width(),
        height: decoded.height(),
        rgba: decoded.into_raw(),
    };
    match scale_to {
        Some(size) if size > 0 => Some(resize_box(&image, size, size)),
        _ => Some(image),
    }
}

/// [`decode_image`] of the file at `path`.
#[must_use]
pub fn load_image(path: &Path, scale_to: Option<u32>) -> Option<Image> {
    decode_image(&fs::read(path).ok()?, scale_to)
}

/// For each destination pixel along one axis, the source pixels its footprint covers and how much of each:
/// `(first source index, weights)`. This is the box filter of stb_image_resize2 (`STBIR_FILTER_BOX`, whose
/// trapezoid is exactly this area overlap), with clamped edges.
fn axis_coverage(source: u32, destination: u32) -> Vec<(usize, Vec<f32>)> {
    let ratio = f64::from(source) / f64::from(destination);
    (0..destination)
        .map(|index| {
            let start = f64::from(index) * ratio;
            let end = (f64::from(index) + 1.0) * ratio;
            let first = start.floor() as usize;
            let last = ((end.ceil() as usize).max(first + 1)).min(source as usize);
            let weights = (first..last)
                .map(|pixel| {
                    let overlap = end.min(pixel as f64 + 1.0) - start.max(pixel as f64);
                    overlap.max(0.0) as f32
                })
                .collect();
            (first, weights)
        })
        .collect()
}

/// Resamples `image` to `width` x `height` with a box filter. Colours are weighted by their alpha while
/// filtering (so transparent texels do not bleed their colour into the edges) and returned as straight alpha.
#[must_use]
pub fn resize_box(image: &Image, width: u32, height: u32) -> Image {
    if image.width == 0 || image.height == 0 || width == 0 || height == 0 {
        return Image {
            width,
            height,
            rgba: vec![0; width as usize * height as usize * 4],
        };
    }
    let columns = axis_coverage(image.width, width);
    let rows = axis_coverage(image.height, height);
    let stride = image.width as usize * 4;
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for (first_row, row_weights) in &rows {
        for (first_column, column_weights) in &columns {
            // Premultiplied sums over the footprint.
            let mut sum = [0.0f32; 4];
            let mut total = 0.0f32;
            for (y, row_weight) in row_weights.iter().enumerate() {
                let row = &image.rgba[(first_row + y) * stride..][..stride];
                for (x, column_weight) in column_weights.iter().enumerate() {
                    let weight = row_weight * column_weight;
                    let pixel = &row[(first_column + x) * 4..][..4];
                    let alpha = f32::from(pixel[3]) / 255.0;
                    sum[0] += f32::from(pixel[0]) * alpha * weight;
                    sum[1] += f32::from(pixel[1]) * alpha * weight;
                    sum[2] += f32::from(pixel[2]) * alpha * weight;
                    sum[3] += alpha * weight;
                    total += weight;
                }
            }
            if total <= 0.0 || sum[3] <= 0.0 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let to_byte = |value: f32| (value + 0.5).clamp(0.0, 255.0) as u8;
            rgba.extend_from_slice(&[
                to_byte(sum[0] / sum[3]),
                to_byte(sum[1] / sum[3]),
                to_byte(sum[2] / sum[3]),
                to_byte(sum[3] / total * 255.0),
            ]);
        }
    }
    Image {
        width,
        height,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets;

    #[test]
    fn decodes_the_embedded_icon() {
        let bytes = assets::data("images/app_icon.png").expect("embedded");
        let image = decode_image(bytes, None).expect("a PNG");
        assert_eq!(
            image.rgba.len(),
            image.width as usize * image.height as usize * 4
        );
        let scaled = decode_image(bytes, Some(32)).expect("a PNG");
        assert_eq!(
            (scaled.width, scaled.height, scaled.rgba.len()),
            (32, 32, 32 * 32 * 4)
        );
        assert!(decode_image(b"not an image", None).is_none());
        assert!(decode_image(&[], None).is_none());
    }

    #[test]
    fn box_filter_weights_colour_by_alpha() {
        // 2x1: opaque red beside fully transparent green. The average is half-transparent pure red, not a
        // red-green mix.
        let image = Image {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 255, 0, 0],
        };
        assert_eq!(resize_box(&image, 1, 1).rgba, [255, 0, 0, 128]);
    }

    #[test]
    fn box_filter_averages_whole_footprints() {
        // 4x2 opaque greys halved in both directions: each output pixel is the mean of a 2x2 block.
        let greys = [0u8, 100, 50, 150, 200, 100, 250, 150];
        let rgba = greys
            .iter()
            .flat_map(|&grey| [grey, grey, grey, 255])
            .collect();
        let scaled = resize_box(
            &Image {
                width: 4,
                height: 2,
                rgba,
            },
            2,
            1,
        );
        assert_eq!(scaled.rgba, [100, 100, 100, 255, 150, 150, 150, 255]);
        // A non-integer ratio splits the middle pixel between both outputs.
        let row = Image {
            width: 3,
            height: 1,
            rgba: vec![0, 0, 0, 255, 90, 90, 90, 255, 255, 255, 255, 255],
        };
        assert_eq!(
            resize_box(&row, 2, 1).rgba,
            [30, 30, 30, 255, 200, 200, 200, 255]
        );
    }
}
