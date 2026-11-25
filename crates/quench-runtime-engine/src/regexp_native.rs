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

/// Whether this thread's hot regexp programs run natively: only under a compiling mode. The
/// stencil tier stitches a program after `stitch_runs` searches, the Cranelift tiers compile it
/// after `native_runs`, each where the mode runs it (`compiler`).
pub(crate) fn enable(enabled: bool, (stitch_runs, native_runs): (u32, u32)) {
    quench_regexp::backtrack::native::set_native_compiler(compiler(stitch_runs, native_runs));
    quench_regexp::backtrack::native::enable_native_search(enabled);
}

/// The compiler `quench_regexp` asks for code: the tiers as the mode climbs them, as a
/// JavaScript function climbs them (`admission::Tier`). The stencil tier first where it runs and
/// the target has the regexp rows, then the Cranelift tiers where they run; a `--b-jit` target
/// without the rows compiles with Cranelift, as it did before the rows.
fn compiler(stitch_runs: u32, native_runs: u32) -> NativeCompiler {
    use crate::compiled::admission::{tier_runs, Tier};
    let stencils = tier_runs(&[Tier::Base]) && stitch::available();
    let cranelift = tier_runs(Tier::CRANELIFT);
    match (stencils, cranelift) {
        (true, true) => NativeCompiler { runs: stitch_runs, start: stitch_start, optimize: Some((native_runs, start)) },
        (true, false) => NativeCompiler { runs: stitch_runs, start: stitch_start, optimize: None },
        (false, _) => NativeCompiler { runs: native_runs, start, optimize: None },
    }
}

/// The unit of a regexp program's compile, which the one specializer clears.
fn unit() -> Option<CompiledUnit> {
    use crate::compiled::admission::{specialize, UnitFacts, UnitKind, UnitRequest};
    specialize(UnitRequest { kind: UnitKind::Regexp, facts: UnitFacts::default() }).compiled()
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
