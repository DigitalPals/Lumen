//! Internationalization for lumen-shell runtime labels.

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

macro_rules! t {
    ($message_id:literal) => {{
        i18n_embed_fl::fl!($crate::i18n::loader(), $message_id)
    }};
    ($message_id:literal, $($args:tt)*) => {{
        i18n_embed_fl::fl!($crate::i18n::loader(), $message_id, $($args)*)
    }};
}

pub(crate) use t;

macro_rules! td {
    ($message_id:expr) => {{ $crate::i18n::loader().get($message_id) }};
}

pub(crate) use td;

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
