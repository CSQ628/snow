# Source capability and implementation matrix

Updated 2026-10-24. This distinguishes functionality, regression coverage, and
platform qualification. “Implemented” does not claim pixel identity with QPainter
or certification of physical input hardware. All source references below are
relative to `../snow_draw_engine_qt/` unless explicitly local.

The reusable engine/editor/model/geometry/display crates are path dependencies and
are unchanged. Their existing tests are architectural evidence; this task ran only
the new package's related tests, not the repository-wide suite.

## Drawing and editing

| Discovered capability / source evidence | GPUI implementation | Verification and boundaries |
| --- | --- | --- |
| Rectangle/ellipse/diamond, independent corner radii, rotation (`document`, rounded diamond geometry) | Implemented in retained scene and `vector.rs`, using engine geometry | Showcase, pixel/export tests, native draw/resize; rounded diamond uses the engine path constructor. |
| Straight/curved/elbow arrows, shaft/head variants and bindings (`core/arrow`, editor arrow workflow) | Implemented from published path/head primitives; attached-label gap | Arrow alpha/outline regression, shared engine workflows; not every head/style combination has a separate native capture. |
| Chunked freehand, pen pressure, stabilization and continuation (`core/path`, editor free draw) | Implemented; retained typed geometry patches and pressure outlines | 200-sample patch/full-scene equality; 30,000-input optimized benchmark; cancellation/repeated gesture tests. Windows mouse events carry no pressure. |
| Solid/dash/dot, fill/border/hatch/crosshatch, opacity, multiply highlights | Implemented | Renderer/export fixtures; rough Artist/Cartoonist styles use seeded SVG displacement, a visual approximation of the Qt rough-stroke algorithm. |
| Text, fonts/fallback, bold/italic, alignment, wrapping, background and outline (`editor/text`, Qt `src/text`) | Implemented; font bytes retained within a 64 MiB budget; long-text metrics separated from outline rasterization | UTF-16/surrogate, bidi/combining, Hebrew visual caret/hit-test/selection in short and long paths, grapheme/ZWJ, long wrapping, metrics/outline agreement, style-only undo, composition tests. Plain uniformly styled text, as in the engine; no arbitrary rich-text spans. |
| Native inline editing, preedit selection/caret, candidate rectangle and clipboard | Implemented through GPUI `EntityInputHandler`, with rotated coordinate mapping | GPUI protocol tests and native Chinese/emoji preedit capture. Physical Windows candidate-window interaction and screen-reader behavior remain unqualified. |
| Draft commit/cancel, no-op existing edit, local undo | Implemented separately from document history | No-op edit preserves a narrow wrap box; complete IME edit is one document undo; rejected replacements preserve draft/preedit/history. Limit: 64 KiB UTF-8 per edited text. |
| Shape/serial/arrow/bubble labels, generated distance/angle labels | Implemented engine request/measurement responses, label editing and serial Create Text API | Showcase generated measurements; bound-label synchronization, template and text tests. Enter opens selected label; serial creation uses the engine command. |
| Serial types/formats/increment/connectors | Implemented via engine and scene; solid badges force solid fill and choose contrasting text | Solid light badge regression, showcase numbers, shared formatting/label engine APIs. |
| Speech bubble body/tail and handles | Implemented from engine body/tail path; attached text uses host layout measurements | Scene lowering and shared interaction state machine. Tail handling is available through the tool; no separate physical-input qualification. |
| Distance/angle multi-stage tools and value/unit styles | Implemented through shared editor; generated text measured by renderer | Showcase, roundtrip/render tests; style commands exposed through controlled engine API. |
| Hit testing, rectangular marquee, shift multiselection, quick selection | Implemented through shared indexed editor | Native selection/resize, offscreen rotation/multiselection/template tests. No implemented lasso workflow was found in the baseline. |
| Move/resize/rotate/duplicate drag, constraints, radii, binding geometry | Implemented through engine input/commands | Interrupted/repeated gestures, outside release, native resize, rotation undo/redo, template regeneration. |
| Selection controls and feedback (`snow_canvas_renderer.cpp`, `snow_canvas_widget_selection_hit_testing.cpp`) | Qt rounded resize handles, circular radius/rotation controls, dashed group/text/magnifier frames, text/tail plus symbols, magnifier grip, hollow pen-filter contours, and snap ticks/dashes; frame edges and text move rings remain interactive outside the content clip | Offscreen production-path coverage at multiple zooms/rotations, clipped dash phase on huge selections, intersecting pen contours; clipped edge resize and text move, control workflows and undo in `selection_controls.rs`. |
| Z-order, align/distribute, selected opacity | Implemented controlled engine commands, with demo front/back and duplicate controls | Source engine commands and history/template tests; minimal demo does not expose every command as a button. No user grouping workflow exists in the baseline despite the unused `GroupMember` declaration. |

