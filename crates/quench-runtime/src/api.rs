//! Public embedding API for driving a Quench JavaScript context.

use crate::{Context, JsError, Value};

pub fn strict_mode() -> bool {
    crate::interpreter::is_strict_mode()
}

pub fn set_strict_mode(strict: bool) {
    crate::interpreter::set_strict_mode(strict);
}

pub fn direct_eval() -> bool {
    crate::interpreter::is_direct_eval()
}

pub fn set_direct_eval(direct: bool) {
    crate::interpreter::set_direct_eval(direct);
}

pub fn native_this() -> Option<Value> {
    crate::interpreter::get_native_this()
}

/// Result returned by runtime operations.
pub type JsResult<T> = Result<T, JsError>;
/// Host callback exposed as a JavaScript function value.
pub type HostCallback = Box<dyn Fn(Vec<Value>) -> JsResult<Value>>;

/// Operations required by a host-facing JavaScript runtime adapter.
pub trait QuenchRuntime {
    type Context;
    type Value;

    fn new_context(&mut self) -> JsResult<Self::Context>;
    fn eval(&mut self, ctx: &mut Self::Context, source: &str) -> JsResult<Self::Value>;
    fn eval_script(
        &mut self,
        ctx: &mut Self::Context,
        source: &str,
        strict: bool,
    ) -> JsResult<Self::Value>;
    fn eval_module(&mut self, ctx: &mut Self::Context, source: &str) -> JsResult<Self::Value>;
    fn global(&mut self, ctx: &mut Self::Context) -> Self::Value;
    fn get(
        &mut self,
        ctx: &mut Self::Context,
        target: &Self::Value,
        key: &Self::Value,
    ) -> JsResult<Self::Value>;
    fn set(
        &mut self,
        ctx: &mut Self::Context,
        target: &Self::Value,
        key: Self::Value,
        value: Self::Value,
    ) -> JsResult<bool>;
    fn call(
        &mut self,
        ctx: &mut Self::Context,
        function: &Self::Value,
        this: Self::Value,
        args: &[Self::Value],
    ) -> JsResult<Self::Value>;
    fn host_function(
        &mut self,
        ctx: &mut Self::Context,
        callback: HostCallback,
    ) -> JsResult<Self::Value>;
}

#[derive(Default)]
/// The built-in Quench adapter backed by [`Context`].
pub struct DefaultQuenchRuntime;

impl QuenchRuntime for DefaultQuenchRuntime {
    type Context = Context;
    type Value = Value;

    fn new_context(&mut self) -> JsResult<Self::Context> {
        Context::new()
    }
    fn eval(&mut self, ctx: &mut Self::Context, source: &str) -> JsResult<Self::Value> {
        ctx.eval(source)
    }
    fn eval_script(
        &mut self,
        ctx: &mut Self::Context,
        source: &str,
        strict: bool,
    ) -> JsResult<Self::Value> {
        ctx.eval_script(source, strict)
    }
    fn eval_module(&mut self, ctx: &mut Self::Context, source: &str) -> JsResult<Self::Value> {
        ctx.eval_es_module(source)
    }
    fn global(&mut self, ctx: &mut Self::Context) -> Self::Value {
        ctx.get_global("globalThis").unwrap_or(Value::Undefined)
    }
    fn get(
        &mut self,
        _ctx: &mut Self::Context,
        target: &Self::Value,
        key: &Self::Value,
    ) -> JsResult<Self::Value> {
        match target {
            Value::Object(object) => match key {
                Value::String(name) => object
                    .borrow()
                    .get(name)
                    .ok_or_else(|| JsError("property not found".into())),
                _ => object
                    .borrow()
                    .get_property(key)
                    .ok_or_else(|| JsError("property not found".into())),
            },
            _ => Err(JsError("target is not an object".into())),
        }
    }
    fn set(
        &mut self,
        _ctx: &mut Self::Context,
        target: &Self::Value,
        key: Self::Value,
        value: Self::Value,
    ) -> JsResult<bool> {
        match target {
            Value::Object(object) => {
                match key {
                    Value::String(name) => object.borrow_mut().set(&name, value),
                    Value::Symbol(symbol) => object
                        .borrow_mut()
                        .set_symbol(&symbol.property_key(), value),
                    _ => return Err(JsError("property key is not coercible".into())),
                }
                Ok(true)
            }
            _ => Err(JsError("target is not an object".into())),
        }
    }
    fn call(
        &mut self,
        ctx: &mut Self::Context,
        function: &Self::Value,
        this: Self::Value,
        args: &[Self::Value],
    ) -> JsResult<Self::Value> {
        let _ = ctx;
        crate::eval::function::call_value_with_this(function.clone(), args.to_vec(), this)
    }
    fn host_function(
        &mut self,
        _ctx: &mut Self::Context,
        callback: HostCallback,
    ) -> JsResult<Self::Value> {
        Ok(Value::NativeFunction(std::rc::Rc::new(
            crate::value::NativeFunction::new(callback),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::{DefaultQuenchRuntime, QuenchRuntime};
    use crate::Value;

    #[test]
    fn runtime_api_evaluates_and_reads_global_property() {
        let mut engine = DefaultQuenchRuntime;
        let mut ctx = engine.new_context().unwrap();
        engine.eval(&mut ctx, "globalThis.answer = 42").unwrap();
        let global = engine.global(&mut ctx);
        let key = Value::String("answer".into());
        assert_eq!(
            engine.get(&mut ctx, &global, &key).unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn runtime_api_evaluates_modules() {
        let mut engine = DefaultQuenchRuntime;
        let mut ctx = engine.new_context().unwrap();
        assert!(engine
            .eval_module(&mut ctx, "export const answer = 42;")
            .is_ok());
    }

    #[test]
    fn runtime_api_evaluates_strict_scripts() {
        let mut engine = DefaultQuenchRuntime;
        let mut ctx = engine.new_context().unwrap();
        assert!(engine.eval_script(&mut ctx, "with ({}) {}", false).is_ok());
        assert!(engine.eval_script(&mut ctx, "with ({}) {}", true).is_err());
    }
}
