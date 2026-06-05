//! Unit tests for runts-ink style types.
//!
//! These tests verify that the Ink-compatible style enums
//! map correctly to their Ratatui equivalents.

use runts_ink::{
    BorderStyle, Borders, Display, Overflow, Position, Wrap,
};

#[test]
fn test_border_style_default() {
    assert_eq!(BorderStyle::default(), BorderStyle::Single);
}

#[test]
fn test_border_style_to_ratatui() {
    use ratatui::widgets::BorderType;
    
    assert_eq!(BorderStyle::Single.to_ratatui(), BorderType::Plain);
    assert_eq!(BorderStyle::Double.to_ratatui(), BorderType::Double);
    assert_eq!(BorderStyle::Round.to_ratatui(), BorderType::Rounded);
    assert_eq!(BorderStyle::Bold.to_ratatui(), BorderType::Thick);
    assert_eq!(BorderStyle::Classic.to_ratatui(), BorderType::Plain); // Manual corners
}

#[test]
fn test_border_style_serde_roundtrip() {
    let styles = [
        BorderStyle::Single,
        BorderStyle::Double,
        BorderStyle::Round,
        BorderStyle::Bold,
        BorderStyle::Classic,
    ];
    
    for style in styles {
        let json = serde_json::to_string(&style).unwrap();
        let parsed: BorderStyle = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, style);
    }
}

#[test]
fn test_borders_default() {
    let b = Borders::default();
    assert!(!b.top);
    assert!(!b.right);
    assert!(!b.bottom);
    assert!(!b.left);
}

#[test]
fn test_borders_all() {
    let b = Borders::ALL;
    assert!(b.top);
    assert!(b.right);
    assert!(b.bottom);
    assert!(b.left);
}

#[test]
fn test_borders_horizontal() {
    let b = Borders::HORIZONTAL;
    assert!(b.top);
    assert!(!b.right);
    assert!(b.bottom);
    assert!(!b.left);
}

#[test]
fn test_borders_vertical() {
    let b = Borders::VERTICAL;
    assert!(!b.top);
    assert!(b.right);
    assert!(!b.bottom);
    assert!(b.left);
}

#[test]
fn test_borders_to_ratatui() {
    use ratatui::widgets::Borders as RBorders;
    
    let all = Borders::ALL.to_ratatui();
    assert!(all.contains(RBorders::TOP));
    assert!(all.contains(RBorders::RIGHT));
    assert!(all.contains(RBorders::BOTTOM));
    assert!(all.contains(RBorders::LEFT));
    
    let none = Borders::default().to_ratatui();
    assert!(!none.contains(RBorders::TOP));
    
    let h = Borders::HORIZONTAL.to_ratatui();
    assert!(h.contains(RBorders::TOP));
    assert!(h.contains(RBorders::BOTTOM));
    assert!(!h.contains(RBorders::LEFT));
    assert!(!h.contains(RBorders::RIGHT));
}

#[test]
fn test_borders_serde() {
    for borders in [Borders::default(), Borders::ALL, Borders::HORIZONTAL, Borders::VERTICAL] {
        let json = serde_json::to_string(&borders).unwrap();
        let parsed: Borders = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.top, borders.top);
        assert_eq!(parsed.right, borders.right);
        assert_eq!(parsed.bottom, borders.bottom);
        assert_eq!(parsed.left, borders.left);
    }
}

#[test]
fn test_display_default() {
    assert_eq!(Display::default(), Display::Flex);
}

#[test]
fn test_display_serde() {
    for display in [Display::Flex, Display::None] {
        let json = serde_json::to_string(&display).unwrap();
        let parsed: Display = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, display);
    }
}

#[test]
fn test_display_equality() {
    assert_eq!(Display::Flex, Display::Flex);
    assert_ne!(Display::Flex, Display::None);
    assert_ne!(Display::None, Display::Flex);
}

#[test]
fn test_overflow_default() {
    assert_eq!(Overflow::default(), Overflow::Visible);
}

#[test]
fn test_overflow_serde() {
    for overflow in [Overflow::Visible, Overflow::Hidden] {
        let json = serde_json::to_string(&overflow).unwrap();
        let parsed: Overflow = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, overflow);
    }
}

#[test]
fn test_position_default() {
    assert_eq!(Position::default(), Position::Relative);
}

#[test]
fn test_position_serde() {
    for position in [Position::Relative, Position::Absolute] {
        let json = serde_json::to_string(&position).unwrap();
        let parsed: Position = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, position);
    }
}

#[test]
fn test_wrap_default() {
    assert_eq!(Wrap::default(), Wrap::Wrap);
}

#[test]
fn test_wrap_to_ratatui() {
    use ratatui::widgets::Wrap as RWrap;
    
    // Wrap maps to trim: false
    assert_eq!(Wrap::Wrap.to_ratatui(), RWrap { trim: false });
    
    // Hard maps to trim: true
    assert_eq!(Wrap::Hard.to_ratatui(), RWrap { trim: true });
    
    // Truncate also maps to trim: true (simplified)
    assert_eq!(Wrap::Truncate.to_ratatui(), RWrap { trim: true });
    
    // TruncateMiddle also maps to trim: true
    assert_eq!(Wrap::TruncateMiddle.to_ratatui(), RWrap { trim: true });
}

#[test]
fn test_wrap_serde_roundtrip() {
    let wraps = [Wrap::Wrap, Wrap::Hard, Wrap::Truncate, Wrap::TruncateMiddle];
    
    for wrap in wraps {
        let json = serde_json::to_string(&wrap).unwrap();
        let parsed: Wrap = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, wrap);
    }
}

#[test]
fn test_wrap_equality() {
    assert_eq!(Wrap::Wrap, Wrap::Wrap);
    assert_ne!(Wrap::Wrap, Wrap::Hard);
    assert_ne!(Wrap::Truncate, Wrap::TruncateMiddle);
}
