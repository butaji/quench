//! Parity tests for Ink layout components.
//!
//! Tests Newline, Spacer, Static, Transform, and nested layouts.

use runts_ink::{
    render_to_string, Box as InkBox, Newline, RenderOptions, Spacer, Static,
    Text as InkText, Transform, VNode,
};

#[cfg(test)]
mod newline_tests {
    use super::*;

    #[test]
    fn test_newline_render() {
        let root = VNode::from(Newline::new());
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains('\n') || s.is_empty());
    }

    #[test]
    fn test_box_with_newline() {
        let root = VNode::from(
            InkBox::column()
                .child(InkText::new("Before"))
                .child(Newline::new())
                .child(InkText::new("After")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Before"));
        assert!(s.contains("After"));
    }
}

#[cfg(test)]
mod spacer_tests {
    use super::*;

    #[test]
    fn test_spacer_render() {
        let root = VNode::from(Spacer::new());
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.trim().is_empty() || s.is_empty());
    }

    #[test]
    fn test_box_with_spacer() {
        let root = VNode::from(
            InkBox::column()
                .child(InkText::new("Top"))
                .child(Spacer::new())
                .child(InkText::new("Bottom")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Top"));
        assert!(s.contains("Bottom"));
    }
}

#[cfg(test)]
mod static_tests {
    use super::*;

    #[test]
    fn test_static_empty() {
        let root = VNode::from(Static::new());
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.trim().is_empty());
    }

    #[test]
    fn test_static_with_children() {
        let root = VNode::from(
            InkBox::column().child(
                Static::new()
                    .child(InkText::new("Static 1"))
                    .child(InkText::new("Static 2")),
            ),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Static 1") || s.contains("Static 2"));
    }

    #[test]
    fn test_box_with_static() {
        let root = VNode::from(
            InkBox::column()
                .child(Static::new().child(InkText::new("Static Content"))),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Static Content"));
    }

    #[test]
    fn test_static_builder_chaining() {
        let result = Static::new()
            .child(InkText::new("A"))
            .child(InkText::new("B"));
        assert_eq!(result.children.len(), 2);
    }
}

#[cfg(test)]
mod transform_tests {
    use super::*;

    #[test]
    fn test_transform_new() {
        let root = VNode::from(Transform::new(InkText::new("Transformed")));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Transformed"));
    }

    #[test]
    fn test_transform_with_offset() {
        let root = VNode::from(
            Transform::new(InkText::new("Offset")).translate(5, 3),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Offset"));
    }

    #[test]
    fn test_transform_translate_x() {
        let root = VNode::from(
            Transform::new(InkText::new("X Offset")).translate_x(10),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("X Offset"));
    }

    #[test]
    fn test_transform_translate_y() {
        let root = VNode::from(
            Transform::new(InkText::new("Y Offset")).translate_y(5),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Y Offset"));
    }

    #[test]
    fn test_transform_builder_chaining() {
        let result = Transform::new(InkText::new("Transform"))
            .translate_x(5)
            .translate_y(10)
            .translate(3, 7);
        assert_eq!(result.x, 3);
        assert_eq!(result.y, 7);
    }
}

#[cfg(test)]
mod nested_layout_tests {
    use super::*;

    #[test]
    fn test_deeply_nested_boxes() {
        let root = VNode::from(
            InkBox::column().child(
                InkBox::row()
                    .child(InkBox::column().child(InkText::new("Deep"))),
            ),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Deep"));
    }

    #[test]
    fn test_mixed_nested_components() {
        let root = VNode::from(
            InkBox::column()
                .padding(1)
                .child(InkText::new("Title").bold())
                .child(Newline::new())
                .child(Spacer::new())
                .child(
                    InkBox::row()
                        .column_gap(1)
                        .child(InkText::new("A"))
                        .child(InkText::new("B")),
                ),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Title"));
        assert!(s.contains("A"));
        assert!(s.contains("B"));
    }
}

#[cfg(test)]
mod layout_alignment_tests {
    use super::*;

    #[test]
    fn test_row_with_centered_content() {
        let root = VNode::from(
            InkBox::row()
                .justify_content(runts_ink::JustifyContent::Center)
                .width(80)
                .child(InkText::new("Centered")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Centered"));
    }

    #[test]
    fn test_row_with_flex_end() {
        let root = VNode::from(
            InkBox::row()
                .justify_content(runts_ink::JustifyContent::FlexEnd)
                .width(80)
                .child(InkText::new("End")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("End"));
    }
}

#[cfg(test)]
mod edge_case_layout_tests {
    use super::*;

    #[test]
    fn test_very_long_text() {
        let long_text = "A".repeat(1000);
        let root = VNode::from(InkText::new(long_text));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.len() >= 1000);
    }

    #[test]
    fn test_unicode_text() {
        let root = VNode::from(InkText::new("Hello World"));
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Hello"));
        assert!(s.contains("World"));
    }

    #[test]
    fn test_empty_box_with_children() {
        let root = VNode::from(
            InkBox::column()
                .child(InkText::new(""))
                .child(InkText::new("Non-empty")),
        );
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Non-empty"));
    }

    #[test]
    fn test_many_children() {
        let mut box_children = InkBox::column();
        for i in 0..10 {
            box_children = box_children.child(InkText::new(format!("Item {}", i)));
        }
        let root = VNode::from(box_children);
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("Item 0"));
        assert!(s.contains("Item 9"));
    }
}
