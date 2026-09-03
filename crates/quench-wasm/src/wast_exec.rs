//! Execute-class wast directives against a Native instance.

use std::collections::HashMap;

use quench_runtime::hir::ImportKind;
use quench_runtime::instance::{Instance, InvokeError, ResolvedImport};
use quench_runtime::slot::Slot;
use wasmparser::WasmFeatures;
use wast::core::{WastArgCore, WastRetCore};
use wast::{QuoteWat, WastArg, WastExecute, WastInvoke, WastRet, Wat};

use crate::wast_script::DirectiveResult;

pub struct Store {
    current: Option<Instance>,
    named: HashMap<String, Instance>,
    defs: HashMap<String, Vec<u8>>,
    spectest: Instance,
}

impl Store {
    pub fn new() -> Self {
        Self {
            current: None,
            named: HashMap::new(),
            defs: HashMap::new(),
            spectest: Instance::spectest(),
        }
    }

    pub fn instantiate_quote(&mut self, module: &mut QuoteWat<'_>, features: WasmFeatures) {
        let name = module.name().map(|id| id.name().to_string());
        match module.encode() {
            Ok(bytes) => {
                let instance = self.build(&bytes, features);
                if let Some(name) = name {
                    self.named.insert(name, instance.clone());
                }
                self.current = Some(instance);
            }
            Err(_) => self.current = Some(Instance::unsupported()),
        }
    }

    pub fn define_quote(&mut self, module: &mut QuoteWat<'_>, features: WasmFeatures) {
        if let (Some(name), Ok(bytes)) = (
            module.name().map(|id| id.name().to_string()),
            module.encode(),
        ) {
            self.defs.insert(name, bytes);
        }
        let _ = features;
    }

    pub fn instantiate_def(
        &mut self,
        instance: Option<&str>,
        module: Option<&str>,
        features: WasmFeatures,
    ) {
        let Some(module) = module else { return };
        let Some(bytes) = self.defs.get(module).cloned() else {
            return;
        };
        let built = self.build(&bytes, features);
        if let Some(name) = instance {
            self.named.insert(name.to_string(), built.clone());
        }
        self.current = Some(built);
    }

    pub fn register(&mut self, name: &str, module: Option<&str>) {
        let instance = if let Some(module) = module {
            self.named.get(module).cloned()
        } else {
            self.current.clone()
        };
        if let Some(instance) = instance {
            self.named.insert(name.to_string(), instance);
        }
    }

    fn build(&self, bytes: &[u8], features: WasmFeatures) -> Instance {
        self.try_build(bytes, features)
            .unwrap_or_else(|_| Instance::unsupported())
    }

    fn try_build(&self, bytes: &[u8], features: WasmFeatures) -> Result<Instance, InvokeError> {
        Instance::from_bytes(bytes, features, |module, name, kind, types| {
            self.lookup(module, name, kind, types)
        })
    }

    fn lookup(
        &self,
        module: &str,
        name: &str,
        kind: &ImportKind,
        types: &[quench_runtime::hir::FuncSig],
    ) -> Result<ResolvedImport, InvokeError> {
        let instance = if module == "spectest" {
            &self.spectest
        } else {
            self.named
                .get(module)
                .ok_or(InvokeError::Unlinkable("unknown import"))?
        };
        instance.match_import(name, kind, types)
    }
}

