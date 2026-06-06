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

mod parsers;
use parsers::*;

use crate::{
    components::{AlignItems, JustifyContent},
    render_to_string, BorderStyle, Box as InkBox, FlexDirection, Newline, RenderOptions,
    Spacer, Text as InkText, VNode, VNodeContent,
};
use rquickjs::{Ctx, Function, Object, Result as JsResult, Value};
use std::cell::RefCell;

/// Convert a VNode to a JS object using rquickjs. The
/// - `VNode::Box(b)` -> `{Box: {props, children: []}}`
/// - `VNode::Text(t)` -> `{Text: {content, props}}`
/// - `VNode::Newline` -> `{Newline: {}}`
/// - `VNode::Spacer` -> `{Spacer: {}}`
fn vnode_to_js<'js>(ctx: &Ctx<'js>, node: &VNode) -> JsResult<Value<'js>> {
    let obj = Object::new(ctx.clone())?;
    let inner = Object::new(ctx.clone())?;
    convert_vnode_to_js(ctx, &obj, &inner, node)?;
    Ok(Value::from_object(obj))
}

fn convert_vnode_to_js<'js>(ctx: &Ctx<'js>, obj: &Object<'js>, inner: &Object<'js>, node: &VNode) -> JsResult<()> {
    match &node.0 {
        VNodeContent::Box(b) => vnode_to_js_box(ctx, obj, inner, b),
        VNodeContent::Text(t) => vnode_to_js_text(ctx, obj, inner, t),
        VNodeContent::Newline(_) => obj.set("Newline", inner.clone()),
        VNodeContent::Spacer(_) => obj.set("Spacer", inner.clone()),
        VNodeContent::Static(s) => vnode_to_js_static(ctx, obj, inner, s),
        VNodeContent::Transform(t) => vnode_to_js_transform(ctx, obj, inner, t),
        VNodeContent::Fragment(c) => vnode_to_js_fragment(ctx, obj, inner, c),
    }
}

fn vnode_to_js_text<'js>(ctx: &Ctx<'js>, obj: &Object<'js>, inner: &Object<'js>, t: &InkText) -> JsResult<()> {
    let props = Object::new(ctx.clone())?;
    inner.set("__content", &t.content)?;
    inner.set("__props", props)?;
    obj.set("Text", inner.clone())
}

fn vnode_to_js_static<'js>(ctx: &Ctx<'js>, obj: &Object<'js>, inner: &Object<'js>, s: &crate::components::Static) -> JsResult<()> {
    let children = vnode_to_js_children(ctx, &s.children)?;
    inner.set("__children", children)?;
    obj.set("Static", inner.clone())
}

fn vnode_to_js_transform<'js>(ctx: &Ctx<'js>, obj: &Object<'js>, inner: &Object<'js>, t: &crate::components::Transform) -> JsResult<()> {
    let child = vnode_to_js(ctx, &t.child)?;
    inner.set("__child", child)?;
    obj.set("Transform", inner.clone())
}

