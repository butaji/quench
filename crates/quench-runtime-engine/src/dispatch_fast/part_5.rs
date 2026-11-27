//! Functions of `dispatch_fast` in a codegen unit of their own (B1: a staged edit recompiles only the units that inline it).
#![allow(unused_imports)]
#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt};
use super::*;

/// Whether the iteration step at `pc` (`iteration_step_in_lane`) is a destructuring step as
/// `binding_iterator_step` emits it once its test is threaded: a plain read of the result's
/// `done`, a move of it to another register, a `JumpTrue` of it past the step, and a plain
/// read of the result's `value`. `iterator_step_binding_miss` then makes the reads and the
/// move from the step itself; their atoms it tests as it runs.
pub(super) fn binding_step_in_lane(decoded: &[WideInstruction], pc: usize) -> bool {
    let result = decoded[pc].result_register();
    let record = |offset: usize| decoded.get(pc + offset).copied();
    let reads_result = |offset: usize| {
        record(offset).is_some_and(|read| quench_runtime::staged::reads_named_field(read.op()) && read.result_flags() == 0 && read.b() == result)
    };
    let (Some(done), Some(flag), Some(test), Some(value)) =
        (record(BINDING_DONE_READ), record(BINDING_DONE_MOVE), record(BINDING_DONE_TEST), record(BINDING_VALUE_READ))
    else {
        return false;
    };
    let done = done.result_register();
    reads_result(BINDING_DONE_READ)
        && reads_result(BINDING_VALUE_READ)
        && quench_runtime::staged::copies_register(flag.op())
        && flag.result_flags() == 0
        && flag.b() == done
        && flag.a() != done
        && flag.a() != result
        && quench_runtime::staged::truth_jump(test.op()) == Some(true)
        && test.a() == done
        && test.jump_target() as usize > pc + BINDING_VALUE_READ
        && value.result_register() != done
        && value.result_register() != flag.a()
        && pc + BINDING_VALUE_READ + 1 < decoded.len()
}

/// The row of a `Binary` or `NumericAdd` record whose only result flag is a numeric local target
/// (`FLAG_LOCAL_RESULT`), at accumulator field `acc`; `None` for any other record.
pub(super) fn local_result_row(instruction: WideInstruction, acc: u8) -> Option<Handler> {
    if instruction.result_flags() != quench_runtime::bytecode::NUMERIC_LOCAL_TARGET || !instruction.writes_numeric_local() {
        return None;
    }
    macro_rules! at {
        ($op:expr) => {
            match acc {
                ACC_NONE => generated_row!($op, ACC_NONE, lane_frame::FLAG_LOCAL_RESULT) as Handler,
                ACC_B => generated_row!($op, ACC_B, lane_frame::FLAG_LOCAL_RESULT) as Handler,
                _ => generated_row!($op, ACC_C, lane_frame::FLAG_LOCAL_RESULT) as Handler,
            }
        };
    }
    match instruction.op() as u16 {
        quench_runtime::staged::BINARY_ROW => Some(at!(quench_runtime::staged::BINARY_ROW)),
        quench_runtime::staged::NUMERIC_ADD_ROW => Some(at!(quench_runtime::staged::NUMERIC_ADD_ROW)),
        _ => None,
    }
}

/// Whether `handler` is a numeric local target's row (`local_result_row`).
pub(super) fn is_local_result_row(handler: *const ()) -> bool {
    [quench_runtime::staged::BINARY_ROW, quench_runtime::staged::NUMERIC_ADD_ROW].into_iter().any(|op| {
        let instruction = WideInstruction::new(quench_runtime::bytecode::Op::from_index(usize::from(op)).unwrap(),
            quench_runtime::bytecode::NUMERIC_LOCAL_TARGET, 0, 0, 0);
        [ACC_NONE, ACC_B, ACC_C].into_iter().any(|acc| local_result_row(instruction, acc).is_some_and(|row| row as *const () == handler))
    })
}

/// A nested `GetField` whose only result flag stores its value to a field of `this` too
/// (`row_get_field_sink`).
pub(super) fn sinks_field_in_lane(instruction: WideInstruction) -> bool {
    instruction.result_flags() == quench_runtime::bytecode::SET_THIS_REGISTER
        && instruction.writes_current_this()
}

/// Whether the instruction's only result flag, if any, returns the result from the frame.
pub(super) fn returns_in_lane(instruction: WideInstruction) -> bool {
    instruction.result_flags() & !quench_runtime::bytecode::RETURN_REGISTER == 0
}

/// The register every compiled linear-memory access of `function` goes
/// through, when there is exactly one: the direct accesses', or in a body with none, its
/// vector accesses' (`WasmMemoryAddress`) when the prologue binds it, as lowering binds a
/// memory32 memory once per call.
pub(crate) fn memory_register(function: &Function) -> Option<u16> {
    let instructions = || function.code.iter().map(|packed| function.widened(*packed));
    let direct = instructions().filter_map(quench_runtime::staged::wasm_memory::bound_memory_register);
    single_register(direct).or_else(|| {
        let vector = single_register(
            instructions().filter(|instruction| quench_runtime::staged::wasm_memory_address(instruction.op())).map(|instruction| instruction.c()),
        )?;
        instructions()
            .take_while(|instruction| {
                quench_runtime::staged::wasm_setup_family(instruction.op())
            })
            .any(|instruction| quench_runtime::staged::wasm_instance_binding(instruction.op()) && instruction.a() == vector)
            .then_some(vector)
    })
}

/// The one register `registers` names, when they name exactly one.
pub(super) fn single_register(mut registers: impl Iterator<Item = u16>) -> Option<u16> {
    let first = registers.next()?;
    registers.all(|register| register == first).then_some(first)
}

/// Whether a lane run of `instruction` could write `register`.
pub(super) fn writes_in_lane(instruction: WideInstruction, register: u16) -> bool {
    let op = instruction.op();
    let result = matches!(
        op.field_layout(InstructionField::A),
        FieldLayout::ResultRegister | FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister
    ) && instruction.a() == register;
    let optional = op.field_layout(InstructionField::B) == FieldLayout::OptionalRegister
        && instruction
            .b()
            .checked_sub(quench_runtime::bytecode::OPTIONAL_REGISTER_BIAS)
            == Some(register);
    let filled = quench_runtime::staged::wasm_fill_registers(op)
        && (instruction.b()..instruction.b().saturating_add(instruction.c())).contains(&register);
    result || optional || filled
}

/// The handler of a returning record of `kind` (`returning_kind`) in a view that has no leaf form:
/// each kind is one opcode's (its traits name it), so the record is that opcode's `returning_row`.
pub(super) fn returning_handler(kind: u8) -> Handler {
    match kind {
        RETURNING_FIELD => row_returning_field_first::<NOT_LEAF>,
        RETURNING_UNARY => returning_row_of!(quench_runtime::bytecode::UNARY_ROW, RETURNING_UNARY, NOT_LEAF) as Handler,
        RETURNING_BINARY => returning_row_of!(quench_runtime::bytecode::BINARY_ROW, RETURNING_BINARY, NOT_LEAF) as Handler,
        RETURNING_OBJECT => returning_row_of!(quench_runtime::bytecode::OBJECT_PAIR, RETURNING_OBJECT, NOT_LEAF) as Handler,
        RETURNING_OBJECT_LITERAL => returning_row_of!(quench_runtime::bytecode::OBJECT_LITERAL_ROW, RETURNING_OBJECT_LITERAL, NOT_LEAF) as Handler,
        _ => returning_row_of!(quench_runtime::bytecode::SUPER_CONST_ARRAY_OBJECT, RETURNING_LITERAL, NOT_LEAF) as Handler,
    }
}

/// Whether local `slot` of `function` is a plain frame local: no environment holds it, no mapped
/// `arguments` aliases it and it is no self binding. A checked local row reads and writes such a slot
/// in its frame's locals (`lane_frame::row_frame`); any other slot takes its hand handler.
pub(super) fn plain_frame_local(function: &Function, slot: usize) -> bool {
    !function.local_slot_uses_environment(slot)
        && quench_runtime::vm::arguments::mapped_arguments_slot(function, slot).is_none()
        && !function.is_self_binding_slot(slot)
}

