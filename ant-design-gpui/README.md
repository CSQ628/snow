# Ant Design GPUI

An Apache-2.0 Rust foundation for Ant Design controls built with GPUI. Button will
be the first public control. This crate provides themes, retained configuration,
localization, all 829 pinned icons, focus, accessibility and motion infrastructure.

The reference is Ant Design **6.6.7**, revision
`f8f5595ab5e891fff6a2a06c5b3b98af643d008e`. Windows native behavior is verified;
macOS/Linux APIs remain portable but require validation on those platforms.

## Native example and integration

```powershell
cd ant-design-gpui
cargo run --locked --offline --example foundation
```

The example uses private inspection probes to demonstrate theme/density, locales,
RTL, icons, focus, availability and motion. Tab/Shift+Tab navigate; Enter/Space
activate. These probes are not exported library components.

Call `init(cx)` once per application (repeated calls are safe). Create a
`ConfigScope::new_window(cx)` per window and return a retained
`UiRoot::new(scope.clone(), content_view, window, cx)` from the window constructor.
The root installs namespaced traversal, native typography, text/background colors
and a non-tab-stop focus fallback. The example also calls
`window.activate_window()`; `App::activate` is a no-op on this pinned Windows backend.

GPUI 0.2.2 is pinned to `mg-chao/gpui` revision
`2c0bf70b54c018f99419f386d1cf1105dca57661`, matching `snow-draw-engine-gpui`.
Defaults are disabled and `gpu-extensions` enabled. Consumers select their own
backend; native platform dependencies are development-only here. Keep these
projects' GPUI pins synchronized.

## Retained configuration

Scopes are explicit, cheaply cloned handles for views, callbacks and overlays.
Configuration-dependent views retain `scope.observe(cx)`'s subscription. Rendering
reads `scope.snapshot(cx)` without regenerating tokens. GPUI coalesces inherited
updates at the end of the current application update.

```rust
use ant_design_gpui::{ConfigPatch, ConfigScope, Direction, Size};
use ant_design_gpui::theme::{AlgorithmChain, Color, SeedTokenPatch, ThemeConfig};

# fn configure(cx: &mut gpui::App) -> Result<(), Box<dyn std::error::Error>> {
let window = ConfigScope::new_window(cx);
let local = window.child(ConfigPatch {
    direction: Some(Direction::Rtl),
    size: Some(Size::Small),
    theme: Some(ThemeConfig {
        seed: SeedTokenPatch {
            color_primary: Some(Color::rgb(0x722ed1)),
            ..Default::default()
        },
        algorithms: Some(AlgorithmChain::dark_compact()),
        ..Default::default()
    }),
    ..Default::default()
}, cx)?;
let accent = local.snapshot(cx).theme.tokens.color_primary;
# let _ = accent;
# Ok(())
# }
```

`ConfigScope::application(cx)` controls shared defaults. Scopes inherit unset
fields; explicit `false` overrides inherited `true`. `ThemeConfig::inherit=false`
resets theme inheritance. Locale selection replaces the base catalog before local
message overrides; direction is independent of language.

`set_patch` replaces local overrides; `update_patch` edits and resolves once.
Invalid updates leave patch/snapshot unchanged. If a parent change invalidates a
custom child algorithm, the child keeps its last good snapshot and exposes
`last_error` until recovery. Effective no-op updates preserve snapshot identity
without notifying. Weak observers avoid cycles; children release obsolete themes.

Snapshots cache fonts and icon palettes. `availability(local_disabled, busy)`,
`focus_ring()` and `motion_policy(cx)` give future controls consistent defaults.

## Complete native tokens

`SeedTokens`, `MapTokens`, `AliasTokens` and their optional-field patches come
from one inventory. Colors use sRGB; dimensions are logical pixels; durations
are `Duration`. Font stacks, easing, decorations and shadows have native types.
`Color` converts directly to GPUI `Rgba`, `Hsla` and `Fill`.

`ThemeConfig::try_resolve` validates customization. Resolution merges seeds,
runs ordered default/dark/compact algorithms, applies map overrides, derives
aliases and reapplies explicit final overrides. Custom `MappingAlgorithm`
implementations must be pure; custom steps compare by `Arc` identity. The HSV
palette algorithm and rounding follow upstream. Typed preset/tone indexing
unifies legacy spellings such as `blue-1` and `blue1`.

Future controls implement `ComponentTheme` with associated token/patch types,
default derivation, merging and application. Register through
`ComponentThemes::set` or `set_theme`; resolve with
`ResolvedTheme::component::<Marker>()`. Results are cached per type/snapshot.
Component-specific global overrides and algorithm chains are supported.

