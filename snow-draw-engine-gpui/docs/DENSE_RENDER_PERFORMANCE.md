# Dense-render performance follow-up

The asynchronous display scheduling described in this historical record is
superseded by [interactive frame synchronization](INTERACTIVE_FRAME_SYNC.md).
Display pixels and controls now share frame preparation; only reconstruction and
exports use workers. The CPU measurements below retain their original meaning.

Historical first pass. A subsequent accepted retained-damage implementation is
recorded in [RETAINED_DAMAGE_AND_CLEANUP.md](RETAINED_DAMAGE_AND_CLEANUP.md).
Its conservative eligible edit path supersedes the warm full-redraw limitation
below; the original matched results and interpretation are retained as evidence.

2026-10-23, CPU-only. This follow-up profiles the existing renderer, removes one
verified source of repeated work, and stops at the measured architectural boundary.
GPUI/runtime versions, raster quality, geometry and the 64 MiB vector budget remain
unchanged. No native GUI was launched while Input owned the GUI slot.

## Reproduce

```powershell
cd D:\snow-apps-workspace-2\snow-apps\snow-draw-engine-gpui
cargo bench --locked --offline -p snow-draw-engine-gpui --bench dense_profile --profile canvas-perf -- ..\artifacts\snow-draw-gpui-dense-current.json
```

Historical matched artifacts are `../artifacts/snow-draw-gpui-dense-before.json`
and `../artifacts/snow-draw-gpui-dense-after.json`. The before artifact was captured
with phase instrumentation present, before removing unused pattern definitions.
The after artifact uses the identical benchmark and instrumentation.

Fixture: 10,000 rectangles on a 100×100 lattice, 5×3 units, 9×6 spacing, all visible
in 960×640 at scale 1. Each cold sample clears render/vector caches but retains the
renderer/font database. Both builds use `canvas-perf` (Release, opt3), Rust 1.97.1,
Windows MSVC, MAIN-PC/24 logical processors. There are 5 cold samples, 9 changed-camera
redraws, 12 alternating opacity edits of the same item, 120 hover updates, 20 CPU
RGBA→BGRA copy samples, and 5 cancellation samples.

## Cause and measured change

The old lowerer emitted both hatch definitions for every rectangle/arrow, including
ordinary solid/border fills. They were unused in the final image, but increased XML
parsing and conservative retained-tree accounting. The 10k scene exceeded 64 MiB;
on each edit its traversal evicted and rebuilt about 2,388 unrelated trees.

`vector.rs::patterns(fill, style)` now emits definitions only for visible Line or
CrossLine fills. Tapered shafts use solid stroke-color fills and omit these unused
definitions too. Hatch/crosshatch pixels are still tested. The complete scene now
fits in 62,615,200 retained vector bytes; a single opacity edit rebuilds exactly 1
tree and changed-camera frames rebuild 0. Previously the cache held 67,106,248 bytes
of a partially retained scene and repeatedly rebuilt omitted trees.

| Workload | Before min / median / max(ms) | After min / median / max(ms) |
| --- | --- | --- |
| Cold render,5 samples |186.80 /193.64 /204.04 |150.28 /151.84 /159.54 |
| Changed camera,9 samples |79.54 /81.82 /89.24 |48.27 /50.36 /53.83 |
| Single-item opacity redraw,12 samples |74.73 /77.45 /86.37 |44.85 /51.12 /76.84 |
| Hover engine/retained patch,120 samples |0.630 /0.644 /1.124 |0.639 /0.670 /1.133 |

Cold median improves 21.6%, camera redraw 38.5%, and edited-frame redraw 34.0% in
these matched runs. All hover renders hit the unchanged raster cache. The source
command+patch cost for opacity edits was 5.67ms median before and 7.84ms after;
it was not optimized and varies with load (after max 14.00ms).

Cold pixel hash is identical (`9160061050340257044`), and every corresponding
camera/edit frame hash matches. Hashes are FNV-1a over full RGBA buffers. The final
offscreen showcase sample also compares byte-for-byte equal in decoded RGBA to
the pre-optimization sample. These are stronger checks than timing alone.

## Phase costs and interpretation

Opt-in `CanvasRenderer::set_profiling(true)` exposes `CanvasRenderProfile`; normal
item hot paths do not read phase clocks. Medians below are per-phase medians and
need not sum exactly to the total median.

