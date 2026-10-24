//! Backend-independent vector lowering. The same shaped outlines feed preview and export.
use std::{collections::HashMap, fmt::Write, sync::Arc};
#[cfg(test)]
#[path = "vector_tests.rs"]
mod tests;

use resvg::usvg;
use snow_draw_engine::*;
use unicode_segmentation::UnicodeSegmentation;

use crate::CanvasError;

pub(crate) fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub(crate) fn color(value: ColorRgba8) -> String {
    format!("#{:02x}{:02x}{:02x}", value.r, value.g, value.b)
}

pub(crate) fn wrap_svg(content: &str) -> String {
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1">{content}</svg>"#)
}

pub(crate) fn path_commands(commands: impl IntoIterator<Item = PathCommand>) -> String {
    let mut path = String::new();
    for command in commands {
        match command {
            PathCommand::MoveTo { point: [x, y] } => write!(path, "M{x} {y} "),
            PathCommand::LineTo { point: [x, y] } => write!(path, "L{x} {y} "),
            PathCommand::QuadTo {
                control: [x, y],
                end: [ex, ey],
            } => {
                write!(path, "Q{x} {y} {ex} {ey} ")
            }
            PathCommand::CubicTo {
                control_1: [x, y],
                control_2: [x2, y2],
                end: [ex, ey],
            } => {
                write!(path, "C{x} {y} {x2} {y2} {ex} {ey} ")
            }
        }
        .expect("writing to String");
    }
    path
}

pub(crate) fn rectangle_path(item: &RectangleDisplayItem) -> String {
    let w = item.width / 2.0;
    let h = item.height / 2.0;
    match item.shape {
        DisplayRectangleShape::Ellipse => {
            format!("M{} 0 A{w} {h} 0 1 0 {w} 0 A{w} {h} 0 1 0 {} 0 Z", -w, -w)
        }
        DisplayRectangleShape::Diamond => {
            let mut path = path_commands(
                snow_draw_engine_document::RoundedDiamond::new(
                    item.width,
                    item.height,
                    item.corner_radii,
                )
                .path_commands(Point::new(0.0, 0.0), 0.0),
            );
            path.push('Z');
            path
        }
        DisplayRectangleShape::Rectangle => {
            let limit = w.min(h).max(0.0);
            let r = item.corner_radii;
            let [tl, tr, br, bl] = [r.top_left, r.top_right, r.bottom_right, r.bottom_left]
                .map(|value| value.clamp(0.0, limit));
            format!(
                "M{} {} H{} Q{w} {} {w} {} V{} Q{w} {h} {} {h} H{} Q{} {h} {} {} V{} Q{} {} {} {} Z",
                -w + tl,
                -h,
                w - tr,
                -h,
                -h + tr,
                h - br,
                w - br,
                -w + bl,
                -w,
                -w,
                h - bl,
                -h + tl,
                -w,
                -h,
                -w + tl,
                -h,
            )
        }
    }
}

fn transform(item: &RectangleDisplayItem) -> String {
    format!(
        "translate({} {}) rotate({})",
        item.center_x,
        item.center_y,
        item.rotation.to_degrees()
    )
}

fn fill_attributes(fill: ColorRgba8, style: DisplayFillStyle) -> String {
    if !style.is_visible(fill) {
        return "fill=\"none\"".into();
    }
    let paint = match style {
        DisplayFillStyle::Line => "url(#hatch)".into(),
        DisplayFillStyle::CrossLine => "url(#cross)".into(),
        _ => color(fill),
    };
    format!(
        "fill=\"{paint}\" fill-opacity=\"{}\"",
        f64::from(fill.a) / 255.0
    )
}

fn patterns(fill: ColorRgba8, style: DisplayFillStyle) -> String {
    // Definitions are inert for solid/border fills but make every parsed tree
    // and its conservative cache accounting substantially larger.
    if !matches!(style, DisplayFillStyle::Line | DisplayFillStyle::CrossLine)
        || !style.is_visible(fill)
    {
        return String::new();
    }
    let c = color(fill);
    format!(
        r#"<defs><pattern id="hatch" width="8" height="8" patternUnits="userSpaceOnUse"><path d="M-2 2 L2 -2 M0 8 L8 0 M6 10 L10 6" stroke="{c}" stroke-width="1"/></pattern><pattern id="cross" width="8" height="8" patternUnits="userSpaceOnUse"><path d="M0 0 L8 8 M0 8 L8 0" stroke="{c}" stroke-width="1"/></pattern></defs>"#
    )
}

fn stroke_attributes(stroke: ColorRgba8, width: f64, style: StrokeStyle) -> String {
    let dash = match style {
        StrokeStyle::Solid => String::new(),
        StrokeStyle::Dashed => format!("stroke-dasharray=\"{} {}\"", width * 5.0, width * 3.0),
        StrokeStyle::Dotted => format!("stroke-dasharray=\"0 {}\"", width * 2.5),
    };
    format!(
        "stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"{width}\" stroke-linecap=\"round\" stroke-linejoin=\"round\" {dash}",
        color(stroke),
        f64::from(stroke.a) / 255.0,
    )
}

