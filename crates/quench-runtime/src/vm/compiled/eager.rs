//! Eager native publication: every requested body compiled at once with no code-cache
//! budget, its frozen whole-program facts, the per-function tier dispatcher and the
//! general-dispatch fallback tables, addressed directly because nothing is linked.
use super::*;

/// Eager publication has no code-cache admission budget: all requested bodies
/// must compile or the whole program fails. Owners release code and operands together.
#[cfg(feature = "aot")]
pub(in crate::vm) fn compile_eager_native<H: Host>(
    program: ResidualProgram, options: super::native_graph::NativeJsCodegenOptions,
) -> Result<crate::NativeJsProgram<H>, String> {
    #[cfg(feature = "jit")]
    if options.backend == super::native_graph::NativeJsBackend::Stencil && super::native_stencil::available() {
        return super::native_stencil::compile_program(program).map(|(native, _)| native);
    }
    use super::specialize::Specializer;
    program.freeze_native_diagnostics();
    let isa = super::jit_backend::target_isa_for("speed", super::jit_backend::CodePublication::RelocatableObject)
        .ok_or("native target unavailable")?;
    // The static call hints are the same facts the linked image derives from the
    // program; the eager publisher must fold them too, or its typed regions would
    // be a second, weaker lowering of the interpreter.
    let targets = super::compiled::static_call_targets(&program);
    let globals = frozen_globals(&program);
    let frozen_field_facts = frozen_fields(&program);
    let constant_slots: Box<[Value]> = vec![Value::UNDEFINED; program.constants.len()].into_boxed_slice();
    let constant_block = Rc::new(std::cell::RefCell::new(constant_slots));
    let constant_base = constant_block.borrow().as_ptr() as usize;
    // The image's invalidation word: zero while every whole-program assumption holds.
    let invalidation = crate::code_dependencies::Invalidation::default();
    let last_tier = Rc::new(std::cell::Cell::new(0u64));
    let extension = EagerCraneliftExtension::<H> { program: &program, targets, globals: &globals,
        fields: &frozen_field_facts, constants: constant_base, isa, options, entries: Vec::new(),
        owners: EagerOwners::default(), counts: Default::default(), invalidation: invalidation.clone(),
        last_tier: last_tier.as_ptr() as usize, host: std::marker::PhantomData };
    let (entries, owners, mut counts) = super::specialize::CallThreaded::frozen(extension).specialize(&program)?;
    counts.frozen_globals = globals.len();
    counts.frozen_fields = frozen_field_facts.len();
    let frozen = frozen_assumptions(&owners, (&globals, &frozen_field_facts), constant_block, &invalidation, last_tier);
    // SAFETY: every entry was lowered from its exact function for host H.
    unsafe { crate::NativeJsProgram::from_entries_with_counts(program, entries, Rc::new(owners), counts, Some(frozen)) }
}

/// The image's whole-program assumptions: the facts its code folded (held method atoms,
/// globals, prototype fields) and the words it reads them through.
#[cfg(feature = "aot")]
fn frozen_assumptions(owners: &EagerOwners, (globals, fields): (&super::frozen_facts::FrozenValues, &super::frozen_facts::FrozenValues),
    constants: Rc<std::cell::RefCell<Box<[Value]>>>, invalidation: &crate::code_dependencies::Invalidation,
    last_tier: Rc<std::cell::Cell<u64>>) -> crate::vm::native_program::FrozenAssumptions {
    crate::vm::native_program::FrozenAssumptions {
        invalidation: owners.invalidation.clone(),
        atoms: owners.frozen_atoms.iter().collect(),
        globals: globals.atoms().collect(),
        fields: fields.atoms().collect(),
        constants,
        watch: Rc::new(crate::code_dependencies::PublishedCode::with_invalidation((), invalidation.clone())),
        last_tier,
    }
}