## Locales and icons

`Locale::{en_us,zh_cn,zh_tw}` provide complete typed common catalogs. `MessageKey`
is independent of translated text. `Locale::custom(id, patch)` falls back to
English per field; `LocalePatch` provides inherited overrides. Component catalogs
will accompany their controls. Button's name comes from its content or explicit
accessible label, rather than a generic icon/loading announcement.

`IconName` contains 829 variants. `IconRef` supports built-ins, static custom
descriptors outside the `antd/` namespace, and the host's existing asset paths.
No application asset source replacement is required.

```rust
use ant_design_gpui::icons::{IconName, icon};
use gpui::{Styled, px};
let search = icon(IconName::SearchOutlined).size(px(16.));
// In a view, also pass .palette(scope.snapshot(cx).icon_palette).
# let _ = search;
```

Icons are decorative; the parent control owns accessible meaning. Artwork only
mirrors when requested. Monochrome rotation uses atlas transforms; use
`LoadingOutlined` for animated loading. Static full-color transforms are cached.
Two-tone output preserves source layering/effects; its secondary default is Ant's
light palette tint of the theme primary color.

The app-owned `IconRenderer` defaults to 256 parsed entries, 512 raster entries,
16 MiB raster data and 1 MiB per raster. Limits cover cache-owned CPU data.
Active/cached GPUI scenes retain separate image leases so eviction cannot
invalidate a replayed scene. Resident-image statistics expose this working set;
weak resident lookup avoids rerasterizing visible images when CPU limits are
smaller than that set. Final lease release reclaims matching atlas entries.
Default trim/clear helpers dispose unleased images after painting; standalone
eviction returns retired images to the owner. Clear caches after replacing a host
asset in place; clear refreshes windows once to update cached scenes safely.
Warm rendering reuses immutable content identities and parsed/rasterized data.

## Focus, accessibility and motion

Retain one `ActivationGuard`, focus handle and blur subscription per actionable
control. Update availability immediately when state changes. Apply the guard to
the identified element, then the focus ring, then `Accessibility`. Disabled
controls relinquish focus and traversal; busy controls retain focus and block
activation. Pointer, keyboard and accessible actions share the guarded dispatch.

`Accessibility` preserves the underlying element's identity and lifecycle while
adding disabled/busy/invalid/read-only/hidden metadata. Existing GPUI APIs supply
values, selected/checked/expanded state and other semantics. Icon-only controls
need purpose-specific labels. Focus outlines have a transparent 1px gap and do
not alter layout/shadows. Programmatic focus preserves GPUI's input modality.

`MotionPolicy` adapts token durations/easing to GPUI animations. Zero-duration or
disabled motion is static; reduced-motion loops schedule no frames.
`ScalarTransition` provides continuous retargeting with caller-owned time and no
independent timer. Hosts propagate OS preferences with `App::set_reduce_motion`;
this GPUI revision does not do so automatically. Scope overrides never change
that application preference.

## Focused verification

Use Rust 1.98.1 and the VS 2026 x64 environment on Windows:

```powershell
cargo check --locked --offline --lib --examples --benches
cargo fmt --check
cargo clippy --locked --offline --lib --tests --examples --benches -- -D warnings
cargo test --locked --offline --lib
python scripts/generate_icons.py --check
./scripts/test-native-accessibility.ps1
./scripts/run-foundation-perf.ps1
```

These cover only the foundation crate. Native UIA verification is opt-in and
briefly opens an interactive window, checking actual names, roles, focus,
availability and invocation. Offscreen checks cover scope updates, traversal,
press cancellation, focus geometry, animation scheduling and icon atlas reuse.

Static fixtures compare the complete seed/map/alias inventory against execution
of pinned upstream TypeScript, colors 8.0.1 and fast-color 3.0.1. Regeneration and
provenance are documented in `fixtures/theme/`. Cargo needs no Node/Python,
external checkout or network for fixture/icon generation.

The performance script verifies the `windows-msvc-performance` tree, then runs
Cargo's optimized Release benchmark beneath it. Measurements cover cold themes,
deep scope updates, no-op/read paths, warm two-tone rendering and cache churn.
Warm paths assert unchanged parse/raster misses. Timings are diagnostic rather
than hardware-independent CI thresholds.

See [LICENSE](LICENSE), [COPYRIGHT](COPYRIGHT) and
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md), plus the recorded
[validation results](VALIDATION.md). Generated builds, measurements
and license bundles stay under the repository's ignored `build/` directory.