fn vnode_to_js_fragment<'js>(ctx: &Ctx<'js>, obj: &Object<'js>, inner: &Object<'js>, c: &[VNode]) -> JsResult<()> {
    let arr = vnode_to_js_children(ctx, c)?;
    inner.set("__children", arr)?;
    obj.set("Fragment", inner.clone())
}
fn vnode_to_js_box<'js>(ctx: &Ctx<'js>, obj: &Object<'js>, inner: &Object<'js>, b: &InkBox) -> JsResult<()> {
    let props = Object::new(ctx.clone())?;
    set_box_props(&props, b)?;
    let children = vnode_to_js_children(ctx, &b.children)?;
    inner.set("__children", children)?;
    inner.set("__props", props)?;
    obj.set("Box", inner.clone())?;
    Ok(())
}
fn set_box_props<'js>(props: &Object<'js>, b: &InkBox) -> JsResult<()> { props.set("flexDirection", box_flex_dir(b))?; props.set("justifyContent", box_justify(b))?; props.set("alignItems", box_align(b))?; box_border(props, b)?; box_padding(props, b); Ok(()) }
fn box_flex_dir(b: &InkBox) -> &'static str { match b.flex_direction { FlexDirection::Row => "row", FlexDirection::Column => "column", FlexDirection::RowReverse => "row-reverse", FlexDirection::ColumnReverse => "column-reverse" } }
fn box_justify(b: &InkBox) -> &'static str { match b.justify_content { JustifyContent::FlexStart => "flex-start", JustifyContent::FlexEnd => "flex-end", JustifyContent::Center => "center", JustifyContent::SpaceBetween => "space-between", JustifyContent::SpaceAround => "space-around", JustifyContent::SpaceEvenly => "space-evenly" } }
fn box_align(b: &InkBox) -> &'static str { match b.align_items { AlignItems::FlexStart => "flex-start", AlignItems::FlexEnd => "flex-end", AlignItems::Center => "center", AlignItems::Stretch => "stretch", AlignItems::Baseline => "baseline" } }
fn box_border<'js>(props: &Object<'js>, b: &InkBox) -> JsResult<()> {
    if has_any_border(&b.borders) {
        let style = border_style_name(b.border_style);
        props.set("borderStyle", style)?;
    }
    Ok(())
}

fn has_any_border(b: &crate::style::Borders) -> bool {
    b.top || b.right || b.bottom || b.left
}

fn border_style_name(style: BorderStyle) -> &'static str {
    const STYLES: [(BorderStyle, &str); 5] = [
        (BorderStyle::Single, "single"),
        (BorderStyle::Double, "double"),
        (BorderStyle::Round, "round"),
        (BorderStyle::Bold, "bold"),
        (BorderStyle::Classic, "classic"),
    ];
    STYLES.iter().find(|(s, _)| *s == style).map(|(_, n)| *n).unwrap_or("single")
}
fn box_padding<'js>(props: &Object<'js>, b: &InkBox) { if let Some(p) = b.padding_left { let _ = props.set("paddingX", p as i32); } if let Some(p) = b.padding_top { let _ = props.set("paddingY", p as i32); } }
fn vnode_to_js_children<'js>(ctx: &Ctx<'js>, children: &[VNode]) -> JsResult<Object<'js>> {
    let arr = Object::new(ctx.clone())?;
    for (i, c) in children.iter().enumerate() { let child_js = vnode_to_js(ctx, c)?; arr.set(i.to_string().as_str(), child_js)?; }
    Ok(arr)
}

/// Convert a JS handle back to a VNode.
fn vnode_from_js<'js>(ctx: &Ctx<'js>, v: &Value<'js>) -> JsResult<VNode> {
    let obj = v.as_object().ok_or_else(|| rquickjs::Error::FromJs { from: "value", to: "VNode", message: Some("expected object".to_string()) })?;
    if let Some(inner) = obj.get::<_, Object>("Box").ok() { return vnode_from_js_box(ctx, &inner); }
    if let Some(inner) = obj.get::<_, Object>("Text").ok() { return vnode_from_js_text(&inner); }
    if obj.get::<_, Object>("Newline").is_ok() { return Ok(VNode::from(Newline::new())); }
    if obj.get::<_, Object>("Spacer").is_ok() { return Ok(VNode::from(Spacer::new())); }
    Err(rquickjs::Error::FromJs { from: "object", to: "VNode", message: Some("unknown VNode shape".to_string()) })
}
fn vnode_from_js_box<'js>(ctx: &Ctx<'js>, inner: &Object<'js>) -> JsResult<VNode> {
    let children: Object = inner.get("__children").unwrap_or_else(|_| Object::new(ctx.clone()).unwrap());
    let props: Object = inner.get("__props").unwrap_or_else(|_| Object::new(ctx.clone()).unwrap());
    let mut b = InkBox::new();
    apply_box_props(&props, &mut b);
    for i in 0..children.len() { if let Ok(child) = children.get::<_, Value>(i.to_string().as_str()) { if let Ok(c) = vnode_from_js(ctx, &child) { b = b.child(c); } } }
    Ok(VNode::from(b))
}
fn apply_box_props<'js>(props: &Object<'js>, b: &mut InkBox) {
    apply_box_flex_dir(props, b);
    apply_box_flex_grow(props, b);
    apply_box_border_style(props, b);
    apply_box_justify(props, b);
    apply_box_align(props, b);
    apply_box_padding_x(props, b);
    apply_box_padding_y(props, b);
    apply_box_padding(props, b);
}