fn rectangle(item: &RectangleDisplayItem) -> String {
    let path = rectangle_path(item);
    let fill = fill_attributes(item.fill, item.fill_style);
    let stroke = stroke_attributes(item.stroke, item.stroke_width, item.stroke_style);
    format!(
        "{}<g transform=\"{}\" opacity=\"{}\">{}</g>",
        patterns(item.fill, item.fill_style),
        transform(item),
        item.opacity,
        styled_path(
            &path,
            &fill,
            &stroke,
            item.stroke_rendering_style,
            item.stroke_rendering_seed,
            item.width.min(item.height)
        )
    )
}

fn styled_path(
    path: &str,
    fill: &str,
    stroke: &str,
    style: StrokeRenderingStyle,
    seed: u32,
    scale: f64,
) -> String {
    if style == StrokeRenderingStyle::Architect {
        return format!("<path d=\"{path}\" {fill} {stroke}/>");
    }
    let displacement = snow_draw_engine_core::arrow::stroke_rendering_outset(style, scale) * 2.0;
    format!(
        r#"<defs><filter id="rough" x="-50%" y="-50%" width="200%" height="200%" color-interpolation-filters="sRGB"><feTurbulence type="fractalNoise" baseFrequency="0.045" numOctaves="2" seed="{seed}" result="noise"/><feDisplacementMap in="SourceGraphic" in2="noise" scale="{displacement}" xChannelSelector="R" yChannelSelector="G"/></filter></defs><path d="{path}" {fill} stroke="none"/><path d="{path}" fill="none" {stroke} filter="url(#rough)"/>"#
    )
}

