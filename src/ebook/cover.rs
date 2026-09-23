// Copyright (c) 2026 Otávio C.
// SPDX-License-Identifier: MIT

//! Album-art thumbnails, the cover collage, and the title plate drawn into it.

use std::path::{Path, PathBuf};

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ImageReader, Rgb, RgbImage};

pub const ART_MAX_EDGE: u32 = 600;
const JPEG_QUALITY: u8 = 82;
pub const COVER_WIDTH: u32 = 1600;
pub const COVER_HEIGHT: u32 = 2400;
const MAX_TILES: u64 = 96;
const MAX_COLUMNS: u64 = 8;
const COVER_DIM: u32 = 150;

const TITLE_FONT: &[u8] = include_bytes!("../../assets/Lora-Regular.ttf");

const PLATE_COLOR: Rgb<u8> = Rgb([20, 20, 26]);
const TITLE_COLOR: Rgb<u8> = Rgb([255, 255, 255]);
const TITLE_SCALE: f32 = 0.13;
const TITLE_TRACKING: f32 = 0.14;
const PLATE_PADDING_X: f32 = 0.75;
const PLATE_PADDING_Y: f32 = 0.45;
const PLATE_MAX_WIDTH: f32 = 0.86;

fn decode(path: &Path) -> Option<RgbImage> {
    let reader = ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    Some(reader.decode().ok()?.to_rgb8())
}

fn encode_jpeg(image: &RgbImage) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, JPEG_QUALITY)
        .encode_image(image)
        .ok()?;
    Some(bytes)
}

fn fit_within(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= max_edge || longest == 0 {
        return (width, height);
    }
    (
        scale(width, max_edge, longest).max(1),
        scale(height, max_edge, longest).max(1),
    )
}

fn scale(value: u32, numerator: u32, denominator: u32) -> u32 {
    if denominator == 0 {
        return 0;
    }
    let product = u64::from(value).saturating_mul(u64::from(numerator));
    product
        .checked_div(u64::from(denominator))
        .and_then(|scaled| u32::try_from(scaled).ok())
        .unwrap_or(u32::MAX)
}

#[must_use]
pub fn thumbnail(path: &Path, max_edge: u32) -> Option<Vec<u8>> {
    let image = decode(path)?;
    let (width, height) = fit_within(image.width(), image.height(), max_edge);
    let resized = if width == image.width() && height == image.height() {
        image
    } else {
        image::imageops::resize(&image, width, height, FilterType::Lanczos3)
    };
    encode_jpeg(&resized)
}

fn fill_cell(image: &RgbImage, cell_width: u32, cell_height: u32) -> RgbImage {
    let (width, height) = (image.width().max(1), image.height().max(1));

    let by_height = u64::from(width).saturating_mul(u64::from(cell_height))
        >= u64::from(height).saturating_mul(u64::from(cell_width));
    let (scaled_width, scaled_height) = if by_height {
        (
            scale(width, cell_height, height).max(cell_width),
            cell_height,
        )
    } else {
        (
            cell_width,
            scale(height, cell_width, width).max(cell_height),
        )
    };

    let mut scaled =
        image::imageops::resize(image, scaled_width, scaled_height, FilterType::Lanczos3);
    let x = scaled_width.saturating_sub(cell_width) / 2;
    let y = scaled_height.saturating_sub(cell_height) / 2;
    image::imageops::crop(&mut scaled, x, y, cell_width, cell_height).to_image()
}

fn dim(image: &mut RgbImage) {
    for pixel in image.pixels_mut() {
        for channel in &mut pixel.0 {
            let dimmed = u32::from(*channel)
                .saturating_mul(COVER_DIM)
                .checked_div(255)
                .unwrap_or(0);
            *channel = u8::try_from(dimmed).unwrap_or(u8::MAX);
        }
    }
}

fn grid_for(count: usize) -> (u32, u32) {
    let count = u64::try_from(count).unwrap_or(MAX_TILES).min(MAX_TILES);
    if count == 0 {
        return (0, 0);
    }
    let ratio = count
        .saturating_mul(u64::from(COVER_WIDTH))
        .checked_div(u64::from(COVER_HEIGHT))
        .unwrap_or(1);
    let columns = ratio.isqrt().clamp(1, MAX_COLUMNS);
    let rows = count.checked_div(columns).unwrap_or(1).max(1);
    (
        u32::try_from(columns).unwrap_or(1),
        u32::try_from(rows).unwrap_or(1),
    )
}

