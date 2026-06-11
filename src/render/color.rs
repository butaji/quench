//! Color parsing utilities

use ratatui::style::Color;

/// Parse color string to ratatui Color
pub fn parse_color(s: &str) -> Option<Color> {
    match s.to_lowercase().as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "gray" | "grey" => Some(Color::Gray),
        "brightblack" | "brightBlack" => Some(Color::Indexed(8)),
        "brightred" | "brightRed" => Some(Color::Indexed(9)),
        "brightgreen" | "brightGreen" => Some(Color::Indexed(10)),
        "brightyellow" | "brightYellow" => Some(Color::Indexed(11)),
        "brightblue" | "brightBlue" => Some(Color::Indexed(12)),
        "brightmagenta" | "brightMagenta" => Some(Color::Indexed(13)),
        "brightcyan" | "brightCyan" => Some(Color::Indexed(14)),
        "brightwhite" | "brightWhite" => Some(Color::Indexed(15)),
        _ => parse_hex_color(s),
    }
}

/// Parse hex color (#rgb or #rrggbb) to ratatui Color
fn parse_hex_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if !s.starts_with('#') {
        return None;
    }
    let hex = &s[1..];
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Color::Rgb(r, g, b))
        }
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some(Color::Rgb(r, g, b))
        }
        _ => None,
    }
}
