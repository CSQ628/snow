// Copyright (C) 2026 mg-chao
// SPDX-License-Identifier: Apache-2.0
// Common messages adapted from Ant Design 6.6.7 (MIT); see THIRD_PARTY_NOTICES.md.

//! Typed, immutable locale catalogs. Locale selection never changes layout direction.

use gpui::SharedString;
use std::sync::{Arc, LazyLock};

macro_rules! common_messages {
    ($($field:ident => $key:ident),+ $(,)?) => {
        /// All common messages required by a complete locale catalog.
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct CommonMessages { $(pub $field: SharedString,)+ }

        /// A partial override. An absent field inherits its parent's message.
        #[derive(Clone, Debug, Default, PartialEq, Eq)]
        pub struct CommonMessagePatch { $(pub $field: Option<SharedString>,)+ }

        /// Stable message keys, independent of their translated text.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum MessageKey { $($key,)+ }

        impl MessageKey {
            pub const ALL: &'static [Self] = &[$(Self::$key,)+];
        }

        impl CommonMessages {
            pub fn text(&self, key: MessageKey) -> &SharedString {
                match key { $(MessageKey::$key => &self.$field,)+ }
            }

            pub fn patched(&self, patch: &CommonMessagePatch) -> Self {
                Self { $($field: patch.$field.clone().unwrap_or_else(|| self.$field.clone()),)+ }
            }
        }

        impl CommonMessagePatch {
            pub fn is_empty(&self) -> bool { $(self.$field.is_none())&&+ }
        }
    }
}

common_messages! {
    placeholder => Placeholder,
    close => Close,
    sortable => Sortable,
    show => Show,
    hide => Hide,
    clear_input => ClearInput,
    search => Search,
    show_password => ShowPassword,
    hide_password => HidePassword,
    increase_value => IncreaseValue,
    decrease_value => DecreaseValue,
    verification_code => VerificationCode,
    code_character => CodeCharacter,
    no_data => NoData,
    clear_selection => ClearSelection,
    remove_selection => RemoveSelection,
}

/// Popconfirm-owned messages, independent of application-supplied titles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PopconfirmMessages {
    pub ok_text: SharedString,
    pub cancel_text: SharedString,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PopconfirmMessagePatch {
    pub ok_text: Option<SharedString>,
    pub cancel_text: Option<SharedString>,
}

impl PopconfirmMessages {
    fn patched(&self, patch: &PopconfirmMessagePatch) -> Self {
        Self {
            ok_text: patch
                .ok_text
                .clone()
                .unwrap_or_else(|| self.ok_text.clone()),
            cancel_text: patch
                .cancel_text
                .clone()
                .unwrap_or_else(|| self.cancel_text.clone()),
        }
    }
}

/// Locale identity is metadata, never a translated display label.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum LocaleId {
    #[default]
    EnUs,
    ZhCn,
    ZhTw,
    Custom(SharedString),
}

impl LocaleId {
    pub fn as_str(&self) -> &str {
        match self {
            Self::EnUs => "en_US",
            Self::ZhCn => "zh_CN",
            Self::ZhTw => "zh_TW",
            Self::Custom(id) => id.as_ref(),
        }
    }
}

/// A fully resolved catalog. Future controls add typed component catalogs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Locale {
    pub id: LocaleId,
    pub common: CommonMessages,
    pub popconfirm: PopconfirmMessages,
}

impl Locale {
    pub fn en_us() -> Arc<Self> {
        static VALUE: LazyLock<Arc<Locale>> = LazyLock::new(|| {
            Arc::new(Locale {
                id: LocaleId::EnUs,
                popconfirm: PopconfirmMessages {
                    ok_text: "OK".into(),
                    cancel_text: "Cancel".into(),
                },
                common: CommonMessages {
                    placeholder: "Please select".into(),
                    close: "Close".into(),
                    sortable: "sortable".into(),
                    show: "Show".into(),
                    hide: "Hide".into(),
                    clear_input: "Clear input".into(),
                    search: "Search".into(),
                    show_password: "Show password".into(),
                    hide_password: "Hide password".into(),
                    increase_value: "Increase value".into(),
                    decrease_value: "Decrease value".into(),
                    verification_code: "Verification code".into(),
                    code_character: "Character {index} of {length}".into(),
                    no_data: "No data".into(),
                    clear_selection: "Clear selection".into(),
                    remove_selection: "Remove {label}".into(),
                },
            })
        });
        VALUE.clone()
    }