enum Outcome {
    Values(Vec<Slot>),
    Trap(&'static str),
    Exception,
    Unimplemented,
    Missing,
}

pub fn score_return(
    line: usize,
    exec: &mut WastExecute<'_>,
    expected: &[WastRet<'_>],
    store: &mut Store,
    features: WasmFeatures,
) -> DirectiveResult {
    let kind = "assert_return".to_string();
    match run(exec, store, features) {
        Outcome::Unimplemented => unimplemented(line, &kind),
        Outcome::Values(got) => compare_rets(line, kind, expected, &got),
        Outcome::Trap(got) => fail(line, kind, "return", got),
        Outcome::Exception => fail(line, kind, "return", "exception"),
        Outcome::Missing => fail(line, kind, "return", "unknown export"),
    }
}

pub fn score_trap(
    line: usize,
    exec: &mut WastExecute<'_>,
    message: &str,
    store: &mut Store,
    features: WasmFeatures,
) -> DirectiveResult {
    let kind = "assert_trap".to_string();
    let expected = format!("trap ({message})");
    match run(exec, store, features) {
        Outcome::Unimplemented => unimplemented(line, &kind),
        Outcome::Trap(got) if trap_matches(got, message) => DirectiveResult {
            line,
            kind,
            passed: true,
            expected,
            got: got.to_string(),
        },
        Outcome::Trap(got) => fail(line, kind, &expected, got),
        Outcome::Values(_) => fail(line, kind, &expected, "return"),
        Outcome::Exception => fail(line, kind, &expected, "exception"),
        Outcome::Missing => fail(line, kind, &expected, "unknown export"),
    }
}

pub fn score_exception(
    line: usize,
    exec: &mut WastExecute<'_>,
    store: &mut Store,
    features: WasmFeatures,
) -> DirectiveResult {
    let kind = "assert_exception".to_string();
    match run(exec, store, features) {
        Outcome::Exception => DirectiveResult {
            line,
            kind,
            passed: true,
            expected: "exception".to_string(),
            got: "exception".to_string(),
        },
        Outcome::Unimplemented => unimplemented(line, &kind),
        Outcome::Trap(got) => fail(line, kind, "exception", got),
        Outcome::Values(_) => fail(line, kind, "exception", "return"),
        Outcome::Missing => fail(line, kind, "exception", "unknown export"),
    }
}

pub fn score_unlinkable(
    line: usize,
    module: &mut Wat<'_>,
    message: &str,
    features: WasmFeatures,
    store: &Store,
) -> DirectiveResult {
    let kind = "assert_unlinkable".to_string();
    let expected = format!("unlinkable ({message})");
    let bytes = match module.encode() {
        Ok(bytes) => bytes,
        Err(error) => {
            return DirectiveResult {
                line,
                kind,
                passed: true,
                expected,
                got: error.to_string(),
            };
        }
    };
    match store.try_build(&bytes, features) {
        Err(InvokeError::Unlinkable(got)) => DirectiveResult {
            line,
            kind,
            passed: true,
            expected,
            got: got.to_string(),
        },
        Err(InvokeError::Unimplemented) => unimplemented(line, &kind),
        Err(error) => fail(line, kind, &expected, error.message()),
        Ok(_) => fail(line, kind, &expected, "linked"),
    }
}

pub fn score_exhaustion(
    line: usize,
    invoke: &WastInvoke<'_>,
    message: &str,
    store: &mut Store,
) -> DirectiveResult {
    let kind = "assert_exhaustion".to_string();
    let expected = format!("exhaustion ({message})");
    match run_invoke(invoke, store) {
        Outcome::Unimplemented => unimplemented(line, &kind),
        Outcome::Trap(got) if trap_matches(got, message) || got.contains("exhaust") => {
            DirectiveResult {
                line,
                kind,
                passed: true,
                expected,
                got: got.to_string(),
            }
        }
        Outcome::Trap(got) => fail(line, kind, &expected, got),
        Outcome::Values(_) => fail(line, kind, &expected, "return"),
        Outcome::Exception => fail(line, kind, &expected, "exception"),
        Outcome::Missing => fail(line, kind, &expected, "unknown export"),
    }
}

pub fn score_invoke(line: usize, invoke: &WastInvoke<'_>, store: &mut Store) -> DirectiveResult {
    let kind = "invoke".to_string();
    match run_invoke(invoke, store) {
        Outcome::Unimplemented => unimplemented(line, &kind),
        Outcome::Values(_) => DirectiveResult {
            line,
            kind,
            passed: true,
            expected: "invoke".to_string(),
            got: "ok".to_string(),
        },
        Outcome::Trap(got) => fail(line, kind, "invoke", got),
        Outcome::Exception => fail(line, kind, "invoke", "exception"),
        Outcome::Missing => fail(line, kind, "invoke", "unknown export"),
    }
}

fn run(exec: &mut WastExecute<'_>, store: &mut Store, features: WasmFeatures) -> Outcome {
    match exec {
        WastExecute::Invoke(invoke) => run_invoke(invoke, store),
        WastExecute::Get { module, global, .. } => run_get(*module, global, store),
        WastExecute::Wat(wat) => run_wat(wat, store, features),
    }
}

fn run_get(module: Option<wast::token::Id<'_>>, global: &str, store: &Store) -> Outcome {
    let instance = if let Some(module) = module {
        store.named.get(module.name())
    } else {
        store.current.as_ref()
    };
    match instance.map(|i| i.get_global(global)) {
        Some(Ok(slot)) => Outcome::Values(vec![slot]),
        Some(Err(InvokeError::MissingExport)) | None => Outcome::Missing,
        Some(Err(InvokeError::Unimplemented)) => Outcome::Unimplemented,
        Some(Err(InvokeError::TypeMismatch)) => Outcome::Trap("type mismatch"),
        Some(Err(InvokeError::Failure(failure))) => Outcome::Trap(failure.message()),
        Some(Err(InvokeError::Unlinkable(message))) => Outcome::Trap(message),
    }
}

fn run_wat(wat: &mut Wat<'_>, store: &Store, features: WasmFeatures) -> Outcome {
    let bytes = match wat.encode() {
        Ok(bytes) => bytes,
        Err(_) => return Outcome::Unimplemented,
    };
    match store.try_build(&bytes, features) {
        Ok(_) => Outcome::Values(Vec::new()),
        Err(InvokeError::Failure(failure)) => Outcome::Trap(failure.message()),
        Err(InvokeError::Unlinkable(message)) => Outcome::Trap(message),
        Err(InvokeError::Unimplemented) => Outcome::Unimplemented,
        Err(InvokeError::TypeMismatch) => Outcome::Trap("type mismatch"),
        Err(InvokeError::MissingExport) => Outcome::Missing,
    }
}

fn run_invoke(invoke: &WastInvoke<'_>, store: &mut Store) -> Outcome {
    let Some(args) = args_slots(&invoke.args) else {
        return Outcome::Unimplemented;
    };
    let result = if let Some(module) = invoke.module {
        store
            .named
            .get(module.name())
            .ok_or(InvokeError::MissingExport)
            .and_then(|instance| instance.invoke(invoke.name, &args))
    } else {
        store
            .current
            .as_ref()
            .ok_or(InvokeError::MissingExport)
            .and_then(|instance| instance.invoke(invoke.name, &args))
    };
    match result {
        Ok(values) => Outcome::Values(values),
        Err(InvokeError::Failure(quench_runtime::unwind::Failure::Exception { .. })) => {
            Outcome::Exception
        }
        Err(InvokeError::Failure(failure)) => Outcome::Trap(failure.message()),
        Err(InvokeError::Unimplemented) => Outcome::Unimplemented,
        Err(InvokeError::TypeMismatch) => Outcome::Trap("type mismatch"),
        Err(InvokeError::MissingExport) => Outcome::Missing,
        Err(InvokeError::Unlinkable(message)) => Outcome::Trap(message),
    }
}

fn args_slots(args: &[WastArg<'_>]) -> Option<Vec<Slot>> {
    args.iter().map(arg_slot).collect()
}

fn arg_slot(arg: &WastArg<'_>) -> Option<Slot> {
    match arg {
        WastArg::Core(WastArgCore::I32(value)) => Some(Slot::native_i32(*value)),
        WastArg::Core(WastArgCore::I64(value)) => {
            Some(Slot::Native(quench_runtime::native::Native::I64(*value)))
        }
        WastArg::Core(WastArgCore::F32(value)) => Some(Slot::Native(
            quench_runtime::native::Native::F32(value.bits),
        )),
        WastArg::Core(WastArgCore::F64(value)) => Some(Slot::Native(
            quench_runtime::native::Native::F64(value.bits),
        )),
        WastArg::Core(WastArgCore::V128(value)) => Some(Slot::Native(
            quench_runtime::native::Native::V128(v128_bits(value)),
        )),
        WastArg::Core(WastArgCore::RefNull(_)) => Some(Slot::Native(
            quench_runtime::native::Native::Ref(quench_runtime::native::RefVal::Null),
        )),
        WastArg::Core(WastArgCore::RefExtern(id)) => Some(Slot::Native(
            quench_runtime::native::Native::Ref(quench_runtime::native::RefVal::Extern(*id)),
        )),
        WastArg::Core(WastArgCore::RefHost(id)) => Some(Slot::Native(
            quench_runtime::native::Native::Ref(quench_runtime::native::RefVal::Host(*id)),
        )),
        _ => None,
    }
}

fn v128_bits(value: &wast::core::V128Const) -> u128 {
    u128::from_le_bytes(value.to_le_bytes())
}

fn v128_matches(pattern: &wast::core::V128Pattern, bits: u128) -> bool {
    let b = bits.to_le_bytes();
    match pattern {
        wast::core::V128Pattern::I8x16(vals) => {
            vals.iter().enumerate().all(|(i, lane)| *lane == b[i] as i8)
        }
        wast::core::V128Pattern::I16x8(vals) => vals
            .iter()
            .enumerate()
            .all(|(i, lane)| *lane == i16::from_le_bytes([b[i * 2], b[i * 2 + 1]])),
        wast::core::V128Pattern::I32x4(vals) => vals
            .iter()
            .enumerate()
            .all(|(i, lane)| *lane == i32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())),
        wast::core::V128Pattern::I64x2(vals) => vals
            .iter()
            .enumerate()
            .all(|(i, lane)| *lane == i64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap())),
        wast::core::V128Pattern::F32x4(vals) => vals.iter().enumerate().all(|(i, lane)| {
            let got = u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
            nan_f32(lane, got)
        }),
        wast::core::V128Pattern::F64x2(vals) => vals.iter().enumerate().all(|(i, lane)| {
            let got = u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap());
            nan_f64(lane, got)
        }),
    }
}

