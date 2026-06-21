//! Verifies that every auto-generated i18n key from the config schema
//! has a matching FTL entry with a `.description` attribute.
//!
//! Runs as part of `cargo test -p lumen-i18n`. If this fails, a config
//! field was added without a corresponding FTL entry in
//! `crates/lumen-i18n/locales/en-US/config/`.

use lumen_config::{
    EnumVariants,
    schemas::{
        bar::{BarButtonVariant, BarConfig, BorderLocation, IconPosition, Location, ShadowPreset},
        general::{GeneralConfig, Layer},
        modules::{
            ActiveIndicator, AppIconSource, CavaDirection, CavaInput, CavaStyle, DisplayMode,
            ExecutionMode, HermesChatLocalHistory, HermesChatTransportMode, IconSource,
            LabelStrategy, MediaIconType, ModulesConfig, Numbering, PopupCloseBehavior,
            PopupPosition, ProviderOrder, StackingOrder, TailscaleLabel, TemperatureUnit,
            TimeFormat, UrgencyBarThreshold, UrgentMode, WeatherProvider,
        },
        osd::{OsdConfig, OsdPosition},
        styling::{
            FontWeightClass, MatugenScheme, RoundingLevel, StylingConfig, ThemeProvider,
            WallustBackend, WallustColorspace, WallustPalette,
        },
        wallpaper::{CyclingMode, FitMode, TransitionType, WallpaperConfig},
    },
};
use lumen_i18n::loader;

#[test]
fn all_config_i18n_keys_have_ftl_entries() {
    let ftl = loader();

    let mut missing_keys = Vec::new();
    let mut missing_descriptions = Vec::new();

    let config_keys = collect_config_keys();
    let enum_keys = collect_enum_keys();

    for key in &config_keys {
        if !ftl.has(key) {
            missing_keys.push(key.to_string());
            continue;
        }

        if !ftl.has_attr(key, "description") {
            missing_descriptions.push(key.to_string());
        }
    }

    for key in &enum_keys {
        if !ftl.has(key) {
            missing_keys.push(key.to_string());
        }
    }

    let mut failures = String::new();

    if !missing_keys.is_empty() {
        failures.push_str(&format!(
            "\n{} keys missing from FTL:\n",
            missing_keys.len()
        ));

        for key in &missing_keys {
            failures.push_str(&format!("  {key}\n"));
        }
    }

    if !missing_descriptions.is_empty() {
        failures.push_str(&format!(
            "\n{} keys missing .description:\n",
            missing_descriptions.len()
        ));

        for key in &missing_descriptions {
            failures.push_str(&format!("  {key}\n"));
        }
    }

    assert!(
        failures.is_empty(),
        "FTL coverage failures ({} config keys, {} enum keys checked):{failures}",
        config_keys.len(),
        enum_keys.len()
    );
}

fn collect_config_keys() -> Vec<&'static str> {
    let mut keys = Vec::new();

    keys.extend(GeneralConfig::all_i18n_keys());
    keys.extend(BarConfig::all_i18n_keys());
    keys.extend(StylingConfig::all_i18n_keys());
    keys.extend(OsdConfig::all_i18n_keys());
    keys.extend(WallpaperConfig::all_i18n_keys());
    keys.extend(ModulesConfig::all_i18n_keys());

    keys
}

fn collect_enum_keys() -> Vec<&'static str> {
    let mut keys = Vec::new();

    extend_enum_keys::<Layer>(&mut keys);
    extend_enum_keys::<Location>(&mut keys);
    extend_enum_keys::<BorderLocation>(&mut keys);
    extend_enum_keys::<RoundingLevel>(&mut keys);
    extend_enum_keys::<ShadowPreset>(&mut keys);
    extend_enum_keys::<BarButtonVariant>(&mut keys);
    extend_enum_keys::<FontWeightClass>(&mut keys);
    extend_enum_keys::<IconPosition>(&mut keys);
    extend_enum_keys::<FitMode>(&mut keys);
    extend_enum_keys::<TransitionType>(&mut keys);
    extend_enum_keys::<CyclingMode>(&mut keys);
    extend_enum_keys::<OsdPosition>(&mut keys);
    extend_enum_keys::<ThemeProvider>(&mut keys);
    extend_enum_keys::<MatugenScheme>(&mut keys);
    extend_enum_keys::<WallustPalette>(&mut keys);
    extend_enum_keys::<WallustBackend>(&mut keys);
    extend_enum_keys::<WallustColorspace>(&mut keys);
    extend_enum_keys::<CavaInput>(&mut keys);
    extend_enum_keys::<CavaStyle>(&mut keys);
    extend_enum_keys::<CavaDirection>(&mut keys);
    extend_enum_keys::<TimeFormat>(&mut keys);
    extend_enum_keys::<WeatherProvider>(&mut keys);
    extend_enum_keys::<TemperatureUnit>(&mut keys);
    extend_enum_keys::<MediaIconType>(&mut keys);
    extend_enum_keys::<HermesChatTransportMode>(&mut keys);
    extend_enum_keys::<HermesChatLocalHistory>(&mut keys);
    extend_enum_keys::<DisplayMode>(&mut keys);
    extend_enum_keys::<Numbering>(&mut keys);
    extend_enum_keys::<UrgentMode>(&mut keys);
    extend_enum_keys::<ActiveIndicator>(&mut keys);
    extend_enum_keys::<LabelStrategy>(&mut keys);
    extend_enum_keys::<IconSource>(&mut keys);
    extend_enum_keys::<PopupPosition>(&mut keys);
    extend_enum_keys::<StackingOrder>(&mut keys);
    extend_enum_keys::<PopupCloseBehavior>(&mut keys);
    extend_enum_keys::<UrgencyBarThreshold>(&mut keys);
    extend_enum_keys::<AppIconSource>(&mut keys);
    extend_enum_keys::<TailscaleLabel>(&mut keys);
    extend_enum_keys::<ProviderOrder>(&mut keys);
    extend_enum_keys::<ExecutionMode>(&mut keys);

    keys
}

fn extend_enum_keys<T: EnumVariants>(keys: &mut Vec<&'static str>) {
    keys.extend(T::variants().iter().map(|variant| variant.fluent_key));
}
