# Ant Design GPUI

Apache-2.0 Ant Design controls built with GPUI. Button, Checkbox and Divider use
native layout, focus, accessibility and motion, with retained configuration,
localization, complete themes and all 829 pinned icons.

The reference is Ant Design **6.6.7**, revision
`f8f5595ab5e891fff6a2a06c5b3b98af643d008e`. Windows native behavior is verified;
macOS/Linux APIs remain portable but require validation on those platforms.

## Component gallery

```powershell
cd ant-design-gpui
cargo run --locked --offline --example gallery
# Optional: -- --dark --compact --rtl --reduced-motion
# Reproduce a narrow layout: -- --window-size 640x720 --capture C:/temp/gallery.png
# Open Checkbox directly: -- --component checkbox
# Open Divider directly: -- --component divider
```

The native gallery has a fixed component sidebar, direct example links and a
separately scrolling page. A persistent toolbar switches light/dark themes,
comfortable/compact density, preview direction and motion; its arrow returns to
the top. Display changes preserve active loading requests and scroll position
within the new page bounds. The sidebar becomes an icon rail in narrow windows.

Button's 15 public examples retain the reference checkout's order, labels and
descriptions. Nine previews are complete; six compact Planned cards explain the
capabilities needed for their full reference examples. Titles precede previews,
and cards use two columns only when the padded content has sufficient room.
The Color & Variant matrix follows the reference's responsive component sizing.
Tab/Shift+Tab navigate; Enter/Space activate controls.

Checkbox's seven public examples follow the reference order, with working
controlled state, disabled controls, groups, indeterminate Check all, native
grid layout and semantic styling. Select Checkbox in the sidebar or use
`--component checkbox`; Space toggles focused checkboxes.

Divider's nine examples cover horizontal and vertical separators, title placement,
spacing, plain text, solid/dashed/dotted lines, component tokens and semantic styling.
Select Divider in the sidebar or use `--component divider`.

Run only this example's offscreen regression tests with
`cargo test --locked --offline --example gallery`.

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

## Button

```rust
use ant_design_gpui::{Button, ConfigScope};
use ant_design_gpui::icons::IconName;

# fn save_button(scope: ConfigScope) -> impl gpui::IntoElement {
Button::new("save")
    .scope(scope)
    .label("Save")
    .primary()
    .icon(IconName::SaveOutlined)
    .on_click(|activation, window, cx| {
        // All activation paths share this callback.
    })
# }
```

Build Buttons during a view's render with stable, locally unique IDs. Internal
state retains focus and pending input across rerenders and is released on unmount.
Pass the window's `ConfigScope` with `.scope(...)`; omitting it uses application
defaults. Each Button observes its own scope, including inside cached parent views.
`.on_click(cx.listener(...))` works with a handler taking `&behavior::Activation`.

`ButtonType::{Default, Primary, Dashed, Text, Link}` supplies familiar appearances.
For independent choices, use `ButtonColor::{Default, Primary, Danger, Preset(...)}`
and `ButtonVariant::{Outlined, Dashed, Solid, Filled, Text, Link}`. Explicit color
and variant override their type defaults; `.variant(Solid)` alone implies primary.
`.danger(true)` selects danger unless color was explicit. The provider's input
`Variant` is independent of the Button variant.

Sizes inherit the provider's `Size` unless `.size(...)` overrides it. Shapes are
`Default`, `Square`, `Circle` and `Round`; upstream's square alias retains the
standard corner radius. `.block(true)` fills available width, and `.ghost(true)`
renders bordered appearances over their parent's background.

`.icon(...)` and `.icon_placement(Start | End)` follow logical RTL order. Empty
labels produce square icon-only controls; give these `.accessibility_label(...)`.
Plain two-character Chinese labels receive Ant Design's automatic space unless
an icon is present, the variant is Text/Link, or `.auto_insert_space(false)` is set.
Accessible names keep the original label. Applications localize labels and explicit
accessible descriptions through their own catalogs.

