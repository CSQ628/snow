# Snow Draw Engine GPUI

An Apache-2.0 native GPUI canvas and standalone demo. It reuses the UI-independent
Snow Draw Engine Rust crates from `../snow_draw_engine_qt/crates`; it does not link Qt
or depend on the Ant Design component library. The existing Qt project is unchanged.

`gpui-pre` and `gpui-pre-platform` are pinned to **0.3.9**, matching the manifest and
lockfile inspected in `D:\snow-apps-workspace\snow-apps\ant-design-gpui`. Platform
features match that checkout: `font-kit`, `x11`, `wayland`, `runtime_shaders`.

## Run

Use the repository's Rust 1.97.1 and VS 2026 developer tool environment:

```powershell
cd D:\snow-apps-workspace-2\snow-apps\snow-draw-engine-gpui
.\scripts\run-demo.ps1
# Faster CPU composition after the first optimized build:
.\scripts\run-demo.ps1 -Optimized
```

Cargo output is isolated in `../build/snow-draw-gpui`, with two build jobs. The demo
opens a showcase document. Choose New to draw, Image to replace its immutable source,
Open/Save for a `.snowdraw` session, or PNG/SVG to export the current viewport.
Image decoding/encoding and Smart Erase reconstruction run on workers. Display
composition runs once at frame preparation, from the same scene as selection controls.
A failed import preserves the
current document. Auto regions installs clearly labelled sample host regions; it is
not an OCR service. Watermark toggles a sample watermark and participates in undo.

Shortcuts: V select, R shape, A arrow, L line, P pen, T text, E object eraser;
Space+drag or middle drag pans; wheel zooms/adjusts the active tool. Shift constrains
or adds selection. Double click text to edit; Ctrl+Enter commits, Escape cancels,
Enter inserts a line break. Ctrl+Z/Y undo/redo, Ctrl+D duplicates, Delete removes,
Ctrl+C/V transfers drawing templates, Ctrl+Shift+C copies an overlay-free PNG.
Ctrl+0 fits the source; Ctrl+plus/minus changes zoom. Text editing supports grapheme
navigation, selection, clipboard, local undo and UTF-16 IME composition.

With the Text tool, clicking existing text opens its editor at the clicked character;
the initial press can also drag a text selection. Clicking outside an active editor
commits it without creating another text element with that press. Selected glyphs
turn white over a blue highlight, and the text-colored caret blinks while focused.

## Architecture and integration

- `CanvasDocument`: engine, immutable source pixels, resource revisions, validated
  versioned persistence. Controlled commands keep the engine reusable by Snow Shot.
- `CanvasSession`: viewport/camera, retained typed patches, interaction ownership and
  explicit close/cancellation output for multi-view hosts.
- `CanvasView`: GPUI entity/custom element, native hitbox capture, focus/IME adapter,
  native overlays, display updates coalesced at frame preparation; independent
  export/reconstruction workers never publish stale display pixels.
- `CanvasRenderer`: retained vector/font caches, ordered immutable effect passes,
  bounded mask crops, source-grid Smart Erase cache, offscreen PNG/SVG export.
- `smart_erase/`: affine and periodic fast paths plus perceptual guides and multiscale
  PatchMatch. Source observations and alpha remain immutable; masked colors never
  become donors. Interactive failures remain visible as unresolved masks; strict
  exports return an error.

Canvas input stays in logical pixels. The display raster follows camera zoom and
window DPI, with physical pixel extents and padding for ordered filter support.
The reference scene retains effect dependencies independently of display resolution,
including when a host selects a fixed composition grid. DPI, zoom and viewport changes
refresh the raster; idle paints reuse the completed image.

Use `CanvasRenderer::snapshot(&mut document, &mut session)` for headless exports:
it supplies text measurements and retains all ordered-effect dependencies.
`CanvasView::snapshot()` returns the prepared view snapshot or an error.
For full screenshot/scrolling regions, use `snapshot_for_export(region, output_size)`
with `export_snapshot_png_to_writer`, `export_snapshot_tiles_to_writer`, or
`export_snapshot_svg_to_writer`. These stream bounded bands without allocating the
full output raster; the SVG writer embeds the lossless composite. Use
`DrawingSnapshot` to encode/decode persistence on a worker and construct its engine
on the owner thread. Call `CanvasSession::close` when removing a shared view and consume its optional pointer-release output.
Plain SVG documents preserve paths and outlined text; ordered backdrop effects are
flattened to a lossless embedded PNG. Selection overlays are excluded from exports.

`ImageSourceLayer` supports paint order, independent X/Y placement, coverage and
Smart Erase eligibility. `SourceImage::from_layers_cancellable` freezes separate
visual and reconstruction composites. `CanvasPresentation` supplies immutable
before/after layers and an independent output backdrop. `CanvasViewOptions` owns
visibility, gesture/content/decorative clips, text bounds, focus-popup behavior
and an optional fixed composition grid. `SmartEraseSnapshot` shares completed
reconstruction results with another renderer; frozen missing results fail explicitly.

