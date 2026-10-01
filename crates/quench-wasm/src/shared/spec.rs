//! Exercise every directive in the pinned i32 file through shared execution.
//! No legacy executor or unsupported-directive skip is permitted here.

use crate::{Engine, Error, Module};
use rqj::{Host, Runtime, WasmI32Function, WasmTrap};
use std::collections::HashMap;
use wast::core::{WastArgCore, WastRetCore};
use wast::{Wast, WastArg, WastDirective, WastExecute, WastInvoke, WastRet};

struct TestHost;
impl Host for TestHost {
    fn write_line(&mut self, _: &str) {}
    fn clock_millis(&mut self) -> f64 {
        0.0
    }
}

#[test]
fn pinned_i32_directives_use_shared_execution() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../quench-wasm-test/testsuite/i32.wast");
    let source = std::fs::read_to_string(path).unwrap();
    let buffer = wast::parser::ParseBuffer::new(&source).unwrap();
    let mut script: Wast<'_> = wast::parser::parse(&buffer).unwrap();
    let engine = Engine::new();
    let mut runtime = Runtime::new(TestHost);
    let mut current = None;
    let mut functions = HashMap::new();
    let mut modules = 0;
    let mut returns = 0;
    let mut traps = 0;
    let mut invalid = 0;
    let mut malformed = 0;
    for directive in &mut script.directives {
        let line = directive.span().linecol_in(&source).0 + 1;
        match directive {
            WastDirective::Module(module) => {
                current = Some(engine.compile(&module.encode().unwrap()).unwrap());
                functions.clear();
                modules += 1;
            }
            WastDirective::AssertReturn { exec, results, .. } => {
                let got = invoke(
                    exec,
                    current.as_ref().unwrap(),
                    &mut functions,
                    &mut runtime,
                )
                .unwrap_or_else(|error| panic!("i32.wast:{line}: {error}"));
                let expected = match results.as_slice() {
                    [] => None,
                    [WastRet::Core(WastRetCore::I32(value))] => Some(*value),
                    _ => panic!("i32.wast:{line}: unexpected result type"),
                };
                assert_eq!(got, expected, "i32.wast:{line}");
                returns += 1;
            }
            WastDirective::AssertTrap { exec, message, .. } => {
                let error = invoke(
                    exec,
                    current.as_ref().unwrap(),
                    &mut functions,
                    &mut runtime,
                )
                .expect_err("expected Wasm trap");
                let expected = match *message {
                    "integer divide by zero" => WasmTrap::IntegerDivideByZero,
                    "integer overflow" => WasmTrap::IntegerOverflow,
                    _ => panic!("i32.wast:{line}: unexpected trap class"),
                };
                assert_eq!(error.wasm_trap(), Some(expected), "i32.wast:{line}");
                assert_eq!(error.to_string(), *message, "i32.wast:{line}");
                traps += 1;
            }
            WastDirective::AssertInvalid {
                module, message, ..
            } => {
                let error = engine.compile(&module.encode().unwrap()).unwrap_err();
                assert!(
                    matches!(error, Error::Validate(_)),
                    "i32.wast:{line}: {error}"
                );
                assert!(
                    error.to_string().contains(*message),
                    "i32.wast:{line}: {error}"
                );
                invalid += 1;
            }
            WastDirective::AssertMalformed {
                module, message, ..
            } => {
                let error = match module.encode() {
                    Err(error) => error.to_string(),
                    Ok(bytes) => match engine.compile(&bytes).unwrap_err() {
                        Error::Parse(error) => error,
                        error => panic!("i32.wast:{line}: expected parse error, got {error}"),
                    },
                };
                // The spec labels grammar failures; decoder diagnostic wording
                // is implementation-specific. The parse-error class is required.
                assert!(!error.is_empty(), "i32.wast:{line}: expected {message}");
                malformed += 1;
            }
            _ => panic!("i32.wast:{line}: unsupported directive"),
        }
    }
    assert_eq!(
        modules + returns + traps + invalid + malformed,
        script.directives.len()
    );
    eprintln!(
        "shared i32: {} directives, {returns} returns, {traps} traps, {invalid} invalid, {malformed} malformed",
        script.directives.len()
    );
}

fn invoke(
    execute: &WastExecute<'_>,
    module: &Module,
    functions: &mut HashMap<String, WasmI32Function>,
    runtime: &mut Runtime<TestHost>,
) -> Result<Option<i32>, rqj::JsError> {
    let WastExecute::Invoke(WastInvoke {
        module: None,
        name,
        args,
        ..
    }) = execute
    else {
        panic!("unsupported execution directive");
    };
    let function = functions
        .entry((*name).to_owned())
        .or_insert_with(|| module.lower_shared_i32(name).unwrap());
    let args = args
        .iter()
        .map(|arg| match arg {
            WastArg::Core(WastArgCore::I32(value)) => *value,
            _ => panic!("unexpected argument type"),
        })
        .collect::<Vec<_>>();
    runtime.execute_wasm_i32(function, &args)
}
