//! Selection glyphs use the same outlines and line positions as document text.
//! Cache one viewport-clipped raster; caret blinking never rebuilds it.
use super::*;
use resvg::tiny_skia::{Pixmap, Transform};
use std::fmt::Write;

#[derive(Default)]
pub(super) struct TextSelectionOverlay {
    cached: Option<(SelectionKey, Arc<RenderImage>)>,
}

#[derive(PartialEq)]
struct SelectionKey {
    svg: String,
    extent: [u32; 4],
    scale: u32,
}

impl TextSelectionOverlay {
    pub(super) fn clear(&mut self, window: &mut Window) {
        if let Some((_, image)) = self.cached.take() {
            let _ = window.drop_image(image);
        }
    }

    fn paint(
        &mut self,
        key: Option<(SelectionKey, Bounds<Pixels>)>,
        metrics: &VectorCache,
        bounds: Bounds<Pixels>,
        window: &mut Window,
    ) -> Result<(), CanvasError> {
        let Some((key, image_bounds)) = key else {
            self.clear(window);
            return Ok(());
        };
        if !self.cached.as_ref().is_some_and(|cached| cached.0 == key) {
            let pixels = rasterize(&key, metrics)?;
            let image = Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]));
            self.clear(window);
            self.cached = Some((key, image));
        }
        let (_, image) = self.cached.as_ref().unwrap();
        window
            .paint_image(
                bounds,
                image_bounds,
                Corners::all(px(0.0)),
                image.clone(),
                0,
                false,
            )
            .map_err(|error| CanvasError::InvalidFile(error.to_string()))
    }
}

pub(super) fn paint_selected_text(
    this: &mut CanvasView,
    rectangles: &[(f64, f64, f64, f64)],
    window: &mut Window,
) -> Result<(), CanvasError> {
    let key = this.draft.as_ref().and_then(|draft| {
        selection_key(
            draft,
            rectangles,
            this.session.scene().frame(),
            this.bounds,
            window.scale_factor(),
        )
    });
    this.text_selection_overlay
        .paint(key, this.metrics.as_ref().unwrap(), this.bounds, window)
}

pub(super) fn caret_path(this: &mut CanvasView) -> Option<(gpui::Path<Pixels>, draw::ColorRgba8)> {
    let draft = this.draft.as_ref()?;
    let caret = draft.buffer.caret;
    let height = draft.layout.line_height;
    let color = editing_color(draft.style.color);
    let (x, y) = this.text_local_position(caret)?;
    let path = caret_stroke(
        this.text_world_position(x, y)?,
        this.text_world_position(x, y + height)?,
        this.session.scene().frame().camera.zoom,
        this.bounds,
    )?;
    Some((path, color))
}

fn caret_stroke(
    start: Point<Pixels>,
    end: Point<Pixels>,
    zoom: f64,
    bounds: Bounds<Pixels>,
) -> Option<gpui::Path<Pixels>> {
    let stroke = resvg::tiny_skia::Stroke {
        width: (1.2 * zoom).max(1.0) as f32,
        line_cap: resvg::tiny_skia::LineCap::Butt,
        ..Default::default()
    };
    overlay_geometry::stroke_line(start, end, bounds, &stroke, None)
}

pub(super) fn editing_color(color: draw::ColorRgba8) -> draw::ColorRgba8 {
    if color.a == 0 {
        draw::ColorRgba8 {
            r: 30,
            g: 30,
            b: 30,
            a: 255,
        }
    } else {
        color
    }
}

