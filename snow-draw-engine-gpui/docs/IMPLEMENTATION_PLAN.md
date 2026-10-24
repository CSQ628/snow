# Snow Draw Engine: GPUI integration plan

Date: 2026-10-23. Target: `snow-draw-engine-gpui` (corrected by the user after the initial plan).
This is a new Rust project beside the existing `snow_draw_engine_qt` Qt project.
The existing Qt implementation and its Rust workspace remain authoritative and untouched.

## Baseline and evidence

Repository instructions: root `AGENTS.md`; no nested instructions apply to this new directory.
The working tree was clean using `git status --ignore-submodules=all` at the start.
Ordinary status cannot read `.git/modules/simd-paddle-ocr-rs` (permission denied).
No attempt is made to circumvent that restriction. Neither reference checkout is modified.

The current `D:/snow-apps-workspace/snow-apps/ant-design-gpui/Cargo.toml` and
`Cargo.lock` both resolve `gpui-pre = 0.3.9` and `gpui-pre-platform = 0.3.9`.
The platform feature set is `font-kit`, `x11`, `wayland`, `runtime_shaders`;
development enables `test-support` on both. Match those versions/features exactly.
Do not link the unfinished Ant component library or use its source/output directory.
Use an isolated Cargo target directory and bounded build jobs.

Architectural reference: `D:/gpui-kit/AGENTS.md`, `skills/gpui-kit/SKILL.md`,
`skills/gpui-kit-design-guides/SKILL.md`, and the canonical coding/design guides.
Apply retained entities, explicit state ownership, stable element IDs, scoped subscriptions,
weak async completion handles, native focus/input, and demand-driven invalidation.
This library deliberately uses GPUI directly: the user's exact dependency/version request
and independence from another evolving UI library take precedence over Kit's umbrella policy.

## Existing architecture

The Rust crates in `snow_draw_engine_qt/crates/` already separate the reusable layers:

| Layer | Responsibilities / evidence |
| --- | --- |
| `snow-draw-engine-core` | Geometry/camera conversions (`geometry.rs`), chunked paths (`path.rs`), pressure geometry, snapping, arrow primitives and render styles. |
| `snow-draw-engine-document` | Stable generational `ElementId`, typed element records, paint order, bounds/hit testing, bindings, validation, transactional operations (`document.rs`, `transaction.rs`). |
| `snow-draw-engine-model` | Indexed document access, spatial/relation/snap indices (`query/`). |
| `snow-draw-engine-interaction` | Host-neutral pointer/wheel/key/focus events, capture and cursor commands (`lib.rs`). |
| `snow-draw-engine-editor` | Tools, gesture state machines, previews, selection/resize/rotation, styles, text draft/layout contracts, bindings and annotation workflows. |
| `snow-draw-engine-scene` | Retained document scene, viewport culling, overlay/decorations, ordered source/effect runs, damage, range and geometry patches. |
| `snow-draw-engine` | `Engine`, multi-viewport sessions, shared history, atomic host operations, serialization, text measurement requests (`engine/`, `history.rs`, `session.rs`). |
| `snow-draw-engine-display` | Renderer-neutral scene, overlay, decoration and incremental patch protocol (`lib.rs`). |
| `snow-draw-engine-c` | C ABI for Qt; unnecessary for a Rust GPUI host. |

Qt supplies platform-specific functionality absent from the Rust model:
`src/core` adapts lifecycle/input/runtime handles; `src/text` owns the QTextEdit/IME overlay
and font metrics; `src/rendering` owns QPainter, images, ordered filter composition,
pressure/filter mask caches, magnifier sampling, exports and smart-erase workers.
`qt-demo/demo_window.cpp` exercises the public widget/runtime APIs. A renderer rewrite
must account for these responsibilities; linking `Engine` alone is not feature parity.

## Capability inventory and acceptance map

Status begins as **planned**. The completed implementation must update each row with
implemented/tested/deferred/blocked and concrete verification. Unsupported features
must be visible in documentation/API errors, never silently rendered as a different effect.

