//! Decode validated modules for the shared VM. There is no executor here.

use crate::{Error, Module};
use rqj::{Engine, WasmI32Function};
use wasmparser::{Encoding, ExternalKind, Parser, Payload, ValType};

impl Module {
    /// Lower one exported standalone i32 function into the JavaScript VM's
    /// residual bytecode. Stateful sections and unsupported operators fail
    /// explicitly until their shared lowering is implemented.
    pub fn lower_shared_i32(&self, export: &str) -> Result<WasmI32Function, Error> {
        let mut types = Vec::new();
        let mut functions = Vec::new();
        let mut bodies = Vec::new();
        let mut selected = None;
        for payload in Parser::new(0).parse_all(self.bytes()) {
            match payload.map_err(parse_error)? {
                Payload::Version {
                    encoding: Encoding::Module,
                    ..
                }
                | Payload::CodeSectionStart { .. }
                | Payload::CustomSection(_)
                | Payload::End(_) => {}
                Payload::TypeSection(reader) => {
                    for ty in reader.into_iter_err_on_gc_types() {
                        types.push(ty.map_err(parse_error)?);
                    }
                }
                Payload::FunctionSection(reader) => {
                    for ty in reader {
                        functions.push(ty.map_err(parse_error)?);
                    }
                }
                Payload::ExportSection(reader) => {
                    for item in reader {
                        let item = item.map_err(parse_error)?;
                        if item.name == export && item.kind == ExternalKind::Func {
                            selected = Some(item.index as usize);
                        }
                    }
                }
                Payload::CodeSectionEntry(body) => bodies.push(body),
                _ => {
                    return Err(Error::Unsupported(
                        "module requires state, imports, or an unsupported section".into(),
                    ));
                }
            }
        }
        let selected = selected
            .ok_or_else(|| Error::Unsupported(format!("unknown function export: {export}")))?;
        let signature = &types[functions[selected] as usize];
        if signature
            .params()
            .iter()
            .chain(signature.results())
            .any(|ty| *ty != ValType::I32)
            || signature.results().len() > 1
        {
            return Err(Error::Unsupported(
                "function requires non-i32 or multiple results".into(),
            ));
        }
        let params = u16::try_from(signature.params().len())
            .map_err(|_| Error::Unsupported("too many parameters".into()))?;
        let body = &bodies[selected];
        let mut locals = 0u16;
        for local in body.get_locals_reader().map_err(parse_error)? {
            let (count, ty) = local.map_err(parse_error)?;
            if ty != ValType::I32 {
                return Err(Error::Unsupported(
                    "function requires non-i32 locals".into(),
                ));
            }
            locals = u16::try_from(count)
                .ok()
                .and_then(|count| locals.checked_add(count))
                .ok_or_else(|| Error::Unsupported("too many locals".into()))?;
        }
        Engine::lower_wasm_i32_function(
            export,
            params,
            locals,
            !signature.results().is_empty(),
            body.get_operators_reader().map_err(parse_error)?,
        )
        .map_err(|error| Error::Unsupported(error.to_string()))
    }
}

