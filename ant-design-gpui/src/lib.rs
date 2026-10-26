// Copyright (C) 2026 mg-chao
// SPDX-License-Identifier: Apache-2.0

#![doc = include_str!("../README.md")]

pub mod behavior;
pub mod button;
pub mod checkbox;
pub mod config;
pub mod divider;
pub mod icons;
pub mod input;
pub mod input_number;
pub use input_number::{
    ExactDecimal, InputNumber, InputNumberEvent, InputNumberState, NumberOptions, NumericMode,
    NumericValue,
};
pub mod locale;
pub mod radio;
pub mod root;
pub mod segmented;
pub mod switch;
pub mod tag;
pub mod theme;

pub use button::Button;
pub use checkbox::{Checkbox, CheckboxChange, CheckboxGroup, CheckboxOption};
pub use config::{
    ConfigPatch, ConfigScope, Direction, ResolvedConfig, Size, ValidationStatus, Variant,
};
pub use divider::Divider;
pub use input::{
    CountConfig, CountInfo, Input, InputChange, InputCommit, InputEvent, InputGroup,
    InputGroupContext, InputGroupItem, InputGroupOrientation, InputSearch, InputState, OtpEvent,
    OtpInput, OtpOptions, OtpState, PasswordInput, SearchInput, SearchSource, TextArea,
};
pub use radio::{Radio, RadioChange, RadioGroup, RadioOption};
pub use root::UiRoot;
pub use segmented::{Segmented, SegmentedOption};
pub use switch::{Switch, SwitchChange};
pub use tag::{CheckableTag, CheckableTagGroup, Tag, TagChange, TagClose, TagOption};

/// Install application defaults and namespaced keyboard bindings once per GPUI app.
///
/// Existing application keymaps and assets are preserved. Native backends remain
/// the responsibility of the application.
pub fn init(cx: &mut gpui::App) {
    config::initialize(cx);
    behavior::initialize(cx);
}