## Effects and source resources

| Discovered capability / source evidence | GPUI implementation | Verification and boundaries |
| --- | --- | --- |
| Rectangular/pen mosaic, Gaussian blur, grayscale, inversion, emboss and brightness | Implemented ordered immutable source passes and effect-run masks in `renderer.rs` | Immutable sibling backdrop, crop/full equivalence at fractional scale, premultiplied blur/brightness, canvas-anchored mosaic tests. Blur/emboss are native Rust kernels; no byte-for-byte OpenCV/QPainter parity claim. |
| Object eraser and rectangle/brush Restore Background | Implemented as distinct engine tools | Restore replaces all premultiplied channels, including alpha; transparent original removes annotations. Covered by compositing and history tests. |
| Smart Erase (`src/rendering/snow_canvas_smart_erase_algorithm.cpp`) | Implemented affine/periodic paths, Lab/structural guides, conservative pyramid and multiscale PatchMatch | Flat/affine, periodic, noisy bands/remote panel, diagonal/texture-diversity, alpha/known-byte invariance, masked-color independence, cancellation and memory-limit tests. This is inpainting, not blur or harmonic fill. Scalar kernels, random sequence and edge details differ from Qt. |
| Smart Erase asynchronous preview/cache/export lifecycle | Live masks paint with current controls; one reconstruction worker + latest request supplies source/geometry-keyed results, with cooperative cancellation. Workers never replace display frames; strict exports reject unresolved previews/errors | Camera/opacity-independent result reuse, frozen/shared cache snapshots, no-donor/error visibility and strict export tests. Budget: 512 MiB conservative scratch estimate, 2 Mi-pixel solve ROI, 128 MiB result cache. Results retain affected pixels plus sampling guards, not donor halos; 20 small results reuse across cameras. Large unsupported solve regions fail explicitly. |
| Magnifier lens/source/bridge/outlines, wheel factor | Implemented pristine visual-source sampling and engine geometry | Translated/rotated contour and gradient regression. Axis-aligned source mapping agrees with the discovered Qt contract. |
| Spotlight cutouts and watermark | Implemented before scene / after scene composition respectively; per-view render areas | Showcase/tool controls, renderer scene path and snapshot exports. Watermark font/raster placement is backend-specific. |
| Auto Filter recognized regions/categories | Implemented host data path through engine | Demo installs clearly labelled sample regions; no OCR service is included or claimed. |
| Immutable image assets and asynchronous decode | Implemented bounded PNG/JPEG/WebP decode, native placement and revisioning | Source/session roundtrip, malformed/oversize rejection, atomic document replacement. No editable engine image-node primitive exists; payload-free legacy scene placeholders return an error. |
| Multiple original image layers, coverage and donor eligibility (`snow_canvas_image_source_types.h`) | Implemented `ImageSourceLayer` and bounded, cancellable `SourceImage::from_layers`; independent X/Y placement | Separate visual and eligible reconstruction composites preserve valid donors under visual-only overlays. Both persist. Effective coverage bounds and per-layer projection avoid the whole-output × layer cross product. |
| Source replacement/history | Implemented `replace_background`, ordered with engine host-history tokens | Source/drawing/host-token interleave undo/redo test. Source-history snapshots are runtime-only, matching engine host-token persistence. Current source is saved; retained assets have a 1 GiB/128-distinct-image budget (clones are deduplicated). `set_background` is the explicit untracked initialization/update API. |
| Source lifetime and cache disposal | Implemented immutable `Arc` resources, stable pixel identities, bounded tree/layout/result caches and explicit clear hooks | Cache/revision/stale job tests; native image replacement/drop hooks; source identity survives clones and cannot alias a recycled allocation address. |

