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

mod box_props;
mod parsers;
mod text_props;

use crate::{
    render_to_string, Box as InkBox, Newline, RenderOptions, Spacer, Text as InkText, VNode,
    VNodeContent,
};
use rquickjs::{Ctx, Function, Object, Result as JsResult, Value};

/* -------------------------------------------------------------------------- */
/* VNode -> JS object                                                         */
/* -------------------------------------------------------------------------- */

fn vnode_to_js<'js>(ctx: &Ctx<'js>, node: &VNode) -> JsResult<Value<'js>> {
    let obj = Object::new(ctx.clone())?;
    let inner = Object::new(ctx.clone())?;
    convert_vnode_to_js(ctx, &obj, &inner, node)?;
    Ok(Value::from_object(obj))
}

fn convert_vnode_to_js<'js>(
    ctx: &Ctx<'js>,
    obj: &Object<'js>,
    inner: &Object<'js>,
    node: &VNode,
) -> JsResult<()> {
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

fn vnode_to_js_box<'js>(
    ctx: &Ctx<'js>,
    obj: &Object<'js>,
    inner: &Object<'js>,
    b: &InkBox,
) -> JsResult<()> {
    let props = Object::new(ctx.clone())?;
    box_props::serialize_box_props(&props, b)?;
    let children = vnode_to_js_children(ctx, &b.children)?;
    inner.set("__children", children)?;
    inner.set("__props", props)?;
    obj.set("Box", inner.clone())?;
    Ok(())
}

fn vnode_to_js_text<'js>(
    ctx: &Ctx<'js>,
    obj: &Object<'js>,
    inner: &Object<'js>,
    t: &InkText,
) -> JsResult<()> {
    let props = Object::new(ctx.clone())?;
    text_props::serialize_text_props(&props, t)?;
    inner.set("__content", &t.content)?;
    inner.set("__props", props)?;
    obj.set("Text", inner.clone())
}

fn vnode_to_js_static<'js>(
    ctx: &Ctx<'js>,
    obj: &Object<'js>,
    inner: &Object<'js>,
    s: &crate::components::Static,
) -> JsResult<()> {
    let children = vnode_to_js_children(ctx, &s.children)?;
    inner.set("__children", children)?;
    obj.set("Static", inner.clone())
}

fn vnode_to_js_transform<'js>(
    ctx: &Ctx<'js>,
    obj: &Object<'js>,
    inner: &Object<'js>,
    t: &crate::components::Transform,
) -> JsResult<()> {
    let child = vnode_to_js(ctx, &t.child)?;
    inner.set("__child", child)?;
    obj.set("Transform", inner.clone())
}

fn vnode_to_js_fragment<'js>(
    ctx: &Ctx<'js>,
    obj: &Object<'js>,
    inner: &Object<'js>,
    c: &[VNode],
) -> JsResult<()> {
    let arr = vnode_to_js_children(ctx, c)?;
    inner.set("__children", arr)?;
    obj.set("Fragment", inner.clone())
}

fn vnode_to_js_children<'js>(
    ctx: &Ctx<'js>,
    children: &[VNode],
) -> JsResult<Object<'js>> {
    let arr = Object::new(ctx.clone())?;
    for (i, c) in children.iter().enumerate() {
        let child_js = vnode_to_js(ctx, c)?;
        arr.set(i.to_string().as_str(), child_js)?;
    }
    Ok(arr)
}

/* -------------------------------------------------------------------------- */
/* JS object -> VNode                                                         */
/* -------------------------------------------------------------------------- */

fn vnode_from_js<'js>(ctx: &Ctx<'js>, v: &Value<'js>) -> JsResult<VNode> {
    let obj = v.as_object().ok_or_else(|| rquickjs::Error::FromJs {
        from: "value",
        to: "VNode",
        message: Some("expected object".to_string()),
    })?;
    if let Ok(inner) = obj.get::<_, Object>("Box") {
        return vnode_from_js_box(ctx, &inner);
    }
    if let Ok(inner) = obj.get::<_, Object>("Text") {
        return vnode_from_js_text(&inner);
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

fn vnode_from_js_box<'js>(ctx: &Ctx<'js>, inner: &Object<'js>) -> JsResult<VNode> {
    let children: Object = inner
        .get("__children")
        .unwrap_or_else(|_| Object::new(ctx.clone()).unwrap());
    let props: Object = inner
        .get("__props")
        .unwrap_or_else(|_| Object::new(ctx.clone()).unwrap());
    let mut b = InkBox::new();
    box_props::apply_box_props(&props, &mut b);
    for i in 0..children.len() {
        if let Ok(child) = children.get::<_, Value>(i.to_string().as_str()) {
            if let Ok(c) = vnode_from_js(ctx, &child) {
                b = b.child(c);
            }
        }
    }
    Ok(VNode::from(b))
}

