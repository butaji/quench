//! Eager native publication: every requested body compiled at once with no code-cache budget.
//! A function's lane body is lowered under the image's frozen whole-program facts and entered
//! through a trampoline (`ahead_entry`) whose leaves continue in per-PC semantic entries; a
//! function the lane lowering does not take keeps the call-threaded body. Everything is
//! addressed directly because nothing is linked.
use super::*;

#[cfg(feature = "aot")]
pub(crate) fn compile_eager_native<H: Host>(
    program: ResidualProgram, options: super::native_graph::NativeJsCodegenOptions,
) -> Result<crate::NativeJsProgram<H>, String> {
    #[cfg(feature = "jit")]
    if options.backend == super::native_graph::NativeJsBackend::Stencil && super::native_stencil::available() {
        return super::native_stencil::compile_program(program).map(|(native, _)| native);
    }
    use super::specializer::Specializer;
    program.freeze_native_diagnostics();
    let isa = super::jit_backend::target_isa_for("speed", super::jit_backend::CodePublication::RelocatableObject)
        .ok_or("native target unavailable")?;
    let globals = frozen_globals(&program);
    let frozen_field_facts = frozen_fields(&program);
    let constant_slots: Box<[Value]> = vec![Value::UNDEFINED; program.constants.len()].into_boxed_slice();
    let constant_block = Rc::new(std::cell::RefCell::new(constant_slots));
    let constant_base = constant_block.borrow().as_ptr() as usize;
    // The image's invalidation word: zero while every whole-program assumption holds.
    let invalidation = crate::code_dependencies::Invalidation::default();
    let extension = EagerCraneliftExtension::<H> { program: &program, globals: &globals,
        fields: &frozen_field_facts, constants: constant_base, isa, options, entries: Vec::new(),
        owners: EagerOwners::default(), counts: Default::default(), invalidation: invalidation.clone(),
        host: std::marker::PhantomData };
    let (entries, mut owners, mut counts) = super::specialize::CallThreaded::frozen(extension).specialize(&program)?;
    counts.frozen_globals = globals.len();
    counts.frozen_fields = frozen_field_facts.len();
    let frozen = frozen_assumptions(&mut owners, &globals, &frozen_field_facts, constant_block, invalidation);
    // SAFETY: every entry was lowered from its exact function for host H.
    unsafe { crate::NativeJsProgram::from_entries_with_counts(program, entries, Rc::new(owners), counts, Some(frozen)) }
}

