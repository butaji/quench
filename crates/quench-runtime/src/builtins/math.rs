//! Math built-in

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{to_number, NativeFunction, Object, Value};
use crate::Context;

fn rand_simple() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos();
    (nanos as f64) / (u32::MAX as f64)
}

pub fn register_math(ctx: &mut Context) {
    let math = Object::new(crate::value::ObjectKind::Ordinary);
    let math = Rc::new(RefCell::new(math));

    register_unary_math_fns(&math);
    register_binary_math_fns(&math);
    register_reduce_math_fns(&math);
    register_math_constants(&math);

    ctx.set_global("Math".to_string(), Value::Object(math));
}

fn register_unary_math_fns(math: &Rc<RefCell<Object>>) {
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
    math_fn!("sin", f64::sin);
    math_fn!("cos", f64::cos);
    math_fn!("tan", f64::tan);
    math_fn!("asin", f64::asin);
    math_fn!("acos", f64::acos);
    math_fn!("atan", f64::atan);
    math_fn!("log", f64::ln);
    math_fn!("log10", f64::log10);
    math_fn!("log2", f64::log2);
    math_fn!("exp", f64::exp);
    math_fn!("log1p", f64::ln_1p);
}

fn register_binary_math_fns(math: &Rc<RefCell<Object>>) {
    math.borrow_mut().set("atan2", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let y = args.first().map(to_number).unwrap_or(0.0);
        let x = args.get(1).map(to_number).unwrap_or(0.0);
        Ok(Value::Number(y.atan2(x)))
    }))));
    math.borrow_mut().set("pow", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let base = args.first().map(to_number).unwrap_or(0.0);
        let exp = args.get(1).map(to_number).unwrap_or(1.0);
        Ok(Value::Number(base.powf(exp)))
    }))));
}

fn register_reduce_math_fns(math: &Rc<RefCell<Object>>) {
    math.borrow_mut().set("max", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let max = args.iter().map(to_number).fold(f64::NEG_INFINITY, f64::max);
        Ok(Value::Number(max))
    }))));
    math.borrow_mut().set("min", Value::NativeFunction(Rc::new(NativeFunction::new(|args| {
        let min = args.iter().map(to_number).fold(f64::INFINITY, f64::min);
        Ok(Value::Number(min))
    }))));
    math.borrow_mut().set("random", Value::NativeFunction(Rc::new(NativeFunction::new(|_args| {
        Ok(Value::Number(rand_simple()))
    }))));
}

fn register_math_constants(math: &Rc<RefCell<Object>>) {
    math.borrow_mut().set("PI", Value::Number(std::f64::consts::PI));
    math.borrow_mut().set("E", Value::Number(std::f64::consts::E));
}
