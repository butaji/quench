//! Box prop setters (JS -> Rust) and serializers
//! (Rust -> JS) for the rquickjs bridge.

use crate::{
    components::{AlignContent, AlignItems, AlignSelf, Box as InkBox, FlexWrap, JustifyContent},
    js_bridge::parsers::*,
    style::{BorderStyle, Display, Overflow, Position},
};
use rquickjs::{Object, Result as JsResult, Value};

/* -------------------------------------------------------------------------- */
/* Helpers                                                                    */
/* -------------------------------------------------------------------------- */

fn apply_prop(props: &Object<'_>, name: &str, b: &mut InkBox, setter: fn(&Value, &mut InkBox)) {
    match props.get::<_, Value>(name) {
        Ok(v) if !v.is_undefined() && !v.is_null() => {
            setter(&v, b);
        }
        _ => {}
    }
}

fn str_prop(v: &Value) -> Option<String> {
    to_string(v)
}

/* -------------------------------------------------------------------------- */
/* Individual setters                                                         */
/* -------------------------------------------------------------------------- */

fn set_flex_dir(v: &Value, b: &mut InkBox) {
    if let Some(s) = str_prop(v) { b.flex_direction = parse_flex_dir(&s); }
}
fn set_flex_wrap(v: &Value, b: &mut InkBox) {
    if let Some(s) = str_prop(v) { b.flex_wrap = parse_flex_wrap(&s); }
}
fn set_flex_grow(v: &Value, b: &mut InkBox) { b.flex_grow = to_f32(v); }
fn set_flex_shrink(v: &Value, b: &mut InkBox) { b.flex_shrink = to_f32(v); }
fn set_flex_basis(v: &Value, b: &mut InkBox) { b.flex_basis_pct = to_f32(v); }
fn set_justify(v: &Value, b: &mut InkBox) {
    if let Some(s) = str_prop(v) { b.justify_content = parse_justify(&s); }
}
fn set_align_items(v: &Value, b: &mut InkBox) {
    if let Some(s) = str_prop(v) { b.align_items = parse_align_items(&s); }
}
fn set_align_self(v: &Value, b: &mut InkBox) {
    if let Some(s) = str_prop(v) { b.align_self = parse_align_self(&s); }
}
fn set_align_content(v: &Value, b: &mut InkBox) {
    if let Some(s) = str_prop(v) { b.align_content = parse_align_content(&s); }
}
fn set_width(v: &Value, b: &mut InkBox) { b.width = Some(to_u16(v)); }
fn set_height(v: &Value, b: &mut InkBox) { b.height = Some(to_u16(v)); }
fn set_min_width(v: &Value, b: &mut InkBox) { b.min_width = Some(to_u16(v)); }
fn set_min_height(v: &Value, b: &mut InkBox) { b.min_height = Some(to_u16(v)); }
fn set_max_width(v: &Value, b: &mut InkBox) { b.max_width = Some(to_u16(v)); }
fn set_max_height(v: &Value, b: &mut InkBox) { b.max_height = Some(to_u16(v)); }
fn set_padding(v: &Value, b: &mut InkBox) {
    let p = to_u16(v);
    b.padding_top = Some(p);
    b.padding_right = Some(p);
    b.padding_bottom = Some(p);
    b.padding_left = Some(p);
}
fn set_padding_x(v: &Value, b: &mut InkBox) {
    let p = to_u16(v);
    b.padding_left = Some(p);
    b.padding_right = Some(p);
}
fn set_padding_y(v: &Value, b: &mut InkBox) {
    let p = to_u16(v);
    b.padding_top = Some(p);
    b.padding_bottom = Some(p);
}
fn set_padding_top(v: &Value, b: &mut InkBox) { b.padding_top = Some(to_u16(v)); }
fn set_padding_right(v: &Value, b: &mut InkBox) { b.padding_right = Some(to_u16(v)); }
fn set_padding_bottom(v: &Value, b: &mut InkBox) { b.padding_bottom = Some(to_u16(v)); }
fn set_padding_left(v: &Value, b: &mut InkBox) { b.padding_left = Some(to_u16(v)); }
fn set_margin(v: &Value, b: &mut InkBox) {
    let m = to_u16(v);
    b.margin_top = Some(m);
    b.margin_right = Some(m);
    b.margin_bottom = Some(m);
    b.margin_left = Some(m);
}
fn set_margin_top(v: &Value, b: &mut InkBox) { b.margin_top = Some(to_u16(v)); }
fn set_margin_right(v: &Value, b: &mut InkBox) { b.margin_right = Some(to_u16(v)); }
fn set_margin_bottom(v: &Value, b: &mut InkBox) { b.margin_bottom = Some(to_u16(v)); }
fn set_margin_left(v: &Value, b: &mut InkBox) { b.margin_left = Some(to_u16(v)); }
fn set_margin_x(v: &Value, b: &mut InkBox) {
    let m = to_u16(v);
    b.margin_left = Some(m);
    b.margin_right = Some(m);
}
fn set_margin_y(v: &Value, b: &mut InkBox) {
    let m = to_u16(v);
    b.margin_top = Some(m);
    b.margin_bottom = Some(m);
}
fn set_gap(v: &Value, b: &mut InkBox) {
    let g = to_u16(v);
    b.column_gap = Some(g);
    b.row_gap = Some(g);
}
fn set_column_gap(v: &Value, b: &mut InkBox) { b.column_gap = Some(to_u16(v)); }
fn set_row_gap(v: &Value, b: &mut InkBox) { b.row_gap = Some(to_u16(v)); }
fn set_position(v: &Value, b: &mut InkBox) {
    if let Some(s) = str_prop(v) { b.position = parse_position(&s); }
}
fn set_top(v: &Value, b: &mut InkBox) { b.top = Some(to_u16(v)); }
fn set_right(v: &Value, b: &mut InkBox) { b.right = Some(to_u16(v)); }
fn set_bottom(v: &Value, b: &mut InkBox) { b.bottom = Some(to_u16(v)); }
fn set_left(v: &Value, b: &mut InkBox) { b.left = Some(to_u16(v)); }
fn set_display(v: &Value, b: &mut InkBox) {
    if let Some(s) = str_prop(v) { b.display = parse_display(&s); }
}
fn set_overflow_x(v: &Value, b: &mut InkBox) {
    if let Some(s) = str_prop(v) { b.overflow_x = parse_overflow(&s); }
}
fn set_overflow_y(v: &Value, b: &mut InkBox) {
    if let Some(s) = str_prop(v) { b.overflow_y = parse_overflow(&s); }
}
fn set_overflow(v: &Value, b: &mut InkBox) {
    if let Some(s) = str_prop(v) {
        let o = parse_overflow(&s);
        b.overflow_x = o;
        b.overflow_y = o;
    }
}
fn set_border_style(v: &Value, b: &mut InkBox) {
    if let Some(s) = str_prop(v) {
        b.border_style = parse_border_style(&s);
        if !b.borders.top && !b.borders.right && !b.borders.bottom && !b.borders.left {
            b.borders = crate::style::Borders::ALL;
        }
    }
}
fn set_border_top(v: &Value, b: &mut InkBox) { b.borders.top = to_bool(v); }
fn set_border_right(v: &Value, b: &mut InkBox) { b.borders.right = to_bool(v); }
fn set_border_bottom(v: &Value, b: &mut InkBox) { b.borders.bottom = to_bool(v); }
fn set_border_left(v: &Value, b: &mut InkBox) { b.borders.left = to_bool(v); }
fn set_border_color(v: &Value, b: &mut InkBox) {
    if let Some(s) = str_prop(v) { b.border_color = Some(parse_color(&s)); }
}
fn set_border_dim_color(v: &Value, b: &mut InkBox) { b.border_dim_color = to_bool(v); }
fn set_border_background_color(v: &Value, b: &mut InkBox) {
    if let Some(s) = str_prop(v) { b.border_background_color = Some(parse_color(&s)); }
}
fn set_background_color(v: &Value, b: &mut InkBox) {
    if let Some(s) = str_prop(v) { b.background_color = Some(parse_color(&s)); }
}
fn set_z_index(v: &Value, b: &mut InkBox) { b.z_index = to_f32(v) as i16; }