`.disabled(false)` overrides a disabled provider. Disabled controls leave the tab
order; `.loading(true)` blocks activation but retains focus. `.loading_delay(...)`
keeps the control ready until the delay expires, cancels when cleared or unmounted,
and does not restart on unrelated rerenders. `.loading_icon(...)` overrides the
default rotating loader. `.loading_icon_spin(true)` opts a custom icon into rotation;
`false` stops the default spinner. Static custom loaders do not schedule continuous frames.
Monochrome loaders rotate through the GPU atlas; colored icon rotation uses CPU
raster variants on the pinned GPUI backend.
Color transitions, loader expansion/collapse and click waves respect scoped and
application reduced motion. `.wave(false)` disables the click wave. Link controls
have no wave. Idle controls schedule no animation frames.

Pointer, Enter, Space and accessible invocation share availability guards. For
programmatic focus, pass a retained `behavior::focus_handle(cx)` using
`.focus_handle(&handle)`; externally supplied handles keep their tab settings.
`.href(...)` exposes native link semantics and opens through GPUI, with Enter
activation. `.automation_id(...)` supplies a stable native accessibility identifier.

Customize component tokens with `ComponentThemes::set::<ButtonTheme>(ButtonTokenPatch
{ ... })`. Component-local global overrides and algorithms affect the complete
control, including fonts, focus and motion. GPUI `Styled` refinements apply directly
to the native root, so relative widths, flex, margins and focus bounds agree.
`.icon_style(StyleRefinement)` and `.content_style(StyleRefinement)` customize the
icon slot and label independently while preserving the control's accessible name.

```powershell
cd ant-design-gpui
cargo run --locked --offline --example button
# Optional: --dark --compact --rtl --reduced-motion --capture C:/temp/button.png
cargo test --locked --offline --lib button::
./scripts/run-button-perf.ps1
```

The gallery contains interactive loading, theme, density, RTL, availability and
motion controls, all six variants and all 13 preset colors. The performance script
requires the repository's configured `windows-msvc-performance` Release build.
Independent [browser fixtures](fixtures/button/README.md) check 2,208 original
Ant Design computed styles across four themes and three interaction states.
`scripts/test-button-accessibility.ps1` provides opt-in Windows UI Automation checks.

## Checkbox

```rust
use ant_design_gpui::{Checkbox, ConfigScope};

# fn remember_checkbox(scope: ConfigScope) -> impl gpui::IntoElement {
Checkbox::new("remember")
    .scope(scope)
    .label("Remember me")
    .default_checked(true)
    .on_change(|change, window, cx| {
        // change.checked is the requested new value.
    })
# }
```

Use stable, locally unique IDs when building controls during render. An
uncontrolled Checkbox retains its own value; `.default_checked(...)` sets only
its initial value. `.checked(...)` supplies a controlled value: update application
state in `.on_change(cx.listener(...))` and rerender to accept the change.
`.indeterminate(true)` displays a mixed mark independently of the checked value,
which supports deriving a Check all control from the selected values.

The entire label toggles the control. Space and native accessibility actions use
the same guarded change handler. Disabled controls leave the tab order and
relinquish focus; `.disabled(false)` overrides a disabled provider. Supply
`.accessibility_label(...)` for an unlabeled Checkbox, `.automation_id(...)` for
native automation, and `.focus_handle(&handle)` for programmatic focus.

```rust
use ant_design_gpui::{ConfigScope, checkbox::{CheckboxGroup, CheckboxOption}};

# fn fruit_choices(scope: ConfigScope) -> impl gpui::IntoElement {
CheckboxGroup::new("fruit")
    .scope(scope)
    .accessibility_label("Fruit choices")
    .options([
        CheckboxOption::new("apple", "Apple", "Apple"),
        CheckboxOption::new("pear", "Pear", "Pear"),
        CheckboxOption::new("orange", "Orange", "Orange"),
    ])
    .default_value(["Apple"])
    .on_change(|values, window, cx| {
        // Values follow the current option order.
    })
# }
```