/// Everything the eager entries address: code, operand arrays and the per-PC
/// semantic-entry tables of the fallback bodies.
#[cfg(feature = "aot")]
#[derive(Default)]
pub(super) struct EagerOwners {
    functions: Vec<(Vec<CodeMemory>, Box<[super::specialize::InstructionSite]>)>,
    operand_arrays: Vec<Box<[super::NativeJsOperands]>>,
    entry_tables: Vec<Box<[*const ()]>>,
    /// The word the image's stable facts are checked against.
    invalidation: crate::code_dependencies::Invalidation,
    /// Method atoms the image holds unnamed (`StableFacts::atoms`).
    frozen_atoms: crate::vm::AtomSet,
}

/// The call-threaded generating extension for live Cranelift code: semantic
/// entries and operand records are bound by address, so nothing is linked.
#[cfg(feature = "aot")]
struct EagerCraneliftExtension<'p, H> {
    program: &'p ResidualProgram,
    targets: Vec<Vec<lane_unit::StaticTargets>>,
    /// Whole-program globals the frozen regions may fold.
    globals: &'p super::frozen_facts::FrozenValues,
    /// Whole-program prototype values the frozen regions may fold.
    fields: &'p super::frozen_facts::FrozenValues,
    /// The address of the image's constant block.
    constants: usize,
    isa: std::sync::Arc<dyn cranelift_codegen::isa::TargetIsa>,
    options: super::native_graph::NativeJsCodegenOptions,
    entries: Vec<crate::NativeJsFunctionEntry>,
    owners: EagerOwners,
    counts: crate::NativeJsPublicationCounts,
    invalidation: crate::code_dependencies::Invalidation,
    /// The word the tier dispatcher records the chosen tier in.
    last_tier: usize,
    host: std::marker::PhantomData<fn(&mut H)>,
}

#[cfg(feature = "aot")]
impl<'p, H: Host> super::specialize::CallThreadedExtension for EagerCraneliftExtension<'p, H> {
    type Residual = (Vec<crate::NativeJsFunctionEntry>, EagerOwners, crate::NativeJsPublicationCounts);

    fn function(&mut self, function_id: u32, sites: Vec<super::specialize::InstructionSite>) -> Result<(), String> {
        let isa = self.isa.clone();
        let isa = &*isa;
        let sites = sites.into_boxed_slice();
        self.counts.functions += 1;
        self.counts.wide_frame_functions += usize::from(self.program.function_get(function_id as usize)
            .is_some_and(|function| function.registers > crate::compile::liveness::MAX_MAP_REGISTERS));
        self.counts.semantic_entry_sites += sites.iter().filter(|site| site.calls_semantic_entry()).count();
        self.counts.static_transfers += sites.iter().filter(|site| !site.calls_semantic_entry()).count();
        let code = self.lower_function(function_id, &sites, isa)?;
        // SAFETY: the final body or wrapper implements the public native entry
        // ABI; owner retains every private region and immutable operand array.
        self.entries.push(unsafe { std::mem::transmute::<*mut u8, crate::NativeJsFunctionEntry>(code.last().expect("native entry compiled").pointer()) });
        self.owners.functions.push((code, sites));
        Ok(())
    }

    fn finish(mut self) -> Result<Self::Residual, String> {
        // The owners carry the same word the code reads, so retiring the image's
        // watch is what the compiled invalidation points observe.
        self.owners.invalidation = self.invalidation.clone();
        Ok((self.entries, self.owners, self.counts))
    }
}

#[cfg(feature = "aot")]
impl<'p, H: Host> EagerCraneliftExtension<'p, H> {
    /// The largest body first: a function over the region budget is split into
    /// private regions with a dispatch wrapper; every other function takes the
    /// general-dispatch baseline or the call-threaded body.
    fn lower_function(&mut self, function_id: u32, sites: &[super::specialize::InstructionSite],
        isa: &dyn cranelift_codegen::isa::TargetIsa,
    ) -> Result<Vec<CodeMemory>, String> {
        let Some(regions) = self.options.regions(sites.len()) else {
            return self.lower_baseline_or_threaded(function_id, sites, isa);
        };
        self.counts.call_threaded += 1;
        self.counts.tier_call_threaded += 1;
        self.lower_regions(function_id, sites, isa, &regions)
    }