#[cfg(feature = "jit")]
pub(crate) fn derive_lane_view<'live>(
    function_id: u32,
    function: &Function,
    constants: &[Value],
    heap: quench_runtime::vm::tier_queries::HeapReads<'_>,
    kind: quench_runtime::bytecode::ProgramKind,
    live: &dyn Fn() -> Option<&'live std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
    sites: &[quench_runtime::bytecode::FieldSite],
    plan: crate::compiled::admission::RecordPlan,
    diagnostics: bool,
    proper_tail_calls: bool,
    tiers: &'static Tiers,
) -> LaneView {
    let loop_tier = plan.loop_sites;
    // A returning call is admitted by its function and host (`returning_call_admitted`), both fixed here.
    let admits_returns = quench_runtime::staged::call_complete::concrete_admits_returning_call(proper_tail_calls, function.strict);
    // Each pass below reads instructions by PC; they are decoded once, here.
    let decoded: Vec<WideInstruction> = function.instructions().collect();
    // Every backward jump observes host termination, including allocation-free
    // Wasm loops. Collection and stack-floor exits share this safepoint.

    // Only Wasm bodies access linear memory.
    let memory_register = (kind == quench_runtime::bytecode::ProgramKind::Wasm)
        .then(|| memory_register(function))
        .flatten();
    let entries = entered_records(&decoded, function.parameter_end_pc);
    // Wasm instructions, whose fusions the passes under `wasm` make, occur only in Wasm
    // bodies: JavaScript compiles to none of them.
    let wasm = kind == quench_runtime::bytecode::ProgramKind::Wasm;
    let resolves_names = function.has_dynamic_scope_access();
    // One function-wide fact, consulted per local load; it scans the whole body.
    let kept_arguments = std::cell::OnceCell::new();
    let mut producer: Option<u16> = None;
    // Per instruction: the general path executes it, and its record reads
    // the accumulator.
    let mut exits = Vec::with_capacity(function.code.len());
    let mut reads_accumulator = Vec::with_capacity(function.code.len());
    // Per instruction: the register whose value a record reading the accumulator reads
    // there, `NO_ACCUMULATOR` for a record that reads none.
    let mut accumulator_registers = Vec::with_capacity(function.code.len());
    // Per instruction: the register its selected record carries as the
    // accumulator to the record after it.
    let mut produced = Vec::with_capacity(function.code.len());
    let mut records: Box<[LaneInstruction]> = function
        .code
        .iter()
        .enumerate()
        .map(|(pc, _)| {
            let instruction = decoded[pc];
            let op = instruction.op();
            let binds_memory =
                quench_runtime::staged::wasm_instance_binding(op) && Some(instruction.a()) == memory_register;
            let memory_access = MemoryLoad::from_direct_op(op).is_some()
                || MemoryStore::from_direct_op(op).is_some();
            let keeps_view = if memory_access {
                Some(instruction.b()) == memory_register
            } else {
                binds_memory
                    || memory_register.is_none_or(|register| !writes_in_lane(instruction, register))
            };
            let operand = operand_in_lane(instruction, pc, constants)
                .filter(|_| {
                    keeps_view && fields_fit_in_lane(instruction, function) && call_fits_in_lane(instruction)
                })
                .map(|operand| call_result_marked_in_lane(operand, instruction, pc, live))
                .map(|operand| call_awaited_marked_in_lane(operand, &decoded, pc, live));
            // Control reaches this record only from the previous one when nothing enters it
            // (`entered_records`); the general loop never enters at a record that reads the
            // accumulator.
            let carried = producer.take().filter(|_| ACCUMULATOR && !entries.contains(pc));
            let acc = carried.map_or(ACC_NONE, |register| {
                accumulator_field(instruction, register)
            });
            // A table is backward when one of its entries jumps back.
            let backward_jump = if quench_runtime::staged::wasm_branch_table(op) {
                    quench_runtime::bytecode::branch_table_entries(pc, instruction).any(|entry| {
                        decoded[entry].imm() as usize <= pc
                    })
                } else {
                    op.immediate_role() == ImmediateRole::JumpTarget
                        && instruction.imm() as usize <= pc
                };
            // A memory binding installs its view whether or not the record has an operand.
            let selected = (binds_memory || operand.is_some()).then_some(()).and_then(|_| if binds_memory {
                Some(LaneHandler {
                    handler: lane_instance_binding_memory as Handler,
                    produces: false,
                })
            } else if quench_runtime::staged::wasm_f64_unary(op)
                && F64UnaryOperator::from_tag(instruction.imm())
                    == Some(F64UnaryOperator::SquareRoot)
            {
                handler_in_lane(backward_jump, acc, op as usize)
                    .and_then(|_| {
                        LaneHandler::producer(match acc {
                            ACC_NONE => protocol_row!({ quench_runtime::bytecode::WASM_F64_UNARY_ROW as u16 }, ACC_NONE, { protocol_family::LOOP | protocol_misses::loops::F64_SQRT_NONE as u16 }) as Handler,
                            ACC_A => protocol_row!({ quench_runtime::bytecode::WASM_F64_UNARY_ROW as u16 }, ACC_A, { protocol_family::LOOP | protocol_misses::loops::F64_SQRT_A as u16 }) as Handler,
                            ACC_B => protocol_row!({ quench_runtime::bytecode::WASM_F64_UNARY_ROW as u16 }, ACC_B, { protocol_family::LOOP | protocol_misses::loops::F64_SQRT_B as u16 }) as Handler,
                            ACC_C => protocol_row!({ quench_runtime::bytecode::WASM_F64_UNARY_ROW as u16 }, ACC_C, { protocol_family::LOOP | protocol_misses::loops::F64_SQRT_C as u16 }) as Handler,
                            _ => protocol_row!({ quench_runtime::bytecode::WASM_F64_UNARY_ROW as u16 }, ACC_PAIR, { protocol_family::LOOP | protocol_misses::loops::F64_SQRT_PAIR as u16 }) as Handler,
                        })
                    })
            } else if admits_row(op) && op.reads_arguments()
                && function.arguments_slot == u16::try_from(instruction.local_slot()).ok()
                && kept_arguments.get_or_init(|| function.kept_arguments()).is_some()
            {
                // The count of the function's kept arguments: its row with that fact (`row_near`).
                let row = handler_in_lane(backward_jump, acc, op as usize);
                row.map(|row| LaneHandler { handler: row_near::<ARGUMENTS_COUNT>(row.handler as *const ()).unwrap_or(row.handler), ..row })
            } else if quench_runtime::staged::local_read_family(op)
                && function.arguments_slot == u16::try_from(instruction.local_slot()).ok()
                && let Some(kept) = *kept_arguments.get_or_init(|| function.kept_arguments())
            {
                // The binding a keeping activation may leave unmaterialized: its local read's row answers it
                // (`Frame::arguments_binding`). The object is made where no forwarding call follows the read
                // and the activation keeps its values to its last read (`KeptArguments::Passes`); else the marker.
                let forwards = decoded.get(pc + 1).is_some_and(|next| {
                    quench_runtime::staged::forwarded_call_family(next.op()) && next.call_window().last() == Some(instruction.result_register())
                });
                let object = kept == quench_runtime::bytecode::KeptArguments::Passes && !forwards;
                handler_in_lane(backward_jump, acc, op as usize)
                    .and_then(|row| lane_frame::kept_row(row.handler as *const (), object)
                        .map(|handler| LaneHandler { handler, ..row }))
            } else if quench_runtime::staged::calls(op) && iteration_step_in_lane(&decoded, pc) {
                Some(LaneHandler {
                    handler: match for_of_step_in_lane(&decoded, pc) {
                        Some(ForOfTest::Emitted) => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::ITERATION | protocol_misses::iteration::READS_EMITTED as u16 }>(),
                        Some(ForOfTest::Threaded) => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::ITERATION | protocol_misses::iteration::READS_THREADED as u16 }>(),
                        None if binding_step_in_lane(&decoded, pc) => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::ITERATION | protocol_misses::iteration::BINDING as u16 }>(),
                        None => protocol_handler::<{ quench_runtime::bytecode::CALL_ROW as u16 }, ACC_NONE, { protocol_family::ITERATION | protocol_misses::iteration::STEP as u16 }>(),
                    },
                    produces: false,
                })
            } else if spreads_rest_view(instruction) {
                Some(LaneHandler { handler: call_rest_miss as Handler, produces: false })
            } else if quench_runtime::staged::writes_environment_slot(op) && resolves_names {
                None
            } else if quench_runtime::staged::checked_local(op)
                && !plain_frame_local(function, instruction.local_slot())
            {
                // A slot an environment, a mapped `arguments` or the self binding holds keeps its hand
                // handler; `specialize_frame_locals` then gives it the handler of its binding facts.
                Some(LaneHandler { handler: lane_frame::checked_local_hand(op) as Handler, produces: false })
            } else if quench_runtime::staged::reads_named_field(op) && sinks_field_in_lane(instruction) {
                Some(LaneHandler {
                    handler: generated_row!(FIELD_LEAF_READ, ACC_NONE, SINK_SITE) as Handler,
                    produces: false,
                })
            } else if op.immediate_layout() == ImmediateLayout::ConstructCountAndFlags && ImmediateLayout::super_construct(instruction.imm()) {
                // `super(...)`: the parent pushed in place in the frame the lane runs (`derived_frames`).
                Some(LaneHandler { handler: derived_frames::super_construct_in_frame as Handler, produces: false })
            } else if let Some(kind) = returning_kind(instruction) {
                Some(LaneHandler { handler: returning_handler(kind), produces: false })
            } else if let Some(handler) = wasm.then(|| wasm_lane_handler(instruction)).flatten() {
                handler
            } else {
                handler_in_lane(backward_jump, acc, op as usize)
            });
            // A generated row reads its record's `a` as its result register (`LaneFields::wide`);
            // a record whose `a` carries result flags (a numeric local target) runs the general
            // step, which decodes them, so no row tests for them.
            let row = handler_in_lane(backward_jump, acc, op as usize);
            let selected = selected.map(|chosen| {
                let flagged = admits_row(op)
                    && matches!(op.field_layout(InstructionField::A), FieldLayout::ResultRegister)
                    && instruction.a() & !quench_runtime::bytecode::REGISTER_MASK != 0;
                // A row reads `a` as a register, so it cannot carry a numeric local target's flags:
                // such a record steps, whichever row its opcode and accumulator chose (a specialised
                // one included), but a Number row whose only flag is a numeric local target moves its
                // result to the slot itself. A return alone is no such flag: the handlers chosen for
                // it (`returning_kind`, the call rows, a returning field site) read it, and a plain
                // row's record with it steps (`generic`), so a call or `return x | 0` stays in the lane.
                let property = (quench_runtime::staged::reads_named_field(op) || admits_row(op))
                    && matches!(op.field_layout(InstructionField::A), FieldLayout::ResultRegister)
                    && instruction.a() & !(quench_runtime::bytecode::REGISTER_MASK | quench_runtime::bytecode::RETURN_REGISTER) != 0;
                let generic = flagged && row.is_some_and(|row| row.handler as *const () == chosen.handler as *const ());
                if let Some(handler) = generic.then(|| local_result_row(instruction, acc)).flatten() {
                    LaneHandler { handler, ..chosen }
                } else if property || generic {
                    LaneHandler { handler: general_step_in_lane, ..chosen }
                } else {
                    chosen
                }
            });
            producer = selected
                .filter(|selected| selected.produces)
                .map(|_| instruction.a());
            produced.push(producer);
            exits.push(selected.is_none());
            reads_accumulator.push(selected.is_some() && acc != ACC_NONE);
            accumulator_registers.push(
                carried.filter(|_| selected.is_some() && acc != ACC_NONE).unwrap_or(NO_ACCUMULATOR),
            );
            let handler = selected.map_or(exit_lane as Handler, |selected| selected.handler) as *const ();
            LaneInstruction {
                handler,
                operand: operand.unwrap_or(LaneOperand { imm: 0 }),
                a: instruction.a(),
                b: instruction.b(),
                // A kept argument read finds the values after the function's registers, whose
                // count its unused field carries (`ARGUMENT_GET_ROW`).
                c: if op as u16 == quench_runtime::staged::ARGUMENT_GET_ROW {
                    function.registers as u16
                } else {
                    instruction.c()
                },
                call_link: std::cell::Cell::new(NO_CALL_LINK),
                mode: 0,
            }
        })
        .collect();
    // Each record's handler before any fusion: a record still holding it
    // runs only its own instruction.
    let mut selected: Vec<*const ()> = records.iter().map(|record| record.handler).collect();
    // Whether a record's handler serves `op` with accumulator field `acc`.
    let serves = |handler: *const (), op: u16, acc: u8| {
        handler_in_lane(false, acc, usize::from(op))
            .is_some_and(|served| handler == served.handler as *const ())
    };
    // The direction and accumulator field (among `accs`) of the plain handler serving `op`
    // that `handler` is, for an instruction whose handler depends on its jump's direction.
    let plain_served = |handler: *const (), op: u16, accs: &[u8]| {
        [false, true].into_iter().find_map(|backward| {
            accs.iter().copied().find(|&acc| {
                handler_in_lane(backward, acc, usize::from(op))
                    .is_some_and(|served| handler == served.handler as *const ())
            })
            .map(|acc| (backward, acc))
        })
    };
    // Runs of vector instructions execute as one record each (`vector_region`): placed
    // before the fusions below, which then leave each region's first record alone. Only a
    // body with SIMD operators derives the liveness they read.
    let vectors = wasm
        && decoded.iter().any(|instruction| quench_runtime::staged::wasm_simd(instruction.op()) || quench_runtime::staged::wasm_simd_shuffle(instruction.op()));
    let vector_regions: Box<[vector_region::VectorRegion]> = match vectors.then(live).flatten() {
        Some(live) => vector_region::vector_regions(
            &decoded,
            constants,
            heap,
            memory_register,
            live,
            &entries,
            &exits,
            &produced,
        ),
        None => Vec::new(),
    }
    .into_boxed_slice();
    for region in &vector_regions {
        records[region.first()].handler = vector_region::region_handler() as *const ();
        records[region.first()].operand = LaneOperand { region };
    }
    if wasm {
        // A table follows its selected entry's jump, so every entry must run in
        // the lane.
        for (pc, &instruction) in decoded.iter().enumerate() {
            if quench_runtime::staged::wasm_branch_table(instruction.op())
                && quench_runtime::bytecode::branch_table_entries(pc, instruction).any(|entry| exits[entry])
            {
                records[pc].handler = exit_lane as Handler as *const ();
                exits[pc] = true;
                reads_accumulator[pc] = false;
                accumulator_registers[pc] = NO_ACCUMULATOR;
            }
        }
    }
    // A record's plain jump, with its direction.
    let jump_direction = |handler: *const ()| {
        [false, true].into_iter().find(|&backward| {
            handler_in_lane(backward, ACC_NONE, quench_runtime::staged::JUMP_ROW as usize)
                .is_some_and(|jump| handler == jump.handler as *const ())
        })
    };
    let _entry_jumps = branch_table_entry_records(&decoded);
    // A plain move or add of a constant whose next record jumps fuses with that jump
    // (`staged::FUSED_ROWS`, `fuse_rows`), except at a hot OSR latch: its jump record stays
    // visible to `install_loop_sites`, which runs before the fusion pass.
    let _then_fuses = |records: &[LaneInstruction], pc: usize| {
        pc + 1 < records.len()
            && (serves(records[pc].handler, quench_runtime::staged::MOVE_ROW, ACC_NONE) || serves(records[pc].handler, quench_runtime::staged::WASM_I32_ADD_IMMEDIATE_ROW, ACC_NONE))
            && jump_direction(records[pc + 1].handler).is_some_and(|backward| !(loop_tier && backward))
    };
    specialize_returning_binaries(&decoded, constants, &mut records);
    specialize_frame_locals(function, &mut records);
    specialize_captures(&mut records);
    // A Wasm view's records take no JavaScript context (`script_handler!`): it has no environment windows.
    let environment_window = (kind != quench_runtime::bytecode::ProgramKind::Wasm)
        .then(|| specialize_environment_window(function, &decoded, &mut records)).flatten();
    let (loops, osr_rejections) = if loop_tier && kind != quench_runtime::bytecode::ProgramKind::Wasm {
        install_loop_sites(
            function_id,
            function,
            &decoded,
            constants,
            sites,
            live,
            (diagnostics, tiers),
            &mut records,
        )
    } else {
        Default::default()
    };
    // The fused records are script handlers; Wasm bodies run none.
    if !wasm {
        specialize_constant_returns(&mut records);
        specialize_script_calls(&mut records, function.undefined_register, admits_returns);
        specialize_proved_arrays(&decoded, &mut records);
    }
    // Loop kernels take their test records last, keeping each record's final handler.
    // Every mode runs them: a loop a kernel takes keeps no tier site (`loop_sites`), since the
    // compiled tiers project the interpreter's fastest form of the loop, its kernel.
    let loop_kernels = if !wasm {
        loop_kernel::install_loop_kernels(&decoded, constants, function, &exits, &mut records, live().map(|masks| &**masks))
    } else {
        Box::default()
    };
    // Adjacent generated rows the staged pair list names run as one generated record.
    // Wasm records fuse too (a bit-field extraction then its i32 operator).
    // A plain `Binary` record with no accumulator field runs its operator's row rather than fusing
    // (`operator_row`): its operator and operand tests fold, which a fused pair's or chain's run-time
    // operator does not. Records that read the accumulator wait for the pass at the end, past the
    // accumulator hazard check, which recognizes only generated rows.
    // The int32 kinds of the registers (`int_kinds`), which pick the operator records' proven forms.
    // A loop kernel or vector region writes its registers from its own code, under no kind.
    let int_kinds = (!wasm && loop_kernels.is_empty() && vector_regions.is_empty())
        .then(|| int_kinds::analyze(&decoded, constants, live().map(|masks| &**masks), usize::from(function.registers),
            resolves_names, plan.speculates))
        .flatten();
    let mut int_speculating = Vec::new();
    if !wasm {
        let before: Vec<*const ()> = records.iter().map(|record| record.handler).collect();
        specialize_plain_operators(&decoded, constants, &mut records, true, int_kinds.as_ref(), &mut int_speculating);
        for ((record, selected), before) in records.iter().zip(selected.iter_mut()).zip(before) {
            if record.handler != before {
                *selected = record.handler;
            }
        }
    }
    let fused = crate::jit_routes::jit().map_or_else(Vec::new, |routes| (routes.fuse)(&mut records, &decoded, constants, live().map(|masks| &**masks), loop_tier, function.handlers.is_empty()));
    let proven_counters = int_kinds.as_ref().map(|kinds| int_steps::prove_loop_steps(&decoded, &mut records, kinds, &mut int_speculating)).unwrap_or_default();
    if let Some(kinds) = int_kinds.as_ref() { prove_compare_jumps(constants, &mut records, kinds, &mut int_speculating); }
    // A record reads the accumulator its predecessor produced only when control reaches it
    // from that predecessor running its own handler. A fusion replaces a record's handler, or
    // runs records ahead of it and lands past them, without carrying what the replaced or
    // skipped producers wrote (a fused postfix update skips its `IncDec` row), so a plain
    // accumulator reader within reach of a changed record reads its operand from the register.
    // A fused record that ends at the reader's predecessor publishes that predecessor's carry-out
    // (its last link's `Carried`, `fused_rows`, `chain_rows`), so it does not count as a change
    // there, as long as the link publishes the register the reader takes. A change that reports no
    // span (`fuse_target_backedges`) counts as one.
    if !wasm {
        const FUSED_REACH: usize = 6;
        let changed: Vec<bool> = records.iter().zip(&selected).map(|(record, &selected)| record.handler != selected).collect();
        let mut fused_last: Vec<Option<usize>> = vec![None; records.len()];
        for (head, last) in fused {
            fused_last[head] = Some(last);
        }
        for pc in 1..records.len() {
            let publishes = produced[pc - 1].is_some() && lane_frame::row_produces(decoded[pc - 1].op());
            // Whether the change at `head` can leave the reader at `pc` without the carry it expects.
            let hazard = |head: usize| match fused_last[head] {
                Some(last) if last + 1 < pc => false,
                Some(last) if last + 1 == pc => !publishes,
                _ => true,
            };
            if !(pc.saturating_sub(FUSED_REACH)..pc).any(|head| changed[head] && hazard(head)) {
                continue;
            }
            let op = decoded[pc].op() as u16;
            let Some((backward, _)) = plain_served(records[pc].handler, op, &[ACC_A, ACC_B, ACC_C, ACC_PAIR]) else { continue };
            if let Some(plain) = handler_in_lane(backward, ACC_NONE, usize::from(op)) {
                records[pc].handler = plain.handler as *const ();
            }
        }
    }
    // Last, so the fusions above and the accumulator check see each record's generated row.
    if !wasm {
        specialize_plain_operators(&decoded, constants, &mut records, false, None, &mut int_speculating);
        specialize_register_operands(&mut records);
    }
    let min_run = if closes_loop(&decoded, &exits) {
        LANE_ENTRY_MIN_RUN
    } else {
        LANE_ENTRY_MIN_CALL_RUN
    };
    // A resumed JavaScript loop body's straight tail jumps back to its head, so its run
    // continues there; a Wasm body resumes nowhere, and following its jumps only made more
    // of its PCs entries, whose lane runs a `return_call` chain left at once (TOD-19:
    // `return_call_ref.wast` 510 -> 797 ms).
    let runs = runs_ahead_in_lane(&decoded, &exits, min_run, !wasm);
    let mut entries = vec![0u64; runs.len().div_ceil(u64::BITS as usize)];
    for (pc, (run, reads)) in runs.iter().zip(&reads_accumulator).enumerate() {
        if *run >= min_run && !reads {
            entries[pc / u64::BITS as usize] |= 1 << (pc % u64::BITS as usize);
        }
    }
    let entries = entries.into_boxed_slice();
    let returns_from_entry = reads_accumulator.first() == Some(&false) && entry_run_returns(&decoded, &exits);
    let activation = ActivationShape::of(function);
    let hinted_records = |names: bool| -> Box<[u32]> {
        decoded
            .iter()
            .enumerate()
            .filter(|&(pc, instruction)| {
                holds_field_hint(instruction.op())
                    && is_global_name_access(instruction.op()) == names
                    && !loops.iter().any(|site| site.hookable && site.header as usize == pc)
            })
            .map(|(pc, _)| pc as u32)
            .collect()
    };
    let (field_records, name_records) = (hinted_records(false), hinted_records(true));
    let string_case_filters = string_case_filter::specialize(heap, constants, &decoded, &mut records);
    let records = Records::new(records, decoded.iter().fold(false, |seen, instruction| seen || has_operand_feedback(instruction.op())));
    seed_declared_feedback(function, &records);
    // Whether a record may read a field through `this`: the trusted rows' sites (`note_lane_this`).
    let this_rows = decoded.iter().any(|instruction| {
        matches!(instruction.op(), quench_runtime::bytecode::Op::SetThisField | quench_runtime::bytecode::Op::SetThisFieldStrict | quench_runtime::bytecode::Op::CallThisMethod)
            || match instruction.field_lookup() {
                Some(quench_runtime::bytecode::FieldLookup::Atom { base, .. }) => base == quench_runtime::bytecode::FieldBase::THIS,
                Some(quench_runtime::bytecode::FieldLookup::Site(index)) => sites.get(index).is_none_or(|site| site.base == quench_runtime::bytecode::FieldBase::THIS),
                None => false,
            }
            || [instruction.a(), instruction.b(), instruction.c()].into_iter().any(|word| {
                let operand = Operand(word);
                operand.kind() == Some(quench_runtime::bytecode::OperandKind::Field) && sites.get(usize::from(operand.payload())).is_none_or(|site| site.base == quench_runtime::bytecode::FieldBase::THIS)
            })
    });
    let first = records.base();
    // The first loop closing at a backedge owns it, as a front-to-back search would find it.
    let mut loop_at_backedge: Vec<(u32, u32)> = loops.iter().enumerate().map(|(index, site)| (site.backedge, index as u32)).collect();
    loop_at_backedge.sort_unstable();
    loop_at_backedge.dedup_by_key(|&mut (backedge, _)| backedge);
    LaneView {
        records,
        entries,
        tiers,
        memory_register,
        activation,
        this_rows,
        target_body: std::cell::OnceCell::new(),
        _string_case_filters: string_case_filters,
        environment_window,
        returns_from_entry,
        loops,
        loop_at_backedge: loop_at_backedge.into_boxed_slice(),
        vector_regions,
        _loop_kernels: loop_kernels.into_iter().map(|kernel| kernel as Box<dyn std::any::Any>).collect(),
        start: std::cell::Cell::new(first),
        prologue: std::cell::UnsafeCell::new(LaneInstruction {
            handler: std::ptr::null(),
            operand: LaneOperand { imm: 0 },
            a: 0,
            b: 0,
            c: 0,
            call_link: std::cell::Cell::new(NO_CALL_LINK),
            mode: 0,
        }),
        tier_direct: std::cell::Cell::new(None),
        tier: std::cell::OnceCell::new(),
        entry: crate::compiled::entry_cell::EntryCell::new(),
        accumulator_registers: accumulator_registers.into_boxed_slice(),
        plain_backward_jump_sites: decoded
            .iter()
            .enumerate()
            .filter(|(pc, instruction)| quench_runtime::staged::jumps(instruction.op()) && instruction.jump_target() <= *pc as u32)
            .count(),
        osr_rejections: (!osr_rejections.is_empty())
            .then(|| osr_rejections.into_boxed_slice()),
        field_records,
        name_records,
        int_speculating: int_speculating.into_boxed_slice(),
        proven_counters: proven_counters.into_boxed_slice(),
        speculative_registers: int_kinds.as_ref().map(|kinds| kinds.speculative().collect()).unwrap_or_default(),
    }
}

