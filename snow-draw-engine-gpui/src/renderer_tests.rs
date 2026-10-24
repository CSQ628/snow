use super::*;
#[test]
fn profiling_is_pixel_neutral_and_mid_render_cancel_keeps_the_last_completed_image() {
    let mut document = crate::CanvasDocument::default();
    let mut session = crate::CanvasSession::new(&mut document).unwrap();
    session.set_surface(&mut document, 200, 100).unwrap();
    session
        .set_camera(
            &mut document,
            Camera {
                center: Point::new(100.0, 50.0),
                zoom: 1.0,
            },
        )
        .unwrap();
    let ops:Vec<_>=(0..100).map(|i|serde_json::json!({"type":"rectangle","bounds":[(i%20)*9,(i/20)*18,5,10],"style":{"fill":[40,100,190,255],"stroke_width":1}})).collect();
    document
        .command(|e| {
            e.apply_annotation_json(
                &serde_json::to_vec(&serde_json::json!({"version":1,"operations":ops})).unwrap(),
            )
        })
        .unwrap();
    session.refresh(&document).unwrap();
    let original = session.scene().clone();
    let mut renderer = CanvasRenderer::default();
    let expected = renderer.render(&original, None, 1, 1.0).unwrap();
    renderer.clear_render_state();
    renderer.set_profiling(true);
    let profiled = renderer.render(&original, None, 1, 1.0).unwrap();
    assert_eq!(profiled, expected);
    assert_eq!(renderer.last_render_profile().vector_builds, 100);
    let count = renderer.render_count();
    session
        .set_camera(
            &mut document,
            Camera {
                center: Point::new(101.0, 50.0),
                zoom: 1.0,
            },
        )
        .unwrap();
    let cancel = crate::smart_erase::Cancellation::after_checks(8);
    assert!(matches!(
        renderer.render_cancellable(session.scene(), None, 1, 1.0, &cancel, true),
        Err(CanvasError::Cancelled)
    ));
    assert_eq!(renderer.render_count(), count);
    let retained = renderer.render(&original, None, 1, 1.0).unwrap();
    assert!(Arc::ptr_eq(&retained, &profiled));
    assert!(renderer.last_render_profile().raster_cache_hit);
}
#[test]
fn vector_svg_rejects_a_bounded_output_instead_of_growing_without_limit() {
    let mut document = crate::CanvasDocument::default();
    let mut session = crate::CanvasSession::new(&mut document).unwrap();
    session.set_surface(&mut document, 100, 100).unwrap();
    document
        .command(|e| {
            e.apply_annotation_json(
                br#"{"version":1,"operations":[{"type":"rectangle","bounds":[10,10,60,60]}]}"#,
            )
        })
        .unwrap();
    session.refresh(&document).unwrap();
    let mut renderer = CanvasRenderer::default();
    let complete = renderer.export_svg(session.scene(), None, 1, 1.0).unwrap();
    assert!(complete.contains("<path"));
    assert!(matches!(
        renderer.export_svg_bounded(session.scene(), None, 1, 1.0, complete.len() - 1),
        Err(CanvasError::ResourceLimit)
    ));
    assert_eq!(
        renderer
            .export_svg_bounded(session.scene(), None, 1, 1.0, complete.len())
            .unwrap(),
        complete
    );
}

