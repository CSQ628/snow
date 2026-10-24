use crate::{engine::*, *};
use image::{Rgba, RgbaImage};
fn scene(items: Vec<SceneDisplayItem>, frame: FrameView, revision: u64) -> RetainedScene {
    let mut patch = ViewportPatch {
        frame_view: frame,
        ..Default::default()
    };
    patch.scene.revision = revision;
    patch.scene.ops.push(ReplaceRangeOp {
        start: 0,
        delete_count: 0,
        insert_items: items,
    });
    let mut result = RetainedScene::default();
    result.apply(&patch).unwrap();
    result
}
fn fixture() -> (Vec<SceneDisplayItem>, FrameView, SourceImage) {
    let items = (0..100)
        .map(|i| {
            SceneDisplayItem::Rectangle(RectangleDisplayItem {
                id: DisplayItemId {
                    index: i,
                    generation: 1,
                },
                center_x: 20.0 + f64::from(i % 10) * 25.0,
                center_y: 20.0 + f64::from(i / 10) * 18.0,
                width: 22.0,
                height: 17.0,
                rotation: 0.1 + f64::from(i % 3) * 0.13,
                opacity: 0.65,
                fill: ColorRgba8 {
                    r: 60,
                    g: 120,
                    b: 190,
                    a: 190,
                },
                fill_style: DisplayFillStyle::Solid,
                stroke: ColorRgba8 {
                    r: 30,
                    g: 70,
                    b: 130,
                    a: 210,
                },
                stroke_width: 1.3,
                ..Default::default()
            })
        })
        .collect();
    let frame = FrameView {
        camera: Camera {
            center: Point::new(140.125, 110.75),
            zoom: 1.15,
        },
        surface: SurfaceSize {
            width: 400,
            height: 300,
        },
        ..Default::default()
    };
    let source = SourceImage::new(RgbaImage::from_fn(200, 160, |x, y| {
        Rgba([
            (x % 251) as u8,
            (y % 251) as u8,
            120,
            if x % 7 == 0 { 100 } else { 230 },
        ])
    }))
    .unwrap()
    .with_placement(Point::new(-30.25, -40.125), 2.0)
    .unwrap();
    (items, frame, source)
}
fn compare(a: &RgbaImage, b: &RgbaImage) {
    assert_eq!(a.dimensions(), b.dimensions());
    let maximum = a
        .as_raw()
        .iter()
        .zip(b.as_raw())
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(maximum <= 1, "partial/full channel difference {maximum}");
}

#[test]
fn large_translated_coordinates_fall_back_before_float_paint_bounds_diverge() {
    let (mut items, mut frame, _) = fixture();
    items.truncate(1);
    frame.camera = Camera {
        center: Point::new(999_999_000.0, 0.0),
        zoom: 1.0,
    };
    let SceneDisplayItem::Rectangle(r) = &mut items[0] else {
        unreachable!()
    };
    r.center_x = 999_999_008.5;
    r.center_y = 0.0;
    let mut renderer = CanvasRenderer::default();
    renderer.set_damage_rendering(true);
    renderer.set_profiling(true);
    let before = renderer
        .render(&scene(items.clone(), frame, 1), None, 1, 1.0)
        .unwrap();
    let SceneDisplayItem::Rectangle(r) = &mut items[0] else {
        unreachable!()
    };
    r.opacity = 0.2;
    let next = scene(items, frame, 2);
    let actual = renderer.render(&next, None, 1, 1.0).unwrap();
    assert!(!renderer.last_render_profile().partial_recomposition);
    assert!(!renderer.last_render_profile().raster_cache_hit);
    assert_ne!(
        before.as_raw(),
        actual.as_raw(),
        "fixture must paint changed pixels"
    );
    compare(
        &actual,
        &CanvasRenderer::default()
            .render(&next, None, 1, 1.0)
            .unwrap(),
    );
}

#[test]
fn damage_cancellation_and_coalesced_revisions_keep_the_completed_baseline() {
    let (mut items, frame, source) = fixture();
    let first = scene(items.clone(), frame, 1);
    let mut renderer = CanvasRenderer::default();
    renderer.set_damage_rendering(true);
    renderer.set_profiling(true);
    let completed = renderer.render(&first, Some(&source), 1, 1.25).unwrap();
    // Skip intermediate revisions: damage must compare against completed pixels,
    // including both distant mutations, rather than only the last engine patch.
    for index in [33, 44] {
        let SceneDisplayItem::Rectangle(r) = &mut items[index] else {
            unreachable!()
        };
        r.opacity = 0.25;
        r.center_x += 1.0;
    }
    let latest = scene(items, frame, 20);
    let cancel = crate::smart_erase::Cancellation::after_checks(2);
    assert!(matches!(
        renderer.render_cancellable(&latest, Some(&source), 1, 1.25, &cancel, true),
        Err(CanvasError::Cancelled)
    ));
    assert!(std::sync::Arc::ptr_eq(
        &completed,
        &renderer.render(&first, Some(&source), 1, 1.25).unwrap()
    ));
    let actual = renderer.render(&latest, Some(&source), 1, 1.25).unwrap();
    assert!(renderer.last_render_profile().partial_recomposition);
    compare(
        &actual,
        &CanvasRenderer::default()
            .render(&latest, Some(&source), 1, 1.25)
            .unwrap(),
    );
}

