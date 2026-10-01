//! Exercise every directive in the pinned numeric files through shared execution.
//! No legacy executor or unsupported-directive skip is permitted here.

use crate::{Engine, Error, Module};
use rqj::{Host, Runtime, WasmFunction, WasmTrap, WasmValue};
use std::collections::HashMap;
use wast::core::{NanPattern, WastArgCore, WastRetCore};
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
    run_numeric_spec("i32");
}

#[test]
fn pinned_i64_directives_use_shared_execution() {
    run_numeric_spec("i64");
}

#[test]
fn pinned_f32_directives_use_shared_execution() {
    run_numeric_spec("f32");
}
#[test]
fn pinned_f64_directives_use_shared_execution() {
    run_numeric_spec("f64");
}
#[test]
fn pinned_f32_comparisons_use_shared_execution() {
    run_numeric_spec("f32_cmp");
}
#[test]
fn pinned_f64_comparisons_use_shared_execution() {
    run_numeric_spec("f64_cmp");
}
#[test]
fn pinned_f32_bitwise_operators_use_shared_execution() {
    run_numeric_spec("f32_bitwise");
}
#[test]
fn pinned_f64_bitwise_operators_use_shared_execution() {
    run_numeric_spec("f64_bitwise");
}
#[test]
fn pinned_conversion_directives_use_shared_execution() {
    run_numeric_spec("conversions");
}

fn run_numeric_spec(file: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../quench-wasm-test/testsuite/{file}.wast"));
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
                .unwrap_or_else(|error| panic!("{file}.wast:{line}: {error}"));
                assert_result(got, results, &format!("{file}.wast:{line}"));
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
                    "invalid conversion to integer" => WasmTrap::InvalidConversionToInteger,
                    _ => panic!("{file}.wast:{line}: unexpected trap class"),
                };
                assert_eq!(error.wasm_trap(), Some(expected), "{file}.wast:{line}");
                assert_eq!(error.to_string(), *message, "{file}.wast:{line}");
                traps += 1;
            }
            WastDirective::AssertInvalid {
                module, message, ..
            } => {
                let error = engine.compile(&module.encode().unwrap()).unwrap_err();
                assert!(
                    matches!(error, Error::Validate(_)),
                    "{file}.wast:{line}: {error}"
                );
                assert!(
                    error.to_string().contains(*message),
                    "{file}.wast:{line}: {error}"
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
                        error => panic!("{file}.wast:{line}: expected parse error, got {error}"),
                    },
                };
                // The spec labels grammar failures; decoder diagnostic wording
                // is implementation-specific. The parse-error class is required.
                assert!(!error.is_empty(), "{file}.wast:{line}: expected {message}");
                malformed += 1;
            }
            _ => panic!("{file}.wast:{line}: unsupported directive"),
        }
    }
    assert_eq!(
        modules + returns + traps + invalid + malformed,
        script.directives.len()
    );
    eprintln!(
        "shared {file}: {} directives, {returns} returns, {traps} traps, {invalid} invalid, {malformed} malformed",
        script.directives.len()
    );
}

fn invoke(
    execute: &WastExecute<'_>,
    module: &Module,
    functions: &mut HashMap<String, WasmFunction>,
    runtime: &mut Runtime<TestHost>,
) -> Result<Option<WasmValue>, rqj::JsError> {
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
        .or_insert_with(|| module.lower_shared(name).unwrap());
    let args = args
        .iter()
        .map(|arg| match arg {
            WastArg::Core(WastArgCore::I32(value)) => WasmValue::I32(*value),
            WastArg::Core(WastArgCore::I64(value)) => WasmValue::I64(*value),
            WastArg::Core(WastArgCore::F32(value)) => WasmValue::F32(value.bits),
            WastArg::Core(WastArgCore::F64(value)) => WasmValue::F64(value.bits),
            _ => panic!("unexpected argument type"),
        })
        .collect::<Vec<_>>();
    runtime.execute_wasm(function, &args)
}

fn assert_result(got: Option<WasmValue>, results: &[WastRet<'_>], context: &str) {
    match results {
        [] => assert_eq!(got, None, "{context}"),
        [WastRet::Core(WastRetCore::I32(value))] => {
            assert_eq!(got, Some(WasmValue::I32(*value)), "{context}")
        }
        [WastRet::Core(WastRetCore::I64(value))] => {
            assert_eq!(got, Some(WasmValue::I64(*value)), "{context}")
        }
        [WastRet::Core(WastRetCore::F32(pattern))] => {
            assert!(matches!(got, Some(WasmValue::F32(_))), "{context}: {got:?}");
            assert_float(
                got.unwrap(),
                *pattern,
                |value| WasmValue::F32(value.bits),
                context,
            );
        }
        [WastRet::Core(WastRetCore::F64(pattern))] => {
            assert!(matches!(got, Some(WasmValue::F64(_))), "{context}: {got:?}");
            assert_float(
                got.unwrap(),
                *pattern,
                |value| WasmValue::F64(value.bits),
                context,
            );
        }
        _ => panic!("{context}: unexpected result type"),
    }
}

fn assert_float<T>(
    got: WasmValue,
    pattern: NanPattern<T>,
    value: impl FnOnce(T) -> WasmValue,
    context: &str,
) {
    match pattern {
        NanPattern::Value(expected) => assert_eq!(got, value(expected), "{context}"),
        NanPattern::CanonicalNan => assert!(got.is_canonical_nan(), "{context}: {got:?}"),
        NanPattern::ArithmeticNan => assert!(got.is_arithmetic_nan(), "{context}: {got:?}"),
    }
}