#[test]
fn many_small_smart_erase_results_discard_donor_halos_and_reuse_across_cameras() {
    let source =
        SourceImage::new(RgbaImage::from_pixel(1100, 1100, Rgba([60, 110, 160, 255]))).unwrap();
    let mut renderer = CanvasRenderer::default();
    let filters: Vec<_> = (0..20)
        .map(|i| FilterDisplayItem {
            center_x: 100.0 + f64::from(i) * 40.0,
            center_y: 550.0,
            width: 4.0,
            height: 4.0,
            opacity: 1.0,
            filter: FilterRenderSpec::resolve(DisplayFilterType::SmartErase, 0.5),
            ..Default::default()
        })
        .collect();
    let mut output = Pixmap::new(32, 32).unwrap();
    for filter in &filters {
        let mut f = frame(32, 32);
        f.camera.center = Point::new(filter.center_x, filter.center_y);
        renderer
            .smart_erase(
                filter,
                Some(&source),
                f,
                1.0,
                &mut output,
                &Default::default(),
            )
            .unwrap();
    }
    let snapshot = renderer.smart_erase_snapshot();
    assert_eq!(snapshot.result_count(), 20);
    assert!(
        snapshot.retained_bytes() < 64 * 1024,
        "cached donor halos instead of affected pixels"
    );
    renderer.restore_smart_erase_snapshot(&snapshot);
    for filter in &filters {
        let mut f = frame(32, 32);
        f.camera.center = Point::new(filter.center_x + 0.25, filter.center_y);
        f.camera.zoom = 1.5;
        renderer
            .smart_erase(
                filter,
                Some(&source),
                f,
                1.0,
                &mut output,
                &Default::default(),
            )
            .unwrap();
    }
    assert_eq!(renderer.smart_erase_snapshot().result_count(), 20);
}
fn frame(w: u32, h: u32) -> FrameView {
    FrameView {
        surface: SurfaceSize {
            width: w,
            height: h,
        },
        camera: Camera {
            center: Point::new(f64::from(w) / 2.0, f64::from(h) / 2.0),
            zoom: 1.0,
        },
        ..Default::default()
    }
}
#[test]
fn blur_uses_premultiplied_colors_and_brightness_endpoints() {
    let f = frame(20, 10);
    let src = RgbaImage::from_fn(20, 10, |x, _| {
        if x < 10 {
            Rgba([255, 0, 0, 0])
        } else {
            Rgba([0, 0, 255, 255])
        }
    });
    let blurred = effect(
        &src,
        &src,
        &FilterRenderSpec::resolve(DisplayFilterType::GaussianBlur, 0.2),
        f,
        1.0,
    )
    .unwrap();
    for p in blurred.pixels() {
        if p[3] > 0 {
            assert_eq!(p[0], 0, "transparent red leaked");
            assert!(p[2] > 240);
        }
    }
    let original = RgbaImage::from_pixel(1, 1, Rgba([20, 80, 140, 120]));
    for (s, expected) in [
        (0.0, [0, 0, 0, 120]),
        (0.5, [20, 80, 140, 120]),
        (1.0, [255, 255, 255, 120]),
    ] {
        let out = effect(
            &original,
            &original,
            &FilterRenderSpec::resolve(DisplayFilterType::Brightness, s),
            frame(1, 1),
            1.0,
        )
        .unwrap();
        assert_eq!(out.get_pixel(0, 0).0, expected);
    }
}
#[test]
fn mosaic_uses_canvas_anchored_center_samples() {
    let src = RgbaImage::from_fn(20, 10, |x, y| Rgba([x as u8, y as u8, 0, 255]));
    let spec = FilterRenderSpec {
        mosaic_block_size: 4.0,
        ..FilterRenderSpec::resolve(DisplayFilterType::Mosaic, 0.5)
    };
    let output = effect(&src, &src, &spec, frame(20, 10), 1.0).unwrap();
    for y in 0..8 {
        for x in 0..20 {
            assert_eq!(
                output.get_pixel(x, y).0,
                [(x / 4 * 4 + 2).min(19) as u8, (y / 4 * 4 + 2) as u8, 0, 255]
            );
        }
    }
}
#[test]
fn magnifier_gradient_has_translated_contours_and_samples_pristine_source() {
    let mut renderer = CanvasRenderer::default();
    let mut lens = MagnifierDisplayItem {
        lens: RectangleDisplayItem {
            center_x: 145.0,
            center_y: 70.0,
            width: 60.0,
            height: 60.0,
            rotation: 0.25,
            shape: DisplayRectangleShape::Ellipse,
            stroke: ColorRgba8 {
                r: 40,
                g: 80,
                b: 200,
                a: 255,
            },
            stroke_width: 2.0,
            opacity: 1.0,
            ..Default::default()
        },
        source_center: Point::new(35.0, 70.0),
        source_width: 30.0,
        source_height: 30.0,
        factor: 2.0,
        connection_type: MagnifierConnectionType::Gradient,
        source_outline: true,
        leader: None,
    };
    let source = SourceImage::new(RgbaImage::from_fn(200, 140, |x, _| {
        Rgba([x as u8, 100, 50, 255])
    }))
    .unwrap();
    let f = frame(200, 140);
    let mut gradient = Pixmap::new(200, 140).unwrap();
    renderer
        .magnifier(&lens, Some(&source), f, 1.0, &mut gradient)
        .unwrap();
    assert!(
        gradient.pixel(90, 70).unwrap().alpha() > 0,
        "bridge is absent"
    );
    let center = gradient.pixel(145, 70).unwrap().demultiply();
    assert!((i32::from(center.red()) - 35).abs() < 3);
    lens.connection_type = MagnifierConnectionType::None;
    let mut no_bridge = Pixmap::new(200, 140).unwrap();
    renderer
        .magnifier(&lens, Some(&source), f, 1.0, &mut no_bridge)
        .unwrap();
    assert_eq!(no_bridge.pixel(90, 70).unwrap().alpha(), 0);
}
#[test]
fn smart_erase_native_cache_is_camera_opacity_independent_and_preview_is_cheap() {
    let mut renderer = CanvasRenderer::default();
    let source = SourceImage::new(RgbaImage::from_fn(80, 60, |x, y| {
        if (25..55).contains(&x) && (20..40).contains(&y) {
            Rgba([255, 0, 0, 255])
        } else {
            Rgba([70, 130, 190, 255])
        }
    }))
    .unwrap()
    .with_placement(Point::new(-20.0, -10.0), 1.25)
    .unwrap();
    let mut filter = FilterDisplayItem {
        center_x: 30.0,
        center_y: 27.5,
        width: 37.5,
        height: 25.0,
        opacity: 1.0,
        filter: FilterRenderSpec::resolve(DisplayFilterType::SmartErase, 0.5),
        ..Default::default()
    };
    let f = frame(120, 90);
    let mut out = Pixmap::new(120, 90).unwrap();
    filter.filter.render_phase = 1;
    renderer
        .smart_erase(
            &filter,
            Some(&source),
            f,
            1.0,
            &mut out,
            &Default::default(),
        )
        .unwrap();
    assert!(renderer.smart_cache.is_empty());
    filter.filter.render_phase = 0;
    renderer
        .smart_erase(
            &filter,
            Some(&source),
            f,
            1.0,
            &mut out,
            &Default::default(),
        )
        .unwrap();
    assert_eq!(renderer.smart_cache.len(), 1);
    let cached = renderer.smart_cache[0].filled.pixels().clone();
    filter.opacity = 0.4;
    let mut zoomed = f;
    zoomed.camera.zoom = 2.0;
    renderer
        .smart_erase(
            &filter,
            Some(&source),
            zoomed,
            1.5,
            &mut out,
            &Default::default(),
        )
        .unwrap();
    assert_eq!(renderer.smart_cache.len(), 1);
    assert!(Arc::ptr_eq(
        &cached,
        renderer.smart_cache[0].filled.pixels()
    ));
    let retained = &renderer.smart_cache[0].filled;
    let x = ((filter.center_x - retained.origin().x) / retained.units_per_pixel()) as u32;
    let y = ((filter.center_y - retained.origin().y) / retained.units_per_pixel()) as u32;
    assert_eq!(cached.get_pixel(x, y).0, [70, 130, 190, 255]);
    let snapshot = renderer.smart_erase_snapshot();
    let mut worker = CanvasRenderer::default();
    worker.restore_smart_erase_snapshot(&snapshot);
    worker
        .smart_erase(
            &filter,
            Some(&source),
            f,
            1.0,
            &mut out,
            &Default::default(),
        )
        .unwrap();
    assert_eq!(worker.smart_erase_snapshot().result_count(), 1);
    filter.center_x += 1.0;
    assert!(
        worker
            .smart_erase(
                &filter,
                Some(&source),
                f,
                1.0,
                &mut out,
                &Default::default()
            )
            .is_err()
    );
    worker.unfreeze_smart_erase();
    worker
        .smart_erase(
            &filter,
            Some(&source),
            f,
            1.0,
            &mut out,
            &Default::default(),
        )
        .unwrap();
    assert_eq!(worker.smart_erase_snapshot().result_count(), 2);
}