    pub fn zh_cn() -> Arc<Self> {
        static VALUE: LazyLock<Arc<Locale>> = LazyLock::new(|| {
            Arc::new(Locale {
                id: LocaleId::ZhCn,
                popconfirm: PopconfirmMessages {
                    ok_text: "确定".into(),
                    cancel_text: "取消".into(),
                },
                common: CommonMessages {
                    placeholder: "请选择".into(),
                    close: "关闭".into(),
                    sortable: "可排序".into(),
                    show: "显示".into(),
                    hide: "隐藏".into(),
                    clear_input: "清空输入".into(),
                    search: "搜索".into(),
                    show_password: "显示密码".into(),
                    hide_password: "隐藏密码".into(),
                    increase_value: "增加数值".into(),
                    decrease_value: "减少数值".into(),
                    verification_code: "验证码".into(),
                    code_character: "第 {index} 位，共 {length} 位".into(),
                    no_data: "暂无数据".into(),
                    clear_selection: "清空选择".into(),
                    remove_selection: "移除 {label}".into(),
                },
            })
        });
        VALUE.clone()
    }

    pub fn zh_tw() -> Arc<Self> {
        static VALUE: LazyLock<Arc<Locale>> = LazyLock::new(|| {
            Arc::new(Locale {
                id: LocaleId::ZhTw,
                popconfirm: PopconfirmMessages {
                    ok_text: "確定".into(),
                    cancel_text: "取消".into(),
                },
                common: CommonMessages {
                    placeholder: "請選擇".into(),
                    close: "關閉".into(),
                    sortable: "可排序".into(),
                    show: "顯示".into(),
                    hide: "隱藏".into(),
                    clear_input: "清空輸入".into(),
                    search: "搜尋".into(),
                    show_password: "顯示密碼".into(),
                    hide_password: "隱藏密碼".into(),
                    increase_value: "增加數值".into(),
                    decrease_value: "減少數值".into(),
                    verification_code: "驗證碼".into(),
                    code_character: "第 {index} 位，共 {length} 位".into(),
                    no_data: "暫無數據".into(),
                    clear_selection: "清空選擇".into(),
                    remove_selection: "移除 {label}".into(),
                },
            })
        });
        VALUE.clone()
    }

    /// Create a custom catalog with English fallback for every unspecified field.
    pub fn custom(id: impl Into<SharedString>, common: CommonMessagePatch) -> Arc<Self> {
        Arc::new(Self {
            id: LocaleId::Custom(id.into()),
            common: Self::en_us().common.patched(&common),
            popconfirm: Self::en_us().popconfirm.clone(),
        })
    }

    pub fn text(&self, key: MessageKey) -> &SharedString {
        self.common.text(key)
    }
}

/// Selecting a locale replaces the base catalog; common overrides are applied afterward.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalePatch {
    pub locale: Option<Arc<Locale>>,
    pub common: CommonMessagePatch,
    pub popconfirm: PopconfirmMessagePatch,
}

impl LocalePatch {
    pub fn resolve(&self, parent: &Arc<Locale>) -> Arc<Locale> {
        self.resolve_with_previous(parent, None)
    }

