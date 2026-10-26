// Copyright (C) 2026 mg-chao
// SPDX-License-Identifier: Apache-2.0

use std::sync::OnceLock;

use ant_design_gpui::{
    Button, ConfigScope, Direction,
    button::ButtonType,
    icons::{IconName, IconPalette, icon},
    theme::AlgorithmChain,
};
use gpui::{
    AnyElement, AppContext, Context, Entity, FontWeight, IntoElement, Pixels, Render, Role,
    ScrollAnchor, ScrollHandle, Subscription, Window, div, prelude::*, px, relative,
};

use super::{
    button::{ButtonPage, DEMOS, Preview},
    catalog::{COMPONENTS, ComponentId},
    checkbox::{self, CheckboxPage},
    divider::{self, DividerPage},
};

pub(super) fn sidebar_width(viewport_width: Pixels) -> Pixels {
    px(if viewport_width >= px(1040.) {
        232.
    } else {
        72.
    })
}

pub(super) fn page_padding(viewport_width: Pixels) -> Pixels {
    px(if viewport_width < px(800.) { 24. } else { 32. })
}

fn theme_algorithms() -> &'static [AlgorithmChain; 4] {
    static ALGORITHMS: OnceLock<[AlgorithmChain; 4]> = OnceLock::new();
    ALGORITHMS.get_or_init(|| {
        [
            AlgorithmChain::default(),
            AlgorithmChain::dark(),
            AlgorithmChain::compact(),
            AlgorithmChain::dark_compact(),
        ]
    })
}

#[derive(Clone, Copy)]
enum DisplayControl {
    Theme,
    Density,
    Direction,
    Motion,
}

pub(super) struct Gallery {
    pub scope: ConfigScope,
    pub selected: ComponentId,
    pub button: Entity<ButtonPage>,
    pub checkbox: Entity<CheckboxPage>,
    pub divider: Entity<DividerPage>,
    pub scroll: ScrollHandle,
    anchors: Vec<ScrollAnchor>,
    checkbox_anchors: Vec<ScrollAnchor>,
    divider_anchors: Vec<ScrollAnchor>,
    _config: Subscription,
}

impl Gallery {
    pub fn new(scope: ConfigScope, cx: &mut Context<Self>) -> Self {
        let scroll = ScrollHandle::new();
        let anchors: Vec<_> = DEMOS
            .iter()
            .map(|_| ScrollAnchor::for_handle(scroll.clone()))
            .collect();
        let checkbox_anchors: Vec<_> = checkbox::DEMOS
            .iter()
            .map(|_| ScrollAnchor::for_handle(scroll.clone()))
            .collect();
        let divider_anchors: Vec<_> = divider::DEMOS
            .iter()
            .map(|_| ScrollAnchor::for_handle(scroll.clone()))
            .collect();
        Self {
            button: cx.new(|cx| ButtonPage::new(scope.clone(), anchors.clone(), cx)),
            checkbox: cx.new(|cx| CheckboxPage::new(scope.clone(), checkbox_anchors.clone(), cx)),
            divider: cx.new(|cx| DividerPage::new(scope.clone(), divider_anchors.clone(), cx)),
            _config: scope.observe(cx),
            scope,
            selected: ComponentId::Button,
            scroll,
            anchors,
            checkbox_anchors,
            divider_anchors,
        }
    }

    fn appearance(&self, cx: &gpui::App) -> (bool, bool) {
        let config = self.scope.snapshot(cx);
        let algorithms = config.theme_config().algorithms.as_ref();
        let presets = theme_algorithms();
        (
            algorithms == Some(&presets[1]) || algorithms == Some(&presets[3]),
            algorithms == Some(&presets[2]) || algorithms == Some(&presets[3]),
        )
    }

    fn toggle(&mut self, control: DisplayControl, cx: &mut Context<Self>) {
        let config = self.scope.snapshot(cx);
        let (mut dark, mut compact) = self.appearance(cx);
        self.scope
            .update_patch(cx, |patch| match control {
                DisplayControl::Theme | DisplayControl::Density => {
                    match control {
                        DisplayControl::Theme => dark = !dark,
                        DisplayControl::Density => compact = !compact,
                        _ => unreachable!(),
                    }
                    patch.theme.get_or_insert_default().algorithms = Some(
                        theme_algorithms()[usize::from(dark) + 2 * usize::from(compact)].clone(),
                    );
                }
                DisplayControl::Direction => {
                    patch.direction = Some(if config.direction == Direction::Ltr {
                        Direction::Rtl
                    } else {
                        Direction::Ltr
                    })
                }
                DisplayControl::Motion => patch.motion = Some(!config.motion),
            })
            .expect("valid gallery display settings");
        cx.notify();
    }

