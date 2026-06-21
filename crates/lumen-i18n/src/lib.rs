//! i18n lookups for Lumen, backed by embedded Fluent (FTL) files.
//!
//! ```ignore
//! use lumen_i18n::{t, t_attr};
//!
//! let text = t("app-name");
//! let desc = t_attr("app-name", "description");
//! ```

use std::{env, sync::OnceLock};

use i18n_embed::{
    LanguageLoader,
    fluent::{FluentLanguageLoader, fluent_language_loader},
};
use rust_embed::RustEmbed;
use unic_langid::{LanguageIdentifier, langid};

#[derive(RustEmbed)]
#[folder = "locales/"]
struct Localizations;

static LOADER: OnceLock<FluentLanguageLoader> = OnceLock::new();

/// Returns the Fluent loader, detecting system locale on first call and
/// falling back to en-US.
///
/// # Panics
///
/// Panics if embedded FTL resources fail to load.
#[allow(clippy::expect_used)]
pub fn loader() -> &'static FluentLanguageLoader {
    LOADER.get_or_init(|| {
        let loader = fluent_language_loader!();
        loader
            .load_fallback_language(&Localizations)
            .expect("embedded FTL resources are valid");

        let requested = requested_languages();
        let _ = i18n_embed::select(&loader, &Localizations, &requested);

        loader
    })
}

/// Returns the localized string for `key`, or the key itself if no FTL entry exists.
pub fn t(key: &str) -> String {
    loader().get(key)
}

/// Returns a localized attribute value, or the attribute path if no FTL entry exists.
pub fn t_attr(key: &str, attr: &str) -> String {
    loader().get_attr(key, attr)
}

fn requested_languages() -> Vec<LanguageIdentifier> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|key| env::var(key).ok())
        .filter_map(|value| parse_locale(&value))
        .chain(std::iter::once(fallback_language()))
        .collect()
}

fn parse_locale(value: &str) -> Option<LanguageIdentifier> {
    let locale = value
        .split('.')
        .next()
        .unwrap_or(value)
        .split('@')
        .next()
        .unwrap_or(value)
        .replace('_', "-");

    if locale.eq_ignore_ascii_case("c") || locale.eq_ignore_ascii_case("posix") {
        return Some(fallback_language());
    }

    locale.parse().ok()
}

fn fallback_language() -> LanguageIdentifier {
    langid!("en-US")
}

#[cfg(test)]
mod tests {
    use super::{parse_locale, t};

    #[test]
    fn keys_from_both_files_work() {
        let _ = t("app-name");
        let _ = t("settings-bar-scale");
    }

    #[test]
    fn c_utf8_locale_maps_to_english() {
        assert_eq!(
            parse_locale("C.UTF-8").map(|locale| locale.to_string()),
            Some(String::from("en-US"))
        );
    }

    #[test]
    fn locale_encoding_suffix_is_ignored() {
        assert_eq!(
            parse_locale("fr_FR.UTF-8").map(|locale| locale.to_string()),
            Some(String::from("fr-FR"))
        );
    }
}
