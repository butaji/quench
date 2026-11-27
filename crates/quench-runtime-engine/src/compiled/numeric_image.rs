//! A numeric function compiled to a relocatable image and linked: the one path of the live
//! numeric JIT and of ahead-of-time compilation.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// A function's numeric region lowered, before its code is produced: the specializer's output
/// for `function_id`, which both the live JIT and the ahead-of-time image compile.
struct NumericLowering {
    function: ir::Function,
    isa: std::sync::Arc<dyn cranelift_codegen::isa::TargetIsa>,
    params: usize,
    number_result: NativeNumberResult,
}

/// Lowers `function_id` as a numeric region through the shared semantic SSA specializer.
fn lower_numeric_region(program: &ResidualProgram, function_id: u32, admitted: crate::compiled::admission::CompiledUnit) -> Option<NumericLowering> {
    use quench_runtime::vm::specializer::Specializer;
    let function = program.function_get(function_id as usize)?;
    let unit = lane_unit::unit(program, None, function_id, function, &|_, _| Vec::new(), &|_| None)?;
    let isa = quench_runtime::vm::jit_backend::target_isa()?;
    let region = lane_region::RegionSpecializer { function_id, unit: &unit, admitted,
        helpers: LaneHelpers::Numeric, fields: &[],
        global_reads: &Default::default(), facts: VmFacts::STATIC, generic: &[],
        direct: &std::collections::BTreeMap::new(), frame_captures: &[], isa: &*isa,
        entry_fields: &lane_region::NO_ENTRY_FIELDS, kinds: &[], int_entries: &[], number_entries: &[], string_entries: &[], typed_captures: &[], int_captures: &[], typed_kinds: &[],
        plan: lane_region::RegionPlan { entry_bounds: true, coverage: lane_region::Coverage::Full,
            completion: lane_region::Completion::Number, views: lane_region::ElementViews::Dense } }
        .specialize(program).ok()?;
    Some(NumericLowering { params: usize::from(function.params), number_result: region.number_result?, function: region.function, isa })
}

/// Bind the numeric JIT ABI to the same semantic SSA specializer used by
/// frameless JIT bodies and linked AOT regions, without a canonical VM frame: the region's
/// image, linked to this process.
#[cfg(test)]
pub(crate) fn compile_js_typed_numeric_region(program: &ResidualProgram, function_id: u32)
    -> Option<crate::js_compiled::JsJitArtifact> {
    NumericImage::compile(program, function_id)?.load()
}

fn numeric_artifact(code: Vec<u8>, params: usize, number_result: NativeNumberResult) -> crate::js_compiled::JsJitArtifact {
    crate::js_compiled::JsJitArtifact { code, params, result: number_result }
}

pub(crate) use quench_runtime::vm::engine_types::NumericImage;

#[quench_macros::ext(NumericImageOps1)]
impl NumericImage {
    /// The image of `function_id`, `None` where the region lowers to code that needs a link
    /// this build cannot make.
    #[cfg(test)]
    pub(crate) fn compile(program: &ResidualProgram, function_id: u32) -> Option<Self> {
        Self::build(lower_numeric_region(program, function_id, crate::compiled::admission::test_unit())?, function_id, crate::compiled::admission::test_unit())
    }

    /// Lowers and compiles `function_id` on the code generator's thread, from a copy of its
    /// program that shares nothing with it (`ResidualProgram::detached`): the lowering reads
    /// only the program, so the program's thread does no more than copy the function. The image
    /// arrives when the work is done, `None` where the region does not lower or compile.
    pub(crate) fn start(program: &ResidualProgram, function_id: u32, unit: crate::compiled::admission::CompiledUnit) -> Option<quench_runtime::vm::jit_backend::OffThread<Option<Self>>> {
        program.function_get(function_id as usize)?;
        let detached = program.detached(&[function_id]);
        Some(quench_runtime::vm::jit_backend::OffThread::start((detached, function_id, unit), |(detached, function_id, unit)| Self::build(lower_numeric_region(&detached, function_id, unit)?, function_id, unit)))
    }

