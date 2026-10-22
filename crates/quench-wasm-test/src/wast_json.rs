//! A `.wast` script as JSON commands with binary modules, for engines that run Wasm through
//! a JavaScript host and cannot read `.wast` text (Node, Bun). The shape follows
//! `wast2json`: one command per directive, each module as its encoded binary (hex), each
//! value as its bit pattern in decimal so 64-bit integers and NaN payloads survive JSON.

use serde_json::{json, Value};
use wast::core::{AbstractHeapType, HeapType, NanPattern, V128Pattern, WastArgCore, WastRetCore};
use wast::lexer::Lexer;
use wast::parser::{self, ParseBuffer};
use wast::token::{F32, F64};
use wast::{QuoteWat, Wast, WastArg, WastDirective, WastExecute, WastInvoke, WastRet, Wat};

use crate::legacy_try::unfold_if_legacy;

/// `source` (the script at `filename`) as `{"commands": [...]}`, or the parse error.
pub fn wast_to_json(filename: &str, source: &str) -> Result<Value, String> {
    let source = unfold_if_legacy(filename, source);
    let source = source.as_ref();
    let mut lexer = Lexer::new(source);
    lexer.allow_confusing_unicode(true);
    let buffer = ParseBuffer::new_with_lexer(lexer).map_err(|error| error.to_string())?;
    let mut script: Wast<'_> = parser::parse(&buffer).map_err(|error| error.to_string())?;
    let commands = script
        .directives
        .iter_mut()
        .map(|directive| {
            let line = directive.span().linecol_in(source).0 + 1;
            let mut command = directive_json(directive);
            command["line"] = json!(line);
            command
        })
        .collect::<Vec<_>>();
    Ok(json!({ "commands": commands }))
}

fn directive_json(directive: &mut WastDirective<'_>) -> Value {
    match directive {
        WastDirective::Module(module) => {
            json!({"cmd": "module", "name": quote_name(module), "binary": quote_binary(module)})
        }
        WastDirective::ModuleDefinition(module) => json!({
            "cmd": "module_definition", "name": quote_name(module), "binary": quote_binary(module)
        }),
        WastDirective::ModuleInstance {
            instance, module, ..
        } => json!({
            "cmd": "module_instance",
            "instance": instance.map(|id| id.name()),
            "module": module.map(|id| id.name()),
        }),
        WastDirective::AssertMalformed { module, .. }
        | WastDirective::AssertMalformedCustom { module, .. } => {
            json!({"cmd": "assert_malformed", "binary": quote_binary(module)})
        }
        WastDirective::AssertInvalid { module, .. }
        | WastDirective::AssertInvalidCustom { module, .. } => {
            json!({"cmd": "assert_invalid", "binary": quote_binary(module)})
        }
        WastDirective::AssertUnlinkable { module, .. } => {
            json!({"cmd": "assert_unlinkable", "binary": wat_binary(module)})
        }
        WastDirective::Register { name, module, .. } => {
            json!({"cmd": "register", "as": name, "module": module.map(|id| id.name())})
        }
        WastDirective::Invoke(invoke) => json!({"cmd": "action", "action": invoke_json(invoke)}),
        WastDirective::AssertReturn { exec, results, .. } => json!({
            "cmd": "assert_return",
            "action": execute_json(exec),
            "expected": results.iter().map(ret_json).collect::<Vec<_>>(),
        }),
        WastDirective::AssertTrap { exec, .. } => {
            json!({"cmd": "assert_trap", "action": execute_json(exec)})
        }
        WastDirective::AssertExhaustion { call, .. } => {
            json!({"cmd": "assert_exhaustion", "action": invoke_json(call)})
        }
        WastDirective::AssertException { exec, .. } => {
            json!({"cmd": "assert_exception", "action": execute_json(exec)})
        }
        other => json!({"cmd": "unsupported", "kind": directive_kind(other)}),
    }
}