fn vnode_from_js_text(inner: &Object<'_>) -> JsResult<VNode> {
    let content: String = inner.get("__content").unwrap_or_else(|_| String::new());
    let props: Object = inner
        .get("__props")
        .unwrap_or_else(|_| Object::new(inner.ctx().clone()).unwrap());
    let mut t = InkText::new(content);
    text_props::apply_text_props(&props, &mut t);
    Ok(VNode::from(t))
}

/* -------------------------------------------------------------------------- */
/* Bridge installation                                                        */
/* -------------------------------------------------------------------------- */

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
    let fns = make_bridge_fns(ctx)?;
    runts_ink.set("box", fns.0)?;
    runts_ink.set("text", fns.1)?;
    runts_ink.set("newline", fns.2)?;
    runts_ink.set("spacer", fns.3)?;
    runts_ink.set("render_to_string", fns.4)?;
    Ok(())
}

fn make_bridge_fns<'js>(
    ctx: Ctx<'js>,
) -> JsResult<(Function<'js>, Function<'js>, Function<'js>, Function<'js>, Function<'js>)> {
    Ok((
        make_box_fn(ctx.clone())?,
        make_text_fn(ctx.clone())?,
        make_newline_fn(ctx.clone())?,
        make_spacer_fn(ctx.clone())?,
        make_render_fn(ctx.clone())?,
    ))
}

fn make_box_fn<'js>(ctx: Ctx<'js>) -> JsResult<Function<'js>> {
    Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, props: Object<'js>| -> JsResult<Value<'js>> {
            let mut b = InkBox::new();
            box_props::apply_box_props(&props, &mut b);
            if let Ok(children_v) = props.get::<_, Value>("children") {
                box_add_children(&ctx, &props, children_v, &mut b)?;
            }
            vnode_to_js(&ctx, &VNode::from(b))
        },
    )
}

fn box_add_children<'js>(
    ctx: &Ctx<'js>,
    _props: &Object<'js>,
    children_v: Value<'js>,
    b: &mut InkBox,
) -> JsResult<()> {
    let child_count: Option<usize> = children_v
        .as_array()
        .map(|a| a.len())
        .or_else(|| children_v.as_object().map(|o| o.len()));
    if let Some(len) = child_count {
        for i in 0..len {
            let key = i.to_string();
            let child_val = if let Some(arr) = children_v.as_array() {
                arr.get(i)
            } else if let Some(obj) = children_v.as_object() {
                obj.get::<_, Value>(key.as_str())
            } else {
                continue;
            };
            if let Ok(child) = child_val {
                if let Ok(c) = vnode_from_js(ctx, &child) {
                    *b = b.clone().child(c);
                }
            }
        }
    }
    Ok(())
}

fn make_text_fn<'js>(ctx: Ctx<'js>) -> JsResult<Function<'js>> {
    Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, content: Value<'js>, props: Object<'js>| -> JsResult<Value<'js>> {
            let content_str = text_props::extract_string_content(content);
            let mut t = InkText::new(content_str);
            text_props::apply_text_props(&props, &mut t);
            vnode_to_js(&ctx, &VNode::from(t))
        },
    )
}

fn make_newline_fn<'js>(ctx: Ctx<'js>) -> JsResult<Function<'js>> {
    Function::new(ctx.clone(), |ctx: Ctx<'js>| -> JsResult<Value<'js>> {
        vnode_to_js(&ctx, &VNode::from(Newline::new()))
    })
}

fn make_spacer_fn<'js>(ctx: Ctx<'js>) -> JsResult<Function<'js>> {
    Function::new(ctx.clone(), |ctx: Ctx<'js>| -> JsResult<Value<'js>> {
        vnode_to_js(&ctx, &VNode::from(Spacer::new()))
    })
}

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
