//! Manual CPU-only measurement; run this test with the Release canvas-perf profile.
use crate::{engine::*, *};
use std::{hint::black_box, time::Instant};

fn scene(items: Vec<SceneDisplayItem>, revision: u64) -> RetainedScene {
    let mut patch = ViewportPatch {
        frame_view: FrameView {
            camera: Camera {
                center: Point::new(480.0, 320.0),
                zoom: 1.0,
            },
            surface: SurfaceSize {
                width: 960,
                height: 640,
            },
            ..Default::default()
        },
        ..Default::default()
    };
    patch.scene.revision = revision;
    patch.scene.ops.push(ReplaceRangeOp {
        start: 0,
        delete_count: 0,
        insert_items: items,
    });
    let mut scene = RetainedScene::default();
    scene.apply(&patch).unwrap();
    scene
}

#[test]
#[ignore = "manual performance measurement: use --profile canvas-perf"]
fn interactive_damage_frame_cost() {
    if cfg!(debug_assertions) {
        panic!("benchmark requires an optimized Release profile");
    }
    let source = SourceImage::new(image::RgbaImage::from_fn(960, 640, |x, y| {
        image::Rgba([(x % 251) as u8, (y % 241) as u8, 140, 220])
    }))
    .unwrap();
    let mut report = Vec::new();
    for count in [0, 1000, 10000] {
        let base: Vec<_> = (0..count)
            .map(|i| {
                SceneDisplayItem::Rectangle(RectangleDisplayItem {
                    id: DisplayItemId {
                        index: i,
                        generation: 1,
                    },
                    center_x: f64::from(i % 100) * 9.0 + 5.0,
                    center_y: f64::from(i / 100) * 6.0 + 5.0,
                    width: 5.0,
                    height: 3.0,
                    fill: ColorRgba8 {
                        r: 30,
                        g: 90,
                        b: 180,
                        a: 180,
                    },
                    fill_style: DisplayFillStyle::Solid,
                    ..Default::default()
                })
            })
            .collect();
        let mut reference_hashes = Vec::new();
        for damage in [false, true] {
            let mut renderer = CanvasRenderer::default();
            renderer.set_damage_rendering(damage);
            renderer.set_profiling(true);
            renderer
                .render(&scene(base.clone(), 1), Some(&source), 1, 1.25)
                .unwrap();
            let mut samples = Vec::new();
            let mut partial = 0;
            for step in 0_u32..16 {
                let mut items = base.clone();
                if step != 15 {
                    items.push(SceneDisplayItem::Rectangle(RectangleDisplayItem {
                        id: DisplayItemId {
                            index: 10001,
                            generation: 1,
                        },
                        center_x: 450.0 + f64::from(step) * 2.0,
                        center_y: 300.0,
                        width: 140.0 + f64::from(step),
                        height: 85.0,
                        rotation: f64::from(step) * 0.03,
                        fill: ColorRgba8 {
                            r: 220,
                            g: 70,
                            b: 100,
                            a: 180,
                        },
                        fill_style: DisplayFillStyle::Solid,
                        ..Default::default()
                    }));
                }
                let next = scene(items, u64::from(step) + 2);
                let start = Instant::now();
                let image = renderer.render(&next, Some(&source), 1, 1.25).unwrap();
                // Include the same full RGBA copy/channel conversion used for GPUI upload.
                let mut bgra = (*image).clone();
                for pixel in bgra.pixels_mut() {
                    pixel.0.swap(0, 2);
                }
                black_box(&bgra);
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
                partial += usize::from(renderer.last_render_profile().partial_recomposition);
                let hash = image
                    .as_raw()
                    .iter()
                    .fold(0xcbf29ce484222325u64, |hash, byte| {
                        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
                    });
                if damage {
                    assert_eq!(hash, reference_hashes[step as usize]);
                } else {
                    reference_hashes.push(hash);
                }
            }
            samples.sort_by(f64::total_cmp);
            report.push(serde_json::json!({"elements":count,"damage":damage,
                "samples":samples.len(),"partial_frames":partial,
                "min_ms":samples[0],"median_ms":samples[samples.len()/2],
                "max_ms":samples[samples.len()-1]}));
        }
    }
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
