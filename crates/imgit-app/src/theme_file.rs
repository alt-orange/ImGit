//! Custom theme files: JSON, one theme per file, in the profile's "themes" folder.
//!
//! ```json
//! {
//!     "name": "Paper",
//!     "base": "ImGit Light",
//!     "background": "#FAF7F0",
//!     "accent": "#2F7D6D",
//!     "colors": { "text_normal": "#1F2328E0", "selected_row": "rgba(47, 125, 109, 0.25)" }
//! }
//! ```
//!
//! "base" is the built-in theme it starts from (ImGit Dark when left out). "background", "accent", "link",
//! "text_accent", "avatar" and "prompt" replace that theme's own: the surfaces and text are derived from the
//! background again (light ones get dark text). "colors" then sets palette colours by name (the theme palette's
//! fields, "graph_0" .. "graph_9" for the lanes). Colours are "#RGB", "#RGBA", "#RRGGBB", "#RRGGBBAA",
//! "rgb(r, g, b)" or "rgba(r, g, b, a)". Members a theme has no use for are ignored.

use std::fmt;
use std::str::FromStr;

use serde::de::{Deserialize, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

/// A colour, each channel in 0..=1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    #[must_use]
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
}

impl Default for Rgba {
    /// Opaque black.
    fn default() -> Self {
        Self::new(0.0, 0.0, 0.0, 1.0)
    }
}

/// The text is not one of the colour forms a theme file takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, thiserror::Error)]
#[error("not a colour: expected #RGB, #RRGGBB, #RRGGBBAA, rgb(r, g, b) or rgba(r, g, b, a)")]
pub struct ParseColorError;

impl FromStr for Rgba {
    type Err = ParseColorError;

    /// "#RGB", "#RGBA", "#RRGGBB", "#RRGGBBAA" (hex digits of either case), "rgb(r, g, b)" or
    /// "rgba(r, g, b, a)" with `r`, `g`, `b` in 0..=255 and `a` in 0..=1 (decimal numbers, clamped). Spaces
    /// around the colour and around the numbers are fine.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let text = trim(text);
        let color = match text.strip_prefix('#') {
            Some(hex) => parse_hex(hex),
            None => parse_functional(text),
        };
        color.ok_or(ParseColorError)
    }
}

/// [`Rgba::from_str`] without the reason: `None` when `text` is not a colour.
#[must_use]
pub fn parse_color(text: &str) -> Option<Rgba> {
    text.parse().ok()
}

/// The white space of the C locale, which is what a theme file's colours may be padded with.
fn trim(text: &str) -> &str {
    text.trim_matches(|c| matches!(c, ' ' | '\t'..='\r'))
}

fn parse_hex(hex: &str) -> Option<Rgba> {
    let digits: Vec<u8> = hex
        .chars()
        .map(|c| c.to_digit(16).and_then(|digit| u8::try_from(digit).ok()))
        .collect::<Option<_>>()?;
    let channels: Vec<f32> = match digits.len() {
        3 | 4 => digits
            .iter()
            .map(|&digit| f32::from(digit * 17) / 255.0)
            .collect(),
        6 | 8 => digits
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[high, low]| f32::from(high * 16 + low) / 255.0)
            .collect(),
        _ => return None,
    };
    Some(Rgba {
        r: channels[0],
        g: channels[1],
        b: channels[2],
        a: channels.get(3).copied().unwrap_or(1.0),
    })
}

fn parse_functional(text: &str) -> Option<Rgba> {
    let (arguments, has_alpha) = match text.strip_prefix("rgba(") {
        Some(arguments) => (arguments, true),
        None => (text.strip_prefix("rgb(")?, false),
    };
    let values: Vec<f32> = arguments
        .strip_suffix(')')?
        .split(',')
        .map(|part| parse_decimal(trim(part)))
        .collect::<Option<_>>()?;
    let channel = |value: f32| (value / 255.0).clamp(0.0, 1.0);
    match (values.as_slice(), has_alpha) {
        (&[r, g, b], false) => Some(Rgba::new(channel(r), channel(g), channel(b), 1.0)),
        (&[r, g, b, a], true) => Some(Rgba::new(
            channel(r),
            channel(g),
            channel(b),
            a.clamp(0.0, 1.0),
        )),
        _ => None,
    }
}