/* -------------------------------------------------------------------------- */
/* Apply all box props from a JS object                                       */
/* -------------------------------------------------------------------------- */

pub fn apply_box_props(props: &Object<'_>, b: &mut InkBox) {
    apply_flex_props(props, b);
    apply_size_props(props, b);
    apply_spacing_props(props, b);
    apply_align_props(props, b);
    apply_position_props(props, b);
    apply_border_props(props, b);
    apply_misc_props(props, b);
}

fn apply_flex_props(props: &Object<'_>, b: &mut InkBox) {
    apply_prop(props, "flexDirection", b, set_flex_dir);
    apply_prop(props, "flexWrap", b, set_flex_wrap);
    apply_prop(props, "flexGrow", b, set_flex_grow);
    apply_prop(props, "flexShrink", b, set_flex_shrink);
    apply_prop(props, "flexBasis", b, set_flex_basis);
}

fn apply_size_props(props: &Object<'_>, b: &mut InkBox) {
    apply_prop(props, "width", b, set_width);
    apply_prop(props, "height", b, set_height);
    apply_prop(props, "minWidth", b, set_min_width);
    apply_prop(props, "minHeight", b, set_min_height);
    apply_prop(props, "maxWidth", b, set_max_width);
    apply_prop(props, "maxHeight", b, set_max_height);
}