fn parse_error(error: wasmparser::BinaryReaderError) -> Error {
    Error::Parse(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rqj::{ExecutionRequest, Host, Runtime};

    #[derive(Default)]
    struct TestHost;
    impl Host for TestHost {
        fn write_line(&mut self, _: &str) {}
        fn clock_millis(&mut self) -> f64 {
            0.0
        }
    }

    fn lower(wat: &str) -> WasmI32Function {
        crate::Engine::new()
            .compile_wat(wat)
            .unwrap()
            .lower_shared_i32("f")
            .unwrap()
    }

    #[test]
    fn shared_i32_arithmetic_wraps_at_wasm_boundaries() {
        let mut runtime = Runtime::new(TestHost);
        for (op, left, right, expected) in [
            ("add", i32::MAX, 1, i32::MIN),
            ("sub", i32::MIN, 1, i32::MAX),
            ("mul", i32::MAX, 2, -2),
            ("mul", i32::MIN, -1, i32::MIN),
            ("add", -2, 1, -1),
        ] {
            let function = lower(&format!(
                "(module (func (export \"f\") (param i32 i32) (result i32) local.get 0 local.get 1 i32.{op}))"
            ));
            assert_eq!(
                runtime.execute_wasm_i32(&function, &[left, right]).unwrap(),
                Some(expected)
            );
        }
    }

    #[test]
    fn shared_locals_are_zeroed_and_tee_preserves_stack_value() {
        let function = lower(
            "(module (func (export \"f\") (param i32) (result i32) (local i32 i32) local.get 1 local.get 0 i32.add local.tee 2 drop local.get 2))",
        );
        let mut runtime = Runtime::new(TestHost);
        assert_eq!(
            runtime.execute_wasm_i32(&function, &[17]).unwrap(),
            Some(17)
        );
        assert_eq!(
            runtime.execute_wasm_i32(&function, &[-42]).unwrap(),
            Some(-42)
        );
        runtime.collect(function.residual()).unwrap();
    }

    #[test]
    fn shared_void_result_and_argument_count_are_explicit() {
        let function = lower(
            "(module (func (export \"f\") (param i32) (local i32) local.get 0 local.set 1 nop))",
        );
        let mut runtime = Runtime::new(TestHost);
        assert!(runtime.execute_wasm_i32(&function, &[]).is_err());
        assert!(runtime.execute_wasm_i32(&function, &[1, 2]).is_err());
        assert_eq!(runtime.execute_wasm_i32(&function, &[1]).unwrap(), None);
    }

    #[test]
    fn shared_wasm_and_javascript_use_the_same_runtime() {
        let mut runtime = Runtime::new(TestHost);
        let function = lower("(module (func (export \"f\") (result i32) i32.const -2147483648))");
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "if (2147483647 + 1 !== 2147483648) throw 'JS overflow';",
                "arithmetic.js",
            ))
            .unwrap();
        assert_eq!(
            runtime.execute_wasm_i32(&function, &[]).unwrap(),
            Some(i32::MIN)
        );
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "if (2 * 3 !== 6) throw 'JS multiply';",
                "arithmetic.js",
            ))
            .unwrap();
    }

    #[test]
    fn shared_lowering_rejects_unsupported_semantics() {
        for wat in [
            "(module (memory 1) (func (export \"f\") (result i32) i32.const 1))",
            "(module (func (export \"f\") (result i32) block (result i32) i32.const 1 end))",
            "(module (func (export \"f\") (result i64) i64.const 1))",
            "(module (func (export \"f\") (result i32) i32.const 1 return))",
            "(module (import \"m\" \"f\" (func)) (func (export \"f\")))",
            "(module (func $s) (start $s) (func (export \"f\")))",
        ] {
            let module = crate::Engine::new().compile_wat(wat).unwrap();
            assert!(
                matches!(module.lower_shared_i32("f"), Err(Error::Unsupported(_))),
                "{wat}"
            );
        }
        let module = crate::Engine::new().compile_wat("(module)").unwrap();
        assert!(module.lower_shared_i32("missing").is_err());
    }

    #[test]
    fn shared_lowering_uses_wide_encoding_for_deep_stacks() {
        let wat = format!(
            "(module (func (export \"f\") (result i32) {} {}))",
            "i32.const 1 ".repeat(300),
            "i32.add ".repeat(299)
        );
        let function = lower(&wat);
        let mut runtime = Runtime::new(TestHost);
        assert_eq!(runtime.execute_wasm_i32(&function, &[]).unwrap(), Some(300));
    }

    #[test]
    fn shared_lowering_resolves_the_exported_function_index() {
        let function = lower(
            "(module (func (result i32) i32.const 99) (func (export \"f\") (param i32) (result i32) local.get 0 i32.const 3 i32.mul))",
        );
        let mut runtime = Runtime::new(TestHost);
        assert_eq!(runtime.execute_wasm_i32(&function, &[7]).unwrap(), Some(21));
    }

    #[test]
    fn shared_integer_traps_are_typed_and_runtime_recovers() {
        use rqj::WasmTrap;

        let divide = lower(
            "(module (func (export \"f\") (param i32 i32) (result i32) local.get 0 local.get 1 i32.div_s))",
        );
        let remainder = lower(
            "(module (func (export \"f\") (param i32 i32) (result i32) local.get 0 local.get 1 i32.rem_s))",
        );
        let mut runtime = Runtime::new(TestHost);
        for (args, trap) in [
            ([i32::MIN, 0], WasmTrap::IntegerDivideByZero),
            ([i32::MIN, -1], WasmTrap::IntegerOverflow),
        ] {
            let error = runtime.execute_wasm_i32(&divide, &args).unwrap_err();
            assert_eq!(error.wasm_trap(), Some(trap));
            assert_eq!(error.to_string(), trap.to_string());
            runtime.collect(divide.residual()).unwrap();
            assert_eq!(runtime.execute_wasm_i32(&divide, &[7, 2]).unwrap(), Some(3));
        }
        assert_eq!(
            runtime
                .execute_wasm_i32(&remainder, &[i32::MIN, -1])
                .unwrap(),
            Some(0)
        );
        let error = runtime.execute_wasm_i32(&divide, &[]).unwrap_err();
        assert_eq!(error.wasm_trap(), None);

        let program = rqj::Engine::specialize("throw new Error('guest');", "throw.js").unwrap();
        assert_eq!(runtime.execute(&program).unwrap_err().wasm_trap(), None);
    }
}

#[cfg(test)]
mod spec;
