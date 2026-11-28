#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;
#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt};
// The body of `generated_row`, included once per shard module (`lane_frame::generated_rows_*`,
// `generated_row_handler`): each module is its own codegen unit.
view_handler! {
    /// The lane handler of every opcode `admits_row` admits, generated from its staged row
    /// rather than written: the record decodes by the row family's one decoder
    /// (`Operation::decode_flow`), the row steps on `Concrete` over the lane's frame, and the
    /// step picks the continuation. `OP` and `ACC` are static, so rustc folds the decode and the
    /// row to the residual a hand-written handler would be (Futamura's first projection).
    pub(crate) fn generated_row<OP: u16, ACC: u8, FLAG: u8>(cx, r, ip, i, view) {
        let cx: *mut LaneContext = row_context::<OP>(cx);
        // An accumulator field the opcode reads no register at: the view never selects this
        // instance, which every table and handler set still names. It continues as the row that
        // reads none, so its code is this one jump rather than another copy of the row.
        if const { ACC != ACC_NONE && !accumulator_readable(LaneFields::<OP, FLAG>::OPCODE, ACC) } {
            sibling!(generated_row!(OP, ACC_NONE, FLAG), r, ip, view)
        }
        // A closure read (`CLOSURE_CELL_SITE`) reads its record's `mode` word alone, before the field row's base
        // asserts, which a closure read's base does not satisfy.
        if const { FLAG == crate::dispatch_fast::CLOSURE_CELL_SITE } {
            sibling!(closure_cell_read::<OP, ACC, FLAG>, r, ip, view)
        }
        // Where a planned named read misses: the own read past its record's hint, which reads
        // through the site's cache and rebinds the hint.
        macro_rules! property_miss { () => {
            sibling!(property_miss_row::<OP, ACC, FLAG>, r, ip, view)
        } }
        // A deletion shares the property decoder but reads no named property: its misses and results are
        // the generic ones (the call-out's, below), not a named read's plan.
        let property = const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(quench_runtime::bytecode::Decoder::Property))
            && (OP == crate::dispatch_fast::FIELD_LEAF_READ || LaneFields::<OP, FLAG>::OPCODE.field_store().is_some()) };
        // A named store (`SetField` and its `this` and strict forms): the general store handlers (`store_miss!`).
        let store = const { LaneFields::<OP, FLAG>::OPCODE.field_store().is_some() };
        let this_store = const { matches!(LaneFields::<OP, FLAG>::OPCODE.field_store(),
            Some(quench_runtime::bytecode::FieldStore { receiver: quench_runtime::bytecode::StoreReceiver::This, .. })) };
        // A store's miss: the general write's activation path. The row was chosen at load, so a miss never
        // rewrites the record; it takes the general step in place.
        macro_rules! store_miss { () => {{
            let script: *mut LaneContext = view.memory.bytes.cast();
            if this_store {
                sibling!(crate::dispatch_fast::row_set_this_field_miss, r, ip, Carried::script_in(script, view))
            }
            sibling!(crate::dispatch_fast::row_set_object_field_miss, r, ip, Carried::script_in(script, view))
        }} }
        if property && !store {
            // A returning site's result carries its return; no other read's carries a flag: a record with
            // other flags holds the general step from its edge on (`derive_lane_view`, `edge_rewrite`).
            let flags = if const { crate::dispatch_fast::returning_field_site(FLAG) } { quench_runtime::bytecode::RETURN_REGISTER } else { 0 };
            // The site fact names the base (`own_site`), `this` or a register: stated so the decode folds.
            unsafe {
                if const { !crate::dispatch_fast::sink_site(FLAG) } {
                    std::hint::assert_unchecked(i.a & !flags <= quench_runtime::bytecode::REGISTER_MASK);
                }
                if const { crate::dispatch_fast::own_site_this(FLAG) } {
                    std::hint::assert_unchecked(i.b == quench_runtime::bytecode::FieldBase::THIS.0);
                } else if const { crate::dispatch_fast::nested_trusted_site(FLAG) } {
                    // A trusted nested read's record names the nested base; its site names the first hop's base.
                    std::hint::assert_unchecked(i.b == quench_runtime::bytecode::FieldBase::NESTED);
                } else if const { !crate::dispatch_fast::sink_site(FLAG) && !crate::dispatch_fast::trusted_local_site(FLAG) } {
                    // A read's base is its site's, not the record's operand; a local's, its slot's. A trusted local read's
                    // base is its slot's flagged local base (`FieldBase::local`), which no register bound holds.
                    std::hint::assert_unchecked(i.b <= quench_runtime::bytecode::REGISTER_MASK);
                }
            }
        }
        if const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(quench_runtime::bytecode::Decoder::Element)) } {
            // A keyed read's row is installed for a register base and key without result flags
            // (`element_read_row`), a keyed write's operands are three registers, as the
            // handlers these rows replaced read them: stated so the decode's operand tests fold.
            unsafe {
                std::hint::assert_unchecked(i.a <= quench_runtime::bytecode::REGISTER_MASK);
                std::hint::assert_unchecked(i.b <= quench_runtime::bytecode::REGISTER_MASK);
                std::hint::assert_unchecked(i.c <= quench_runtime::bytecode::REGISTER_MASK);
            }
        }
        // The private and general reads answer at their record before the staged plan runs: a
        // private read (its brand check throws) and a general read (it may run a getter) take the record's
        // address and the lane's helpers, so they leave the row through `field_read_slow_row`.
        if property && const { crate::dispatch_fast::private_field_site(FLAG) || crate::dispatch_fast::general_site(FLAG) } {
            sibling!(field_read_slow_row::<OP, ACC, FLAG>, r, ip, view)
        }
        // A record whose result carries flags holds the general step from its edge on (`derive_lane_view`,
        // `edge_rewrite`): this row's result is a register, stated so the decode's tests fold.
        if const { LaneFields::<OP, FLAG>::OPCODE.result_layout().allowed_flags() != 0 && !crate::dispatch_fast::sink_site(FLAG)
            && !local_result_site(OP, FLAG)
            && !(OP == crate::dispatch_fast::FIELD_LEAF_READ && crate::dispatch_fast::returning_field_site(FLAG)) } {
            unsafe { std::hint::assert_unchecked(i.a <= quench_runtime::bytecode::REGISTER_MASK) }
        }
        // The row family's decoder, a constant of the instantiation, so only its arm is compiled.
        let fields = LaneFields::<OP, FLAG>(i);
        if const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(quench_runtime::bytecode::Decoder::Element)) } {
            // A keyed read's row is installed for a register base and key without result flags
            // (`element_read_row`): stated so the decode's operand tests fold.
            unsafe {
                std::hint::assert_unchecked(i.a <= quench_runtime::bytecode::REGISTER_MASK);
                std::hint::assert_unchecked(i.b <= quench_runtime::bytecode::REGISTER_MASK);
                std::hint::assert_unchecked(i.c <= quench_runtime::bytecode::REGISTER_MASK);
            }
        }
        // `if const`, not `match const`: rustc compiles only a taken `if const` branch, so each
        // instantiation holds its one decoder (a `match` on a constant compiles every arm).
        // A superinstruction's pair decodes from its parts in the program's table (`super_parts_in_lane`).
        let decoded = (if const { quench_runtime::staged::super_const_array_family(LaneFields::<OP, FLAG>::OPCODE) } {
            Operation::decode_literal_with(quench_runtime::staged::Fields::wide(fields), unsafe { crate::dispatch_fast::super_parts_in_lane(cx, i.imm() as usize) }, ())
        } else {
            row_decode!(OP, FLAG, fields)
        })
        .map(|operation| if const { FLAG == FLAG_ROOT } { operation.rooted() } else { operation });
        let Some(operation) = decoded else { sibling!(general_step_in_lane, r, ip, view) };
        // The row's stage reads the heap's tables (the property arena, the class space) and the store gate.
        let mut stage = quench_runtime::staged::concrete_over_vm(unsafe { vm(cx) });
        let mut frame = row_frame::<OP, ACC, FLAG>(cx, r, ip, i, view);
        frame.trust_this = TRUSTS_THIS;
        // A numeric local target: the frame's slot the value moves to, and the register of that number the
        // row writes, put back once it has (`FLAG_LOCAL_RESULT`). A record outside a frame run takes the
        // general step.
        let local = if const { local_result_site(OP, FLAG) } {
            let register = i.a & quench_runtime::bytecode::REGISTER_MASK;
            let Some(slot) = (unsafe { crate::dispatch_fast::numeric_local_slot(cx, register) }) else {
                sibling!(general_step_in_lane, r, ip, view)
            };
            Some((register, slot, r.get(register)))
        } else { None };
        if let Some(index) = const { crate::dispatch_fast::typed_site_kind(FLAG) } {
            // The element kind the row's site fixes (`element_typed_row`, chosen at load), so no access dispatches on
            // it, and the typed array records the view proof reads.
            let script: *mut LaneContext = view.memory.bytes.cast();
            frame.typed = Some((quench_runtime::staged::typed_elements::native_kind_at(index),
                unsafe { vm(script) }.heap_reads().typed_view_directory()));
        }
        if store {
            // A named store reads no record: every ordinary object is a class cell, so the store takes the general
            // write at its miss (`store_miss!`), chosen at load.
            store_miss!()
        } else if property {
            // A JavaScript view carries its context (`script_handler!`).
            let script: *mut LaneContext = view.memory.bytes.cast();
            debug_assert_eq!(script, cx, "a JavaScript view carries its context");
            if const { crate::dispatch_fast::math_site(FLAG) } {
                // The Math call the read's value feeds, computed by the VM's functions (`PropertyFrame::math_call`).
                frame.math = Some(MathCalls::of(script));
            }
            if const { crate::dispatch_fast::nested_trusted_site(FLAG) } {
                // A nested read of two proven fixed cells (`trusted_read`): the first hop's word is the record's operand and
                // the second's its `mode`. The plan's base is the site's, which `this` names through the frame.
                let code = unsafe { &*(*script).code };
                let site = unsafe { code.field_sites.get_unchecked(i.imm() as usize) };
                if site.base == quench_runtime::bytecode::FieldBase::THIS { frame.this = r.this(); }
                frame.plan = Some(RecordPlan::nested_trusted(site.base, unsafe { i.operand.field.shape }, i.mode as u32));
            } else if const { crate::dispatch_fast::sink_site(FLAG) } {
                // A nested read that stores its value to `this` reads through its general path (`row_get_field_sink`),
                // as the read's record names no object record.
                property_miss!()
            } else if const { crate::dispatch_fast::private_field_site(FLAG) } {
                // A private name's field of the record, under the brand its site names (`FieldPlan::Private`).
                if const { crate::dispatch_fast::own_site_this(FLAG) } { frame.this = r.this(); }
                frame.plan = Some(RecordPlan::private(i.imm()));
            } else if const { crate::dispatch_fast::trusted_site(FLAG) || crate::dispatch_fast::returning_field_site(FLAG) } {
                // A fixed class's own field at the slot word the load chose (`trusted_read`): the cell is read with no class
                // or layout guard. A returning read's value completes its return (`returning_field_site`). A field the class may
                // leave absent leaves at the miss (`property_miss_row`).
                if const { crate::dispatch_fast::own_site_this(FLAG) } { frame.this = r.this(); }
                let word = unsafe { i.operand.field.shape };
                frame.plan = Some(RecordPlan::host_read(quench_runtime::staged::property::FieldPlan::Fixed {
                    slot: word & !crate::dispatch_fast::TRUSTED_HOLE,
                    hole: word & crate::dispatch_fast::TRUSTED_HOLE != 0,
                }));
            } else if const { crate::dispatch_fast::length_site(FLAG) } {
                // A register's `length`: an array's or a string's, as the site's tier decided.
                let machine = unsafe { vm(script) };
                frame.plan = Some(RecordPlan::length(if const { FLAG == crate::dispatch_fast::ARRAY_LENGTH_SITE } {
                    quench_runtime::staged::ic::IcOp::ArrayLength { records: machine.heap_reads().live_array_records() as u64 }
                } else {
                    quench_runtime::staged::ic::IcOp::StringLength { records: machine.heap_reads().live_string_records() as u64 }
                }));
            } else if const { crate::dispatch_fast::host_read_site(FLAG) } {
                // A typed view's `length`, which the VM's geometry answers from the receiver's word (`FieldPlan::Length`):
                // the frame's host steps make the answer, no record is addressed.
                frame.geometry = Some(GeometrySteps::of(script));
                frame.plan = Some(RecordPlan::host_read(quench_runtime::staged::property::FieldPlan::Length));
            } else {
                // An own-field read that no class-keyed or load-time plan proves reads through its general path.
                property_miss!()
            }
        }
        match row_step!(LaneFields::<OP, FLAG>::OPCODE, &mut stage, &mut frame, operation) {
            Step::Value(value) if property && const { crate::dispatch_fast::returning_field_site(FLAG) } => {
                // A returning read's value completes its activation as a returning record does
                // (`lane_return_value!`), so it writes no register.
                let value = Value::from_bits(value);
                let script: *mut LaneContext = view.memory.bytes.cast();
                lane_return_value!(script, ip, value, crate::dispatch_fast::returning_leaf(FLAG))
            }
            Step::Value(value) if property => {
                let value = Value::from_bits(value);
                let script: *mut LaneContext = view.memory.bytes.cast();
                r.set(i.a, value);
                if const { crate::dispatch_fast::math_site(FLAG) } {
                    // The Math call the value feeds: its copies and the call are written here, or the
                    // records after the read resume the call's own records.
                    let resume = crate::dispatch_fast::native_calls::math_row_resume::<_, FLAG>(&mut frame, &mut stage, r, ip, value.to_bits());
                    next!(script, r, resume, Carried::script_in(script, view))
                }
                next!(script, r, following(ip), Carried::script_in(script, view))
            }
            Step::Unresolved if property => property_miss!(),
            Step::Value(value) if const { local_result_site(OP, FLAG) } => {
                let Some((register, slot, kept)) = local else { unreachable!() };
                // SAFETY: `numeric_local_slot` gave a live local slot of the running frame.
                unsafe { *slot = Value::from_bits(value) };
                r.set(register, kept);
                // The result is no register's, so no record reads it as the accumulator.
                next!(cx, r, following(ip), view)
            }
            Step::Value(value) => {
                // A row that answers its value without writing its register (`row_writes_result`):
                // the register is written here, as every producer writes it beside the carried value.
                if const { !quench_runtime::staged::row_writes_result(LaneFields::<OP, FLAG>::OPCODE) } {
                    r.set(i.a, Value::from_bits(value));
                }
                if const { row_produces(LaneFields::<OP, FLAG>::OPCODE) } {
                    next!(cx, r, following(ip), Carried { acc: Slot::of(Value::from_bits(value)), ..view })
                }
                // No record reads a non-producer's accumulator (`derive_lane_view`).
                next!(cx, r, following(ip), view)
            }
            // A row that left the lane run ends the run.
            Step::Control(LaneControl::Edge(Edge::Leave)) => return exit(ip),
            // A keyed read past its in-place arm: the record's keyed paths (a slot, a getter).
            Step::Unresolved if const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(quench_runtime::bytecode::Decoder::Element))
                && matches!(LaneFields::<OP, FLAG>::OPCODE.field_layout(quench_runtime::bytecode::InstructionField::A), quench_runtime::bytecode::FieldLayout::ResultRegister) } => {
                let script: *mut LaneContext = view.memory.bytes.cast();
                sibling!(crate::dispatch_fast::row_index_read_miss, r, ip, Carried::script_in(script, view))
            }
            // A keyed write past its in-place replacement: an append, a hole, a typed array, a key.
            Step::Unresolved if const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(quench_runtime::bytecode::Decoder::Element)) } => {
                let script: *mut LaneContext = view.memory.bytes.cast();
                sibling!(crate::dispatch_fast::set_index_slot_miss, r, ip, Carried::script_in(script, view))
            }
            // A transfer (a Wasm return) resumes the register file the frame switched to.
            Step::Control(LaneControl::Resumed(resumed)) => resume!(cx, resumed),
            Step::Control(_) | Step::Unresolved => {
                if let Some((register, _, kept)) = local { r.set(register, kept); }
                sibling!(const { row_miss::<FLAG>(LaneFields::<OP, FLAG>::OPCODE) }, r, ip, view)
            }
        }
    }
}