/// A decimal number that is all of `text`: an optional '-', digits with an optional fraction (".5" and "5."
/// included) and an optional exponent. Nothing else `f32::from_str` would take ("inf", "+1", "nan").
fn parse_decimal(text: &str) -> Option<f32> {
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(at) => (&unsigned[..at], Some(&unsigned[at + 1..])),
        None => (unsigned, None),
    };
    let all_digits = |digits: &str| digits.bytes().all(|b| b.is_ascii_digit());
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.len() + fraction.len() == 0 || !all_digits(whole) || !all_digits(fraction) {
        return None;
    }
    if let Some(exponent) = exponent {
        let digits = exponent.strip_prefix(['-', '+']).unwrap_or(exponent);
        if digits.is_empty() || !all_digits(digits) {
            return None;
        }
    }
    // Through f64, as the colours of the themes written so far were read.
    text.parse::<f64>().ok().map(|value| value as f32)
}

/// One theme file's contents.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ThemeFile {
    /// Never blank.
    pub name: String,
    /// The built-in theme it starts from; `None` for the default theme.
    pub base: Option<String>,
    pub background: Option<Rgba>,
    pub accent: Option<Rgba>,
    pub link: Option<Rgba>,
    pub text_accent: Option<Rgba>,
    pub avatar: Option<Rgba>,
    pub prompt: Option<Rgba>,
    /// Palette colours by name, in file order.
    pub colors: Vec<(String, Rgba)>,
}

/// Why a file is not a theme.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ThemeFileError {
    /// Not JSON; `line` is the 1-based line the parser stopped on.
    #[error("line {line}: {message}")]
    Syntax { line: usize, message: String },
    #[error("a theme is a JSON object")]
    NotAnObject,
    #[error("the theme has no \"name\"")]
    NoName,
    #[error("\"base\" is not a theme name")]
    BaseNotAName,
    /// One of the theme's own colours ("background", "accent"...).
    #[error("\"{0}\" is not a colour")]
    NotAColor(&'static str),
    #[error("\"colors\" is not an object")]
    ColorsNotAnObject,
    /// A member of "colors".
    #[error("colour \"{0}\" is not a colour")]
    PaletteNotAColor(String),
}

impl ThemeFile {
    /// The theme in `json`, or why it is not one.
    ///
    /// # Errors
    /// See [`ThemeFileError`].
    pub fn parse(json: &str) -> Result<Self, ThemeFileError> {
        let document: Json =
            serde_json::from_str(json).map_err(|error| ThemeFileError::Syntax {
                line: error.line(),
                message: syntax_message(&error),
            })?;
        let Json::Object(members) = &document else {
            return Err(ThemeFileError::NotAnObject);
        };
        // The first member of a name counts.
        let member = |key: &str| {
            members
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value)
        };

        let name = match member("name") {
            Some(Json::String(name)) if !trim(name).is_empty() => trim(name).to_owned(),
            _ => return Err(ThemeFileError::NoName),
        };
        let base = match member("base") {
            None => None,
            Some(Json::String(base)) => Some(base.clone()).filter(|base| !base.is_empty()),
            Some(_) => return Err(ThemeFileError::BaseNotAName),
        };
        let color = |key: &'static str| {
            member(key)
                .map(|value| value.as_color().ok_or(ThemeFileError::NotAColor(key)))
                .transpose()
        };
        let mut theme = Self {
            name,
            base,
            background: color("background")?,
            accent: color("accent")?,
            link: color("link")?,
            text_accent: color("text_accent")?,
            avatar: color("avatar")?,
            prompt: color("prompt")?,
            colors: Vec::new(),
        };
        match member("colors") {
            None => {}
            Some(Json::Object(colors)) => {
                theme.colors = colors
                    .iter()
                    .map(|(key, value)| match value.as_color() {
                        Some(color) => Ok((key.clone(), color)),
                        None => Err(ThemeFileError::PaletteNotAColor(key.clone())),
                    })
                    .collect::<Result<_, _>>()?;
            }
            Some(_) => return Err(ThemeFileError::ColorsNotAnObject),
        }
        Ok(theme)
    }
}

impl FromStr for ThemeFile {
    type Err = ThemeFileError;

    fn from_str(json: &str) -> Result<Self, Self::Err> {
        Self::parse(json)
    }
}

/// serde_json's message without its " at line L column C" (the line leads the error instead).
fn syntax_message(error: &serde_json::Error) -> String {
    let message = error.to_string();
    match message.rfind(" at line ") {
        Some(at) => message[..at].to_owned(),
        None => message,
    }
}

