    #[test]
    fn yoga_layout_text_in_box() {
        let root = InkBox::column()
            .padding(1)
            .child(VNode::from(Text::new("Hello")))
            .into();
        let layout = compute(&root, 40, 10);
        assert!(!layout.rects.is_empty());
        let (_x, _y, w, h) = layout.rects[0];
        assert!(w > 0 && h > 0);
    }

    #[test]
    fn yoga_row_layout_positions_children_side_by_side() {
        let root = InkBox::row()
            .child(VNode::from(Text::new("A")))
            .child(VNode::from(Text::new("B")))
            .into();
        let layout = compute(&root, 40, 4);
        assert!(layout.rects.len() >= 3);
        let (_, _, aw, _) = layout.rects[1];
        let (bx, _, _, _) = layout.rects[2];
        assert_eq!(aw, 1);
        assert_eq!(bx, aw);
    }

    #[test]
    fn debug_all_border_styles() {
        use crate::components::Box as InkBox;
        use crate::components::Text;
        let root = InkBox::column()
            .padding(1)
            .child(VNode::from(Text::new("Border Styles Demo")))
            .child({
                let mut b = InkBox::new(); b.margin_top = Some(1);
                b.child(VNode::from(
                    InkBox::new().border_style(crate::style::BorderStyle::Single).padding(1)
                        .child(VNode::from(Text::new("Single border")))
                ))
            })
            .child({
                let mut b = InkBox::new(); b.margin_top = Some(1);
                b.child(VNode::from(
                    InkBox::new().border_style(crate::style::BorderStyle::Double).padding(1)
                        .child(VNode::from(Text::new("Double border")))
                ))
            })
            .into();
        let layout = compute(&root, 80, 24);
        for (i, r) in layout.rects.iter().enumerate() {
            eprintln!("rect[{}] = {:?}", i, r);
        }
    }

    #[test]
    fn yoga_margin_box_size() {
        use crate::components::Box as InkBox;
        let mut left_box = InkBox::new();
        left_box.margin_right = Some(2);
        left_box.border_style = crate::style::BorderStyle::Single;
        left_box.borders = crate::style::Borders::ALL;
        left_box.padding_top = Some(1);
        left_box.padding_bottom = Some(1);
        left_box.padding_left = Some(1);
        left_box.padding_right = Some(1);

        left_box = left_box.child(VNode::from(Text::new("Left")));
        let row = InkBox::row()
            .width(80)
            .child(VNode::from(left_box));
        let root: VNode = row.into();
        let layout = compute(&root, 80, 24);
        eprintln!("margin box rects (explicit row width): {:?}", layout.rects);
        assert!(layout.rects.len() >= 3);
        let (_, _, bw, bh) = layout.rects[1];
        assert_eq!((bw, bh), (8, 5), "margin box should be 8x5, got {}x{}", bw, bh);
        let (_, _, tw, th) = layout.rects[2];
        assert_eq!((tw, th), (4, 1), "text should be 4x1, got {}x{}", tw, th);
    }

    #[test]
    fn yoga_margin_box_in_auto_row() {
        use crate::components::Box as InkBox;
        let mut left_box = InkBox::new();
        left_box.margin_right = Some(2);
        left_box.border_style = crate::style::BorderStyle::Single;
        left_box.borders = crate::style::Borders::ALL;
        left_box.padding_top = Some(1);
        left_box.padding_bottom = Some(1);
        left_box.padding_left = Some(1);
        left_box.padding_right = Some(1);
        left_box = left_box.child(VNode::from(Text::new("Left")));

        let mut right_box = InkBox::new();
        right_box.border_style = crate::style::BorderStyle::Single;
        right_box.borders = crate::style::Borders::ALL;
        right_box.padding_top = Some(1);
        right_box.padding_bottom = Some(1);
        right_box.padding_left = Some(1);
        right_box.padding_right = Some(1);
        right_box = right_box.child(VNode::from(Text::new("Right")));

        let row = InkBox::row()
            .child(VNode::from(left_box))
            .child(VNode::from(right_box));

        let root = InkBox::column()
            .padding(1)
            .child(row);
        let layout = compute(&root.into(), 80, 24);
        eprintln!("margin box rects (auto row width): {:?}", layout.rects);
        assert!(layout.rects.len() >= 4);
        let (_, _, bw, bh) = layout.rects[2];
        assert_eq!((bw, bh), (8, 5), "margin box should be 8x5, got {}x{}", bw, bh);
    }

    #[test]
    fn yoga_layout_honours_display_none() {
        let mut hidden = InkBox::column();
        hidden.display = InkDisplay::None;
        hidden = hidden.child(VNode::from(Text::new("secret")));
        let root = InkBox::column().child(VNode::from(hidden)).into();
        let layout = compute(&root, 40, 4);
        assert_eq!(layout.rects.len(), 2);
        let (_, _, w, h) = layout.rects[1];
        assert_eq!((w, h), (0, 0));
    }
