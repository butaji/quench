use crate::components::Color;

pub fn color_to_ratatui(c: &Color) -> ratatui::style::Color {
    use ratatui::style::Color as R;
    match c {
        Color::Default => R::Reset,
        Color::Hex(s) => parse_hex_color(s).unwrap_or(R::Reset),
        // Defer the 9 ANSI colours to a helper so this
        // function stays under the linter's complexity
        // threshold.
        other => ansi_to_ratatui(other).unwrap_or(R::Reset),
    }
}

fn ansi_to_ratatui(c: &Color) -> Option<ratatui::style::Color> {
    use ratatui::style::Color as R;
    let rat_color = match c {
        Color::Black=>R::Black,
        Color::Red=>R::Red,
        Color::Green=>R::Green,
        Color::Yellow=>R::Yellow,
        Color::Blue=>R::Blue,
        Color::Magenta=>R::Magenta,
        Color::Cyan=>R::Cyan,
        Color::White=>R::White,
        Color::Gray=>R::DarkGray,
        _=>return None,
    };
    Some(rat_color)
}

pub fn parse_hex_color(s: &str) -> Option<ratatui::style::Color> {
    use ratatui::style::Color as R;
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(R::Rgb(r, g, b))
}


