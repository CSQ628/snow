# Validation record

Date: 2026-10-23. Checkout: `D:\snow-apps-workspace-2\snow-apps`.
Package: `snow-draw-engine-gpui`; existing Qt/reference sources unchanged.
GPUI runtime/platform are exactly 0.3.9 with the current Ant platform features.

## Focused verification

```powershell
cd D:\snow-apps-workspace-2\snow-apps\snow-draw-engine-gpui
cargo test --locked --offline -p snow-draw-engine-gpui --lib --test canvas_engine
cargo fmt -p snow-draw-engine-gpui -- --check
cargo clippy --locked --offline -p snow-draw-engine-gpui --all-targets --features native-capture -- -D warnings
cargo build --locked --offline -p snow-draw-engine-gpui --bin snow-draw-demo --features native-capture
.\scripts\run-performance.ps1 -OutputFile ..\artifacts\snow-draw-gpui-performance-streaming.json
```

Related tests after host completion and default-enabled retained damage:
**54 unit + 23 integration, all passed**,18.54s/7.79s respectively.
This includes offscreen GPUI tests,
not the repository-wide or upstream workspace suites. Strict Clippy across all
package targets/native-capture passed with warnings denied; rustfmt and the final
native-capture demo build passed. The final offscreen CLI sample was regenerated
at `../artifacts/snow-draw-gpui-sample-final.png` and visually inspected. After the
bounded performance follow-up, `snow-draw-gpui-sample-profiled.png` has identical
decoded RGBA pixels.

Coverage includes retained patches/stale revisions, source and session/history
roundtrip, source replacement interleaved with drawing undo, capture interruption,
outside release, repeated actions, selection/rotation/multiselect/templates,
UTF-16/preedit/graphemes/bidi/long text, bound labels, style-only history and
target-preserving Delete, viewport/DPI/cache/worker generations, ordered immutable
effect passes, premultiplied alpha, restore, magnifier, Smart Erase quality/limits,
frozen/shared results and cache lifetime, semantic vector SVG roundtrip, strict
export, host stages/clips/layers, large sources and streamed/disjoint exports.

Late review regressions additionally cover:

- Hebrew visual caret ordering, glyph-box pointer hits and selection in short and
  long shaping paths; logical UTF-16 storage remains separate from visual geometry.
- Configurable persistence budgets paired between save/load, bounded vector SVG
  construction, and actual SVG rasterization matching odd-size fractional PNG grids.
- RGB decode RGBA-byte preflight, including an image header with missing payload.
- Worker-owned persistence encoding/decoding and cancelled atomic save preserving
  the previous target with no temporary left behind.
- Full/streamed/SVG/packed outputs sharing fractional-scale dimensions and grid.
- Coarse export queries adapting to several broad ordered blur supports.
- A 4096² output fitting nominal 16 Mi pixels but requiring streamed halo handling.
- Strip/tile pixel agreement across fractional-scale blur/mosaic passes; complete
  thick connector and external magnifier leader bounds during culling.
- 20 small Smart Erase results retaining under 64 KiB collectively and reusing
  frozen results after camera/zoom changes rather than retaining donor halos.
- Retained partial/full pixel agreement for translucent rotated geometry,
  pressure/head changes, fractional scales/placed sources, cancellation/coalescing,
  broad edits and all unsupported dependencies; large f32-coordinate fallback.
- Command-key remapping during drafts while preserving printable/IME input,
  pan release after resolver replacement, cursor priority/DPI factory caching,
  history-neutral cache reset without waiting for its mutex, deferred Send cache
  payloads and bounded owner-thread document retirement/backpressure.

## Native Windows validation and visual inspection

Final artifact directory: `../artifacts/snow-draw-gpui-native-host-reviewed/`.
`validation.json` reports **passed**, 14,512.4208ms, 1280×880 logical, 1.5× scale.
The native process exited successfully; no canvas GUI was left running.

Fourteen checks used a real Windows GPUI window and supported
`Window::dispatch_event`/`dispatch_keystroke` plus `EntityInputHandler`:
GPU readback of showcase; New toolbar button; repeated gestures and undo/redo;
interrupted capture/preview cancellation; outside release; selection/resize;
Chinese/emoji UTF-16 preedit and atomic commit; pan; zoom shortcut; unchanged
document pixels across light/dark theme; host key resolution/crosshair/dirty
diagnostics/cache reset; live-draft function-key cancellation and AltGr text
protocol; repeated translucent plain-scene edits; owner-thread retirement.

