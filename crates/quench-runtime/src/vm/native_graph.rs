//! Native control flow over statically selected shared semantic operations.

use super::native_object::{
    NATIVE_DATA_NAMESPACE, NATIVE_FUNCTION_NAMESPACE, NativeData, NativeFunction,
    NativeFunctionAddress,
};
use super::native_operands::NativeJsOperands;
use super::specialize::{CallThreaded, CallThreadedExtension, Continuation, InstructionSite, Specializer};
use crate::bytecode::{ControlFlowLayout, Function, Op, ProgramKind, ResidualProgram};
use cranelift_codegen::{
    ir::{self, InstBuilder, MemFlagsData, condcodes::IntCC, types},
    isa::TargetIsa,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use std::{num::NonZeroUsize, ops::Range};

// Bounds the working CFG, not the source function. Large unpartitioned native
// graphs exhausted 14 GiB on a 50 KiB input; callers may choose any nonzero budget.
const DEFAULT_REGION_INSTRUCTION_BUDGET: NonZeroUsize = NonZeroUsize::new(1024).unwrap();
const NATIVE_JS_REGION_EXIT: u32 = super::compiled::NATIVE_JS_INVALID_PC + 1;
// The shared entry ABI's memory-base word is unused by JavaScript bodies.
const REGION_START_PC_PARAMETER: usize = 1;

/// How executable memory receives the call-threaded graph. Both publications
/// residualize the same graph over the same semantic entries; they differ in
/// when the residual shape is compiled (`native_stencil`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeJsBackend {
    /// Copy and patch shapes compiled once per process. Falls back to
    /// `Cranelift` where the target has no stencil library.
    #[default]
    Stencil,
    /// Lower every function (or region) through Cranelift.
    Cranelift,
}

#[derive(Clone, Copy, Debug)]
pub struct NativeJsCodegenOptions {
    /// Bounds the Cranelift working CFG; stencil publication has no CFG.
    pub region_instruction_budget: NonZeroUsize,
    pub backend: NativeJsBackend,
}

impl Default for NativeJsCodegenOptions {
    fn default() -> Self {
        Self {
            region_instruction_budget: DEFAULT_REGION_INSTRUCTION_BUDGET,
            backend: NativeJsBackend::default(),
        }
    }
}

impl NativeJsCodegenOptions {
    fn region_count(self, count: usize) -> usize {
        let width = self.region_instruction_budget.get();
        if count > width { count.div_ceil(width) } else { 0 }
    }

    pub(super) fn regions(self, count: usize) -> Option<Vec<Range<usize>>> {
        let width = self.region_instruction_budget.get();
        let regions = self.region_count(count);
        (regions != 0).then(|| {
            (0..regions)
                .map(|index| {
                    let start = index * width;
                    start..start.saturating_add(width).min(count)
                })
                .collect()
        })
    }
}

/// Link inputs for all functions in one source program, including its entry.
/// Semantic imports name compiler-owned operations, never process addresses.
pub struct NativeJsModule {
    pub functions: Vec<NativeFunction>,
    pub data: Vec<NativeData>,
    entry_table: usize,
}

impl NativeJsModule {
    /// Linker symbol for the ABI header followed by function-ID-ordered entries.
    pub fn entry_table_symbol(&self) -> &str {
        &self.data[self.entry_table].name
    }
}

/// Lower every function before execution. Deferred function bodies are resolved
/// through the program's existing authority, just as they are for the JIT.
pub fn lower_native_js_module(
    program: &ResidualProgram,
    namespace: &str,
) -> Result<NativeJsModule, String> {
    lower_native_js_module_with_options(program, namespace, NativeJsCodegenOptions::default())
}

pub fn lower_native_js_module_with_options(
    program: &ResidualProgram,
    namespace: &str,
    options: NativeJsCodegenOptions,
) -> Result<NativeJsModule, String> {
    if program.kind == ProgramKind::Wasm {
        return Err("JavaScript native graph requires a JS or TS program".into());
    }
    let isa = super::jit_backend::target_isa_for(
        "speed",
        super::jit_backend::CodePublication::RelocatableObject,
    )
    .ok_or("native target unavailable")?;
    CallThreaded(ObjectExtension { isa, namespace, options, program, functions: Vec::new() }).specialize(program)
}

/// The call-threaded generating extension for linked objects: each semantic
/// entry is an imported symbol, each operand record is object data, and the
/// linker binds both.
struct ObjectExtension<'n> {
    isa: std::sync::Arc<dyn TargetIsa>,
    namespace: &'n str,
    options: NativeJsCodegenOptions,
    /// Guarded shared lowering (scalar, boolean and fallback regions) reads the bytecode.
    program: &'n ResidualProgram,
    functions: Vec<Vec<InstructionSite>>,
}

