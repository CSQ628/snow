use snow_draw_engine_gpui::{CanvasDocument, CanvasRenderer, CanvasSession, engine::*};
use std::{hint::black_box, time::Instant};
fn timing(mut f: impl FnMut(), samples: usize) -> serde_json::Value {
    let mut values = Vec::with_capacity(samples);
    for _ in 0..samples {
        let t = Instant::now();
        f();
        values.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    values.sort_by(f64::total_cmp);
    serde_json::json!({"samples":samples,"median_ms":values[samples/2],"p95_ms":values[((samples as f64*0.95).ceil()as usize).saturating_sub(1)],"min_ms":values[0],"max_ms":values[samples-1]})
}
fn main() {
    let destination = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "../artifacts/snow-draw-gpui-performance.json".into());
    let mut results = Vec::new();
    for count in [1000, 10000] {
        let start = Instant::now();
        let mut doc = CanvasDocument::default();
        let mut session = CanvasSession::new(&mut doc).unwrap();
        session.set_surface(&mut doc, 960, 640).unwrap();
        session
            .set_camera(
                &mut doc,
                Camera {
                    center: Point::new(480.0, 320.0),
                    zoom: 1.0,
                },
            )
            .unwrap();
        for base in (0..count).step_by(128) {
            let ops:Vec<_>=(base..(base+128).min(count)).map(|i|{let x=(i%100)as f64*90.0;let y=(i/100)as f64*65.0;serde_json::json!({"type":"rectangle","bounds":[x,y,65,40],"style":{"stroke":[47,100,168,255],"fill":[211,229,251,255],"stroke_width":2}})}).collect();
            let data =
                serde_json::to_vec(&serde_json::json!({"version":1,"operations":ops})).unwrap();
            doc.command(|e| e.apply_annotation_json(&data)).unwrap();
        }
        session.refresh(&doc).unwrap();
        let construction = start.elapsed().as_secs_f64() * 1000.0;
        let visible = session.scene().items().len();
        let mut renderer = CanvasRenderer::default();
        let cold = timing(
            || {
                black_box(
                    renderer
                        .render(session.scene(), None, doc.resource_revision(), 1.0)
                        .unwrap(),
                );
            },
            1,
        );
        let warm = timing(
            || {
                black_box(
                    renderer
                        .render(session.scene(), None, doc.resource_revision(), 1.0)
                        .unwrap(),
                );
            },
            1000,
        );
        let vector_builds = renderer.vector_build_count();
        let mut n = 0;
        let pan = timing(
            || {
                n += 1;
                session
                    .set_camera(
                        &mut doc,
                        Camera {
                            center: Point::new(480.0 + (n % 12) as f64 * 100.0, 320.0),
                            zoom: 1.0,
                        },
                    )
                    .unwrap();
                black_box(session.scene().items().len());
            },
            120,
        );
        let revision = session.scene().cursor().unwrap().scene_revision;
        let mut n = 0;
        session.set_tool(&mut doc, ActiveTool::Select).unwrap();
        let overlay = timing(
            || {
                n += 1;
                session
                    .input(
                        &mut doc,
                        InputEvent::Pointer(PointerEvent {
                            pointer_id: 1,
                            event_type: PointerEventType::Move,
                            device: PointerDevice::Mouse,
                            position: Point::new((n % 900) as f64, 400.0),
                            button: None,
                            buttons: PointerButtons(0),
                            modifiers: Modifiers::default(),
                            pressure: None,
                            timestamp_ms: n,
                        }),
                    )
                    .unwrap();
            },
            120,
        );
        results.push(serde_json::json!({"elements":count,"initial_visible_items":visible,"construction_ms":construction,"cold_raster":cold,"cached_raster":warm,"pan_patch":pan,"hover_patch":overlay,"hover_preserves_scene_revision":revision==session.scene().cursor().unwrap().scene_revision,"vector_builds":vector_builds,"raster_count":renderer.render_count()}));
        session.close(&mut doc).unwrap();
        let drop_start = Instant::now();
        drop(doc);
        results.last_mut().unwrap()["document_drop_ms"] =
            serde_json::json!(drop_start.elapsed().as_secs_f64() * 1000.0);
    }
    let mut doc = CanvasDocument::default();
    let mut session = CanvasSession::new(&mut doc).unwrap();
    session.set_surface(&mut doc, 960, 640).unwrap();
    session.set_tool(&mut doc, ActiveTool::FreeDraw).unwrap();
    let start = Instant::now();
    for i in 0..30000 {
        let kind = if i == 0 {
            PointerEventType::Down
        } else if i == 29999 {
            PointerEventType::Up
        } else {
            PointerEventType::Move
        };
        session
            .input(
                &mut doc,
                InputEvent::Pointer(PointerEvent {
                    pointer_id: 1,
                    event_type: kind,
                    device: PointerDevice::Pen,
                    position: Point::new((i % 900) as f64, 300.0 + (i as f64 * 0.01).sin() * 100.0),
                    button: Some(PointerButton::Primary),
                    buttons: PointerButtons(if i == 29999 { 0 } else { 1 }),
                    modifiers: Modifiers::default(),
                    pressure: Some(0.2 + (i % 100) as f64 / 125.0),
                    timestamp_ms: i,
                }),
            )
            .unwrap();
    }
    let long_ms = start.elapsed().as_secs_f64() * 1000.0;
    let mut renderer = CanvasRenderer::default();
    let long_raster = timing(
        || {
            black_box(
                renderer
                    .render(session.scene(), None, doc.resource_revision(), 1.0)
                    .unwrap(),
            );
        },
        1,
    );
    let output = serde_json::json!({"profile":"canvas-perf (inherits release, opt-level=3)","unix_seconds":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),"machine":std::env::var("COMPUTERNAME").unwrap_or_default(),"logical_processors":std::thread::available_parallelism().map(|n|n.get()).unwrap_or(0),"surface":[960,640],"scale":1,"large_documents":results,"long_pressure_stroke":{"input_samples":30000,"input_and_retained_patch_ms":long_ms,"cold_raster":long_raster}});
    let mut output = output;
    let mut dense_results = Vec::new();
    for count in [1000, 10000] {
        let mut doc = CanvasDocument::default();
        let mut session = CanvasSession::new(&mut doc).unwrap();
        session.set_surface(&mut doc, 960, 640).unwrap();
        session
            .set_camera(
                &mut doc,
                Camera {
                    center: Point::new(480.0, 320.0),
                    zoom: 1.0,
                },
            )
            .unwrap();
        for base in (0..count).step_by(128) {
            let ops: Vec<_> = (base..(base+128).min(count)).map(|i| serde_json::json!({"type":"rectangle","bounds":[(i%100)as f64*9.0,(i/100)as f64*6.0,5,3],"style":{"fill":[40,100,190,255],"stroke_width":1}})).collect();
            doc.command(|e| {
                e.apply_annotation_json(
                    &serde_json::to_vec(&serde_json::json!({"version":1,"operations":ops}))
                        .unwrap(),
                )
            })
            .unwrap();
        }
        session.refresh(&doc).unwrap();
        let mut renderer = CanvasRenderer::default();
        let cold = timing(
            || {
                renderer.clear_render_state();
                black_box(
                    renderer
                        .render(session.scene(), None, doc.resource_revision(), 1.0)
                        .unwrap(),
                );
            },
            3,
        );
        dense_results.push(serde_json::json!({"elements":count,"visible_items":session.scene().items().len(),"cold_raster":cold,"retained_vector_budget_bytes":renderer.retained_vector_bytes()}));
    }
    output["dense_documents"] = serde_json::json!(dense_results);
    let mut doc = snow_draw_engine_gpui::showcase_document().unwrap();
    let mut session = CanvasSession::new(&mut doc).unwrap();
    session.set_surface(&mut doc, 960, 640).unwrap();
    session
        .set_camera(
            &mut doc,
            Camera {
                center: Point::new(480.0, 320.0),
                zoom: 1.0,
            },
        )
        .unwrap();
    let mut renderer = CanvasRenderer::default();
    renderer.prepare(&mut doc, &mut session).unwrap();
    output["screenshot_effects"] = timing(
        || {
            renderer.clear_render_state();
            black_box(
                renderer
                    .render(
                        session.scene(),
                        doc.background(),
                        doc.resource_revision(),
                        1.0,
                    )
                    .unwrap(),
            );
        },
        5,
    );
    let style = doc
        .engine()
        .viewport_style_toolbar_state(session.viewport())
        .unwrap()
        .text_style;
    let mut text = "Canvas e\u{301} שלום 你好 😀 ".repeat(4000);
    let mut end = 64 * 1024;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    let mut text_results = Vec::new();
    for length in [32 * 1024, text.len()] {
        let mut end = length;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let value = &text[..end];
        let natural = timing(
            || {
                renderer.clear_render_state();
                black_box(renderer.measure_text(value, &style, 1.0e8, false));
            },
            3,
        );
        let wrapped = timing(
            || {
                renderer.clear_render_state();
                black_box(renderer.measure_text(value, &style, 600.0, true));
            },
            3,
        );
        text_results.push(serde_json::json!({"utf8_bytes":value.len(),"natural":natural,"wrapped_600":wrapped,"retained_layout_budget_bytes":renderer.retained_text_layout_bytes()}));
    }
    output["long_unicode_text"] = serde_json::json!(text_results);
    let mut doc = CanvasDocument::default();
    let mut session = CanvasSession::new(&mut doc).unwrap();
    session.set_surface(&mut doc, 192, 160).unwrap();
    session
        .set_camera(
            &mut doc,
            Camera {
                center: Point::new(96.0, 80.0),
                zoom: 1.0,
            },
        )
        .unwrap();
    doc.set_background(Some(
        snow_draw_engine_gpui::SourceImage::new(image::RgbaImage::from_fn(192, 160, |x, y| {
            let n = (((x.wrapping_mul(73856093)) ^ (y.wrapping_mul(19349663))) % 25) as i32 - 12;
            let base = if y < 60 {
                [55, 80, 110]
            } else if y < 90 {
                [205, 65, 90]
            } else {
                [65, 150, 40]
            };
            image::Rgba([
                (base[0] + n) as u8,
                (base[1] + n) as u8,
                (base[2] + n) as u8,
                255,
            ])
        }))
        .unwrap(),
    ));
    let mut style = doc
        .engine()
        .viewport_style_toolbar_state(session.viewport())
        .unwrap()
        .filter_style;
    style.filter_type = CanvasFilterType::SmartErase;
    doc.command(|e| {
        e.set_viewport_filter_style(session.viewport(), style, FILTER_STYLE_PROPERTY_ALL)
    })
    .unwrap();
    session
        .set_tool(&mut doc, ActiveTool::RectangleFilter)
        .unwrap();
    for (kind, x, y) in [
        (PointerEventType::Down, 62.0, 26.0),
        (PointerEventType::Move, 130.0, 120.0),
        (PointerEventType::Up, 130.0, 120.0),
    ] {
        session
            .input(
                &mut doc,
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
    let mut renderer = CanvasRenderer::default();
    let cold = timing(
        || {
            renderer.clear_render_state();
            black_box(
                renderer
                    .render(
                        session.scene(),
                        doc.background(),
                        doc.resource_revision(),
                        1.0,
                    )
                    .unwrap(),
            );
        },
        3,
    );
    let mut n = 0;
    let cached = timing(
        || {
            n += 1;
            session
                .set_camera(
                    &mut doc,
                    Camera {
                        center: Point::new(96.0 + f64::from(n) * 0.01, 80.0),
                        zoom: 1.0,
                    },
                )
                .unwrap();
            black_box(
                renderer
                    .render(
                        session.scene(),
                        doc.background(),
                        doc.resource_revision(),
                        1.0,
                    )
                    .unwrap(),
            );
        },
        20,
    );
    output["smart_erase"] = serde_json::json!({"source":[192,160],"mask":[62,26,68,94],"cold_reconstruction":cold,"camera_change_with_cached_reconstruction":cached,"completed_results":renderer.smart_erase_snapshot().result_count()});
    let mut large_rasters = Vec::new();
    for [width, height] in [[11520, 2160], [256, 100000]] {
        let mut doc = CanvasDocument::default();
        doc.set_background(Some(
            snow_draw_engine_gpui::SourceImage::new(image::RgbaImage::from_fn(
                width,
                height,
                |x, y| {
                    image::Rgba([
                        (x % 251) as u8,
                        (y % 251) as u8,
                        ((x / 16 + y / 16) % 251) as u8,
                        255,
                    ])
                },
            ))
            .unwrap(),
        ));
        let mut session = CanvasSession::new(&mut doc).unwrap();
        let mut renderer = CanvasRenderer::default();
        let snapshot = renderer
            .snapshot_for_export(
                &mut doc,
                &mut session,
                snow_draw_engine_gpui::CanvasRect::new(
                    0.0,
                    0.0,
                    f64::from(width),
                    f64::from(height),
                )
                .unwrap(),
                [width, height],
            )
            .unwrap();
        let duration = timing(
            || {
                renderer
                    .export_snapshot_png_to_writer(
                        &snapshot,
                        1.0,
                        std::io::sink(),
                        &Default::default(),
                    )
                    .unwrap()
            },
            1,
        );
        large_rasters.push(serde_json::json!({"source_pixels":[width,height],"source_rgba_bytes":u64::from(width)*u64::from(height)*4,"stream_to_sink":duration,"peak_working_surface_pixels":renderer.peak_working_surface_pixels(),"full_output_rgba_allocated":false,"output_band_budget_bytes":8*1024*1024}));
    }
    output["large_raster_streaming"] = serde_json::json!(large_rasters);
    let path = std::path::Path::new(&destination);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, serde_json::to_vec_pretty(&output).unwrap()).unwrap();
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
