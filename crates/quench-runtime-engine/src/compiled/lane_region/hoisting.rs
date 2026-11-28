use super::*;

impl Lowering<'_, '_> {
    /// Prove the views of the arrays the loop at `header` indexes without redefining them, on
    /// its entering edge; a base that is no such array leaves at the header.
    pub(super) fn hoist_arrays(&mut self, state: &mut State, header: usize) {
        let arrays: Vec<LoopArray> = self.loop_arrays.iter().copied().filter(|array| array.header == header).collect();
        if arrays.is_empty() { return; }
        let exit = self.guard_exit(state, header as u32);
        for array in arrays {
            let bits = self.tagged(state, array.source);
            let variables = self.array_variables(array);
            self.builder.def_var(variables.bits, bits);
            match array.views {
                ElementViews::Dense => {
                    let view = super::emit_dense_array_view(&mut self.builder, self.payload, bits, array.writable, self.array_access(), exit);
                    if self.int32_loop(array) {
                        // An int32 loop keeps the backing's exact count of others, which is zero: a backing the solve
                        // proves plain (`proven_plain_backing`) has that count by its fact; any other is guarded here
                        // before any mark, and kept by its int32 stores (`int32_loop`), so the view marks nothing.
                        if !self.proven_plain_backing(header, array.source) {
                            let others = {
                                let mut stage = super::object_slots::emit(&mut self.builder);
                                stage.load_u32(quench_runtime::staged::region::Region::Elements.mutable(), view.values, quench_runtime::tier_surface::OTHERS_FROM_VALUES)
                            };
                            let none = self.raw_compare_imm(Relation::Equal, others, 0);
                            emit_js_guard(&mut self.builder, none, exit);
                        }
                    } else {
                        // Stores through the hoisted view then write the element alone.
                        if array.writable { super::emit_dense_array_uncounted(&mut self.builder, view); }
                        // A read-only array no store or append of its loop changes, read only as int32s, is
                        // proved once here: a backing with others (a non-int32 or a hole) leaves at the entry, so
                        // its reads need no tag test (`proved_int_element_read`).
                        if self.int_only_array(array) {
                            let mut stage = super::object_slots::emit(&mut self.builder);
                            quench_runtime::staged::elements::int_only_backing(&mut stage, view.values, exit)
                                .expect("the emit stage continues past its guards");
                            self.int_only.insert((array.header, array.source));
                        }
                    }
                    self.builder.def_var(variables.values, view.values);
                    self.builder.def_var(variables.length, view.length);
                    let unproved = self.raw_const(Lane::I32, 0);
                    self.builder.def_var(variables.room, unproved);
                    if (array.header..=array.back).any(|pc| native_calls::searches(&self.intrinsics, pc)) {
                        let base = self.builder.use_var(variables.bits);
                        let held = self.helper_call(LaneHelper::PrototypesHoldNoIndex, &[base]);
                        let clear = self.raw_compare_imm(Relation::NotEqual, held, 0);
                        let checked = match self.prototype_clear.get(&array.source).copied() {
                            Some(checked) => checked,
                            None => {
                                let checked = self.builder.declare_var(types::I8);
                                self.prototype_clear.insert(array.source, checked);
                                checked
                            }
                        };
                        self.builder.def_var(checked, clear);
                    }
                }
                ElementViews::Typed(_) => {
                    let kind = array.views.observed_kind().and_then(NativeKind::new);
                    // A view an access before the loop proved and kept for the same register and kind is the loop's.
                    let register = array.source;
                    let kept = kind.and_then(|native| self.typed_view_of(state, (register, native.kind, array.writable), exit));
                    match kept {
                        Some(view) => {
                            self.builder.def_var(variables.values, view.values);
                            self.builder.def_var(variables.length, view.length);
                        }
                        None => {
                            self.hoist_typed_view(ViewAccess { kind, writable: array.writable }, bits, variables, exit);
                            if let (Some(register), Some(native)) = (register, kind) {
                                let (values, length) = (self.builder.use_var(variables.values), self.builder.use_var(variables.length));
                                state.proofs.typed.retain(|view| view.register != register);
                                state.proofs.typed.push(TypedProof { register, kind: native.kind, values, length, writable: array.writable, kept: false });
                            }
                        }
                    }
                }
                ElementViews::Double => {
                    let (values, length) = super::emit_double_array_view(&mut self.builder, self.payload, bits, array.writable, self.array_access().record, exit);
                    self.builder.def_var(variables.values, values);
                    self.builder.def_var(variables.length, length);
                }
            }
        }
    }