/// Whether a record holds an own-field hint (`LaneInstruction::field_hint`) in a `field`
/// operand: every field read or store's of its receiver, and a global name access's or top-level
/// `var` capture read's of the global object (`name_row`, `lane_load_root_var`).
pub(super) fn holds_field_hint(op: Op) -> bool {
    quench_runtime::staged::stores_field(op) || quench_runtime::staged::reads_named_field(op) || quench_runtime::staged::reads_capture(op) || is_global_name_access(op)
}

/// Whether an instruction is a by-name load (`LOAD_NAME`) or store, whose hint a global
/// lexical declaration clears (`LaneView::clear_name_hints`).
pub(super) fn is_global_name_access(op: Op) -> bool {
    op.traits().has(quench_runtime::bytecode::Traits::GLOBAL_NAME_ACCESS)
}

/// Whether an instruction's record leaves its `call_link` unread: any but a call's, whose
/// handlers keep their target hint there (`linked_script_call_entry`).
pub(super) fn holds_no_call_link(op: Op) -> bool {
    !op.traits().has(quench_runtime::bytecode::Traits::CALL_LINK)
}

/// The handler of a loop site's backward jump: the jump row in the loop's flag (`FLAG_LOOP`), which
/// takes the jump while the site warms up and leaves any other to `loop_jump_miss`.
pub(super) fn loop_jump_row() -> Handler {
    row_jump!(JUMP, true, FLAG_LOOP, { quench_runtime::staged::NO_LINK }, { quench_runtime::staged::NO_LINK }, { quench_runtime::staged::NO_LINK }) as Handler
}

