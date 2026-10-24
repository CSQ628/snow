# Qt host boundaries and remaining adapter differences

Paths in the Qt column are relative to `../snow_draw_engine_qt/`. This maps public
host behavior rather than claiming identical framework APIs. The detailed drawing
matrix remains in [CAPABILITY_MATRIX.md](CAPABILITY_MATRIX.md).

| Source contract | GPUI source / status |
| --- | --- |
| `include/snow_draw_engine_qt/snow_canvas_runtime.h:67–69`, `destroyAsync()`; `src/core/snow_canvas_runtime_session.cpp::destroyAsync`, process cleanup queue | `cleanup.rs::retire_canvas_document` defers destruction on the owner thread, at most two pending documents and one drop per scheduled turn; overflow applies synchronous backpressure. View release/replacement invokes it. Immutable sources and renderer/metrics payloads retire on CPU workers. `canvas_cleanup_diagnostics` reports document queue/completed/overflow/drop timing. |
| `crates/snow-draw-engine-scene/src/scene_cache.rs:40–42`, `Mutex<Option<Rc<DuplicateScene>>>` | The preserved engine is non-`Send`; no unsafe transfer is introduced. A single engine drop is indivisible and history has no finite bound. Release measurements include100k shapes20.71–26.54ms and1000×32KiB text22.47–28.41ms. Deferral is implemented, but a hard deadline requires a safe incremental core drain API. Diagnostics measure document drops, not worker cache destruction or GPUI image/driver cleanup. |
| `snow_canvas_custom_renderer.h`, before/original/after painting and independent revisions | `host.rs::CanvasPresentation`, immutable source identities and `renderer.rs::render_presented_cancellable`; implemented snapshot composition. Borrowed painter callbacks are replaced with immutable host images for worker safety. |
| `snow_canvas_image_source_types.h`, source layers/coverage/donor eligibility | `document.rs::SourceImage::from_layers_cancellable`, `host.rs::ImageSourceLayer`; implemented separate visual/eligible composites and projection limits. |
| `snow_canvas_custom_renderer.h::SnowCanvasFilterRenderReference`, Replace/Composite | `host.rs::CanvasRenderReference` plus `CanvasPresentation::clear_background`; view composition grid/background ownership implemented. Exports recompose at output density. |
| `snow_canvas_widget.h:160–202`, text Delete/bounds, style-popup focus scope, visibility/interaction/wheel/clip/decor areas | `host.rs::CanvasViewOptions`, `CanvasPresentation`, `view.rs::set_options`, `begin/end_text_style_popup_interaction`, `view/text_input.rs`; implemented and tested. |
| `snow_canvas_runtime.h:71–83`, Smart Erase seed/freeze/worker completion | `renderer.rs::SmartEraseSnapshot`, seed/restore/unfreeze and strict snapshot exports. A worker can seed a separate renderer, export the required region, then publish/freeze its completed snapshot only on success. There is no same-named one-call Qt `prepareSmartEraseForExport` wrapper. |
| `snow_canvas_runtime.h` export sources/tiles; `snow_canvas_export_types.h` | `export.rs` snapshot/PNG/SVG/packed-tile writer APIs; implemented with bounded local halos. Source ownership is contiguous; a lazy stitch/row provider is not included. |
| `snow_canvas_widget.h:51`, `setCommandKeyResolver` | `view/host_controls.rs::set_command_key_resolver`: UI-owned command identity callback; native printable/IME contents and modifiers remain intact. Function/navigation keys can resolve during editing. Original pan key identity survives resolver replacement until release. |
| `snow_canvas_widget.h:56–62`, cursor layers and DPI-sensitive crosshair factory | `CanvasCursorLayer::{CanvasTool,Host}` overrides semantic native cursor; `set_crosshair_factory` caches validated GPUI quad geometry per scale and uses current-frame hover state. GPUI0.3.9 exposes no arbitrary bitmap/persistent-hidden cursor API: native crosshair remains the center/fallback. A custom OS bitmap cursor is an upstream/platform limit, not a stub factory. |
| `snow_canvas_widget.h:156`, `clearRenderState` | `CanvasView::clear_render_state` invalidates generations, detaches metrics payloads, and defers renderer clearing/composition to frame preparation. Display state is UI-owned; exports use independent renderers, so there is no shared renderer mutex. The previous completed image remains available until paint. Font database stays retained during reset. |
| `snow_canvas_widget.h:216`, dirty-rectangle visualization | `CanvasView::set_show_dirty_regions` paints native scene/decor red and overlay blue rectangles in logical view coordinates. `RetainedScene::{dirty_regions,overlay_dirty_regions}` exposes latest patch diagnostics. These are not a cumulative invalidation contract; actual partial rendering diffs completed scenes in `damage.rs`. |
| Snap/grid/guide/style/alignment/creation settings | `CanvasView::command`, `CanvasSession::mutate`, controlled `CanvasDocument::command`, and reused engine viewport commands; native overlays render guide geometry. The minimal demo exposes a subset of settings. |
| Qt tablet/touch input and physical IME/accessibility | Typed engine pen/touch inputs exist; native GPUI mouse provider has no pressure. Physical tablet/touch, Windows candidate-window and screen-reader qualification remain open. |

Host controls have focused offscreen GPUI coverage, including live text remapping,
DPI factory caching, document-history neutrality, mutex-independent reset, and
owner-thread retirement/backpressure. See [HOST_AND_DAMAGE_PLAN.md](HOST_AND_DAMAGE_PLAN.md)
for design gates and [RETAINED_DAMAGE_AND_CLEANUP.md](RETAINED_DAMAGE_AND_CLEANUP.md)
for measured behavior and remaining platform/core limits.
