use std::str::FromStr;

use serde::Deserialize;
use termcolor::ColorChoice;

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ColorMode {
    Auto,
    Always,
    Ansi,
    Never,
}

impl Into<ColorChoice> for ColorMode {
    fn into(self) -> ColorChoice {
        match self {
            ColorMode::Auto => ColorChoice::Auto,
            ColorMode::Always => ColorChoice::Always,
            ColorMode::Ansi => ColorChoice::AlwaysAnsi,
            ColorMode::Never => ColorChoice::Never,
        }
    }
}

impl FromStr for ColorMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "always" => Ok(ColorMode::Always),
            "ansi" => Ok(ColorMode::Ansi),
            "auto" => {
                if atty::is(atty::Stream::Stdout) {
                    Ok(ColorMode::Auto)
                } else {
                    Ok(ColorMode::Never)
                }
            }
            "never" => Ok(ColorMode::Never),
            _ => Err("Invalid color value".to_string()),
        }
    }
}