Host controls include `set_command_key_resolver`, prioritized native cursor layers,
a DPI-cached crosshair geometry factory, `clear_render_state`, and dirty-region
diagnostic overlays. The demo's Damage, Clear cache and Crosshair controls exercise
these APIs. Safe ordinary geometry edits use retained partial recomposition; other
dependencies conservatively use full rendering. See
[retained damage and cleanup](docs/RETAINED_DAMAGE_AND_CLEANUP.md) for gates,
measurements and ownership details.

See [implementation plan](docs/IMPLEMENTATION_PLAN.md),
[capability matrix](docs/CAPABILITY_MATRIX.md), and
[validation/performance record](docs/VALIDATION.md),
[dense-render follow-up](docs/DENSE_RENDER_PERFORMANCE.md),
[interactive frame synchronization](docs/INTERACTIVE_FRAME_SYNC.md), and
[Qt host API map](docs/HOST_API_MAP.md).

## Checks and captures

```powershell
cargo fmt -p snow-draw-engine-gpui -- --check
cargo clippy --locked --offline -p snow-draw-engine-gpui --all-targets --features native-capture -- -D warnings
cargo test --locked --offline -p snow-draw-engine-gpui --lib --test canvas_engine
.\scripts\run-performance.ps1
# Opens one native window, dispatches interaction scenarios, captures, then exits:
.\scripts\run-demo.ps1 -Capture -OutputDirectory ..\artifacts\snow-draw-gpui-native-final
# Focused native text interaction, selection pixels and caret blink checks:
cargo run --locked --offline -p snow-draw-engine-gpui --bin snow-draw-demo --features native-capture -- --capture-text ..\artifacts\snow-draw-gpui-text
# No native window:
cargo run --locked --offline -p snow-draw-engine-gpui --bin snow-draw-demo -- --export-sample ..\artifacts\snow-draw-gpui-sample.png
```

Native capture uses GPUI `render_to_image`. The scenario uses the real window's
supported GPUI input dispatch and the native text-input protocol. Windows IME
candidate UI itself is not automated. Coordinate native runs with other workspace
UI tasks. These commands are scoped to this package; do not run the repository-wide
suite or formatter for this change.

## Practical limits

This backend composes document pixels on the CPU and presents a retained GPUI image;
transient overlays use GPUI paths. It is not a GPU tessellation/backdrop-filter backend.
The source-grid image is resampled for pan/zoom/DPI, so enlarged text is softer than
native glyph rendering. Images are immutable host source pixels, not editable image
nodes (the shared engine has no implemented image-element primitive).

Interactive/working rasters are limited to 16 Mi pixels and 16,384 per dimension;
this is **not** a source-image or total export limit. Owned `SourceImage` buffers
are independent of the renderer's one sampled-background cache (up to 64 MiB RGBA,
invalidated by sampling changes and released by cache reset). Source buffers
support up to 8 GiB RGBA. Decode defaults to 512 MiB RGBA, preflighted before
conversion; `decode_with_limit` accepts an explicit budget up to 8 GiB. Sources are
immutable contiguous buffers; this is not an out-of-core image provider.

Streaming PNG/SVG and packed-tile exports support up to 8 GiB decoded RGBA output,
with an 8 MiB scanline limit (2,097,152 pixels wide), canvas coordinates within
±1e9, and render scale 0.1–8. Bands use at most 8 MiB/128 rows; tiles are at most
1024 pixels wide plus locally accumulated effect halos. A halo must fit the
working-raster budget and at most 8192 pixels of physical support; impossible
working regions fail explicitly. Dimensions use the ceiling of the stored f32
scale multiplied in f64, consistently across export paths. Convenience encoded
buffers are capped at 96 MiB; use writer APIs for larger files. Drawing/session
files are capped at 512 MiB; default save/load both cap each decoded asset at
512 MiB, with paired `DrawingSnapshot::*_with_limit` APIs for larger assets. Layer projection supports 128 layers/1 GiB input,
512 MiB per visual/eligible composite and 128 Mi projected samples.

Smart Erase has a conservative 512 MiB scratch estimate (256 bytes per cropped
working pixel), a 2 Mi-pixel reconstruction ROI including donor context, and a
128 MiB result cache. Cached results discard donor halos. Oversized reconstruction
returns a visible item warning and an export error. Vector trees use a conservative
64 MiB source/outline complexity budget, line layouts 16 MiB, font bytes 64 MiB,
and source-history assets 1 GiB/128 distinct images. Editing text is limited to
64 KiB UTF-8. History metadata follows the engine's lifetime. View release/replacement
defers document retirement on its owning thread (two pending maximum); source and
renderer/metrics payloads retire on CPU workers. One engine drop and queue-overflow
backpressure remain indivisible; no hard teardown deadline is claimed. Headless
hosts can call `retire_canvas_document` and inspect `canvas_cleanup_diagnostics`.
See [large-image contracts](docs/LARGE_IMAGES.md) for the
comparison with Snow Shot and concrete supported sizes.

Native GPUI mouse events do not expose tablet pressure; typed engine input accepts
pen pressure/touch for hosts with a native provider. See the matrix for verification
coverage and the remaining differences from the Qt renderer.