    /// The typed array `bits` holds, proved from its record once on the loop's entering edge
    /// (`prove_typed_view`): a proof that leaves takes the slow arm, where the VM publishes the
    /// record (`LaneHelper::TypedArrayView`) and the proof is made again, leaving at the loop's
    /// entry where it fails still.
    pub(super) fn hoist_typed_view(&mut self, access: ViewAccess, bits: ir::Value, variables: ArrayVariables, exit: ir::Block) {
        let publish = cold_block(&mut self.builder);
        let done = self.builder.create_block();
        self.prove_typed_view_into(variables, bits, access, publish);
        self.goto(done, &[]);
        self.builder.switch_to_block(publish);
        self.builder.seal_block(publish);
        let published = self.helper_call(LaneHelper::TypedArrayView, &[bits]);
        let admitted = self.raw_compare_imm(Relation::NotEqual, published, 0);
        emit_js_guard(&mut self.builder, admitted, exit);
        self.prove_typed_view_into(variables, bits, access, exit);
        self.goto(done, &[]);
        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
    }

    /// The proof of the typed array `bits` for `access`, leaving at `miss`, whose view the
    /// array's variables then hold.
    pub(super) fn prove_typed_view_into(&mut self, variables: ArrayVariables, bits: ir::Value, access: ViewAccess, miss: ir::Block) {
        let records = self.typed_records();
        let view = prove_typed_view(&mut super::object_slots::emit(&mut self.builder), records, bits, access, miss)
            .expect("the emit stage continues past its guards");
        self.builder.def_var(variables.values, view.values);
        self.builder.def_var(variables.length, view.length);
        if let ViewKind::Dynamic(kind) = view.kind { self.builder.def_var(variables.kind, kind); }
    }

    pub(super) fn array_variables(&mut self, array: LoopArray) -> ArrayVariables {
        let builder = &mut self.builder;
        let length = if array.views == ElementViews::Dense { types::I32 } else { types::I64 };
        *self.array_vars.entry(array.source).or_insert_with(|| ArrayVariables {
            bits: builder.declare_var(types::I64),
            values: builder.declare_var(types::I64),
            length: builder.declare_var(length),
            kind: builder.declare_var(types::I64),
            room: builder.declare_var(types::I32),
        })
    }

    /// After the element store's helper, which may grow any record array's backing, through
    /// whichever register names it: every dense or double view hoisted around `pc` reads its
    /// backing again (`emit_dense_array_reload`, `emit_double_array_reload`), and its append room is proved again
    /// (`forget_append_rooms`). A typed region's views, which no such store moves, stay.
    pub(super) fn reload_dense_arrays(&mut self, pc: usize) {
        self.forget_append_rooms(pc);
        self.reload_dense_views(pc);
    }