All six images were opened at original resolution and visually inspected:
`canvas-native.png`, `canvas-dark.png`, `canvas-ime-draft.png`,
`canvas-interactions.png`, `canvas-host-controls.png`, `canvas-retained-edits.png`. Shapes,
generated labels, separate image effects, readable controls, dark theme,
composition/caret, committed Unicode output, all four custom crosshair arms,
diagnostic outlines and overlapping translucent shapes were present. This is native
GPUI protocol/dispatch evidence, **not physical OS input injection or Windows IME
candidate-window/accessibility/tablet/touch certification**.

This native refresh follows the final host/text/large-source/retained-damage
implementation. Large image/export performance remains separately covered by
focused CPU tests/benchmarks. No GUI was launched while another task owned the
slot; after this process exited, the slot was explicitly released with zero
matching canvas processes. Enlarged source-grid text remains softer than direct
native glyph rasterization.

The first extended run (`snow-draw-gpui-native-host-final/`) passed 13 behavioral
checks but exposed missing custom crosshair painting. The previous-frame hitbox
query was replaced with current-frame hover, verified by four painted quads in
the offscreen regression and **144 magenta crosshair pixels** in final readback.
Diagnostic regions now use outlines rather than obscuring content with fills.

An earlier interpretation of the dark preview as blank was incorrect. Original
resolution inspection and identical old/new SHA256
`10c2a44b5c1d71d07a5eb4163c4a6e84e36fde8ab2595571af38d81fd2febe72`
confirm the earlier image already contained the drawing. No dark-rendering bug
is claimed. The harness now waits for normal theme/frame processing and adds a
preventive spatial pixel guard: **187067 content pixels** in each theme and
**0.0 mean canvas RGB difference** in the final run. Both replaced documents
retired (pending 0/completed 2/overflow 0; maximum measured drop 0.1521ms for these
small native fixtures). These small fixture costs do not replace the large
teardown benchmark or establish a hard deadline.

## Optimized measurements

The later matched dense-scene investigation is in
[DENSE_RENDER_PERFORMANCE.md](DENSE_RENDER_PERFORMANCE.md):194→152ms cold median,
77→51ms single-item redraw, identical output. The accepted retained-damage follow-up
is in [RETAINED_DAMAGE_AND_CLEANUP.md](RETAINED_DAMAGE_AND_CLEANUP.md), with a matched
44.42→2.94ms eligible edit median and identical frame hashes; camera changes retain
full composition. The earlier404ms below remains a
historical observation; its difference from the new baseline is not established.

Raw artifact: `../artifacts/snow-draw-gpui-performance-streaming.json`.
`canvas-perf` inherits Release, opt-level 3, Windows MSVC Rust 1.97.1; MAIN-PC,
24 logical processors; ordinary surface 960×640 at scale 1. The profile preserves
the exact GPUI Windows snapshot's runtime-shader path. These are CPU timings on
a shared machine, not GPU frame/presentation latency or isolated lab results.

| Workload | Latest measurement |
| --- | --- |
| 1k / 10k sparse elements, 110 visible | Construction 16.21 / 375.23ms; cold raster 12.65 / 14.11ms (one sample). |
| Sparse 10k camera/hover patches | Median 0.0769 / 0.2574ms; p95 0.1284 / 0.2977ms, 120 samples each. Hover preserves scene revision. |
| Cached unchanged raster | ~0.0002ms median, 1000 samples; measures cache lookup, not redraw. |
| 1k / 10k dense, all visible | Cold 44.35 / 404.33ms median of 3; 10k max 442.90ms. Retained vector complexity 67,105,976 bytes, within 64 MiB. |
| 30,000 pressure inputs | 563.10ms including retained patch updates; cold raster 18.27ms (one sample). |
| Screenshot/effect showcase | 72.74ms median / 93.74ms max, 5 cold samples. |
| 32 / 64 KiB Unicode layout | Natural 124.45 / 174.62ms; wrapped 600-unit width 352.43 / 348.43ms, 3 samples, fonts retained. |
| Textured Smart Erase 192×160 | Cold 56.13ms median of 3; cached result with camera change 2.71ms median / 3.40ms p95 of 20. |
| Three 4K displays 11520×2160 | 99,532,800 source bytes; streamed PNG to sink 1.884s; peak working surface 135,696 pixels. |
| Scrolling source 256×100,000 | 102,400,000 source bytes; streamed PNG to sink 1.766s; peak working surface 34,320 pixels. |
| Owner-thread engine drop 1k / 10k, historical | 0.267 /7.021ms, one sample each; superseded distributions below. |