## Host integration, persistence and export

| Discovered capability / source evidence | GPUI implementation | Verification and boundaries |
| --- | --- | --- |
| Camera/pan/zoom/DPI and multiple viewport ownership | Implemented separate `CanvasSession`; logical input and source-grid presentation | Precise camera roundtrip, 1×/1.5×/2× test scale contracts, native 1.5× capture/pan/zoom; idle view closure cannot steal another view's gesture. |
| Capture/cancel/focus/tool switch and mouse/keyboard shortcuts | Implemented GPUI hitbox capture, weak listeners and typed engine input | Escape, focus loss, outside release, interrupted/repeated actions, undo/redo; no idle repaint timer. Pen/touch typed inputs are available; native pressure/touch hardware provider remains a host integration requirement. |
| Custom before/original/after host painting, separate revisions (`snow_canvas_custom_renderer.h`) | Implemented immutable `CanvasPresentation` before/after/override snapshots; pristine visual/eligible sources remain separate | Host-stage/backdrop tests. Publish new immutable image identities for changed content; changing a temporary presentation does not replace pristine source/cache identity. Custom GPUI elements can surround the canvas; no borrowed QPainter callback crosses worker threads. |
| Replace/Composite and fixed reference presentation | Implemented opaque clear/background replacement or transparent output over a GPUI-owned background; `CanvasRenderReference` selects view composition rectangle/density | `clear_background` plus before/override/after stages. Automatic and explicit reference queries produce equivalent exports. Export recomposes at requested output density; it does not resample the view's retained raster. |
| Content visibility, clip/gesture bounds and separate decoration areas (`widget.h`) | Implemented `CanvasViewOptions` / `CanvasPresentation` | Presentation clip/tile tests; existing selection controls and editing remain usable outside the clip. Options are view state and do not modify document history. |
| Text editing bounds, Delete mode, style-popup focus scope (`widget.h`) | Implemented bounded automatic-width draft layout, target-preserving empty commit and nested popup interaction API | Text bounds, style-only commit/undo, owner-preserving Delete and popup draft tests. Host must bracket its popup with begin/end calls. |
| Clipboard drawing templates / native text / image PNG | Implemented regenerated IDs/bindings, native text clipboard, asynchronous strict PNG snapshot | Template undo/redo, text and PNG tests across scales. Protocol/Windows clipboard support is distinct from physical OS paste application qualification. |
| Versioned session persistence/history/resources | Implemented `.snowdraw` JSON envelope with engine session and lossless visual/eligible PNGs | Roundtrip retains drawing history/current resources; invalid import leaves the active document intact. Send-able `DrawingSnapshot` moves image codecs to workers. Files capped at 512 MiB; paired configurable save/load asset budgets prevent writing an unreopenable file; streamed writes use a complete sibling temporary and atomic rename; cancellation preserves the previous target. |
| PNG export, bounds/scale, no transient overlays | Implemented immutable `CanvasSnapshot` with complete downstream effect support | 1.25×/1.5× odd pixel grid regression, viewport crop invariance across three blur passes, source freshness and live-erase strictness. Scale 0.1–8. Sources/output may exceed the 16 Mi-pixel/16,384 working-tile limit: writer APIs stream up to 8 GiB RGBA with bounded scanlines/bands/halos. Exact boundaries, fractional dimensions, tall output and multi-pass adaptive queries are tested; see LARGE_IMAGES.md. |
| SVG export | Implemented paths and portable outlined text for plain scenes; lossless embedded PNG for backdrop effects/custom presentation | SVG parse/raster roundtrip asserts actual vector paths/no text/no raster for plain documents; effect fallback embedded pixels match. Effect documents are intentionally flattened; large sources/output also use the streamed lossless composite. Streaming SVG roundtrips its actual embedded PNG; plain vector assertions remain intact. |
| Disjoint-tile output / independent export backdrop (`runtime.h`) | Implemented `export_snapshot_tiles_png` and snapshot presentation override | Packed rectangles equal corresponding full-frame pixels, preserving filter halos/grid. Streaming composes only requested source regions with locally accumulated halos, preserving pre-cull pass identities and original pixel grid. Packed output and PNG/SVG writer APIs avoid full output RGBA allocation. |
| Share/seed/freeze completed Smart Erase results (`runtime.h`) | Implemented immutable `SmartEraseSnapshot`, seed/restore/unfreeze APIs | Frozen matching geometry succeeds; missing geometry fails; source pixels, placement and native geometry participate in matching. Snapshots share immutable buffers rather than copying them. |