fn arrow(item: &ArrowDisplayItem, background: ColorRgba8) -> String {
    let mut path = if item.geometry.chunks.is_empty() {
        path_commands(item.path_commands.iter().copied())
    } else {
        path_commands(
            item.geometry
                .chunks
                .iter()
                .flat_map(|chunk| chunk.commands.iter().copied()),
        )
    };
    if item.geometry.closed {
        path.push('Z');
    }
    let tapered = item.arrow_shaft_type == snow_draw_engine_core::arrow::ArrowShaftType::Tapered;
    let fill = if tapered {
        fill_attributes(item.stroke, DisplayFillStyle::Solid)
    } else {
        fill_attributes(item.fill, item.fill_style)
    };
    let stroke = stroke_attributes(
        item.stroke,
        if tapered { 0.0 } else { item.stroke_width },
        item.stroke_style,
    );
    let mut output = if tapered {
        String::new()
    } else {
        patterns(item.fill, item.fill_style)
    };
    write!(output, "<g opacity=\"{}\">", item.opacity).unwrap();
    let clipped =
        item.bound_text_id.is_some() && !item.bound_text_overlay && item.label_bounds.is_some();
    if clipped {
        let bounds = item.label_bounds.unwrap();
        let left = bounds.min_x - 5.0;
        let top = bounds.min_y - 5.0;
        let right = bounds.max_x + 5.0;
        let bottom = bounds.max_y + 5.0;
        write!(output,r#"<defs><clipPath id="label-gap"><path clip-rule="evenodd" d="M-100000000 -100000000 H100000000 V100000000 H-100000000 Z M{left} {top} H{right} V{bottom} H{left} Z"/></clipPath></defs><g clip-path="url(#label-gap)">"#).unwrap();
    }
    if let Some(pressure) = &item.pressure_geometry {
        let outline = path_commands(
            pressure
                .chunks
                .iter()
                .flat_map(|chunk| chunk.commands.iter().copied()),
        );
        write!(
            output,
            "<path d=\"{outline}\" {}/>",
            fill_attributes(item.stroke, DisplayFillStyle::Solid)
        )
        .unwrap();
    } else {
        output.push_str(&styled_path(
            &path,
            &fill,
            &stroke,
            item.stroke_rendering_style,
            item.stroke_rendering_seed,
            item.stroke_width * 10.0,
        ));
    }
    for head in &item.arrowhead_primitives {
        let fill_color = match head.fill_mode {
            ArrowheadDisplayFillMode::Stroke => item.stroke,
            ArrowheadDisplayFillMode::Background => background,
        };
        let fill = color(fill_color);
        let fill_alpha = f64::from(fill_color.a) / 255.0;
        let head_style = if head.dash_mode == ArrowheadDisplayDashMode::Inherit {
            item.stroke_style
        } else {
            StrokeStyle::Solid
        };
        let head_stroke = stroke_attributes(item.stroke, item.stroke_width, head_style);
        match head.kind {
            ArrowheadDisplayPrimitiveKind::Circle => {
                write!(
                    output,
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{fill}\" fill-opacity=\"{fill_alpha}\" {head_stroke}/>",
                    head.center[0],
                    head.center[1],
                    head.diameter / 2.0
                )
                .unwrap();
            }
            ArrowheadDisplayPrimitiveKind::Line | ArrowheadDisplayPrimitiveKind::Polygon => {
                let mut polygon = String::new();
                for (index, [x, y]) in head.points.iter().enumerate() {
                    write!(polygon, "{}{x} {y} ", if index == 0 { "M" } else { "L" }).unwrap();
                }
                let fill = if head.kind == ArrowheadDisplayPrimitiveKind::Polygon {
                    polygon.push('Z');
                    fill
                } else {
                    "none".into()
                };
                write!(
                    output,
                    "<path d=\"{polygon}\" fill=\"{fill}\" fill-opacity=\"{fill_alpha}\" {head_stroke}/>"
                )
                .unwrap();
            }
        }
    }
    if clipped {
        output.push_str("</g>");
    }
    output.push_str("</g>");
    output
}

#[derive(Clone)]
pub(crate) struct TextLine {
    pub text: String,
    pub width: f64,
    pub start: usize,
    pub carets: Vec<(usize, f64)>,
    pub clusters: Vec<(std::ops::Range<usize>, f64, f64)>,
    pub ink_reserve: f64,
}

impl TextLine {
    pub fn caret_x(&self, byte: usize) -> f64 {
        self.carets
            .iter()
            .find(|(index, _)| *index == byte)
            .map_or(self.width, |(_, x)| *x)
    }
    pub fn byte_at_x(&self, x: f64) -> usize {
        self.carets
            .iter()
            .min_by(|(_, a), (_, b)| (a - x).abs().total_cmp(&(b - x).abs()))
            .map_or(0, |(index, _)| *index)
    }
    pub fn selection_segments(&self, range: std::ops::Range<usize>) -> Vec<(f64, f64)> {
        let mut segments: Vec<_> = self
            .clusters
            .iter()
            .filter(|(cluster, _, _)| cluster.start < range.end && cluster.end > range.start)
            .map(|(_, a, b)| (a.min(*b), a.max(*b)))
            .collect();
        segments.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for (left, right) in segments {
            if let Some(last) = merged.last_mut()
                && left <= last.1 + 0.01
            {
                last.1 = last.1.max(right);
            } else {
                merged.push((left, right));
            }
        }
        merged
    }
}

#[derive(Clone)]
pub(crate) struct TextLayout {
    pub lines: Vec<TextLine>,
    pub width: f64,
    pub height: f64,
    pub line_height: f64,
    pub ink_reserve: f64,
}

/// Font database and parsed outlines are retained per renderer, never rebuilt each frame.
pub(crate) struct VectorCache {
    pub profiling: bool,
    pub profile: crate::CanvasRenderProfile,
    options: usvg::Options<'static>,
    items: HashMap<DisplayItemId, (SceneDisplayItem, Arc<usvg::Tree>, usize)>,
    item_bytes: usize,
    lines: HashMap<String, TextLine>,
    line_bytes: usize,
    pub builds: u64,
    clear_color: ColorRgba8,
}

#[derive(Clone)]
struct ClusterAdvance {
    glyphs: Vec<(u16, i32, i32)>, // glyph id, x advance, x offset in font units
    units: f64,
}
type ShapingMetrics = HashMap<(usvg::fontdb::ID, usize), HashMap<usize, ClusterAdvance>>;

#[allow(clippy::too_many_arguments)]
fn cluster_advance(
    cache: &mut ShapingMetrics,
    tree: &usvg::Tree,
    font: usvg::fontdb::ID,
    run_index: usize,
    run: std::ops::Range<usize>,
    text: &str,
    rtl: bool,
    start: usize,
) -> Option<ClusterAdvance> {
    cache
        .entry((font, run_index))
        .or_insert_with(|| {
            tree.fontdb()
                .with_face_data(font, |data, face_index| {
                    let Some(face) = rustybuzz::Face::from_slice(data, face_index) else {
                        return HashMap::new();
                    };
                    let mut buffer = rustybuzz::UnicodeBuffer::new();
                    buffer.push_str(&text[run.clone()]);
                    buffer.set_direction(if rtl {
                        rustybuzz::Direction::RightToLeft
                    } else {
                        rustybuzz::Direction::LeftToRight
                    });
                    let shaped = rustybuzz::shape(&face, &[], buffer);
                    let mut map: HashMap<usize, ClusterAdvance> = HashMap::new();
                    for (info, position) in
                        shaped.glyph_infos().iter().zip(shaped.glyph_positions())
                    {
                        map.entry(info.cluster as usize)
                            .or_insert_with(|| ClusterAdvance {
                                glyphs: Vec::new(),
                                units: f64::from(face.units_per_em()),
                            })
                            .glyphs
                            .push((info.glyph_id as u16, position.x_advance, position.x_offset));
                    }
                    map
                })
                .unwrap_or_default()
        })
        .get(&(start - run.start))
        .cloned()
}

fn tree_storage(group: &usvg::Group) -> usize {
    group.children().iter().fold(1024_usize, |bytes, node| {
        bytes.saturating_add(match node {
            usvg::Node::Group(group) => tree_storage(group),
            usvg::Node::Path(path) => {
                1024_usize.saturating_add(path.data().segments().count().saturating_mul(64))
            }
            usvg::Node::Text(text) => tree_storage(text.flattened()).saturating_add(
                text.layouted()
                    .iter()
                    .flat_map(|span| &span.positioned_glyphs)
                    .map(|glyph| 256 + glyph.text.len())
                    .sum::<usize>(),
            ),
            usvg::Node::Image(image) => match image.kind() {
                usvg::ImageKind::SVG(tree) => tree_storage(tree.root()),
                usvg::ImageKind::JPEG(data)
                | usvg::ImageKind::PNG(data)
                | usvg::ImageKind::GIF(data)
                | usvg::ImageKind::WEBP(data) => data.len().saturating_add(1024),
            },
        })
    })
}

impl Default for VectorCache {
    fn default() -> Self {
        let (font_resolver, _font_cache) = crate::font_cache::resolver();
        let mut options = usvg::Options {
            font_family: "Segoe UI".into(),
            font_resolver,
            ..Default::default()
        };
        options.fontdb_mut().load_system_fonts();
        Self {
            profiling: false,
            profile: Default::default(),
            options,
            items: HashMap::new(),
            item_bytes: 0,
            lines: HashMap::new(),
            line_bytes: 0,
            builds: 0,
            clear_color: ColorRgba8 {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
        }
    }
}

impl VectorCache {
    pub fn take_cached_payload(&mut self) -> impl Send + 'static {
        self.item_bytes = 0;
        self.line_bytes = 0;
        (
            std::mem::take(&mut self.items),
            std::mem::take(&mut self.lines),
        )
    }
    pub fn retained_bytes(&self) -> (usize, usize) {
        (self.item_bytes, self.line_bytes)
    }
    pub fn clear(&mut self) {
        self.items.clear();
        self.item_bytes = 0;
        self.lines.clear();
        self.line_bytes = 0;
    }
    pub fn set_clear_color(&mut self, color: ColorRgba8) {
        if color != self.clear_color {
            self.items.clear();
            self.item_bytes = 0;
            self.clear_color = color;
        }
    }
    pub fn parse(&self, content: &str) -> Result<usvg::Tree, CanvasError> {
        usvg::Tree::from_str(&wrap_svg(content), &self.options)
            .map_err(|error| CanvasError::InvalidFile(error.to_string()))
    }

    pub fn text_width(&mut self, text: &str, style: &TextStyle) -> f64 {
        self.shape_line(text, style).width
    }

    fn shape_line(&mut self, text: &str, style: &TextStyle) -> TextLine {
        let key = format!(
            "{}|{}|{}|{}|{text}",
            style.font_family.as_deref().unwrap_or("Segoe UI"),
            style.font_size,
            style.bold,
            style.italic
        );
        if let Some(line) = self.lines.get(&key) {
            return line.clone();
        }
        if text.len() > 2048 {
            // Obtain the parsed CSS font specification from the same pinned SVG
            // parser, including quoted family lists and generic family names.
            let font = self
                .parse(&text_span("M", style, 0.0, style.font_size))
                .ok()
                .and_then(|tree| {
                    tree.root().children().iter().find_map(|node| {
                        if let usvg::Node::Text(text) = node {
                            text.chunks()
                                .first()?
                                .spans()
                                .first()
                                .map(|span| span.font().clone())
                        } else {
                            None
                        }
                    })
                });
            if let Some(font) = font
                && let Some(line) =
                    crate::text_shaping::shape(&mut self.options, text, style, &font)
            {
                return self.store_line(key, line);
            }
        }
        let content = text_span(
            text,
            &TextStyle {
                color: ColorRgba8 {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                },
                ..style.clone()
            },
            0.0,
            style.font_size,
        );
        let mut line = TextLine {
            text: text.into(),
            width: 0.0,
            start: 0,
            carets: vec![(0, 0.0)],
            clusters: Vec::new(),
            ink_reserve: 0.0,
        };
        if let Ok(tree) = self.parse(&content)
            && let Some(shaped) = tree.root().children().iter().find_map(|node| match node {
                usvg::Node::Text(text) => Some(text),
                _ => None,
            })
        {
            line.width = f64::from(shaped.bounding_box().width());
            let ink = shaped.flattened().bounding_box();
            line.ink_reserve = (-f64::from(ink.x()))
                .max(f64::from(ink.right()) - line.width)
                .max(0.0);
            let all_glyphs: Vec<_> = shaped
                .layouted()
                .iter()
                .flat_map(|span| span.positioned_glyphs.iter())
                .collect();
            let glyphs: Vec<_> = all_glyphs
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, g)| !g.text.is_empty())
                .collect();
            let bidi = unicode_bidi::BidiInfo::new(text, Some(unicode_bidi::Level::ltr()));
            if let Some(paragraph) = bidi.paragraphs.first() {
                let (levels, runs) = bidi.visual_runs(paragraph, paragraph.range.clone());
                let mut run_index = 0;
                let mut consumed = 0;
                let mut clusters = Vec::new();
                // Recover the cluster cursor from the same HarfBuzz font and full bidi
                // run used by usvg. PositionedGlyph::transform includes mark offsets;
                // using it directly gives incorrect caret positions for combining glyphs.
                let mut offsets = ShapingMetrics::new();
                for (raw_index, glyph) in glyphs {
                    while run_index < runs.len() && consumed >= runs[run_index].len() {
                        run_index += 1;
                        consumed = 0;
                    }
                    let Some(run) = runs.get(run_index) else {
                        break;
                    };
                    let rtl = levels[run.start].is_rtl();
                    let length = glyph.text.len().min(run.len().saturating_sub(consumed));
                    let start = if rtl {
                        run.end - consumed - length
                    } else {
                        run.start + consumed
                    };
                    let end = start + length;
                    let mut delta = 0.0;
                    if let Some(metrics) = cluster_advance(
                        &mut offsets,
                        &tree,
                        glyph.font,
                        run_index,
                        run.clone(),
                        text,
                        rtl,
                        start,
                    ) {
                        let marker = if rtl { 0 } else { metrics.glyphs.len() - 1 };
                        if metrics.glyphs[marker].0 == glyph.id.0 {
                            let begin = if rtl {
                                raw_index
                            } else {
                                raw_index
                                    .saturating_add(1)
                                    .saturating_sub(metrics.glyphs.len())
                            };
                            let mut advance = 0_i64;
                            // usvg merges fallback glyphs before positioning a cluster.
                            // Mirror that actual merged sequence, rather than using
                            // preceding advances from the final mark's font.
                            for part in 0..marker {
                                let actual = all_glyphs.get(begin + part).copied().unwrap_or(glyph);
                                let value = cluster_advance(
                                    &mut offsets,
                                    &tree,
                                    actual.font,
                                    run_index,
                                    run.clone(),
                                    text,
                                    rtl,
                                    start,
                                )
                                .and_then(|m| m.glyphs.get(part).copied())
                                .filter(|m| m.0 == actual.id.0)
                                .map_or(metrics.glyphs[part].1, |m| m.1);
                                advance += i64::from(value);
                            }
                            delta = (advance as f64 + f64::from(metrics.glyphs[marker].2))
                                * style.font_size
                                / metrics.units;
                        }
                    }
                    clusters.push((start, end, rtl, f64::from(glyph.transform().tx) - delta));
                    consumed += length;
                }
                line.carets.clear();
                for (index, (start, end, rtl, left)) in clusters.iter().copied().enumerate() {
                    let right = clusters
                        .get(index + 1)
                        .map_or(f64::from(shaped.bounding_box().right()), |c| c.3);
                    let boundaries: Vec<_> = text[start..end]
                        .grapheme_indices(true)
                        .map(|(offset, _)| start + offset)
                        .chain([end])
                        .collect();
                    let count = boundaries.len().saturating_sub(1).max(1) as f64;
                    for (part, boundary) in boundaries.iter().copied().enumerate() {
                        let f = part as f64 / count;
                        line.carets.push((
                            boundary,
                            if rtl {
                                right - (right - left) * f
                            } else {
                                left + (right - left) * f
                            },
                        ));
                    }
                    for (part, pair) in boundaries.windows(2).enumerate() {
                        let a = part as f64 / count;
                        let b = (part + 1) as f64 / count;
                        let (x0, x1) = if rtl {
                            (right - (right - left) * a, right - (right - left) * b)
                        } else {
                            (left + (right - left) * a, left + (right - left) * b)
                        };
                        line.clusters
                            .push((pair[0]..pair[1], x0.min(x1), x0.max(x1)));
                    }
                }
            }
        }
        if line.carets.is_empty() {
            line.carets.push((0, 0.0));
        }
        if !line.carets.iter().any(|(index, _)| *index == text.len()) {
            line.carets.push((text.len(), line.width));
        }
        self.store_line(key, line)
    }
    fn store_line(&mut self, key: String, line: TextLine) -> TextLine {
        let bytes = key.len()
            + line.text.len()
            + line.carets.len() * std::mem::size_of::<(usize, f64)>()
            + line.clusters.len() * std::mem::size_of::<(std::ops::Range<usize>, f64, f64)>();
        const LINE_BUDGET: usize = 16 * 1024 * 1024;
        if self.lines.len() >= 4096 || self.line_bytes.saturating_add(bytes) > LINE_BUDGET {
            self.lines.clear();
            self.line_bytes = 0;
        }
        if bytes <= LINE_BUDGET {
            self.lines.insert(key, line.clone());
            self.line_bytes += bytes;
        }
        line
    }

    pub fn layout(&mut self, text: &str, style: &TextStyle, max_width: f64) -> TextLayout {
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            // Shape a paragraph once to choose breaks. Shaping every growing word
            // prefix makes a long paste quadratic. Final lines are shaped again
            // because joining and ligatures can change at a line boundary.
            let natural = self.shape_line(paragraph, style);
            if natural.width <= max_width || paragraph.is_empty() {
                lines.push(natural);
                continue;
            }
            let advances: HashMap<_, _> = natural
                .clusters
                .iter()
                .map(|(range, left, right)| (range.start, right - left))
                .collect();
            let mut boundaries = vec![(0, 0.0)];
            let mut advance = 0.0;
            for (index, grapheme) in paragraph.grapheme_indices(true) {
                advance += advances.get(&index).copied().unwrap_or(style.font_size);
                boundaries.push((index + grapheme.len(), advance));
            }
            let word_ends: Vec<_> = paragraph
                .split_word_bound_indices()
                .map(|(index, word)| index + word.len())
                .collect();
            let mut start = 0;
            while start + 1 < boundaries.len() {
                let target = boundaries[start].1 + max_width.max(0.0);
                let mut end = boundaries
                    .partition_point(|(_, width)| *width <= target)
                    .saturating_sub(1)
                    .max(start + 1);
                let word = word_ends.partition_point(|byte| *byte <= boundaries[end].0);
                if word > 0 && word_ends[word - 1] > boundaries[start].0 {
                    end = boundaries
                        .binary_search_by_key(&word_ends[word - 1], |b| b.0)
                        .unwrap_or(end);
                }
                let mut shaped =
                    self.shape_line(&paragraph[boundaries[start].0..boundaries[end].0], style);
                if shaped.width > max_width && end > start + 1 {
                    // Bounded binary correction for context-sensitive shaping.
                    let (mut low, mut high) = (start + 1, end);
                    while low + 1 < high {
                        let middle = (low + high) / 2;
                        let probe = self.shape_line(
                            &paragraph[boundaries[start].0..boundaries[middle].0],
                            style,
                        );
                        if probe.width <= max_width {
                            low = middle;
                        } else {
                            high = middle;
                        }
                    }
                    end = low;
                    shaped =
                        self.shape_line(&paragraph[boundaries[start].0..boundaries[end].0], style);
                }
                lines.push(shaped);
                start = end;
            }
        }
        let mut offset = 0;
        for line in &mut lines {
            line.start = offset;
            offset += line.text.len();
            if text[offset..].starts_with('\n') {
                offset += 1;
            }
        }
        let width = lines.iter().map(|line| line.width).fold(1.0, f64::max);
        let line_height = style.font_size * 1.3;
        let ink_reserve = lines
            .iter()
            .map(|line| line.ink_reserve)
            .fold(0.0, f64::max);
        TextLayout {
            height: lines.len() as f64 * line_height,
            width,
            line_height,
            lines,
            ink_reserve,
        }
    }

    pub fn item(&mut self, item: &SceneDisplayItem) -> Result<Arc<usvg::Tree>, CanvasError> {
        let lookup_start = self.profiling.then(std::time::Instant::now);
        if self.profiling {
            self.profile.vector_requests += 1;
        }
        let id = item_id(item);
        if let Some((old, tree, _)) = id.and_then(|id| self.items.get(&id))
            && old == item
        {
            if self.profiling {
                self.profile.vector_hits += 1;
                self.profile.vector_cache_ms += crate::profile::elapsed(lookup_start);
            }
            return Ok(tree.clone());
        }
        if self.profiling {
            self.profile.vector_cache_ms += crate::profile::elapsed(lookup_start);
        }
        let lower_start = self.profiling.then(std::time::Instant::now);
        let content = self.content(item)?;
        if self.profiling {
            self.profile.vector_lower_and_layout_ms += crate::profile::elapsed(lower_start);
        }
        let parse_start = self.profiling.then(std::time::Instant::now);
        let tree = Arc::new(self.parse(&content)?);
        if self.profiling {
            self.profile.svg_parse_and_shape_ms += crate::profile::elapsed(parse_start);
            self.profile.vector_builds += 1;
        }
        let cache_start = self.profiling.then(std::time::Instant::now);
        self.builds += 1;
        // Count flattened outline segments and positioned glyphs as well as source
        // bytes: a short text string can expand into substantial outline storage.
        let estimate = content
            .len()
            .saturating_mul(8)
            .saturating_add(tree_storage(tree.root()));
        const ITEM_BUDGET: usize = 64 * 1024 * 1024;
        if let Some(id) = id
            && estimate <= ITEM_BUDGET
        {
            if let Some((_, _, bytes)) = self.items.remove(&id) {
                self.item_bytes -= bytes;
            }
            while self.item_bytes.saturating_add(estimate) > ITEM_BUDGET
                || self.items.len() >= 16384
            {
                let Some(victim) = self.items.keys().next().copied() else {
                    break;
                };
                if let Some((_, _, bytes)) = self.items.remove(&victim) {
                    self.item_bytes -= bytes;
                }
            }
            self.items
                .insert(id, (item.clone(), tree.clone(), estimate));
            self.item_bytes += estimate;
        }
        if self.profiling {
            self.profile.vector_cache_ms += crate::profile::elapsed(cache_start);
        }
        Ok(tree)
    }

    pub fn retain(&mut self, items: &[SceneDisplayItem]) {
        let ids: std::collections::HashSet<_> = items.iter().filter_map(item_id).collect();
        self.items.retain(|id, (_, _, bytes)| {
            let keep = ids.contains(id);
            if !keep {
                self.item_bytes -= *bytes;
            }
            keep
        });
    }

    pub fn content(&mut self, item: &SceneDisplayItem) -> Result<String, CanvasError> {
        Ok(match item {
            SceneDisplayItem::Rectangle(item) => rectangle(item),
            SceneDisplayItem::Arrow(item) => arrow(item, self.clear_color),
            SceneDisplayItem::SpeechBubble(item) => arrow(&item.path, self.clear_color),
            SceneDisplayItem::Text(item) => self.text(item),
            SceneDisplayItem::SerialNumber(item) => {
                let square = matches!(
                    item.serial_number_type,
                    DisplaySerialNumberType::OutlinedSquare | DisplaySerialNumberType::SolidSquare
                );
                let solid = matches!(
                    item.serial_number_type,
                    DisplaySerialNumberType::SolidCircle | DisplaySerialNumberType::SolidSquare
                );
                let shape = RectangleDisplayItem {
                    center_x: item.center_x,
                    center_y: item.center_y,
                    width: item.diameter,
                    height: item.diameter,
                    rotation: item.rotation,
                    corner_radii: item.corner_radii,
                    shape: if square {
                        DisplayRectangleShape::Rectangle
                    } else {
                        DisplayRectangleShape::Ellipse
                    },
                    stroke: item.color,
                    stroke_width: item.stroke_width,
                    stroke_style: item.stroke_style,
                    stroke_rendering_style: item.stroke_rendering_style,
                    stroke_rendering_seed: item.stroke_rendering_seed,
                    fill: if solid { item.color } else { item.fill },
                    fill_style: if solid {
                        DisplayFillStyle::Solid
                    } else {
                        item.fill_style
                    },
                    opacity: item.opacity,
                    ..Default::default()
                };
                let mut output = rectangle(&shape);
                if item.serial_number_type != DisplaySerialNumberType::Circle {
                    output.push_str(&self.text(&TextDisplayItem {
                        center_x: item.center_x,
                        center_y: item.center_y,
                        width: item.diameter,
                        height: item.diameter,
                        rotation: item.rotation,
                        text: item.label.clone(),
                        color: if solid {
                            serial_text_color(item.color)
                        } else {
                            item.color
                        },
                        font_size: item.font_size,
                        font_family: item.font_family.clone(),
                        horizontal_align: DisplayTextHorizontalAlign::Center,
                        opacity: item.opacity,
                        ..Default::default()
                    }));
                }
                output
            }
            SceneDisplayItem::SerialNumberConnector(item) => {
                let baseline = if item.has_baseline {
                    format!(
                        "M{} {} L{} {}",
                        item.baseline_start_x,
                        item.baseline_start_y,
                        item.baseline_end_x,
                        item.baseline_end_y
                    )
                } else {
                    String::new()
                };
                format!(
                    "<path d=\"M{} {} L{} {} {baseline}\" fill=\"none\" opacity=\"{}\" {}/>",
                    item.start_x,
                    item.start_y,
                    item.end_x,
                    item.end_y,
                    item.opacity,
                    stroke_attributes(item.stroke, item.stroke_width, StrokeStyle::Solid)
                )
            }
            SceneDisplayItem::Magnifier(item) => rectangle(&item.lens),
            SceneDisplayItem::Filter(_) => String::new(),
            SceneDisplayItem::Image | SceneDisplayItem::Stroke => {
                return Err(CanvasError::Unsupported(
                    "payload-free legacy scene placeholders",
                ));
            }
        })
    }

    fn text(&mut self, item: &TextDisplayItem) -> String {
        let style = TextStyle {
            color: item.color,
            font_size: item.font_size.max(1.0),
            bold: item.bold,
            italic: item.italic,
            font_family: item.font_family.clone(),
            fill: item.fill,
            fill_style: FillStyle::Solid,
            stroke: item.stroke,
            stroke_width: item.stroke_width,
            corner_radii: item.corner_radii,
            horizontal_align: TextHorizontalAlign::Left,
            vertical_align: TextVerticalAlign::Center,
            opacity: item.opacity,
        };
        let layout = self.layout(&item.text, &style, item.width.max(1.0));
        let (padding_x, padding_y) = snow_draw_engine_document::text_fill_outset(
            &snow_draw_engine_document::TextPaintGeometry {
                center: Point::new(item.center_x, item.center_y),
                width: item.width,
                height: item.height,
                rotation: item.rotation,
                content_width: layout.width,
                content_height: layout.height,
                horizontal_align: style.horizontal_align,
                vertical_align: style.vertical_align,
                has_text: !item.text.is_empty(),
                font_size: item.font_size,
                fill: item.fill,
                fill_style: match item.fill_style {
                    DisplayFillStyle::Border => FillStyle::Border,
                    DisplayFillStyle::Solid => FillStyle::Solid,
                    DisplayFillStyle::Line => FillStyle::Line,
                    DisplayFillStyle::CrossLine => FillStyle::CrossLine,
                },
                stroke: item.stroke,
                stroke_width: item.stroke_width,
                text_ink_reserve: layout.ink_reserve,
            },
        );
        let top = match item.vertical_align {
            DisplayTextVerticalAlign::Top => -item.height / 2.0,
            DisplayTextVerticalAlign::Center => -layout.height / 2.0,
            DisplayTextVerticalAlign::Bottom => item.height / 2.0 - layout.height,
        };
        let mut output = format!(
            "<g transform=\"translate({} {}) rotate({})\" opacity=\"{}\">",
            item.center_x,
            item.center_y,
            item.rotation.to_degrees(),
            item.opacity
        );
        for (index, line) in layout.lines.iter().enumerate() {
            let x = match item.horizontal_align {
                DisplayTextHorizontalAlign::Left => -item.width / 2.0,
                DisplayTextHorizontalAlign::Center => -line.width / 2.0,
                DisplayTextHorizontalAlign::Right => item.width / 2.0 - line.width,
            };
            let y = top + index as f64 * layout.line_height;
            if item.fill.a != 0 {
                output.push_str(&rectangle(&RectangleDisplayItem {
                    center_x: x + line.width / 2.0,
                    center_y: y + layout.line_height / 2.0,
                    width: line.width + padding_x * 2.0,
                    height: layout.line_height + padding_y * 2.0,
                    fill: item.fill,
                    fill_style: item.fill_style,
                    stroke_width: 0.0,
                    corner_radii: item.corner_radii,
                    ..Default::default()
                }));
            }
            output.push_str(&text_span(&line.text, &style, x, y + style.font_size));
        }
        output.push_str("</g>");
        output
    }
}