impl CallThreadedExtension for ObjectExtension<'_> {
    type Residual = NativeJsModule;

    fn function(&mut self, _function_id: u32, sites: Vec<InstructionSite>) -> Result<(), String> {
        self.functions.push(sites);
        Ok(())
    }

    fn finish(self) -> Result<NativeJsModule, String> {
        let Self { isa, namespace, options, program, functions: bodies } = self;
        let mut functions = Vec::new();
        let abi_function = 0;
        functions.push(NativeFunction::Import {
            name: crate::NATIVE_JS_ABI_SYMBOL.into(),
            signature: ir::Signature::new(isa.default_call_conv()),
        });
        let lane_helpers = u32::try_from(functions.len()).map_err(|_| "native helper symbol count exceeds u32")?;
        let inline_targets = super::compiled::static_call_targets(program);
        let typed_units: Vec<_> = (0..program.function_count()).map(|id|
            if options.regions(program.function(id).code.len()).is_none() {
                super::compiled::lower_static_typed_regions(program, id as u32, &inline_targets[id], &*isa, lane_helpers)
            } else { Vec::new() }).collect();
        if typed_units.iter().any(|units| !units.is_empty()) {
            functions.push(NativeFunction::Import {
                name: crate::NATIVE_JS_LANE_HELPERS_SYMBOL.into(),
                signature: super::compiled::native_lane_helpers_signature(&*isa),
            });
        }
        let mut operations = vec![None; Op::COUNT];
        for sites in &bodies {
            // Fallback tables name an entry for every PC, so every op is collected.
            for op in sites.iter().map(|site| site.op) {
                if operations[op as usize].is_none() {
                    let index = u32::try_from(functions.len())
                        .map_err(|_| "native symbol count exceeds u32")?;
                    operations[op as usize] = Some(index);
                    functions.push(NativeFunction::Import {
                        name: format!(
                            "{}{}",
                            crate::bytecode::NATIVE_JS_OPERATION_PREFIX,
                            Op::NAMES[op as usize]
                        ),
                        signature: super::compiled::native_js_operation_signature(&*isa),
                    });
                }
            }
        }
        let first_definition =
            u32::try_from(functions.len()).map_err(|_| "native symbol count exceeds u32")?;
        let region_count = (0..program.function_count())
            .try_fold(0usize, |count, id| count.checked_add(options.region_count(program.function(id).code.len())))
            .ok_or("native region count overflow")?;
        let mut boolean_regions = Vec::new();
        let mut captured_stores = Vec::new();
        for id in 1..program.function_count() {
            let body = program.function(id);
            if let Some(store) = super::js_compiled::captured_field_store_candidate(program, body) {
                captured_stores.push((u32::try_from(id).map_err(|_| "native function count exceeds u32")?, store));
            }
            if body.params != 0 || !body.plain_activation(program) || options.region_count(body.code.len()) != 0
                || !body.code.iter().copied().enumerate().all(|(pc, packed)| {
                    if packed.is_wide() { return false; }
                    let instruction = packed.as_wide();
                    match instruction.op().control_flow_layout() {
                        ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump =>
                            usize::try_from(instruction.jump_target()).is_ok_and(|target| target > pc),
                        _ => true,
                    }
                })
                || super::js_compiled::numeric_capture_candidate(body).is_none()
                || super::js_compiled::cached_numeric_field_inputs_candidate(body).is_none()
            {
                continue;
            }
            let function_id = u32::try_from(id).map_err(|_| "native function count exceeds u32")?;
            if let Some(region) = super::js_compiled::lower_number_domain_boolean_tree(
                program, function_id, body, &*isa,
            ).filter(|region| {
                region.field_argument.is_none()
                    && !region.capture_arguments.is_empty()
                    && !region.numeric_field_inputs.is_empty()
                    && region.capture_arguments.iter().all(|capture| capture.depth == 0)
                    && region.numeric_field_inputs.iter().all(|input| {
                        input.receiver == super::js_compiled::JsJitFieldReceiver::This
                    })
            }) {
                boolean_regions.push((function_id, region));
            }
        }
        let scalar_regions: Vec<_> = (0..program.function_count()).map(|id| {
            let body = program.function(id);
            if id == 0 || options.regions(body.code.len()).is_some()
                || !super::compiled::js_general_dispatch_candidate(program, body)
            {
                Vec::new()
            } else {
                super::compiled::lower_static_scalar_regions(program, id as u32, &*isa)
            }
        }).collect();
        let scalar_count = scalar_regions.iter().try_fold(0usize, |count, regions|
            count.checked_add(regions.len())).ok_or("native scalar symbol count overflow")?;
        let scalar_base = (first_definition as usize)
            .checked_add(program.function_count())
            .and_then(|base| base.checked_add(region_count))
            .ok_or("native scalar symbol count overflow")?;
        let boolean_base = scalar_base.checked_add(scalar_count)
            .ok_or("native boolean symbol count overflow")?;
        u32::try_from(boolean_base).map_err(|_| "native scalar symbol count exceeds u32")?;
        let fallback_base = boolean_base.checked_add(boolean_regions.len())
            .ok_or("native fallback symbol count overflow")?;
        let mut data = Vec::with_capacity(program.function_count());
        let mut region_definitions = Vec::new();
        let mut fallback_definitions = Vec::new();
        let mut fallback_tables = Vec::new();
        let mut scalar_definitions = Vec::new();
        for (id, scalar_regions) in scalar_regions.into_iter().enumerate() {
            let body = program.function(id);
            let function_id = u32::try_from(id).map_err(|_| "native function count exceeds u32")?;
            let sites = &bodies[id];
            let byte_len = sites
                .len()
                .checked_mul(std::mem::size_of::<NativeJsOperands>())
                .ok_or("native operand data size overflow")?;
            let mut bytes = Vec::with_capacity(byte_len);
            for site in sites {
                site.operands.append_native_bytes(&mut bytes);
            }
            let units: Vec<_> = typed_units[id].iter().map(|unit|
                super::compiled::StaticTypedRegionEntry { header: unit.header, function: &unit.function }).collect();
            let native = if let Some(regions) = options.regions(body.code.len()) {
                let region_base = (first_definition as usize)
                    .checked_add(program.function_count())
                    .and_then(|base| base.checked_add(region_definitions.len()))
                    .ok_or("native symbol count overflow")?;
                let region_end = region_base
                    .checked_add(regions.len())
                    .ok_or("native symbol count overflow")?;
                u32::try_from(region_end).map_err(|_| "native symbol count exceeds u32")?;
                for (index, region) in regions.iter().cloned().enumerate() {
                    region_definitions.push(NativeFunction::Definition {
                        name: format!("{namespace}_function_{id}_region_{index}"),
                        function: lower_object_body(
                            sites,
                            function_id,
                            &*isa,
                            &operations,
                            Some(region),
                        )?,
                    });
                }
                let mut signature = None;
                lower_region_dispatch(
                    function_id,
                    body.code.len(),
                    options.region_instruction_budget.get(),
                    regions.len(),
                    &*isa,
                    |builder, index, context, pc, result| {
                        let signature = *signature.get_or_insert_with(|| {
                            builder.import_signature(super::compiled::native_entry_signature(
                                isa.pointer_type(),
                                isa.default_call_conv(),
                            ))
                        });
                        let symbol =
                            u32::try_from(region_base + index).expect("validated native region symbol");
                        let name =
                            builder
                                .func
                                .declare_imported_user_function(ir::UserExternalName::new(
                                    NATIVE_FUNCTION_NAMESPACE,
                                    symbol,
                                ));
                        let callee = builder.import_function(ir::ExtFuncData {
                            name: ir::ExternalName::user(name),
                            signature,
                            colocated: false,
                            patchable: false,
                        });
                        let arguments = region_call_arguments(builder, &*isa, context, pc, result);
                        let call = builder.ins().call(callee, &arguments);
                        builder.inst_results(call)[0]
                    },
                )?
            } else {
                let fallback = fallback_base.checked_add(fallback_definitions.len())
                    .and_then(|index| u32::try_from(index).ok())
                    .ok_or("native fallback symbol count exceeds u32")?;
                match super::compiled::lower_js_general_dispatch(
                    program, function_id, false, &*isa,
                    super::compiled::JsGeneralCallout::Relocatable { fallback },
                    &scalar_regions.iter().enumerate().map(|(offset, region)| {
                        super::compiled::StaticRegionEntry {
                            range: region.range.clone(),
                            target: super::compiled::StaticRegionTarget::Symbol(
                                u32::try_from(scalar_base + scalar_definitions.len() + offset)
                                    .expect("native scalar symbol index")),
                            fields: &region.fields,
                            frame: &region.frame,
                        }
                    }).collect::<Vec<_>>(), &units,
                ) {
                    Ok((native, _)) => {
                        let table = program.function_count().checked_add(fallback_tables.len())
                            .and_then(|index| u32::try_from(index).ok())
                            .ok_or("native fallback table count exceeds u32")?;
                        fallback_tables.push(object_fallback_table(
                            body, &format!("{namespace}_function_{id}_fallback_table"), &*isa, &operations,
                        )?);
                        fallback_definitions.push(NativeFunction::Definition {
                            name: format!("{namespace}_function_{id}_fallback"),
                            function: lower_object_fallback(body, function_id, table, &*isa)?,
                        });
                        native
                    }
                    Err(_) => lower_object_body(sites, function_id, &*isa, &operations, None)?,
                }
            };
            data.push(NativeData {
                name: format!("{namespace}_operands_{id}"),
                bytes,
                alignment: std::mem::align_of::<NativeJsOperands>() as u64,
                writable: false,
                function_addresses: Vec::new(),
            });
            functions.push(NativeFunction::Definition {
                name: format!("{namespace}_function_{id}"),
                function: native,
            });
            for (index, region) in scalar_regions.into_iter().enumerate() {
                scalar_definitions.push(NativeFunction::Definition {
                    name: format!("{namespace}_function_{id}_scalar_{index}"),
                    function: region.function,
                });
            }
        }
        data.extend(fallback_tables);
        let pointer_bytes = usize::from(isa.pointer_bytes());
        if pointer_bytes != std::mem::size_of::<usize>() {
            return Err("native image pointer width differs from host".into());
        }
        let words = program
            .function_count()
            .checked_add(crate::NATIVE_JS_IMAGE_HEADER_WORDS)
            .ok_or("native entry table size overflow")?;
        let entries_bytes = words
            .checked_mul(pointer_bytes)
            .ok_or("native entry table size overflow")?;
        let guarded_count = boolean_regions.len().checked_add(captured_stores.len())
            .ok_or("native guarded-call count overflow")?;
        let index_bytes = if guarded_count == 0 { 0 } else { program.function_count().checked_mul(pointer_bytes)
            .ok_or("native guarded index size overflow")? };
        let regions_base = entries_bytes.checked_add(index_bytes).ok_or("native guarded index size overflow")?;
        let bytes = boolean_regions.len()
            .checked_mul(std::mem::size_of::<super::native_program::NativeBooleanRegion>())
            .and_then(|bytes| regions_base.checked_add(bytes))
            .ok_or("native boolean region table size overflow")?;
        let stores_base = bytes;
        let bytes = captured_stores.len()
            .checked_mul(std::mem::size_of::<super::native_program::NativeCapturedStore>())
            .and_then(|stores| bytes.checked_add(stores))
            .ok_or("native captured-store table size overflow")?;
        let mut table = vec![0; bytes];
        let count_offset = super::native_program::BOOLEAN_REGION_COUNT_WORD * pointer_bytes;
        table[count_offset..count_offset + pointer_bytes]
            .copy_from_slice(&boolean_regions.len().to_ne_bytes());
        let store_count_offset = super::native_program::CAPTURED_STORE_COUNT_WORD * pointer_bytes;
        table[store_count_offset..store_count_offset + pointer_bytes]
            .copy_from_slice(&captured_stores.len().to_ne_bytes());
        use super::native_program::{GuardedCallIndex, GuardedCallKind};
        let indices = boolean_regions.iter().enumerate().map(|(row, (function, _))| (GuardedCallKind::Boolean, row, *function))
            .chain(captured_stores.iter().enumerate().map(|(row, (function, _))| (GuardedCallKind::CapturedStore, row, *function)));
        for (kind, row, function) in indices {
            let base = entries_bytes + function as usize * pointer_bytes;
            if table[base..base + pointer_bytes].iter().any(|byte| *byte != 0) {
                return Err("multiple native guarded plans for one function".into());
            }
            let word = GuardedCallIndex::encode(kind, row).ok_or("native guarded-call index overflow")?;
            table[base..base + pointer_bytes].copy_from_slice(&word.to_ne_bytes());
        }
        for (index, (function, store)) in captured_stores.iter().enumerate() {
            use super::native_program::NativeCapturedStore as Layout;
            let base = stores_base + index * std::mem::size_of::<Layout>();
            let mut write = |offset: usize, value: &[u8]| {
                table[base + offset..base + offset + value.len()].copy_from_slice(value);
            };
            write(std::mem::offset_of!(Layout, function), &function.to_ne_bytes());
            write(std::mem::offset_of!(Layout, atom), &store.atom.to_ne_bytes());
            write(std::mem::offset_of!(Layout, capture_slot), &store.capture_slot.to_ne_bytes());
            write(std::mem::offset_of!(Layout, cache_site), &store.cache_site.to_ne_bytes());
            let operator = match store.update {
                Some(operation) => u16::try_from(operation.operator())
                    .map_err(|_| "captured-store operator exceeds native layout")?,
                None => super::js_compiled::CAPTURED_STORE_ASSIGNMENT,
            };
            write(std::mem::offset_of!(Layout, operator), &operator.to_ne_bytes());
        }
        let mut addresses = Vec::with_capacity(words);
        addresses.push(NativeFunctionAddress {
            offset: 0,
            function: abi_function,
        });
        for id in 0..program.function_count() {
            let word = id + crate::NATIVE_JS_IMAGE_HEADER_WORDS;
            let offset = u32::try_from(word * pointer_bytes)
                .map_err(|_| "native entry table exceeds relocation offsets")?;
            let function = first_definition
                .checked_add(u32::try_from(id).map_err(|_| "native function count exceeds u32")?)
                .ok_or("native function symbol count exceeds u32")?;
            addresses.push(NativeFunctionAddress { offset, function });
        }
        for (index, (function, region)) in boolean_regions.iter().enumerate() {
            use super::native_program::NativeBooleanRegion as Layout;
            let base = regions_base + index * std::mem::size_of::<Layout>();
            let mut write = |offset: usize, value: &[u8]| {
                table[base + offset..base + offset + value.len()].copy_from_slice(value);
            };
            write(std::mem::offset_of!(Layout, function), &function.to_ne_bytes());
            write(std::mem::offset_of!(Layout, capture_count), &(region.capture_arguments.len() as u16).to_ne_bytes());
            write(std::mem::offset_of!(Layout, field_count), &(region.numeric_field_inputs.len() as u16).to_ne_bytes());
            let flags = if region.requires_signed_int32_arguments { Layout::SIGNED_INT32_INPUTS } else { 0 };
            write(std::mem::offset_of!(Layout, flags), &flags.to_ne_bytes());
            for (slot, capture) in region.capture_arguments.iter().enumerate() {
                write(std::mem::offset_of!(Layout, capture_slots) + slot * std::mem::size_of::<u16>(), &capture.slot.to_ne_bytes());
            }
            for (slot, field) in region.numeric_field_inputs.iter().enumerate() {
                if usize::from(field.source) == slot {
                    write(std::mem::offset_of!(Layout, field_sites) + slot * std::mem::size_of::<u16>(), &field.cache_site.to_ne_bytes());
                }
                write(std::mem::offset_of!(Layout, field_sources) + slot * std::mem::size_of::<u16>(), &field.source.to_ne_bytes());
            }
            addresses.push(NativeFunctionAddress {
                offset: u32::try_from(base + std::mem::offset_of!(Layout, entry))
                    .map_err(|_| "native boolean table exceeds relocation offsets")?,
                function: u32::try_from(boolean_base + index)
                    .map_err(|_| "native boolean symbol count exceeds u32")?,
            });
        }
        functions.extend(region_definitions);
        functions.extend(scalar_definitions);
        functions.extend(boolean_regions.into_iter().map(|(id, region)| NativeFunction::Definition {
            name: format!("{namespace}_function_{id}_boolean"), function: region.function,
        }));
        functions.extend(fallback_definitions);
        u32::try_from(functions.len()).map_err(|_| "native symbol count exceeds u32")?;
        let entry_table = data.len();
        data.push(NativeData {
            name: format!("{namespace}_entries"),
            bytes: table,
            alignment: pointer_bytes as u64,
            writable: false,
            function_addresses: addresses,
        });
        Ok(NativeJsModule {
            functions,
            data,
            entry_table,
        })
    }
}

