# Host completion and retained damage design

2026-10-23. Continue the verified canvas without changing GPUI0.3.9 or the shared
Qt/core sources. Native UI remains coordinated with the parent; initial checks
are CPU/offscreen only.

## Host APIs

1. Resolve command key identities through a UI-owned callback. Preserve native
   logical text/IME payloads and modifiers. Remember the original physical pan key
   until release so replacing a resolver cannot leave pan latched.
2. Native cursor overrides have CanvasTool/Host priority. A DPI-cached factory
   supplies custom GPUI crosshair geometry. The pinned GPUI cursor enum has no
   arbitrary bitmap or persistent hidden cursor; native crosshair remains its
   center/fallback. Do not claim OS bitmap-cursor support.
3. View cache reset cancels stale generations and schedules cache clearing on the
   existing worker, keeping previous pixels visible; no UI mutex wait. Diagnostic
   engine damage rectangles paint as native overlays and never enter exports or
   history. Retain their view-coordinate contract, not a canvas-coordinate guess.

## Teardown investigation

Benchmark owner-thread drops for10k/100k shapes, a long edit history, and sizeable
text payloads. Report distributions and fixture limits. The reused core exposes
neither a public incremental history/model drain nor a finite history bound, so
an absolute worst-case destruction time cannot be promised.

If measurements justify deferral, detach the UI immediately and use a bounded
foreground retirement queue: engine/Rc state remains on its owning thread, one
document per scheduled turn, with explicit backpressure and completion diagnostics.
Send-capable immutable image/cache resources may retire on CPU workers separately.
Deferral does not bound one engine drop; that requires a future safe core drain API.
Never use unsafe Send or transfer Rc-owned engine state to a worker.

## Retained damage proposal

Matched evidence before this phase: dense10k warm single-item redraw≈51ms, of which
≈44ms rasterizes unchanged paths. Retaining more parsed trees cannot remove that
cost. Introduce a conservative partial-recomposition path using the existing
renderer and retained full image, not a new GPU backend.

Eligibility: identical frame/scale/resource/presentation inputs, unchanged paint
order and item count, and supported ordinary source-over geometry. Initially
admit Architect rectangles/arrows only. Text/labels, rough filters, multiply,
backdrop effects, magnifiers, decorations and changed frame/resources retain full
composition until their dependency/paint bounds can be proved separately.

Diff against the last **completed** retained scene, rather than trusting only the
latest engine dirty patch: a worker may coalesce several revisions. Union old/new
paint bounds for changed items, add antialias guards, clamp to the pixel viewport.
If damage exceeds35% of surface, more than64 items change, order/IDs differ or any
contract is uncertain, use existing full rendering. Recompose all intersecting
objects in the dirty rectangle from pristine background, then replace its pixels
in a copy of the previous image. Preserve the full scene's vector-cache lifetime;
do not prune caches to the temporary cull. Commit image/key/scene only on success.

Acceptance gates before enabling by default:

- Focused pixel comparisons against forced full rendering for move/resize/style,
  overlapping translucent shapes, arrows/pressure, fractional camera/scale, source
  placement/alpha and interrupted/coalesced revisions. Difference tolerance stays
  at the existing≤1LSB export/raster contract; no weakened comparison.
- Explicit full fallback tests for every ineligible category and reorder/removal.
- Same optimized dense fixture, unchanged pixel hashes,≥2× lower median single
  edit render time with min/median/max distributions. Cold performance should not
  materially regress. Bounded working pixels and one retained prior scene/image.
- Cancellation preserves the completed image/key; no partial publication and no
  idle repaint loops. GUI upload/visible latency remain separately measured.

If these gates fail, leave damage disabled and report the measured reason. This
plan does not authorize speculative whole-renderer replacement.

The first cropped-camera prototype failed the unchanged≤1LSB gate (2LSB on rotated
translucent edges). The implementation now retains original frame/transform and
full-stride bounded scratch storage, samples background only inside damage, draws
intersecting items in their original coordinates, and publishes only that region.
This removes translation-dependent rasterizer roundoff without changing quality.
Working storage remains within the existing full-surface budget; damaged pixel
count is reported separately from allocated working-surface pixels.

Final review added a conservative f32 precision gate for large translated geometry,
named/function command resolution during text editing, and worker retirement of
Send renderer/metrics payloads. The engine itself stays on its owning thread.
Implementation, benchmark distributions and exact remaining limits are recorded
in [RETAINED_DAMAGE_AND_CLEANUP.md](RETAINED_DAMAGE_AND_CLEANUP.md).