#[must_use]
pub fn pick_evenly<T: Clone>(items: &[T], count: usize) -> Vec<T> {
    if items.is_empty() || count == 0 {
        return Vec::new();
    }
    if items.len() <= count {
        return items.to_vec();
    }
    (0..count)
        .filter_map(|i| {
            let numerator = i
                .saturating_mul(2)
                .saturating_add(1)
                .saturating_mul(items.len());
            let index = numerator.checked_div(count.saturating_mul(2))?;
            items.get(index).cloned()
        })
        .collect()
}

fn collage(art_paths: &[PathBuf]) -> Option<RgbImage> {
    let candidates = pick_evenly(art_paths, usize::try_from(MAX_TILES).unwrap_or(0));
    let decoded: Vec<RgbImage> = candidates.iter().filter_map(|p| decode(p)).collect();
    if decoded.is_empty() {
        return None;
    }

    let (columns, rows) = grid_for(decoded.len());
    let cell_width = COVER_WIDTH.checked_div(columns)?;
    let cell_height = COVER_HEIGHT.checked_div(rows)?;
    let cells = usize::try_from(columns.saturating_mul(rows)).unwrap_or(0);

    let indices: Vec<usize> = pick_evenly(&(0..decoded.len()).collect::<Vec<usize>>(), cells);
    let tiles: Vec<RgbImage> = indices
        .iter()
        .filter_map(|&i| decoded.get(i))
        .map(|img| fill_cell(img, cell_width, cell_height))
        .collect();
    if tiles.is_empty() {
        return None;
    }

    let mut canvas = RgbImage::new(COVER_WIDTH, COVER_HEIGHT);
    for row in 0..rows {
        for column in 0..columns {
            let index = row.saturating_mul(columns).saturating_add(column);
            let Some(tile) = usize::try_from(index)
                .ok()
                .and_then(|i| i.checked_rem(tiles.len()))
                .and_then(|i| tiles.get(i))
            else {
                continue;
            };
            image::imageops::replace(
                &mut canvas,
                tile,
                i64::from(column.saturating_mul(cell_width)),
                i64::from(row.saturating_mul(cell_height)),
            );
        }
    }

    dim(&mut canvas);
    Some(canvas)
}

#[allow(
    clippy::as_conversions,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::suboptimal_flops
)]
fn title_width(font: &impl Font, title: &str, scale: PxScale) -> f32 {
    let scaled = font.as_scaled(scale);
    let tracking = scale.x * TITLE_TRACKING;
    let mut width = 0.0;
    let mut previous: Option<ab_glyph::GlyphId> = None;
    for c in title.chars() {
        let id = scaled.glyph_id(c);
        if let Some(prev) = previous {
            width += scaled.kern(prev, id);
        }
        width += scaled.h_advance(id) + tracking;
        previous = Some(id);
    }
    (width - tracking).max(0.0)
}

#[allow(
    clippy::as_conversions,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::suboptimal_flops
)]
fn draw_title(canvas: &mut RgbImage, title: &str) {
    let title = title.trim();
    if title.is_empty() {
        return;
    }
    let Ok(font) = FontRef::try_from_slice(TITLE_FONT) else {
        return;
    };

    let canvas_width = canvas.width() as f32;
    let canvas_height = canvas.height() as f32;

    let mut size = canvas_width * TITLE_SCALE;
    let max_text = canvas_width * PLATE_MAX_WIDTH - 2.0 * size * PLATE_PADDING_X;
    for _ in 0..48 {
        let width = title_width(&font, title, PxScale::from(size));
        if width <= max_text.max(1.0) || size <= 8.0 {
            break;
        }
        size *= 0.94;
    }

    let scale = PxScale::from(size);
    let scaled = font.as_scaled(scale);
    let text_width = title_width(&font, title, scale);
    let ascent = scaled.ascent();
    let descent = scaled.descent();

    let pad_x = size * PLATE_PADDING_X;
    let pad_y = size * PLATE_PADDING_Y;
    let plate_width = text_width + 2.0 * pad_x;
    let plate_height = (ascent - descent) + 2.0 * pad_y;
    let plate_x = (canvas_width - plate_width) / 2.0;
    let plate_y = (canvas_height - plate_height) / 2.0;

    fill_rect(
        canvas,
        plate_x,
        plate_y,
        plate_width,
        plate_height,
        PLATE_COLOR,
    );
    stroke_rect(
        canvas,
        plate_x,
        plate_y,
        plate_width,
        plate_height,
        size * 0.05,
    );

    let mut pen_x = plate_x + pad_x;
    let baseline = plate_y + pad_y + ascent;
    let tracking = size * TITLE_TRACKING;
    let mut previous: Option<ab_glyph::GlyphId> = None;

    for c in title.chars() {
        let id = scaled.glyph_id(c);
        if let Some(prev) = previous {
            pen_x += scaled.kern(prev, id);
        }
        let glyph = id.with_scale_and_position(scale, ab_glyph::point(pen_x, baseline));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|x, y, coverage| {
                let px = bounds.min.x + x as f32;
                let py = bounds.min.y + y as f32;
                blend_pixel(canvas, px, py, TITLE_COLOR, coverage.clamp(0.0, 1.0));
            });
        }
        pen_x += scaled.h_advance(id) + tracking;
        previous = Some(id);
    }
}