fn lower_object_body(
    sites: &[InstructionSite],
    function_id: u32,
    isa: &dyn TargetIsa,
    operations: &[Option<u32>],
    region: Option<Range<usize>>,
) -> Result<ir::Function, String> {
    let mut object = ObjectOperations::new(function_id, isa, operations);
    lower_body(
        sites,
        function_id,
        isa,
        region,
        |builder, site, context, result| object.emit(builder, site.op, site.pc, context, result),
    )
}

struct ObjectOperations<'a> {
    function_id: u32,
    isa: &'a dyn TargetIsa,
    operations: &'a [Option<u32>],
    entries: Vec<Option<ir::FuncRef>>,
    operand_base: Option<ir::GlobalValue>,
}

impl<'a> ObjectOperations<'a> {
    fn new(function_id: u32, isa: &'a dyn TargetIsa, operations: &'a [Option<u32>]) -> Self {
        Self {
            function_id,
            isa,
            operations,
            entries: vec![None; Op::COUNT],
            operand_base: None,
        }
    }

    fn emit(
        &mut self, builder: &mut FunctionBuilder<'_>, op: Op, pc: usize,
        context: ir::Value, result: ir::Value,
    ) -> ir::Value {
        let entry = *self.entries[op as usize].get_or_insert_with(|| {
            let signature = builder.import_signature(
                super::compiled::native_js_operation_signature(self.isa),
            );
            let name = builder.func.declare_imported_user_function(ir::UserExternalName::new(
                NATIVE_FUNCTION_NAMESPACE,
                self.operations[op as usize].expect("collected operation"),
            ));
            builder.import_function(ir::ExtFuncData {
                name: ir::ExternalName::user(name), signature, colocated: false, patchable: false,
            })
        });
        let base = *self.operand_base.get_or_insert_with(|| {
            let name = builder.func.declare_imported_user_function(ir::UserExternalName::new(
                NATIVE_DATA_NAMESPACE, self.function_id,
            ));
            builder.create_global_value(ir::GlobalValueData::Symbol {
                name: ir::ExternalName::user(name),
                offset: ir::immediates::Imm64::new(0),
                colocated: false,
                tls: false,
            })
        });
        let address = builder.ins().symbol_value(self.isa.pointer_type(), base);
        // Both callers bound the instruction count, so this offset is host-addressable.
        let offset = i64::try_from(pc * std::mem::size_of::<NativeJsOperands>())
            .expect("host addressable operand offset");
        let address = builder.ins().iadd_imm_s(address, offset);
        let pc = builder.ins().iconst(types::I64, pc as i64);
        let call = builder.ins().call(entry, &[context, pc, address, result]);
        builder.inst_results(call)[0]
    }
}