fn selection_key(
    draft: &TextDraft,
    rectangles: &[(f64, f64, f64, f64)],
    frame: draw::FrameView,
    bounds: Bounds<Pixels>,
    scale: f32,
) -> Option<(SelectionKey, Bounds<Pixels>)> {
    let selection = draft.buffer.selection();
    if selection.is_empty() || draft.buffer.marked.is_some() || rectangles.is_empty() {
        return None;
    }
    let zoom = frame.camera.zoom;
    let center = draw::canvas_to_view(draft.center, &frame.camera, frame.surface);
    let (sin, cos) = draft.rotation.sin_cos();
    let to_view = |x: f64, y: f64| {
        (
            center.x + (x * cos - y * sin) * zoom,
            center.y + (x * sin + y * cos) * zoom,
        )
    };
    let mut corners = rectangles
        .iter()
        .flat_map(|&(left, top, right, bottom)| {
            [(left, top), (right, top), (right, bottom), (left, bottom)]
        })
        .map(|(x, y)| to_view(x, y));
    let first = corners.next()?;
    let mut extent = [first.0, first.1, first.0, first.1];
    for (x, y) in corners {
        extent[0] = extent[0].min(x);
        extent[1] = extent[1].min(y);
        extent[2] = extent[2].max(x);
        extent[3] = extent[3].max(y);
    }
    let scale64 = f64::from(scale);
    let left = (extent[0] * scale64).floor().max(0.0);
    let top = (extent[1] * scale64).floor().max(0.0);
    let right = (extent[2] * scale64)
        .ceil()
        .min((f64::from(frame.surface.width) * scale64).ceil());
    let bottom = (extent[3] * scale64)
        .ceil()
        .min((f64::from(frame.surface.height) * scale64).ceil());
    if ![left, top, right, bottom].iter().all(|n| n.is_finite()) || right <= left || bottom <= top {
        return None;
    }
    let extent = [
        left as u32,
        top as u32,
        (right - left) as u32,
        (bottom - top) as u32,
    ];
    let mut svg = String::from("<defs><clipPath id=\"text-selection\">");
    for &(left, top, right, bottom) in rectangles {
        write!(
            svg,
            "<rect x=\"{left}\" y=\"{top}\" width=\"{}\" height=\"{}\"/>",
            right - left,
            bottom - top
        )
        .unwrap();
    }
    write!(svg, "</clipPath></defs><g transform=\"translate({} {}) scale({zoom}) rotate({})\" opacity=\"{}\"><g clip-path=\"url(#text-selection)\">", center.x, center.y, draft.rotation.to_degrees(), draft.style.opacity).unwrap();
    let mut style = draft.style.clone();
    style.color = draw::ColorRgba8 {
        r: 255,
        g: 255,
        b: 255,
        a: style.color.a,
    };
    style.stroke = draw::ColorRgba8 {
        r: 255,
        g: 255,
        b: 255,
        a: style.stroke.a,
    };
    let width = if draft.auto_resize {
        draft.layout.width.max(1.0)
    } else {
        draft.width
    };
    for (index, line) in draft.layout.lines.iter().enumerate() {
        // Shape the entire intersecting line, retaining ligatures and bidi context.
        if selection.start >= line.start + line.text.len() || selection.end <= line.start {
            continue;
        }
        let left = match style.horizontal_align {
            draw::TextHorizontalAlign::Left => -width / 2.0,
            draw::TextHorizontalAlign::Center => -line.width / 2.0,
            draw::TextHorizontalAlign::Right => width / 2.0 - line.width,
        };
        let baseline =
            -draft.layout.height / 2.0 + index as f64 * draft.layout.line_height + style.font_size;
        svg.push_str(&crate::vector::text_span(
            &line.text, &style, left, baseline,
        ));
    }
    svg.push_str("</g></g>");
    let origin = bounds.origin + point(px(left as f32 / scale), px(top as f32 / scale));
    let image_bounds = Bounds::new(
        origin,
        gpui::size(px(extent[2] as f32 / scale), px(extent[3] as f32 / scale)),
    );
    Some((
        SelectionKey {
            svg,
            extent,
            scale: scale.to_bits(),
        },
        image_bounds,
    ))
}