Groups accept typed values and explicit option IDs so focus remains stable when
options reorder. `.value(...)` controls selection; `.default_value(...)` initializes
retained selection. An option's explicit disabled setting overrides the group and
provider. Groups wrap with token spacing and support `.vertical(true)`; GPUI
`Styled` refinements also allow native grid layouts and full-width controls.

Checkbox follows the scope's theme, font, direction and motion policy. Ant Design
does not define Checkbox-specific component tokens; use
`ComponentThemeConfig::<CheckboxTheme>::default().with_tokens(...)` for component-local
global token overrides. Root `Styled`, `.icon_style(StyleRefinement)` and
`.label_style(StyleRefinement)` customize the semantic slots. The gallery shows
both static refinements and styles derived from controlled state.

```powershell
cargo run --locked --offline --example gallery -- --component checkbox
cargo test --locked --offline --lib checkbox::
cargo test --locked --offline --example gallery checkbox_
./scripts/run-checkbox-perf.ps1
```

Colors, ticks and click waves follow scoped motion and application reduced motion;
`.wave(false)` disables the wave. Idle controls schedule no animation frames. Tick
meshes are retained across redraws, color changes and scrolling, with no image assets.
Independent [browser fixtures](fixtures/checkbox/README.md) compare 576 original
Ant Design states across four themes, both directions, hover and keyboard focus.
`scripts/test-checkbox-accessibility.ps1` checks native Windows checkbox roles,
mixed state, toggling and keyboard interaction.

## Divider

```rust
use ant_design_gpui::{Divider, Size};
use ant_design_gpui::divider::{DividerTitlePlacement, DividerVariant};
use gpui::px;

let section = Divider::new("details")
    .label("Details")
    .title_placement(DividerTitlePlacement::Start);
let subtle = Divider::new("subsection")
    .label("More information").plain(true)
    .variant(DividerVariant::Dashed).size(Size::Small);
let inline = Divider::new("actions-separator").vertical(true);
let edge_title = Divider::new("edge-title")
    .label("Settings")
    .title_placement(DividerTitlePlacement::End)
    .title_margin(px(0.));
# let _ = (section, subtle, inline, edge_title);
```

Pass `.scope(scope.clone())` to inherit a window's theme, direction and explicitly
configured size. Unset horizontal spacing is 24px without a title and 16px with
one; small/medium spacing is 8px/16px. Large preserves the unset spacing. Theme
algorithms derive these values from the effective aliases. Vertical dividers use
0.9em height and 8px inline margins by default; place them in an `items_center()`
row alongside text or actions. Vertical dividers ignore titles and size.

`Start` and `End` mirror in RTL; `Left` and `Right` are physical placements.
`.title_margin(...)` replaces the short rail and outer padding for edge titles.
`.child(...)` accepts any GPUI title element; `.label(...)` accepts text, including
`"0"`. The last title setter wins. Titles remain on one line; narrow layouts let
the rails collapse before the title overflows, matching Ant Design.

Native `Styled` refinements apply to the root. `.rail_style(...)` and
`.content_style(...)` refine the two semantic slots. Border color/width style
the rail stroke. Register `DividerTokenPatch` through `DividerTheme` for scoped
title padding, edge rail proportions and vertical margins. Padding accepts
pixels/rems or font multiples (`relative(1.)` means 1em).

Dividers expose native separator roles and orientation without a tab stop.
Text titles supply their accessible name; use `.accessibility_label(...)` for
custom titles when necessary. They own only a scope subscription and two bounded
paint caches. Solid rails use one quad; dashed/dotted rails reuse tessellation
across redraws, scrolling and color changes, with no per-mark layout children.
Idle dividers schedule zero animation frames.

Run `cargo test --locked --offline --lib divider::` and
`cargo test --locked --offline --example gallery divider` for focused coverage.
Independent browser references and regeneration instructions are in
[`fixtures/divider/README.md`](fixtures/divider/README.md).
Run `scripts/run-divider-perf.ps1` for optimized Release measurements under
`windows-msvc-performance`.

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
`focus_ring()` and `motion_policy(cx)` give controls consistent defaults.

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

Controls implement `ComponentTheme` with associated token/patch types,
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