/// As much of a JSON document as a theme reads: strings, and objects with their members in file order (a
/// repeated name included). Everything else is checked for being JSON and dropped.
#[derive(Debug)]
enum Json {
    String(String),
    Object(Vec<(String, Json)>),
    Other,
}

impl Json {
    fn as_color(&self) -> Option<Rgba> {
        match self {
            Self::String(text) => parse_color(text),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct JsonVisitor;

        impl<'de> Visitor<'de> for JsonVisitor {
            type Value = Json;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON value")
            }

            fn visit_str<E>(self, text: &str) -> Result<Json, E> {
                Ok(Json::String(text.to_owned()))
            }

            fn visit_string<E>(self, text: String) -> Result<Json, E> {
                Ok(Json::String(text))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
                let mut members = Vec::with_capacity(map.size_hint().unwrap_or(0));
                while let Some(member) = map.next_entry()? {
                    members.push(member);
                }
                Ok(Json::Object(members))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Json::Other)
            }

            fn visit_bool<E>(self, _: bool) -> Result<Json, E> {
                Ok(Json::Other)
            }

            fn visit_i64<E>(self, _: i64) -> Result<Json, E> {
                Ok(Json::Other)
            }

            fn visit_u64<E>(self, _: u64) -> Result<Json, E> {
                Ok(Json::Other)
            }

            fn visit_f64<E>(self, _: f64) -> Result<Json, E> {
                Ok(Json::Other)
            }

            fn visit_unit<E>(self) -> Result<Json, E> {
                Ok(Json::Other)
            }
        }