fn rasterize(key: &SelectionKey, metrics: &VectorCache) -> Result<image::RgbaImage, CanvasError> {
    let [left, top, width, height] = key.extent;
    crate::document::validate_surface(width, height)?;
    let tree = metrics.parse(&key.svg)?;
    let scale = f32::from_bits(key.scale);
    let transform = Transform::from_row(scale, 0.0, 0.0, scale, -(left as f32), -(top as f32));
    let mut pixels = Pixmap::new(width, height).ok_or(CanvasError::ResourceLimit)?;
    resvg::render(&tree, transform, &mut pixels.as_mut());
    let mut bgra = image::RgbaImage::new(width, height);
    for (destination, source) in bgra.pixels_mut().zip(pixels.pixels()) {
        let source = source.demultiply();
        *destination = image::Rgba([source.blue(), source.green(), source.red(), source.alpha()]);
    }
    Ok(bgra)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(metrics: &mut VectorCache, text: &str) -> TextDraft {
        let style = draw::TextStyle {
            font_size: 28.0,
            color: draw::ColorRgba8 {
                r: 200,
                g: 20,
                b: 40,
                a: 180,
            },
            opacity: 0.6,
            bold: false,
            italic: false,
            font_family: None,
            fill: draw::ColorRgba8 {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
            fill_style: draw::FillStyle::Solid,
            stroke: draw::ColorRgba8 {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
            stroke_width: 0.0,
            corner_radii: Default::default(),
            horizontal_align: draw::TextHorizontalAlign::Left,
            vertical_align: draw::TextVerticalAlign::Center,
        };
        let layout = metrics.layout(text, &style, 1.0e8);
        let mut buffer = TextBuffer::new(text.into());
        buffer.anchor = 0;
        TextDraft {
            buffer,
            target: draw::TextCommitTarget::New,
            center: draw::Point::new(0.0, 0.0),
            rotation: 0.0,
            style,
            width: layout.width,
            auto_resize: true,
            layout,
            changed: false,
            minimum_grapheme_width: 1.0,
            ink_reserve: 0.0,
            preferred_x: None,
            caret_affinity: None,
        }
    }

    fn frame(zoom: f64) -> draw::FrameView {
        draw::FrameView {
            camera: draw::Camera {
                center: draw::Point::new(0.0, 0.0),
                zoom,
            },
            surface: draw::SurfaceSize {
                width: 640,
                height: 480,
            },
            ..Default::default()
        }
    }

    fn bounds() -> Bounds<Pixels> {
        Bounds::new(point(px(10.0), px(20.0)), gpui::size(px(640.0), px(480.0)))
    }

    #[test]
    fn text_selection_white_glyphs_keep_shared_shaping_and_opacity() {
        let mut metrics = VectorCache::default();
        let mut draft = draft(&mut metrics, "office text");
        draft.buffer.caret = 2;
        let line = &draft.layout.lines[0];
        let left = -draft.layout.width / 2.0;
        let top = -draft.layout.height / 2.0;
        let rectangles: Vec<_> = line
            .selection_segments(0..2)
            .into_iter()
            .map(|(a, b)| (left + a, top, left + b, top + draft.layout.line_height))
            .collect();
        let (key, _) = selection_key(&draft, &rectangles, frame(1.0), bounds(), 1.5).unwrap();
        let moved_bounds = Bounds {
            origin: bounds().origin + point(px(30.0), px(40.0)),
            ..bounds()
        };
        let (moved_key, moved_image_bounds) =
            selection_key(&draft, &rectangles, frame(1.0), moved_bounds, 1.5).unwrap();
        let (_, image_bounds) =
            selection_key(&draft, &rectangles, frame(1.0), bounds(), 1.5).unwrap();
        assert!(key == moved_key);
        let movement = moved_image_bounds.origin - image_bounds.origin;
        assert!((f32::from(movement.x) - 30.0).abs() < 1.0e-4);
        assert!((f32::from(movement.y) - 40.0).abs() < 1.0e-4);
        assert!(
            key.svg.contains(">office text</text>"),
            "partial ligature selection must keep the complete shaping context"
        );
        let image = rasterize(&key, &metrics).unwrap();
        let ink: Vec<_> = image.pixels().filter(|p| p[3] > 0).collect();
        assert!(!ink.is_empty());
        assert!(
            ink.iter()
                .all(|p| p[0] == 255 && p[1] == 255 && p[2] == 255)
        );
        let maximum_alpha = ink.iter().map(|p| p[3]).max().unwrap();
        assert!(
            (105..=109).contains(&maximum_alpha),
            "text alpha and element opacity must both apply: {maximum_alpha}"
        );
        draft.buffer.marked = Some(0..2);
        assert!(selection_key(&draft, &rectangles, frame(1.0), bounds(), 1.5).is_none());
        draft.buffer.marked = None;
        draft.buffer.anchor = draft.buffer.caret;
        assert!(selection_key(&draft, &rectangles, frame(1.0), bounds(), 1.5).is_none());
    }

    #[test]
    fn text_selection_clips_bidi_multiline_rotation_and_dpi_to_viewport() {
        let mut metrics = VectorCache::default();
        let mut draft = draft(&mut metrics, "Latin אבג 😀\nsecond line");
        let mut rectangles = Vec::new();
        for (index, line) in draft.layout.lines.iter().enumerate() {
            let left = -draft.layout.width / 2.0;
            let top = -draft.layout.height / 2.0 + index as f64 * draft.layout.line_height;
            for (a, b) in line.selection_segments(0..line.text.len()) {
                rectangles.push((left + a, top, left + b, top + draft.layout.line_height));
            }
        }
        draft.rotation = 0.4;
        let (first, _) = selection_key(&draft, &rectangles, frame(1.0), bounds(), 1.0).unwrap();
        let first_pixels = rasterize(&first, &metrics).unwrap();
        assert!(first_pixels.pixels().any(|p| p[3] > 0));
        let (dpi, _) = selection_key(&draft, &rectangles, frame(1.0), bounds(), 2.0).unwrap();
        assert!(first != dpi);
        assert!(dpi.extent[2] >= first.extent[2] * 2 - 1);
        let (zoom, _) = selection_key(&draft, &rectangles, frame(0.5), bounds(), 1.0).unwrap();
        assert!(first != zoom);
        for alignment in [
            draw::TextHorizontalAlign::Center,
            draw::TextHorizontalAlign::Right,
        ] {
            draft.style.horizontal_align = alignment;
            let (aligned, _) =
                selection_key(&draft, &rectangles, frame(1.0), bounds(), 1.0).unwrap();
            assert!(first != aligned);
        }
        draft.center = draw::Point::new(-320.0, -240.0);
        let (clipped, _) = selection_key(&draft, &rectangles, frame(1.0), bounds(), 2.0).unwrap();
        assert_eq!(clipped.extent[0], 0);
        assert_eq!(clipped.extent[1], 0);
        assert!(clipped.extent[2] <= 1280 && clipped.extent[3] <= 960);
        let clipped_pixels = rasterize(&clipped, &metrics).unwrap();
        assert!(clipped_pixels.pixels().any(|p| p[3] > 0));
        draft.center = draw::Point::new(-10_000.0, -10_000.0);
        assert!(selection_key(&draft, &rectangles, frame(1.0), bounds(), 2.0).is_none());
    }

    #[test]
    fn text_caret_geometry_has_centered_flat_ends_and_zoom_minimum() {
        for (zoom, width) in [(0.25, 1.0_f32), (1.0, 1.2), (2.0, 2.4)] {
            for angle in [0.0_f32, 0.4, 1.5] {
                let start = point(px(100.0), px(100.0));
                let (sin, cos) = angle.sin_cos();
                let end = start + point(px(-sin * 30.0), px(cos * 30.0));
                let path = caret_stroke(start, end, zoom, bounds()).unwrap();
                let mut across = (f32::INFINITY, f32::NEG_INFINITY);
                let mut along = (f32::INFINITY, f32::NEG_INFINITY);
                for vertex in &path.vertices {
                    let delta = vertex.xy_position - start;
                    let x = f32::from(delta.x);
                    let y = f32::from(delta.y);
                    let a = x * cos + y * sin;
                    let b = -x * sin + y * cos;
                    across.0 = across.0.min(a);
                    across.1 = across.1.max(a);
                    along.0 = along.0.min(b);
                    along.1 = along.1.max(b);
                }
                assert!((across.0 + width / 2.0).abs() < 0.001);
                assert!((across.1 - width / 2.0).abs() < 0.001);
                assert!(along.0.abs() < 0.001);
                assert!((along.1 - 30.0).abs() < 0.001);
            }
        }
        assert_eq!(
            editing_color(draw::ColorRgba8 {
                r: 200,
                g: 0,
                b: 0,
                a: 0
            }),
            draw::ColorRgba8 {
                r: 30,
                g: 30,
                b: 30,
                a: 255
            }
        );
    }
}