    /// Retain a scoped catalog when an unrelated parent setting changes.
    pub fn resolve_with_previous(
        &self,
        parent: &Arc<Locale>,
        previous: Option<&Arc<Locale>>,
    ) -> Arc<Locale> {
        let base = self.locale.as_ref().unwrap_or(parent);
        if self.common.is_empty() && self.popconfirm == PopconfirmMessagePatch::default() {
            return base.clone();
        }
        let common = base.common.patched(&self.common);
        let popconfirm = base.popconfirm.patched(&self.popconfirm);
        if common == base.common && popconfirm == base.popconfirm {
            return base.clone();
        }
        if let Some(previous) = previous
            .filter(|old| old.id == base.id && old.common == common && old.popconfirm == popconfirm)
        {
            return previous.clone();
        }
        Arc::new(Locale {
            id: base.id.clone(),
            common,
            popconfirm,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popconfirm_catalogs_and_field_overrides_follow_locale_changes() {
        assert_eq!(Locale::en_us().popconfirm.ok_text.as_ref(), "OK");
        assert_eq!(Locale::en_us().popconfirm.cancel_text.as_ref(), "Cancel");
        assert_eq!(Locale::zh_cn().popconfirm.ok_text.as_ref(), "确定");
        assert_eq!(Locale::zh_tw().popconfirm.ok_text.as_ref(), "確定");
        let patch = LocalePatch {
            popconfirm: PopconfirmMessagePatch {
                ok_text: Some("Delete".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let english = patch.resolve(&Locale::en_us());
        assert_eq!(english.popconfirm.ok_text.as_ref(), "Delete");
        assert_eq!(english.popconfirm.cancel_text.as_ref(), "Cancel");
        let chinese = patch.resolve(&Locale::zh_cn());
        assert_eq!(chinese.popconfirm.ok_text.as_ref(), "Delete");
        assert_eq!(chinese.popconfirm.cancel_text.as_ref(), "取消");
        let switched = LocalePatch {
            locale: Some(Locale::zh_tw()),
            ..Default::default()
        }
        .resolve(&english);
        assert_eq!(switched.popconfirm.ok_text.as_ref(), "確定");
    }

    #[test]
    fn popconfirm_equivalent_overrides_and_previous_results_share_catalogs() {
        let english = Locale::en_us();
        let equivalent = LocalePatch {
            popconfirm: PopconfirmMessagePatch {
                ok_text: Some("OK".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(Arc::ptr_eq(&english, &equivalent.resolve(&english)));
        let patch = LocalePatch {
            popconfirm: PopconfirmMessagePatch {
                ok_text: Some("Delete".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let previous = patch.resolve(&english);
        assert!(Arc::ptr_eq(
            &previous,
            &patch.resolve_with_previous(&english, Some(&previous))
        ));
        let custom = Locale::custom("fr_FR", CommonMessagePatch::default());
        assert_eq!(custom.popconfirm, english.popconfirm);
    }

    #[test]
    fn builtin_catalogs_are_complete_and_english_is_identity() {
        for locale in [Locale::en_us(), Locale::zh_cn(), Locale::zh_tw()] {
            for key in MessageKey::ALL {
                assert!(
                    !locale.text(*key).is_empty(),
                    "{}: {key:?}",
                    locale.id.as_str()
                );
            }
        }
        assert_eq!(Locale::en_us().common.placeholder.as_ref(), "Please select");
        assert_eq!(Locale::zh_cn().common.close.as_ref(), "关闭");
        assert_eq!(Locale::zh_tw().common.close.as_ref(), "關閉");
    }

    #[test]
    fn custom_and_scoped_overrides_fall_back_per_field() {
        let custom = Locale::custom(
            "fr_FR",
            CommonMessagePatch {
                close: Some("Fermer".into()),
                ..Default::default()
            },
        );
        assert_eq!(custom.common.close.as_ref(), "Fermer");
        assert_eq!(custom.common.hide.as_ref(), "Hide");
        let scoped = LocalePatch {
            common: CommonMessagePatch {
                show: Some("Afficher".into()),
                ..Default::default()
            },
            ..Default::default()
        }
        .resolve(&custom);
        assert_eq!(scoped.common.close.as_ref(), "Fermer");
        assert_eq!(scoped.common.show.as_ref(), "Afficher");
        let switched = LocalePatch {
            locale: Some(Locale::zh_tw()),
            ..Default::default()
        }
        .resolve(&scoped);
        assert_eq!(switched.common.show.as_ref(), "顯示");
    }

    #[test]
    fn empty_and_equivalent_overrides_reuse_catalogs() {
        let english = Locale::en_us();
        assert!(Arc::ptr_eq(
            &english,
            &LocalePatch::default().resolve(&english)
        ));
        let patch = LocalePatch {
            common: CommonMessagePatch {
                close: Some("Close".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(Arc::ptr_eq(&english, &patch.resolve(&english)));
    }
}
