#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Box as InkBox;
    use crate::render::{AvailableSpace, Layout, RenderOptions, Size, YogaTree, parse_hex_color, render_tree, render_to_string};
    use crate::{BorderStyle, Color, Text, VNode};
    use ratatui::Terminal;
    use ratatui::layout::Rect;

    fn small_area() -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 4,
        }
    }

    #[test]
    fn render_text_into_frame() {
        let backend = ratatui::backend::TestBackend::new(20, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut layout = Layout::new();
        let tree = YogaTree::from_vnode(
            &VNode::from(Text::new("hi")),
            &mut layout,
        );
        tree.compute(
            &mut layout,
            Size {
                width: AvailableSpace::Definite(20.0),
                height: AvailableSpace::Definite(4.0),
            },
        );
        terminal
            .draw(|frame| render_tree(&VNode::from(Text::new("hi")), &layout, frame, small_area()))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let row0: String = (0..2)
            .map(|x| buffer.get(x, 0).symbol().to_string())
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(row0, "hi");
    }

    #[test]
    fn render_text_with_colour() {
        let backend = ratatui::backend::TestBackend::new(20, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let tree = VNode::from(Text::new("red").color(Color::Red));
        let mut layout = Layout::new();
        let yoga_tree = YogaTree::from_vnode(&tree, &mut layout);
        yoga_tree.compute(
            &mut layout,
            Size {
                width: AvailableSpace::Definite(20.0),
                height: AvailableSpace::Definite(1.0),
            },
        );
        terminal
            .draw(|frame| {
                render_tree(
                    &tree,
                    &layout,
                    frame,
                    Rect { x: 0, y: 0, width: 20, height: 1 },
                );
            })
            .unwrap();
        let cell = terminal.backend().buffer().get(0, 0);
        assert_eq!(cell.symbol(), "r");
        assert_eq!(cell.fg, ratatui::style::Color::Red);
    }

    #[test]
    fn render_box_with_padding() {
        let backend = ratatui::backend::TestBackend::new(20, 4);
        let mut terminal = Terminal::new(backend).unwrap();
        let tree = VNode::from(InkBox::column().padding(1).child(Text::new("x")).child(Text::new("y")));
        let mut layout = Layout::new();
        let yoga_tree = YogaTree::from_vnode(&tree, &mut layout);
        yoga_tree.compute(&mut layout, Size { width: AvailableSpace::Definite(20.0), height: AvailableSpace::Definite(4.0) });
        terminal.draw(|frame| render_tree(&tree, &layout, frame, small_area())).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut has_x = false; let mut has_y = false;
        for y in 0..4 { for x in 0..20 { let s = buffer.get(x, y).symbol(); if s == "x" { has_x = true; } if s == "y" { has_y = true; } } }
        assert!(has_x, "expected 'x' somewhere in the rendered buffer");
        assert!(has_y, "expected 'y' somewhere in the rendered buffer");
    }

    #[test]
    fn render_to_string_produces_text() {
        let root = InkBox::column()
            .child(Text::new("hello"))
            .child(Text::new("world"))
            .into();
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        // The output should contain "hello" and "world"
        // on separate lines.
        assert!(s.contains("hello"), "missing 'hello' in: {s:?}");
        assert!(s.contains("world"), "missing 'world' in: {s:?}");
    }

    #[test]
    fn parse_hex_color_works() {
        let c = parse_hex_color("#ff00aa").unwrap();
        if let ratatui::style::Color::Rgb(r, g, b) = c {
            assert_eq!(r, 255);
            assert_eq!(g, 0);
            assert_eq!(b, 170);
        } else {
            panic!("expected Rgb, got {c:?}");
        }
    }

    #[test]
    fn parse_hex_color_rejects_bad_input() {
        assert!(parse_hex_color("ff00aa").is_none()); // no #
        assert!(parse_hex_color("#ff00a").is_none()); // too short
        assert!(parse_hex_color("#zzzzzz").is_none()); // not hex
    }

    /// Nested boxes: a column Box with one child Box
    /// that itself contains two Text children. With
    /// the Yoga-rect-aware walker, the inner Box gets
    /// a non-zero rect and the Texts land inside it.
    #[test]
    fn render_nested_boxes_via_yoga_layout() {
        let inner = InkBox::column()
            .child(Text::new("inner-a"))
            .child(Text::new("inner-b"));
        let root: VNode = InkBox::column().child(inner).into();
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        assert!(s.contains("inner-a"), "missing 'inner-a' in: {s:?}");
        assert!(s.contains("inner-b"), "missing 'inner-b' in: {s:?}");
    }

    /// A `<Box flexDirection="row">` should lay its
    /// two text children side-by-side on a single
    /// line. With the Yoga-rect-aware walker the
    /// second child's `x` is non-zero.
    #[test]
    fn render_row_uses_horizontal_yoga_layout() {
        let root: VNode = InkBox::row()
            .child(Text::new("L"))
            .child(Text::new("R"))
            .into();
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        // The output should contain "L" and "R" on
        // the same line (no newline between them).
        // The joiner trims trailing whitespace so the
        // line looks like "L  R" with whatever gap
        // taffy left between flex items.
        assert!(s.contains('L'), "missing 'L' in: {s:?}");
        assert!(s.contains('R'), "missing 'R' in: {s:?}");
        // The single line containing both characters
        // is the first non-empty line.
        let first_line = s.lines().find(|l| !l.is_empty()).unwrap();
        assert!(
            first_line.contains('L') && first_line.contains('R'),
            "L and R should be on the same line, got: {first_line:?}"
        );
    }

    /// A bordered Box with a Text child should render the text.
    #[test]
    fn render_bordered_box_with_text() {
        let root: VNode = InkBox::column()
            .child(
                InkBox::new()
                    .border_style(BorderStyle::Round)
                    .child(Text::new("green border")),
            )
            .child(
                InkBox::new()
                    .border_style(BorderStyle::Round)
                    .child(Text::new("dim border")),
            )
            .into();
        let s = render_to_string(root, RenderOptions::new()).unwrap();
        eprintln!("Bordered box result:\n{}", s);
        assert!(s.contains("green"), "missing 'green' in: {s:?}");
        assert!(s.contains("dim"), "missing 'dim' in: {s:?}");
    }
}
