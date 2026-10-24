# Interactive frame synchronization

2026-10-24. GPUI previously painted live selection controls over the last image
completed by an independent worker. Letting superseded renders publish prevented
starvation but still displayed two different interaction states. An offscreen
regression dispatched a creation move and painted without draining workers: the
controls' scene was generation 4 while the image was still generation 3.
Qt's `SnowCanvasWidget::Impl::paint` paints content and controls from one cache
in the same paint call.

`CanvasElement::prepaint` now prepares document pixels before painting controls.
Input updates invalidate the view; updates accumulated before a paint are composed
once from the latest scene. The display renderer belongs to the UI thread, with
no worker queue or mutex between input and display. Idle and overlay-only paints
retain the image. Clipboard exports own a separate renderer and immutable snapshot.
If preparation/composition fails, the previous image retains its matching controls.

Smart Erase reconstruction remains asynchronous. The display renderer paints the
current mask without solving, then a cancellable worker supplies source/geometry-keyed
cache entries. The next frame composes the current scene using those entries;
the worker's raster is never published. Explicit invalidation rejects cancelled
work even if it already completed. Failed solves remain visible and are not
retried in an idle loop. Strict exports still perform/validate reconstruction.

To keep common interaction frames inexpensive, retained damage now supports
insertion and removal as well as movement, resize, rotation and style changes.
It compares against the last completed scene, preserves survivor order, rejects
ambiguous identities, includes old/new bounds and repaints intersecting layers.
Damage scratch premultiplication only visits affected rows/columns instead of the
entire surface. One immutable sampled background is retained (at most 64 MiB,
bounded by the existing working-surface limit). Its key covers source pixel
identity/placement, frame, scale and effective clear color. Both full and partial
redraws reuse it; cache reset releases it. This also removes repeated source
resampling when text or effects require a full redraw.
Existing precision, resource, area and dependency fallback gates
remain. Full redraw still handles text, effects, rough styles and other unsupported
damage cases; these frames remain synchronized but can exceed a frame budget.
This is a CPU renderer, not a claim of universally bounded 60 FPS rendering.

## Focused evidence

The creation regression fails before the change and passes afterward. Offscreen
GPUI tests paint without running background tasks and compare current move, resize
and rotation pixels with an independent full render. They also cover coalesced
edits, idle reuse, DPI/zoom, cancellation, document/source/presentation replacement,
cache reset, text draft cancellation, failed preparation and asynchronous Smart
Erase cache publication, including cancellation after worker completion and no
idle retry after an unresolved solve. Renderer tests verify every background-cache
sampling input and compare cached/full pixels. Damage tests compare full/partial pixels for insertion,
rotation, resizing, removal and reused indices at scales 1, 1.25 and 2.

CPU-only optimized measurement: Rust 1.97.1, Windows, `canvas-perf` (Release opt3),
960×640 logical canvas at scale 1.25, transparent gradient source, 16 frames each.
The edited rectangle is inserted, moved/resized/rotated, then removed. Other
rectangles occupy a 100-column lattice. Each sample includes CPU composition plus
the full RGBA copy/channel swap used for GPUI. It excludes engine input/patch
preparation, cold initialization, texture upload and compositor presentation.

| Other elements | Forced full median (ms) | Retained damage median (ms) | Damage min / max (ms) |
| --- | ---: | ---: | ---: |
| 0 | 8.304 | 2.171 | 1.984 / 2.585 |
| 1,000 | 10.521 | 2.223 | 1.869 / 2.672 |
| 10,000 | 35.158 | 5.485 | 4.610 / 7.281 |

All 16 frames per fixture use partial composition, including creation and removal;
every corresponding full/partial RGBA hash is identical. These compare full and
damage modes of the same implementation with the background cache enabled, not
end-to-end before/after input latency.

Final validation: 26 focused GPUI view tests, 12 renderer tests and 7 damage tests
pass, together with the optimized benchmark, package rustfmt and strict Clippy
over all package targets with `native-capture`. The repository-wide suite was not
run. This change was verified offscreen; native end-to-end input latency was not
measured.

```powershell
cargo test --locked --offline -p snow-draw-engine-gpui --lib view::tests
cargo test --locked --offline -p snow-draw-engine-gpui --lib damage_tests
cargo test --locked --offline -p snow-draw-engine-gpui --profile canvas-perf --lib interactive_damage_frame_cost -- --ignored --nocapture
cargo clippy --locked --offline -p snow-draw-engine-gpui --all-targets --features native-capture -- -D warnings
cargo fmt -p snow-draw-engine-gpui -- --check
```