fn apply_box_flex_dir<'js>(props: &Object<'js>, b: &mut InkBox) {
    if let Ok(dir_v) = props.get::<_, Value>("flexDirection") {
        if let Some(s) = dir_v.as_string() {
            if let Ok(s) = s.to_string() {
                *b = b.clone().flex_direction(parse_flex_dir(&s));
            }
        }
    }
}

fn apply_box_flex_grow<'js>(props: &Object<'js>, b: &mut InkBox) {
    if let Ok(grow_v) = props.get::<_, Value>("flexGrow") {
        if let Some(n) = grow_v.as_int() {
            *b = b.clone().flex_grow(n as f32);
        } else if let Some(n) = grow_v.as_float() {
            *b = b.clone().flex_grow(n as f32);
        }
    }
}

fn apply_box_border_style<'js>(props: &Object<'js>, b: &mut InkBox) {
    if let Ok(style_v) = props.get::<_, Value>("borderStyle") {
        if let Some(s) = style_v.as_string() {
            if let Ok(s) = s.to_string() {
                *b = b.clone().border_style(parse_border_style(&s));
            }
        }
    }
}

fn apply_box_justify<'js>(props: &Object<'js>, b: &mut InkBox) {
    if let Ok(p) = props.get::<_, Value>("justifyContent") {
        if let Some(s) = p.as_string() {
            if let Ok(s) = s.to_string() {
                *b = b.clone().justify_content(parse_justify(&s));
            }
        }
    }
}

fn apply_box_align<'js>(props: &Object<'js>, b: &mut InkBox) {
    if let Ok(p) = props.get::<_, Value>("alignItems") {
        if let Some(s) = p.as_string() {
            if let Ok(s) = s.to_string() {
                *b = b.clone().align_items(parse_align_items(&s));
            }
        }
    }
}

fn apply_box_padding_x<'js>(props: &Object<'js>, b: &mut InkBox) {
    if let Ok(p) = props.get::<_, Value>("paddingX") {
        *b = b.clone().padding_x(to_u16(&p));
    }
}

fn apply_box_padding_y<'js>(props: &Object<'js>, b: &mut InkBox) {
    if let Ok(p) = props.get::<_, Value>("paddingY") {
        *b = b.clone().padding_y(to_u16(&p));
    }
}

fn apply_box_padding<'js>(props: &Object<'js>, b: &mut InkBox) {
    if let Ok(p) = props.get::<_, Value>("padding") {
        if p.as_int().is_some() || p.as_float().is_some() {
            *b = b.clone().padding(to_u16(&p));
        }
    }
}
fn vnode_from_js_text<'js>(inner: &Object<'js>) -> JsResult<VNode> {
    let content: String = inner.get("__content").unwrap_or_else(|_| String::new());
    let props: Object = inner.get("__props").unwrap_or_else(|_| Object::new(inner.ctx().clone()).unwrap());
    let mut t = InkText::new(content);
    if props.get::<_, bool>("bold").unwrap_or(false) { t = t.bold(); }
    if props.get::<_, bool>("italic").unwrap_or(false) { t = t.italic(); }
    if props.get::<_, bool>("underline").unwrap_or(false) { t = t.underline(); }
    if let Ok(c) = props.get::<_, String>("color") { if !c.is_empty() { t = t.color(parse_color(&c)); } }
    if let Ok(c) = props.get::<_, String>("bgColor") { if !c.is_empty() { t = t.background_color(parse_color(&c)); } }
    Ok(VNode::from(t))
}