#[test]
fn effects_decorations_resources_presentation_and_frame_changes_force_full_composition() {
    let (base, frame, source) = fixture();
    for case in 0..6 {
        let mut document = crate::showcase_document().unwrap();
        let mut session = CanvasSession::new(&mut document).unwrap();
        session.set_surface(&mut document, 400, 300).unwrap();
        let mut old = if case == 0 {
            session.scene().clone()
        } else {
            scene(base.clone(), frame, 1)
        };
        let mut renderer = CanvasRenderer::default();
        renderer.set_damage_rendering(true);
        renderer.set_profiling(true);
        let mut next_frame = frame;
        let mut scale = 1.0;
        let mut revision = 1;
        if case == 1 {
            let mut patch = ViewportPatch {
                frame_view: frame,
                ..Default::default()
            };
            patch.scene.revision = 1;
            patch.scene.ops.push(ReplaceRangeOp {
                start: 0,
                delete_count: 0,
                insert_items: base.clone(),
            });
            patch.decoration.view.spotlight.active = true;
            old = RetainedScene::default();
            old.apply(&patch).unwrap();
        }
        renderer.render(&old, Some(&source), 1, 1.0).unwrap();
        let mut items = base.clone();
        let SceneDisplayItem::Rectangle(r) = &mut items[44] else {
            unreachable!()
        };
        r.opacity = 0.3;
        match case {
            2 => next_frame.camera.center.x += 0.1,
            3 => scale = 1.5,
            4 => revision = 2,
            _ => {}
        }
        let next = if case == 0 || case == 1 {
            old.culled(old.frame(), |_| true)
        } else {
            scene(items, next_frame, 2)
        };
        let presentation = if case == 5 {
            CanvasPresentation {
                before: Some(source.clone()),
                ..Default::default()
            }
        } else {
            Default::default()
        };
        let actual = renderer
            .render_with_presentation(&next, Some(&source), revision, scale, &presentation)
            .unwrap();
        assert!(
            !renderer.last_render_profile().partial_recomposition,
            "case {case}"
        );
        assert!(!renderer.last_render_profile().raster_cache_hit);
        compare(
            &actual,
            &CanvasRenderer::default()
                .render_with_presentation(&next, Some(&source), revision, scale, &presentation)
                .unwrap(),
        );
    }
}
#[test]
fn retained_damage_matches_full_for_transparency_move_style_and_coalesced_changes() {
    let (mut items, frame, source) = fixture();
    for scale in [1.0, 1.25, 1.5] {
        let mut renderer = CanvasRenderer::default();
        renderer.set_damage_rendering(true);
        renderer.set_profiling(true);
        renderer
            .render(&scene(items.clone(), frame, 1), Some(&source), 1, scale)
            .unwrap();
        for step in 0..12 {
            let SceneDisplayItem::Rectangle(r) = &mut items[44] else {
                unreachable!()
            };
            r.opacity = if step % 2 == 0 { 0.3 } else { 0.8 };
            r.center_x += 0.31;
            r.width += 0.1;
            r.rotation += 0.01;
            if step % 3 == 0 {
                let SceneDisplayItem::Rectangle(r) = &mut items[45] else {
                    unreachable!()
                };
                r.center_y += 0.27;
            }
            let next = scene(items.clone(), frame, step + 2);
            let actual = renderer.render(&next, Some(&source), 1, scale).unwrap();
            assert!(renderer.last_render_profile().partial_recomposition);
            assert!(
                renderer.last_render_profile().damaged_pixels
                    < renderer.last_render_profile().output_pixels / 4
            );
            let expected = CanvasRenderer::default()
                .render(&next, Some(&source), 1, scale)
                .unwrap();
            compare(&actual, &expected);
        }
    }
}
#[test]
fn retained_damage_handles_pressure_outline_and_arrow_heads() {
    let (mut items, frame, source) = fixture();
    let commands = vec![
        PathCommand::MoveTo {
            point: [120.0, 80.0],
        },
        PathCommand::LineTo {
            point: [160.0, 86.0],
        },
        PathCommand::LineTo {
            point: [140.0, 94.0],
        },
        PathCommand::LineTo {
            point: [120.0, 80.0],
        },
    ];
    let pressure = snow_draw_engine_core::PathGeometry::replace_contour_tail(
        None,
        1,
        0,
        vec![(commands, 0.0, 80.0)],
    );
    items.push(SceneDisplayItem::Arrow(ArrowDisplayItem {
        id: DisplayItemId {
            index: 100,
            generation: 1,
        },
        pressure_geometry: Some(std::sync::Arc::new(pressure)),
        opacity: 0.7,
        stroke: ColorRgba8 {
            r: 210,
            g: 60,
            b: 80,
            a: 230,
        },
        stroke_width: 3.0,
        arrowhead_primitives: vec![ArrowheadDisplayPrimitive {
            kind: ArrowheadDisplayPrimitiveKind::Circle,
            center: [160.0, 86.0],
            diameter: 10.0,
            fill_mode: ArrowheadDisplayFillMode::Stroke,
            points: Vec::new(),
            roughness_cap: None,
            dash_mode: ArrowheadDisplayDashMode::Solid,
        }],
        ..Default::default()
    }));
    let mut renderer = CanvasRenderer::default();
    renderer.set_damage_rendering(true);
    renderer.set_profiling(true);
    renderer
        .render(&scene(items.clone(), frame, 1), Some(&source), 1, 1.25)
        .unwrap();
    let SceneDisplayItem::Arrow(a) = items.last_mut().unwrap() else {
        unreachable!()
    };
    a.opacity = 0.3;
    let next = scene(items, frame, 2);
    let actual = renderer.render(&next, Some(&source), 1, 1.25).unwrap();
    assert!(renderer.last_render_profile().partial_recomposition);
    compare(
        &actual,
        &CanvasRenderer::default()
            .render(&next, Some(&source), 1, 1.25)
            .unwrap(),
    );
}
#[test]
fn unsafe_damage_dependencies_and_order_changes_fall_back_to_full() {
    let (base, frame, source) = fixture();
    for case in [0, 2, 3, 4, 5, 6, 7] {
        let mut items = base.clone();
        let mut renderer = CanvasRenderer::default();
        renderer.set_damage_rendering(true);
        renderer.set_profiling(true);
        renderer
            .render(&scene(items.clone(), frame, 1), Some(&source), 1, 1.0)
            .unwrap();
        match case {
            0 => items.swap(0, 1),
            2 => {
                let SceneDisplayItem::Rectangle(r) = &mut items[44] else {
                    unreachable!()
                };
                r.blend_mode = DisplayBlendMode::Multiply;
            }
            3 => {
                let SceneDisplayItem::Rectangle(r) = &mut items[44] else {
                    unreachable!()
                };
                r.stroke_rendering_style = StrokeRenderingStyle::Artist;
            }
            4 => {
                let SceneDisplayItem::Rectangle(r) = &mut items[44] else {
                    unreachable!()
                };
                r.width = 600.0;
                r.height = 400.0;
            }
            5 => {
                for item in &mut items {
                    let SceneDisplayItem::Rectangle(r) = item else {
                        unreachable!()
                    };
                    r.opacity = 0.3;
                }
            }
            7 => items.push(items[44].clone()), // ambiguous duplicate identity
            _ => items.push(SceneDisplayItem::Text(TextDisplayItem {
                text: "Fallback".into(),
                width: 100.0,
                height: 40.0,
                font_size: 20.0,
                ..Default::default()
            })),
        }
        let next = scene(items, frame, 2);
        let actual = renderer.render(&next, Some(&source), 1, 1.0).unwrap();
        assert!(
            !renderer.last_render_profile().partial_recomposition,
            "case {case}"
        );
        compare(
            &actual,
            &CanvasRenderer::default()
                .render(&next, Some(&source), 1, 1.0)
                .unwrap(),
        );
    }
}