/// Gives each backward `Jump` whose loop the shared compiler admits (`analyze_osr_region`) the
/// record that counts it and enters its native code (`loop_jump_row`). Runs after every
/// other pass, which read a jump record's distance from its operand.
pub(super) fn install_loop_sites<'live>(
    function_id: u32,
    function: &Function,
    decoded: &[WideInstruction],
    constants: &[Value],
    sites: &[quench_runtime::bytecode::FieldSite],
    live: &dyn Fn() -> Option<&'live std::rc::Rc<quench_runtime::compile::liveness::RegisterMasks>>,
    (diagnostics, tiers): (bool, &'static Tiers),
    records: &mut [LaneInstruction],
) -> (Box<[LoopSite]>, Vec<(crate::compiled::RegionRejection, usize)>) {
    let backward = jump_row::<JUMP>(true) as *const ();
    let table_entries = branch_table_entry_records(decoded);
    // A fused record ahead of a jump runs that jump itself, reading its offset from the jump's
    // record: a site there would replace the offset those records read with the site (the eslint
    // npm scenario under --jit jumped to a site address read as an offset). The sites are backward
    // jumps, and a move or add never fuses into a hot OSR latch (`then_fuses`), so only a move
    // chain can sit ahead of one.
    let fused_into = |pc: usize| {
        let pair = pc >= 2
            && records[pc - 2].handler == move_pair_jump_handler() as *const ();
        // A branch table takes its entry's jump offset the same way (`row_jump`, `Edge::Entry`):
        // babel-minify-wtb's generator `switch` under --jit jumped to a site read as an offset.
        pair || table_entries.contains(pc)
    };
    // The masks are derived for the first loop site only: a body without a backward jump
    // needs none, and every view under a loop tier had derived them (npm scenarios under
    // --jit: axios -3.6%, playwright -2.5% instructions).
    let masks = std::cell::OnceCell::new();
    let live = || *masks.get_or_init(live);
    let mut rejections = Vec::new();
    let sites: Vec<(usize, LoopSite)> = (0..records.len())
        .filter(|pc| records[*pc].handler == backward && matches!(quench_runtime::staged::Operation::<(), ()>::decode(decoded[*pc], quench_runtime::staged::DecodeContext::EMPTY), Some(quench_runtime::staged::Operation::Jump)) && !fused_into(*pc))
        .map(|pc| {
            let header = decoded[pc].jump_target();
            let (region, promotion_only, analysis_failure, call_boundary_pcs) = match crate::compiled::analyze_osr_region_detailed(
                function,
                decoded,
                constants,
                sites,
                live(),
                header,
                pc as u32, crate::compiled::MethodCalls::Inliner, &|_| false,
            ) {
                Ok(region) => {
                    let call_boundary_pcs = region.call_boundary_pcs();
                    (std::cell::OnceCell::from(region), false, None, call_boundary_pcs)
                }
                // A call may run inlined: the site makes its region once it is hot.
                Err(failure)
                    if failure.is_call_boundary() && live().is_some() =>
                {
                    (
                        std::cell::OnceCell::new(),
                        false,
                        Some(failure),
                        vec![failure.pc],
                    )
                }
                Err(failure) => {
                    let reason = failure.reason;
                    if diagnostics {
                        if let Some((_, count)) =
                            rejections.iter_mut().find(|(existing, _)| *existing == reason)
                        {
                            *count += 1;
                        } else {
                            rejections.push((reason, 1));
                        }
                    }
                    // A loop the region analysis rejects keeps its site, diagnostics or not: its
                    // function's lane body may run it (`loop_retire_in_lane`), and the
                    // site retires to the plain jump where none does. A diagnostics run tiers
                    // as any other.
                    (std::cell::OnceCell::new(), false, Some(failure), Vec::new())
                }
            };
            (
                pc,
                LoopSite {
                    function: function_id,
                    promotion_only,
                    analysis_failure,
                    call_boundary_pcs: call_boundary_pcs.into_boxed_slice(),
                    region,
                    inline_region: std::cell::OnceCell::new(),
                    header,
                    backedge: pc as u32,
                    jump: records[pc].jump(),
                    header_handler: std::cell::Cell::new(std::ptr::null()),
                    hookable: holds_no_call_link(decoded[header as usize].op()),
                    taken: std::cell::Cell::new(0),
                    needed: std::cell::Cell::new(tiers.loops.warmup_taken),
                    entering: std::cell::Cell::new(true),
                    enclosing: std::cell::OnceCell::new(),
                    declines_in_row: std::cell::Cell::new(0),
                    declines: Default::default(),
                    skipped_entries: std::cell::Cell::new(0),
                    diagnostics,
                    tiers,
                    lane_backedge_count: std::cell::Cell::new(0),
                    native_entry_attempts: std::cell::Cell::new(0),
                    native_region_runs: std::cell::Cell::new(0),
                    tier_ups: std::cell::Cell::new(0),
                    body_runs: std::cell::Cell::new(0),
                    entry_guard_misses: std::cell::Cell::new(0),
                    native_exits: std::cell::Cell::new(0),
                    widenings: std::cell::Cell::new(0),
                    compile_attempts: std::cell::Cell::new(0),
                    compile_successes: std::cell::Cell::new(0),
                    tier: std::cell::RefCell::default(), body: std::cell::RefCell::new(LoopBody::initial(tiers)), body_seen: std::cell::Cell::new(0), body_wait: std::cell::Cell::new(0), body_offered: std::cell::Cell::new(false),
                    refused_epoch: std::cell::Cell::new(loop_runs::NO_REFUSAL), refused_view: std::cell::Cell::new(std::ptr::null()), refused_wait: std::cell::Cell::new(0), refused_span: std::cell::Cell::new(0), enclosed: std::cell::Cell::new(true),
                },
            )
        })
        .collect();
    let mut sites = sites;
    // The tiers run a loop the interpreter's kernel takes slower than the kernel does (`Recurrence::beats_rows`): in
    // every mode that loop keeps no site, so its kernel runs it (`install_loop_kernels`).
    sites.retain(|(pc, site)| !loop_kernel::LoopKernel::beats_rows(decoded, constants, function, (site.header as usize, *pc), live().map(|masks| &**masks)));
    let headers: Vec<u32> = sites.iter().map(|(_, site)| site.header).collect();
    for (pc, site) in &mut sites {
        // The header names its site by the distance to the site's jump (`header_site_in_lane`).
        site.hookable &= headers.iter().filter(|header| **header == site.header).count() == 1
            && u16::try_from(*pc - site.header as usize).is_ok_and(|distance| distance < NO_CALL_LINK);
    }
    let pcs: Vec<usize> = sites.iter().map(|(pc, _)| *pc).collect();
    let sites: Box<[LoopSite]> = sites.into_iter().map(|(_, site)| site).collect();
    // The loops each site lies inside that the region analysis admits: such a loop takes the region, and the
    // site runs as plain jumps inside it (`LoopSite::inside_admitted_loop`), so the outermost admitted loop
    // is the one entered per run, not an inner loop re-entered per outer iteration.
    for (index, site) in sites.iter().enumerate() {
        let enclosing: Box<[*const LoopSite]> = sites.iter().enumerate()
            .filter(|(other, outer)| *other != index && outer.header <= site.header && site.backedge <= outer.backedge && outer.is_admitted())
            .map(|(_, outer)| std::ptr::from_ref(outer))
            .collect();
        let _ = site.enclosing.set(enclosing);
    }
    for (pc, site) in pcs.into_iter().zip(sites.iter()) {
        records[pc].handler = loop_jump_row() as *const ();
        let placed = records[pc].set_loop_site(site);
        debug_assert!(placed, "the site's address fits 48 bits");
        if site.hookable {
            records[site.header as usize].call_link.set((pc - site.header as usize) as u16);
        }
    }
    (sites, rejections)
}

/// Gives each `Binary` record that a `Return` consumes its returning form
/// (`specialize_returning_operator`, the calls thread's). Every other record keeps the handler
/// its generated row or jump row gave it: operand kinds and operators are the rows' own decode
/// (`Binary`, `JumpBinaryFalse`, `Unary`), not handler choices made here.
pub(super) fn specialize_returning_binaries(decoded: &[WideInstruction], constants: &[Value], records: &mut [LaneInstruction]) {
    let returning_binary = returning_row_of!(quench_runtime::bytecode::BINARY_ROW, RETURNING_BINARY, NOT_LEAF) as *const ();
    for (pc, &instruction) in decoded.iter().enumerate() {
        if records[pc].handler == returning_binary {
            specialize_returning_operator(instruction, constants, &mut records[pc]);
        }
    }
}

/// Whether `instruction` is a plain `Binary` of an arithmetic operator (`arithmetic_operators!`), which
/// `specialize_plain_operators` gives its operator's row.
pub(super) fn arithmetic_binary(instruction: WideInstruction) -> bool {
    macro_rules! pick { ($op:expr) => { () }; }
    quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Binary) && instruction.result_flags() == 0
        && arithmetic_operators!(instruction.binary_operator(), pick).is_some()
}

/// Gives each plain `Binary` record of an arithmetic operator that still runs its generated row, at
/// any accumulator field, that operator's `operator_row`: the operator becomes a parameter of the
/// instantiation, so the row neither decodes nor dispatches on it (179 against 64 instructions for an
/// int32 add). Its operand kinds and any int32 constant operand pick the form (`OPERANDS_*`). Its misses go back to the
/// generated row, so every operand pair stays decided by the one staged row.
///
/// Where the registers' int32 kinds (`kinds`) prove every register operand of a record with no
/// accumulator field an int32, it takes the proven form (`OPERANDS_PROVEN`, `OPERANDS_PROVEN_INT32`),
/// which reads them with no tag test; one that reads a speculative register is listed in
/// `speculating` with its checked form (`LaneView::retire_int_speculation`). Where the kinds prove no such
/// operand, a register pair takes the guarded form (`OPERANDS_GUARDED`): it tests the tags and runs the proven one.
pub(super) fn specialize_plain_operators(decoded: &[WideInstruction], constants: &[Value], records: &mut [LaneInstruction], accumulator_free: bool,
    kinds: Option<&int_kinds::IntKinds>, speculating: &mut Vec<(u32, *const (), *const ())>) {
    use quench_runtime::bytecode::OperandKind::{Constant, Register};
    use lane_frame::{OPERANDS_ANY, OPERANDS_GUARDED, OPERANDS_INT32, OPERANDS_PROVEN, OPERANDS_PROVEN_INT32, OPERANDS_REGISTERS, OPERANDS_REGISTER_CONSTANT, OPERANDS_REGISTER_INT32};
    macro_rules! row_at {
        ($acc:expr) => {
            generated_row!(quench_runtime::staged::BINARY_ROW, $acc, FLAG_UNREAD) as Handler as *const ()
        };
    }
    let plain = [row_at!(ACC_NONE), row_at!(ACC_B), row_at!(ACC_C)];
    for (pc, &instruction) in decoded.iter().enumerate() {
        let Some(acc) = plain.iter().position(|row| *row == records[pc].handler) else { continue };
        if accumulator_free && acc != 0 {
            continue;
        }
        if instruction.result_flags() != 0 || instruction.numeric_local_target().is_some() {
            continue;
        }
        let int32_constant = |operand: quench_runtime::bytecode::Operand| operand.kind() == Some(Constant)
            && constants.get(usize::from(operand.payload())).is_some_and(|constant| constant.as_int().is_some());
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        let form = match (left.kind(), right.kind()) {
            (Some(Register), Some(Register)) => OPERANDS_REGISTERS,
            (Some(Register), Some(Constant)) if int32_constant(right) => OPERANDS_REGISTER_INT32,
            (Some(Register), Some(Constant)) => OPERANDS_REGISTER_CONSTANT,
            _ if int32_constant(left) || int32_constant(right) => OPERANDS_INT32,
            _ => OPERANDS_ANY,
        };
        // The int32 form's constant, folded into the record (`operator_row` reads it from `mode`, which
        // no fused record has here: fusion runs after this pass and assigns `mode` with its handler).
        if form == OPERANDS_REGISTER_INT32 && let Some(constant) = constants.get(usize::from(right.payload())).and_then(|constant| constant.as_int()) {
            records[pc].mode = u64::from(constant as u32);
        }
        macro_rules! at_form {
            ($acc:expr, $op:expr) => {
                match form {
                    OPERANDS_REGISTERS => operator_row::<{ quench_runtime::bytecode::BINARY_ROW as u16 }, { $acc }, { $op as u8 }, OPERANDS_REGISTERS> as Handler,
                    OPERANDS_REGISTER_INT32 => operator_row::<{ quench_runtime::bytecode::BINARY_ROW as u16 }, { $acc }, { $op as u8 }, OPERANDS_REGISTER_INT32> as Handler,
                    OPERANDS_REGISTER_CONSTANT => operator_row::<{ quench_runtime::bytecode::BINARY_ROW as u16 }, { $acc }, { $op as u8 }, OPERANDS_REGISTER_CONSTANT> as Handler,
                    OPERANDS_INT32 => operator_row::<{ quench_runtime::bytecode::BINARY_ROW as u16 }, { $acc }, { $op as u8 }, OPERANDS_INT32> as Handler,
                    _ => operator_row::<{ quench_runtime::bytecode::BINARY_ROW as u16 }, { $acc }, { $op as u8 }, OPERANDS_ANY> as Handler,
                }
            };
        }
        macro_rules! pick {
            ($op:expr) => {
                match acc {
                    0 => at_form!(ACC_NONE, $op),
                    1 => at_form!(ACC_B, $op),
                    _ => at_form!(ACC_C, $op),
                }
            };
        }
        if let Some(handler) = arithmetic_operators!(instruction.binary_operator(), pick) {
            records[pc].handler = handler as *const ();
            // The proven form, where the kinds prove each register operand an int32.
            let proven = |operand: quench_runtime::bytecode::Operand| operand.register_index().is_some_and(|register| kinds.is_some_and(|kinds| kinds.proven(register)));
            let speculative = |operand: quench_runtime::bytecode::Operand| operand.register_index().is_some_and(|register| kinds.is_some_and(|kinds| kinds.of(register) == int_kinds::Kind::Speculative));
            let proven_form = match form {
                OPERANDS_REGISTERS if acc == 0 && proven(left) && proven(right) => Some(OPERANDS_PROVEN),
                OPERANDS_REGISTER_INT32 if acc == 0 && proven(left) => Some(OPERANDS_PROVEN_INT32),
                _ => None,
            };
            macro_rules! proven_at {
                ($op:expr) => {
                    if proven_form == Some(OPERANDS_PROVEN) {
                        operator_row::<{ quench_runtime::bytecode::BINARY_ROW as u16 }, ACC_NONE, { $op as u8 }, OPERANDS_PROVEN> as Handler
                    } else {
                        operator_row::<{ quench_runtime::bytecode::BINARY_ROW as u16 }, ACC_NONE, { $op as u8 }, OPERANDS_PROVEN_INT32> as Handler
                    }
                };
            }
            if proven_form.is_some() && let Some(proven_handler) = arithmetic_operators!(instruction.binary_operator(), proven_at) {
                if speculative(left) || speculative(right) {
                    speculating.push((pc as u32, proven_handler as *const (), handler as *const ()));
                }
                records[pc].handler = proven_handler as *const ();
            }
            // The guarded form, where the kinds prove neither register of a register pair: the same operator's
            // instance that tests the tags once and runs the proven one (`OPERANDS_GUARDED`). Not listed in
            // `speculating`: its guard is the check, and its general form is the record's requicken target. A
            // register and an int32 constant keep their int32 form, which tests one tag already.
            macro_rules! guarded_at {
                ($op:expr) => { operator_row::<{ quench_runtime::bytecode::BINARY_ROW as u16 }, ACC_NONE, { $op as u8 }, OPERANDS_GUARDED> as Handler };
            }
            if proven_form.is_none() && form == OPERANDS_REGISTERS && acc == 0 && let Some(guarded_handler) = arithmetic_operators!(instruction.binary_operator(), guarded_at) {
                records[pc].handler = guarded_handler as *const ();
            }
        } else if instruction.binary_operator() == oxc_ast::ast::BinaryOperator::StrictEquality as u32 {
            records[pc].handler = strict_equality_row::<false> as Handler as *const ();
        } else if instruction.binary_operator() == oxc_ast::ast::BinaryOperator::StrictInequality as u32 {
            records[pc].handler = strict_equality_row::<true> as Handler as *const ();
        }
    }
}