fn object_fallback_table(
    function: &Function, name: &str, isa: &dyn TargetIsa, operations: &[Option<u32>],
) -> Result<NativeData, String> {
    let pointer_bytes = usize::from(isa.pointer_bytes());
    let size = function.code.len().checked_mul(pointer_bytes).ok_or("native fallback table size overflow")?;
    let mut addresses = Vec::with_capacity(function.code.len());
    for (pc, packed) in function.code.iter().copied().enumerate() {
        let op = super::dispatch_fast::decoded_instruction(function, packed).op();
        addresses.push(NativeFunctionAddress {
            offset: u32::try_from(pc * pointer_bytes).map_err(|_| "native fallback table offset exceeds u32")?,
            function: operations[op as usize].expect("collected operation"),
        });
    }
    Ok(NativeData {
        name: name.into(), bytes: vec![0; size], alignment: pointer_bytes as u64,
        writable: false, function_addresses: addresses,
    })
}

/// One exact semantic operation for a guarded native path's fallback.
fn lower_object_fallback(
    function: &Function, function_id: u32, table: u32, isa: &dyn TargetIsa,
) -> Result<ir::Function, String> {
    let count = u32::try_from(function.code.len()).map_err(|_| "native instruction count exceeds u32")?;
    let mut native = ir::Function::with_name_signature(
        ir::UserFuncName::user(NATIVE_FUNCTION_NAMESPACE, function_id),
        super::compiled::native_js_operation_signature(isa),
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
    let symbol = |builder: &mut FunctionBuilder<'_>, index| {
        let name = builder.func.declare_imported_user_function(ir::UserExternalName::new(
            NATIVE_DATA_NAMESPACE, index,
        ));
        let value = builder.create_global_value(ir::GlobalValueData::Symbol {
            name: ir::ExternalName::user(name), offset: ir::immediates::Imm64::new(0),
            colocated: false, tls: false,
        });
        builder.ins().symbol_value(isa.pointer_type(), value)
    };
    let table_base = symbol(&mut builder, table);
    let table_offset = builder.ins().imul_imm_u(pc, i64::from(isa.pointer_bytes()));
    let entry_address = builder.ins().iadd(table_base, table_offset);
    let entry = builder.ins().load(isa.pointer_type(), MemFlagsData::trusted(), entry_address, 0);
    let operands_base = symbol(&mut builder, function_id);
    let operands_offset = builder.ins().imul_imm_u(pc, std::mem::size_of::<NativeJsOperands>() as i64);
    let operands = builder.ins().iadd(operands_base, operands_offset);
    let signature = builder.import_signature(super::compiled::native_js_operation_signature(isa));
    let call = builder.ins().call_indirect(signature, entry, &[params[0], pc, operands, params[3]]);
    let status = builder.inst_results(call)[0];
    builder.ins().return_(&[status]);
    builder.switch_to_block(invalid);
    let status = builder.ins().iconst(types::I64, i64::from(super::compiled::NATIVE_JS_STOPPED));
    builder.ins().return_(&[status]);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    Ok(native)
}

