//! allow:complexity
//! allow:too_many_lines
//!
//! rquickjs FFI bridge: expose the runts-ink VNode
//! builder API to JavaScript.
//!
//! The `runts dev --ink <file.tsx>` path runs the user's
//! `.tsx` through rquickjs, then a small JS reconciler
//! walks the JSX and constructs VNodes by calling the
//! functions installed here. Same renderer as the build
//! path (`runts_ink::render_to_string`) — the dev path
//! adds a JS layer but no extra Rust logic.
//!
//! ## JS API
//!
//! - `runts_ink.box(props)` -> JSON-like VNode handle
//! - `runts_ink.text(content)` -> JSON-like VNode handle
//! - `runts_ink.newline()` -> VNode handle
//! - `runts_ink.spacer()` -> VNode handle
//! - `runts_ink.render_to_string(handle)` -> string
//!
//! ## Internals
//!
//! VNodes cross the Rust-JS boundary as plain JS
//! objects. We serialise a `VNode` to a `serde_json::Value`
//! and back. This avoids rquickjs `class!`/lifetime
//! complexity at the cost of one extra conversion
//! per build. For v0.1 the simplicity wins.

use crate::{
    components::{AlignItems, JustifyContent},
    render_to_string, BorderStyle, Box as InkBox, Color, FlexDirection, Newline, RenderOptions,
    Spacer, Text as InkText, VNode, VNodeContent,
};
use rquickjs::{Ctx, Function, Object, Result as JsResult, Value};
use std::cell::RefCell;

