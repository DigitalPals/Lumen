use gtk4::glib::DateTime;
use lumen_config::schemas::modules::TimeFormat;
use tracing::error;

pub(super) fn format_time(format: &str, time_format: TimeFormat) -> String {
    let format = hour_format(format, time_format);

    DateTime::now_local()
        .and_then(|dt| dt.format(&format))
        .map(|gstring| gstring.to_string())
        .inspect_err(|e| error!(error = %e, "cannot format time"))
        .unwrap_or_else(|_| String::from("--"))
}

pub(super) fn hour_format(format: &str, time_format: TimeFormat) -> String {
    match time_format {
        TimeFormat::TwelveHour => twelve_hour_format(format),
        TimeFormat::TwentyFourHour => twenty_four_hour_format(format),
    }
}

fn twelve_hour_format(format: &str) -> String {
    let mut formatted = format.replace("%H", "%I");

    if !formatted.contains("%p") {
        formatted.push_str(" %p");
    }

    formatted
}

fn twenty_four_hour_format(format: &str) -> String {
    format
        .replace("%I", "%H")
        .replace(" %p", "")
        .replace("%p", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hour_format_converts_default_to_24_hour() {
        assert_eq!(
            hour_format("%a %b %d %I:%M %p", TimeFormat::TwentyFourHour),
            "%a %b %d %H:%M"
        );
    }

    #[test]
    fn hour_format_converts_24_hour_to_12_hour() {
        assert_eq!(
            hour_format("%a %b %d %H:%M", TimeFormat::TwelveHour),
            "%a %b %d %I:%M %p"
        );
    }
}
