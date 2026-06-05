//! Comprehensive parity tests for runts-ink.
//!
//! These tests verify that the runts-ink rendering produces
//! consistent output that matches Ink's expected behavior.
//!
//! This is the main test file that includes all parity test modules.
//! Each module is in its own file to comply with lint rules.

mod ink_components_tests;
mod ink_events_tests;
mod ink_flex_layout_tests;
mod ink_parity_box_tests;
mod ink_parity_layout_tests;
mod ink_parity_serde_tests;
mod ink_parity_text_tests;

use runts_ink::{render_to_string, Newline, RenderOptions, Spacer, VNode, VNodeContent};

#[cfg(test)]
mod options_tests {
    use super::*;

    #[test]
    fn test_render_options_default() {
        let options = RenderOptions::new();
        assert!(!options.patch_console);
        assert!(!options.exit_on_q);
        assert!(options.exit_on_ctrl_c);
        assert_eq!(options.tick_ms, 100);
        assert!(options.alternate_screen);
        assert_eq!(options.max_fps, 60);
    }

    #[test]
    fn test_render_options_custom() {
        let mut options = RenderOptions::new();
        options.patch_console = true;
        options.exit_on_q = true;
        options.tick_ms = 50;

        assert!(options.patch_console);
        assert!(options.exit_on_q);
        assert_eq!(options.tick_ms, 50);
    }
}

#[cfg(test)]
mod vnode_tests {
    use super::*;
    use runts_ink::{Box as InkBox, Text as InkText};

    #[test]
    fn test_vnode_from_box() {
        let box_node = InkBox::column().child(InkText::new("From Box"));
        let vnode = VNode::from(box_node);
        match &vnode.0 {
            VNodeContent::Box(_) => {}
            _ => panic!("Expected Box VNode"),
        }
    }

    #[test]
    fn test_vnode_from_text() {
        let text = InkText::new("From Text");
        let vnode = VNode::from(text);
        match &vnode.0 {
            VNodeContent::Text(_) => {}
            _ => panic!("Expected Text VNode"),
        }
    }

    #[test]
    fn test_vnode_from_newline() {
        let vnode = VNode::from(Newline::new());
        match &vnode.0 {
            VNodeContent::Newline(_) => {}
            _ => panic!("Expected Newline VNode"),
        }
    }

    #[test]
    fn test_vnode_from_spacer() {
        let vnode = VNode::from(Spacer::new());
        match &vnode.0 {
            VNodeContent::Spacer(_) => {}
            _ => panic!("Expected Spacer VNode"),
        }
    }
}
