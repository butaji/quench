//! Integration tests for js_bridge prop setters.
//!
//! Each test creates an rquickjs context, installs the
//! bridge, builds a VNode with specific props, renders
//! it, and checks the output.

use rquickjs::{Context, Runtime};

fn fresh_ctx() -> Context {
    let runtime = Runtime::new().unwrap();
    rquickjs::Context::full(&runtime).unwrap()
}

fn render_js(code: &str) -> String {
    let ctx = fresh_ctx();
    ctx.with(|ctx| {
        runts_ink::js_bridge::install(&ctx).unwrap();
        let result: String = ctx.eval(code).unwrap();
        result
    })
}

#[test]
fn box_width_height_roundtrip() {
    let ctx = fresh_ctx();
    let (w, h) = ctx.with(|ctx| {
        runts_ink::js_bridge::install(&ctx).unwrap();
        let b: rquickjs::Object = ctx
            .eval(r#"runts_ink.box({ width: 10, height: 3, children: [] })"#)
            .unwrap();
        let inner: rquickjs::Object = b.get("Box").unwrap();
        let props: rquickjs::Object = inner.get("__props").unwrap();
        let w: i32 = props.get("width").unwrap();
        let h: i32 = props.get("height").unwrap();
        (w, h)
    });
    assert_eq!(w, 10);
    assert_eq!(h, 3);
}

#[test]
fn box_min_max_size_renders() {
    let out = render_js(
        r#"
        let b = runts_ink.box({
            minWidth: 5,
            maxWidth: 20,
            minHeight: 2,
            maxHeight: 5,
            children: [runts_ink.text("x", {})]
        });
        runts_ink.render_to_string(b)
    "#,
    );
    assert!(out.contains("x"));
}

#[test]
fn box_flex_wrap_renders() {
    let out = render_js(
        r#"
        let b = runts_ink.box({
            flexWrap: "wrap",
            children: [runts_ink.text("a", {}), runts_ink.text("b", {})]
        });
        runts_ink.render_to_string(b)
    "#,
    );
    assert!(out.contains("a"));
    assert!(out.contains("b"));
}

#[test]
fn box_flex_basis_renders() {
    let out = render_js(
        r#"
        let b = runts_ink.box({
            flexBasis: 50,
            children: [runts_ink.text("fb", {})]
        });
        runts_ink.render_to_string(b)
    "#,
    );
    assert!(out.contains("fb"));
}

#[test]
fn box_position_absolute_renders() {
    let out = render_js(
        r#"
        let b = runts_ink.box({
            position: "absolute",
            top: 1,
            left: 2,
            children: [runts_ink.text("pos", {})]
        });
        runts_ink.render_to_string(b)
    "#,
    );
    assert!(out.contains("pos"));
}

#[test]
fn box_display_none_hides() {
    let out = render_js(
        r#"
        let b = runts_ink.box({
            display: "none",
            children: [runts_ink.text("hidden", {})]
        });
        runts_ink.render_to_string(b)
    "#,
    );
    assert!(!out.contains("hidden"), "display:none should hide content");
}

#[test]
fn box_overflow_hidden_renders() {
    let out = render_js(
        r#"
        let b = runts_ink.box({
            overflowX: "hidden",
            overflowY: "hidden",
            children: [runts_ink.text("ov", {})]
        });
        runts_ink.render_to_string(b)
    "#,
    );
    assert!(out.contains("ov"));
}

#[test]
fn box_margin_padding_roundtrip() {
    let ctx = fresh_ctx();
    let (mt, ml, pt, pl) = ctx.with(|ctx| {
        runts_ink::js_bridge::install(&ctx).unwrap();
        let b: rquickjs::Object = ctx
            .eval(r#"runts_ink.box({ marginTop: 1, marginLeft: 2, paddingTop: 3, paddingLeft: 4, children: [] })"#)
            .unwrap();
        let inner: rquickjs::Object = b.get("Box").unwrap();
        let props: rquickjs::Object = inner.get("__props").unwrap();
        let mt: i32 = props.get("marginTop").unwrap();
        let ml: i32 = props.get("marginLeft").unwrap();
        let pt: i32 = props.get("paddingTop").unwrap();
        let pl: i32 = props.get("paddingLeft").unwrap();
        (mt, ml, pt, pl)
    });
    assert_eq!(mt, 1);
    assert_eq!(ml, 2);
    assert_eq!(pt, 3);
    assert_eq!(pl, 4);
}

#[test]
fn box_gap_renders() {
    let out = render_js(
        r#"
        let b = runts_ink.box({
            gap: 2,
            children: [runts_ink.text("g1", {}), runts_ink.text("g2", {})]
        });
        runts_ink.render_to_string(b)
    "#,
    );
    assert!(out.contains("g1"));
    assert!(out.contains("g2"));
}

#[test]
fn box_column_row_gap_renders() {
    let out = render_js(
        r#"
        let b = runts_ink.box({
            columnGap: 1,
            rowGap: 1,
            children: [runts_ink.text("cr", {})]
        });
        runts_ink.render_to_string(b)
    "#,
    );
    assert!(out.contains("cr"));
}

#[test]
fn box_border_sides_roundtrip() {
    let ctx = fresh_ctx();
    let (top, bottom) = ctx.with(|ctx| {
        runts_ink::js_bridge::install(&ctx).unwrap();
        let b: rquickjs::Object = ctx
            .eval(r#"runts_ink.box({ borderTop: true, borderBottom: false, borderStyle: "single", children: [] })"#)
            .unwrap();
        let inner: rquickjs::Object = b.get("Box").unwrap();
        let props: rquickjs::Object = inner.get("__props").unwrap();
        let top: bool = props.get("borderTop").unwrap();
        let bottom: bool = props.get("borderBottom").unwrap();
        (top, bottom)
    });
    assert!(top);
    assert!(!bottom);
}

#[test]
fn box_border_color_renders() {
    let out = render_js(
        r#"
        let b = runts_ink.box({
            borderStyle: "single",
            borderColor: "red",
            children: [runts_ink.text("bc", {})]
        });
        runts_ink.render_to_string(b)
    "#,
    );
    assert!(out.contains("bc"));
}

#[test]
fn box_background_color_renders() {
    let out = render_js(
        r#"
        let b = runts_ink.box({
            backgroundColor: "blue",
            children: [runts_ink.text("bg", {})]
        });
        runts_ink.render_to_string(b)
    "#,
    );
    assert!(out.contains("bg"));
}

#[test]
fn text_strikethrough_renders() {
    let out = render_js(
        r#"
        let t = runts_ink.text("strike", { strikethrough: true });
        runts_ink.render_to_string(t)
    "#,
    );
    assert!(out.contains("strike"));
}

#[test]
fn text_dim_color_renders() {
    let out = render_js(
        r#"
        let t = runts_ink.text("dim", { dimColor: true });
        runts_ink.render_to_string(t)
    "#,
    );
    assert!(out.contains("dim"));
}

#[test]
fn text_inverse_renders() {
    let out = render_js(
        r#"
        let t = runts_ink.text("inv", { inverse: true });
        runts_ink.render_to_string(t)
    "#,
    );
    assert!(out.contains("inv"));
}

#[test]
fn text_wrap_hard_renders() {
    let out = render_js(
        r#"
        let t = runts_ink.text("wrap", { wrap: "hard" });
        runts_ink.render_to_string(t)
    "#,
    );
    assert!(out.contains("wrap"));
}

#[test]
fn text_background_color_renders() {
    let out = render_js(
        r#"
        let t = runts_ink.text("tbg", { backgroundColor: "green" });
        runts_ink.render_to_string(t)
    "#,
    );
    assert!(out.contains("tbg"));
}

#[test]
fn align_self_content_renders() {
    let out = render_js(
        r#"
        let b = runts_ink.box({
            alignSelf: "center",
            alignContent: "center",
            children: [runts_ink.text("ac", {})]
        });
        runts_ink.render_to_string(b)
    "#,
    );
    assert!(out.contains("ac"));
}

#[test]
fn box_z_index_roundtrip() {
    let ctx = fresh_ctx();
    let z = ctx.with(|ctx| {
        runts_ink::js_bridge::install(&ctx).unwrap();
        let b: rquickjs::Object = ctx
            .eval(r#"runts_ink.box({ zIndex: 5, children: [] })"#)
            .unwrap();
        let inner: rquickjs::Object = b.get("Box").unwrap();
        let props: rquickjs::Object = inner.get("__props").unwrap();
        let z: i32 = props.get("zIndex").unwrap();
        z
    });
    assert_eq!(z, 5);
}

#[test]
fn box_margin_xy_roundtrip() {
    let ctx = fresh_ctx();
    let (mx, my) = ctx.with(|ctx| {
        runts_ink::js_bridge::install(&ctx).unwrap();
        let b: rquickjs::Object = ctx
            .eval(r#"runts_ink.box({ marginX: 3, marginY: 2, children: [] })"#)
            .unwrap();
        let inner: rquickjs::Object = b.get("Box").unwrap();
        let props: rquickjs::Object = inner.get("__props").unwrap();
        let ml: i32 = props.get("marginLeft").unwrap();
        let mr: i32 = props.get("marginRight").unwrap();
        let mt: i32 = props.get("marginTop").unwrap();
        let mb: i32 = props.get("marginBottom").unwrap();
        assert_eq!(ml, 3);
        assert_eq!(mr, 3);
        assert_eq!(mt, 2);
        assert_eq!(mb, 2);
        (ml, mt)
    });
    assert_eq!(mx, 3);
    assert_eq!(my, 2);
}

#[test]
fn box_overflow_shorthand_roundtrip() {
    let ctx = fresh_ctx();
    let (ox, oy) = ctx.with(|ctx| {
        runts_ink::js_bridge::install(&ctx).unwrap();
        let b: rquickjs::Object = ctx
            .eval(r#"runts_ink.box({ overflow: "hidden", children: [] })"#)
            .unwrap();
        let inner: rquickjs::Object = b.get("Box").unwrap();
        let props: rquickjs::Object = inner.get("__props").unwrap();
        let ox: String = props.get("overflowX").unwrap();
        let oy: String = props.get("overflowY").unwrap();
        (ox, oy)
    });
    assert_eq!(ox, "hidden");
    assert_eq!(oy, "hidden");
}

#[test]
fn text_color_renders() {
    let out = render_js(
        r#"
        let t = runts_ink.text("cyan", { color: "cyan" });
        runts_ink.render_to_string(t)
    "#,
    );
    assert!(out.contains("cyan"));
}

#[test]
fn text_bold_renders() {
    let out = render_js(
        r#"
        let t = runts_ink.text("bold", { bold: true });
        runts_ink.render_to_string(t)
    "#,
    );
    assert!(out.contains("bold"));
}

#[test]
fn text_italic_renders() {
    let out = render_js(
        r#"
        let t = runts_ink.text("italic", { italic: true });
        runts_ink.render_to_string(t)
    "#,
    );
    assert!(out.contains("italic"));
}

#[test]
fn text_underline_renders() {
    let out = render_js(
        r#"
        let t = runts_ink.text("underline", { underline: true });
        runts_ink.render_to_string(t)
    "#,
    );
    assert!(out.contains("underline"));
}

#[test]
fn text_wrap_truncate_renders() {
    let out = render_js(
        r#"
        let t = runts_ink.text("truncate", { wrap: "truncate" });
        runts_ink.render_to_string(t)
    "#,
    );
    assert!(out.contains("truncate"));
}
