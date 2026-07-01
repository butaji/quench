// linter-skip
#![allow(clippy::too_many_lines, clippy::function_body_length)]
//! Math built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_number, NativeFunction, Object, Value};
use crate::Context;

// ============================================================================
// Math
// ============================================================================

fn rand_simple() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos();
    (nanos as f64) / (u32::MAX as f64)
}

pub fn register_math(ctx: &mut Context) {
    let math = Object::new(crate::value::ObjectKind::Ordinary);
    let math = Rc::new(RefCell::new(math));

    macro_rules! math_fn {
        ($name:expr, $fn:expr) => {
            math.borrow_mut().set($name, Value::NativeFunction(Rc::new(NativeFunction::new(move |args| {
                let x = args.first().map(to_number).unwrap_or(0.0);
                Ok(Value::Number($fn(x)))
            }))));
        };
    }

    math_fn!("abs", f64::abs);
    math_fn!("floor", f64::floor);
    math_fn!("ceil", f64::ceil);
    math_fn!("round", f64::round);
    math_fn!("sqrt", f64::sqrt);
    math.borrow_mut().set("pow", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let base = args.first().map(to_number).unwrap_or(0.0);
        let exp = args.get(1).map(to_number).unwrap_or(1.0);
        Ok(Value::Number(base.powf(exp)))
    }))));

    math.borrow_mut().set("max", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let max = args.iter().map(to_number).fold(f64::NEG_INFINITY, f64::max);
        Ok(Value::Number(max))
    }))));

    math.borrow_mut().set("min", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let min = args.iter().map(to_number).fold(f64::INFINITY, f64::min);
        Ok(Value::Number(min))
    }))));

    math.borrow_mut().set("PI", Value::Number(std::f64::consts::PI));
    math.borrow_mut().set("E", Value::Number(std::f64::consts::E));
    math.borrow_mut().set("random", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::Number(rand_simple()))
    }))));

    ctx.set_global("Math".to_string(), Value::Object(math));
}
