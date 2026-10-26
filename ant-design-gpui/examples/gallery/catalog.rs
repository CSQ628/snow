// Copyright (C) 2026 mg-chao
// SPDX-License-Identifier: Apache-2.0

//! Navigation metadata stays independent of component page implementations.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ComponentId {
    Button,
}

pub(super) struct Component {
    pub id: ComponentId,
    pub navigation_id: &'static str,
    pub group: &'static str,
    pub title: &'static str,
    pub description: &'static str,
}

pub(super) const COMPONENTS: &[Component] = &[Component {
    id: ComponentId::Button,
    navigation_id: "gallery.navigation.button",
    group: "General",
    title: "Button",
    description: "To trigger an operation.",
}];