    fn build(lowering: NumericLowering, function_id: u32, unit: crate::compiled::admission::CompiledUnit) -> Option<Self> {
        let NumericLowering { function, isa, params, number_result } = lowering;
        let dump = native_dump_directory().map(|_| function.to_string());
        let code = quench_runtime::vm::jit_backend::compile_function(function, &*isa, Some(unit.code_ceiling()), |compiled, function| {
            quench_runtime::vm::jit_backend::relocatable_code::RelocatableCode::extract(compiled, function).ok_or(NativeCompileRejection::Relocations)
        }).ok()?;
        if let (Some(directory), Some(clif)) = (native_dump_directory(), dump) {
            dump_native_bytes(directory, DumpSite::Body { function: function_id, scalar: None }, &code.code, None, &clif);
        }
        Some(Self { code, params, number_result })
    }

    /// The artifact this image loads as, its symbols resolved by `resolve`.
    pub(crate) fn load(&self) -> Option<crate::js_compiled::JsJitArtifact> {
        // Absolute 8-byte fields alone: the code is address-independent until it is placed.
        let code = self.code.link(0, lane_region::numeric_symbol).ok()?;
        Some(numeric_artifact(code, self.params, self.number_result))
    }
}

#[cfg(test)]
#[path = "numeric_image/ahead.rs"]
pub(crate) mod ahead;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiled::direct_call::direct_call;

    #[test]
    fn numeric_image_loads_as_the_function_the_jit_publishes() {
        let program = quench_runtime::Engine::specialize("function mix(a,b){a=a|0;b=b|0;return (a*3+b)|0;}", "numeric-image.js").unwrap();
        let image = NumericImage::compile(&program, 1).expect("a plain numeric function has an image");
        assert!(image.code.relocations.is_empty(), "a function that calls no helper needs no symbol");
        let compiled = CompiledJsFunction::from_artifact(image.load().expect("an image without symbols loads")).unwrap();
        let interrupt = quench_runtime::ExecutionInterrupt::default();
        for (a, b) in [(0_i32, 0_i32), (1, 2), (-7, 5), (100_000_000, 1), (-100_000_000, -1)] {
            let expected = a.wrapping_mul(3).wrapping_add(b);
            let result = direct_call(&compiled, &[Value::integer(a), Value::integer(b)], &interrupt);
            assert!(matches!(result, Some(value) if value.to_bits() == Value::integer(expected).to_bits()), "a={a},b={b}");
        }
    }


    #[test]
    fn numeric_loop_names_its_interrupt_helper_by_symbol_and_runs_once_linked() {
        let source = "function total(n){n=n|0;var s=0;for(var i=0;i<n;i=(i+1)|0){s=(s+i)|0;}return s|0;}";
        let program = quench_runtime::Engine::specialize(source, "numeric-image-loop.js").unwrap();
        let image = NumericImage::compile(&program, 1).expect("a numeric loop has an image");
        assert!(!image.code.relocations.is_empty(), "the loop's interrupt poll names its helper by symbol");
        let linked = CompiledJsFunction::from_artifact(image.load().expect("its symbols resolve")).unwrap();
        let interrupt = quench_runtime::ExecutionInterrupt::default();
        for n in [-3_i32, 0, 1, 10, 1000] {
            let expected = (0..n.max(0)).sum::<i32>();
            let result = direct_call(&linked, &[Value::integer(n)], &interrupt);
            assert!(matches!(result, Some(value) if value.to_bits() == Value::integer(expected).to_bits()), "n={n}");
        }
        interrupt.request_termination();
        assert!(direct_call(&linked, &[Value::integer(1_000_000)], &interrupt).is_none(), "the linked poll sees the interrupt");
    }
}
