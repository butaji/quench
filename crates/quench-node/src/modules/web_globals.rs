use quench_runtime::{execute::VmError, value::Value};
const SOURCE: &str = include_str!("web_globals.js");
pub fn build() -> Result<Value, VmError> {
    let p = quench_runtime::reduce::reduce_global_script_source(SOURCE)
        .map_err(|e| VmError::EvalError(e.join("; ")))?;
    let c = quench_runtime::vm::current_context();
    let mut r = Vec::new();
    let f = quench_runtime::vm::with_current_context(&c, || {
        quench_runtime::vm::execute_in_place_context(p.ops(), &mut r, &c)
    })?;
    quench_runtime::vm::call_value(&f, &Value::Undefined, &[])
}