#[allow(
    clippy::as_conversions,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::suboptimal_flops
)]
fn fill_rect(canvas: &mut RgbImage, x: f32, y: f32, width: f32, height: f32, color: Rgb<u8>) {
    let x0 = x.max(0.0).round() as u32;
    let y0 = y.max(0.0).round() as u32;
    let x1 = (x + width).max(0.0).round().min(canvas.width() as f32) as u32;
    let y1 = (y + height).max(0.0).round().min(canvas.height() as f32) as u32;
    for py in y0..y1 {
        for px in x0..x1 {
            if let Some(pixel) = canvas.get_pixel_mut_checked(px, py) {
                *pixel = color;
            }
        }
    }
}

fn stroke_rect(canvas: &mut RgbImage, x: f32, y: f32, width: f32, height: f32, thickness: f32) {
    let thickness = thickness.max(1.0);
    fill_rect(canvas, x, y, width, thickness, TITLE_COLOR);
    fill_rect(
        canvas,
        x,
        y + height - thickness,
        width,
        thickness,
        TITLE_COLOR,
    );
    fill_rect(canvas, x, y, thickness, height, TITLE_COLOR);
    fill_rect(
        canvas,
        x + width - thickness,
        y,
        thickness,
        height,
        TITLE_COLOR,
    );
}

#[allow(
    clippy::as_conversions,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::suboptimal_flops
)]
fn blend_pixel(canvas: &mut RgbImage, x: f32, y: f32, color: Rgb<u8>, coverage: f32) {
    if x < 0.0 || y < 0.0 {
        return;
    }
    let Some(pixel) = canvas.get_pixel_mut_checked(x as u32, y as u32) else {
        return;
    };
    for (channel, target) in pixel.0.iter_mut().zip(color.0) {
        let blended = f32::from(*channel) * (1.0 - coverage) + f32::from(target) * coverage;
        *channel = blended.clamp(0.0, 255.0) as u8;
    }
}

