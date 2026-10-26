// Copyright (C) 2026 mg-chao
// SPDX-License-Identifier: Apache-2.0

//! Reusable interaction, semantics, focus and motion primitives for native controls.

mod accessibility;
mod activation;
mod focus;
mod motion;

pub use accessibility::{Accessibility, SemanticElement};
pub use activation::{Activation, ActivationGuard, Availability};
pub use focus::{
    FocusNext, FocusPrevious, FocusRing, FocusRingElement, focus_handle, initialize, traversal,
};
pub use motion::{MotionPolicy, ScalarTransition, TransitionSample, evaluate_cubic_bezier};

#[cfg(test)]
mod tests;
