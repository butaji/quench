use crate::{
    components::{AlignItems, JustifyContent},
    BorderStyle, Color, FlexDirection,
};
use rquickjs::{Result as JsResult, Value};

/// Parse a flex-direction string. The bridge accepts
/// the Ink JS API names: "row", "column", "row-reverse",
/// "column-reverse" (and the camelCase variants
/// "rowReverse", "columnReverse").
pub fn parse_flex_dir(s: &str) -> FlexDirection {
    match s {
        "row" | "Row" => FlexDirection::Row,
        "column" | "Column" => FlexDirection::Column,
        "row-reverse" | "rowReverse" | "RowReverse" => FlexDirection::RowReverse,
        "column-reverse" | "columnReverse" | "ColumnReverse" => FlexDirection::ColumnReverse,
        _ => FlexDirection::Row,
    }
}

/// Parse a border-style string. Ink supports
/// "single", "double", "round", "bold", "classic", and
/// the special "none".
pub fn parse_border_style(s: &str) -> BorderStyle {
    match s {
        "single" | "Single" => BorderStyle::Single,
        "double" | "Double" => BorderStyle::Double,
        "round" | "Round" => BorderStyle::Round,
        "bold" | "Bold" => BorderStyle::Bold,
        "classic" | "Classic" => BorderStyle::Classic,
        _ => BorderStyle::Single,
    }
}

/// Parse a `justifyContent` string into a
/// `JustifyContent`.
pub fn parse_justify(s: &str) -> JustifyContent {
    match s {
        "flex-start" | "FlexStart" => JustifyContent::FlexStart,
        "flex-end" | "FlexEnd" => JustifyContent::FlexEnd,
        "center" | "Center" => JustifyContent::Center,
        "space-between" | "SpaceBetween" => JustifyContent::SpaceBetween,
        "space-around" | "SpaceAround" => JustifyContent::SpaceAround,
        _ => JustifyContent::FlexStart,
    }
}

/// Parse an `alignItems` string into an
/// `AlignItems`.
pub fn parse_align_items(s: &str) -> AlignItems {
    match s {
        "flex-start" | "FlexStart" => AlignItems::FlexStart,
        "flex-end" | "FlexEnd" => AlignItems::FlexEnd,
        "center" | "Center" => AlignItems::Center,
        "stretch" | "Stretch" => AlignItems::Stretch,
        "baseline" | "Baseline" => AlignItems::Baseline,
        _ => AlignItems::FlexStart,
    }
}

/// Parse a color. Ink's `color` prop accepts a color
/// name string ("red", "blue", etc.) or a hex string
/// ("#rrggbb"). Mirrors Ink 5's supported color set.
pub fn parse_color(s: &str) -> Color {
    if s.starts_with('#') && s.len() == 7 {
        return Color::Hex(s.to_string());
    }
    color_by_name(s)
}

fn color_by_name(s: &str) -> Color {
    const PAIRS: &[(&str, Color)] = &[
        ("black", Color::Black),
        ("red", Color::Red),
        ("green", Color::Green),
        ("yellow", Color::Yellow),
        ("blue", Color::Blue),
        ("magenta", Color::Magenta),
        ("cyan", Color::Cyan),
        ("white", Color::White),
        ("gray", Color::Gray),
        ("grey", Color::Gray),
    ];
    for (name, color) in PAIRS {
        if *name == s {
            return (*color).clone();
        }
    }
    Color::Default
}

/// Get a u16 from a JS number. Returns 0 if not a
/// number.
pub fn to_u16(v: &Value<'_>) -> u16 {
    if let Some(n) = v.as_int() {
        n.max(0).min(u16::MAX as i32) as u16
    } else if let Some(n) = v.as_float() {
        n.max(0.0).min(u16::MAX as f64) as u16
    } else {
        0
    }
}