#[must_use]
pub fn cover_image(art_paths: &[PathBuf], title: &str) -> Option<Vec<u8>> {
    let mut canvas = collage(art_paths)
        .unwrap_or_else(|| RgbImage::from_pixel(COVER_WIDTH, COVER_HEIGHT, PLATE_COLOR));
    draw_title(&mut canvas, title);
    encode_jpeg(&canvas)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_within_leaves_small_images_alone() {
        assert_eq!(fit_within(300, 200, 600), (300, 200));
        assert_eq!(fit_within(600, 600, 600), (600, 600));
    }

    #[test]
    fn fit_within_preserves_aspect_ratio() {
        assert_eq!(fit_within(1500, 1500, 600), (600, 600));
        assert_eq!(fit_within(1200, 600, 600), (600, 300));
        assert_eq!(fit_within(600, 1200, 600), (300, 600));
    }

    #[test]
    fn fit_within_never_returns_a_zero_edge() {
        let (_, height) = fit_within(10_000, 1, 600);
        assert!(height >= 1);
    }

    #[test]
    fn scale_does_not_overflow_on_large_dimensions() {
        assert_eq!(scale(100_000, 100_000, 100_000), 100_000);
        assert_eq!(scale(10, 0, 5), 0);
        assert_eq!(scale(10, 5, 0), 0);
    }

    #[test]
    fn pick_evenly_returns_everything_when_it_fits() {
        assert_eq!(pick_evenly(&[1, 2, 3], 5), vec![1, 2, 3]);
        assert_eq!(pick_evenly(&[1, 2, 3], 3), vec![1, 2, 3]);
    }

    #[test]
    fn pick_evenly_spreads_across_the_whole_list() {
        let items: Vec<u32> = (0..100).collect();
        let picked = pick_evenly(&items, 4);
        assert_eq!(picked.len(), 4);
        assert_eq!(picked, vec![12, 37, 62, 87]);
    }

    #[test]
    fn pick_evenly_handles_empty_input() {
        assert!(pick_evenly::<u32>(&[], 4).is_empty());
        assert!(pick_evenly(&[1, 2, 3], 0).is_empty());
    }

    #[test]
    fn fill_cell_produces_exactly_the_requested_size() {
        let wide = RgbImage::new(400, 100);
        let filled = fill_cell(&wide, 200, 200);
        assert_eq!((filled.width(), filled.height()), (200, 200));

        let tall = RgbImage::new(100, 400);
        let filled = fill_cell(&tall, 200, 200);
        assert_eq!((filled.width(), filled.height()), (200, 200));
    }

    #[test]
    fn collage_of_nothing_is_none() {
        assert!(collage(&[]).is_none());
    }

    #[test]
    fn a_cover_is_produced_even_with_no_album_art() {
        assert!(cover_image(&[], "Lyrics").is_some());
    }

    #[test]
    fn the_title_is_drawn_into_the_image() {
        let mut canvas = RgbImage::from_pixel(400, 600, Rgb([0, 0, 0]));
        draw_title(&mut canvas, "Lyrics");
        assert!(
            canvas.pixels().any(|p| p.0[0] > 200),
            "no light pixels: the title was never rasterized"
        );
    }

    #[test]
    fn an_empty_title_draws_nothing() {
        let mut canvas = RgbImage::from_pixel(400, 600, Rgb([0, 0, 0]));
        draw_title(&mut canvas, "   ");
        assert!(canvas.pixels().all(|p| p.0 == [0, 0, 0]));
    }

    #[test]
    #[allow(clippy::as_conversions, clippy::cast_precision_loss)]
    fn a_long_title_is_shrunk_to_fit_the_cover() {
        let font = FontRef::try_from_slice(TITLE_FONT).expect("bundled font parses");
        let long = "A Really Very Long Book Title That Would Otherwise Overflow";
        let nominal = PxScale::from(COVER_WIDTH as f32 * TITLE_SCALE);
        let max_text = COVER_WIDTH as f32 * PLATE_MAX_WIDTH;
        assert!(title_width(&font, long, nominal) > max_text);

        let mut canvas = RgbImage::from_pixel(COVER_WIDTH, COVER_HEIGHT, Rgb([0, 0, 0]));
        draw_title(&mut canvas, long);
        for y in 0..COVER_HEIGHT {
            for x in [0_u32, COVER_WIDTH - 1] {
                assert_eq!(
                    canvas.get_pixel(x, y).0,
                    [0, 0, 0],
                    "title overflowed at {x},{y}"
                );
            }
        }
    }

    #[test]
    fn the_grid_never_asks_for_more_images_than_it_has() {
        for count in 1..=200_usize {
            let (columns, rows) = grid_for(count);
            let cells = usize::try_from(columns.saturating_mul(rows)).unwrap_or(0);
            assert!(
                cells <= count,
                "grid for {count} images wants {cells} cells"
            );
            assert!(columns >= 1 && rows >= 1, "empty grid for {count}");
        }
    }

    #[test]
    fn the_grid_grows_with_the_library() {
        assert_eq!(grid_for(8), (2, 4));
        assert_eq!(grid_for(24), (4, 6));
        assert_eq!(grid_for(1_000), (8, 12));
    }

    #[test]
    fn a_single_album_still_yields_a_grid() {
        assert_eq!(grid_for(1), (1, 1));
        assert_eq!(grid_for(0), (0, 0));
    }
}