    fn lower_baseline_or_threaded(&mut self, function_id: u32, sites: &[super::specialize::InstructionSite],
        isa: &dyn cranelift_codegen::isa::TargetIsa,
    ) -> Result<Vec<CodeMemory>, String> {
        match self.lower_general_dispatch(function_id, sites, isa)? {
            Some(lowered) => {
                self.counts.general_dispatch += 1;
                self.counts.typed_regions += lowered.typed_regions;
                self.counts.scalar_regions += lowered.scalar_regions;
                if lowered.typed_regions > 0 { self.counts.tier_specialized += 1; }
                else { self.counts.tier_general += 1; }
                Ok(lowered.code)
            }
            None => {
                self.counts.call_threaded += 1;
                self.counts.tier_call_threaded += 1;
                Ok(vec![compile_eager_body::<H>(sites, function_id, isa, None)?])
            }
        }
    }

    fn lower_regions(&mut self, function_id: u32, sites: &[super::specialize::InstructionSite],
        isa: &dyn cranelift_codegen::isa::TargetIsa, regions: &[std::ops::Range<usize>],
    ) -> Result<Vec<CodeMemory>, String> {
        let mut code = Vec::with_capacity(regions.len() + 1);
        for region in regions.iter().cloned() {
            code.push(compile_eager_body::<H>(sites, function_id, isa, Some(region))?);
        }
        let mut signature = None;
        let wrapper = super::native_graph::lower_region_dispatch(function_id, sites.len(),
            self.options.region_instruction_budget.get(), regions.len(), isa,
            |builder, index, context, pc, result| {
                let signature = *signature.get_or_insert_with(|| builder.import_signature(
                    native_entry_signature(isa.pointer_type(), isa.default_call_conv())));
                let target = builder.ins().iconst(isa.pointer_type(), code[index].pointer() as usize as i64);
                let arguments = super::native_graph::region_call_arguments(builder, isa, context, pc, result);
                let call = builder.ins().call_indirect(signature, target, &arguments);
                builder.inst_results(call)[0]
            })?;
        code.push(publish_eager_graph(wrapper, isa)?);
        Ok(code)
    }

    /// The same residual the linked publisher builds: general dispatch with
    /// scalar and typed regions inlined, and a per-PC semantic-entry fallback.
    /// `None` leaves a function the general baseline does not admit to the
    /// call-threaded body.
    fn lower_general_dispatch(&mut self, function_id: u32, sites: &[super::specialize::InstructionSite],
        isa: &dyn cranelift_codegen::isa::TargetIsa,
    ) -> Result<Option<EagerLowering>, String> {
        let program = self.program;
        let Some(function) = program.function_get(function_id as usize) else { return Ok(None) };
        // The root function and bodies outside the baseline keep the
        // call-threaded body, exactly as the linked publisher does.
        if function_id == 0 || !js_general_dispatch_candidate(program, function) {
            return Ok(None);
        }
        let (fallback, operands, table) = self.lower_fallback(function_id, sites, function.code.len(), isa)?;
        let scalar = lower_static_scalar_regions(program, function_id, isa);
        let scalar_entries: Vec<_> = scalar.iter().map(|region|
            StaticRegionEntry { range: region.range.clone(), fields: &region.fields, function: &region.function }).collect();
        let units = self.frozen_units(program, function_id, isa);
        let typed_entries: Vec<_> = units.iter().map(|unit|
            StaticTypedRegionEntry { header: unit.header, function: &unit.function }).collect();
        // T2: general dispatch with the typed regions; T1: the same baseline without
        // them; T0: the call-threaded body. The dispatcher reads the image's invalidation
        // word and selects, all three bodies already resident.
        let callout = JsGeneralCallout::Address(fallback.pointer() as usize);
        let Some((ladder, typed, scalars)) = self.lower_ladder(program, function_id, sites, isa,
            callout, &scalar_entries, &typed_entries) else { return Ok(None) };
        let mut code = vec![fallback];
        code.extend(ladder);
        self.owners.operand_arrays.push(operands);
        self.owners.entry_tables.push(table);
        Ok(Some(EagerLowering { code, typed_regions: typed, scalar_regions: scalars }))
    }

