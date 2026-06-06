//! Text prop setters (JS -> Rust) and serializers
//! (Rust -> JS) for the rquickjs bridge.

use crate::{
    components::Text as InkText,
    js_bridge::parsers::*,
};
use rquickjs::{Object, Result as JsResult, Value};

/* -------------------------------------------------------------------------- */
/* Apply text props from a JS object                                          */
/* -------------------------------------------------------------------------- */

pub fn apply_text_props(props: &Object<'_>, t: &mut InkText) {
    apply_text_bool_props(props, t);
    apply_text_color_props(props, t);
    apply_text_wrap(props, t);
}

fn apply_text_bool_props(props: &Object<'_>, t: &mut InkText) {
    if let Ok(v) = props.get::<_, bool>("bold") { t.bold = v; }
    if let Ok(v) = props.get::<_, bool>("italic") { t.italic = v; }
    if let Ok(v) = props.get::<_, bool>("underline") { t.underline = v; }
    if let Ok(v) = props.get::<_, bool>("strikethrough") { t.strikethrough = v; }
    if let Ok(v) = props.get::<_, bool>("dimColor") { t.dim_color = v; }
    if let Ok(v) = props.get::<_, bool>("inverse") { t.inverse = v; }
}

fn apply_text_color_props(props: &Object<'_>, t: &mut InkText) {
    if let Ok(s) = props.get::<_, String>("color") {
        if !s.is_empty() { t.color = parse_color(&s); }
    }
    if let Ok(s) = props.get::<_, String>("bgColor") {
        if !s.is_empty() { t.background_color = parse_color(&s); }
    }
    if let Ok(s) = props.get::<_, String>("backgroundColor") {
        if !s.is_empty() { t.background_color = parse_color(&s); }
    }
}

fn apply_text_wrap(props: &Object<'_>, t: &mut InkText) {
    if let Ok(s) = props.get::<_, String>("wrap") {
        t.wrap = parse_wrap(&s);
    }
}

/* -------------------------------------------------------------------------- */
/* Serialization (Rust -> JS)                                                  */
/* -------------------------------------------------------------------------- */

pub fn serialize_text_props<'js>(props: &Object<'js>, t: &InkText) -> JsResult<()> {
    serialize_text_bools(props, t)?;
    serialize_text_colors(props, t)?;
    serialize_text_wrap(props, t)?;
    Ok(())
}

fn serialize_text_bools<'js>(props: &Object<'js>, t: &InkText) -> JsResult<()> {
    serialize_text_bools_a(props, t)?;
    serialize_text_bools_b(props, t)?;
    Ok(())
}

fn serialize_text_bools_a<'js>(props: &Object<'js>, t: &InkText) -> JsResult<()> {
    if t.bold { props.set("bold", true)?; }
    if t.italic { props.set("italic", true)?; }
    if t.underline { props.set("underline", true)?; }
    Ok(())
}

fn serialize_text_bools_b<'js>(props: &Object<'js>, t: &InkText) -> JsResult<()> {
    if t.strikethrough { props.set("strikethrough", true)?; }
    if t.dim_color { props.set("dimColor", true)?; }
    if t.inverse { props.set("inverse", true)?; }
    Ok(())
}

fn serialize_text_colors<'js>(props: &Object<'js>, t: &InkText) -> JsResult<()> {
    if t.color != crate::components::Color::Default {
        props.set("color", color_name(&t.color))?;
    }
    if t.background_color != crate::components::Color::Default {
        props.set("backgroundColor", color_name(&t.background_color))?;
    }
    Ok(())
}

fn serialize_text_wrap<'js>(props: &Object<'js>, t: &InkText) -> JsResult<()> {
    if t.wrap != crate::style::Wrap::Wrap {
        props.set("wrap", wrap_name(&t.wrap))?;
    }
    Ok(())
}

pub fn extract_string_content(content: Value<'_>) -> String {
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

fn color_name(c: &crate::components::Color) -> String {
    serde_json::to_string(c)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}

fn wrap_name(w: &crate::style::Wrap) -> &'static str {
    match w {
        crate::style::Wrap::Wrap => "wrap",
        crate::style::Wrap::Hard => "hard",
        crate::style::Wrap::Truncate => "truncate",
        crate::style::Wrap::TruncateMiddle => "truncate-middle",
    }
}
