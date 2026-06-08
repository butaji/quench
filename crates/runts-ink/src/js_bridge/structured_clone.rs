//! structuredClone() implementation for rquickjs bridge
//!
//! Provides structured cloning for deep copying objects.

use rquickjs::{Ctx, Function, Object, Result as JsResult};

/// Install structuredClone into the given rquickjs Context.
pub fn install<'js>(ctx: &Ctx<'js>) -> JsResult<()> {
    let globals = ctx.globals();
    let f = Function::new(ctx.clone(), |ctx: Ctx<'js>, value: rquickjs::Value<'js>| -> JsResult<rquickjs::Value<'js>> {
        // Try to use JSON global for deep cloning
        let json_obj: Object = ctx.globals().get("JSON")?;
        let stringify: Function = json_obj.get("stringify")?;
        let parse: Function = json_obj.get("parse")?;
        
        let json_str: String = stringify.call((value.clone(),))?;
        let cloned: rquickjs::Value = parse.call((json_str,))?;
        Ok(cloned)
    })?;
    globals.set("structuredClone", f)
}