/// Runs each `LoadConst` whose register the next record, a `Return`, returns as one record
/// (`return_constant_miss`). The `Return` record stays for jumps that reach it, and the
/// fused record's general return continues there.
pub(super) fn specialize_constant_returns(records: &mut [LaneInstruction]) {
    let load = generated_row!(quench_runtime::staged::LOAD_CONST_ROW, ACC_NONE, FLAG_UNREAD) as Handler as *const ();
    let ret = return_row!(quench_runtime::bytecode::RETURN_ROW, returning_code(RETURN_KIND, NOT_LEAF)) as *const ();
    for pc in 0..records.len().saturating_sub(1) {
        if records[pc].handler == load && records[pc + 1].handler == ret && records[pc + 1].a == records[pc].a {
            records[pc].handler = return_row!(quench_runtime::bytecode::RETURN_ROW, returning_code(CONSTANT_KIND, NOT_LEAF)) as *const ();
        }
    }
}

/// The proved row of the element record running `handler` (`specialize_proved_arrays`): the row of
/// the same opcode with `FLAG_SET`, which reads or replaces its base's element through the proof of
/// the array's storage (`proof_span`) and runs its own full check where that declines.
pub(super) fn proved_row(handler: *const ()) -> Option<Handler> {
    if plain_row::<ELEMENT_READ>(handler) {
        return Some(generated_row!(ELEMENT_READ, ACC_NONE, FLAG_SET));
    }
    // A store keeps its generic record (`set_index_miss`) until a proof names it: the keyed store
    // has no row of its own in the lane, so this is the one place a store's row is installed.
    if handler == protocol_row!(ELEMENT_WRITE, ACC_NONE, { protocol_family::ELEMENT | protocol_misses::element::SET_INDEX as u16 }) as Handler as *const () {
        return Some(generated_row!(ELEMENT_WRITE, ACC_NONE, FLAG_SET));
    }
    None
}

/// The proved element read row (`proved_row` of `ELEMENT_READ`), which the primitive bench counts.
#[cfg(test)]
pub(crate) fn proved_element_read_row() -> *const () { generated_row!(ELEMENT_READ, ACC_NONE, FLAG_SET) as Handler as *const () }

/// The proved element write row (`proved_row` of `ELEMENT_WRITE`), which the primitive bench counts.
#[cfg(test)]
pub(crate) fn proved_element_write_row() -> *const () { generated_row!(ELEMENT_WRITE, ACC_NONE, FLAG_SET) as Handler as *const () }

/// The span of the array a proved element row (`proved_row`) reads or stores through the proof of its
/// base register's slot (`ArrayStorageProofs::read_span`, `write_span`), proving the array first where
/// the slot holds another. `None` where the array takes the row's own check instead.
///
/// # Safety
/// `context` is the running lane context of `H`, live for the row.
pub(super) unsafe fn proof_span(context: *mut u8, register: u16, base: Value, writable: bool)
    -> Option<(std::ptr::NonNull<Value>, usize)> {
    let cx = context.cast::<LaneContext>();
    // SAFETY: the context's proofs and its VM are distinct, both live for the run.
    let (proofs, vm) = unsafe { (&mut (*cx).proofs, vm(cx)) };
    if !proofs.holds(register, base) && !vm.prove_array_storage(proofs, register, base) {
        return None;
    }
    if writable { proofs.write_span(register, base) } else { proofs.read_span(register, base) }
}

/// Gives each element read and store whose register base its innermost loop does not
/// redefine the proved row (`proved_row`), which reaches the element through the
/// run's proof of the array's storage (`ArrayStorageProofs`) instead of decoding the array:
/// the loop proves the array once, at its first access, and each access after checks only
/// that its slot still proves the base. A base whose proof slot another such base of the
/// same loop takes keeps its record, as their accesses would evict each other's proofs.
/// V8-v7 NavierStokes, whose `lin_solve` row loop reads four elements and stores one each
/// iteration: 795.3 -> 717.4 instructions per element stored, Callgrind, no-JIT static
/// production build, compile cache and startup snapshot off, x86_64 4 vCPU, 2026-11-01;
/// Crypto -4.7% instructions at fixed iterations, the other six within +0.06%.
///
/// The proofs alone decide soundness, whatever control flow does: a base the loop
/// redefines through a definition `defined_register` does not report only misses its
/// proof, and its record takes the generic one. Aggregate profiles count each generic
/// store, so they keep the generic records.
pub(super) fn specialize_proved_arrays(decoded: &[WideInstruction], records: &mut [LaneInstruction]) {
    if cfg!(feature = "profile-aggregate") {
        return;
    }
    let loops: Vec<std::ops::RangeInclusive<usize>> = decoded
        .iter()
        .enumerate()
        .filter(|(pc, instruction)| {
            instruction.op().immediate_role() == ImmediateRole::JumpTarget && instruction.imm() as usize <= *pc
        })
        .map(|(pc, instruction)| instruction.imm() as usize..=pc)
        .collect();
    // Each candidate: its record, proved row, base register and innermost loop.
    let candidates: Vec<(usize, Handler, u16, usize)> = (0..records.len())
        .filter_map(|pc| {
            let proved = proved_row(records[pc].handler)?;
            let base = records[pc].b;
            let (index, _) = loops
                .iter()
                .enumerate()
                .filter(|(_, body)| body.contains(&pc))
                .min_by_key(|(_, body)| body.end() - body.start())?;
            decoded[loops[index].clone()]
                .iter()
                .all(|instruction| instruction.defined_register() != Some(base))
                .then_some((pc, proved, base, index))
        })
        .collect();
    for &(pc, proved, base, index) in &candidates {
        let slot = quench_runtime::tier_surface::ArrayStorageProofs::slot(base);
        let shared = candidates.iter().any(|&(_, _, other, other_index)| {
            other_index == index && other != base && quench_runtime::tier_surface::ArrayStorageProofs::slot(other) == slot
        });
        if !shared {
            records[pc].handler = proved as *const ();
        }
    }
}

/// Whether the target run holds: `PREFIX` (when there is one) at the target, `COPY` (when there is one)
/// after it, then `TEST`. With no prefix, `TEST` is the target record itself.
pub(super) fn target_run_holds<const PREFIX: u16, const COPY: u16, const TEST: u16>(target: *const (), after: &dyn Fn(usize) -> *const ()) -> bool {
    if const { PREFIX == quench_runtime::staged::NO_LINK } {
        lane_frame::plain_row::<TEST>(target)
    } else if const { COPY == quench_runtime::staged::NO_LINK } {
        lane_frame::plain_row::<PREFIX>(target) && lane_frame::plain_row::<TEST>(after(1))
    } else {
        lane_frame::plain_row::<PREFIX>(target) && lane_frame::plain_row::<COPY>(after(1)) && lane_frame::plain_row::<TEST>(after(2))
    }
}

/// The `FUSED_TARGET` form of the pair `(A, B)`, a back edge whose target run is `PREFIX` then `TEST`:
/// the fused record `fused_rows` makes with that run.
pub(super) fn pair_target_form<const A: u16, const B: u16, const PREFIX: u16, const TEST: u16>() -> TargetForm {
    (
        target_run_holds::<PREFIX, { quench_runtime::staged::NO_LINK }, TEST> as fn(*const (), &dyn Fn(usize) -> *const ()) -> bool,
        // `B` is a jump whose direction is its distance (`fused_rows` reads `BACKWARD` only for a branch table).
        fused_rows!(A, B, false, { quench_runtime::staged::FUSED_TARGET }, TEST, PREFIX, 0) as Handler,
    )
}

/// The `(IncDec, Jump)` back edge whose target is its loop's test alone, `fused_rows` with the test's
/// comparison fixed (`CMP`, `lane_frame::target_comparison`).
pub(super) const fn inc_dec_test_row<const CMP: u8>() -> Handler {
    fused_rows!({ quench_runtime::staged::INC_DEC_ROW }, { quench_runtime::staged::JUMP_ROW }, false, { quench_runtime::staged::FUSED_TARGET },
        { quench_runtime::staged::COMPARE_JUMP_ROW }, { quench_runtime::staged::NO_LINK }, CMP) as Handler
}

/// The (checked, proven) handler pairs of the back edge over a register-register comparison (`target_comparison`:
/// odd codes; even ones name a constant).
pub(super) fn proven_step_forms() -> [(Handler, Handler); 6] {
    macro_rules! forms { ($($code:literal),*) => { [$((inc_dec_test_row::<$code>(), inc_dec_test_row::<{ $code | lane_frame::PROVEN_STEP }>())),*] }; }
    forms!(1, 3, 5, 7, 9, 11)
}

/// The (checked, proven) pair whose checked form is `handler`.
pub(super) fn checked_step(handler: *const ()) -> Option<(Handler, Handler)> {
    proven_step_forms().into_iter().find(|(checked, _)| *checked as *const () == handler)
}