#[test]
fn retained_damage_matches_full_for_creation_resize_rotation_removal_and_id_reuse() {
    let (base, frame, source) = fixture();
    for scale in [1.0, 1.25, 2.0] {
        let mut items = base.clone();
        let mut renderer = CanvasRenderer::default();
        renderer.set_profiling(true);
        renderer
            .render(&scene(items.clone(), frame, 1), Some(&source), 1, scale)
            .unwrap();
        for step in 0..5 {
            match step {
                0 | 4 => {
                    let SceneDisplayItem::Rectangle(mut created) = base[44].clone() else {
                        unreachable!()
                    };
                    created.id = DisplayItemId {
                        index: 200,
                        generation: 1 + step,
                    };
                    created.center_x += 4.0;
                    created.fill.r = 230;
                    // Insert below overlapping translucent items as well as above others.
                    items.insert(40, SceneDisplayItem::Rectangle(created));
                }
                1 => {
                    let SceneDisplayItem::Rectangle(created) = &mut items[40] else {
                        unreachable!()
                    };
                    created.center_x += 7.0;
                    created.width += 9.0;
                    created.rotation += 0.7;
                }
                2 => {
                    items.remove(40);
                }
                3 => {
                    items.remove(44);
                }
                _ => unreachable!(),
            }
            let next = scene(items.clone(), frame, u64::from(step) + 2);
            let actual = renderer.render(&next, Some(&source), 1, scale).unwrap();
            assert!(
                renderer.last_render_profile().partial_recomposition,
                "step {step}"
            );
            compare(
                &actual,
                &CanvasRenderer::default()
                    .render(&next, Some(&source), 1, scale)
                    .unwrap(),
            );
        }
    }
}
