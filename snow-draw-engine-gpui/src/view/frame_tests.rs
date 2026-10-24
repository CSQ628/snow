use super::*;

fn pointer(cx: &mut VisualTestContext, position: Point<Pixels>, down: bool) {
    cx.update(|window, cx| {
        let event = if down {
            gpui::PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position,
                ..Default::default()
            })
        } else {
            gpui::PlatformInput::MouseMove(MouseMoveEvent {
                position,
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            })
        };
        window.dispatch_event(event, cx);
    });
}

#[gpui::test]
fn creation_pixels_and_selection_are_current_in_the_same_frame(cx: &mut TestAppContext) {
    let (view, mut visual) = fixture(cx, CanvasDocument::default());
    visual.update(|window, cx| {
        view.update(cx, |v, cx| v.set_tool(draw::ActiveTool::Shape, window, cx));
    });
    paint(&mut visual);
    pointer(&mut visual, point(px(200.0), px(180.0)), true);
    for x in [240.0, 280.0, 320.0] {
        pointer(&mut visual, point(px(x), px(260.0)), false);
        // Paint without draining workers: a displayed frame must already match
        // the scene that supplies its selection controls.
        visual.update(|window, cx| window.draw(cx).clear(cx));
        view.read_with(&visual, |v, _| {
            assert!(!v.session.scene().items().is_empty());
            assert_eq!(v.rendered_generation, v.generation);
            assert!(v.error.is_none());
        });
    }
}

#[gpui::test]
fn failed_frame_retains_controls_from_the_completed_scene(cx: &mut TestAppContext) {
    let (view, mut visual) = fixture(cx, CanvasDocument::default());
    visual.update(|window, cx| {
        view.update(cx, |v, cx| v.set_tool(draw::ActiveTool::Shape, window, cx))
    });
    pointer(&mut visual, point(px(200.0), px(180.0)), true);
    pointer(&mut visual, point(px(240.0), px(230.0)), false);
    paint(&mut visual);
    visual.update(|window, cx| {
        view.update(cx, |v, cx| {
            let painted = v.display_scene().clone();
            let frame = v.session.scene().frame();
            v.session
                .set_camera(
                    &mut v.document,
                    draw::Camera {
                        center: draw::Point::new(90.0, 90.0),
                        ..frame.camera
                    },
                )
                .unwrap();
            // Force reference preparation to fail after the live viewport advances.
            let mut other = CanvasDocument::default();
            v.reference_session = CanvasSession::new(&mut other).unwrap();
            v.changed(window, cx);
            assert!(!v.reference_valid);
            assert_eq!(v.display_scene().frame(), painted.frame());
            assert_eq!(v.display_scene().overlays(), painted.overlays());
            assert_ne!(v.session.scene().frame(), painted.frame());
        })
    });
}

#[gpui::test]
fn smart_erase_paints_current_mask_and_only_imports_valid_worker_results(cx: &mut TestAppContext) {
    for mode in [
        "complete",
        "cancel_pending",
        "cancel_completed",
        "no_donors",
    ] {
        let mut document = CanvasDocument::default();
        document.set_background(Some(
            SourceImage::new(image::RgbaImage::from_pixel(
                160,
                120,
                image::Rgba([60, 90, 140, 255]),
            ))
            .unwrap(),
        ));
        let (view, mut visual) = fixture(cx, document);
        visual.update(|window, cx| {
            view.update(cx, |v, cx| {
                let viewport = v.session.viewport();
                let mut style = v
                    .document
                    .engine()
                    .viewport_style_toolbar_state(viewport)
                    .unwrap()
                    .filter_style;
                style.filter_type = draw::CanvasFilterType::SmartErase;
                v.document
                    .command(|e| {
                        e.set_viewport_filter_style(
                            viewport,
                            style,
                            draw::FILTER_STYLE_PROPERTY_ALL,
                        )
                    })
                    .unwrap();
                v.set_tool(draw::ActiveTool::RectangleFilter, window, cx);
            })
        });
        paint(&mut visual);
        let (start, end) = if mode == "no_donors" {
            (point(px(1.0), px(1.0)), point(px(639.0), px(479.0)))
        } else {
            (point(px(280.0), px(220.0)), point(px(340.0), px(280.0)))
        };
        pointer(&mut visual, start, true);
        pointer(&mut visual, end, false);
        visual.update(|window, cx| {
            window.dispatch_event(
                gpui::PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position: end,
                    ..Default::default()
                }),
                cx,
            );
            window.draw(cx).clear(cx);
        });
        let preview = view.read_with(&visual, |v, _| {
            assert_eq!(v.rendered_generation, v.generation);
            assert!(v.smart_task.is_some());
            assert_eq!(
                v.renderer
                    .as_ref()
                    .unwrap()
                    .smart_erase_snapshot()
                    .result_count(),
                0,
                "frame preparation ran reconstruction on the UI thread"
            );
            v.image.clone().unwrap()
        });
        if mode == "cancel_completed" {
            let scheduler = visual.dispatcher.scheduler().clone();
            for _ in 0..100 {
                if scheduler.pending_task_counts().1 > 0 {
                    break;
                }
                assert!(scheduler.tick(), "reconstruction worker was never queued");
            }
            assert!(scheduler.pending_task_counts().1 > 0);
            while scheduler.tick_background_only() {}
        }
        if mode.starts_with("cancel_") {
            visual.update(|window, cx| {
                view.update(cx, |v, cx| {
                    v.replace_document(CanvasDocument::default(), window, cx)
                        .unwrap();
                })
            });
        }
        paint(&mut visual);
        view.read_with(&visual, |v, _| {
            assert!(!v.is_rendering(), "{}", v.render_diagnostics());
            assert_eq!(
                v.error.is_some(),
                mode == "no_donors",
                "{mode}: {:?}",
                v.error
            );
            assert_eq!(
                v.renderer
                    .as_ref()
                    .unwrap()
                    .smart_erase_snapshot()
                    .result_count(),
                usize::from(mode == "complete")
            );
            if mode != "no_donors" {
                assert_ne!(v.image.as_ref().unwrap().as_bytes(0), preview.as_bytes(0));
            }
        });
        let completed = view.read_with(&visual, |v, _| v.image.clone().unwrap());
        paint(&mut visual);
        view.read_with(&visual, |v, _| {
            assert!(!v.is_rendering());
            assert!(
                Arc::ptr_eq(&completed, v.image.as_ref().unwrap()),
                "{mode}: idle retry"
            );
        });
    }
}