fn apply_spacing_props(props: &Object<'_>, b: &mut InkBox) {
    apply_prop(props, "padding", b, set_padding);
    apply_prop(props, "paddingX", b, set_padding_x);
    apply_prop(props, "paddingY", b, set_padding_y);
    apply_prop(props, "paddingTop", b, set_padding_top);
    apply_prop(props, "paddingRight", b, set_padding_right);
    apply_prop(props, "paddingBottom", b, set_padding_bottom);
    apply_prop(props, "paddingLeft", b, set_padding_left);
    apply_prop(props, "margin", b, set_margin);
    apply_prop(props, "marginTop", b, set_margin_top);
    apply_prop(props, "marginRight", b, set_margin_right);
    apply_prop(props, "marginBottom", b, set_margin_bottom);
    apply_prop(props, "marginLeft", b, set_margin_left);
    apply_prop(props, "marginX", b, set_margin_x);
    apply_prop(props, "marginY", b, set_margin_y);
    apply_prop(props, "gap", b, set_gap);
    apply_prop(props, "columnGap", b, set_column_gap);
    apply_prop(props, "rowGap", b, set_row_gap);
}

fn apply_align_props(props: &Object<'_>, b: &mut InkBox) {
    apply_prop(props, "justifyContent", b, set_justify);
    apply_prop(props, "alignItems", b, set_align_items);
    apply_prop(props, "alignSelf", b, set_align_self);
    apply_prop(props, "alignContent", b, set_align_content);
}

fn apply_position_props(props: &Object<'_>, b: &mut InkBox) {
    apply_prop(props, "position", b, set_position);
    apply_prop(props, "top", b, set_top);
    apply_prop(props, "right", b, set_right);
    apply_prop(props, "bottom", b, set_bottom);
    apply_prop(props, "left", b, set_left);
    apply_prop(props, "display", b, set_display);
    apply_prop(props, "overflowX", b, set_overflow_x);
    apply_prop(props, "overflowY", b, set_overflow_y);
    apply_prop(props, "overflow", b, set_overflow);
}

fn apply_border_props(props: &Object<'_>, b: &mut InkBox) {
    apply_prop(props, "borderStyle", b, set_border_style);
    apply_prop(props, "borderTop", b, set_border_top);
    apply_prop(props, "borderRight", b, set_border_right);
    apply_prop(props, "borderBottom", b, set_border_bottom);
    apply_prop(props, "borderLeft", b, set_border_left);
    apply_prop(props, "borderColor", b, set_border_color);
    apply_prop(props, "borderDimColor", b, set_border_dim_color);
    apply_prop(props, "borderBackgroundColor", b, set_border_background_color);
}