#[test]
fn cropped_effects_match_full_surface_at_fractional_render_scale() {
    let mut document = crate::CanvasDocument::default();
    let mut session = crate::CanvasSession::new(&mut document).unwrap();
    session.set_surface(&mut document, 160, 120).unwrap();
    session
        .set_camera(
            &mut document,
            Camera {
                center: Point::new(80.0, 60.0),
                zoom: 1.0,
            },
        )
        .unwrap();
    document.set_background(Some(
        SourceImage::new(RgbaImage::from_fn(160, 120, |x, y| {
            Rgba([
                ((x * 17 + y * 7) % 256) as u8,
                (y * 3 % 256) as u8,
                (x * 5 % 256) as u8,
                if x > 60 { 200 } else { 255 },
            ])
        }))
        .unwrap(),
    ));
    document.command(|e|e.apply_annotation_json(br#"{"version":1,"operations":[{"type":"rectangle_filter","bounds":[41,30,31,20],"style":{"filter":"gaussian_blur","strength":0.2}}]}"#)).unwrap();
    session.refresh(&document).unwrap();
    let mut renderer = CanvasRenderer::default();
    let scale = 1.5;
    let actual = renderer
        .render(
            session.scene(),
            document.background(),
            document.resource_revision(),
            scale,
        )
        .unwrap();
    let frame = session.scene().frame();
    let pristine = background_surface(frame, document.background(), 240, 180, scale);
    let SceneDisplayItem::Filter(filter) = &session.scene().items()[0] else {
        panic!()
    };
    let full = effect(&pristine, &pristine, &filter.filter, frame, scale).unwrap();
    let mask = renderer
        .filter_mask(filter, viewport_transform(frame, scale), 240, 180)
        .unwrap();
    let mut expected = image_to_pixmap(&pristine).unwrap();
    replace_masked(
        &mut expected,
        &full,
        &mask.pixels().iter().map(|p| p.alpha()).collect::<Vec<_>>(),
    );
    let expected = pixmap_to_image(&expected);
    assert!(
        actual
            .as_raw()
            .iter()
            .zip(expected.as_raw())
            .all(|(a, b)| a.abs_diff(*b) <= 1)
    );
}
#[test]
fn unresolved_smart_erase_keeps_later_content_visible_and_strict_export_fails() {
    let mut document = crate::CanvasDocument::default();
    let mut session = crate::CanvasSession::new(&mut document).unwrap();
    session.set_surface(&mut document, 80, 60).unwrap();
    session
        .set_camera(
            &mut document,
            Camera {
                center: Point::new(40.0, 30.0),
                zoom: 1.0,
            },
        )
        .unwrap();
    document.set_background(Some(
        SourceImage::new(RgbaImage::from_pixel(80, 60, Rgba([70, 130, 190, 255]))).unwrap(),
    ));
    let mut style = document
        .engine()
        .viewport_style_toolbar_state(session.viewport())
        .unwrap()
        .filter_style;
    style.filter_type = CanvasFilterType::SmartErase;
    document
        .command(|e| {
            e.set_viewport_filter_style(session.viewport(), style, FILTER_STYLE_PROPERTY_ALL)
        })
        .unwrap();
    session
        .set_tool(&mut document, ActiveTool::RectangleFilter)
        .unwrap();
    for (kind, x, y) in [
        (PointerEventType::Down, 0.0, 0.0),
        (PointerEventType::Move, 80.0, 60.0),
        (PointerEventType::Up, 80.0, 60.0),
    ] {
        session
            .input(
                &mut document,
                InputEvent::Pointer(PointerEvent {
                    pointer_id: 1,
                    event_type: kind,
                    device: PointerDevice::Mouse,
                    position: Point::new(x, y),
                    button: Some(PointerButton::Primary),
                    buttons: PointerButtons(if kind == PointerEventType::Up { 0 } else { 1 }),
                    modifiers: Modifiers::default(),
                    pressure: None,
                    timestamp_ms: 1,
                }),
            )
            .unwrap();
    }
    document.command(|e|e.apply_annotation_json(br#"{"version":1,"operations":[{"type":"rectangle","bounds":[20,20,20,20],"style":{"fill":[30,210,80,255],"stroke":[0,0,0,0],"stroke_width":1}}]}"#)).unwrap();
    session.refresh(&document).unwrap();
    let mut renderer = CanvasRenderer::default();
    let image = renderer
        .render_cancellable(
            session.scene(),
            document.background(),
            document.resource_revision(),
            1.0,
            &Default::default(),
            false,
        )
        .unwrap();
    assert_eq!(image.get_pixel(30, 30).0, [30, 210, 80, 255]);
    assert_eq!(renderer.warnings().len(), 1);
    assert!(
        renderer
            .export_png(
                session.scene(),
                document.background(),
                document.resource_revision(),
                1.0
            )
            .is_err()
    );
}

#[test]
fn partial_smart_erase_underpaints_eligible_source_beneath_visual_overlay() {
    let base = SourceImage::new(RgbaImage::from_pixel(80, 60, Rgba([50, 100, 150, 255]))).unwrap();
    let overlay =
        SourceImage::new(RgbaImage::from_pixel(80, 60, Rgba([240, 10, 30, 128]))).unwrap();
    let source = SourceImage::from_layers(&[
        crate::ImageSourceLayer::new(base),
        crate::ImageSourceLayer {
            image: overlay,
            canvas_rect: None,
            coverage: None,
            smart_erase_source: false,
        },
    ])
    .unwrap()
    .unwrap();
    let mut renderer = CanvasRenderer::default();
    let mut output = image_to_pixmap(source.pixels()).unwrap();
    let filter = FilterDisplayItem {
        center_x: 40.0,
        center_y: 30.0,
        width: 20.0,
        height: 20.0,
        opacity: 0.5,
        filter: FilterRenderSpec::resolve(DisplayFilterType::SmartErase, 0.5),
        ..Default::default()
    };
    renderer
        .smart_erase(
            &filter,
            Some(&source),
            frame(80, 60),
            1.0,
            &mut output,
            &Default::default(),
        )
        .unwrap();
    let result = pixmap_to_image(&output);
    assert_eq!(result.get_pixel(40, 30).0, [50, 100, 150, 255]);
    assert_eq!(result.get_pixel(5, 5), source.pixels().get_pixel(5, 5));
}

#[test]
fn smart_erase_on_a_large_source_allocates_only_the_local_working_region() {
    let source =
        SourceImage::new(RgbaImage::from_pixel(64, 300000, Rgba([70, 120, 170, 255]))).unwrap();
    let mut renderer = CanvasRenderer::default();
    let mut output = Pixmap::new(80, 60).unwrap();
    let frame = FrameView {
        camera: Camera {
            center: Point::new(32.0, 150000.0),
            zoom: 1.0,
        },
        surface: SurfaceSize {
            width: 80,
            height: 60,
        },
        ..Default::default()
    };
    let filter = FilterDisplayItem {
        center_x: 32.0,
        center_y: 150000.0,
        width: 20.0,
        height: 20.0,
        opacity: 1.0,
        filter: FilterRenderSpec::resolve(DisplayFilterType::SmartErase, 0.5),
        ..Default::default()
    };
    renderer
        .smart_erase(
            &filter,
            Some(&source),
            frame,
            1.0,
            &mut output,
            &Default::default(),
        )
        .unwrap();
    let cache = renderer.smart_erase_snapshot();
    assert_eq!(cache.result_count(), 1);
    assert!(cache.retained_bytes() < 1024 * 1024);
    assert_eq!(
        pixmap_to_image(&output).get_pixel(40, 30).0,
        [70, 120, 170, 255]
    );
    let weak = Arc::downgrade(source.pixels());
    drop(source);
    assert!(
        weak.upgrade().is_none(),
        "small erase cache retained the entire large source"
    );
}