/// Bindings choose linker symbols or live JIT addresses; the graph and semantic
/// continuation protocol are identical in both publication modes.
pub(super) fn lower_body(
    sites: &[InstructionSite],
    function_id: u32,
    isa: &dyn TargetIsa,
    region: Option<Range<usize>>,
    mut operation: impl FnMut(&mut FunctionBuilder<'_>, &InstructionSite, ir::Value, ir::Value) -> ir::Value,
) -> Result<ir::Function, String> {
    u32::try_from(sites.len()).map_err(|_| "native instruction count exceeds u32")?;
    let is_region = region.is_some();
    let range = region.unwrap_or(0..sites.len());
    if range.start > range.end || range.end > sites.len() {
        return Err("invalid native region bounds".into());
    }
    let count =
        u32::try_from(range.len()).map_err(|_| "native region instruction count exceeds u32")?;
    let mut native = ir::Function::with_name_signature(
        ir::UserFuncName::user(NATIVE_FUNCTION_NAMESPACE, function_id),
        super::compiled::native_entry_signature(isa.pointer_type(), isa.default_call_conv()),
    );
    let mut builder_context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut builder_context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let context = builder.block_params(entry)[0];
    let result = builder.block_params(entry)[4];
    let start_pc = if is_region {
        builder.block_params(entry)[REGION_START_PC_PARAMETER]
    } else {
        super::compiled::native_js_start_pc(&mut builder, context)
    };
    let resume = builder.create_block();
    let pc = builder.append_block_param(resume, types::I64);
    let select = builder.create_block();
    let selector = builder.append_block_param(select, types::I32);
    let invalid = builder.create_block();
    let invalid_pc = builder.append_block_param(invalid, types::I64);
    let stopped = builder.create_block();
    let blocks: Vec<_> = (0..count).map(|_| builder.create_block()).collect();
    builder.ins().jump(resume, &[ir::BlockArg::Value(start_pc)]);

    for (offset, block) in blocks.iter().copied().enumerate() {
        let site = &sites[range.start + offset];
        builder.switch_to_block(block);
        if let Continuation::Static(target) = site.continuation {
            match target.checked_sub(range.start).and_then(|offset| blocks.get(offset)) {
                Some(block) => {
                    builder.ins().jump(*block, &[]);
                }
                None => {
                    let target = builder.ins().iconst(types::I64, target as i64);
                    builder.ins().jump(resume, &[ir::BlockArg::Value(target)]);
                }
            }
            continue;
        }
        let status = operation(&mut builder, site, context, result);
        let continued = builder.ins().icmp_imm_u(IntCC::Equal, status, 0);
        let successor = builder.create_block();
        builder.ins().brif(continued, successor, &[], stopped, &[]);
        builder.switch_to_block(successor);
        let next = builder
            .ins()
            .load(types::I64, MemFlagsData::trusted(), result, 0);
        if site.continuation == Continuation::Fallthrough
            && let Some(block) = blocks.get(offset + 1)
        {
            let fallthrough = builder
                .ins()
                .icmp_imm_u(IntCC::Equal, next, (site.pc + 1) as i64);
            builder.ins().brif(
                fallthrough,
                *block,
                &[],
                resume,
                &[ir::BlockArg::Value(next)],
            );
        } else {
            builder.ins().jump(resume, &[ir::BlockArg::Value(next)]);
        }
    }
    builder.switch_to_block(resume);
    // Check the full PC before narrowing it for br_table. A high word must not
    // alias a valid instruction and replay guest effects.
    let offset = builder.ins().iadd_imm_s(pc, -(range.start as i64));
    let in_range = builder
        .ins()
        .icmp_imm_u(IntCC::UnsignedLessThan, offset, i64::from(count));
    let index = builder.ins().ireduce(types::I32, offset);
    builder.ins().brif(
        in_range,
        select,
        &[ir::BlockArg::Value(index)],
        invalid,
        &[ir::BlockArg::Value(pc)],
    );
    builder.switch_to_block(select);
    let default = builder
        .func
        .dfg
        .block_call(invalid, &[ir::BlockArg::Value(pc)]);
    let cases: Vec<_> = blocks
        .iter()
        .map(|block| builder.func.dfg.block_call(*block, &[]))
        .collect();
    let table = builder.create_jump_table(ir::JumpTableData::new(default, &cases));
    builder.ins().br_table(selector, table);
    builder.switch_to_block(invalid);
    builder
        .ins()
        .store(MemFlagsData::trusted(), invalid_pc, result, 0);
    let status = builder.ins().iconst(
        types::I32,
        i64::from(if is_region {
            NATIVE_JS_REGION_EXIT
        } else {
            super::compiled::NATIVE_JS_INVALID_PC
        }),
    );
    builder.ins().return_(&[status]);
    builder.switch_to_block(stopped);
    let status = builder
        .ins()
        .iconst(types::I32, i64::from(super::compiled::NATIVE_JS_STOPPED));
    builder.ins().return_(&[status]);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    Ok(native)
}

/// Private regions reuse the native ABI, carrying their starting PC in its
/// otherwise unused memory-base word. Guest arguments remain in the VM frame.
pub(super) fn region_call_arguments(
    builder: &mut FunctionBuilder<'_>,
    isa: &dyn TargetIsa,
    context: ir::Value,
    pc: ir::Value,
    result: ir::Value,
) -> [ir::Value; 5] {
    let length = builder.ins().iconst(types::I64, 0);
    let arguments = builder.ins().iconst(isa.pointer_type(), 0);
    [context, pc, length, arguments, result]
}

pub(super) fn lower_region_dispatch(
    function_id: u32,
    instruction_count: usize,
    width: usize,
    region_count: usize,
    isa: &dyn TargetIsa,
    mut call: impl FnMut(&mut FunctionBuilder<'_>, usize, ir::Value, ir::Value, ir::Value) -> ir::Value,
) -> Result<ir::Function, String> {
    let count =
        u32::try_from(instruction_count).map_err(|_| "native instruction count exceeds u32")?;
    if width == 0 || instruction_count.div_ceil(width) != region_count {
        return Err("invalid native region plan".into());
    }
    let mut native = ir::Function::with_name_signature(
        ir::UserFuncName::user(NATIVE_FUNCTION_NAMESPACE, function_id),
        super::compiled::native_entry_signature(isa.pointer_type(), isa.default_call_conv()),
    );
    let mut context_builder = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut native, &mut context_builder);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let context = builder.block_params(entry)[0];
    let result = builder.block_params(entry)[4];
    let start = super::compiled::native_js_start_pc(&mut builder, context);
    let resume = builder.create_block();
    let pc = builder.append_block_param(resume, types::I64);
    let select = builder.create_block();
    let selector = builder.append_block_param(select, types::I32);
    let invalid = builder.create_block();
    let invalid_pc = builder.append_block_param(invalid, types::I64);
    let stopped = builder.create_block();
    let status = builder.append_block_param(stopped, types::I32);
    let blocks: Vec<_> = (0..region_count).map(|_| builder.create_block()).collect();
    builder.ins().jump(resume, &[ir::BlockArg::Value(start)]);
    for (index, block) in blocks.iter().copied().enumerate() {
        builder.switch_to_block(block);
        let status = call(&mut builder, index, context, pc, result);
        let continued =
            builder
                .ins()
                .icmp_imm_u(IntCC::Equal, status, i64::from(NATIVE_JS_REGION_EXIT));
        let next = builder.create_block();
        builder.ins().brif(
            continued,
            next,
            &[],
            stopped,
            &[ir::BlockArg::Value(status)],
        );
        builder.switch_to_block(next);
        let next_pc = builder
            .ins()
            .load(types::I64, MemFlagsData::trusted(), result, 0);
        builder.ins().jump(resume, &[ir::BlockArg::Value(next_pc)]);
    }
    builder.switch_to_block(resume);
    let in_range = builder
        .ins()
        .icmp_imm_u(IntCC::UnsignedLessThan, pc, i64::from(count));
    let divisor = builder.ins().iconst(types::I64, width as i64);
    let region = builder.ins().udiv(pc, divisor);
    let index = builder.ins().ireduce(types::I32, region);
    builder.ins().brif(
        in_range,
        select,
        &[ir::BlockArg::Value(index)],
        invalid,
        &[ir::BlockArg::Value(pc)],
    );
    builder.switch_to_block(select);
    let default = builder
        .func
        .dfg
        .block_call(invalid, &[ir::BlockArg::Value(pc)]);
    let cases: Vec<_> = blocks
        .iter()
        .map(|block| builder.func.dfg.block_call(*block, &[]))
        .collect();
    let table = builder.create_jump_table(ir::JumpTableData::new(default, &cases));
    builder.ins().br_table(selector, table);
    builder.switch_to_block(invalid);
    builder
        .ins()
        .store(MemFlagsData::trusted(), invalid_pc, result, 0);
    let invalid_status = builder
        .ins()
        .iconst(types::I32, i64::from(super::compiled::NATIVE_JS_INVALID_PC));
    builder.ins().return_(&[invalid_status]);
    builder.switch_to_block(stopped);
    builder.ins().return_(&[status]);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    Ok(native)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_objects_include_entry_and_deferred_functions_in_js_and_ts() {
        for program in [
            crate::Engine::specialize(
                "function outer(x) { function inner(y) { return y + 1; } return inner(x); } outer(41);",
                "graph.js",
            ).unwrap(),
            crate::Engine::specialize_typescript(
                "function outer(x: number): number { function inner(y: number): number { return y + 1; } return inner(x); } outer(41);",
                "graph.cts",
            ).unwrap(),
        ] {
            let module = lower_native_js_module(&program, "graph").unwrap();
            let definitions: Vec<_> = module.functions.iter().filter_map(|function| match function {
                NativeFunction::Definition { name, .. } => Some(name.as_str()),
                NativeFunction::Import { .. } => None,
            }).collect();
            assert!(definitions.len() >= program.function_count());
            assert!(definitions.contains(&"graph_function_0"), "root must execute natively");
            assert!(definitions.iter().any(|name| name.ends_with("_fallback")), "eligible functions use shared guarded lowering");
            let object = super::super::native_object::emit_native_module("graph", module.functions, module.data)
                .expect("all function and operand relocations resolve");
            assert!(!object.is_empty());
        }
    }

    #[test]
    fn captured_field_predicates_have_sparse_relocatable_entries() {
        for program in [
            crate::Engine::specialize(
                "function make(mask, expected) { function check() { return (this.value & mask) !== 0 || this.value == expected; } return check; }",
                "boolean.js",
            ).unwrap(),
            crate::Engine::specialize_typescript(
                "function make(mask: number, expected: number) { function check(this: {value: number}): boolean { return (this.value & mask) !== 0 || this.value == expected; } return check; }",
                "boolean.ts",
            ).unwrap(),
        ] {
            let module = lower_native_js_module(&program, "boolean").unwrap();
            assert!(module.functions.iter().any(|function| matches!(function,
                NativeFunction::Definition { name, .. } if name.ends_with("_boolean")
            )));
            let table = &module.data[module.entry_table];
            let word = std::mem::size_of::<usize>();
            let offset = super::super::native_program::BOOLEAN_REGION_COUNT_WORD * word;
            let count = usize::from_ne_bytes(table.bytes[offset..offset + word].try_into().unwrap());
            assert_eq!(count, 1);
            assert_eq!(table.function_addresses.len(), program.function_count() + 2);
            let region_base = (crate::NATIVE_JS_IMAGE_HEADER_WORDS + program.function_count() * 2) * word;
            let source = region_base + std::mem::offset_of!(super::super::native_program::NativeBooleanRegion, field_sources);
            let width = std::mem::size_of::<u16>();
            assert_eq!(u16::from_ne_bytes(table.bytes[source..source + width].try_into().unwrap()), 0);
            assert_eq!(u16::from_ne_bytes(table.bytes[source + width..source + width * 2].try_into().unwrap()), 0);
            let object = super::super::native_object::emit_native_module(
                "boolean", module.functions, module.data,
            ).unwrap();
            assert!(!object.is_empty());
        }
    }

    #[test]
    fn guarded_lookup_indexes_both_payload_kinds_by_function() {
        use super::super::native_program::{GuardedCallIndex, GuardedCallKind, NativeBooleanRegion, NativeCapturedStore};
        let program = crate::Engine::specialize(
            "function make(mask) { return {check:function(){return (this.value & mask)!==0;},mark:function(){this.value=mask;}}; }",
            "indexed-calls.js",
        ).unwrap();
        let module = lower_native_js_module(&program, "indexed").unwrap();
        let table = &module.data[module.entry_table];
        let word = std::mem::size_of::<usize>();
        let read = |offset| usize::from_ne_bytes(table.bytes[offset..offset + word].try_into().unwrap());
        assert_eq!(read(super::super::native_program::BOOLEAN_REGION_COUNT_WORD * word), 1);
        assert_eq!(read(super::super::native_program::CAPTURED_STORE_COUNT_WORD * word), 1);
        let index = (crate::NATIVE_JS_IMAGE_HEADER_WORDS + program.function_count()) * word;
        let boolean = index + program.function_count() * word;
        let store = boolean + std::mem::size_of::<NativeBooleanRegion>();
        for (kind, record, function_offset) in [
            (GuardedCallKind::Boolean, boolean, std::mem::offset_of!(NativeBooleanRegion, function)),
            (GuardedCallKind::CapturedStore, store, std::mem::offset_of!(NativeCapturedStore, function)),
        ] {
            let offset = record + function_offset;
            let function = u32::from_ne_bytes(table.bytes[offset..offset + std::mem::size_of::<u32>()].try_into().unwrap());
            assert_eq!(read(index + function as usize * word), GuardedCallIndex::encode(kind, 0).unwrap());
        }
    }
}