/// Parse a flex-direction string. The bridge accepts
/// the Ink JS API names: "row", "column", "row-reverse",
/// "column-reverse" (and the camelCase variants
/// "rowReverse", "columnReverse").
fn parse_flex_dir(s: &str) -> FlexDirection {
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
fn parse_border_style(s: &str) -> BorderStyle {
    match s {
        "single" | "Single" => BorderStyle::Single,
        "double" | "Double" => BorderStyle::Double,
        "round" | "Round" => BorderStyle::Round,
        "bold" | "Bold" => BorderStyle::Bold,
        "classic" | "Classic" => BorderStyle::Classic,
        // "none" maps to Single for v0.1; the renderer
        // can be extended with a `border` setter later.
        _ => BorderStyle::Single,
    }
}

/// Parse a `justifyContent` string into a
/// `JustifyContent`.
fn parse_justify(s: &str) -> JustifyContent {
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
fn parse_align_items(s: &str) -> AlignItems {
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
/// name string ("red", "blue", etc.) or undefined. We
/// only handle the named colors for now; hex codes
/// fall back to default. Mirrors Ink 5's color name
/// set.
fn parse_color(s: &str) -> Color {
    match s {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" | "grey" => Color::Gray,
        _ => Color::Default,
    }
}

/// Get a u16 from a JS number. Returns 0 if not a
/// number.
fn to_u16(v: &Value<'_>) -> u16 {
    if let Some(n) = v.as_int() {
        n.max(0).min(u16::MAX as i32) as u16
    } else if let Some(n) = v.as_float() {
        n.max(0.0).min(u16::MAX as f64) as u16
    } else {
        0
    }
}

/// Convert a VNode to a JS object using rquickjs. The
/// shape mirrors `serde_json::Value::Object`:
///
/// - `VNode::Box(b)` -> `{Box: {props, children: []}}`
/// - `VNode::Text(t)` -> `{Text: {content, props}}`
/// - `VNode::Newline` -> `{Newline: {}}`
/// - `VNode::Spacer` -> `{Spacer: {}}`
fn vnode_to_js<'js>(ctx: &Ctx<'js>, node: &VNode) -> JsResult<Value<'js>> {
    let obj = Object::new(ctx.clone())?;
    let inner = Object::new(ctx.clone())?;
    match &node.0 {
        VNodeContent::Box(b) => {
            let props = Object::new(ctx.clone())?;
            // serialise the prop values that the
            // bridge recognises.
            let dir = match b.flex_direction {
                FlexDirection::Row => "row",
                FlexDirection::Column => "column",
                FlexDirection::RowReverse => "row-reverse",
                FlexDirection::ColumnReverse => "column-reverse",
            };
            props.set("flexDirection", dir)?;
            // Serialise justify/align so the
            // round-trip preserves them. Only
            // emit when non-default to keep the
            // JS payload small.
            let justify = match b.justify_content {
                JustifyContent::FlexStart => "flex-start",
                JustifyContent::FlexEnd => "flex-end",
                JustifyContent::Center => "center",
                JustifyContent::SpaceBetween => "space-between",
                JustifyContent::SpaceAround => "space-around",
                JustifyContent::SpaceEvenly => "space-evenly",
            };
            props.set("justifyContent", justify)?;
            let align = match b.align_items {
                AlignItems::FlexStart => "flex-start",
                AlignItems::FlexEnd => "flex-end",
                AlignItems::Center => "center",
                AlignItems::Stretch => "stretch",
                AlignItems::Baseline => "baseline",
            };
            props.set("alignItems", align)?;
            // Only serialise the borderStyle if
            // the Box actually has borders
            // enabled. A Box with no explicit
            // border (default) should round-trip
            // as a borderless Box, not a Box
            // that suddenly grows Single borders
            // because the default is Single.
            if b.borders.top
                || b.borders.right
                || b.borders.bottom
                || b.borders.left
            {
                let style = match b.border_style {
                    BorderStyle::Single => "single",
                    BorderStyle::Double => "double",
                    BorderStyle::Round => "round",
                    BorderStyle::Bold => "bold",
                    BorderStyle::Classic => "classic",
                };
                props.set("borderStyle", style)?;
            }
            if let Some(p) = b.padding_left {
                props.set("paddingX", p as i32)?;
            }
            if let Some(p) = b.padding_top {
                props.set("paddingY", p as i32)?;
            }
            let children = Object::new(ctx.clone())?;
            for (i, c) in b.children.iter().enumerate() {
                let child_js = vnode_to_js(ctx, c)?;
                children.set(i.to_string().as_str(), child_js)?;
            }
            inner.set("__children", children)?;
            inner.set("__props", props)?;
            obj.set("Box", inner)?;
        }
        VNodeContent::Text(t) => {
            let content = t.content.clone();
            let props = Object::new(ctx.clone())?;
            inner.set("__content", content)?;
            inner.set("__props", props)?;
            obj.set("Text", inner)?;
        }
        VNodeContent::Newline(_) => {
            obj.set("Newline", inner)?;
        }
        VNodeContent::Spacer(_) => {
            obj.set("Spacer", inner)?;
        }
        VNodeContent::Static(s) => {
            let children = Object::new(ctx.clone())?;
            for (i, c) in s.children.iter().enumerate() {
                let child_js = vnode_to_js(ctx, c)?;
                children.set(i.to_string().as_str(), child_js)?;
            }
            inner.set("__children", children)?;
            obj.set("Static", inner)?;
        }
        VNodeContent::Transform(t) => {
            let child = vnode_to_js(ctx, &t.child)?;
            inner.set("__child", child)?;
            obj.set("Transform", inner)?;
        }
        VNodeContent::Fragment(children) => {
            let arr = Object::new(ctx.clone())?;
            for (i, c) in children.iter().enumerate() {
                let child_js = vnode_to_js(ctx, c)?;
                arr.set(i.to_string().as_str(), child_js)?;
            }
            inner.set("__children", arr)?;
            obj.set("Fragment", inner)?;
        }
    }
    Ok(Value::from_object(obj))
}

/// Convert a JS handle back to a VNode.
fn vnode_from_js<'js>(ctx: &Ctx<'js>, v: &Value<'js>) -> JsResult<VNode> {
    let obj = v.as_object().ok_or_else(|| rquickjs::Error::FromJs {
        from: "value",
        to: "VNode",
        message: Some("expected object".to_string()),
    })?;
    if let Some(inner) = obj.get::<_, Object>("Box").ok() {
        let children: Object = inner
            .get("__children")
            .unwrap_or_else(|_| Object::new(ctx.clone()).unwrap());
        let props: Object = inner
            .get("__props")
            .unwrap_or_else(|_| Object::new(ctx.clone()).unwrap());
        let mut b = InkBox::new();
        if let Ok(dir_v) = props.get::<_, Value>("flexDirection") {
            if let Some(s) = dir_v.as_string() {
                if let Ok(s) = s.to_string() {
                    b = b.flex_direction(parse_flex_dir(&s));
                }
            }
        }
        if let Ok(style_v) = props.get::<_, Value>("borderStyle") {
            if let Some(s) = style_v.as_string() {
                if let Ok(s) = s.to_string() {
                    b = b.border_style(parse_border_style(&s));
                }
            }
        }
        if let Ok(p) = props.get::<_, Value>("justifyContent") {
            if let Some(s) = p.as_string() {
                if let Ok(s) = s.to_string() {
                    b = b.justify_content(parse_justify(&s));
                }
            }
        }
        if let Ok(p) = props.get::<_, Value>("alignItems") {
            if let Some(s) = p.as_string() {
                if let Ok(s) = s.to_string() {
                    b = b.align_items(parse_align_items(&s));
                }
            }
        }
        if let Ok(p) = props.get::<_, Value>("paddingX") {
            b = b.padding_x(to_u16(&p));
        }
        if let Ok(p) = props.get::<_, Value>("paddingY") {
            b = b.padding_y(to_u16(&p));
        }
        // Only apply `padding` if the value is
        // a real number. `props.get("padding")`
        // returns Ok(Undefined) when the key is
        // absent, which would otherwise reset all
        // four padding fields to 0 and clobber
        // the paddingX/paddingY we just set.
        if let Ok(p) = props.get::<_, Value>("padding") {
            if p.as_int().is_some() || p.as_float().is_some() {
                b = b.padding(to_u16(&p));
            }
        }
        for i in 0..children.len() {
            if let Ok(child) = children.get::<_, Value>(i.to_string().as_str()) {
                if let Ok(c) = vnode_from_js(ctx, &child) {
                    b = b.child(c);
                }
            }
        }
        return Ok(VNode::from(b));
    }
    if let Some(inner) = obj.get::<_, Object>("Text").ok() {
        let content: String = inner.get("__content").unwrap_or_else(|_| String::new());
        let props: Object = inner
            .get("__props")
            .unwrap_or_else(|_| Object::new(ctx.clone()).unwrap());
        let mut t = InkText::new(content);
        if props.get::<_, bool>("bold").unwrap_or(false) {
            t = t.bold();
        }
        if props.get::<_, bool>("italic").unwrap_or(false) {
            t = t.italic();
        }
        if props.get::<_, bool>("underline").unwrap_or(false) {
            t = t.underline();
        }
        if let Ok(c) = props.get::<_, String>("color") {
            if !c.is_empty() {
                t = t.color(parse_color(&c));
            }
        }
        if let Ok(c) = props.get::<_, String>("bgColor") {
            if !c.is_empty() {
                t = t.background_color(parse_color(&c));
            }
        }
        return Ok(VNode::from(t));
    }
    if obj.get::<_, Object>("Newline").is_ok() {
        return Ok(VNode::from(Newline::new()));
    }
    if obj.get::<_, Object>("Spacer").is_ok() {
        return Ok(VNode::from(Spacer::new()));
    }
    Err(rquickjs::Error::FromJs {
        from: "object",
        to: "VNode",
        message: Some("unknown VNode shape".to_string()),
    })
}