/// Install the bridge into the given rquickjs Context.
/// Install the bridge into the given rquickjs Context.
/// Sets a global `runts_ink` object with `box`, `text`,
/// `newline`, `spacer`, and `render_to_string`.
pub fn install(ctx: &Ctx<'_>) -> JsResult<()> {
    let globals = ctx.globals();
    let runts_ink = Object::new(ctx.clone())?;
    install_functions(ctx.clone(), &runts_ink)?;
    globals.set("runts_ink", runts_ink)
}

fn install_functions<'js>(ctx: Ctx<'js>, runts_ink: &Object<'js>) -> JsResult<()> {
    let box_fn = make_box_fn(ctx.clone())?;
    runts_ink.set("box", box_fn)?;
    let text_fn = make_text_fn(ctx.clone())?;
    runts_ink.set("text", text_fn)?;
    let newline_fn = make_newline_fn(ctx.clone())?;
    runts_ink.set("newline", newline_fn)?;
    let spacer_fn = make_spacer_fn(ctx.clone())?;
    runts_ink.set("spacer", spacer_fn)?;
    let render_fn = make_render_fn(ctx.clone())?;
    runts_ink.set("render_to_string", render_fn)
}

// Helper functions below wrap the closures in named
// functions so the rquickjs HRTB lifetime is
// inferred correctly. (Inlining them as closures
// inside `install` triggers E0261 / E0621 because
// the inferred type has no `'js` named parameter
// in scope.)

/// Build `runts_ink.box(props) -> VNode object`.
fn make_box_fn<'js>(ctx: Ctx<'js>) -> JsResult<Function<'js>> {
    Function::new(ctx.clone(), |ctx: Ctx<'js>, props: Object<'js>| -> JsResult<Value<'js>> { box_fn_impl(&ctx, &props) })
}
fn box_fn_impl<'js>(ctx: &Ctx<'js>, props: &Object<'js>) -> JsResult<Value<'js>> {
    let mut b = InkBox::new();
    apply_box_fn_props(props, &mut b)?;
    if let Ok(children_v) = props.get::<_, Value>("children") { box_add_children(ctx, props, children_v, &mut b)?; }
    vnode_to_js(ctx, &VNode::from(b))
}
fn apply_box_fn_props<'js>(props: &Object<'js>, b: &mut InkBox) -> JsResult<()> {
    apply_fn_flex_dir(props, b)?;
    apply_fn_border_style(props, b)?;
    apply_fn_justify(props, b);
    apply_fn_align(props, b);
    apply_fn_padding_x(props, b);
    apply_fn_padding_y(props, b);
    apply_fn_padding(props, b);
    Ok(())
}

fn apply_fn_flex_dir<'js>(props: &Object<'js>, b: &mut InkBox) -> JsResult<()> {
    if let Ok(dir_v) = props.get::<_, Value>("flexDirection") {
        if let Some(s) = dir_v.as_string() {
            let s = s.to_string().map_err(|e| rquickjs::Error::FromJs { from: "string", to: "string", message: Some(format!("{e:?}")) })?;
            *b = b.clone().flex_direction(parse_flex_dir(&s));
        }
    }
    Ok(())
}

fn apply_fn_border_style<'js>(props: &Object<'js>, b: &mut InkBox) -> JsResult<()> {
    if let Ok(style_v) = props.get::<_, Value>("borderStyle") {
        if let Some(s) = style_v.as_string() {
            let s = s.to_string().map_err(|e| rquickjs::Error::FromJs { from: "string", to: "string", message: Some(format!("{e:?}")) })?;
            *b = b.clone().border_style(parse_border_style(&s));
        }
    }
    Ok(())
}

