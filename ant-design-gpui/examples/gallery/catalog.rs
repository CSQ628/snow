// Copyright (C) 2026 mg-chao
// SPDX-License-Identifier: Apache-2.0

//! Navigation metadata stays independent of component page implementations.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ComponentId {
    #[default]
    Button,
    Divider,
    Checkbox,
    Radio,
    Segmented,
    Switch,
    Tag,
}

pub(super) struct Component {
    pub id: ComponentId,
    pub navigation_id: &'static str,
    pub group: &'static str,
    pub title: &'static str,
    pub description: &'static str,
}

pub(super) const COMPONENTS: &[Component] = &[
    Component {
        id: ComponentId::Button,
        navigation_id: "gallery.navigation.button",
        group: "General",
        title: "Button",
        description: "To trigger an operation.",
    },
    Component {
        id: ComponentId::Divider,
        navigation_id: "gallery.navigation.divider",
        group: "Layout",
        title: "Divider",
        description: "A divider line separates different content.",
    },
    Component {
        id: ComponentId::Checkbox,
        navigation_id: "gallery.navigation.checkbox",
        group: "Data Entry",
        title: "Checkbox",
        description: "Collect user's choices.",
    },
    Component {
        id: ComponentId::Radio,
        navigation_id: "gallery.navigation.radio",
        group: "Data Entry",
        title: "Radio",
        description: "Used to select a single state from multiple options.",
    },
    Component {
        id: ComponentId::Segmented,
        navigation_id: "gallery.navigation.segmented",
        group: "Data Entry",
        title: "Segmented",
        description: "Display multiple options and select a single option.",
    },
    Component {
        id: ComponentId::Switch,
        navigation_id: "gallery.navigation.switch",
        group: "Data Entry",
        title: "Switch",
        description: "Used to toggle between two states.",
    },
    Component {
        id: ComponentId::Tag,
        navigation_id: "gallery.navigation.tag",
        group: "Data Display",
        title: "Tag",
        description: "Used for marking and categorization.",
    },
];