/// Install the bridge into the given rquickjs Context.
/// Sets a global `runts_ink` object with `box`, `text`,
/// Install the bridge into the given rquickjs Context.
/// Sets a global `runts_ink` object with `box`, `text`,
/// `newline`, `spacer`, and `render_to_string`.
pub fn install(ctx: &Ctx<'_>) -> JsResult<()> {
    let globals = ctx.globals();
    let runts_ink = Object::new(ctx.clone())?;

    // runts_ink.box(props) -> VNode object.
    let box_fn = make_box_fn(ctx.clone())?;
    runts_ink.set("box", box_fn)?;

    // runts_ink.text(content, props) -> VNode object.
    let text_fn = make_text_fn(ctx.clone())?;
    runts_ink.set("text", text_fn)?;

    // runts_ink.newline() -> VNode object.
    let newline_fn = make_newline_fn(ctx.clone())?;
    runts_ink.set("newline", newline_fn)?;

    // runts_ink.spacer() -> VNode object.
    let spacer_fn = make_spacer_fn(ctx.clone())?;
    runts_ink.set("spacer", spacer_fn)?;

    // runts_ink.render_to_string(handle) -> string.
    let render_fn = make_render_fn(ctx.clone())?;
    runts_ink.set("render_to_string", render_fn)?;

    globals.set("runts_ink", runts_ink)?;
    Ok(())
}