fn apply_fn_justify<'js>(props: &Object<'js>, b: &mut InkBox) {
    if let Ok(p) = props.get::<_, Value>("justifyContent") {
        if let Some(s) = p.as_string() {
            if let Ok(s) = s.to_string() {
                *b = b.clone().justify_content(parse_justify(&s));
            }
        }
    }
}

fn apply_fn_align<'js>(props: &Object<'js>, b: &mut InkBox) {
    if let Ok(p) = props.get::<_, Value>("alignItems") {
        if let Some(s) = p.as_string() {
            if let Ok(s) = s.to_string() {
                *b = b.clone().align_items(parse_align_items(&s));
            }
        }
    }
}

fn apply_fn_padding_x<'js>(props: &Object<'js>, b: &mut InkBox) {
    if let Ok(p) = props.get::<_, Value>("paddingX") {
        *b = b.clone().padding_x(to_u16(&p));
    }
}

fn apply_fn_padding_y<'js>(props: &Object<'js>, b: &mut InkBox) {
    if let Ok(p) = props.get::<_, Value>("paddingY") {
        *b = b.clone().padding_y(to_u16(&p));
    }
}

fn apply_fn_padding<'js>(props: &Object<'js>, b: &mut InkBox) {
    if let Ok(p) = props.get::<_, Value>("padding") {
        if p.as_int().is_some() || p.as_float().is_some() {
            *b = b.clone().padding(to_u16(&p));
        }
    }
}
fn box_add_children<'js>(ctx: &Ctx<'js>, props: &Object<'js>, children_v: Value<'js>, b: &mut InkBox) -> JsResult<()> {
    let child_count: Option<usize> = children_v.as_array().map(|a| a.len()).or_else(|| children_v.as_object().map(|o| o.len()));
    if let Some(len) = child_count { for i in 0..len { let key = i.to_string(); let child_val = if let Some(arr) = children_v.as_array() { arr.get(i) } else if let Some(obj) = children_v.as_object() { obj.get::<_, Value>(key.as_str()) } else { continue; }; if let Ok(child) = child_val { if let Ok(c) = vnode_from_js(ctx, &child) { *b = b.clone().child(c); } } } }
    Ok(())
}

/// Build `runts_ink.text(content, props) -> VNode object`.
fn make_text_fn<'js>(ctx: Ctx<'js>) -> JsResult<Function<'js>> {
    Function::new(ctx.clone(), |ctx: Ctx<'js>, content: rquickjs::Value<'js>, props: Object<'js>| -> JsResult<Value<'js>> {
        make_text_vnode(&ctx, content, &props)
    })
}

fn make_text_vnode<'js>(ctx: &Ctx<'js>, content: rquickjs::Value<'js>, props: &Object<'js>) -> JsResult<Value<'js>> {
    let content_str = extract_string_content(content);
    let mut t = InkText::new(content_str);
    apply_text_styles(props, &mut t);
    vnode_to_js(ctx, &VNode::from(t))
}

fn extract_string_content(content: rquickjs::Value<'_>) -> String {
    if let Some(s) = content.as_string() {
        let raw = s.to_string().unwrap_or_default();
        if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
            raw[1..raw.len() - 1].to_string()
        } else {
            raw
        }
    } else {
        content.get::<String>().unwrap_or_default()
    }
}

fn apply_text_styles(props: &Object<'_>, t: &mut InkText) {
    if let Ok(true) = props.get::<_, bool>("bold") { *t = t.clone().bold(); }
    if let Ok(true) = props.get::<_, bool>("italic") { *t = t.clone().italic(); }
    if let Ok(true) = props.get::<_, bool>("underline") { *t = t.clone().underline(); }
    if let Ok(c) = props.get::<_, String>("color") { if !c.is_empty() { *t = t.clone().color(parse_color(&c)); } }
    if let Ok(c) = props.get::<_, String>("bgColor") { if !c.is_empty() { *t = t.clone().background_color(parse_color(&c)); } }
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
    include!("tests.inc");
}
