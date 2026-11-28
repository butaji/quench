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
use quench_runtime::vm::tier_policy::regexp_compiles;

/// Whether this thread's regexp programs may run natively: only under a compiling mode. A pattern
/// gets code where `tier_policy::regexp_compiles` picks it (`compiler`); every other pattern runs
/// the shared matcher. Without the JIT's routes (`jit_routes`) no program compiles.
pub(crate) fn enable(enabled: bool) {
    match crate::jit_routes::jit() {
        Some(routes) => (routes.regexp)(enabled),
        None => quench_regexp::backtrack::native::enable_native_search(false),
    }
}

/// `enable`'s route, which `jit_routes` installs.
pub(crate) fn enable_compiler(enabled: bool) {
    quench_regexp::backtrack::native::set_native_compiler(compiler());
    quench_regexp::backtrack::native::enable_native_search(enabled);
}

/// The compiler `quench_regexp` asks for code: the pick (`regexp_compiles`) and the tier that
/// builds the code. The Cranelift u-jit builds it where it runs; a target with the stencil regexp
/// rows and no Cranelift tiers stitches it from those rows.
fn compiler() -> NativeCompiler {
    use quench_runtime::vm::tier_policy::{base_runs, cranelift_runs};
    let stencils = base_runs() && stitch::available();
    let cranelift = cranelift_runs();
    let start: fn(NativePlan) -> NativeBuild = if stencils && !cranelift { stitch_start } else { start };
    NativeCompiler { compiles: regexp_compiles, start }
}

/// The unit of a regexp program's compile, which the one specializer clears.
fn unit() -> Option<CompiledUnit> {
    use crate::compiled::admission::{specialize, UnitFacts, UnitKind, UnitRequest};
    specialize(UnitRequest { kind: UnitKind::Regexp, facts: UnitFacts::default() })
}

/// The stencil tier's search of `plan`, stitched at once (`stitch`).
fn stitch_start(plan: NativePlan) -> NativeBuild {
    let mut code = unit().and_then(|_| stitch::stitch(&plan)).and_then(stitch::Stitched::publish);
    Box::new(move || Poll::Ready(code.take()))
}

/// Compiles `plan` on the code generator's thread, once the one specializer clears its unit; the
/// program polls for the code and keeps its current tier meanwhile.
fn start(plan: NativePlan) -> NativeBuild {
    let Some(unit) = unit() else {
        return Box::new(|| Poll::Ready(None));
    };
    let mut pending = OffThread::start((plan, unit), |(plan, unit)| compile(&plan, unit));
    Box::new(move || match pending.poll() {
        Poll::Pending => Poll::Pending,
        Poll::Ready(bytes) => Poll::Ready(bytes.flatten().and_then(|bytes| publish(&bytes))),
    })
}

/// The native search of `plan` for an ahead image (`aot_image::regexps`), compiled as a hot program's.
pub(crate) fn ahead_code(plan: &NativePlan) -> Option<Vec<u8>> {
    compile(plan, unit()?)
}

/// `code`, an ahead image's search (`ahead_code`), placed in executable memory: its entry, and the memory it keeps.
pub(crate) fn place_ahead(code: &[u8]) -> Option<(NativeFn, std::rc::Rc<dyn std::any::Any>)> {
    let NativeCode { entry, keep } = publish(code)?;
    Some((entry, std::rc::Rc::from(keep)))
}

/// The machine code of `plan`, whose unit the specializer cleared.
fn compile(plan: &NativePlan, unit: CompiledUnit) -> Option<Vec<u8>> {
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
    compile_function(function, &*isa, Some(unit.code_ceiling()), |compiled, _| {
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

/// The stencil tier's search: the program's rows stitched (`quench-bjit`'s regexp family).
mod stitch;

#[cfg(test)]
pub(crate) mod tests;