fn directive_kind(directive: &WastDirective<'_>) -> &'static str {
    match directive {
        WastDirective::AssertSuspension { .. } => "assert_suspension",
        WastDirective::Thread(_) => "thread",
        WastDirective::Wait { .. } => "wait",
        _ => "other",
    }
}

fn quote_name(module: &QuoteWat<'_>) -> Option<String> {
    module.name().map(|id| id.name().to_owned())
}

/// The module's binary as hex; `null` when it does not encode (text-level malformation).
fn quote_binary(module: &mut QuoteWat<'_>) -> Value {
    module.encode().map_or(Value::Null, |bytes| json!(hex(&bytes)))
}

fn wat_binary(module: &mut Wat<'_>) -> Value {
    module.encode().map_or(Value::Null, |bytes| json!(hex(&bytes)))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn invoke_json(invoke: &WastInvoke<'_>) -> Value {
    json!({
        "type": "invoke",
        "module": invoke.module.map(|id| id.name()),
        "field": invoke.name,
        "args": invoke.args.iter().map(arg_json).collect::<Vec<_>>(),
    })
}

fn execute_json(exec: &mut WastExecute<'_>) -> Value {
    match exec {
        WastExecute::Invoke(invoke) => invoke_json(invoke),
        WastExecute::Get { module, global, .. } => {
            json!({"type": "get", "module": module.map(|id| id.name()), "field": global})
        }
        WastExecute::Wat(module) => json!({"type": "module", "binary": wat_binary(module)}),
    }
}

fn arg_json(arg: &WastArg<'_>) -> Value {
    let WastArg::Core(arg) = arg else {
        return json!({"type": "unsupported"});
    };
    match arg {
        WastArgCore::I32(value) => bits("i32", u64::from(*value as u32)),
        WastArgCore::I64(value) => bits("i64", *value as u64),
        WastArgCore::F32(value) => bits("f32", u64::from(value.bits)),
        WastArgCore::F64(value) => bits("f64", value.bits),
        WastArgCore::V128(value) => json!({"type": "v128", "bytes": hex(&value.to_le_bytes())}),
        WastArgCore::RefNull(heap) => json!({"type": "null", "heap": heap_name(heap)}),
        WastArgCore::RefExtern(id) => json!({"type": "externref", "id": id}),
        WastArgCore::RefHost(id) => json!({"type": "hostref", "id": id}),
    }
}

fn ret_json(ret: &WastRet<'_>) -> Value {
    let WastRet::Core(ret) = ret else {
        return json!({"type": "unsupported"});
    };
    ret_core_json(ret)
}

fn ret_core_json(ret: &WastRetCore<'_>) -> Value {
    match ret {
        WastRetCore::I32(value) => bits("i32", u64::from(*value as u32)),
        WastRetCore::I64(value) => bits("i64", *value as u64),
        WastRetCore::F32(pattern) => float_pattern("f32", pattern, |value: &F32| u64::from(value.bits)),
        WastRetCore::F64(pattern) => float_pattern("f64", pattern, |value: &F64| value.bits),
        WastRetCore::V128(pattern) => v128_pattern(pattern),
        WastRetCore::RefNull(_) => json!({"type": "null"}),
        WastRetCore::RefExtern(id) => json!({"type": "externref", "id": id}),
        WastRetCore::RefHost(id) => json!({"type": "hostref", "id": id}),
        WastRetCore::RefFunc(_) => json!({"type": "funcref"}),
        WastRetCore::RefAny => json!({"type": "ref", "heap": "any"}),
        WastRetCore::RefEq => json!({"type": "ref", "heap": "eq"}),
        WastRetCore::RefArray => json!({"type": "ref", "heap": "array"}),
        WastRetCore::RefStruct => json!({"type": "ref", "heap": "struct"}),
        WastRetCore::RefI31 | WastRetCore::RefI31Shared => json!({"type": "ref", "heap": "i31"}),
        WastRetCore::Either(options) => json!({
            "type": "either", "values": options.iter().map(ret_core_json).collect::<Vec<_>>()
        }),
    }
}

fn bits(ty: &str, value: u64) -> Value {
    json!({"type": ty, "bits": value.to_string()})
}

fn float_pattern<T>(ty: &str, pattern: &NanPattern<T>, bits_of: impl Fn(&T) -> u64) -> Value {
    match pattern {
        NanPattern::CanonicalNan => json!({"type": ty, "nan": "canonical"}),
        NanPattern::ArithmeticNan => json!({"type": ty, "nan": "arithmetic"}),
        NanPattern::Value(value) => bits(ty, bits_of(value)),
    }
}

fn v128_pattern(pattern: &V128Pattern) -> Value {
    let (lanes, values): (&str, Vec<Value>) = match pattern {
        V128Pattern::I8x16(lanes) => ("i8", lanes.iter().map(|v| bits("i8", u64::from(*v as u8))).collect()),
        V128Pattern::I16x8(lanes) => ("i16", lanes.iter().map(|v| bits("i16", u64::from(*v as u16))).collect()),
        V128Pattern::I32x4(lanes) => ("i32", lanes.iter().map(|v| bits("i32", u64::from(*v as u32))).collect()),
        V128Pattern::I64x2(lanes) => ("i64", lanes.iter().map(|v| bits("i64", *v as u64)).collect()),
        V128Pattern::F32x4(lanes) => (
            "f32",
            lanes.iter().map(|lane| float_pattern("f32", lane, |value: &F32| u64::from(value.bits))).collect(),
        ),
        V128Pattern::F64x2(lanes) => (
            "f64",
            lanes.iter().map(|lane| float_pattern("f64", lane, |value: &F64| value.bits)).collect(),
        ),
    };
    json!({"type": "v128", "lanes": lanes, "values": values})
}

fn heap_name(heap: &HeapType<'_>) -> &'static str {
    match heap {
        HeapType::Abstract {
            ty: AbstractHeapType::Func | AbstractHeapType::NoFunc,
            ..
        } => "func",
        HeapType::Abstract {
            ty: AbstractHeapType::Extern | AbstractHeapType::NoExtern,
            ..
        } => "extern",
        _ => "any",
    }
}


#[cfg(test)]
mod tests {
    use super::wast_to_json;

    #[test]
    fn commands_carry_binaries_and_bit_patterns() {
        let json = wast_to_json(
            "inline.wast",
            r#"(module (func (export "f") (param i64) (result f32) f32.const nan))
               (assert_return (invoke "f" (i64.const -1)) (f32.const nan:canonical))
               (assert_malformed (module quote "(func") "unexpected end")"#,
        )
        .unwrap();
        let commands = json["commands"].as_array().unwrap();
        assert_eq!(commands[0]["cmd"], "module");
        assert!(commands[0]["binary"].as_str().unwrap().starts_with("0061736d"));
        assert_eq!(commands[1]["action"]["args"][0]["bits"], u64::MAX.to_string());
        assert_eq!(commands[1]["expected"][0]["nan"], "canonical");
        assert_eq!(commands[2]["cmd"], "assert_malformed");
        assert!(commands[2]["binary"].is_null());
        assert_eq!(commands[1]["line"], 2);
    }
}