| Capability | Existing evidence | GPUI acceptance |
| --- | --- | --- |
| Rectangle, ellipse, diamond, rounded corners | `document.rs: RectangleData, HighlightShape`; rounded diamond engine tests | All shapes, rotation, individual radii, fill/border and hit geometry agree. |
| Lines/arrows | `document/arrow/`; `ArrowDisplayItem` | Straight/curved/elbow routing, bindings, focus/end/segment handles, head primitives, shaft variants and labels. |
| Freehand | `free_draw.rs`, `free_draw_pressure.rs`, `core/path.rs` | Stabilized/chunked paths, straight/curve continuation, pressure outlines, interrupted/repeated strokes. |
| Stroke/fill styles | `core/arrow.rs`, `stroke_rendering.rs`, `DisplayFillStyle` | Solid/dashed/dotted, rough styles, hatching/crosshatching, opacity and multiply highlights. |
| Text and labels | `editor/text`, `engine/text_commands.rs`, Qt `src/text` | Unicode, fallback fonts, bold/italic, wrapping/alignment, backgrounds, native IME, selection, edit/commit/cancel and one history entry per edit. |
| Shape/arrow/bubble/serial text bindings | `shape_text.rs`, `arrow_text.rs`, `speech_bubble.rs` | Host metrics supplied through engine request/response APIs; preserve ownership during transforms/history. |
| Serial numbers | `SerialNumberData`, `serial_number_format.rs` | Circle/square/solid/outline types, formats, increment, attached labels/connectors. |
| Distance and angle | `distance.rs`, `angle.rs`, engine tests | Real multi-stage creation, generated labels, unit/value controls and selection. |
| Speech bubbles | `speech_bubble.rs`, `SpeechBubbleDisplayItem` | Body and tail path, tail handles and text layout. |
| Selection | `editor/selection.rs`, `input/quick_selection.rs` | Hit testing, rectangular marquee, shift/multiple, quick selection, cancellation. No lasso workflow was found. |
| Transforms | `geometry/resize.rs`, `edit_workflow/` | Move/duplicate drag/resize/rotate, constraints, corner radius, multi-selection bounds. |
| Stacking/alignment | `engine/document_commands.rs` | Front/back/forward/backward, alignment/distribution and opacity via existing commands. `GroupMember` is declared but no user grouping workflow is implemented. |
| Viewport | `core/geometry.rs`, `engine/viewports.rs` | DPI-correct logical input and physical rendering, pointer zoom anchor, pan, reset/fit, multiple viewport ownership. |
| Images | Qt image source/export APIs; `SceneDisplayItem::Image` has no payload | Host-owned base image, asynchronous decode, bounded pixels, background restore, save/load resource preservation. Arbitrary editable image elements are not existing functionality. |
| Mosaic/blur/color effects | `DisplayFilterType`, Qt filter renderer/tests | Ordered composition, rectangular/pen masks, rotation, opacity; process earlier scene content and source padding, preserve effect-run semantics. |
| Erasers | `eraser_workflow.rs`, Qt eraser tests | Object eraser versus rectangle/brush background restore are distinct tools, fully undoable. |
| Smart erase | Qt smart-erase async pipeline, `tests/snow_canvas_smart_erase.md` | Explicit provider boundary with versioned source/mask, cancellation/stale results and bounded memory. Do not label blur or a fill as inpainting parity. |
| Magnifier | `magnifier.rs`, Qt magnifier renderer/tests | Lens/source geometry, sampling pristine host base-image layers, connections/outlines and wheel factor. |
| Spotlight/watermark | document/decorations and Qt renderers | All cutout shapes and tiled text with rotation/gaps; export consistently. |
| Auto filter | `auto_filter.rs`, `auto_filter_workflow.rs` | Host-provided recognized regions/category data; no OCR inference is supplied by the canvas itself. |
| Undo/redo | `history.rs`, engine mutation tests | Commands and gestures use engine history; draft/session state separate from document. |
| Clipboard/templates | `draw_template.rs`, `session.rs`, Qt runtime | Copy/paste engine templates with regenerated IDs and binding preservation; native text clipboard in editor. |
| Persistence | `session.rs`, `persistence.rs/tests` | Validated versioned engine session plus host-owned image envelope; atomic replacement on success, resources survive reload. |
| Export | Qt export types/render APIs | PNG and SVG/snapshot paths, scale/bounds limits, effects, no selection overlays in document exports. |
| Input | `interaction/lib.rs`, tablet Qt tests | Mouse/capture/focus loss/shortcuts tested. Pressure/touch accepted through a host-neutral API; native platform availability must be reported separately. |
| Resource lifetime/custom content | lifetime/custom-renderer/image tests | Per-canvas ownership, bounded caches, stale-job rejection, no strong entity cycles; custom extension seam rather than copying QPainter callbacks. |

## Proposed ownership and rendering

`CanvasDocument` owns one Rust `Engine` and document resources. It exposes controlled
commands and events, never a public mutable engine field. `CanvasSession` owns one viewport,
patch cursor and view state. The GPUI `CanvasView` is a retained entity owning focus,
capture state, the text edit entity, subscription/task handles and the renderer.
The demo shell owns only presentation/control choices and file dialogs.

Keep the existing core model/history/tools/geometry as path dependencies. A native Rust
adapter consumes the display protocol; it does not call the C ABI or depend on Qt/OpenCV.
Apply patch revisions atomically, validate range and geometry revisions, request a full
reset on a stale cursor, and retain IDs across selection/camera changes.

The GPUI paint API does not expose arbitrary backdrop filters or destination restore
compositing. Use a retained, scale-aware CPU composition backend for ordered effect runs,
with GPUI owning the native window, hitboxes, focus/input and texture presentation.
Use `resvg`/`tiny-skia` for anti-aliased paths, font shaping/fallback and compositing;
do not reconstruct QWidget/QPainter plumbing. This also gives deterministic offscreen
rendering/export. Cache parsed per-item vector content and completed render surfaces by
revision; overlay-only changes must not rerasterize the document. Native overlays stay
responsive while an asynchronous asset/effect request completes. Explicitly measure this
backend; a future GPU backend can replace it behind the same display/renderer boundary.