/// The comparison jumps whose operands the int32 kinds prove (`lane_frame::proven_compare_jump`): each takes its proven
/// instance, which reads its register operands untagged. One that reads a speculative register is listed in
/// `speculating` with its formed instance (`LaneView::retire_int_speculation`), which it takes back once an update
/// overflows that register. A register pair the kinds do not prove takes its guarded instance (`guarded_compare_jump`),
/// which tests the tags once per execution. Runs after fusion, so the fused runs and back edges keep the plain rows they were built on.
pub(super) fn prove_compare_jumps(constants: &[Value], records: &mut [LaneInstruction], kinds: &int_kinds::IntKinds,
    speculating: &mut Vec<(u32, *const (), *const ())>) {
    use quench_runtime::bytecode::Operand;
    let proven = |operand: Operand| operand.register_index().is_some_and(|register| kinds.proven(register));
    let speculative = |operand: Operand| operand.register_index().is_some_and(|register| kinds.of(register) == int_kinds::Kind::Speculative);
    for (pc, record) in records.iter_mut().enumerate() {
        let (left, right) = (Operand(record.b), Operand(record.c));
        let Some((proven_handler, formed)) = lane_frame::proven_compare_jump(record, constants, proven) else {
            if let Some(guarded) = lane_frame::guarded_compare_jump(record) { record.handler = guarded as *const (); }
            continue
        };
        if speculative(left) || speculative(right) {
            speculating.push((pc as u32, proven_handler as *const (), formed as *const ()));
        }
        record.handler = proven_handler as *const ();
    }
}

/// `handler`, a back edge's `FUSED_TARGET` record, with its test's comparison fixed where it is the
/// `(IncDec, Jump)` pair's run of the test `test` alone and that test is one `target_comparison`
/// fixes: the loop's step then switches on no operator. Any other handler is itself.
pub(super) fn fixed_comparison(handler: Handler, test: &LaneInstruction) -> Handler {
    if handler as *const () != inc_dec_test_row::<0>() as *const () { return handler }
    match lane_frame::target_comparison(test) {
        1 => inc_dec_test_row::<1>(), 2 => inc_dec_test_row::<2>(), 3 => inc_dec_test_row::<3>(),
        4 => inc_dec_test_row::<4>(), 5 => inc_dec_test_row::<5>(), 6 => inc_dec_test_row::<6>(),
        7 => inc_dec_test_row::<7>(), 8 => inc_dec_test_row::<8>(), 9 => inc_dec_test_row::<9>(),
        10 => inc_dec_test_row::<10>(), 11 => inc_dec_test_row::<11>(), 12 => inc_dec_test_row::<12>(),
        _ => handler,
    }
}

/// The `FUSED_TARGET` form of a lone backward jump whose target run is `PREFIX` then `TEST`: the jump's
/// record `row_jump` with that run (`target_run`).
pub(super) fn lone_target_form<const PREFIX: u16, const TEST: u16>() -> TargetForm {
    (
        target_run_holds::<PREFIX, { quench_runtime::staged::NO_LINK }, TEST> as fn(*const (), &dyn Fn(usize) -> *const ()) -> bool,
        row_jump!(JUMP, true, FLAG_UNREAD, PREFIX, { quench_runtime::staged::NO_LINK }, TEST) as Handler,
    )
}

/// The `FUSED_TARGET` form of a lone backward jump whose target run is `PREFIX`, a copy (`FUSED_TARGET_COPY_ROWS`)
/// and `TEST`: the jump's record `row_jump` with that run, its copy slot the copy (`target_run`).
pub(super) fn lone_copy_target_form<const PREFIX: u16, const TEST: u16>() -> TargetForm {
    (
        target_run_holds::<PREFIX, { quench_runtime::staged::FUSED_TARGET_COPY_ROWS[0] }, TEST> as fn(*const (), &dyn Fn(usize) -> *const ()) -> bool,
        row_jump!(JUMP, true, FLAG_UNREAD, PREFIX, { quench_runtime::staged::FUSED_TARGET_COPY_ROWS[0] }, TEST) as Handler,
    )
}

/// The `FUSED_TARGET` forms of the pair `(A, B)`'s back edge.
pub(super) fn pair_target_forms<const A: u16, const B: u16>() -> [TargetForm; 12] {
    target_forms!(pair_target_form, A, B)
}

/// The `FUSED_TARGET` forms of a lone backward jump.
pub(super) fn lone_target_forms() -> [TargetForm; 12] {
    target_forms!(lone_target_form)
}

/// The nine `FUSED_TARGET` forms of a lone back jump's copy run: each `FUSED_TARGET_PREFIX_ROWS` row, then
/// the copy, then each `FUSED_TARGET_ROWS` test.
pub(super) fn lone_copy_target_forms() -> [TargetForm; 9] {
    [
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[0] }, { quench_runtime::staged::FUSED_TARGET_ROWS[0] }>(),
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[0] }, { quench_runtime::staged::FUSED_TARGET_ROWS[1] }>(),
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[0] }, { quench_runtime::staged::FUSED_TARGET_ROWS[2] }>(),
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[1] }, { quench_runtime::staged::FUSED_TARGET_ROWS[0] }>(),
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[1] }, { quench_runtime::staged::FUSED_TARGET_ROWS[1] }>(),
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[1] }, { quench_runtime::staged::FUSED_TARGET_ROWS[2] }>(),
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[2] }, { quench_runtime::staged::FUSED_TARGET_ROWS[0] }>(),
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[2] }, { quench_runtime::staged::FUSED_TARGET_ROWS[1] }>(),
        lone_copy_target_form::<{ quench_runtime::staged::FUSED_TARGET_PREFIX_ROWS[2] }, { quench_runtime::staged::FUSED_TARGET_ROWS[2] }>(),
    ]
}

/// The handler of the first form of `forms` whose target run holds at `target` (`after` as `target_run_holds`).
pub(super) fn target_run_handler(forms: &[TargetForm], target: *const (), after: &dyn Fn(usize) -> *const ()) -> Option<Handler> {
    forms.iter().find(|(holds, _)| holds(target, after)).map(|(_, handler)| *handler)
}

/// The handler of a lone back jump's target run at `target`: the first of `lone` that holds, else, where the
/// record after the target is a copy (`FUSED_TARGET_COPY_ROWS`) whose destination the test after it names
/// (`a`, `b` or `c`), the first of `lone_copy` that holds. The copy's register write is the copy's own row's.
pub(super) fn lone_target_handler(lone: &[TargetForm], lone_copy: &[TargetForm], records: &[LaneInstruction], target: usize, after: &dyn Fn(usize) -> *const ()) -> Option<Handler> {
    let target_handler = records.get(target)?.handler;
    if let Some(handler) = target_run_handler(lone, target_handler, after) {
        return Some(handler);
    }
    let copied = match (records.get(target + 1), records.get(target + 2)) {
        (Some(copy), Some(test)) => [test.a, test.b, test.c].contains(&copy.a),
        _ => false,
    };
    if !copied {
        return None;
    }
    target_run_handler(lone_copy, target_handler, after)
}

/// Gives each backward jump whose target run holds (a test, or a variable read or an update then a test,
/// at the jump's target: `FUSED_TARGET_ROWS`, `FUSED_TARGET_PREFIX_ROWS`) the `FUSED_TARGET` record, in
/// one of two places, the target keeping its own row for every other entry:
/// - a jump after one of `BACK_EDGE_PAIRS`' pairs: the pair's record becomes the pair, the jump and the
///   target run in one dispatch (`pair_target_forms`);
/// - a jump with no pair before it: the jump's own record runs the target run (`lone_target_forms`).
/// Runs after `fuse_chains` and before `fuse_rows`, so the target records are still their own rows, and
/// a lone jump is not rewritten where `fuse_rows` would fuse the record before it with the jump.
///
/// Subsumes `lane_inc_dec_loop` and `lane_inc_dec_small_loop` (an update pair into a register or constant
/// test), `lane_inc_dec_load_loop` (an update pair into a loaded bound, then its test), `lane_jump_back_nullish`
/// and `specialize_nullish_back_edges` (a lone back jump into a nullish test), and `lane_jump_back_update_small`
/// (an update then its test, and its copy-then-test form: a lone back jump whose target is an update, a `Move`
/// the test after it reads, then the test; `lone_copy_target_forms`).
pub(crate) fn fuse_target_backedges(records: &mut [LaneInstruction], osr_loop: bool) {
    macro_rules! pair {
        ($pair:literal) => {
            (
                (|handler| plain_row::<{ quench_runtime::staged::FUSED_ROWS[$pair].0 }>(handler)) as fn(*const ()) -> bool,
                pair_target_forms::<{ quench_runtime::staged::FUSED_ROWS[$pair].0 }, { quench_runtime::staged::FUSED_ROWS[$pair].1 }>(),
            )
        };
    }
    let pairs = [pair!(31), pair!(32), pair!(37)];
    let lone = lone_target_forms();
    let lone_copy = lone_copy_target_forms();
    let back = jump_row::<JUMP>(true) as *const ();
    let record = std::mem::size_of::<LaneInstruction>() as isize;
    for jump in 0..records.len() {
        if records[jump].handler != back {
            continue;
        }
        // The jump's target: its distance is in bytes from the jump record, as `take_jump` reads it.
        let Ok(target) = usize::try_from(jump as isize + records[jump].jump() / record) else {
            continue;
        };
        let Some(target_handler) = records.get(target).map(|record_at| record_at.handler) else {
            continue;
        };
        let after = |k: usize| records.get(target + k).map_or(std::ptr::null(), |record_at| record_at.handler);
        let Some(head) = jump.checked_sub(1) else {
            // No record before the jump: a lone back jump.
            if let Some(handler) = lone_target_handler(&lone, &lone_copy, records, target, &after) {
                records[jump].mode = target_test_word(|k| records.get(target + k));
                records[jump].handler = handler as *const ();
            }
            continue;
        };
        let first = records[head].handler;
        if let Some((_, forms)) = pairs.iter().find(|(first_row, _)| first_row(first)) {
            // A move at a hot OSR latch keeps its own record, as `fuse_rows` keeps it.
            let latch = osr_loop && plain_row::<{ quench_runtime::staged::MOVE_ROW }>(first);
            if !latch {
                if let Some(handler) = target_run_handler(forms, target_handler, &after) {
                    let handler = fixed_comparison(handler, &records[target]);
                    let word = target_word(&records[jump]) | target_test_word(|k| records.get(target + k));
                    records[head].handler = handler as *const ();
                    records[head].mode = word;
                }
            }
            continue;
        }
        // The window pair (`fuse_rows`' `window_pairs`) whose first row is a window local's stores before a jump: leave its jump.
        if lane_frame::is_window_row::<{ quench_runtime::staged::FUSED_ROWS[31].0 }>(first) {
            continue;
        }
        if let Some(handler) = lone_target_handler(&lone, &lone_copy, records, target, &after) {
            records[jump].mode = target_test_word(|k| records.get(target + k));
            records[jump].handler = handler as *const ();
        }
    }
}

/// The target test code of a back edge's record `mode` (`lane_frame::TARGET_TEST_SHIFT`): the code of the run's first record (`run(k)`) past its prefix and copy rows where that is a plain comparison jump.
pub(super) fn target_test_word<'a>(run: impl Fn(usize) -> Option<&'a LaneInstruction>) -> u64 {
    use quench_runtime::staged::{COMPARE_JUMP_ROW as TEST, FUSED_TARGET_COPY_ROWS as C, FUSED_TARGET_PREFIX_ROWS as P};
    let passes = |h| plain_row::<{ P[0] }>(h) || plain_row::<{ P[1] }>(h) || plain_row::<{ P[2] }>(h) || plain_row::<{ C[0] }>(h);
    let test = (0..3).map_while(&run).find(|record| !passes(record.handler)).filter(|record| plain_row::<TEST>(record.handler));
    test.map_or(0, |record| u64::from(lane_frame::target_comparison(record)) << lane_frame::TARGET_TEST_SHIFT)
}