    fn display_control(
        &self,
        control: DisplayControl,
        id: &'static str,
        label: &'static str,
        icon: IconName,
        active: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let config = self.scope.snapshot(cx);
        let tokens = &config.theme.tokens;
        div()
            .id(id)
            .debug_selector(move || id.into())
            .child(
                Button::new(id)
                    .automation_id(id)
                    .scope(self.scope.clone())
                    .button_type(ButtonType::Text)
                    .label(label)
                    .icon(icon)
                    .accessibility_description(match control {
                        DisplayControl::Theme => "Toggle light and dark themes",
                        DisplayControl::Density => "Toggle standard and compact density",
                        DisplayControl::Direction => {
                            "Toggle left-to-right and right-to-left component previews"
                        }
                        DisplayControl::Motion => "Toggle component animations",
                    })
                    .when(active, |button| {
                        button
                            .bg(tokens.color_primary_bg)
                            .text_color(tokens.color_primary)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(control, cx))),
            )
            .into_any_element()
    }

    fn top_control(&self, id: &'static str, label: &'static str, cx: &Context<Self>) -> AnyElement {
        div()
            .id(id)
            .debug_selector(move || id.into())
            .child(
                Button::new(id)
                    .scope(self.scope.clone())
                    .button_type(ButtonType::Text)
                    .label(label)
                    .icon(IconName::ArrowUpOutlined)
                    .automation_id(id)
                    .accessibility_label("Back to top")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.scroll.set_offset(Default::default());
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    fn toolbar(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let config = self.scope.snapshot(cx);
        let tokens = &config.theme.tokens;
        let (dark, compact) = self.appearance(cx);
        div()
            .id("gallery.toolbar")
            .debug_selector(|| "gallery.toolbar".into())
            .role(Role::Group)
            .aria_label("Preview settings")
            .flex_shrink_0()
            .min_h(px(64.))
            .px(px(24.))
            .py(px(12.))
            .border_b_1()
            .border_color(tokens.color_border_secondary)
            .bg(tokens.color_bg_container)
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .when(window.viewport_size().width >= px(1040.), |toolbar| {
                toolbar.child(div().text_color(tokens.color_text_tertiary).child(
                    match self.selected {
                        ComponentId::Button => "General / Button",
                        ComponentId::Divider => "Layout / Divider",
                        ComponentId::Checkbox => "Data Entry / Checkbox",
                    },
                ))
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(4.))
                    .children([
                        self.display_control(
                            DisplayControl::Theme,
                            "gallery.control.theme",
                            if dark { "Dark" } else { "Light" },
                            if dark {
                                IconName::MoonOutlined
                            } else {
                                IconName::SunOutlined
                            },
                            dark,
                            cx,
                        ),
                        self.display_control(
                            DisplayControl::Density,
                            "gallery.control.density",
                            if compact { "Compact" } else { "Comfortable" },
                            IconName::CompressOutlined,
                            compact,
                            cx,
                        ),
                        self.display_control(
                            DisplayControl::Direction,
                            "gallery.control.direction",
                            if config.direction == Direction::Rtl {
                                "RTL"
                            } else {
                                "LTR"
                            },
                            IconName::SwapOutlined,
                            config.direction == Direction::Rtl,
                            cx,
                        ),
                        self.display_control(
                            DisplayControl::Motion,
                            "gallery.control.motion",
                            if config.motion {
                                "Motion on"
                            } else {
                                "Motion off"
                            },
                            if config.motion {
                                IconName::PlayCircleOutlined
                            } else {
                                IconName::PauseCircleOutlined
                            },
                            !config.motion,
                            cx,
                        ),
                        self.top_control("gallery.control.top", "", cx),
                    ]),
            )
            .into_any_element()
    }

    fn navigation(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let config = self.scope.snapshot(cx);
        let tokens = &config.theme.tokens;
        let expanded = sidebar_width(window.viewport_size().width) > px(72.);
        let mut entries = div().flex().flex_col().gap(px(4.));
        let mut group = None;
        for component in COMPONENTS {
            let id = component.id;
            if expanded && group != Some(component.group) {
                entries = entries.child(
                    div()
                        .px(px(12.))
                        .pt(px(if group.is_some() { 16. } else { 0. }))
                        .pb(px(4.))
                        .text_size(px(12.))
                        .text_color(tokens.color_text_tertiary)
                        .child(component.group),
                );
            }
            group = Some(component.group);
            entries = entries.child(
                div()
                    .id(component.navigation_id)
                    .debug_selector(|| component.navigation_id.into())
                    .child(
                        Button::new(component.navigation_id)
                            .automation_id(component.navigation_id)
                            .scope(self.scope.clone())
                            .label(if expanded { component.title } else { "" })
                            .icon(match id {
                                ComponentId::Divider => IconName::BorderHorizontalOutlined,
                                _ => IconName::AppstoreOutlined,
                            })
                            .accessibility_label(component.title)
                            .accessibility_description(if self.selected == id {
                                "Selected component"
                            } else {
                                "View component examples"
                            })
                            .button_type(ButtonType::Text)
                            .block(true)
                            .w_full()
                            .h(px(40.))
                            .when(expanded, |button| button.justify_start())
                            .when(self.selected == id, |button| {
                                button
                                    .bg(tokens.color_primary_bg)
                                    .text_color(tokens.color_primary)
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if this.selected != id {
                                    this.selected = id;
                                    this.scroll.set_offset(Default::default());
                                    cx.notify();
                                }
                            })),
                    ),
            );
        }
        if expanded {
            entries = entries.child(
                div()
                    .px(px(12.))
                    .pt(px(24.))
                    .pb(px(8.))
                    .text_size(px(12.))
                    .text_color(tokens.color_text_tertiary)
                    .child("On this page"),
            );
            let links = DEMOS
                .iter()
                .zip(&self.anchors)
                .filter(|_| self.selected == ComponentId::Button)
                .map(|(demo, anchor)| {
                    (
                        demo.id,
                        demo.title,
                        matches!(demo.preview, Preview::Pending(_)),
                        anchor,
                    )
                })
                .chain(
                    checkbox::DEMOS
                        .iter()
                        .zip(&self.checkbox_anchors)
                        .filter(|_| self.selected == ComponentId::Checkbox)
                        .map(|(demo, anchor)| (demo.id, demo.title, false, anchor)),
                )
                .chain(
                    divider::DEMOS
                        .iter()
                        .zip(&self.divider_anchors)
                        .filter(|_| self.selected == ComponentId::Divider)
                        .map(|(demo, anchor)| (demo.id, demo.title, false, anchor)),
                );
            for (id, title, pending, anchor) in links {
                let anchor = anchor.clone();
                let debug_id = format!("gallery.jump.{id}");
                entries = entries.child(
                    div().id(id).debug_selector(move || debug_id.clone()).child(
                        Button::new(id)
                            .scope(self.scope.clone())
                            .button_type(ButtonType::Text)
                            .block(true)
                            .h(px(30.))
                            .px(px(12.))
                            .justify_start()
                            .label(match id {
                                "custom-disabled-bg" => "Disabled background",
                                "style-class" => "Semantic styling",
                                "divider-size" => "Spacing size",
                                "divider-style-class" => "Semantic styling",
                                _ => title,
                            })
                            .text_size(px(12.))
                            .text_color(if pending {
                                tokens.color_text_tertiary
                            } else {
                                tokens.color_text_secondary
                            })
                            .accessibility_description(format!(
                                "Jump to {}{}",
                                title,
                                if pending { ", planned preview" } else { "" }
                            ))
                            .on_click(cx.listener(move |_, _, window, cx| {
                                anchor.scroll_to(window, cx);
                                let entity = cx.weak_entity();
                                window.on_next_frame(move |_, cx| {
                                    let _ = entity.update(cx, |_, cx| cx.notify());
                                });
                                cx.notify();
                            })),
                    ),
                );
            }
        }
        div()
            .id("gallery.navigation")
            .role(Role::Navigation)
            .aria_label("Components and examples")
            .debug_selector(|| "gallery.sidebar".into())
            .w(sidebar_width(window.viewport_size().width))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(tokens.color_border_secondary)
            .bg(tokens.color_bg_container)
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(88.))
                    .flex_shrink_0()
                    .px(px(if expanded { 24. } else { 20. }))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        icon(IconName::AntDesignOutlined)
                            .size(px(28.))
                            .palette(IconPalette {
                                text: tokens.color_primary.to_rgba(),
                                ..config.icon_palette
                            }),
                    )
                    .when(expanded, |brand| {
                        brand.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Ant Design GPUI"),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(tokens.color_text_tertiary)
                                        .child("Component gallery"),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .id("gallery.navigation.scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .px(px(if expanded { 12. } else { 16. }))
                    .when(expanded, |nav| {
                        nav.child(
                            div()
                                .px(px(12.))
                                .pb(px(8.))
                                .text_size(px(12.))
                                .text_color(tokens.color_text_tertiary)
                                .child("Components"),
                        )
                    })
                    .child(entries),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(tokens.color_border_secondary)
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        Button::new("gallery.reference")
                            .scope(self.scope.clone())
                            .button_type(ButtonType::Text)
                            .block(true)
                            .w_full()
                            .label(if expanded { "Ant Design reference" } else { "" })
                            .icon(IconName::BookOutlined)
                            .accessibility_label(match self.selected {
                                ComponentId::Button => "Open Ant Design Button reference",
                                ComponentId::Divider => "Open Ant Design Divider reference",
                                ComponentId::Checkbox => "Open Ant Design Checkbox reference",
                            })
                            .href(match self.selected {
                                ComponentId::Button => "https://ant.design/components/button",
                                ComponentId::Divider => "https://ant.design/components/divider",
                                ComponentId::Checkbox => "https://ant.design/components/checkbox",
                            }),
                    )
                    .when(expanded, |footer| {
                        footer.child(
                            div()
                                .px(px(12.))
                                .text_size(px(12.))
                                .text_color(tokens.color_text_tertiary)
                                .child("Ant Design 6.6.7 · GPUI"),
                        )
                    }),
            )
            .into_any_element()
    }
}

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let config = self.scope.snapshot(cx);
        let tokens = &config.theme.tokens;
        let component = COMPONENTS
            .iter()
            .find(|component| component.id == self.selected)
            .expect("registered component page");
        let live = match self.selected {
            ComponentId::Button => DEMOS
                .iter()
                .filter(|demo| !matches!(demo.preview, Preview::Pending(_)))
                .count(),
            ComponentId::Checkbox => checkbox::DEMOS.len(),
            ComponentId::Divider => divider::DEMOS.len(),
        };
        div()
            .id("gallery")
            .size_full()
            .flex()
            .overflow_hidden()
            .text_left()
            .child(self.navigation(window, cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(self.toolbar(window, cx))
                    .child(
                        div()
                            .id("gallery.content")
                            .role(Role::Main)
                            .aria_label(format!("{} examples", component.title))
                            .debug_selector(|| "gallery.content".into())
                            .flex_1()
                            .min_h(px(0.))
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .child(
                                div()
                                    .w_full()
                                    .max_w(px(1360.))
                                    .mx_auto()
                                    .p(page_padding(window.viewport_size().width))
                                    .flex()
                                    .flex_col()
                                    .gap(px(24.))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_wrap()
                                            .items_start()
                                            .justify_between()
                                            .gap(px(16.))
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(8.))
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .text_color(tokens.color_text_tertiary)
                                                            .child(component.group),
                                                    )
                                                    .child(
                                                        div()
                                                            .id("gallery.title")
                                                            .role(Role::Heading)
                                                            .text_size(px(32.))
                                                            .line_height(relative(1.25))
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .child(component.title),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_color(tokens.color_text_secondary)
                                                            .child(component.description),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(tokens.color_text_tertiary)
                                                    .child(format!("{live} live examples")),
                                            ),
                                    )
                                    .child(match self.selected {
                                        ComponentId::Button => {
                                            self.button.clone().into_any_element()
                                        }
                                        ComponentId::Checkbox => {
                                            self.checkbox.clone().into_any_element()
                                        }
                                        ComponentId::Divider => {
                                            self.divider.clone().into_any_element()
                                        }
                                    })
                                    .child(
                                        div()
                                            .flex()
                                            .flex_wrap()
                                            .items_center()
                                            .justify_between()
                                            .gap(px(16.))
                                            .text_size(px(12.))
                                            .text_color(tokens.color_text_tertiary)
                                            .child(match self.selected {
                                                ComponentId::Button => {
                                                    "Tab to navigate · Enter or Space to activate"
                                                }
                                                ComponentId::Checkbox => {
                                                    "Tab to navigate · Space to toggle"
                                                }
                                                ComponentId::Divider => {
                                                    "Start and end follow the preview direction"
                                                }
                                            })
                                            .child(
                                                div()
                                                    .id("gallery.footer.top")
                                                    .debug_selector(|| "gallery.footer.top".into())
                                                    .child(
                                                        Button::new("gallery.top")
                                                            .scope(self.scope.clone())
                                                            .button_type(ButtonType::Text)
                                                            .label("Back to top")
                                                            .icon(IconName::ArrowUpOutlined)
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    this.scroll.set_offset(
                                                                        Default::default(),
                                                                    );
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    ),
                                            ),
                                    ),
                            ),
                    ),
            )
    }
}
