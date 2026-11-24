//! The compile of a hot regexp program: a unit the one specializer clears (`UnitKind::Regexp`),
//! its native search (`staged::regexp::emit_search`) built on the shared builder start
//! (`jit_backend::builder::build`) and compiled and placed as every other unit is.
#[allow(unused_imports)]
use crate::ext_traits::*;
use crate::compiled::CodeMemory;
use quench_runtime::vm::jit_backend::{builder::build, compile_function, target_isa, NativeCompileRejection, OffThread};
use crate::compiled::admission::CompiledUnit;
use crate::compiled::emit_machine::raw;
use std::task::Poll;
use cranelift_codegen::ir::{self, types, MemFlagsData};
use cranelift_codegen::isa::TargetIsa;
use quench_regexp::backtrack::native::{NativeBuild, NativeCode, NativeCompiler, NativeFn, NativePlan};
use quench_runtime::staged::regexp::{emit_search, PARAMETERS};

use quench_runtime::vm::tier_policy::REGEXP_NATIVE_MAX_CODE_BYTES;
/// Whether this thread's hot regexp programs run natively: only under a compiling mode.
pub(crate) fn enable(enabled: bool, runs: u32) {
    quench_regexp::backtrack::native::set_native_compiler(compiler(runs));
    quench_regexp::backtrack::native::enable_native_search(enabled);
}

/// The compiler `quench_regexp` asks for code.
fn compiler(runs: u32) -> NativeCompiler {
    NativeCompiler { runs, start }
}

/// Compiles `plan` on the code generator's thread, once the one specializer clears its unit; the
/// program polls for the code and keeps the interpreter meanwhile.
fn start(plan: NativePlan) -> NativeBuild {
    use crate::compiled::admission::{specialize, UnitFacts, UnitKind, UnitRequest};
    let Some(unit) = specialize(UnitRequest { kind: UnitKind::Regexp, facts: UnitFacts::default() }).compiled() else {
        return Box::new(|| Poll::Ready(None));
    };
    let mut pending = OffThread::start((plan, unit), |(plan, unit)| compile(&plan, unit));
    Box::new(move || match pending.poll() {
        Poll::Pending => Poll::Pending,
        Poll::Ready(bytes) => Poll::Ready(bytes.flatten().and_then(|bytes| publish(&bytes))),
    })
}

/// The machine code of `plan`, whose unit the specializer cleared.
fn compile(plan: &NativePlan, _unit: CompiledUnit) -> Option<Vec<u8>> {
    let isa = target_isa()?;
    if isa.pointer_type() != types::I64 {
        return None;
    }
    let mut function = ir::Function::with_name_signature(ir::UserFuncName::default(), signature(&*isa));
    // The shared builder start: entered at its first block with the signature's parameters,
    // sealed and finalized after the search's rows.
    build(&mut function, &*isa, |builder, parameters| {
        let entry = builder.current_block().expect("`build` enters the first block");
        let parameters: [ir::Value; PARAMETERS] = parameters.try_into().unwrap_or_else(|_| unreachable!("`PARAMETERS` parameters"));
        emit_search(plan, raw(builder), MemFlagsData::trusted(), entry, parameters);
    });
    compile_function(function, &*isa, Some(REGEXP_NATIVE_MAX_CODE_BYTES), |compiled, _| {
        if !compiled.buffer.relocs().is_empty() {
            return Err(NativeCompileRejection::Relocations);
        }
        Ok(compiled.code_buffer().to_vec())
    })
    .ok()
}

/// `code` in executable memory, as a native search.
fn publish(code: &[u8]) -> Option<NativeCode> {
    let memory = CodeMemory::new(code)?;
    // SAFETY: the code is a function of `NativeFn`'s signature, which `emit` builds.
    let entry = unsafe { std::mem::transmute::<*mut u8, NativeFn>(memory.pointer()) };
    Some(NativeCode { entry, keep: Box::new(memory) })
}

fn signature(isa: &dyn TargetIsa) -> ir::Signature {
    crate::compiled::call_signature::word_signature(isa.default_call_conv(), PARAMETERS, 1, types::I64)
}

#[cfg(test)]
pub(crate) mod tests;
