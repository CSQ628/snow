//! Native message queue -> GPUI CanvasView -> Present1 submission benchmark.
#[cfg(target_os = "windows")]
mod native {
    use anyhow::{Context as _, Result, ensure};
    use gpui::{prelude::*, *};
    use snow_draw_engine_gpui::{CanvasDocument, CanvasSession, CanvasView, engine as draw};
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant as WallInstant},
    };
    use windows::{
        Win32::{
            Foundation::{HWND, LPARAM, WPARAM},
            UI::WindowsAndMessaging::*,
        },
        core::HSTRING,
    };

    #[derive(Default)]
    struct InputState {
        handled: u64,
        callback_started: Option<WallInstant>,
    }

    struct BenchView {
        canvas: Entity<CanvasView>,
        input: Rc<RefCell<InputState>>,
    }
    impl Render for BenchView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(self.canvas.clone())
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key != "f24" {
                        return;
                    }
                    let opacity = {
                        let mut input = this.input.borrow_mut();
                        input.callback_started = Some(WallInstant::now());
                        input.handled += 1;
                        if input.handled.is_multiple_of(2) {
                            1.0
                        } else {
                            0.5
                        }
                    };
                    this.canvas.update(cx, |canvas, cx| {
                        canvas.command(
                            |engine, viewport| {
                                engine.set_selected_opacity_with_viewport_changes(viewport, opacity)
                            },
                            window,
                            cx,
                        )
                    });
                    cx.stop_propagation();
                }))
        }
    }

    fn fixture() -> Result<(CanvasDocument, draw::ElementId)> {
        let mut document = CanvasDocument::default();
        for start in (0..10_000).step_by(128) {
            let operations: Vec<_> = (start..(start + 128).min(10_000)).map(|index| serde_json::json!({
                "type":"rectangle", "bounds":[(index % 100) as f64 * 9.6, (index / 100) as f64 * 6.4, 5, 3],
                "style":{"fill":[40,100,190,255],"stroke_width":1}
            })).collect();
            let bytes =
                serde_json::to_vec(&serde_json::json!({"version":1,"operations":operations}))?;
            document.command(|engine| engine.apply_annotation_json(&bytes))?;
        }
        let mut session = CanvasSession::new(&mut document)?;
        session.flush(&document)?;
        let node = session
            .scene()
            .nodes()
            .find(|(_, node)| matches!(node.item.as_ref(), draw::SceneDisplayItem::Rectangle(_)))
            .context("rectangle fixture")?
            .1;
        let selected = draw::ElementId {
            index: node.id.element.index,
            generation: node.id.element.generation,
        };
        ensure!(
            session.close(&mut document)?.is_none(),
            "fixture viewport held pointer capture"
        );
        Ok((document, selected))
    }

    async fn presented(
        service: &Arc<dyn GpuService>,
        count: u64,
        after: WallInstant,
        cx: &AsyncApp,
    ) -> Result<GpuServiceDiagnostics> {
        let deadline = WallInstant::now() + Duration::from_secs(30);
        loop {
            let diagnostics = service.diagnostics();
            if diagnostics.presentations > count
                && diagnostics
                    .last_presentation_end
                    .is_some_and(|end| end >= after)
            {
                return Ok(diagnostics);
            }
            ensure!(
                WallInstant::now() < deadline,
                "native presentation timed out"
            );
            cx.background_executor()
                .timer(Duration::from_millis(1))
                .await;
        }
    }

    fn native_handle(title: &str) -> Result<HWND> {
        unsafe { FindWindowW(None, &HSTRING::from(title)) }.context("finding benchmark HWND")
    }

    fn percentile(values: &mut [f64], percent: usize) -> f64 {
        values.sort_by(f64::total_cmp);
        values[(values.len() * percent).div_ceil(100).saturating_sub(1)]
    }

    async fn measure(
        window: WindowHandle<BenchView>,
        selected: draw::ElementId,
        title: String,
        output: std::path::PathBuf,
        cx: &mut AsyncApp,
    ) -> Result<()> {
        cx.background_executor()
            .timer(Duration::from_millis(300))
            .await;
        let initial = WallInstant::now();
        let (service, input, refresh, scale, logical_size) =
            window.update(cx, |root, window, cx| {
                let canvas = root.canvas.clone();
                let input = root.input.clone();
                canvas.update(cx, |canvas, cx| {
                    canvas.fit(window, cx);
                    canvas.command(
                        |engine, viewport| {
                            engine.select_element_with_viewport_changes(viewport, selected)
                        },
                        window,
                        cx,
                    );
                    canvas.focus_handle(cx).focus(window, cx);
                });
                (
                    window.gpu_service(),
                    input,
                    window.refresh_interval(cx),
                    window.scale_factor(),
                    window.viewport_size(),
                )
            })?;
        let service = service?;
        presented(&service, 0, initial, cx).await?;
        let display_start = service.diagnostics();
        let handle = native_handle(&title)?;
        let mut samples = Vec::new();
        let mut queue_latencies = Vec::new();
        let mut callback_latencies = Vec::new();
        for index in 0..72 {
            let before = service.diagnostics();
            let expected = input.borrow().handled + 1;
            let posted = WallInstant::now();
            unsafe {
                PostMessageW(Some(handle), WM_KEYDOWN, WPARAM(0x87), LPARAM(1))?;
                PostMessageW(Some(handle), WM_KEYUP, WPARAM(0x87), LPARAM(0xc0000001))?;
            }
            let deadline = posted + Duration::from_secs(30);
            let callback = loop {
                {
                    let input = input.borrow();
                    if input.handled >= expected {
                        break input.callback_started.context("input timestamp")?;
                    }
                }
                ensure!(
                    WallInstant::now() < deadline,
                    "F24 native input was not dispatched"
                );
                cx.background_executor()
                    .timer(Duration::from_millis(1))
                    .await;
            };
            let after = presented(&service, before.presentations, callback, cx).await?;
            let end = after
                .last_presentation_end
                .context("presentation timestamp")?;
            let queued_ms = end.duration_since(posted).as_secs_f64() * 1000.0;
            let callback_ms = end.duration_since(callback).as_secs_f64() * 1000.0;
            if index >= 8 {
                queue_latencies.push(queued_ms);
                callback_latencies.push(callback_ms);
            }
            samples.push(serde_json::json!({
                "native_queue_to_present_submission_ms": queued_ms,
                "native_callback_to_present_submission_ms": callback_ms,
                "compositor_cpu_ms": (after.compositor_cpu_ns - before.compositor_cpu_ns) as f64 / 1e6,
                "present_call_cpu_ms": (after.present_cpu_ns - before.present_cpu_ns) as f64 / 1e6,
                "presented_pixels": after.presented_pixels - before.presented_pixels,
                "full_presentations": after.full_presentations - before.full_presentations,
            }));
        }
        let error = window.read_with(cx, |root, cx| {
            root.canvas.read(cx).error().map(ToString::to_string)
        })?;
        ensure!(error.is_none(), "Canvas error: {error:?}");
        let idle_before = service.diagnostics();
        cx.background_executor()
            .timer(Duration::from_millis(200))
            .await;
        let idle_after = service.diagnostics();
        ensure!(
            idle_after.presentations == idle_before.presentations,
            "idle window submitted a new presentation"
        );

        let before_resize = service.diagnostics();
        let resized_at = WallInstant::now();
        window.update(cx, |_, window, _| {
            window.resize(size(px(1152.0), px(680.0)))
        })?;
        let resized = presented(&service, before_resize.presentations, resized_at, cx).await?;
        ensure!(
            resized.full_presentations > before_resize.full_presentations,
            "resize did not reconstruct the window"
        );
        let _was_visible_before_minimize = unsafe { ShowWindow(handle, SW_MINIMIZE) };
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
        let before_restore = service.diagnostics();
        let restored_at = WallInstant::now();
        let _was_visible_before_restore = unsafe { ShowWindow(handle, SW_RESTORE) };
        let restored = presented(&service, before_restore.presentations, restored_at, cx).await?;
        ensure!(
            restored.full_presentations > before_restore.full_presentations,
            "restore did not reconstruct the window"
        );

        let queue_p95 = percentile(&mut queue_latencies, 95);
        let callback_p95 = percentile(&mut callback_latencies, 95);
        let capabilities = service.capabilities();
        let report = serde_json::json!({
            "profile":"canvas-perf", "elements":10_000, "warmup_samples":8,
            "adapter":{"name":capabilities.adapter_name.as_ref(), "vendor_id":capabilities.vendor_id, "device_id":capabilities.device_id},
            "logical_size":[f32::from(logical_size.width), f32::from(logical_size.height)], "dpi_scale":scale,
            "physical_size":[f32::from(logical_size.width)*scale, f32::from(logical_size.height)*scale],
            "refresh_interval_ms":refresh.map(|interval| interval.as_secs_f64()*1000.0),
            "native_queue_to_present_submission_p95_ms":queue_p95,
            "native_callback_to_present_submission_p95_ms":callback_p95,
            "target_ms":8.33, "meets_native_queue_target":queue_p95<=8.33,
            "idle_presentations":idle_after.presentations-idle_before.presentations,
            "resize_and_minimize_restore":"passed",
            "startup_readback_bytes":display_start.readback_bytes,
            "display_readback_bytes":service.diagnostics().readback_bytes - display_start.readback_bytes,
            "measurement":"Posted WM_KEYDOWN -> GPUI native dispatch -> CanvasView edit -> Present1 return. Excludes physical scanout; GPU retirement is measured by gpu_profile.",
            "samples":samples,
        });
        std::fs::write(output, serde_json::to_vec_pretty(&report)?)?;
        Ok(())
    }

    pub fn run() -> Result<()> {
        let output = std::env::args_os()
            .nth(1)
            .context("output JSON path")?
            .into();
        let (document, selected) = fixture()?;
        let failed = Arc::new(AtomicBool::new(false));
        let exit_failed = failed.clone();
        let title = format!("Snow native GPU input benchmark {}", std::process::id());
        gpui_platform::application().run(move |cx| {
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                            None,
                            size(px(1280.0), px(720.0)),
                            cx,
                        ))),
                        titlebar: Some(TitlebarOptions {
                            title: Some(title.clone().into()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    |window, cx| {
                        cx.new(|cx| BenchView {
                            canvas: cx.new(|cx| {
                                CanvasView::new(document, window, cx)
                                    .expect("canvas initialization")
                            }),
                            input: Rc::default(),
                        })
                    },
                )
                .expect("native benchmark window");
            cx.activate(true);
            cx.spawn(async move |cx| {
                if let Err(error) = measure(window, selected, title, output, cx).await {
                    failed.store(true, Ordering::Relaxed);
                    eprintln!("Native input benchmark failed: {error:#}");
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
        ensure!(
            !exit_failed.load(Ordering::Relaxed),
            "native input benchmark failed"
        );
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    native::run()
}
#[cfg(not(target_os = "windows"))]
fn main() {
    panic!("native_input currently requires the Windows GPU backend");
}
