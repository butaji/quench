//! `vm` module — minimal `runInNewContext`/`runInContext` that evaluate
//! source text as a classic script through the runtime's reducer.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn run_in_new_context(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))?;
    let program = quench_runtime::reduce::reduce_global_script_source(&source)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let mut context = quench_runtime::vm::current_context();
    if let Some(sandbox @ Value::Object(_)) = args.get(1) {
        for key in execute::own_enumerable_keys(sandbox) {
            let value = execute::get_property_result(sandbox, &key)?;
            context = context.with_host_value(key, value);
        }
    }
    let mut registers = Vec::new();
    quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::execute_in_place_context(program.ops(), &mut registers, &context)
    })
}

pub fn build() -> Value {
    let run = crate::host::capability(crate::registry::SPEC_VM_RUN_IN_NEW_CONTEXT);
    let source = r#"(function(run){
      function runThis(code){return run(code);}
      function createContext(o){o=o||{};o.__vm_context__=true;return o}
      function isContext(o){return !!(o&&o.__vm_context__===true)}
      function Script(code){this.code=String(code)}
      Script.prototype.runInNewContext=function(s){return run(this.code,s)};
      Script.prototype.runInContext=function(s){return run(this.code,s)};
      Script.prototype.runInThisContext=function(){return run(this.code)};
      function compileFunction(code,params){params=params||[];return eval('(function('+params.join(',')+'){'+code+'})')}
      return {runInNewContext:run,runInContext:run,runInThisContext:runThis,Script:Script,createContext:createContext,isContext:isContext,compileFunction:compileFunction};
    })"#;
    let Ok(program) = quench_runtime::reduce::reduce_global_script_source(source) else {
        return Value::Undefined;
    };
    let context = quench_runtime::vm::current_context();
    let mut regs = Vec::new();
    let factory = quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::execute_in_place_context(program.ops(), &mut regs, &context)
    })
    .unwrap_or(Value::Undefined);
    quench_runtime::vm::call_value(&factory, &Value::Undefined, &[run]).unwrap_or(Value::Undefined)
}