fn apply_misc_props(props: &Object<'_>, b: &mut InkBox) {
    apply_prop(props, "backgroundColor", b, set_background_color);
    apply_prop(props, "zIndex", b, set_z_index);
}

/* -------------------------------------------------------------------------- */
/* Serialization (Rust -> JS)                                                  */
/* -------------------------------------------------------------------------- */

pub fn serialize_box_props<'js>(props: &Object<'js>, b: &InkBox) -> JsResult<()> {
    serialize_flex_props(props, b)?;
    serialize_size_props(props, b)?;
    serialize_spacing_props(props, b)?;
    serialize_align_props(props, b)?;
    serialize_position_props(props, b)?;
    serialize_border_props(props, b)?;
    serialize_misc_props(props, b)?;
    Ok(())
}

fn serialize_flex_props<'js>(props: &Object<'js>, b: &InkBox) -> JsResult<()> {
    props.set("flexDirection", flex_dir_name(b))?;
    props.set("flexWrap", flex_wrap_name(b))?;
    props.set("flexGrow", b.flex_grow)?;
    props.set("flexShrink", b.flex_shrink)?;
    props.set("flexBasis", b.flex_basis_pct)?;
    Ok(())
}

fn serialize_size_props<'js>(props: &Object<'js>, b: &InkBox) -> JsResult<()> {
    set_opt_u16(props, "width", b.width)?;
    set_opt_u16(props, "height", b.height)?;
    set_opt_u16(props, "minWidth", b.min_width)?;
    set_opt_u16(props, "minHeight", b.min_height)?;
    set_opt_u16(props, "maxWidth", b.max_width)?;
    set_opt_u16(props, "maxHeight", b.max_height)?;
    Ok(())
}

fn serialize_spacing_props<'js>(props: &Object<'js>, b: &InkBox) -> JsResult<()> {
    serialize_padding_margin(props, b)?;
    serialize_gaps(props, b)?;
    Ok(())
}

fn serialize_padding_margin<'js>(props: &Object<'js>, b: &InkBox) -> JsResult<()> {
    set_opt_u16(props, "paddingTop", b.padding_top)?;
    set_opt_u16(props, "paddingRight", b.padding_right)?;
    set_opt_u16(props, "paddingBottom", b.padding_bottom)?;
    set_opt_u16(props, "paddingLeft", b.padding_left)?;
    set_opt_u16(props, "marginTop", b.margin_top)?;
    set_opt_u16(props, "marginRight", b.margin_right)?;
    set_opt_u16(props, "marginBottom", b.margin_bottom)?;
    set_opt_u16(props, "marginLeft", b.margin_left)?;
    Ok(())
}

fn serialize_gaps<'js>(props: &Object<'js>, b: &InkBox) -> JsResult<()> {
    set_opt_u16(props, "columnGap", b.column_gap)?;
    set_opt_u16(props, "rowGap", b.row_gap)?;
    Ok(())
}

fn serialize_align_props<'js>(props: &Object<'js>, b: &InkBox) -> JsResult<()> {
    props.set("justifyContent", justify_name(b))?;
    props.set("alignItems", align_name(b))?;
    props.set("alignSelf", align_self_name(b))?;
    props.set("alignContent", align_content_name(b))?;
    Ok(())
}

fn serialize_position_props<'js>(props: &Object<'js>, b: &InkBox) -> JsResult<()> {
    props.set("position", position_name(b))?;
    set_opt_u16(props, "top", b.top)?;
    set_opt_u16(props, "right", b.right)?;
    set_opt_u16(props, "bottom", b.bottom)?;
    set_opt_u16(props, "left", b.left)?;
    props.set("display", display_name(b))?;
    props.set("overflowX", overflow_name(&b.overflow_x))?;
    props.set("overflowY", overflow_name(&b.overflow_y))?;
    Ok(())
}

fn serialize_border_props<'js>(props: &Object<'js>, b: &InkBox) -> JsResult<()> {
    if has_any_border(&b.borders) {
        props.set("borderStyle", border_style_name(b.border_style))?;
    }
    props.set("borderTop", b.borders.top)?;
    props.set("borderRight", b.borders.right)?;
    props.set("borderBottom", b.borders.bottom)?;
    props.set("borderLeft", b.borders.left)?;
    set_opt_color(props, "borderColor", &b.border_color)?;
    props.set("borderDimColor", b.border_dim_color)?;
    set_opt_color(props, "borderBackgroundColor", &b.border_background_color)?;
    Ok(())
}