/// The handler of the `IncDec` record `update` that the record `jump` follows, where `jump` is an
/// unconditional jump: the generated pair of `IncDec` and `Jump` (`fused_row`, the `staged::FUSED_ROWS`
/// entry of that pair, `FUSED_PLAIN`). `None` for any other pair. A backward jump whose target run holds
/// at `target` (a test, or a variable read or an update then a test) takes the pair's `FUSED_TARGET`
/// record instead (`pair_target_forms`), so the target run runs in the same dispatch. The loader's
/// `fuse_rows` pass and a loop site that retires (`loop_retire_in_lane`) fuse by it, so a declined loop runs
/// what the interpreter's own does. The target record keeps its own row.
///
/// Returns the handler and the record's `mode` word (`target_word`), which the caller writes with it.
pub(super) fn update_fusion(update: &LaneInstruction, jump: &LaneInstruction, target: Option<Ip>) -> Option<(Handler, u64)> {
    update_direction(update)?;
    let backward = match jump.handler {
        plain if plain == jump_row::<JUMP>(true) as *const () => true,
        plain if plain == jump_row::<JUMP>(false) as *const () => false,
        _ => return None,
    };
    if let (true, Some(target)) = (backward, target) {
        let forms = pair_target_forms::<{ quench_runtime::staged::INC_DEC_ROW }, { quench_runtime::staged::JUMP_ROW }>();
        // SAFETY: a backward jump's target is a validated record of the running view.
        let target_handler = unsafe { (*target).handler };
        // A target run's rows fall through to the records after it, which validation makes records of the view
        // (a copy is a value row, so the record after it is one too).
        let after = |k: usize| unsafe { (*(0..k).fold(target, |at, _| following(at))).handler };
        if let Some(handler) = target_run_handler(&forms, target_handler, &after) {
            // SAFETY: as the read of `target_handler`.
            let handler = fixed_comparison(handler, unsafe { &*target });
            // SAFETY: as `after`: the run's rows fall through to records of the view.
            let test = target_test_word(|k| Some(unsafe { &*(0..k).fold(target, |at, _| following(at)) }));
            return Some((handler, target_word(jump) | test));
        }
    }
    let pair = lane_frame::fused_row::<{ quench_runtime::staged::INC_DEC_ROW }, { quench_runtime::staged::JUMP_ROW }, { quench_runtime::staged::FUSED_PLAIN }>(backward);
    Some((pair, target_word(jump)))
}

/// The `mode` word of a `FUSED_ROWS` or `FUSED_TARGET` pair record whose second row is the jump `jump`:
/// its direction (`MODE_BACKWARD` where the jump's distance is not positive, as `fused_rows` reads it).
pub(super) fn target_word(jump: &LaneInstruction) -> u64 {
    if jump.jump() <= 0 { quench_runtime::staged::MODE_BACKWARD } else { 0 }
}

/// Whether `record`, an `IncDec` running its row (`generated_row`), decrements (its immediate flag);
/// `None` for any other record.
pub(super) fn update_direction(record: &LaneInstruction) -> Option<bool> {
    is_row::<{ quench_runtime::staged::INC_DEC_ROW }>(record.handler).then(|| record.imm() != 0)
}

/// An environment-window record's own immediate and the window's register count.
#[inline(always)]
pub(super) fn environment_window_fields(i: &LaneInstruction) -> (usize, u16) {
    let imm = i.imm();
    (usize::from(quench_runtime::staged::window_slot(imm)), quench_runtime::staged::window_base(imm))
}

/// Gives a function whose locals an environment activation or a suspended frame's window could
/// hold (`CallPlan::environment_activation`, `CallPlan::suspended_environment`) the records that read
/// its locals or make its closures a window's: each record's immediate carries the window's register
/// count above its slot (`environment_window_fields`). A plain slot's local record takes its generated
/// row with `FLAG_WINDOW` (`window_records::window_local_handler`), which reads the window's locals in an
/// activation's run; any other local record takes its miss. Returns the window every record names.
pub(super) fn specialize_environment_window(
    function: &Function,
    decoded: &[WideInstruction],
    records: &mut [LaneInstruction],
) -> Option<EnvironmentWindow> {
    if u32::from(function.locals) > u64::BITS
        || (function.arguments_slot.is_some() && !function.reads_passed_arguments_only() && !function.arguments_ride_window())
    {
        return None;
    }
    // A function that resolves names at run time stores and makes closures through the scope
    // its activation carries (`scoped_names`).
    let scoped = function.has_dynamic_scope_access();
    let registers = function.registers;
    let mut replacements = Vec::new();
    for (pc, &instruction) in decoded.iter().enumerate() {
        match window_record(function, instruction) {
            WindowRecord::Keeps => {}
            WindowRecord::Eval => replacements.push((pc, Some(lane_eval::direct_eval_row_handler()), instruction.imm())),
            WindowRecord::Local(own) => {
                if own > ENVIRONMENT_WINDOW_FIELD {
                    return None;
                }
                let handler = window_local_handler(function, instruction.op(), own as usize, scoped);
                replacements.push((pc, handler, own | u32::from(registers) << ENVIRONMENT_WINDOW_SHIFT));
            }
        }
    }
    for (pc, handler, imm) in replacements {
        if let Some(handler) = handler {
            records[pc].handler = handler as *const ();
        }
        records[pc].operand = LaneOperand { imm };
    }
    Some(EnvironmentWindow {
        locals: function.locals,
        captured: (0..usize::from(function.locals))
            .filter(|slot| function.local_slot_uses_environment(*slot))
            .fold(0, |mask, slot| mask | 1 << slot),
        rest: function.rest,
        self_binding: function.self_binding_slot,
        parameter_registers: function.parameters_promoted_in_place(),
    })
}

/// Gives each capture record its handler by the capture's depth, which the bytecode fixes. A
/// record past the function's own scope keeps its row: an outer `LoadCapture` reads through the
/// lane's remembered walks (`Reads::of_captures`), an outer `StoreCapture` takes the general step.
/// A depth-0 record is its row read as
/// the zero depth it is (`row_near`).
pub(super) fn specialize_captures(records: &mut [LaneInstruction]) {
    for pc in 0..records.len() {
        let record = &records[pc];
        let own = ImmediateLayout::capture_depth(record.imm()) == 0;
        let handler: Handler = if !own {
            continue;
        } else if let Some(near) = row_near::<CAPTURE_READ>(record.handler).or_else(|| row_near::<CAPTURE_WRITE>(record.handler)) {
            // Its own scope's capture: the row reads its depth as the zero it is.
            near
        } else {
            continue;
        };
        records[pc].handler = handler as *const ();
    }
}

/// Gives each plain two-operand arithmetic record whose operands are both registers its row at
/// `FLAG_REGISTERS`, which reads them without testing their tags. Runs after the fusion passes, which
/// find the rows by their plain handlers.
pub(super) fn specialize_register_operands(records: &mut [LaneInstruction]) {
    for record in records.iter_mut() {
        if !lane_frame::register_operands(record) {
            continue;
        }
        let row = lane_frame::registers_row::<{ quench_runtime::staged::BINARY_ROW }>(record.handler)
            .or_else(|| lane_frame::registers_row::<{ quench_runtime::staged::NUMERIC_ADD_ROW }>(record.handler))
            .or_else(|| lane_frame::registers_row::<{ quench_runtime::staged::NUMERIC_MULTIPLY_ROW }>(record.handler));
        if let Some(row) = row {
            record.handler = row as *const ();
        }
    }
}

/// Gives each frame `LoadLocal` record the handler of its slot's static binding facts
/// (`LOCAL_LOAD_ROW`): a slot a mapped `arguments` mirrors, or a self binding, keeps the
/// general handler. A `StoreLocal` keeps the row for every slot: its binding is resolved there
/// (the row's frame, else the general step), which also covers a captured environment's slot.
pub(super) fn specialize_frame_locals(function: &Function, records: &mut [LaneInstruction]) {
    for record in records.iter_mut() {
        let slot = record.imm() as usize;
        let load = is_row::<{ quench_runtime::staged::LOAD_ENV_LOCAL_ROW }>(record.handler);
        let store = is_row::<{ quench_runtime::staged::STORE_ENV_LOCAL_ROW }>(record.handler);
        if (!load && !store) || quench_runtime::vm::arguments::mapped_arguments_slot(function, slot).is_some() {
            continue;
        }
        // A slot the frame's captured environment holds is read by its `Names` row; one it does not hold is the frame's own local, which the generated
        // LoadLocal and StoreLocal rows read and write.
        let environment = function.local_slot_uses_environment(slot);
        let handler: Handler = match (load, environment) {
            // A captured slot's load keeps its environment row (`LoadEnvLocal`'s `Names` row).
            (true, true) => continue,
            (true, false) => generated_row!(quench_runtime::staged::LOCAL_LOAD_ROW, ACC_NONE, FLAG_UNREAD),
            _ if function.is_self_binding_slot(slot) => continue,
            // An environment slot's store keeps its row, which misses to the general store.
            (false, true) => continue,
            (false, false) => generated_row!(quench_runtime::staged::LOCAL_STORE_ROW, ACC_NONE, FLAG_UNREAD),
        };
        record.handler = handler as *const ();
    }
}

/// Gives a returning `Binary` record of an arithmetic operator on a register and a constant its
/// operator's `returning_operator_row`, its fields resolved as `specialize_register_operands`
/// resolves a non-returning one's. A record whose right operand is a register, and any other,
/// keeps its `returning_row`: a constant operand replaces the record's operator field (the
/// operand is one union with it), so the row's operator is its static parameter instead.
pub(super) fn specialize_returning_operator(instruction: WideInstruction, constants: &[Value], record: &mut LaneInstruction) {
    use quench_runtime::bytecode::OperandKind::{Constant, Register};
    let (left, right) = (instruction.operand_b(), instruction.operand_c());
    if left.kind() != Some(Register) || instruction.numeric_local_target().is_some() {
        return;
    }
    let constant = match right.kind() {
        Some(Constant) => match constants.get(usize::from(right.payload())) {
            Some(constant) => *constant,
            None => return,
        },
        _ => return,
    };
    let int32 = constant.as_int().is_some();
    macro_rules! pick {
        ($op:expr) => {
            if int32 { returning_operator_row_of!($op, NOT_LEAF, true) } else { returning_operator_row_of!($op, NOT_LEAF, false) }
        };
    }
    let Some(handler) = arithmetic_operators!(instruction.binary_operator(), pick)
        .or_else(|| relation_operators!(instruction.binary_operator(), pick))
    else {
        return;
    };
    record.handler = handler as *const ();
    record.b = left.payload();
    record.operand = LaneOperand { constant };
}

/// Whether some loop of `function`, from a backward jump's target through the jump, has no
/// instruction the lane exits at.
/// Whether one activation of `function` may run an instruction more than once: it has a
/// backward jump, or a branch table, whose entries may jump back.
pub(super) fn may_loop(function: &Function) -> bool {
    function.code.iter().enumerate().any(|(pc, packed)| {
        let instruction = function.widened(*packed);
        quench_runtime::staged::wasm_branch_table(instruction.op())
            || (instruction.op().immediate_role() == ImmediateRole::JumpTarget
                && instruction.imm() as usize <= pc)
    })
}

/// How many lane records control reaches from each PC before one that exits, counted up
/// to `cap`: where `follows_jumps`, a `Jump` continues at its target, as a resumed loop body
/// does, and any other record at the next PC. Each pass extends every count by one step, so
/// `cap` passes reach the fixed point the capped counts allow; without jumps one pass does.
/// An `Await` counts as `cap`: a resumed async frame's lane run that stops there suspends
/// without the general loop (`Vm::stop_awaits_in_lane`), so a short run to it repays its entry.
pub(super) fn runs_ahead_in_lane(decoded: &[WideInstruction], exits: &[bool], cap: usize, follows_jumps: bool) -> Vec<usize> {
    let next = |pc: usize| match decoded[pc].op() {
        _ if follows_jumps && quench_runtime::staged::jump_family(decoded[pc].op()) => decoded[pc].imm() as usize,
        _ => pc + 1,
    };
    let mut runs = vec![0usize; exits.len()];
    for _ in 0..cap {
        let mut changed = false;
        for pc in (0..exits.len()).rev() {
            let run = if quench_runtime::staged::awaits(decoded[pc].op()) {
                cap
            } else if exits[pc] {
                0
            } else {
                (1 + runs.get(next(pc)).copied().unwrap_or(0)).min(cap)
            };
            changed |= run != runs[pc];
            runs[pc] = run;
        }
        if !changed {
            break;
        }
    }
    runs
}

/// Whether the straight-line run from the first instruction, through its unconditional jumps,
/// reaches a `Return` before any record the lane exits at.
pub(super) fn entry_run_returns(decoded: &[WideInstruction], exits: &[bool]) -> bool {
    let mut pc = 0;
    for _ in 0..decoded.len() {
        if exits.get(pc).is_none_or(|exits| *exits) {
            return false;
        }
        match decoded[pc].op() {
            _ if quench_runtime::staged::return_family(decoded[pc].op()) => return true, _ if decoded[pc].returns_from_frame() => return true, // fused with its return
            _ if quench_runtime::staged::jump_family(decoded[pc].op()) => pc = decoded[pc].imm() as usize,
            _ => pc += 1,
        }
    }
    false
}

