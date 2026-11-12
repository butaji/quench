//! A numeric function compiled to a relocatable image and linked: the one path of the live
//! numeric JIT and of ahead-of-time compilation.
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
fn lower_numeric_region(program: &ResidualProgram, function_id: u32) -> Option<NumericLowering> {
    use crate::vm::specializer::Specializer;
    let function = program.function_get(function_id as usize)?;
    let unit = lane_unit::unit(program, function_id, function, &|_, _| Vec::new(), &|_| None)?;
    let isa = crate::vm::jit_backend::target_isa()?;
    let region = lane_region::RegionSpecializer { function_id, unit: &unit,
        helpers: LaneHelpers::Numeric, fields: &std::collections::BTreeMap::new(),
        global_reads: &Default::default(), facts: VmFacts::STATIC, generic: &[],
        direct: &std::collections::BTreeMap::new(), frame_captures: &[], isa: &*isa,
        entry_fields: &lane_region::NO_ENTRY_FIELDS, number_reads: &[], int_entries: &[], typed_captures: &[], int_captures: &[],
        plan: lane_region::RegionPlan { coverage: lane_region::Coverage::Full,
            completion: lane_region::Completion::Number, views: lane_region::ElementViews::Dense } }
        .specialize(program).ok()?;
    Some(NumericLowering { params: usize::from(function.params), number_result: region.number_result?, function: region.function, isa })
}

/// Bind the numeric JIT ABI to the same semantic SSA specializer used by
/// frameless JIT bodies and linked AOT regions, without a canonical VM frame: the region's
/// image, linked to this process.
#[cfg(test)]
pub(in crate::vm) fn compile_js_typed_numeric_region(program: &ResidualProgram, function_id: u32)
    -> Option<crate::vm::js_compiled::JsJitArtifact> {
    NumericImage::compile(program, function_id)?.load()
}

fn numeric_artifact(code: Vec<u8>, params: usize, number_result: NativeNumberResult) -> crate::vm::js_compiled::JsJitArtifact {
    crate::vm::js_compiled::JsJitArtifact { code, params, result: number_result }
}

/// A numeric function compiled ahead of time: position-independent code with the relocations
/// that link it, and what its entry returns. It is the same lowering as the live JIT's
/// (`lower_numeric_region`); loading links it into the artifact the JIT publishes.
#[derive(Debug)]
pub(in crate::vm) struct NumericImage {
    code: crate::vm::jit_backend::relocatable_code::RelocatableCode,
    params: usize,
    number_result: NativeNumberResult,
}

impl NumericImage {
    /// The image of `function_id`, `None` where the region lowers to code that needs a link
    /// this build cannot make.
    #[cfg(test)]
    pub(in crate::vm) fn compile(program: &ResidualProgram, function_id: u32) -> Option<Self> {
        Self::build(lower_numeric_region(program, function_id)?, function_id)
    }

    /// Lowers `function_id` here and compiles it on the code generator's thread: the image
    /// arrives when the work is done, `None` where the compile declined.
    pub(in crate::vm) fn start(program: &ResidualProgram, function_id: u32) -> Option<crate::vm::jit_backend::Pending<Option<Self>>> {
        let lowering = lower_numeric_region(program, function_id)?;
        Some(crate::vm::jit_backend::run_in_background(move || Self::build(lowering, function_id)))
    }

    fn build(lowering: NumericLowering, function_id: u32) -> Option<Self> {
        let NumericLowering { function, isa, params, number_result } = lowering;
        let dump = native_dump_directory().map(|_| function.to_string());
        let code = crate::vm::jit_backend::compile_function(function, &*isa, Some(crate::vm::js_compiled::MAX_CODE_BYTES), |compiled, function| {
            crate::vm::jit_backend::relocatable_code::RelocatableCode::extract(compiled, function).ok_or(NativeCompileRejection::Relocations)
        }).ok()?;
        if let (Some(directory), Some(clif)) = (native_dump_directory(), dump) {
            dump_native_bytes(directory, function_id, &code.code, None, &clif);
        }
        Some(Self { code, params, number_result })
    }

    /// The artifact this image loads as, its symbols resolved by `resolve`.
    pub(in crate::vm) fn load(&self) -> Option<crate::vm::js_compiled::JsJitArtifact> {
        // Absolute 8-byte fields alone: the code is address-independent until it is placed.
        let code = self.code.link(0, lane_region::numeric_symbol).ok()?;
        Some(numeric_artifact(code, self.params, self.number_result))
    }
}

#[cfg(test)]
#[path = "numeric_image/ahead.rs"]
mod ahead;

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::direct_call::direct_call;

    #[test]
    fn numeric_image_loads_as_the_function_the_jit_publishes() {
        let program = crate::Engine::specialize("function mix(a,b){a=a|0;b=b|0;return (a*3+b)|0;}", "numeric-image.js").unwrap();
        let image = NumericImage::compile(&program, 1).expect("a plain numeric function has an image");
        assert!(image.code.relocations.is_empty(), "a function that calls no helper needs no symbol");
        let compiled = CompiledJsFunction::from_artifact(image.load().expect("an image without symbols loads")).unwrap();
        let interrupt = crate::ExecutionInterrupt::default();
        for (a, b) in [(0_i32, 0_i32), (1, 2), (-7, 5), (100_000_000, 1), (-100_000_000, -1)] {
            let expected = a.wrapping_mul(3).wrapping_add(b);
            let result = direct_call(&compiled, &[Value::integer(a), Value::integer(b)], &interrupt);
            assert!(matches!(result, Some(value) if value.to_bits() == Value::integer(expected).to_bits()), "a={a},b={b}");
        }
    }


    #[test]
    fn numeric_loop_names_its_interrupt_helper_by_symbol_and_runs_once_linked() {
        let source = "function total(n){n=n|0;var s=0;for(var i=0;i<n;i=(i+1)|0){s=(s+i)|0;}return s|0;}";
        let program = crate::Engine::specialize(source, "numeric-image-loop.js").unwrap();
        let image = NumericImage::compile(&program, 1).expect("a numeric loop has an image");
        assert!(!image.code.relocations.is_empty(), "the loop's interrupt poll names its helper by symbol");
        let linked = CompiledJsFunction::from_artifact(image.load().expect("its symbols resolve")).unwrap();
        let interrupt = crate::ExecutionInterrupt::default();
        for n in [-3_i32, 0, 1, 10, 1000] {
            let expected = (0..n.max(0)).sum::<i32>();
            let result = direct_call(&linked, &[Value::integer(n)], &interrupt);
            assert!(matches!(result, Some(value) if value.to_bits() == Value::integer(expected).to_bits()), "n={n}");
        }
        interrupt.request_termination();
        assert!(direct_call(&linked, &[Value::integer(1_000_000)], &interrupt).is_none(), "the linked poll sees the interrupt");
    }
}