| Cold dense phase | Before(ms) | After(ms) |
| --- | --- | --- |
| Background creation |1.11 |0.96 |
| Vector source lowering/layout |30.89 |26.72 |
| SVG parsing / text shaping |116.02 |82.58 |
| Tree lookup/storage/budget accounting |5.59 |2.78 |
| Path rasterization/compositing |35.13 |34.06 |
| Ordered effects |0 |0 |
| Retention/final RGBA conversion |1.33 |1.42 |

This dense fixture contains no text or effects: its parse/shape cost is XML/path
parsing, not font shaping. A separate showcase sample records label measurement
and effects; raw JSON contains `sample_engine_label_layout_ms` and
`sample_profile`.
The after sample has 27.11ms background sampling,5.24ms vector lowering/layout,
3.44ms SVG parsing/shaping,2.24ms ordinary raster and 5.42ms effects (44.12ms total).
Effect timings may include nested vector work for magnifiers; they are not an
exclusive partition for every scene type.

CPU RGBA→BGRA copy is0.36ms median before / 0.42ms after. GPUI texture upload,
GPU drawing, compositor presentation and visible input-to-pixel latency were
**not measured** in this CPU-only follow-up. The copy was not moved or redesigned
because it is small relative to the measured bottleneck.

The earlier broader benchmark recorded 404.33ms cold median of 3 for this same
rectangle lattice, viewport and optimized profile. This follow-up records 193.64ms
before the optimization, using 5 samples in a dedicated benchmark with opt-in
timers. Machine CPU load/frequency, scheduler activity and contention were not
recorded, so the cause of that run-to-run difference is **not established**.
Do not present404→152ms as the optimization's speedup. The supported comparison
is the matched194→152ms pair above.

## Cancellation, queues and visible latency

For each cancellation sample a fresh dense renderer starts on a CPU worker; the
controller requests cancellation after 5ms and measures until that worker joins.
All 5 runs return `CanvasError::Cancelled`, with 0.206–0.253ms cancel-to-join after
the optimization. This is evidence for the small-item dense fixture, not a hard
real-time bound on one huge path, text shaping call or noninterruptible library
operation.

Deterministic tests prove a cancelled partial render preserves the previous
completed image/cache key. An offscreen GPUI test applies 24 edits to a 1k document
before yielding, keeps the previous image available, coalesces intermediate work,
and finishes on the final generation with at most 2 additional completed renders.
`CanvasView::schedule_render` owns one worker and one latest pending request;
ordinary edits let that worker finish and publish before composing the latest request.
Cancelling every superseded generation would starve presentation during continuous
pointer movement. Explicit invalidation cancels the worker and rejects its result even
if it finished before the invalidation: gesture/text cancellation, document/source
replacement, presentation changes, cache reset and snapshot preparation failures retain
that protection. Offscreen regression tests overlap creation, selection movement,
resize and rotation events with both worker execution and foreground publication, check changed
pixels before pointer release, and verify invalidated results never publish. No idle
repaint loop or unbounded edit queue is introduced.

These mechanisms bound pending work and allow native overlays/previous pixels to
remain usable. They do **not** turn a 51ms edited-frame composition into a 16.7ms
visible update. Full dense redraw remains below 60fps, and cold visibility costs
about 152ms here. Actual interactive-frame latency requires a separately coordinated
native measurement; it must not be inferred from cancellation or cache-hit timing.

## Verification and next architectural decision

Final scoped checks pass: 43 unit + 23 integration tests, rustfmt, strict Clippy over
all package targets/native-capture, and the native-feature demo build. Added
regressions cover 10k tree retention with a single edit, fill coverage, pixel-neutral
profiling, mid-render cancellation, and latest-generation coalescing. Independent
read-only review found no concrete correctness issue in this delta.

The remaining warm redraw is dominated by rasterizing all 10k visible paths
(about 44ms). Further substantial single-item edit improvements require retained
damage tiles/layers and effect-aware invalidation, rather than more tree parsing
changes. A future GPU path is another larger design option. Neither was undertaken
speculatively in this bounded follow-up. Adopt retained damage only with tests for
ordered effect dependencies, dirty region expansion, blending and memory lifetime.
See [HOST_API_MAP.md](HOST_API_MAP.md) for remaining Qt host differences.