view_handler! {
    /// The closure read of a field row (`CLOSURE_CELL_SITE`, chosen by `named_read_row`): the read's value is the one
    /// closure object the record's `mode` word addresses (the closure cell `derive_lane_view` wrote there). That cell is full
    /// at every read: the closure's creation edge writes it before any code can reach the closure
    /// (`ProgramStore::write_closure_cell`), and the read's proven identity (`typed_ir::Identity::Closure`) makes the read's
    /// value that closure. So the read takes no receiver, walks no prototype and cannot miss.
    #[inline(never)]
    pub(crate) fn closure_cell_read<OP: u16, ACC: u8, FLAG: u8>(cx, r, ip, i, view) {
        let script: *mut LaneContext = view.memory.bytes.cast();
        // SAFETY: the word is the closure cell of the running program, which keeps its cells for its life; the cell is
        // full (see above).
        let value = unsafe { *(*(i.mode as *const std::cell::OnceCell<Value>)).get().unwrap_unchecked() };
        r.set(i.a, value);
        next!(script, r, following(ip), Carried::script_in(script, view))
    }
}

view_handler! {
    /// The private and general reads of a field row, out of line: a private read's brand check throws,
    /// and a general read may run a getter, so neither runs in the row (`generated_row`). Its record's
    /// address is the one the row named; its miss is the row's property miss.
    #[cold]
    #[inline(never)]
    pub(crate) fn field_read_slow_row<OP: u16, ACC: u8, FLAG: u8>(cx, r, ip, i, view) {
        let script: *mut LaneContext = view.memory.bytes.cast();
        // The base: `this` for a `this` site, else the register the record names.
        let receiver = if const { crate::dispatch_fast::own_site_this(FLAG) } { r.this() } else { r.get(i.b) };
        // A deleted `this` is no object for a private read; the general read takes it as before.
        if const { !crate::dispatch_fast::general_site(FLAG) } && receiver.is_deleted() {
            sibling!(property_miss_row::<OP, ACC, FLAG>, r, ip, view)
        }
        // SAFETY: the context's program outlives the lane run.
        let code = unsafe { &*(*script).code };
        let read = if const { crate::dispatch_fast::private_field_site(FLAG) } {
            LaneFrame::<ACC>::private_value(script, ip.addr(), code, receiver, i.imm())
        } else {
            crate::dispatch_fast::general_field_read(script.cast(), ip.addr(), const { !crate::dispatch_fast::own_site_this(FLAG) }, receiver, i.imm(), i.c)
        };
        match read {
            Some(value) => {
                r.set(i.a, value);
                next!(script, r, following(ip), Carried::script_in(script, view))
            }
            None => sibling!(property_miss_row::<OP, ACC, FLAG>, r, ip, view),
        }
    }
}
