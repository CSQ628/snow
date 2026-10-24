use snow_draw_engine_gpui::{
    CanvasCancellation, CanvasDocument, CanvasRenderer, CanvasSession, engine::*,
};
use std::{
    hint::black_box,
    sync::{Arc, Barrier},
    time::{Duration, Instant},
};

fn hash(image: &image::RgbaImage) -> u64 {
    image.as_raw().iter().fold(14695981039346656037, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(1099511628211)
    })
}
fn fixture() -> (CanvasDocument, CanvasSession) {
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
    for base in (0..10000).step_by(128) {
        let ops:Vec<_>=(base..(base+128).min(10000)).map(|i|serde_json::json!({"type":"rectangle","bounds":[(i%100)as f64*9.0,(i/100)as f64*6.0,5,3],"style":{"fill":[40,100,190,255],"stroke_width":1}})).collect();
        doc.command(|e| {
            e.apply_annotation_json(
                &serde_json::to_vec(&serde_json::json!({"version":1,"operations":ops})).unwrap(),
            )
        })
        .unwrap();
    }
    session.refresh(&doc).unwrap();
    (doc, session)
}
fn main() {
    let destination = std::env::args().nth(1).expect("output JSON path");
    let damage = std::env::args().any(|argument| argument == "--damage");
    let (mut doc, mut session) = fixture();
    let mut renderer = CanvasRenderer::default();
    renderer.set_damage_rendering(damage);
    renderer.set_profiling(true);
    let mut cold = Vec::new();
    let mut pixels = 0;
    for _ in 0..5 {
        renderer.clear_render_state();
        let image = renderer
            .render(session.scene(), None, doc.resource_revision(), 1.0)
            .unwrap();
        pixels = hash(&image);
        cold.push(renderer.last_render_profile());
    }
    let image = renderer
        .render(session.scene(), None, doc.resource_revision(), 1.0)
        .unwrap();
    let idle = renderer.last_render_profile();
    let mut conversion = Vec::new();
    for _ in 0..20 {
        let start = Instant::now();
        let mut bgra = (*image).clone();
        for p in bgra.pixels_mut() {
            p.0.swap(0, 2);
        }
        black_box(bgra);
        conversion.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let mut camera = Vec::new();
    for i in 0..10 {
        let start = Instant::now();
        session
            .set_camera(
                &mut doc,
                Camera {
                    center: Point::new(480.0 + f64::from(i) * 0.1, 320.0),
                    zoom: 1.0,
                },
            )
            .unwrap();
        let patch_ms = start.elapsed().as_secs_f64() * 1000.0;
        let image = renderer
            .render(session.scene(), None, doc.resource_revision(), 1.0)
            .unwrap();
        camera.push(serde_json::json!({"patch_ms":patch_ms,"profile":renderer.last_render_profile(),"pixel_hash":hash(&image)}));
    }
    session
        .set_camera(
            &mut doc,
            Camera {
                center: Point::new(480.0, 320.0),
                zoom: 1.0,
            },
        )
        .unwrap();
    let id = session
        .scene()
        .items()
        .iter()
        .find_map(|item| {
            if let SceneDisplayItem::Rectangle(r) = item {
                Some(ElementId {
                    index: r.id.index,
                    generation: r.id.generation,
                })
            } else {
                None
            }
        })
        .unwrap();
    session
        .mutate(&mut doc, |e, v| {
            e.select_element_with_viewport_changes(v, id)
        })
        .unwrap();
    let mut edits = Vec::new();
    for i in 0..12 {
        let start = Instant::now();
        session
            .mutate(&mut doc, |e, v| {
                e.set_selected_opacity_with_viewport_changes(v, if i % 2 == 0 { 0.5 } else { 1.0 })
            })
            .unwrap();
        let command_patch_ms = start.elapsed().as_secs_f64() * 1000.0;
        let image = renderer
            .render(session.scene(), None, doc.resource_revision(), 1.0)
            .unwrap();
        edits.push(serde_json::json!({"command_patch_ms":command_patch_ms,"profile":renderer.last_render_profile(),"pixel_hash":hash(&image)}));
    }
    let mut hover = Vec::new();
    for i in 0..120 {
        let start = Instant::now();
        session
            .input(
                &mut doc,
                InputEvent::Pointer(PointerEvent {
                    pointer_id: 1,
                    event_type: PointerEventType::Move,
                    device: PointerDevice::Mouse,
                    position: Point::new(930.0, f64::from(i)),
                    button: None,
                    buttons: PointerButtons(0),
                    modifiers: Modifiers::default(),
                    pressure: None,
                    timestamp_ms: i as u64,
                }),
            )
            .unwrap();
        let patch_ms = start.elapsed().as_secs_f64() * 1000.0;
        black_box(
            renderer
                .render(session.scene(), None, doc.resource_revision(), 1.0)
                .unwrap(),
        );
        hover.push(
            serde_json::json!({"patch_ms":patch_ms,"profile":renderer.last_render_profile()}),
        );
    }
    let mut cancellations = Vec::new();
    for _ in 0..5 {
        let scene = session.scene().clone();
        let cancel = CanvasCancellation::default();
        let worker_cancel = cancel.clone();
        let barrier = Arc::new(Barrier::new(2));
        let worker_barrier = barrier.clone();
        let worker = std::thread::spawn(move || {
            let mut renderer = CanvasRenderer::default();
            worker_barrier.wait();
            renderer.render_cancellable(&scene, None, 1, 1.0, &worker_cancel, true)
        });
        barrier.wait();
        std::thread::sleep(Duration::from_millis(5));
        let start = Instant::now();
        cancel.cancel();
        let result = worker.join().unwrap();
        cancellations.push(serde_json::json!({"cancel_to_join_ms":start.elapsed().as_secs_f64()*1000.0,"cancelled":matches!(result,Err(snow_draw_engine_gpui::CanvasError::Cancelled))}));
    }
    let mut sample = snow_draw_engine_gpui::showcase_document().unwrap();
    let mut sample_session = CanvasSession::new(&mut sample).unwrap();
    sample_session.set_surface(&mut sample, 960, 640).unwrap();
    sample_session
        .set_camera(
            &mut sample,
            Camera {
                center: Point::new(480.0, 320.0),
                zoom: 1.0,
            },
        )
        .unwrap();
    let mut sample_renderer = CanvasRenderer::default();
    sample_renderer.set_profiling(true);
    let layout_start = Instant::now();
    sample_renderer
        .prepare(&mut sample, &mut sample_session)
        .unwrap();
    let engine_label_layout_ms = layout_start.elapsed().as_secs_f64() * 1000.0;
    sample_renderer
        .render(
            sample_session.scene(),
            sample.background(),
            sample.resource_revision(),
            1.0,
        )
        .unwrap();
    let output = serde_json::json!({"workload":"10000 ordinary rectangles,100x100,5x3,spacing9x6,960x640,scale1","profile":"canvas-perf release opt3","machine":std::env::var("COMPUTERNAME").unwrap_or_default(),"cold":cold,"cold_pixel_hash":pixels,"idle":idle,"camera":camera,"repeated_single_opacity_edits":edits,"hover":hover,"rgba_to_bgra_copy_ms":conversion,"cancel_after_5ms":cancellations,"retained_vector_bytes":renderer.retained_vector_bytes(),"sample_engine_label_layout_ms":engine_label_layout_ms,"sample_profile":sample_renderer.last_render_profile(),"gpu_upload":"not measured; CPU-only run"});
    std::fs::write(destination, serde_json::to_vec_pretty(&output).unwrap()).unwrap();
    println!(
        "Dense profiling complete: {} cold frames, pixel hash {pixels}",
        cold.len()
    );
}