fn serial_text_color(fill: ColorRgba8) -> ColorRgba8 {
    let linear = |channel: u8| {
        let value = f64::from(channel) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * linear(fill.r) + 0.7152 * linear(fill.g) + 0.0722 * linear(fill.b);
    if 1.05 / (luminance + 0.05) >= (luminance + 0.05) / (31.0 / 255.0 * luminance + 0.05) {
        ColorRgba8 {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        }
    } else {
        ColorRgba8 {
            r: 0,
            g: 0,
            b: 0,
            a: 224,
        }
    }
}

fn text_span(text: &str, style: &TextStyle, x: f64, y: f64) -> String {
    format!(
        "<text x=\"{x}\" y=\"{y}\" xml:space=\"preserve\" font-family=\"{}\" font-size=\"{}\" font-weight=\"{}\" font-style=\"{}\" fill=\"{}\" fill-opacity=\"{}\" stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"{}\" paint-order=\"stroke fill\">{}</text>",
        xml(style.font_family.as_deref().unwrap_or("Segoe UI")),
        style.font_size,
        if style.bold { "bold" } else { "normal" },
        if style.italic { "italic" } else { "normal" },
        color(style.color),
        f64::from(style.color.a) / 255.0,
        color(style.stroke),
        f64::from(style.stroke.a) / 255.0,
        style.stroke_width,
        xml(text)
    )
}

pub(crate) fn item_id(item: &SceneDisplayItem) -> Option<DisplayItemId> {
    Some(match item {
        SceneDisplayItem::Rectangle(item) => item.id,
        SceneDisplayItem::Arrow(item) => item.id,
        SceneDisplayItem::Text(item) => item.id,
        SceneDisplayItem::Filter(item) => item.id,
        SceneDisplayItem::SerialNumber(item) => item.id,
        SceneDisplayItem::SerialNumberConnector(item) => item.id,
        SceneDisplayItem::SpeechBubble(item) => item.body.id,
        SceneDisplayItem::Magnifier(item) => item.lens.id,
        _ => return None,
    })
}