fn serialize_misc_props<'js>(props: &Object<'js>, b: &InkBox) -> JsResult<()> {
    set_opt_color(props, "backgroundColor", &b.background_color)?;
    props.set("zIndex", b.z_index)?;
    Ok(())
}

fn set_opt_u16<'js>(props: &Object<'js>, name: &str, val: Option<u16>) -> JsResult<()> {
    if let Some(v) = val { props.set(name, v)?; }
    Ok(())
}

fn set_opt_color<'js>(props: &Object<'js>, name: &str, val: &Option<crate::components::Color>) -> JsResult<()> {
    if let Some(c) = val { props.set(name, color_name(c))?; }
    Ok(())
}

fn flex_dir_name(b: &InkBox) -> &'static str {
    match b.flex_direction {
        crate::components::FlexDirection::Row => "row",
        crate::components::FlexDirection::Column => "column",
        crate::components::FlexDirection::RowReverse => "row-reverse",
        crate::components::FlexDirection::ColumnReverse => "column-reverse",
    }
}

fn flex_wrap_name(b: &InkBox) -> &'static str {
    match b.flex_wrap {
        FlexWrap::NoWrap => "nowrap",
        FlexWrap::Wrap => "wrap",
        FlexWrap::WrapReverse => "wrap-reverse",
    }
}

fn justify_name(b: &InkBox) -> &'static str {
    match b.justify_content {
        JustifyContent::FlexStart => "flex-start",
        JustifyContent::FlexEnd => "flex-end",
        JustifyContent::Center => "center",
        JustifyContent::SpaceBetween => "space-between",
        JustifyContent::SpaceAround => "space-around",
        JustifyContent::SpaceEvenly => "space-evenly",
    }
}

fn align_name(b: &InkBox) -> &'static str {
    match b.align_items {
        AlignItems::FlexStart => "flex-start",
        AlignItems::FlexEnd => "flex-end",
        AlignItems::Center => "center",
        AlignItems::Stretch => "stretch",
        AlignItems::Baseline => "baseline",
    }
}

fn align_self_name(b: &InkBox) -> &'static str {
    match b.align_self {
        AlignSelf::Auto => "auto",
        AlignSelf::FlexStart => "flex-start",
        AlignSelf::Center => "center",
        AlignSelf::FlexEnd => "flex-end",
        AlignSelf::Stretch => "stretch",
        AlignSelf::Baseline => "baseline",
    }
}

fn align_content_name(b: &InkBox) -> &'static str {
    match b.align_content {
        AlignContent::FlexStart => "flex-start",
        AlignContent::Center => "center",
        AlignContent::FlexEnd => "flex-end",
        AlignContent::Stretch => "stretch",
        AlignContent::SpaceBetween => "space-between",
        AlignContent::SpaceAround => "space-around",
    }
}

fn position_name(b: &InkBox) -> &'static str {
    match b.position {
        Position::Relative => "relative",
        Position::Absolute => "absolute",
    }
}

fn display_name(b: &InkBox) -> &'static str {
    match b.display {
        Display::Flex => "flex",
        Display::None => "none",
    }
}

fn overflow_name(o: &Overflow) -> &'static str {
    match o {
        Overflow::Visible => "visible",
        Overflow::Hidden => "hidden",
    }
}

fn has_any_border(b: &crate::style::Borders) -> bool {
    b.top || b.right || b.bottom || b.left
}

fn border_style_name(style: BorderStyle) -> &'static str {
    match style {
        BorderStyle::Single => "single",
        BorderStyle::Double => "double",
        BorderStyle::Round => "round",
        BorderStyle::Bold => "bold",
        BorderStyle::Classic => "classic",
    }
}

fn color_name(c: &crate::components::Color) -> String {
    serde_json::to_string(c)
        .unwrap_or_default()
        .trim_matches('"')
        .to_string()
}