/// The whole-program assumptions an eager image carries: its invalidation word, the atoms,
/// globals and fields it folded, and its constants.
#[cfg(feature = "aot")]
fn frozen_assumptions(owners: &mut EagerOwners, globals: &crate::dense_map::DenseMap<Value>, fields: &crate::dense_map::DenseMap<Value>,
    constants: Rc<std::cell::RefCell<Box<[Value]>>>, invalidation: crate::code_dependencies::Invalidation,
) -> crate::vm::native_program::FrozenAssumptions {
    crate::vm::native_program::FrozenAssumptions {
        invalidation: owners.invalidation.clone(),
        atoms: std::mem::take(&mut owners.frozen_atoms).iter().map(|atom| atom as Atom).collect(),
        globals: globals.keys().collect(),
        fields: fields.keys().collect(),
        constants,
        watch: Rc::new(crate::code_dependencies::PublishedCode::with_invalidation((), invalidation)),
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
    frozen_atoms: crate::bitset::BitSet,
}

/// The call-threaded generating extension for live Cranelift code: semantic
/// entries and operand records are bound by address, so nothing is linked.
#[cfg(feature = "aot")]
struct EagerCraneliftExtension<'p, H> {
    program: &'p ResidualProgram,
    /// Whole-program globals the lane bodies may fold.
    globals: &'p crate::dense_map::DenseMap<Value>,
    /// Whole-program prototype values the lane bodies may fold.
    fields: &'p crate::dense_map::DenseMap<Value>,
    /// The address of the image's constant block.
    constants: usize,
    isa: std::sync::Arc<dyn cranelift_codegen::isa::TargetIsa>,
    options: super::native_graph::NativeJsCodegenOptions,
    entries: Vec<crate::NativeJsFunctionEntry>,
    owners: EagerOwners,
    counts: crate::NativeJsPublicationCounts,
    invalidation: crate::code_dependencies::Invalidation,
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
        // A body whose precise windows exceed the map budget keeps the conservative
        // all-live roots instead, which `NativeJsProgram` reports.
        self.counts.wide_frame_functions += usize::from(self.program.function_get(function_id as usize)
            .is_some_and(|function| crate::compile::liveness::windows::is_conservative(function, function.instructions().count())));
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
    /// A function over the region budget is split into private call-threaded regions with a
    /// dispatch wrapper; every other function takes its lane body behind a trampoline, or the
    /// call-threaded body where the lane lowering declines it.
    fn lower_function(&mut self, function_id: u32, sites: &[super::specialize::InstructionSite],
        isa: &dyn cranelift_codegen::isa::TargetIsa,
    ) -> Result<Vec<CodeMemory>, String> {
        let Some(regions) = self.options.regions(sites.len()) else {
            return match self.publish_lane_body(function_id, isa) {
                Some(body) => self.lower_trampoline(function_id, sites, isa, body),
                None => {
                    self.counts.call_threaded += 1;
                    Ok(vec![compile_eager_body::<H>(sites, function_id, isa, None)?])
                }
            };
        };
        self.counts.call_threaded += 1;
        self.lower_regions(function_id, sites, isa, &regions)
    }

    /// The function as a lane body under the image's frozen facts (the same body the ahead
    /// images compile, its helpers embedded): it runs from the first record and leaves at
    /// the first instruction it does not lower, or where an assumption was retired. `None`
    /// where the body lowers nothing.
    fn publish_lane_body(&mut self, function_id: u32, isa: &dyn cranelift_codegen::isa::TargetIsa) -> Option<CodeMemory> {
        let program = self.program;
        let unnamed_atoms = super::method_cache::AtomBits::default();
        let facts = VmFacts { frozen: true, frozen_fields: Some(self.fields), frozen_constants: Some(self.constants),
            online: Some(OnlineFacts { method_cache_atoms: &unnamed_atoms, invalidation: self.invalidation.address(), constants: &[] }),
            ..VmFacts::STATIC };
        let reads = frozen_global_reads(program, program.function_get(function_id as usize)?, self.globals);
        let (body, atoms) = lower_lane_body(program, function_id, isa,
            LaneHelpers::Process(LaneHelper::addresses::<H>()), facts, &reads)?;
        atoms.iter().for_each(|&atom| {
            self.owners.frozen_atoms.insert_growing(atom as usize);
        });
        publish_eager_graph(body, isa).ok()
    }

    /// The lane `body`, its per-PC semantic fallback and the trampoline that is the entry.
    fn lower_trampoline(&mut self, function_id: u32, sites: &[super::specialize::InstructionSite],
        isa: &dyn cranelift_codegen::isa::TargetIsa, body: CodeMemory,
    ) -> Result<Vec<CodeMemory>, String> {
        let (fallback, operands, table) = self.lower_fallback(sites, isa)?;
        let (mut body_signature, mut fallback_signature) = (None, None);
        let entry = super::ahead_entry::lower_ahead_entry(super::native_object::NATIVE_FUNCTION_NAMESPACE, function_id, sites.len(), isa,
            super::ahead_entry::AheadTargets {
                body: &mut |builder, words| {
                    let signature = *body_signature.get_or_insert_with(|| builder.import_signature(
                        lane_body_signature(isa.pointer_type(), isa.default_call_conv())));
                    let target = builder.ins().iconst(isa.pointer_type(), body.pointer() as usize as i64);
                    let call = builder.ins().call_indirect(signature, target, words);
                    let results = builder.inst_results(call);
                    (results[0], results[1])
                },
                fallback: &mut |builder, [context, pc, result]| {
                    let signature = *fallback_signature.get_or_insert_with(|| builder.import_signature(native_js_operation_signature(isa)));
                    let target = builder.ins().iconst(isa.pointer_type(), fallback.pointer() as usize as i64);
                    let call = builder.ins().call_indirect(signature, target, &[context, pc, result, result]);
                    builder.inst_results(call)[0]
                },
            })?;
        let entry = publish_eager_graph(entry, isa)?;
        self.counts.lane_bodies += 1;
        self.owners.operand_arrays.push(operands);
        self.owners.entry_tables.push(table);
        Ok(vec![body, fallback, entry])
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

    fn lower_fallback(&mut self, sites: &[super::specialize::InstructionSite],
        isa: &dyn cranelift_codegen::isa::TargetIsa,
    ) -> Result<(CodeMemory, Box<[super::NativeJsOperands]>, Box<[*const ()]>), String> {
        let operands: Box<[super::NativeJsOperands]> = sites.iter().map(|site| site.operands).collect();
        let table: Box<[*const ()]> = sites.iter().map(|site|
            site.op.native_semantic_entry::<H>() as *const ()).collect();
        // Addresses are taken from the boxes `EagerOwners` keeps alive; moving a
        // box does not move its heap buffer.
        let fallback = publish_eager_graph(
            lower_eager_fallback(sites.len(), table.as_ptr(), operands.as_ptr(), isa)?, isa)?;
        Ok((fallback, operands, table))
    }
}

/// One exact semantic operation per PC for a live per-PC fallback:
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
fn publish_eager_graph(function: ir::Function, isa: &dyn cranelift_codegen::isa::TargetIsa) -> Result<CodeMemory, String> {
    super::jit_backend::compile_function(function, isa, None, |compiled, _| {
        if !compiled.buffer.relocs().is_empty() { return Err(NativeCompileRejection::Relocations); }
        CodeMemory::new(compiled.code_buffer()).ok_or(NativeCompileRejection::Publish)
    }).map_err(|error| error.reason().to_owned())
}