Host customization/diagnostics are implemented in `view/host_controls.rs`:
command-key resolver, prioritized native cursor layers/DPI crosshair geometry,
frame-deferred one-call view cache reset, and dirty-region debug overlays. Offscreen
tests cover editing/native text separation, release after resolver replacement,
DPI caching, history neutrality, and reset without rendering in the host callback.
Final native readback verifies all four custom crosshair arms, unchanged canvas
pixels across light/dark theme, history-neutral cache reset, native text protocol,
repeated alpha edits and completed owner-thread document retirement. The fourteen
checks and six original-resolution captures are recorded in `VALIDATION.md`.
Exact source contracts and remaining platform differences are in
[HOST_API_MAP.md](HOST_API_MAP.md).

## Remaining differences and qualification

No discovered drawing tool is replaced by a no-op stub. The limitations above are
deliberate backend/resource contracts or qualification boundaries. The following
are not claimed as completed parity:

- Physical Windows IME candidate-window, accessibility/screen-reader, pen-pressure
  and touch hardware certification; only supported GPUI dispatch/protocol tests ran.
- QPainter-identical font metrics, rough strokes, blur/emboss and Smart Erase pixels.
- Effect neighborhoods exceeding the documented per-tile/scratch budgets; contiguous
  source buffers beyond 8 GiB, or output beyond the documented coordinate/scanline
  limits. Total exports are no longer restricted to one working surface.
- Deferred engine retirement is implemented with an owner-thread queue capped at
  two documents, one drop per scheduled turn and synchronous overflow backpressure.
  Image/cache payloads retire on workers. The reused `Rc` engine has no incremental
  drain API or finite history bound, so one engine drop has no hard deadline.
  Arbitrary OS bitmap/persistent-hidden cursors are unavailable in GPUI0.3.9;
  custom crosshair geometry uses native GPUI quads with its native cursor center.
- GPU tessellation/backdrop-filter composition. Document rendering is retained CPU
  composition during frame preparation; native GPUI draws the view, controls and
  transient overlays from that same scene. Smart Erase reconstruction stays on a worker.
  Enlarged source-grid text can appear softer than native glyph rasterization.

Use the validation record for measured costs. A cached render timing is a cache hit,
not evidence that a cold dense document renders at that rate.

Retained partial composition is enabled for safe Architect source-over rectangles
and arrows with unchanged frame/resources and survivor order, including insertion
and removal; it diffs the last completed scene,
rebuilds intersecting paint dependencies, and publishes atomically. Text, effects,
multiply, decorations, broad edits and unsafe f32 coordinates use full rendering.
Pixel/cancellation/fallback regressions and matched optimized measurements are in
[RETAINED_DAMAGE_AND_CLEANUP.md](RETAINED_DAMAGE_AND_CLEANUP.md).
Frame synchronization and current interaction measurements are in
[INTERACTIVE_FRAME_SYNC.md](INTERACTIVE_FRAME_SYNC.md).
