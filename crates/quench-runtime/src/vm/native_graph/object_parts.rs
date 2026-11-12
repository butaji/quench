//! The pieces of an object publication: each function's lane body, the operation imports,
//! the region and lane entries, and the entry table that names every definition.
use super::*;

/// Every function's lane body, lowered ahead against the linked helper table and the facts
/// the closed program proves (`None` where a function's body is a region dispatcher, the
/// root, or lowers nothing).
pub(super) fn lower_lane_bodies(
    program: &ResidualProgram, options: NativeJsCodegenOptions, isa: &dyn TargetIsa, lane_helpers: u32,
) -> Vec<Option<ir::Function>> {
    // Whole-program facts the closed image can fold with a read-site guard. The expected
    // value is embedded in the code and checked against the live cell or prototype, so no
    // invalidation word or serialized assumption is needed.
    let globals = super::super::compiled::frozen_globals(program);
    let fields = super::super::compiled::frozen_fields(program);
    let facts = super::super::compiled::VmFacts { frozen_fields: Some(&fields), ..super::super::compiled::VmFacts::STATIC };
    let helpers = super::super::compiled::LaneHelpers::Linked(lane_helpers,
        u32::try_from(program.function_count()).expect("validated native namespace"));
    (0..program.function_count()).map(|id| {
        let function = program.function(id);
        if id == 0 || options.regions(function.code.len()).is_some() { return None; }
        let reads = super::super::compiled::frozen_global_reads(program, function, &globals);
        super::super::compiled::lower_lane_body(program, id as u32, isa, helpers, facts, &reads).map(|(lane, _)| lane)
    }).collect()
}

/// The operation imports of every opcode `bodies` use, appended to `functions`: each
/// opcode's symbol index, by opcode.
pub(super) fn import_operations(
    functions: &mut Vec<NativeFunction>, bodies: &[Vec<InstructionSite>], isa: &dyn TargetIsa,
) -> Result<Vec<Option<u32>>, String> {
    let mut operations = vec![None; OPCODE_COUNT];
    for sites in bodies {
        // Fallback tables name an entry for every PC, so every op is collected.
        for op in sites.iter().map(|site| site.op) {
            if operations[op as usize].is_none() {
                let index = u32::try_from(functions.len())
                    .map_err(|_| "native symbol count exceeds u32")?;
                operations[op as usize] = Some(index);
                functions.push(NativeFunction::Import {
                    name: format!("{}{}", crate::bytecode::NATIVE_JS_OPERATION_PREFIX, opcode_name(op as usize)),
                    signature: super::super::compiled::native_js_operation_signature(isa),
                });
            }
        }
    }
    Ok(operations)
}

/// The dispatcher of function `function_id`'s private regions, whose symbols start at
/// `regions.0` and number `regions.1`.
pub(super) fn lower_object_regions(
    function_id: u32, instruction_count: usize, options: NativeJsCodegenOptions, isa: &dyn TargetIsa,
    regions: (usize, usize),
) -> Result<ir::Function, String> {
    let mut signature = None;
    lower_region_dispatch(
        function_id,
        instruction_count,
        options.region_instruction_budget.get(),
        regions.1,
        isa,
        |builder, index, context, pc, result| {
            let signature = *signature.get_or_insert_with(|| {
                builder.import_signature(super::super::compiled::native_entry_signature(
                    isa.pointer_type(),
                    isa.default_call_conv(),
                ))
            });
            let symbol = u32::try_from(regions.0 + index).expect("validated native region symbol");
            let name = builder
                .func
                .declare_imported_user_function(ir::UserExternalName::new(NATIVE_FUNCTION_NAMESPACE, symbol));
            let callee = builder.import_function(ir::ExtFuncData {
                name: ir::ExternalName::user(name),
                signature,
                colocated: false,
                patchable: false,
            });
            let arguments = region_call_arguments(builder, isa, context, pc, result);
            let call = builder.ins().call(callee, &arguments);
            builder.inst_results(call)[0]
        },
    )
}

/// The native entry of function `function_id`: a trampoline over its lane body and its
/// per-PC fallback, which `symbols` name.
pub(super) fn lower_object_entry(
    function_id: u32, instruction_count: usize, isa: &dyn TargetIsa, symbols: (u32, u32),
) -> Result<ir::Function, String> {
    let (mut body_signature, mut fallback_signature) = (None, None);
    super::super::compiled::lower_ahead_entry(NATIVE_FUNCTION_NAMESPACE, function_id, instruction_count, isa,
        super::super::compiled::AheadTargets {
            body: &mut |builder, words| {
                let callee = import_symbol(builder, &mut body_signature, symbols.0,
                    super::super::compiled::lane_body_signature(isa.pointer_type(), isa.default_call_conv()));
                let call = builder.ins().call(callee, words);
                let results = builder.inst_results(call);
                (results[0], results[1])
            },
            fallback: &mut |builder, [context, pc, result]| {
                let callee = import_symbol(builder, &mut fallback_signature, symbols.1,
                    super::super::compiled::native_js_operation_signature(isa));
                let call = builder.ins().call(callee, &[context, pc, result, result]);
                builder.inst_results(call)[0]
            },
        })
}

/// A call target for function symbol `symbol`, its `signature` imported once.
fn import_symbol(
    builder: &mut FunctionBuilder<'_>, imported: &mut Option<ir::SigRef>, symbol: u32, signature: ir::Signature,
) -> ir::FuncRef {
    let signature = *imported.get_or_insert_with(|| builder.import_signature(signature));
    let name = builder.func.declare_imported_user_function(ir::UserExternalName::new(NATIVE_FUNCTION_NAMESPACE, symbol));
    builder.import_function(ir::ExtFuncData { name: ir::ExternalName::user(name), signature, colocated: false, patchable: false })
}

/// The entry table: the ABI header word, then one entry per function, each a relocation to
/// the function's definition (`first_definition` plus its ID).
pub(super) fn entry_table_data(
    isa: &dyn TargetIsa, namespace: &str, function_count: usize, (abi_function, first_definition): (u32, u32),
) -> Result<NativeData, String> {
    let pointer_bytes = usize::from(isa.pointer_bytes());
    if pointer_bytes != std::mem::size_of::<usize>() {
        return Err("native image pointer width differs from host".into());
    }
    let words = function_count
        .checked_add(crate::NATIVE_JS_IMAGE_HEADER_WORDS)
        .ok_or("native entry table size overflow")?;
    let bytes = words
        .checked_mul(pointer_bytes)
        .ok_or("native entry table size overflow")?;
    let mut addresses = Vec::with_capacity(words);
    addresses.push(NativeFunctionAddress { offset: 0, function: abi_function });
    for id in 0..function_count {
        let word = id + crate::NATIVE_JS_IMAGE_HEADER_WORDS;
        let offset = u32::try_from(word * pointer_bytes)
            .map_err(|_| "native entry table exceeds relocation offsets")?;
        let function = first_definition
            .checked_add(u32::try_from(id).map_err(|_| "native function count exceeds u32")?)
            .ok_or("native function symbol count exceeds u32")?;
        addresses.push(NativeFunctionAddress { offset, function });
    }
    Ok(NativeData {
        name: format!("{namespace}_entries"),
        bytes: vec![0; bytes],
        alignment: pointer_bytes as u64,
        writable: false,
        function_addresses: addresses,
    })
}