Later teardown distributions in `snow-draw-gpui-teardown.json` measure100k shapes
20.71/22.08/26.54ms(min/median/max,3 samples), and1000×32KiB text
22.47/24.34/28.41ms(3 samples). `cleanup.rs` defers documents on their owner thread
with at most two pending and overflow backpressure; Send image/renderer/metrics
payloads retire on CPU workers. One core drop remains indivisible and unbounded
for arbitrary histories; queue diagnostics do not include driver/worker cleanup.

The large fixtures allocate source pixels but no full output RGBA buffer; output
bands are bounded to 8 MiB. Peak working-surface counters are not process RSS or
total scratch accounting. One/three-sample p95 entries are maxima, not statistically
robust percentiles. The 64 KiB text layout cost is material and not a 60fps claim.

Earlier long-text experiments exposed repeated font-file mapping and outline
construction in UI measurement. Owned bounded font-byte caches and a metrics-only
long-text path removed that causal overhead. Worker outline rasterization still
has real cold cost. Cold dense documents likewise remain hundreds of milliseconds;
the UI coalesces/cancels rendering and retains prior pixels/native overlays.

## Review and licensing

The final independent read-only review reported no material unresolved correctness
defects in its scoped findings. Earlier reviews identified and drove fixes for source-pass culling,
alpha restore, text/style/label ownership, font shaping agreement, immutable source
identity, source-history accounting, Smart Erase eligible pixels/cache lifetime,
dimension/halo limits, and complete painted culling bounds. The stream mismatch
was traced to f64 reassociation at8-bit channel ties; fixed 16-bit fractional texel
coordinates resolve translated tiles without weakening the≤1LSB comparison.

Repository license collector completed after final direct dependencies:
`../artifacts/snow-draw-gpui-licenses-streaming/third-party/INDEX.md`:
512 notices /284 external Rust packages /21 superset vcpkg entries. No Qt link.
Four archive license omissions use audited upstream texts/provenance in `licenses/`.

Remaining differences are listed in [CAPABILITY_MATRIX.md](CAPABILITY_MATRIX.md)
and exact resource contracts in [LARGE_IMAGES.md](LARGE_IMAGES.md). Most notably,
the reused engine contains `Rc` and is not `Send`; deferred destruction remains on
its owner thread. A hard drop deadline requires a safe incremental core drain API.
No unsafe thread transfer was introduced. No commits, pushes, publication or
unrelated changes were made.

The bounded dense-scene follow-up adds four passing regressions (tree retention,
fill coverage, pixel-neutral profiling/cancellation, and edit coalescing). Strict
checks were rerun successfully after that delta. See [HOST_API_MAP.md](HOST_API_MAP.md)
for implemented customization/diagnostic hooks and the exact teardown source boundary.

The host/damage phase received independent read-only review. Findings for f32
large-coordinate paint bounds, function-key resolution inside drafts, and
foreground cache destruction were fixed and re-reviewed with no material issue
remaining. The conservative partial path is enabled by default; existing pixel
tolerances and resource caps were preserved.

After the final enum-indirection lint fix, all six damage regressions passed
(16.75s). After preserving native AltGr/dead-key input preference, all14 view tests
passed(3.75s), followed by rustfmt check, strict Clippy and native-feature demo build.
No unrelated repository/upstream suites were run. Machine-readable CPU checkpoint:
`../artifacts/snow-draw-gpui-host-damage-verification.json`.

After visual-review fixes, all 15 view tests passed (3.86s), including the new painted
crosshair/DPI regression. This adds one unit test beyond the54+23 scoped baseline;
unchanged engine/export suites were not repeated for the view-only change.
