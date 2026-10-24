# Retained damage, host controls and cleanup

2026-10-23. This implements the acceptance plan in
[HOST_AND_DAMAGE_PLAN.md](HOST_AND_DAMAGE_PLAN.md), preserving GPUI/platform0.3.9,
existing raster quality and the shared engine/Qt sources.

## Host integration

`view/host_controls.rs` adds UI-owned command resolution, prioritized Host/CanvasTool
native cursor overrides, DPI-cached crosshair geometry, one-call asynchronous view
cache reset and latest-patch dirty-region diagnostics. Printable text and IME
payloads, including the native AltGr/dead-key preference, bypass command remapping;
function/navigation keys can resolve while a
draft is active. The physical pan key is remembered until key-up even if the
resolver is replaced. Modifiers retain the native event contract.

Crosshairs are four validated GPUI quads with native cursor center/fallback. The pinned
runtime has no arbitrary bitmap or persistent hidden OS-cursor API. Diagnostic red
scene/decor and blue overlay rectangle outlines use logical viewport pixels and do not
enter document history or exports. They describe the latest engine patch only;
they are not used as a cumulative damage contract after coalescing.

Cache reset cancels stale generations, retains completed pixels, detaches UI
metrics maps for worker destruction, and clears renderer caches on the existing
CPU worker. It does not wait for its mutex or reload the retained font database.
The demo exposes Damage, Clear cache and Crosshair controls.

## Retained recomposition

`damage.rs` diffs the last **completed** scene, so skipped/coalesced revisions
cannot omit earlier edits. It accepts only Architect source-over rectangles and
arrows, identical frame/resources/scale/default presentation, stable IDs/order/count,
at most64 changed items, and at most35% union damage. Paint bounds include old and
new geometry, stroke/head/pressure extents and antialias guards. All intersecting
objects repaint over pristine background in their original order.

Text/labels, serial/bubble items, filters, magnifiers, rough styles, multiply,
decorations, resource/camera changes and uncertain dependencies use full rendering.
This is a conservative optimization boundary, not missing functionality for those
features. Large-coordinate f32 uncertainty also forces full composition: estimated
matrix/geometry rounding must stay within a half-pixel budget. A regression near
camera999999000 proves changed visible pixels match the full renderer rather than
leaving stale pixels outside f64-derived damage bounds.

An initial cropped-camera prototype differed by2 channel levels at translucent
rotated edges and was rejected. The accepted implementation retains the original
frame/transform and full-stride bounded scratch surfaces, samples background only
inside damage, and publishes only the dirty rectangle. Cancellation does not
publish a partial image/key/scene. Vector-cache retention uses the complete scene,
not the temporary cull. `set_damage_rendering(false)` provides a reference path.

The renderer retains one completed scene and one completed RGBA image. A960×640
image is2457600 bytes; the dense vector cache is62615200 bytes, within64MiB.
Damage does not reduce the allocated full-stride scratch dimensions; it reduces
paint/sampling work. `working_pixels` is the largest working surface, **not** summed
scratch or process RSS. `damaged_pixels` reports the published region separately.
Existing image, vector, font, text and Smart Erase budgets remain enforced.

## Matched optimized evidence

Raw artifacts in `../artifacts/`:
`snow-draw-gpui-damage-final-full.json` and
`snow-draw-gpui-damage-final-partial.json` (after the precision gate and default
enablement). The earlier `fullstride-*` artifacts record the preceding accepted
prototype and are not used for the final table.
Same10000-rectangle lattice,960×640,scale1, MAIN-PC, Release opt3
`canvas-perf`, five cold samples, nine changed-camera frames and twelve opacity
edits. The ten-entry camera array includes one initial unchanged/cache-hit frame,
excluded from the changed-camera table.

| CPU workload | Forced full min/median/max(ms) | Damage min/median/max(ms) |
| --- | --- | --- |
| Cold |151.46 /159.51 /174.75 |146.86 /147.77 /158.64 |
| Changed camera |45.37 /47.88 /50.74 |42.29 /47.14 /54.79 |
| Single-item edit |41.95 /44.42 /49.51 |2.68 /2.94 /41.91 |