        deserializer.deserialize_any(JsonVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 1e-6,
            "{actual} is not {expected}"
        );
    }

    #[test]
    fn color_hex_forms() {
        let six = parse_color("#FF8000").unwrap();
        assert_near(six.r, 1.0);
        assert_near(six.g, 128.0 / 255.0);
        assert_near(six.b, 0.0);
        assert_near(six.a, 1.0);

        let three = parse_color("#f80").unwrap();
        assert_near(three.g, 136.0 / 255.0);

        let eight = parse_color(" #00000080 ").unwrap();
        assert_near(eight.a, 128.0 / 255.0);

        let four: Rgba = "#f808".parse().unwrap();
        assert_eq!(four, Rgba::new(1.0, 136.0 / 255.0, 0.0, 136.0 / 255.0));
    }

    #[test]
    fn color_rgb_forms() {
        let rgb = parse_color("rgb(255, 0, 51)").unwrap();
        assert_near(rgb.b, 0.2);
        let rgba = parse_color("rgba(0,0,0,0.25)").unwrap();
        assert_near(rgba.a, 0.25);
        // The number forms the parser takes.
        let forms = parse_color("rgba(25.5e1, 1.275E2, 0, .5)").unwrap();
        assert_near(forms.r, 1.0);
        assert_near(forms.g, 0.5);
        assert_near(forms.a, 0.5);
        // Out of range: clamped.
        assert_eq!(
            parse_color("rgba(300, -5, 1e+1, 7.)"),
            Some(Rgba::new(1.0, 0.0, 10.0 / 255.0, 1.0))
        );
        assert_eq!(
            parse_color("rgb(5.e1, .5e1, 0.e0)"),
            Some(Rgba::new(50.0 / 255.0, 5.0 / 255.0, 0.0, 1.0))
        );
    }

    #[test]
    fn color_rejects_bad_numbers() {
        for text in [
            "rgb(1, 2, )",
            "rgb(1, 2, -)",
            "rgb(1, 2, 3x)",
            "rgb(1, 2, .)",
            "rgb(1, 2, 3e)",
            "rgb(1, 2, +3)",
            "rgb(1, 2, inf)",
            "rgb(1, 2, NaN)",
            "rgb(1, 2, 1e-)",
            "rgb(1, 2, 1 0)",
        ] {
            assert_eq!(parse_color(text), None, "{text}");
        }
    }

    #[test]
    fn color_rejects() {
        for text in [
            "FF8000",
            "#FF800",
            "#GG0000",
            "#+F8000",
            "rgb(1, 2)",
            "rgba(1, 2, 3)",
            "rgb(1, 2, 3, 4)",
            "rgb(1, 2, 3",
            "RGB(1, 2, 3)",
            "red",
            "",
        ] {
            assert_eq!(text.parse::<Rgba>(), Err(ParseColorError), "{text}");
        }
    }

    #[test]
    fn theme_full() {
        let theme = ThemeFile::parse(
            r##"{
            "name": "Paper é",
            "base": "ImGit Light",
            "background": "#FAF7F0",
            "accent": "#2F7D6D",
            "comment": ["ignored", 1, true, null, {"nested": false}],
            "colors": {"text_normal": "#1F2328E0", "graph_3": "rgb(10, 20, 30)"}
        }"##,
        )
        .unwrap();
        assert_eq!(theme.name, "Paper \u{e9}");
        assert_eq!(theme.base.as_deref(), Some("ImGit Light"));
        assert_near(theme.background.unwrap().r, 250.0 / 255.0);
        assert!(theme.accent.is_some());
        assert_eq!(theme.link, None);
        assert_eq!(theme.colors.len(), 2);
        assert_eq!(theme.colors[0].0, "text_normal");
        assert_eq!(theme.colors[1].0, "graph_3");
        assert_eq!(
            theme.colors[1].1,
            Rgba::new(10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 1.0)
        );
    }

    #[test]
    fn theme_errors() {
        use ThemeFileError as E;
        assert!(matches!(
            ThemeFile::parse(""),
            Err(E::Syntax { line: 1, .. })
        ));
        assert_eq!(ThemeFile::parse("[]"), Err(E::NotAnObject));
        assert_eq!(
            ThemeFile::parse(r#"{"base": "ImGit Dark"}"#),
            Err(E::NoName)
        ); // no name
        assert_eq!(ThemeFile::parse(r#"{"name": "  "}"#), Err(E::NoName));
        assert_eq!(ThemeFile::parse(r#"{"name": 3}"#), Err(E::NoName));
        assert_eq!(
            ThemeFile::parse(r#"{"name": "x", "accent": "blue"}"#),
            Err(E::NotAColor("accent"))
        );
        assert_eq!(
            ThemeFile::parse(r#"{"name": "x", "colors": {"accent": 3}}"#),
            Err(E::PaletteNotAColor("accent".to_owned()))
        );
        assert_eq!(
            ThemeFile::parse(r#"{"name": "x", "colors": []}"#),
            Err(E::ColorsNotAnObject)
        );
        assert_eq!(
            ThemeFile::parse(r#"{"name": "x", "base": null}"#),
            Err(E::BaseNotAName)
        );
        assert!(matches!(
            ThemeFile::parse(r#"{"name": "x",}"#),
            Err(E::Syntax { .. })
        ));
        assert!(matches!(
            ThemeFile::parse(r#"{"name": "x"} trailing"#),
            Err(E::Syntax { .. })
        ));
        let unterminated = ThemeFile::parse("{\n\"name\": \"x").unwrap_err();
        assert!(
            unterminated.to_string().contains("line 2"),
            "{unterminated}"
        );
        assert!(
            !unterminated.to_string().contains("column"),
            "{unterminated}"
        );
    }

    #[test]
    fn theme_error_messages() {
        use ThemeFileError as E;
        assert_eq!(E::NotAnObject.to_string(), "a theme is a JSON object");
        assert_eq!(E::NoName.to_string(), "the theme has no \"name\"");
        assert_eq!(E::BaseNotAName.to_string(), "\"base\" is not a theme name");
        assert_eq!(
            E::NotAColor("accent").to_string(),
            "\"accent\" is not a colour"
        );
        assert_eq!(
            E::ColorsNotAnObject.to_string(),
            "\"colors\" is not an object"
        );
        assert_eq!(
            E::PaletteNotAColor("graph_0".to_owned()).to_string(),
            "colour \"graph_0\" is not a colour"
        );
    }

    #[test]
    fn theme_minimal() {
        let theme: ThemeFile = r#"{"name": " Mine "}"#.parse().unwrap();
        assert_eq!(theme.name, "Mine");
        assert_eq!(theme.base, None);
        assert!(theme.colors.is_empty());
        assert_eq!(
            theme,
            ThemeFile {
                name: "Mine".to_owned(),
                ..ThemeFile::default()
            }
        );
    }

    #[test]
    fn theme_repeated_members() {
        // The first of the theme's own members counts; "colors" keeps every entry, in order.
        let theme = ThemeFile::parse(
            r##"{"name": "a", "name": "b", "base": "", "link": "#000", "link": 5,
                "colors": {"x": "#fff", "y": "#000", "x": "#111"}}"##,
        )
        .unwrap();
        assert_eq!(theme.name, "a");
        assert_eq!(theme.base, None);
        assert_eq!(theme.link, Some(Rgba::default()));
        let names: Vec<&str> = theme.colors.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["x", "y", "x"]);
    }
}