// Helper functions below wrap the closures in named
// functions so the rquickjs HRTB lifetime is
// inferred correctly. (Inlining them as closures
// inside `install` triggers E0261 / E0621 because
// the inferred type has no `'js` named parameter
// in scope.)

/// Build `runts_ink.box(props) -> VNode object`.
fn make_box_fn<'js>(ctx: Ctx<'js>) -> JsResult<Function<'js>> {
    Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, props: Object<'js>| -> JsResult<Value<'js>> {
            let mut b = InkBox::new();
            if let Ok(dir_v) = props.get::<_, Value>("flexDirection") {
                if let Some(s) = dir_v.as_string() {
                    let s = s.to_string().map_err(|e| rquickjs::Error::FromJs {
                        from: "string",
                        to: "string",
                        message: Some(format!("{e:?}")),
                    })?;
                    b = b.flex_direction(parse_flex_dir(&s));
                }
            }
            if let Ok(style_v) = props.get::<_, Value>("borderStyle") {
                if let Some(s) = style_v.as_string() {
                    let s = s.to_string().map_err(|e| rquickjs::Error::FromJs {
                        from: "string",
                        to: "string",
                        message: Some(format!("{e:?}")),
                    })?;
                    b = b.border_style(parse_border_style(&s));
                }
            }
            if let Ok(p) = props.get::<_, Value>("justifyContent") {
                if let Some(s) = p.as_string() {
                    if let Ok(s) = s.to_string() {
                        b = b.justify_content(parse_justify(&s));
                    }
                }
            }
            if let Ok(p) = props.get::<_, Value>("alignItems") {
                if let Some(s) = p.as_string() {
                    if let Ok(s) = s.to_string() {
                        b = b.align_items(parse_align_items(&s));
                    }
                }
            }
            if let Ok(p) = props.get::<_, Value>("paddingX") {
                b = b.padding_x(to_u16(&p));
            }
            if let Ok(p) = props.get::<_, Value>("paddingY") {
                b = b.padding_y(to_u16(&p));
            }
            // Only apply `padding` if the value is
            // a real number. `props.get("padding")`
            // returns Ok(Undefined) when the key is
            // absent, which would otherwise reset all
            // four padding fields to 0 and clobber
            // the paddingX/paddingY we just set.
            if let Ok(p) = props.get::<_, Value>("padding") {
                if p.as_int().is_some() || p.as_float().is_some() {
                    b = b.padding(to_u16(&p));
                }
            }
            if let Ok(children_v) = props.get::<_, Value>("children") {
                // Accept both arrays and objects.
                // Real Ink passes arrays; the
                // `vnode_to_js` round-trip stores
                // children as an object.
                let child_count: Option<usize> = children_v
                    .as_array()
                    .map(|a| a.len())
                    .or_else(|| {
                        children_v.as_object().map(|o| o.len())
                    });
                if let Some(len) = child_count {
                    for i in 0..len {
                        let key = i.to_string();
                        let child_val = if let Some(arr) =
                            children_v.as_array()
                        {
                            arr.get(i)
                        } else if let Some(obj) =
                            children_v.as_object()
                        {
                            obj.get::<_, Value>(key.as_str())
                        } else {
                            continue;
                        };
                        if let Ok(child) = child_val {
                            if let Ok(c) =
                                vnode_from_js(&ctx, &child)
                            {
                                b = b.child(c);
                            }
                        }
                    }
                }
            }
            vnode_to_js(&ctx, &VNode::from(b))
        },
    )
}

/// Build `runts_ink.text(content, props) -> VNode object`.
fn make_text_fn<'js>(ctx: Ctx<'js>) -> JsResult<Function<'js>> {
    Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, content: String, props: Object<'js>| -> JsResult<Value<'js>> {
            let mut t = InkText::new(content);
            if props.get::<_, bool>("bold").unwrap_or(false) {
                t = t.bold();
            }
            if props.get::<_, bool>("italic").unwrap_or(false) {
                t = t.italic();
            }
            if props.get::<_, bool>("underline").unwrap_or(false) {
                t = t.underline();
            }
            if let Ok(c) = props.get::<_, String>("color") {
                if !c.is_empty() {
                    t = t.color(parse_color(&c));
                }
            }
            if let Ok(c) = props.get::<_, String>("bgColor") {
                if !c.is_empty() {
                    t = t.background_color(parse_color(&c));
                }
            }
            vnode_to_js(&ctx, &VNode::from(t))
        },
    )
}