    /// T2 (typed), T1 (general) and T0 (call-threaded) resident, with the dispatcher as
    /// the entry. `None` where the baseline does not lower the function at all.
    #[allow(clippy::too_many_arguments)]
    fn lower_ladder(&mut self, program: &ResidualProgram, function_id: u32,
        sites: &[super::specialize::InstructionSite], isa: &dyn cranelift_codegen::isa::TargetIsa,
        callout: JsGeneralCallout, scalar_entries: &[StaticRegionEntry<'_>],
        typed_entries: &[StaticTypedRegionEntry<'_>],
    ) -> Option<(Vec<CodeMemory>, usize, usize)> {
        let typed = typed_entries.len();
        let scalars = scalar_entries.len();
        let (specialized, _) = lower_js_general_dispatch(program, function_id, isa,
            callout, scalar_entries, typed_entries).ok()?;
        let (general, _) = lower_js_general_dispatch(program, function_id, isa,
            callout, &[], &[]).ok()?;
        let specialized = publish_eager_graph(specialized, isa).ok()?;
        let general = publish_eager_graph(general, isa).ok()?;
        let threaded = compile_eager_body::<H>(sites, function_id, isa, None).ok()?;
        let dispatch = lower_tier_dispatch(function_id, threaded.pointer(), general.pointer(),
            specialized.pointer(), self.invalidation.address(), self.last_tier, isa).ok()?;
        let mut ladder = vec![specialized, general, threaded, publish_eager_graph(dispatch, isa).ok()?];
        ladder.shrink_to_fit();
        Some((ladder, typed, scalars))
    }

    /// The static typed regions under the image's whole-program facts: no method
    /// cache names an atom yet, and every folded Stable fact is checked against the
    /// image's invalidation word.
    fn frozen_units(&mut self, program: &ResidualProgram, function_id: u32,
        isa: &dyn cranelift_codegen::isa::TargetIsa,
    ) -> Vec<StaticTypedRegion> {
        let unnamed_atoms = super::method_cache::AtomBits::default();
        let facts = VmFacts { frozen: true, frozen_fields: Some(self.fields), frozen_constants: Some(self.constants),
            online: Some(OnlineFacts {
            method_cache_atoms: &unnamed_atoms, invalidation: self.invalidation.address(), constants: &[],
        }), ..VmFacts::STATIC };
        let function = program.function(function_id as usize);
        let reads = frozen_global_reads(function, self.globals);
        let units = lower_static_typed_regions_process::<H>(program, function_id, &self.targets, isa, facts, &reads);
        for unit in &units {
            self.owners.frozen_atoms.extend(unit.stable.atoms.iter().copied());
        }
        units
    }

    fn lower_fallback(&mut self, _function_id: u32, sites: &[super::specialize::InstructionSite],
        instruction_count: usize, isa: &dyn cranelift_codegen::isa::TargetIsa,
    ) -> Result<(CodeMemory, Box<[super::NativeJsOperands]>, Box<[*const ()]>), String> {
        let operands: Box<[super::NativeJsOperands]> = sites.iter().map(|site| site.operands).collect();
        let table: Box<[*const ()]> = sites.iter().map(|site|
            site.op.native_semantic_entry::<H>() as *const ()).collect();
        // Addresses are taken from the boxes `EagerOwners` keeps alive; moving a
        // box does not move its heap buffer.
        let fallback = publish_eager_graph(
            lower_eager_fallback(instruction_count, table.as_ptr(), operands.as_ptr(), isa)?, isa)?;
        Ok((fallback, operands, table))
    }
}

/// The AOT tier the dispatcher selected (0 call-threaded, 1 general, 2 specialized).
#[cfg(feature = "aot")]
const TIER_THREADED: i64 = 0;
#[cfg(feature = "aot")]
const TIER_GENERAL: i64 = 1;
#[cfg(feature = "aot")]
const TIER_SPECIALIZED: i64 = 2;

/// The image's invalidation word, as the dispatcher reads it.
#[cfg(feature = "aot")]
fn invalidated_word(builder: &mut FunctionBuilder<'_>, isa: &dyn cranelift_codegen::isa::TargetIsa,
    invalidation: usize,
) -> ir::Value {
    let address = builder.ins().iconst(isa.pointer_type(), invalidation as i64);
    builder.ins().load(types::I64, MemFlagsData::trusted(), address, 0)
}

/// The dispatcher's entry: branch on the invalidation word and return the observation
/// address with the call signature.
#[cfg(feature = "aot")]
fn emit_tier_prologue(
    builder: &mut FunctionBuilder<'_>,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    word: ir::Value,
    general_block: ir::Block,
    specialized_block: ir::Block,
    last_tier: usize,
) -> (ir::SigRef, ir::Value) {
    let signature = builder.import_signature(native_entry_signature(isa.pointer_type(), isa.default_call_conv()));
    let observation = builder.ins().iconst(isa.pointer_type(), last_tier as i64);
    let invalidated = builder.ins().icmp_imm_u(IntCC::NotEqual, word, 0);
    builder.ins().brif(invalidated, general_block, &[], specialized_block, &[]);
    (signature, observation)
}

/// Calls `body` from its tier's dispatcher block and returns its status.
#[cfg(feature = "aot")]
fn emit_tier_body(
    builder: &mut FunctionBuilder<'_>,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    observation: ir::Value,
    signature: ir::SigRef,
    body: *mut u8,
    tier: i64,
    params: &[ir::Value],
) {
    let tier = builder.ins().iconst(types::I64, tier);
    builder.ins().store(MemFlagsData::trusted(), tier, observation, 0);
    let target = builder.ins().iconst(isa.pointer_type(), body as usize as i64);
    let call = builder.ins().call_indirect(signature, target, params);
    let status = builder.inst_results(call)[0];
    builder.ins().return_(&[status]);
}

/// Selects one of the three resident bodies: the specialized one while the image's
/// invalidation word is zero, the general baseline once it is set, and the call-threaded
/// body when the policy word names neither. Records the tier in `last_tier`.
#[cfg(feature = "aot")]
fn lower_tier_dispatch(
    function_id: u32,
    threaded: *mut u8,
    general: *mut u8,
    specialized: *mut u8,
    invalidation: usize,
    last_tier: usize,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
) -> Result<ir::Function, String> {
    let mut native = ir::Function::with_name_signature(
        ir::UserFuncName::user(super::native_object::NATIVE_FUNCTION_NAMESPACE, function_id),
        native_entry_signature(isa.pointer_type(), isa.default_call_conv()),
    );
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let params = builder.block_params(entry).to_vec();
    let (specialized_block, general_block, threaded_block) =
        (builder.create_block(), builder.create_block(), builder.create_block());
    let word = invalidated_word(&mut builder, isa, invalidation);
    let (signature, observation) = emit_tier_prologue(&mut builder, isa, word,
        general_block, specialized_block, last_tier);
    for (block, body, tier) in [
        (specialized_block, specialized, TIER_SPECIALIZED),
        (general_block, general, TIER_GENERAL),
        (threaded_block, threaded, TIER_THREADED),
    ] {
        builder.switch_to_block(block);
        emit_tier_body(&mut builder, isa, observation, signature, body, tier, &params);
    }
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    Ok(native)
}

/// One function's eager general-dispatch publication.
#[cfg(feature = "aot")]
struct EagerLowering {
    code: Vec<CodeMemory>,
    typed_regions: usize,
    scalar_regions: usize,
}

/// One exact semantic operation per PC for a live general-dispatch fallback:
/// the same table the linked publisher relocates, addressed directly because
/// this publication has no linker.
#[cfg(feature = "aot")]
fn lower_eager_fallback(
    instruction_count: usize,
    entries: *const *const (),
    operands: *const super::NativeJsOperands,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
) -> Result<ir::Function, String> {
    let count = u32::try_from(instruction_count).map_err(|_| "native instruction count exceeds u32")?;
    let mut native = ir::Function::with_name_signature(
        ir::UserFuncName::user(super::native_object::NATIVE_FUNCTION_NAMESPACE, u32::MAX - 1),
        native_js_operation_signature(isa),
    );
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let params = builder.block_params(entry).to_vec();
    let pc = params[1];
    let valid_pc = builder.create_block();
    let invalid = builder.create_block();
    let valid = builder.ins().icmp_imm_u(IntCC::UnsignedLessThan, pc, i64::from(count));
    builder.ins().brif(valid, valid_pc, &[], invalid, &[]);
    builder.switch_to_block(valid_pc);
    emit_eager_fallback_call(&mut builder, isa, &params, entries, operands);
    builder.switch_to_block(invalid);
    let status = builder.ins().iconst(types::I64, i64::from(NATIVE_JS_STOPPED));
    builder.ins().return_(&[status]);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    Ok(native)
}

/// The fallback's call: `entries[pc](context, pc, operands + pc, result)`.
#[cfg(feature = "aot")]
fn emit_eager_fallback_call(
    builder: &mut FunctionBuilder<'_>,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    params: &[ir::Value],
    entries: *const *const (),
    operands: *const super::NativeJsOperands,
) {
    const PC_PARAMETER: usize = 1;
    const RESULT_PARAMETER: usize = 3;
    let pc = params[PC_PARAMETER];
    let table = builder.ins().iconst(isa.pointer_type(), entries as usize as i64);
    let offset = builder.ins().imul_imm_u(pc, i64::from(isa.pointer_bytes()));
    let slot = builder.ins().iadd(table, offset);
    let callee = builder.ins().load(isa.pointer_type(), MemFlagsData::trusted(), slot, 0);
    let base = builder.ins().iconst(isa.pointer_type(), operands as usize as i64);
    let operands_offset = builder.ins().imul_imm_u(pc, std::mem::size_of::<super::NativeJsOperands>() as i64);
    let record = builder.ins().iadd(base, operands_offset);
    let signature = builder.import_signature(native_js_operation_signature(isa));
    let call = builder.ins().call_indirect(signature, callee, &[params[0], pc, record, params[RESULT_PARAMETER]]);
    let status = builder.inst_results(call)[0];
    builder.ins().return_(&[status]);
}

#[cfg(feature = "aot")]
fn compile_eager_body<H: Host>(
    sites: &[super::specialize::InstructionSite], function_id: u32, isa: &dyn cranelift_codegen::isa::TargetIsa,
    region: Option<std::ops::Range<usize>>,
) -> Result<CodeMemory, String> {
    let mut signature = None;
    let native = super::native_graph::lower_body(sites, function_id, isa, region,
        |builder, site, context, result| {
            let signature = *signature.get_or_insert_with(|| builder.import_signature(native_js_operation_signature(isa)));
            let target = builder.ins().iconst(isa.pointer_type(), site.op.native_semantic_entry::<H>() as usize as i64);
            let arguments = builder.ins().iconst(isa.pointer_type(), site.operands.as_ptr() as i64);
            let pc = builder.ins().iconst(types::I64, site.pc as i64);
            let call = builder.ins().call_indirect(signature, target, &[context, pc, arguments, result]);
            builder.inst_results(call)[0]
        })?;
    publish_eager_graph(native, isa).map_err(|error| format!("native function {function_id}: {error}"))
}

#[cfg(feature = "aot")]
pub(super) fn publish_eager_graph(function: ir::Function, isa: &dyn cranelift_codegen::isa::TargetIsa) -> Result<CodeMemory, String> {
    super::jit_backend::compile_function(function, isa, None, |compiled, _| {
        if !compiled.buffer.relocs().is_empty() { return Err(NativeCompileRejection::Relocations); }
        CodeMemory::new(compiled.code_buffer()).ok_or(NativeCompileRejection::Publish)
    }).map_err(|error| error.reason().to_owned())
}