fn compare_rets(
    line: usize,
    kind: String,
    expected: &[WastRet<'_>],
    got: &[Slot],
) -> DirectiveResult {
    if expected.len() != got.len() {
        return fail(
            line,
            kind,
            &format!("{} values", expected.len()),
            &format!("{} values", got.len()),
        );
    }
    for (want, got) in expected.iter().zip(got) {
        if !ret_matches(want, got) {
            return fail(line, kind, &format!("{want:?}"), &format!("{got:?}"));
        }
    }
    DirectiveResult {
        line,
        kind,
        passed: true,
        expected: "match".to_string(),
        got: "match".to_string(),
    }
}

fn ret_matches(want: &WastRet<'_>, got: &Slot) -> bool {
    if let WastRet::Core(WastRetCore::Either(opts)) = want {
        return opts.iter().any(|opt| match (opt, got) {
            (
                WastRetCore::V128(pattern),
                Slot::Native(quench_runtime::native::Native::V128(bits)),
            ) => v128_matches(pattern, *bits),
            (WastRetCore::I32(v), Slot::Native(quench_runtime::native::Native::I32(g))) => v == g,
            _ => false,
        });
    }
    match (want, got) {
        (
            WastRet::Core(WastRetCore::I32(v)),
            Slot::Native(quench_runtime::native::Native::I32(g)),
        ) => v == g,
        (
            WastRet::Core(WastRetCore::I64(v)),
            Slot::Native(quench_runtime::native::Native::I64(g)),
        ) => v == g,
        (
            WastRet::Core(WastRetCore::F32(pattern)),
            Slot::Native(quench_runtime::native::Native::F32(bits)),
        ) => nan_f32(pattern, *bits),
        (
            WastRet::Core(WastRetCore::F64(pattern)),
            Slot::Native(quench_runtime::native::Native::F64(bits)),
        ) => nan_f64(pattern, *bits),
        (
            WastRet::Core(WastRetCore::RefNull(_)),
            Slot::Native(quench_runtime::native::Native::Ref(quench_runtime::native::RefVal::Null)),
        ) => true,
        (
            WastRet::Core(WastRetCore::RefExtern(Some(id))),
            Slot::Native(quench_runtime::native::Native::Ref(
                quench_runtime::native::RefVal::Extern(g),
            )),
        ) => id == g,
        (
            WastRet::Core(WastRetCore::RefExtern(None)),
            Slot::Native(quench_runtime::native::Native::Ref(
                quench_runtime::native::RefVal::Extern(_)
                | quench_runtime::native::RefVal::ExternBox(_),
            )),
        ) => true,
        (
            WastRet::Core(WastRetCore::RefHost(id)),
            Slot::Native(quench_runtime::native::Native::Ref(
                quench_runtime::native::RefVal::Host(g),
            )),
        ) => id == g,
        (
            WastRet::Core(WastRetCore::RefAny),
            Slot::Native(quench_runtime::native::Native::Ref(
                quench_runtime::native::RefVal::Host(_)
                | quench_runtime::native::RefVal::I31(_)
                | quench_runtime::native::RefVal::Struct(_)
                | quench_runtime::native::RefVal::Array(_)
                | quench_runtime::native::RefVal::Func { .. },
            )),
        ) => true,
        (
            WastRet::Core(WastRetCore::RefFunc(_)),
            Slot::Native(quench_runtime::native::Native::Ref(
                quench_runtime::native::RefVal::Func { .. },
            )),
        ) => true,
        (
            WastRet::Core(WastRetCore::RefI31),
            Slot::Native(quench_runtime::native::Native::Ref(quench_runtime::native::RefVal::I31(
                _,
            ))),
        ) => true,
        (
            WastRet::Core(WastRetCore::RefArray),
            Slot::Native(quench_runtime::native::Native::Ref(
                quench_runtime::native::RefVal::Array(_),
            )),
        ) => true,
        (
            WastRet::Core(WastRetCore::RefStruct),
            Slot::Native(quench_runtime::native::Native::Ref(
                quench_runtime::native::RefVal::Struct(_),
            )),
        ) => true,
        (
            WastRet::Core(WastRetCore::RefEq),
            Slot::Native(quench_runtime::native::Native::Ref(
                quench_runtime::native::RefVal::Array(_)
                | quench_runtime::native::RefVal::Struct(_)
                | quench_runtime::native::RefVal::I31(_),
            )),
        ) => true,
        (
            WastRet::Core(WastRetCore::V128(pattern)),
            Slot::Native(quench_runtime::native::Native::V128(bits)),
        ) => v128_matches(pattern, *bits),
        _ => false,
    }
}

fn nan_f32(pattern: &wast::core::NanPattern<wast::token::F32>, bits: u32) -> bool {
    match pattern {
        wast::core::NanPattern::CanonicalNan => bits & 0x7fff_ffff == 0x7fc0_0000,
        wast::core::NanPattern::ArithmeticNan => {
            bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x0040_0000 != 0
        }
        wast::core::NanPattern::Value(v) => v.bits == bits,
    }
}

fn nan_f64(pattern: &wast::core::NanPattern<wast::token::F64>, bits: u64) -> bool {
    match pattern {
        wast::core::NanPattern::CanonicalNan => {
            bits & 0x7fff_ffff_ffff_ffff == 0x7ff8_0000_0000_0000
        }
        wast::core::NanPattern::ArithmeticNan => {
            bits & 0x7ff0_0000_0000_0000 == 0x7ff0_0000_0000_0000
                && bits & 0x0008_0000_0000_0000 != 0
        }
        wast::core::NanPattern::Value(v) => v.bits == bits,
    }
}

fn trap_matches(got: &str, expected: &str) -> bool {
    got.contains(expected) || expected.contains(got)
}

fn unimplemented(line: usize, kind: &str) -> DirectiveResult {
    DirectiveResult {
        line,
        kind: kind.to_string(),
        passed: false,
        expected: kind.to_string(),
        got: "unimplemented".to_string(),
    }
}

fn fail(line: usize, kind: String, expected: &str, got: &str) -> DirectiveResult {
    DirectiveResult {
        line,
        kind,
        passed: false,
        expected: expected.to_string(),
        got: got.to_string(),
    }
}