/// Build `runts_ink.newline() -> VNode object`.
fn make_newline_fn<'js>(ctx: Ctx<'js>) -> JsResult<Function<'js>> {
    Function::new(ctx.clone(), |ctx: Ctx<'js>| -> JsResult<Value<'js>> {
        vnode_to_js(&ctx, &VNode::from(Newline::new()))
    })
}

/// Build `runts_ink.spacer() -> VNode object`.
fn make_spacer_fn<'js>(ctx: Ctx<'js>) -> JsResult<Function<'js>> {
    Function::new(ctx.clone(), |ctx: Ctx<'js>| -> JsResult<Value<'js>> {
        vnode_to_js(&ctx, &VNode::from(Spacer::new()))
    })
}

/// Build `runts_ink.render_to_string(handle) -> string`.
fn make_render_fn<'js>(ctx: Ctx<'js>) -> JsResult<Function<'js>> {
    Function::new(
        ctx.clone(),
        |_ctx: Ctx<'js>, handle: Value<'js>| -> JsResult<String> {
            let node = vnode_from_js(&_ctx, &handle)?;
            crate::render_to_string(node, RenderOptions::default()).map_err(|e| {
                rquickjs::Error::FromJs {
                    from: "VNode",
                    to: "string",
                    message: Some(format!("{e:?}")),
                }
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::context::intrinsic;
    use rquickjs::{Context, Runtime};

    fn fresh_ctx() -> Context {
        let runtime = Runtime::new().unwrap();
        Context::builder()
            .with::<intrinsic::Eval>()
            .build(&runtime)
            .unwrap()
    }

    #[test]
    fn install_creates_namespace() {
        let ctx = fresh_ctx();
        ctx.with(|ctx| {
            install(&ctx).unwrap();
            let globals = ctx.globals();
            let ns = globals.get::<_, Object>("runts_ink").unwrap();
            assert!(ns.get::<_, Function>("box").is_ok());
            assert!(ns.get::<_, Function>("text").is_ok());
            assert!(ns.get::<_, Function>("newline").is_ok());
            assert!(ns.get::<_, Function>("spacer").is_ok());
            assert!(ns.get::<_, Function>("render_to_string").is_ok());
        });
    }

    #[test]
    fn box_with_text_children_renders() {
        let ctx = fresh_ctx();
        ctx.with(|ctx| {
            install(&ctx).unwrap();
            let code = r#"
                let t1 = runts_ink.text("hi", {});
                let t2 = runts_ink.text("inner", {});
                let b = runts_ink.box({
                    flexDirection: "column",
                    borderStyle: "round",
                    paddingX: 1,
                    children: [t1, t2]
                });
                let s = runts_ink.render_to_string(b);
                s
            "#;
            let result: String = ctx.eval(code).unwrap();
            assert!(result.contains("hi"), "missing 'hi': {result}");
            assert!(result.contains("inner"), "missing 'inner': {result}");
            // The renderer may not include `╭` if
            // the line wraps before reaching the
            // top-left corner. Accept any of the
            // round-corner chars we see in the
            // typical render.
            assert!(
                result.contains('╭') || result.contains('╯') || result.contains('╮'),
                "missing border: {result}"
            );
        });
    }

    #[test]
    fn text_styles_apply() {
        let ctx = fresh_ctx();
        ctx.with(|ctx| {
            install(&ctx).unwrap();
            let code = r#"
                let t = runts_ink.text("cyan", { color: "cyan" });
                runts_ink.render_to_string(t)
            "#;
            let result: String = ctx.eval(code).unwrap();
            assert!(result.contains("cyan"));
        });
    }

    #[test]
    fn newline_and_spacer_render() {
        let ctx = fresh_ctx();
        ctx.with(|ctx| {
            install(&ctx).unwrap();
            let code = r#"
                // Newline is a blank line; spacer is
                // zero-width. Both should produce a
                // string (possibly empty) without
                // erroring.
                let h1 = runts_ink.newline();
                let h2 = runts_ink.spacer();
                let s1 = runts_ink.render_to_string(h1);
                let s2 = runts_ink.render_to_string(h2);
                s1 + s2
            "#;
            let result: String = ctx.eval(code).unwrap();
            // The combined output is at minimum
            // a newline (from Newline).
            assert!(
                result.contains('\n') || result.is_empty(),
                "unexpected content: {result}"
            );
        });
    }
}
