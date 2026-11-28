//! A loop's invariant reads and record stores, hoisted to its entering edge (`Lowering::hoist_reads`): each read is made once
//! there and defined as the value the loop's uses read, and each store's projection (`LoopStore`) is made once there. A fixed
//! class's receiver reads its union slot in its class cell (`fixed_hoisted_word`), and its stores project through
//! `fixed_loop_store`; neither makes a shape or presence test.

use super::*;

impl Lowering<'_, '_> {
    /// The loop's invariant captures and fields, read once on its entry edge.
    pub(crate) fn hoist_reads(&mut self, state: &mut State, header: usize) {
        let Some(reads) = self.hoists.get(&header).cloned() else { return };
        let boundaries = leaders(&self.unit.instructions);
        for InvariantRead { pc, owner, rooted, callee } in reads.iter().copied() {
            let instruction = self.unit.instructions[pc];
            if self.hoisted_by_enclosing_loop(pc, owner) { continue; }
            let field = self.loop_fields.iter().position(|field| field.reads[0] == pc);
            let exit = self.guard_exit(state, header as u32);
            // The flag `typed`: the solver or the read already types the value's int32 tag, so no test is made for it.
            let (value, int32, typed) = if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::LoadCapture { .. })) {
                let Ok(admitted_callee) = self.admitted_callee_read(state, pc, &reads, &boundaries, exit, callee) else { continue };
                let value = self.load_capture(state, pc, instruction, exit, admitted_callee, if rooted { RootedRead::OnEntry } else { RootedRead::No });
                let typed = self.capture_solved_int(pc);
                let int32 = self.capture_int32_use(pc, &boundaries) || typed;
                (value, int32, typed)
            } else {
                let Some(FieldLookup::Atom { atom, base, cache_site }) = instruction.field_lookup() else { continue };
                let register = owner.or_else(|| base.register_index());
                // A fixed class's receiver reads its union slot from its cell on the entering edge: no record, no shape test
                // (`fixed_hoisted_word`). `None` names `this`, so only a register or the `this` base reads one.
                let fixed_read = (register.is_some() || base == quench_runtime::bytecode::FieldBase::THIS)
                    .then(|| self.fixed_hoisted_word(state, pc, register, atom, exit)).flatten();
                if let Some((value, ty)) = fixed_read {
                    (value, ty.is_subtype(Type::INT32), false)
                } else {
                    // A `length` read is made once, natively where its base is a direct array.
                    match register.filter(|_| super::super::names_length(self.program, atom)) {
                        Some(register) => {
                            let length = self.length_read(state, pc, register, (atom, cache_site), exit, Some(header));
                            (int_tagged(&mut self.builder, length), true, true)
                        }
                        None => {
                            let base = register.map_or(quench_runtime::bytecode::FieldBase::THIS, quench_runtime::bytecode::FieldBase::register);
                            let lookup = FieldLookup::Atom { atom, base, cache_site };
                            let Some(value) = self.field_value(state, (base, lookup), &[(atom, cache_site)], HeldRead::default(), exit) else { continue };
                            (value, super::field_reads::int32_read(self.program, self.fields, instruction), false)
                        }
                    }
                }
            };
            let carried = field.is_some();
            let int32 = if carried {
                self.loop_fields[field.unwrap()].stores.iter().all(|store| {
                    self.cell_before(*store, self.unit.instructions[*store].register_a()).ty.is_int32()
                })
            } else { int32 };
            // An int32 read from a load is tested here; a capture the solver types int32, or a length, already holds its tag.
            if int32 && !typed {
                let integer = is_int_tagged(&mut self.builder, value);
                emit_js_guard(&mut self.builder, integer, exit);
            }
            let mut ty = if int32 { Type::INT32 } else { Type::ANY };
            if !int32 {
                // This is a candidate, never a proof from source syntax. Admit the
                // loaded closure on the same entry edge that already hoists its
                // immutable read. A failed admission exits before loop effects.
                let target = self.unit.markers.iter().enumerate().find_map(|(frame, marker)| {
                    let call = self.unit.frames[frame];
                    let supplies = if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::LoadCapture { .. })) {
                        // Preserve the existing caller-capture admission, including
                        // function-bound alternatives. Hoisting a callee's own
                        // captures additionally requires the monomorphic proof below.
                        let destination = instruction.result_register();
                        *marker > pc && call.callee_register == destination
                            && (call.target.callee == CalleeBinding::Function
                                || (call.alternative.is_none() && !self.unit.frames.iter()
                                    .any(|other| other.alternative == Some(*marker))))
                            && !self.unit.instructions[pc + 1..*marker].iter()
                                .any(|instruction| writes(*instruction).contains(&destination))
                    } else { invariant_callee_read(self.unit, [pc], frame, &boundaries) == Some(pc) };
                    supplies.then_some(call.target)
                });
                if let Some(target) = target {
                    // The loaded value is the one closure of the body where the solve proves it (`Identity::Closure`): no binding check.
                    let proven = matches!(target.callee, CalleeBinding::Function)
                        && self.is_body_closure_identity(pc, instruction.result_register(), target.function);
                    if !proven {
                        let matched = match target.callee {
                            CalleeBinding::Function => self.function_binding(value, target.function),
                            CalleeBinding::Identity(identity) => self.is_identity(value, identity),
                        };
                        emit_js_guard(&mut self.builder, matched, exit);
                    }
                    ty = match target.callee {
                        CalleeBinding::Function => Type::function_body(target.function),
                        CalleeBinding::Identity(identity) =>
                            Type::function(identity).expect("admitted callee locator"),
                    };
                }
            }
            let representation = repr(ty);
            let raw = match representation {
                Representation::Int32 => self.raw_unary(Unary::Narrow(Lane::I32), value),
                Representation::Float64 => {
                    super::frame::number_word(&mut self.builder, state, value, exit)
                }
                _ => value,
            };
            let variable = self.builder.declare_var(match representation {
                Representation::Int32 => types::I32,
                Representation::Float64 => types::F64,
                _ => types::I64,
            });
            self.builder.def_var(variable, raw);
            let receiver = if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::GetField { .. })) {
                match instruction.field_lookup() {
                    Some(FieldLookup::Atom { base, .. }) => owner.or_else(|| base.register_index())
                        .map(|owner| (owner, state.values[usize::from(owner)].value.ty)),
                    _ => None,
                }
            } else { None };
            // No own slot holds raw doubles (invariant I), so only an int32 store is carried.
            let stores: smallvec::SmallVec<[usize; INLINE_FIELD_BINDINGS]> = if int32 {
                field.into_iter().flat_map(|index| self.loop_fields[index].stores.iter().copied()).collect()
            } else { smallvec::SmallVec::new() };
            // A fixed class's receiver is no record: its stores project their union slots in its cell (`fixed_loop_store`).
            // An ordinary receiver's store names no loop slot: it takes the general store, whose class arms are the site's.
            let mut has_record_stores = false;
            for store in stores {
                match receiver {
                    Some((owner, _)) if self.writable_class(store, Some(owner)).is_some() => {
                        if let Some(loop_store) = self.fixed_loop_store(state, store, owner) {
                            self.loop_stores.insert(store, loop_store);
                        }
                    }
                    _ => has_record_stores = true,
                }
            }
            // An entry-fixed receiver has no record a store could name, so it proves no header (`fixed_loop_store`).
            let object_header = if has_record_stores && !receiver.is_some_and(|(owner, _)| self.entry_fixed(Some(owner))) {
                receiver.and_then(|(owner, _)| self.object_header(state, Some(owner), false, exit))
            } else { None };
            let read = LoopRead { value: variable, ty, representation, receiver, header: object_header, step: None };
            if let Some(index) = field {
                for pc in &self.loop_fields[index].reads { self.hoisted.insert(*pc, read); }
            } else { self.hoisted.insert(pc, read); }
        }
    }

}