Damage is conservatively expanded across downstream effects. Large sampling effects may
require full-surface invalidation; correctness takes precedence over insufficient damage.
Never claim bounded incremental cost for an operation that invalidates all effect sources.
No frame timer or idle repaint loop. New render work originates from a changed patch,
surface/DPI, resource revision or active text edit. Limit surfaces, decoded image bytes,
cached item count, and async jobs. Reject oversized/malformed documents before replacement.
Async work is generation-tagged and owned by the view; destruction cancels/discards work.

Every effect sharing a `source_pass` samples the same immutable entering backdrop; sibling
filters do not recursively filter one another. Preserve separate `effect_run` identities,
including when an intervening filter is culled. Transparent ordinary drawables still end
passes. Spotlight is applied to the background before scene items; watermark is final.
Background restore interpolates all premultiplied channels, including alpha, against the
pristine source. A transparent original pixel must erase covered annotations completely.
Mosaic cells are anchored in canvas/source coordinates. Blur radius and mosaic cell size
are source-pixel values, transformed by canvas zoom and display scale exactly once.
Reference-grid exports and display resampling must be distinguished from a viewport raster;
any remaining parity difference must be measured and reported.

Text needs two explicit contracts: a Unicode editing buffer with UTF-16 IME mapping and
grapheme-safe commands; and renderer font measurement sent to the engine before publishing
scene patches. A draft is committed atomically through `TextDraftCommit`. A new text element
does not enter history until commit; Escape cancels. Input selection/marked text is transient.

## Phases and verification gates

1. Foundation: manifest/version lock, isolated build configuration, reusable session API,
   validated patch retention, coordinate/capture/cancel lifecycle tests and sample session.
2. Rendering: every scene/overlay/decor variant; ordered effects and images; fill/stroke/text
   styles; cache/damage tests, deterministic pixel/roundtrip tests and export limits.
3. GPUI host: retained custom canvas, native hitbox capture, focus/shortcuts, IME editor,
   asynchronous image loading, file/clipboard workflows, accessible minimal native controls.
4. Capability polish: bound-label measurement, serial/distance/angle/bubble/magnifier,
   effects/erasers/decorations, explicit provider capability errors and fixture documents.
5. Verification: independent read-only review, fix findings, targeted package tests only,
   scoped rustfmt, strict Clippy/build, native `render_to_image` captures and visual inspection,
   interaction checks at multiple scales and realistic release performance measurements.

Native UI capture must be coordinated with the parent before launch. No unrelated processes
are killed. Builds use this checkout's isolated output and limited parallelism. Do not run
repository-wide formatters, workspace test suites, or modify Ant/Kit reference projects.

Required behavior tests: patch reset/staleness/removal/tail edits; pointer capture followed
by Escape/focus loss/outside release/tool change; repeated actions and undo/redo branch;
camera roundtrip/zoom anchor/DPI; invalid import preserves old session; image/resource
roundtrip; renderer stacking/effect masks/rotation/text; idle cache reuse and bounded growth.
Performance fixtures: 1k/10k elements with visible versus offscreen distribution, long strokes,
typical screenshot effects, cold/warm render and overlay-only update. Record profile, machine,
counts and timing methodology; distinguish CPU render time from native GPU frame readback.

## Deliverables

- This plan and an evidence-backed completion matrix with honest limitations.
- Reusable library plus a small demo, sample document(s), explicit run/capture instructions.
- Focused tests, release performance runner/results, native captures after coordination.
- Apache-2.0 license and third-party notices. Run the repository license collector after
  dependencies are added; keep unrelated generated notices out of the change.
- No commits, pushes, external publication or unrelated source edits.

## Implemented phase record

All five phases above produced the library, native demo, sample annotations,
offscreen/native validation and optimized benchmark. The final evidence and exact
scope are in `CAPABILITY_MATRIX.md`, `VALIDATION.md` and `LARGE_IMAGES.md`.
The package remains independent of Ant components and uses the verified exact
GPUI pins. Existing Qt/core/reference sources were preserved.

Late host-boundary review added immutable before/original/after presentation,
layer coverage/donor eligibility, fixed view references, editing bounds/popup
scope, synchronized source history, completed Smart Erase sharing/freezing and
disjoint exports. Large-image review replaced the initial total16 Mi-pixel cap
with bounded working tiles and streamed output. Async persistence assets use a
Send-able snapshot; the non-Send engine stays on its owner thread.

The host completion phase adds command resolution, native cursor priority/DPI
crosshair geometry, asynchronous cache reset, dirty diagnostics and safe deferred
retirement. The shared engine's `Rc` scene-cache ownership prevents worker-thread
destruction; a bounded owner-thread queue defers it, with measured indivisible
drop/backpressure and no hard deadline. Send image/renderer/metrics payloads retire
on workers. See `HOST_AND_DAMAGE_PLAN.md` and `RETAINED_DAMAGE_AND_CLEANUP.md` for
design gates, accepted retained partial recomposition and teardown distributions.
Physical Windows IME/accessibility/pressure/touch qualification and QPainter pixel
identity are also not claimed. No drawing tool was replaced with a no-op stub.
