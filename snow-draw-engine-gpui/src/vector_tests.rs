use super::*;
#[test]
fn solid_border_and_hatch_fill_pixels_remain_distinct() {
    let mut cache = VectorCache::default();
    let mut coverage = Vec::new();
    for style in [
        DisplayFillStyle::Border,
        DisplayFillStyle::Solid,
        DisplayFillStyle::Line,
        DisplayFillStyle::CrossLine,
    ] {
        let item = SceneDisplayItem::Rectangle(RectangleDisplayItem {
            center_x: 32.0,
            center_y: 32.0,
            width: 48.0,
            height: 48.0,
            opacity: 1.0,
            fill: ColorRgba8 {
                r: 40,
                g: 100,
                b: 190,
                a: 255,
            },
            fill_style: style,
            stroke: ColorRgba8 {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
            stroke_width: 0.0,
            ..Default::default()
        });
        let content = cache.content(&item).unwrap();
        let tree = cache.parse(&content).unwrap();
        let mut pixels = resvg::tiny_skia::Pixmap::new(64, 64).unwrap();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::identity(),
            &mut pixels.as_mut(),
        );
        coverage.push(
            (10..54)
                .flat_map(|y| (10..54).map(move |x| (x, y)))
                .filter(|(x, y)| pixels.pixel(*x, *y).unwrap().alpha() > 0)
                .count(),
        );
    }
    assert_eq!(coverage[0], 0);
    assert_eq!(coverage[1], 44 * 44);
    assert!(coverage[2] > 0 && coverage[2] < coverage[1]);
    assert!(coverage[3] > coverage[2] && coverage[3] < coverage[1]);
}
#[test]
fn ten_thousand_solid_items_retain_their_trees_across_a_single_item_edit() {
    let mut cache = VectorCache::default();
    let mut items: Vec<_> = (0..10000)
        .map(|i| {
            SceneDisplayItem::Rectangle(RectangleDisplayItem {
                id: DisplayItemId {
                    index: i,
                    generation: 1,
                },
                center_x: f64::from(i % 100) * 9.0,
                center_y: f64::from(i / 100) * 6.0,
                width: 5.0,
                height: 3.0,
                opacity: 1.0,
                fill: ColorRgba8 {
                    r: 40,
                    g: 100,
                    b: 190,
                    a: 255,
                },
                fill_style: DisplayFillStyle::Solid,
                stroke_width: 1.0,
                ..Default::default()
            })
        })
        .collect();
    for item in &items {
        cache.item(item).unwrap();
    }
    assert!(cache.retained_bytes().0 <= 64 * 1024 * 1024);
    let before = cache.builds;
    for item in &items {
        cache.item(item).unwrap();
    }
    assert_eq!(
        cache.builds, before,
        "unchanged dense scene evicted and rebuilt its own trees"
    );
    let SceneDisplayItem::Rectangle(item) = &mut items[5000] else {
        unreachable!()
    };
    item.opacity = 0.5;
    for item in &items {
        cache.item(item).unwrap();
    }
    assert_eq!(
        cache.builds,
        before + 1,
        "single edit rebuilt unrelated dense items"
    );
}
#[test]
fn hebrew_visual_carets_hit_tests_and_selection_follow_rtl_glyph_order() {
    let mut cache = VectorCache::default();
    let mut document = crate::CanvasDocument::default();
    let session = crate::CanvasSession::new(&mut document).unwrap();
    let style = document
        .engine()
        .viewport_style_toolbar_state(session.viewport())
        .unwrap()
        .text_style;
    for text in ["אבג".to_string(), "אבג".repeat(400)] {
        let line = cache.shape_line(&text, &style);
        assert!(line.caret_x(0) > line.caret_x(2));
        assert!(line.caret_x(2) > line.caret_x(4));
        assert!(line.caret_x(0) > line.width * 0.99);
        assert!(line.caret_x(text.len()).abs() < 0.1);
        for (range, left, right) in &line.clusters {
            assert_eq!(line.byte_at_x(left * 0.8 + right * 0.2), range.end);
            assert_eq!(line.byte_at_x(left * 0.2 + right * 0.8), range.start);
            let selected = line.selection_segments(range.clone());
            assert_eq!(selected.len(), 1);
            assert!((selected[0].0 - left).abs() < 0.01 && (selected[0].1 - right).abs() < 0.01);
        }
    }
    let mixed = cache.shape_line("aאבb", &style);
    let aleph = mixed
        .clusters
        .iter()
        .find(|(r, _, _)| r.start == 1)
        .unwrap();
    let bet = mixed
        .clusters
        .iter()
        .find(|(r, _, _)| r.start == 3)
        .unwrap();
    assert!(
        bet.1 < aleph.1,
        "Hebrew glyph boxes must reverse logical order inside LTR text"
    );
    assert_eq!(mixed.selection_segments(1..5), vec![(bet.1, aleph.2)]);
}
#[test]
fn bidi_selection_uses_cluster_boxes_and_combining_marks_keep_origins() {
    let mut cache = VectorCache::default();
    let mut document = crate::CanvasDocument::default();
    let session = crate::CanvasSession::new(&mut document).unwrap();
    let mut style = document
        .engine()
        .viewport_style_toolbar_state(session.viewport())
        .unwrap()
        .text_style;
    style.font_size = 24.0;
    let line = cache.shape_line("aאבb", &style);
    let selected = line.selection_segments(1..5);
    let width = selected.iter().map(|(a, b)| b - a).sum::<f64>();
    let hebrew: Vec<_> = line
        .clusters
        .iter()
        .filter(|(r, _, _)| r.start >= 1 && r.end <= 5)
        .collect();
    assert_eq!(hebrew.len(), 2);
    let expected = hebrew.iter().map(|(_, a, b)| b - a).sum::<f64>();
    assert!((width - expected).abs() < 0.01);
    assert!(selected[0].0 >= line.clusters[0].2 - 0.01);
    for text in [
        "q\u{0301}z",
        "a\u{0323}\u{0301}z",
        "office",
        "مرحبا بالعالم",
        "aש\u{05b0}\u{05c1}בb",
        "بش\u{0651}\u{064e}ب",
        "हिन्दी",
    ] {
        let line = cache.shape_line(text, &style);
        for (range, a, b) in &line.clusters {
            assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
            assert!(a.is_finite() && b.is_finite() && b >= a);
        }
        assert!(line.width > 0.0);
        assert!(line.carets.iter().any(|(i, _)| *i == text.len()));
        let selected = line.selection_segments(0..text.len());
        assert!(
            (selected.iter().map(|(a, b)| b - a).sum::<f64>() - line.width).abs() < 0.1,
            "cluster coverage for {text:?}"
        );
    }
    let combining = cache.shape_line("q\u{0301}z", &style);
    assert!(combining.clusters[0].1.abs() < 0.1);
}
#[test]
fn arrowhead_and_text_outline_keep_color_alpha() {
    let mut cache = VectorCache::default();
    let arrow = ArrowDisplayItem {
        stroke: ColorRgba8 {
            r: 20,
            g: 40,
            b: 60,
            a: 128,
        },
        arrowhead_primitives: vec![ArrowheadDisplayPrimitive {
            kind: ArrowheadDisplayPrimitiveKind::Circle,
            center: [20.0, 20.0],
            diameter: 10.0,
            fill_mode: ArrowheadDisplayFillMode::Stroke,
            points: Vec::new(),
            roughness_cap: None,
            dash_mode: ArrowheadDisplayDashMode::Solid,
        }],
        ..Default::default()
    };
    let xml = cache.content(&SceneDisplayItem::Arrow(arrow)).unwrap();
    assert!(xml.contains("fill-opacity=\"0.5019607843137255\""));
    let xml = cache
        .content(&SceneDisplayItem::Text(TextDisplayItem {
            text: "Outline".into(),
            width: 100.0,
            height: 40.0,
            font_size: 24.0,
            color: ColorRgba8 {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            stroke: ColorRgba8 {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            stroke_width: 3.0,
            ..Default::default()
        }))
        .unwrap();
    assert!(xml.contains("stroke-width=\"3\""));
    assert!(xml.contains("paint-order=\"stroke fill\""));
}

#[test]
fn wrapping_preserves_long_unicode_input_and_respects_grapheme_boundaries() {
    let mut cache = VectorCache::default();
    let mut document = crate::CanvasDocument::default();
    let session = crate::CanvasSession::new(&mut document).unwrap();
    let style = document
        .engine()
        .viewport_style_toolbar_state(session.viewport())
        .unwrap()
        .text_style;
    let text = "one שלום e\u{301}你好 👩‍👩‍👦 ".repeat(100);
    let layout = cache.layout(&text, &style, 160.0);
    assert_eq!(
        layout
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<String>(),
        text
    );
    assert!(layout.lines.len() > 100);
    for line in &layout.lines {
        assert!(line.width <= 160.1 || line.text.graphemes(true).count() == 1);
        assert!(text.is_char_boundary(line.start));
    }
    let natural = cache.layout(&text, &style, 1.0e8);
    assert_eq!(natural.lines.len(), 1);
    for text in ["你\u{0301}z", "aש\u{05b0}\u{05c1}בb", "بش\u{0651}\u{064e}ب"] {
        let line = cache.shape_line(text, &style);
        let first = line
            .clusters
            .iter()
            .map(|(_, left, _)| *left)
            .fold(f64::INFINITY, f64::min);
        assert!(
            first.abs() < 0.1,
            "fallback cluster origin {text:?}: {first}"
        );
    }
}

#[test]
fn solid_serial_badges_ignore_border_fill_and_choose_contrasting_text() {
    let mut cache = VectorCache::default();
    let item = SerialNumberDisplayItem {
        center_x: 30.0,
        center_y: 30.0,
        diameter: 40.0,
        font_size: 18.0,
        label: "1".into(),
        serial_number_type: DisplaySerialNumberType::SolidCircle,
        color: ColorRgba8 {
            r: 255,
            g: 245,
            b: 100,
            a: 255,
        },
        fill_style: DisplayFillStyle::Border,
        ..Default::default()
    };
    let content = cache
        .content(&SceneDisplayItem::SerialNumber(item))
        .unwrap();
    assert!(content.contains("fill=\"#fff564\""));
    assert!(content.contains("fill-opacity=\"0.8784313725490196\""));
}

#[test]
fn metrics_only_long_text_matches_worker_outline_layout() {
    let mut cache = VectorCache::default();
    let mut document = crate::CanvasDocument::default();
    let session = crate::CanvasSession::new(&mut document).unwrap();
    let style = document
        .engine()
        .viewport_style_toolbar_state(session.viewport())
        .unwrap()
        .text_style;
    for text in [
        "Canvas e\u{301} שלום 你好 😀 ".repeat(100),
        "office a\u{0323}\u{0301} ".repeat(180),
        "مرحبا بالعالم ".repeat(100),
        "a\tb\r".repeat(600),
    ] {
        let metric = cache.shape_line(&text, &style);
        let content = text_span(&text, &style, 0.0, style.font_size);
        let tree = cache.parse(&content).unwrap();
        let outline = tree
            .root()
            .children()
            .iter()
            .find_map(|node| {
                if let usvg::Node::Text(text) = node {
                    Some(text)
                } else {
                    None
                }
            })
            .unwrap();
        assert!(
            (metric.width - f64::from(outline.bounding_box().width())).abs() < 0.5,
            "long metrics mismatch: {} vs {}",
            metric.width,
            outline.bounding_box().width()
        );
        assert_eq!(
            metric
                .clusters
                .iter()
                .map(|(range, _, _)| range.len())
                .sum::<usize>(),
            text.len()
        );
    }
    for family in [
        "monospace",
        "sans-serif",
        "Consolas, monospace",
        "'Times New Roman', serif",
    ] {
        let style = TextStyle {
            font_family: Some(family.into()),
            ..style.clone()
        };
        let text = "family list wrapping ".repeat(120);
        let metric = cache.shape_line(&text, &style);
        let tree = cache
            .parse(&text_span(&text, &style, 0.0, style.font_size))
            .unwrap();
        let outline = tree
            .root()
            .children()
            .iter()
            .find_map(|node| {
                if let usvg::Node::Text(t) = node {
                    Some(t)
                } else {
                    None
                }
            })
            .unwrap();
        assert!(
            (metric.width - f64::from(outline.bounding_box().width())).abs() < 0.5,
            "family {family}"
        );
    }
}