pub(super) fn closes_loop(decoded: &[WideInstruction], exits: &[bool]) -> bool {
    decoded.iter().enumerate().any(|(pc, &instruction)| {
        let target = instruction.imm() as usize;
        instruction.op().immediate_role() == ImmediateRole::JumpTarget
            && target <= pc
            && !exits[target..=pc].contains(&true)
    })
}

/// The register field of `instruction` that reads `register`, as an
/// accumulator position; `ACC_NONE` when no register field reads it, or more
/// than one does. A register pair's first register is the last resort.
pub(super) fn accumulator_field(instruction: WideInstruction, register: u16) -> u8 {
    let mut fields = InstructionField::ALL.iter().copied().filter(|&field| {
        instruction.op().field_layout(field).reads_register()
            && instruction.field_value(field) == register
    });
    match (fields.next(), fields.next()) {
        (Some(field), None) => field.accumulator(),
        (None, _)
            if instruction.op().immediate_layout() == ImmediateLayout::RegisterPair
                && ImmediateLayout::register_pair(instruction.imm()).0 == register =>
        {
            ACC_PAIR
        }
        _ => ACC_NONE,
    }
}

/// The records of every branch table's entries, whose jump offsets the table reads.
pub(super) fn branch_table_entry_records(instructions: &[WideInstruction]) -> InstructionSet {
    let mut entries = InstructionSet::new(instructions.len());
    for (pc, &instruction) in instructions.iter().enumerate() {
        if quench_runtime::staged::wasm_branch_table(instruction.op()) {
            for entry in quench_runtime::bytecode::branch_table_entries(pc, instruction) {
                entries.insert(entry);
            }
        }
    }
    entries
}

/// The records control enters other than from the record before: jump targets, and the end of
/// a generator's parameter initialization, where its first resumption starts.
pub(super) fn entered_records(instructions: &[WideInstruction], parameter_end_pc: u32) -> InstructionSet {
    let mut targets = InstructionSet::new(instructions.len());
    for instruction in instructions {
        let target = instruction.imm() as usize;
        if instruction.op().immediate_role() == ImmediateRole::JumpTarget
            && target < instructions.len()
        {
            targets.insert(target);
        }
    }
    if parameter_end_pc != 0 {
        targets.insert(parameter_end_pc as usize);
    }
    targets
}

/// Restores the copy `saved` a native's callback took of the lane's context before it ran
/// (`Vm::native_callback_in_lane`). The copy's array storage proofs predate whatever the
/// callback changed, so they retire.
///
/// # Safety
/// `cx` is the running lane's context, which nothing else accesses.
pub(super) unsafe fn restore_context_in_lane(cx: *mut LaneContext, saved: LaneContext) {
    unsafe {
        std::ptr::write(cx, saved);
        (*cx).proofs.retire();
    }
}

/// The activation of a native's callback of `shape`, for a call with `args` (`entry`): the
/// window filled, the activation pushed (its plain header where the shape has one, which
/// returns through the trampoline, else its record) and the context made the callee's.
/// `None`, which changes nothing, where the lane stack lacks room.
#[inline(always)]
pub(super) fn push_prepared_in_lane(
    cx: *mut LaneContext,
    shape: &PreparedShape,
    entry: &PreparedEntry,
    args: &[Value],
) -> Option<Ip> {
    let window = match &shape.header {
        Some(header) => admit_callback_header(cx, header, shape.room)?,
        None => admit_prepared_in_lane(cx, &shape.activation, shape.room)?,
    };
    // SAFETY: the admission pushed the activation over the window; `args` holds
    // `entry.argc` values.
    Some(unsafe { enter_prepared_in_lane(cx, entry, args, window) })
}

/// Whether the lane stack admits one more activation with `room` values: the callback
/// family's admission row (`staged::call_callback::admit_callback`).
#[inline(always)]
pub(super) fn admits_callback(stack: &LaneStack, room: usize) -> bool {
    quench_runtime::staged::call_callback::concrete_admit_callback(
        stack.activation_count() as u64, stack.activation_limit.get() as u64, stack.values.room() as u64, room as u64,
    )
}

/// `push_prepared_in_lane`'s admission of a record: `activation` pushed over a window at the lane
/// stack's top with `room`, whose address it answers for the entry to fill. `None`, which
/// changes nothing, where the lane stack lacks room.
#[inline(always)]
pub(super) fn admit_prepared_in_lane(cx: *mut LaneContext, activation: &Activation, room: usize) -> Option<*mut Value> {
    let stack = &mut unsafe { vm(cx) }.lane_stack;
    if !admits_callback(stack, room) {
        return None;
    }
    // SAFETY: as `push_callback_in_lane`'s.
    let at = stack.values.top();
    // A header with its side record (`Completion::ReturnsToNative` or `ArrayStep`, and the
    // values it keeps), as every script activation of the lane's windows is
    // (one-frame-one-call.md F4 (3)).
    unsafe { stack.push_activation(*activation, at) };
    Some(at)
}

/// `admit_prepared_in_lane` of a callback's plain header (`LaneStack::push_callback_header`).
#[inline(always)]
pub(super) fn admit_callback_header(cx: *mut LaneContext, header: &CallbackHeader, room: usize) -> Option<*mut Value> {
    let stack = &mut unsafe { vm(cx) }.lane_stack;
    if !admits_callback(stack, room) {
        return None;
    }
    // SAFETY: admitted above; the window starts at the values' top.
    let at = stack.values.top();
    unsafe { stack.push_callback_header(header, at) };
    Some(at)
}

/// The activation `entry` enters, whose window starts at `registers`,
/// for a call with `args`: the window filled and the context made the callee's. Its first
/// record.
///
/// # Safety
/// The lane stack's capacity holds the window, and `args` holds `entry.argc` values.
#[inline(always)]
pub(super) unsafe fn enter_prepared_in_lane(
    cx: *mut LaneContext,
    entry: &PreparedEntry,
    args: &[Value],
    registers: *mut Value,
) -> Ip {
    // The callback family's window row (`staged::call_callback::fill_callback`).
    quench_runtime::staged::call_callback::fill_callback(
        &mut quench_runtime::staged::Concrete::pure(),
        registers.expose_provenance() as u64, args.as_ptr().expose_provenance() as u64, entry.window(),
    );
    unsafe { enter_filled_in_lane(cx, entry, registers) }
}

/// `enter_prepared_in_lane` of a call passing `values`, written from where the caller holds
/// them (an array step's element, index and array) rather than copied from memory.
///
/// # Safety
/// As `enter_prepared_in_lane`'s, for `entry.argc == N`.
#[inline(always)]
pub(super) unsafe fn enter_prepared_values_in_lane<const N: usize>(
    cx: *mut LaneContext,
    entry: &PreparedEntry,
    values: [Value; N],
    registers: *mut Value,
) -> Ip {
    unsafe {
        let registers = fill_prepared_values_in_lane::<N>(entry, values, registers);
        enter_filled_in_lane(cx, entry, registers)
    }
}

/// `enter_prepared_values_in_lane` of the activation that returned from the window it holds,
/// at the lane stack's top, entered again for its next call (`step_array_iteration_in_place_in_lane`):
/// its return left the context its own and the stack's length past its window, so only the
/// window is filled again.
///
/// # Safety
/// As `enter_prepared_values_in_lane`'s, for the innermost activation, whose window `entry`
/// entered at `registers` and whose return the context still holds.
#[inline(always)]
pub(super) unsafe fn reenter_prepared_values_in_lane<const N: usize>(
    cx: *mut LaneContext,
    entry: &PreparedEntry,
    values: [Value; N],
    registers: *mut Value,
) -> Ip {
    unsafe {
        let registers = fill_prepared_values_in_lane::<N>(entry, values, registers);
        // A capture walk inside the returned run may have cached the closure's dense slots in the
        // window's capture word (`capture_read_value`): the same environment's, which the entry
        // passes as it passed it before.
        Registers(registers).set_capture(entry.capture);
        debug_assert!(
            (*cx).registers.0 == registers
                && (*cx).base == (*entry.view).records()
                && Registers(registers).this() == entry.this
                && Registers(registers).env() == entry.env
                && Registers(registers).capture() == entry.capture
                && vm(cx).lane_stack.top_frame().is_some_and(|frame| frame.window_ptr() == registers),
            "a returning activation's context is its own"
        );
        let vm = vm(cx);
        vm.programs.record_js_jit_lane_entry(entry.program, entry.function);
        // SAFETY: the program store keeps a derived view alive with its program.
        (*entry.view).start()
    }
}

/// The window of an activation of `entry` at `registers` filled for a call
/// passing `values` (`enter_prepared_values_in_lane`): its registers.
///
/// # Safety
/// As `enter_prepared_values_in_lane`'s.
#[inline(always)]
pub(super) unsafe fn fill_prepared_values_in_lane<const N: usize>(
    entry: &PreparedEntry,
    values: [Value; N],
    registers: *mut Value,
) -> *mut Value {
    // The callback family's window row of held words (`staged::call_callback::fill_callback_words`).
    quench_runtime::staged::call_callback::fill_callback_words(
        &mut quench_runtime::staged::Concrete::pure(),
        registers.expose_provenance() as u64, values.map(Value::to_bits), entry.window(),
    );
    registers
}

/// The activation `entry` enters whose window, at `registers`, is filled: the context made the callee's. Its first record.
///
/// # Safety
/// As `enter_prepared_in_lane`'s; the window's values are written.
#[inline(always)]
pub(super) unsafe fn enter_filled_in_lane(
    cx: *mut LaneContext,
    entry: &PreparedEntry,
    registers: *mut Value,
) -> Ip {
    let vm = unsafe { vm(cx) };
    unsafe { vm.lane_stack.values.set_top(registers.add(entry.size + lane_values::CONTEXT)) };
    install_entry_in_lane(cx, entry, registers);
    vm.programs.record_js_jit_lane_entry(entry.program, entry.function);
    // SAFETY: the program store keeps a derived view alive with its program.
    unsafe { (*entry.view).start() }
}

/// The context made `entry`'s callee's, running over the window at `registers`, wherever it lies.
#[inline(always)]
pub(super) fn install_entry_in_lane(cx: *mut LaneContext, entry: &PreparedEntry, registers: *mut Value) {
    // SAFETY: the program store keeps a derived view alive with its program.
    unsafe {
        (*cx).registers = Registers(registers);
        (*cx).base = (*entry.view).records();
    }
    // The callback family's context row (`staged::call_callback::install_callback`).
    let context = entry.context();
    // SAFETY: the running lane's context; the program store keeps the view alive.
    note_lane_this(unsafe { vm(cx) }, Value::from_bits(context.this), || unsafe { (*entry.view).this_rows });
    quench_runtime::staged::call_callback::install_callback(
        &mut quench_runtime::staged::Concrete::pure(), registers.expose_provenance() as u64, context,
    );
}

/// The activation of a suspended frame (`generator_windows`) enters its window at `registers`,
/// a register file of its own that no lane stack holds, at the record of `pc`, where the frame
/// resumes: the context made the callee's.
///
/// # Safety
/// `registers` holds the callee's registers, which nothing else writes while it runs, and `pc`
/// is a record of its view.
#[inline(always)]
pub(super) unsafe fn enter_suspended_in_lane(
    cx: *mut LaneContext,
    entry: &PreparedEntry,
    registers: *mut Value,
    pc: usize,
) -> Ip {
    install_entry_in_lane(cx, entry, registers);
    // SAFETY: the program store keeps a derived view alive with its program.
    unsafe { (*entry.view).records().add(pc) }
}

/// Counts a leave of a lane body at a place its body had not named before on the field sites of
/// the instruction there (`Vm::note_field_leave`): a place counts once per body.
pub(super) fn note_leave_in_lane(cx: *mut LaneContext, function: u32, (inlined, pc): (Option<u32>, usize)) {
    let function = inlined.unwrap_or(function);
    // SAFETY: the running lane's VM and program.
    unsafe { vm(cx).note_field_leave(&*(*cx).code, function, pc) };
}
