//! Strip/tile export keeps working rasters independent of total output size.
use crate::{
    CanvasCancellation, CanvasDocument, CanvasError, CanvasRect, CanvasRenderer, CanvasSession,
    CanvasSnapshot, ExportTile, document::validate_raster, engine::*,
};
use image::RgbaImage;
use std::io::Write;
#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;

fn error(error: impl std::fmt::Display) -> CanvasError {
    CanvasError::InvalidFile(error.to_string())
}

#[derive(Default)]
pub(crate) struct BoundedOutput {
    pub bytes: Vec<u8>,
}
impl Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > 96 * 1024 * 1024 {
            return Err(std::io::Error::other(
                "Encoded output exceeds96MiB; use the streaming writer API",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(crate) fn dimensions(snapshot: &CanvasSnapshot, scale: f32) -> Result<[u32; 2], CanvasError> {
    scaled_dimensions(snapshot.viewport.surface, scale)
}
pub(crate) fn scaled_dimensions(surface: SurfaceSize, scale: f32) -> Result<[u32; 2], CanvasError> {
    if !scale.is_finite() || !(0.1..=8.0).contains(&scale) {
        return Err(ErrorCode::InvalidArgument.into());
    }
    let result = [
        (f64::from(surface.width) * f64::from(scale)).ceil() as u32,
        (f64::from(surface.height) * f64::from(scale)).ceil() as u32,
    ];
    validate_raster(result[0], result[1])?;
    // An individual PNG scanline is bounded to8MiB even for very wide captures.
    if result[0] > 2 * 1024 * 1024 {
        return Err(CanvasError::ResourceLimit);
    }
    Ok(result)
}

impl CanvasRenderer {
    /// Streams a lossless composited SVG image through a base64 encoder. This
    /// large-image path preserves effect pixels rather than editable vectors.
    /// Ordinary small documents retain vectors through `export_snapshot_svg`.
    pub fn export_snapshot_svg_to_writer(
        &mut self,
        snapshot: &CanvasSnapshot,
        scale: f32,
        mut writer: impl Write,
        cancel: &CanvasCancellation,
    ) -> Result<(), CanvasError> {
        let [width, height] = dimensions(snapshot, scale)?;
        cancel.check()?;
        write!(writer, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}"><image width="100%" height="100%" href="data:image/png;base64,"#).map_err(error)?;
        let mut encoded = base64::write::EncoderWriter::new(
            &mut writer,
            &base64::engine::general_purpose::STANDARD,
        );
        self.export_snapshot_png_to_writer(snapshot, scale, &mut encoded, cancel)?;
        encoded.finish().map_err(error)?;
        drop(encoded);
        writer.write_all(b"\"/></svg>").map_err(error)?;
        cancel.check()
    }
    /// Freezes a complete region at a requested output size without creating an
    /// output-sized engine viewport or raster. Uniform scaling preserves geometry.
    pub fn snapshot_for_export(
        &mut self,
        document: &mut CanvasDocument,
        session: &mut CanvasSession,
        region: CanvasRect,
        output_size: [u32; 2],
    ) -> Result<CanvasSnapshot, CanvasError> {
        region.validate()?;
        validate_raster(output_size[0], output_size[1])?;
        if region.width <= 0.0 || region.height <= 0.0 {
            return Err(ErrorCode::InvalidArgument.into());
        }
        let zoom = f64::from(output_size[0]) / region.width;
        if !zoom.is_finite() || (region.height * zoom - f64::from(output_size[1])).abs() > 1.0 {
            return Err(ErrorCode::InvalidArgument.into());
        }
        self.prepare(document, session)?;
        let mut reference = CanvasSession::new(document)?;
        let prepared = (|| {
            // This grid only queries retained geometry. Reduce its density when
            // ordered effect halos exceed the working raster budget; export
            // pixels are still composed independently at their requested scale.
            let mut grid = (3500.0 / region.width).min(3500.0 / region.height);
            loop {
                match crate::snapshot::prepare_fixed_reference(
                    document,
                    &mut reference,
                    crate::CanvasRenderReference {
                        canvas_rect: region,
                        pixels_per_canvas_unit: grid,
                    },
                ) {
                    Ok(()) => break,
                    Err(CanvasError::ResourceLimit) if grid > 1.0e-6 => grid /= 2.0,
                    Err(error) => return Err(error),
                }
            }
            self.prepare(document, &mut reference)?;
            let frame = FrameView {
                camera: Camera {
                    center: Point::new(
                        region.x + f64::from(output_size[0]) / (2.0 * zoom),
                        region.y + f64::from(output_size[1]) / (2.0 * zoom),
                    ),
                    zoom,
                },
                surface: SurfaceSize {
                    width: output_size[0],
                    height: output_size[1],
                },
                ..session.scene().frame()
            };
            Ok(CanvasSnapshot::from_reference(document, &reference, frame))
        })();
        reference.close(document)?;
        prepared
    }

    /// Writes PNG scanlines incrementally. No full output RGBA/PNG buffer is
    /// allocated; source images remain immutable host-owned buffers.
    pub fn export_snapshot_png_to_writer(
        &mut self,
        snapshot: &CanvasSnapshot,
        scale: f32,
        writer: impl Write,
        cancel: &CanvasCancellation,
    ) -> Result<(), CanvasError> {
        let [width, height] = dimensions(snapshot, scale)?;
        let mut encoder = png::Encoder::new(writer, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut png = encoder.write_header().map_err(error)?;
        let mut stream = png.stream_writer_with_size(64 * 1024).map_err(error)?;
        self.write_bands(snapshot, scale, [width, height], None, &mut stream, cancel)?;
        stream.finish().map_err(error)?;
        png.finish().map_err(error)?;
        Ok(())
    }

    pub fn export_snapshot_tiles_to_writer(
        &mut self,
        snapshot: &CanvasSnapshot,
        scale: f32,
        output_size: [u32; 2],
        tiles: &[ExportTile],
        writer: impl Write,
        cancel: &CanvasCancellation,
    ) -> Result<(), CanvasError> {
        let source = dimensions(snapshot, scale)?;
        validate_raster(output_size[0], output_size[1])?;
        if output_size[0] > 2 * 1024 * 1024 || tiles.len() > 16384 {
            return Err(CanvasError::ResourceLimit);
        }
        for tile in tiles {
            let [x, y, w, h] = tile.source;
            let [dx, dy] = tile.destination;
            if w == 0
                || h == 0
                || x.checked_add(w).is_none_or(|v| v > source[0])
                || y.checked_add(h).is_none_or(|v| v > source[1])
                || dx.checked_add(w).is_none_or(|v| v > output_size[0])
                || dy.checked_add(h).is_none_or(|v| v > output_size[1])
            {
                return Err(ErrorCode::InvalidArgument.into());
            }
        }
        let mut encoder = png::Encoder::new(writer, output_size[0], output_size[1]);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut png = encoder.write_header().map_err(error)?;
        let mut stream = png.stream_writer_with_size(64 * 1024).map_err(error)?;
        self.write_bands(
            snapshot,
            scale,
            output_size,
            Some(tiles),
            &mut stream,
            cancel,
        )?;
        stream.finish().map_err(error)?;
        png.finish().map_err(error)?;
        Ok(())
    }

    fn write_bands(
        &mut self,
        snapshot: &CanvasSnapshot,
        scale: f32,
        size: [u32; 2],
        tiles: Option<&[ExportTile]>,
        writer: &mut impl Write,
        cancel: &CanvasCancellation,
    ) -> Result<(), CanvasError> {
        let [width, height] = size;
        let rows = (8 * 1024 * 1024 / u64::from(width) / 4).clamp(1, 128) as u32;
        let mut y = 0;
        while y < height {
            cancel.check()?;
            let band_height = rows.min(height - y);
            let mut band = RgbaImage::new(width, band_height);
            if let Some(tiles) = tiles {
                for tile in tiles {
                    let top = y.max(tile.destination[1]);
                    let bottom = (y + band_height).min(tile.destination[1] + tile.source[3]);
                    if top >= bottom {
                        continue;
                    }
                    let mut dx = 0;
                    while dx < tile.source[2] {
                        let w = 1024.min(tile.source[2] - dx);
                        let region = self.export_region(
                            snapshot,
                            scale,
                            [
                                tile.source[0] + dx,
                                tile.source[1] + top - tile.destination[1],
                                w,
                                bottom - top,
                            ],
                            cancel,
                        )?;
                        image::imageops::replace(
                            &mut band,
                            &region,
                            i64::from(tile.destination[0] + dx),
                            i64::from(top - y),
                        );
                        dx += w;
                    }
                }
            } else {
                let mut x = 0;
                while x < width {
                    let w = 1024.min(width - x);
                    let region =
                        self.export_region(snapshot, scale, [x, y, w, band_height], cancel)?;
                    image::imageops::replace(&mut band, &region, i64::from(x), 0);
                    x += w;
                }
            }
            writer.write_all(band.as_raw()).map_err(error)?;
            y += band_height;
        }
        cancel.check()?;
        Ok(())
    }

    fn export_region(
        &mut self,
        snapshot: &CanvasSnapshot,
        scale: f32,
        rect: [u32; 4],
        cancel: &CanvasCancellation,
    ) -> Result<RgbaImage, CanvasError> {
        let [x, y, width, height] = rect;
        let frame = snapshot.viewport;
        let zoom = frame.camera.zoom * f64::from(scale);
        let mut pad = 2_u32;
        let top_left = view_to_canvas(
            Point::new(
                f64::from(x) / f64::from(scale),
                f64::from(y) / f64::from(scale),
            ),
            &frame.camera,
            frame.surface,
        );
        let mut tile_frame = FrameView {
            camera: Camera {
                center: Point::new(
                    top_left.x + f64::from(width) / (2.0 * zoom),
                    top_left.y + f64::from(height) / (2.0 * zoom),
                ),
                zoom,
            },
            surface: SurfaceSize {
                width: width + pad * 2,
                height: height + pad * 2,
            },
            ..frame
        };
        let mut scene = None;
        for _ in 0..16 {
            cancel.check()?;
            tile_frame.surface = SurfaceSize {
                width: width + pad * 2,
                height: height + pad * 2,
            };
            crate::document::validate_surface(tile_frame.surface.width, tile_frame.surface.height)?;
            let bounds = canvas_viewport(tile_frame.camera, tile_frame.surface);
            let candidate = snapshot
                .scene
                .culled(tile_frame, |item| intersects(item_bounds(item), bounds));
            let support = (crate::snapshot::sampling_support(&candidate) * zoom).ceil();
            if !support.is_finite() || support > 8192.0 {
                return Err(CanvasError::ResourceLimit);
            }
            let required = support as u32 + 2;
            if required <= pad {
                scene = Some(candidate);
                break;
            }
            pad = required;
        }
        let scene = scene.ok_or(CanvasError::ResourceLimit)?;
        let image = self.render_presented_cancellable(
            &scene,
            snapshot.background.as_ref(),
            snapshot.resources,
            1.0,
            cancel,
            true,
            &snapshot.presentation,
        )?;
        Ok(image::imageops::crop_imm(image.as_ref(), pad, pad, width, height).to_image())
    }
}

type Bounds = (f64, f64, f64, f64);
fn intersects(a: Bounds, b: Bounds) -> bool {
    a.0 <= b.2 && a.2 >= b.0 && a.1 <= b.3 && a.3 >= b.1
}
fn rectangle(x: f64, y: f64, w: f64, h: f64, r: f64, pad: f64) -> Bounds {
    let (s, c) = r.sin_cos();
    let dx = c.abs() * w / 2.0 + s.abs() * h / 2.0 + pad;
    let dy = s.abs() * w / 2.0 + c.abs() * h / 2.0 + pad;
    (x - dx, y - dy, x + dx, y + dy)
}
fn union(a: Bounds, b: Bounds) -> Bounds {
    (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))
}
fn arrow(item: &ArrowDisplayItem) -> Bounds {
    let p = item.stroke_width * 4.0 + 8.0;
    let mut b = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    if !item.geometry.chunks.is_empty() {
        let [a, c, d, e] = item.geometry.canvas_bounds;
        b = (a - p, c - p, d + p, e + p);
    }
    if let Some(pressure) = &item.pressure_geometry {
        let [a, c, d, e] = pressure.canvas_bounds;
        b = union(b, (a - p, c - p, d + p, e + p));
    }
    for [x, y] in item.points.iter().copied() {
        b = union(b, (x - p, y - p, x + p, y + p));
    }
    for command in &item.path_commands {
        match command {
            PathCommand::MoveTo { point } | PathCommand::LineTo { point } => {
                b = union(b, (point[0] - p, point[1] - p, point[0] + p, point[1] + p));
            }
            PathCommand::QuadTo { control, end } => {
                for point in [control, end] {
                    b = union(b, (point[0] - p, point[1] - p, point[0] + p, point[1] + p));
                }
            }
            PathCommand::CubicTo {
                control_1,
                control_2,
                end,
            } => {
                for point in [control_1, control_2, end] {
                    b = union(b, (point[0] - p, point[1] - p, point[0] + p, point[1] + p));
                }
            }
        }
    }
    for head in &item.arrowhead_primitives {
        for [x, y] in &head.points {
            b = union(b, (*x - p, *y - p, *x + p, *y + p));
        }
        b = union(
            b,
            rectangle(
                head.center[0],
                head.center[1],
                head.diameter,
                head.diameter,
                0.0,
                p,
            ),
        );
    }
    b
}
pub(crate) fn item_bounds(item: &SceneDisplayItem) -> Bounds {
    match item {
        SceneDisplayItem::Rectangle(r) => rectangle(
            r.center_x,
            r.center_y,
            r.width,
            r.height,
            r.rotation,
            r.stroke_width * 4.0
                + 8.0
                + snow_draw_engine_core::arrow::stroke_rendering_outset(
                    r.stroke_rendering_style,
                    r.width.min(r.height),
                ) * 2.0,
        ),
        SceneDisplayItem::Text(t) => rectangle(
            t.center_x,
            t.center_y,
            t.width,
            t.height,
            t.rotation,
            t.font_size * 2.0 + t.stroke_width * 4.0 + 8.0,
        ),
        SceneDisplayItem::Arrow(a) => arrow(a),
        SceneDisplayItem::SpeechBubble(b) => union(
            arrow(&b.path),
            rectangle(
                b.body.center_x,
                b.body.center_y,
                b.body.width,
                b.body.height,
                b.body.rotation,
                b.body.stroke_width * 4.0 + 8.0,
            ),
        ),
        SceneDisplayItem::Filter(f) => crate::renderer::filter_canvas_bounds(f),
        SceneDisplayItem::SerialNumber(s) => rectangle(
            s.center_x,
            s.center_y,
            s.diameter,
            s.diameter,
            s.rotation,
            s.font_size + s.stroke_width * 4.0 + 8.0,
        ),
        SceneDisplayItem::SerialNumberConnector(c) => {
            let p = c.stroke_width / 2.0 + 2.0;
            let mut bounds = (
                c.start_x.min(c.end_x) - p,
                c.start_y.min(c.end_y) - p,
                c.start_x.max(c.end_x) + p,
                c.start_y.max(c.end_y) + p,
            );
            if c.has_baseline {
                bounds = union(
                    bounds,
                    (
                        c.baseline_start_x.min(c.baseline_end_x) - p,
                        c.baseline_start_y.min(c.baseline_end_y) - p,
                        c.baseline_start_x.max(c.baseline_end_x) + p,
                        c.baseline_start_y.max(c.baseline_end_y) + p,
                    ),
                );
            }
            bounds
        }
        SceneDisplayItem::Magnifier(m) => {
            let bounds = union(
                rectangle(
                    m.lens.center_x,
                    m.lens.center_y,
                    m.lens.width,
                    m.lens.height,
                    m.lens.rotation,
                    m.lens.stroke_width * 4.0 + 8.0,
                ),
                rectangle(
                    m.source_center.x,
                    m.source_center.y,
                    m.source_width,
                    m.source_height,
                    m.lens.rotation,
                    m.lens.stroke_width * 4.0 + 8.0,
                ),
            );
            m.leader
                .as_ref()
                .map_or(bounds, |leader| union(bounds, arrow(leader)))
        }
        _ => (
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::INFINITY,
        ),
    }
}
