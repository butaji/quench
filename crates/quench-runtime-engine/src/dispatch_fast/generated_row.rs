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
        if LaneFields::<OP, FLAG>::FLAG_ROLE && FLAG == FLAG_UNREAD {
            let flagged: Handler = if i.imm() != 0 { generated_row!(OP, ACC, FLAG_SET) } else { generated_row!(OP, ACC, FLAG_CLEAR) };
            requicken(ip, flagged);
            sibling!(flagged, r, ip, view)
        }
        // Where a planned named read misses: the own read past its record's hint, which reads
        // through the site's cache and rebinds the hint; a held read, the site's holder entry.
        macro_rules! property_miss { () => {
            sibling!(property_miss_row::<OP, ACC, FLAG>, r, ip, view)
        } }
        // A deletion shares the property decoder but reads no named property: its misses and results are
        // the generic ones (the call-out's, below), not a named read's plan.
        let property = const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(quench_runtime::bytecode::Decoder::Property))
            && (OP == crate::dispatch_fast::FIELD_LEAF_READ || LaneFields::<OP, FLAG>::OPCODE.field_store().is_some()) };
        // A named store (`SetField` and its `this` and strict forms): its record's own-field hint
        // as a layout store (`RecordPlan::own_store`); whatever it refuses, the store handlers.
        let store = const { LaneFields::<OP, FLAG>::OPCODE.field_store().is_some() };
        let this_store = const { matches!(LaneFields::<OP, FLAG>::OPCODE.field_store(),
            Some(quench_runtime::bytecode::FieldStore { receiver: quench_runtime::bytecode::StoreReceiver::This, .. })) };
        // A store's miss: the general write's activation path (`GENERAL_SITE`, `POLY_SITE`), or the
        // classifier that names the site the record moves to (`row_set_field_classify`).
        macro_rules! store_miss { () => {{
            let script: *mut LaneContext = view.memory.bytes.cast();
            if const { FLAG == crate::dispatch_fast::ADD_SITE } {
                // A refused add: the generated general write, which the record keeps from now on.
                requicken(ip, generated_row!(OP, ACC_NONE, crate::dispatch_fast::GENERAL_SITE));
            }
            if const { FLAG == crate::dispatch_fast::ADD_SITE || FLAG == crate::dispatch_fast::GENERAL_SITE || FLAG == crate::dispatch_fast::POLY_SITE } {
                if this_store {
                    sibling!(crate::dispatch_fast::row_set_this_field_miss, r, ip, Carried::script_in(script, view))
                }
                sibling!(crate::dispatch_fast::row_set_object_field_miss, r, ip, Carried::script_in(script, view))
            }
            if const { FLAG == FLAG_UNREAD } {
                // A tracked runtime's own hint store is the record's first choice from now on, at the row of
                // the type its slot holds (`tracked_store_row`).
                if unsafe { vm(script) }.heap_reads().tracks_slot_representations() {
                    let hint = i.field_hint();
                    requicken(ip, crate::dispatch_fast::tracked_store_row::<OP>(script, hint.receiver, hint.slot));
                }
            }
            // A held-type store missed (its slot's type widened, or another receiver): the tracked row stores it
            // with the VM's stores, and installs the row of the type the slot holds after (`tracked_store_row`).
            if const { crate::dispatch_fast::store_held_site(FLAG) } {
                sibling!(generated_row!(OP, ACC_NONE, crate::dispatch_fast::TRACKED_SITE), r, ip, Carried::script_in(script, view))
            }
            // A store instance missed: the plain row stores it, with the VM's stores the instance does not
            // carry (`frame.stores`), and re-chooses the instance from the hint it leaves (`requicken_store_form`).
            if const { crate::dispatch_fast::store_formed_site(FLAG) } {
                sibling!(generated_row!(OP, ACC_NONE, FLAG_UNREAD), r, ip, Carried::script_in(script, view))
            }
            if this_store {
                sibling!(crate::dispatch_fast::row_set_field_classify::<OP, true>, r, ip, Carried::script_in(script, view))
            }
            sibling!(crate::dispatch_fast::row_set_field_classify::<OP, false>, r, ip, Carried::script_in(script, view))
        }} }
        if property && !store {
            // A returning site's result carries its return; no other read's carries a flag: a record with
            // other flags holds the general step from its edge on (`derive_lane_view`, `requicken`).
            let flags = if const { crate::dispatch_fast::returning_field_site(FLAG) } { quench_runtime::bytecode::RETURN_REGISTER } else { 0 };
            // The site fact names the base (`own_site`), `this` or a register: stated so the decode folds.
            unsafe {
                if const { !crate::dispatch_fast::sink_site(FLAG) } {
                    std::hint::assert_unchecked(i.a & !flags <= quench_runtime::bytecode::REGISTER_MASK);
                }
                if (const { crate::dispatch_fast::own_site_this(FLAG) } || (const { crate::dispatch_fast::runtime_base(FLAG) } && i.b == quench_runtime::bytecode::FieldBase::THIS.0)) {
                    std::hint::assert_unchecked(i.b == quench_runtime::bytecode::FieldBase::THIS.0);
                } else if const { crate::dispatch_fast::nested_site(FLAG) } {
                    // A nested record's lookup is its site (`set_field_lookup_site`): `b` names the nested base, `c` is zero.
                    std::hint::assert_unchecked(i.b == quench_runtime::bytecode::FieldBase::NESTED && i.c == 0);
                } else if const { !crate::dispatch_fast::nested_site(FLAG) && !crate::dispatch_fast::sink_site(FLAG) && FLAG != crate::dispatch_fast::LOCAL_FIELD_SITE } {
                    // A nested read's base is its site's, not the record's operand; a local's, its slot's.
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
        // The cached, private and general reads answer at their record before the staged plan runs: a
        // direct call of the lane's own read, with no erased host. A miss takes the site's miss.
        // A private read (its brand check throws) and a general read (it may run a getter) take the record's
        // address and the lane's helpers, so they leave the row through `field_read_slow_row`.
        if property && const { crate::dispatch_fast::private_field_site(FLAG) || crate::dispatch_fast::general_site(FLAG) } {
            sibling!(field_read_slow_row::<OP, ACC, FLAG>, r, ip, view)
        }
        if property && const { crate::dispatch_fast::cached_field_site(FLAG) } {
            let script: *mut LaneContext = view.memory.bytes.cast();
            // The base: `this` for a `this` site, else the register the record names.
            let receiver = if const { crate::dispatch_fast::own_site_this(FLAG) } { r.this() } else { r.get(i.b) };
            // A deleted `this` is no object for a cached read; the general read takes it as before.
            if receiver.is_deleted() { property_miss!() }
            // SAFETY: the context's program outlives the lane run.
            let code = unsafe { &*(*script).code };
            let read = LaneFrame::<ACC>::cached_read(script, code, receiver, i.imm(), i.c, 0);
            match read {
                Some(value) => {
                    // The general read's word decodes as the staged row decodes its value (`own_site_decodes`).
                    let value = if const { crate::dispatch_fast::own_site_decodes(FLAG) } { crate::dispatch_fast::own_site_decode(FLAG, value.to_bits()) } else { value };
                    r.set(i.a, value);
                    next!(script, r, following(ip), Carried::script_in(script, view))
                }
                None => property_miss!(),
            }
        }
        // A record whose result carries flags holds the general step from its edge on (`derive_lane_view`,
        // `requicken`): this row's result is a register, stated so the decode's tests fold.
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
            // The element kind the row's site fixes (`requicken_typed`), so no access dispatches on
            // it, and the typed array records the view proof reads.
            let script: *mut LaneContext = view.memory.bytes.cast();
            frame.typed = Some((quench_runtime::staged::typed_elements::native_kind_at(index),
                unsafe { vm(script) }.heap_reads().typed_view_directory()));
        }
        if store {
            let script: *mut LaneContext = view.memory.bytes.cast();
            // The live record table: a direct object receiver proves it reserved, and any other
            // receiver leaves at the row's family test before it is addressed.
            let records = unsafe { vm(script) }.heap_reads().live_object_records() as u64;
            // A method cache that may name the atom must see the store (`store_admitted`); an add
            // leaves that to the VM's adds, which admit it themselves.
            if const { FLAG != crate::dispatch_fast::ADD_SITE } && unsafe { vm(script) }.method_caches_may_name(i.imm()) { store_miss!() }
            // `this` as the VM's own-field hit resolves it (`fixed_this_in_lane`), so a fixed one stores;
            // a plain own-field store row keeps the record's `this` and no call on its path.
            if this_store {
                frame.this = if const { FLAG == FLAG_UNREAD || crate::dispatch_fast::store_formed_site(FLAG) || crate::dispatch_fast::store_held_site(FLAG) } { r.this() }
                    else { unsafe { crate::dispatch_fast::fixed_this_in_lane(cx, r) }.unwrap_or(Value::DELETED) };
            }
            // The VM's stores complete what a row's plan refuses (`PropertyFrame::field_store_host` and
            // its siblings), at every plain store row.
            // A store instance (`store_formed_site`) carries none: what its plan refuses, the plain row
            // completes (`store_miss!`), so the instance makes no call and keeps its values in registers.
            // A held-type store (`store_held_site`) carries none either: its miss is the tracked row's.
            if const { !crate::dispatch_fast::store_formed_site(FLAG) && !crate::dispatch_fast::store_held_site(FLAG) } { frame.stores = Some(FieldStores::of(script)); }
            frame.plan = Some(if const { FLAG == crate::dispatch_fast::ADD_SITE } {
                RecordPlan::own_add()
            } else if const { FLAG == crate::dispatch_fast::GENERAL_SITE } {
                RecordPlan::general_store(records)
            } else if const { FLAG == crate::dispatch_fast::POLY_SITE } {
                let hint = i.field_hint();
                RecordPlan::own_polymorphic(hint.receiver, hint.slot, records)
            } else if const { crate::dispatch_fast::store_held_site(FLAG) } {
                let hint = i.field_hint();
                RecordPlan::own_held(hint.receiver, hint.slot, records, const { crate::dispatch_fast::held_of(FLAG) })
            } else if const { FLAG == crate::dispatch_fast::TRACKED_SITE || FLAG == crate::dispatch_fast::NARROW_SITE } {
                let hint = i.field_hint();
                RecordPlan::own_hinted(hint.receiver, hint.slot, records, const { FLAG == crate::dispatch_fast::NARROW_SITE })
            } else {
                let hint = i.field_hint();
                // A store instance's form is its hint layout's own: the requicken that chose the row proved it
                // (`requicken_store_form`), and a hint a store moves retires the row (`store_miss!`, the success arm).
                if const { crate::dispatch_fast::store_formed_site(FLAG) } {
                    unsafe { std::hint::assert_unchecked(quench_runtime::staged::ic::layout_form(hint.receiver) == const { crate::dispatch_fast::own_site_form(FLAG) }) }
                }
                RecordPlan::own_store(hint.receiver, hint.slot, records)
            });
        } else if property {
            // The record's plan: its own-field hint over the object records the lane bound; an
            // unbound table is the miss's to bind.
            // A JavaScript view carries its context (`script_handler!`); an own-field row is
            // installed only once the lane bound the object records (`row_get_field_own_refresh`).
            let script: *mut LaneContext = view.memory.bytes.cast();
            debug_assert_eq!(script, cx, "a JavaScript view carries its context");
            if const { crate::dispatch_fast::math_site(FLAG) } {
                // The Math call the read's value feeds, computed by the VM's functions (`PropertyFrame::math_call`).
                frame.math = Some(MathCalls::of(script));
            }
            if const { FLAG == crate::dispatch_fast::LOCAL_FIELD_SITE } {
                // A local's read through the own-field hint its record holds (`local_field_handler`): the hint's
                // layout over the bound object records. An unbound table or hint leaves to the local's miss.
                let records = unsafe { (*script).binding.object_base } as u64;
                let hint = i.field_hint();
                if records == 0 || hint.receiver == quench_runtime::vm::NO_FIELD_RECEIVER {
                    sibling!(crate::dispatch_fast::local_field_miss, r, ip, Carried::script_in(script, view))
                }
                frame.plan = Some(RecordPlan::own_word(hint.receiver, hint.slot, records));
            } else if const { crate::dispatch_fast::inherited_site(FLAG) } {
                // A read through the record's inherited hint (`IcOp::Held` over the layout its proof
                // last answered); at the other site, of any receiver's object part.
                let machine = unsafe { vm(script) };
                let others = const { crate::dispatch_fast::inherited_other_site(FLAG) };
                let Some((guard, watch, value)) = machine.inherited_hint_operands(i.call_link.get(), i.imm(), others) else { property_miss!() };
                if (const { crate::dispatch_fast::own_site_this(FLAG) } || (const { crate::dispatch_fast::runtime_base(FLAG) } && i.b == quench_runtime::bytecode::FieldBase::THIS.0)) { frame.this = r.this(); }
                let other = if others { Some(std::ptr::from_ref::<Vm>(machine)) } else { None };
                frame.plan = Some(RecordPlan::held(guard, watch, value, machine.heap_reads().live_object_records() as u64, other));
            } else if const { crate::dispatch_fast::nested_site(FLAG) || crate::dispatch_fast::sink_site(FLAG) } {
                // A nested read's two own hops: the record's hint is the first step's layout and
                // slot, the site's second entry the second, both primary own entries; the base is
                // the site's (`PropertyFrame::nested_base`). A hop the record does not hold misses.
                let machine = unsafe { vm(script) };
                if const { FLAG != crate::dispatch_fast::NESTED_SITE && !crate::dispatch_fast::sink_site(FLAG) } {
                    // A formed instance's hops are its record's (`requicken_nested`): the hint the first,
                    // `mode` the second and the base. Their guards are the check, so nothing is looked up.
                    let hint = i.field_hint();
                    let (layout, slot, base) = crate::dispatch_fast::nested_mode_parts(i.mode);
                    if base == quench_runtime::bytecode::FieldBase::THIS { frame.this = r.this(); }
                    let forms = const { crate::dispatch_fast::nested_hop_forms(FLAG) };
                    frame.plan = Some(RecordPlan::nested_formed((hint.receiver, hint.slot, forms[0]), (layout, slot, forms[1]), base,
                        machine.heap_reads().live_object_records() as u64));
                } else {
                let code = unsafe { &*(*script).code };
                let site = unsafe { code.field_sites.get_unchecked(i.imm() as usize) };
                let Some((_, cache)) = site.second else { property_miss!() };
                let entry = machine.field_cache_entry(cache);
                let hint = i.field_hint();
                if entry.holder != quench_runtime::vm::NO_FIELD_HOLDER || entry.receiver == quench_runtime::vm::NO_FIELD_RECEIVER
                    || hint.receiver == quench_runtime::vm::NO_FIELD_RECEIVER {
                    property_miss!()
                }
                if const { crate::dispatch_fast::sink_site(FLAG) } {
                    // The store's operands are the site's, and the names the reads make decline to the
                    // general path (`Vm::reads_function_info`); the store is to the frame's `this`.
                    if site.sink.is_none() || machine.reads_function_info(site.first.0)
                        || site.second.is_some_and(|(atom, _)| machine.reads_function_info(atom)) {
                        property_miss!()
                    }
                    frame.this = r.this();
                    frame.stores = Some(FieldStores::of(script));
                }
                if site.base == quench_runtime::bytecode::FieldBase::THIS { frame.this = r.this(); }
                let forms = const { crate::dispatch_fast::nested_hop_forms(FLAG) };
                frame.plan = Some(RecordPlan::nested((hint.receiver, hint.slot, forms[0]), (entry.receiver, entry.slot, forms[1]), site.base,
                    machine.heap_reads().live_object_records() as u64));
                }
            } else if const { crate::dispatch_fast::private_field_site(FLAG) } {
                // A private name's field of the record, under the brand its site names (`FieldPlan::Private`).
                if const { crate::dispatch_fast::own_site_this(FLAG) } { frame.this = r.this(); }
                frame.plan = Some(RecordPlan::private(i.imm()));
            } else if const { crate::dispatch_fast::cached_field_site(FLAG) } {
                // The cached read of the site's atom, which the VM answers (`FieldPlan::Host`).
                if const { crate::dispatch_fast::own_site_this(FLAG) } { frame.this = r.this(); }
                frame.plan = Some(RecordPlan::host_read(quench_runtime::staged::property::FieldPlan::Host { atom: i.imm(), site: i.c }));
            } else if const { crate::dispatch_fast::returning_field_site(FLAG) } {
                // A returning own read of the record's hint (`RecordPlan::own`); the lane's unbound
                // object records are the miss's to bind.
                let records = unsafe { (*script).binding.object_base } as u64;
                if records == 0 { property_miss!() }
                if const { crate::dispatch_fast::own_site_this(FLAG) } { frame.this = r.this(); }
                let hint = i.field_hint();
                frame.plan = Some(RecordPlan::own_word(hint.receiver, hint.slot, records));
            } else if const { crate::dispatch_fast::cell_own_site(FLAG) } {
                // A receiver outside the object records (a function) by its layout id; a direct
                // object, should the site meet one, through the bound records.
                let machine = unsafe { vm(script) };
                // The live record table: a direct object receiver proves it reserved.
                let records = machine.heap_reads().live_object_records() as u64;
                if const { crate::dispatch_fast::own_site_this(FLAG) } { frame.this = r.this(); }
                let hint = i.field_hint();
                // A fixed form is the hint layout's own (the decision chose the row by it).
                let form = const { crate::dispatch_fast::own_site_form(FLAG) };
                frame.plan = Some(if form == HINTED_FORM {
                    RecordPlan::own_other(hint.receiver, hint.slot, records, std::ptr::from_ref::<Vm>(machine))
                } else {
                    RecordPlan::own_other_formed(hint.receiver, form & !quench_runtime::staged::TAGGED_SLOTS, hint.slot, records, std::ptr::from_ref::<Vm>(machine))
                });
            } else if const { crate::dispatch_fast::own_constant_site(FLAG) } {
                // One object's own constant: its record's address, then the watch on its values.
                let machine = unsafe { vm(script) };
                let Some((object, watch, value)) = machine.own_constant_word_operands(i.call_link.get()) else { property_miss!() };
                frame.plan = Some(RecordPlan::own_constant(object, watch, value));
            } else if const { crate::dispatch_fast::length_site(FLAG) } {
                // A register's `length`: an array's or a string's, as the site's tier decided.
                let machine = unsafe { vm(script) };
                frame.plan = Some(RecordPlan::length(if const { FLAG == crate::dispatch_fast::ARRAY_LENGTH_SITE } {
                    quench_runtime::staged::ic::IcOp::ArrayLength { records: machine.heap_reads().live_array_records() as u64 }
                } else {
                    quench_runtime::staged::ic::IcOp::StringLength { records: machine.heap_reads().live_string_records() as u64 }
                }));
            } else if const { crate::dispatch_fast::host_read_site(FLAG) } {
                // A read the VM's geometry or length answers from the receiver's word (`FieldPlan::Length`,
                // `FieldPlan::Geometry`): the frame's host steps make the answer, no record is addressed.
                frame.geometry = Some(GeometrySteps::of(script));
                frame.plan = Some(RecordPlan::host_read(if const { FLAG == crate::dispatch_fast::TYPED_LENGTH_SITE } {
                    quench_runtime::staged::property::FieldPlan::Length
                } else {
                    quench_runtime::staged::property::FieldPlan::Geometry { atom: i.imm() }
                }));
            } else if const { crate::dispatch_fast::getter_site(FLAG) } {
                // A read that met a getter whose hint returns an own field of the receiver
                // (`IcOp::GetterOwn`): the hint's operands, where the hint is for the read's name;
                // else the getter's refresh, which makes the hint.
                let machine = unsafe { vm(script) };
                let Some((guard, watch, slot)) = machine.getter_hint_operands(i.call_link.get(), i.imm()) else { property_miss!() };
                if const { crate::dispatch_fast::own_site_this(FLAG) } { frame.this = r.this(); }
                frame.plan = Some(RecordPlan::getter(guard, watch, slot, machine.heap_reads().live_object_records() as u64));
            } else if const { FLAG == crate::dispatch_fast::STRING_SITE } {
                // A string receiver's read through the record's `String.prototype` hint.
                let machine = unsafe { vm(script) };
                let Some((watch, value)) = machine.string_hint_operands(i.call_link.get(), i.imm()) else { property_miss!() };
                frame.plan = Some(RecordPlan::held_string(watch, value));
            } else if const { FLAG == crate::dispatch_fast::NUMBER_SITE } {
                // A number receiver's read through the record's `Number.prototype` hint.
                let machine = unsafe { vm(script) };
                let Some((watch, value)) = machine.number_hint_operands(i.call_link.get(), i.imm()) else { property_miss!() };
                frame.plan = Some(RecordPlan::held_number(watch, value));
            } else if const { crate::dispatch_fast::entry_site(FLAG) } {
                // A read the VM's entry lookup answers (`EntryReads`): the polymorphic read's own hint first
                // (`RecordPlan::polymorphic`), the other entries alone.
                let machine = unsafe { vm(script) };
                if (const { crate::dispatch_fast::own_site_this(FLAG) } || (const { crate::dispatch_fast::runtime_base(FLAG) } && i.b == quench_runtime::bytecode::FieldBase::THIS.0)) { frame.this = r.this(); }
                let records = machine.heap_reads().live_object_records() as u64;
                frame.entries = Some(EntryReads::of(script, ip));
                frame.plan = Some(if const { FLAG == crate::dispatch_fast::POLYMORPHIC_SITE } {
                    let hint = i.field_hint();
                    RecordPlan::polymorphic(hint.receiver, hint.slot, records)
                } else {
                    RecordPlan::entry(const { crate::dispatch_fast::entry_kind(FLAG) }, records)
                });
            } else if const { crate::dispatch_fast::held_site(FLAG) } {
                // A read through the record's prototype hint (`IcOp::Held`): the hint's operands,
                // where the lane bound its records and the hint is the read's; else the hint's own
                // read of any receiver, which refreshes it.
                let machine = unsafe { vm(script) };
                let Some((guard, watch, value)) = machine.holder_hint_operands(i.call_link.get(), i.imm()) else { property_miss!() };
                if (const { crate::dispatch_fast::own_site_this(FLAG) } || (const { crate::dispatch_fast::runtime_base(FLAG) } && i.b == quench_runtime::bytecode::FieldBase::THIS.0)) { frame.this = r.this(); }
                // A direct object's record is addressed in the table after its family test; at the
                // held-other site, any other receiver's object part is resolved out of line.
                let other = if const { crate::dispatch_fast::held_other_site(FLAG) } { Some(std::ptr::from_ref::<Vm>(machine)) } else { None };
                frame.plan = Some(RecordPlan::held(guard, watch, value, machine.heap_reads().live_object_records() as u64, other));
            } else {
            let records = unsafe { (*script).binding.object_base } as u64;
            if const { crate::dispatch_fast::own_site_form(FLAG) == HINTED_FORM } {
                // The form of no particular hint is the row a site starts on, before the lane bound
                // its object records: its first miss binds them.
                if records == 0 {
                    if (const { crate::dispatch_fast::own_site_this(FLAG) } || (const { crate::dispatch_fast::runtime_base(FLAG) } && i.b == quench_runtime::bytecode::FieldBase::THIS.0)) {
                        sibling!(row_get_field_own_refresh::<true, false>, r, ip, Carried::script_in(script, view))
                    }
                    sibling!(row_get_field_own_refresh::<false, false>, r, ip, Carried::script_in(script, view))
                }
            }
            debug_assert_ne!(records, 0, "an own-field row runs over bound object records");
            if (const { crate::dispatch_fast::own_site_this(FLAG) } || (const { crate::dispatch_fast::runtime_base(FLAG) } && i.b == quench_runtime::bytecode::FieldBase::THIS.0)) { frame.this = r.this(); }
            let hint = i.field_hint();
            let form = const { crate::dispatch_fast::own_site_form(FLAG) };
            let fixed = (form != HINTED_FORM).then_some(form & !quench_runtime::staged::TAGGED_SLOTS);
            // A fixed form is the hint's layout's own (`own_formed_handler` chose the row by it),
            // stated so the read takes it as a constant, not from the layout table.
            if let Some(form) = fixed { unsafe { std::hint::assert_unchecked(quench_runtime::staged::ic::layout_form(hint.receiver) == form) } }
            frame.plan = Some(RecordPlan::own_word(hint.receiver, hint.slot, records));
            }
        }
        match row_step!(LaneFields::<OP, FLAG>::OPCODE, &mut stage, &mut frame, operation) {
            Step::Value(_) if store => {
                let script: *mut LaneContext = view.memory.bytes.cast();
                // The form the VM's own-field store left for the record (`field_store_host`).
                if let Some(hint) = frame.stored {
                    set_field_hint(ip, hint);
                    // A tracked runtime's own store completes through the VM's own-field hit each time from the
                    // plain row (its layout store observes no representation): the record takes the tracked
                    // site's hinted store, as a refused one does (`store_miss!`), and stops asking the site.
                    if const { FLAG == FLAG_UNREAD } && unsafe { vm(script) }.heap_reads().tracks_slot_representations() {
                        requicken(ip, crate::dispatch_fast::tracked_store_row::<OP>(script, hint.receiver, hint.slot));
                    }
                    // A store the VM made into a narrow run moves the site to the narrow row (`NARROW_SITE`);
                    // a narrow site whose object is no longer narrow goes back to the own store.
                    if const { FLAG == FLAG_UNREAD || FLAG == crate::dispatch_fast::NARROW_SITE } && !unsafe { vm(script) }.heap_reads().tracks_slot_representations() {
                        let narrow = quench_runtime::tier_surface::layouts::auxiliary(hint.receiver) & quench_runtime::value_vec::STORAGE_FORM == quench_runtime::value_vec::NARROW_FORM;
                        if const { FLAG == FLAG_UNREAD } {
                            if narrow { requicken(ip, generated_row!(OP, ACC_NONE, crate::dispatch_fast::NARROW_SITE)); }
                        } else if !narrow {
                            requicken(ip, generated_row!(OP, ACC_NONE, FLAG_UNREAD));
                        }
                    }
                }
                // A plain store's record takes the instance of its hint's storage form (`requicken_store_form`, which
                // leaves a tracked or narrow record to its own site). An instance whose hint a VM store moved retires.
                if const { FLAG == FLAG_UNREAD } {
                    let layout = frame.stored.map_or_else(|| i.field_hint().receiver, |hint| hint.receiver);
                    crate::dispatch_fast::requicken_store_form::<OP>(script, ip, layout);
                }
                if const { crate::dispatch_fast::store_formed_site(FLAG) } && frame.stored.is_some() {
                    requicken(ip, generated_row!(OP, ACC_NONE, FLAG_UNREAD));
                }
                // The tracked row's store leaves the slot's type joined: the record takes the row of that type
                // (`tracked_store_row`), which stores the next value of it in place.
                if const { FLAG == crate::dispatch_fast::TRACKED_SITE } {
                    let hint = frame.stored.unwrap_or_else(|| i.field_hint());
                    requicken(ip, crate::dispatch_fast::tracked_store_row::<OP>(script, hint.receiver, hint.slot));
                }
                next!(script, r, following(ip), Carried::script_in(script, view))
            }
            Step::Unresolved if store => store_miss!(),
            Step::Value(value) if property && const { crate::dispatch_fast::returning_field_site(FLAG) } => {
                // A returning read's value completes its activation as a returning record does
                // (`lane_return_value!`), so it writes no register.
                let value = if const { crate::dispatch_fast::own_site_decodes(FLAG) } { crate::dispatch_fast::own_site_decode(FLAG, value) } else { Value::from_bits(value) };
                let script: *mut LaneContext = view.memory.bytes.cast();
                lane_return_value!(script, ip, value, crate::dispatch_fast::returning_leaf(FLAG))
            }
            Step::Value(value) if property => {
                // The read's value, a slot word decoded where the form may hold a raw double.
                let value = if const { crate::dispatch_fast::own_site_decodes(FLAG) } { crate::dispatch_fast::own_site_decode(FLAG, value) } else { Value::from_bits(value) };
                let script: *mut LaneContext = view.memory.bytes.cast();
                // A sinking read stores its value to `this` before its register is written: a refused
                // store repeats the read from the registers as they were.
                if const { crate::dispatch_fast::sink_site(FLAG) } {
                    let Some((atom, site)) = crate::dispatch_fast::sink_operands(script, i) else {
                        sibling!(crate::dispatch_fast::row_get_field_sink, r, ip, Carried::script_in(script, view))
                    };
                    if quench_runtime::staged::property::PropertyFrame::sink_store(&mut frame, &mut stage, atom, site, value.to_bits()).is_none() {
                        sibling!(crate::dispatch_fast::row_get_field_sink, r, ip, Carried::script_in(script, view))
                    }
                    r.set(i.a & quench_runtime::bytecode::REGISTER_MASK, value);
                    next!(script, r, following(ip), Carried::script_in(script, view))
                }
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
// merge: the store-then-move pair (`element_move_site`) has no site constant in this design; its branch is dropped.
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
                // The general read's word decodes as the staged row decodes its value (`own_site_decodes`).
                let value = if const { crate::dispatch_fast::own_site_decodes(FLAG) } { crate::dispatch_fast::own_site_decode(FLAG, value.to_bits()) } else { value };
                r.set(i.a, value);
                next!(script, r, following(ip), Carried::script_in(script, view))
            }
            None => sibling!(property_miss_row::<OP, ACC, FLAG>, r, ip, view),
        }
    }
}