Eleven of twelve edited frames use partial composition; the first correctly falls
back because the preceding camera frame differs. Final damage is399 pixels of a
614400-pixel working surface. Every corresponding cold/camera/edit RGBA hash is
identical. Cold cost and camera fallback remain similar; eligible repeated edits
exceed the planned2× median improvement. These are CPU render measurements, not
visible input-to-pixel latency, texture upload or compositor presentation timings.
Cold-run variation is not attributed to the damage optimization. The source
command+patch itself costs5.98ms median in the enabled run and is additional to
render time. All120 hover updates hit the raster cache (patch0.62ms median).
CPU RGBA→BGRA copy is0.36ms median; upload remains unmeasured. Five cold-render
cancellation samples return Cancelled0.152–0.178ms after the controller request;
this is not a hard bound on a single noninterruptible parse/path operation.
The final eligible frame requests24 vectors (23 hits/1 rebuild), with0.11ms raster,
1.25ms background and0.91ms finalization; phase clocks do not sum to all planner,
allocation and traversal overhead. Pending work remains one worker/one latest
request, preserving completed pixels while edits coalesce.
Earlier matched parsing optimization and the unexplained historical404ms sample
are preserved in [DENSE_RENDER_PERFORMANCE.md](DENSE_RENDER_PERFORMANCE.md).

Reproduce within this standalone package:

```powershell
cargo bench --locked --offline -p snow-draw-engine-gpui --bench dense_profile --profile canvas-perf -- ..\artifacts\damage-reference.json
cargo bench --locked --offline -p snow-draw-engine-gpui --bench dense_profile --profile canvas-perf -- ..\artifacts\damage-enabled.json --damage
cargo bench --locked --offline -p snow-draw-engine-gpui --bench teardown --profile canvas-perf -- ..\artifacts\teardown-current.json
```

## Safe deferred destruction

The engine owns `Rc` scene state and remains on its creating thread. `cleanup.rs`
defers document destruction to a foreground retirement queue, at most two pending
documents, one drop per scheduled turn after a16ms timer. Overflow drops the oldest
on the owner thread as explicit backpressure. View release/replacement calls this
API automatically; headless GPUI hosts can call `retire_canvas_document`.
`canvas_cleanup_diagnostics` exposes pending/completed/overflow and last/max drop
times. Immutable source pixels, renderer, font/metrics caches and draft layouts are
Send and retire on CPU workers. No unsafe Send or core threading change is used.

`snow-draw-gpui-teardown.json` records owner-thread engine/document destruction
after session release, with core viewport state still owned by the document:

| Fixture | Samples | Min/median/max drop(ms) |
| --- | --- | --- |
|10000 shapes |5 |2.14 /2.58 /3.11 |
|100000 shapes |3 |20.71 /22.08 /26.54 |
|5000 shapes +5000 history edits |3 |3.16 /4.03 /4.36 |
|1000 text items ×32768 bytes |3 |22.47 /24.34 /28.41 |

These distributions justify removing destruction from the immediate host callback.
They do not establish a finite worst case. The core has no incremental drain API
or finite history bound: one engine drop and overflow backpressure remain
indivisible. Remaining queued documents drop on the owner thread at app shutdown.
Bare headless document destruction is synchronous unless retirement is requested.
Diagnostics time document drops, not worker cache release or native image/driver
cleanup. A hard teardown deadline needs a separately designed safe core drain API;
moving Rc to a worker would violate ownership and is not an acceptable substitute.

## Verification boundaries

Focused tests compare partial/full pixels for overlapping alpha, rotated movement,
width/style/pressure/head changes, fractional camera/scales, placed transparent
sources, coalesced revisions and cancellation. Explicit full-fallback tests cover
unsupported dependencies, order/removal, broad edits, resources, presentation and
large-coordinate precision. Host tests cover native text isolation, named keys,
cursor priority/DPI caching, history neutrality, busy-worker cache reset, deferred
Send payloads and owner-thread retirement/backpressure.

Native inspection found that crosshair painting queried the previous frame's
hitboxes. It now uses GPUI's current hover query; an offscreen regression checks
all four painted quads at two pointer positions with actual display scaling.
The capture harness also waits for normal theme notification/frame processing and
adds a preventive comparison of theme-invariant canvas pixels. The final native
run passes 14 checks, with identical light/dark canvas pixels, 144 crosshair pixels,
and both replaced documents retired. All six captures were inspected at original
resolution. An earlier dark-preview interpretation was mistaken; the old and new
dark PNG hashes are identical, so no dark-renderer defect is claimed.

Physical Windows IME candidate UI, screen readers, tablet pressure and touch
hardware remain separate qualification work. The renderer remains CPU based;
native GPUI paints controls, overlays and retained document images. General cold
scene acceleration or effect-aware damage would require a further benchmarked
design, not an implied GPU renderer rewrite.
