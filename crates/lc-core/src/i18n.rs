//! Språk för gränssnittet. Texterna skrivs direkt där de används,
//! `t("Ångra", "Undo")`, så att båda språken alltid finns och står intill
//! varandra. Språket väljs från systemet (`LANG`) och kan ändras i menyn.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    Swedish,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::English, Language::Swedish];

    /// Språkets namn på språket självt.
    pub fn native_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Swedish => "Svenska",
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Swedish => "sv",
        }
    }

    pub fn from_code(code: &str) -> Option<Language> {
        let code = code.trim().to_ascii_lowercase();
        if code.starts_with("sv") {
            Some(Language::Swedish)
        } else if code.starts_with("en") {
            Some(Language::English)
        } else {
            None
        }
    }

    /// Från miljövariablerna (`LC_ALL`, `LC_MESSAGES`, `LANG`). Engelska om inget passar.
    pub fn from_env() -> Language {
        ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .find(|v| !v.is_empty())
            .and_then(|v| Language::from_code(&v))
            .unwrap_or(Language::English)
    }
}

static LANGUAGE: AtomicU8 = AtomicU8::new(0);

pub fn set_language(lang: Language) {
    LANGUAGE.store(lang as u8, Ordering::Relaxed);
}

pub fn language() -> Language {
    match LANGUAGE.load(Ordering::Relaxed) {
        1 => Language::Swedish,
        _ => Language::English,
    }
}

/// Texten på det valda språket.
pub fn t(sv: &'static str, en: &'static str) -> &'static str {
    match language() {
        Language::Swedish => sv,
        Language::English => en,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(Language::from_code("sv_SE.UTF-8"), Some(Language::Swedish));
        assert_eq!(Language::from_code("en_US.UTF-8"), Some(Language::English));
        assert_eq!(Language::from_code("C.UTF-8"), None);
    }

    #[test]
    fn switch() {
        set_language(Language::Swedish);
        assert_eq!(t("Ångra", "Undo"), "Ångra");
        set_language(Language::English);
        assert_eq!(t("Ångra", "Undo"), "Undo");
    }
}