    /// `reload_dense_arrays` after a step that moves no backing and grows none (`pop`, which
    /// shortens one in place): each view reads its count again, and its append room, the
    /// backing's capacity, and its prototypes' proof stay.
    pub(super) fn reload_dense_views(&mut self, pc: usize) {
        let arrays: Vec<LoopArray> = self.loop_arrays.iter().copied()
            .filter(|array| matches!(array.views, ElementViews::Dense | ElementViews::Double) && (array.header..=array.back).contains(&pc)).collect();
        for array in arrays {
            // An int-only array has no store or append in its loop (`int_only_array`), so no reload reaches it.
            debug_assert!(!self.int_only.contains(&(array.header, array.source)), "a reload of an int-only array");
            let variables = self.array_variables(array);
            let bits = self.builder.use_var(variables.bits);
            if array.views == ElementViews::Double {
                let (values, length) = super::emit_double_array_reload(&mut self.builder, self.payload, bits, self.array_access().record);
                self.builder.def_var(variables.values, values);
                self.builder.def_var(variables.length, length);
                continue;
            }
            let (values, length) = super::emit_dense_array_reload(&mut self.builder, self.payload, bits, self.array_access().record);
            // A grown backing may count its values again; stores through the view keep none. An int32
            // loop's count stays exact (`hoist_arrays`), so its view marks nothing.
            if array.writable && !self.int32_loop(array) {
                super::emit_dense_array_uncounted(&mut self.builder, super::DenseArrayView { values, length, writable: true, uncounted: false, double: None });
            }
            self.builder.def_var(variables.values, values);
            self.builder.def_var(variables.length, length);
        }
    }

    /// The hoisted view's elements' address and count.
    pub(super) fn hoisted_view(&mut self, array: LoopArray) -> (ir::Value, ir::Value) {
        let variables = self.array_variables(array);
        (self.builder.use_var(variables.values), self.builder.use_var(variables.length))
    }

    /// A typed region's hoisted array kind: the site's, which its proof compared once, or the record's.
    pub(super) fn hoisted_kind(&mut self, array: LoopArray) -> ViewKind<ir::Value> {
        match array.views.observed_kind().and_then(NativeKind::new) {
            Some(native) => ViewKind::Static(native),
            None => {
                let variables = self.array_variables(array);
                ViewKind::Dynamic(self.builder.use_var(variables.kind))
            }
        }
    }

    /// A store through a hoisted dense array view at a site that left (`left_here`): a Number in place,
    /// else appended inline (`dense_append`), else the VM's store. A reference first passes the barrier
    /// gate (`frame_elements::store_gate`); a gate miss (a marked or non-direct base) takes the VM's
    /// store directly, and the in-place and appended paths only run past the gate. The VM's store,
    /// which may move any backing, is followed by the reload of every view around `pc`.
    pub(super) fn dense_element_write(&mut self, state: &mut State, array: LoopArray, pc: u32, instruction: WideInstruction,
        number: bool, exit: ir::Block) {
        let index = self.int(state, instruction.register_c(), exit);
        let (values, length) = self.hoisted_view(array);
        let in_bounds = self.ranges.in_bounds(pc as usize);
        let value = self.tagged(state, instruction.register_a());
        let base = self.tagged(state, instruction.register_b());
        let key = self.tagged(state, instruction.register_c());
        let (slow, helper, done) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(slow);
        self.builder.set_cold_block(helper);
        if !number {
            let fresh = self.holder_fresh(state, Some(instruction.register_b()));
            let mut stage = super::object_slots::emit(&mut self.builder);
            stage.fresh = fresh;
            frame_elements::store_gate(&mut stage, self.payload, base, value, helper)
                .expect("the emit stage continues past its guards");
        }
        let view = super::DenseArrayView { values, length, writable: true, uncounted: true, double: None };
        // The view's backing counts no values (`hoist_arrays`, reload): the store writes the element,
        // and the unknown count stays unknown under the store of a reference as under a Number.
        self.index_write(state, index, value, view, slow, in_bounds);
        self.goto(done, &[]);
        self.builder.switch_to_block(slow);
        self.builder.seal_block(slow);
        self.dense_append(array, pc, index, value, base, done);
        self.goto(helper, &[]);
        self.builder.switch_to_block(helper);
        self.builder.seal_block(helper);
        if self.int32_loop(array) {
            // An int32 loop's store past the end would make a hole, which its reads do not test for
            // (`int32_loop`): it leaves before any write, and the VM makes the store.
            self.goto(exit, &[]);
        } else {
            let proof = super::EmitProof::IndexSetHelper { miss: exit };
            self.step(state, frame::Edges::ValueGuarded { miss: exit },
                quench_runtime::staged::Operation::IndexSetHelper { base, key, value, proof });
            self.reload_dense_arrays(pc as usize);
            self.goto(done, &[]);
        }
        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
    }
}
