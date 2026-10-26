// Copyright (C) 2026 mg-chao
// SPDX-License-Identifier: Apache-2.0

#![doc = include_str!("../README.md")]

pub mod behavior;
pub mod button;
pub mod checkbox;
pub mod config;
pub mod divider;
pub mod icons;
pub mod locale;
pub mod root;
pub mod theme;

pub use button::Button;
pub use checkbox::{Checkbox, CheckboxChange, CheckboxGroup, CheckboxOption};
pub use config::{
    ConfigPatch, ConfigScope, Direction, ResolvedConfig, Size, ValidationStatus, Variant,
};
pub use divider::Divider;
pub use root::UiRoot;

/// Install application defaults and namespaced keyboard bindings once per GPUI app.
///
/// Existing application keymaps and assets are preserved. Native backends remain
/// the responsibility of the application.
pub fn init(cx: &mut gpui::App) {
    config::initialize(cx);
    behavior::initialize(cx);
}
