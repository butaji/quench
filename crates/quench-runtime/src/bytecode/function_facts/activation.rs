//! A function's activation: its bindings, what its code reads of them, and the window its locals ride.

use super::super::*;

impl Function {
    /// Whether the function runs a direct eval, which declares `var`s in its variable
    /// environment and reads its scope by name, or records binding sites, the lexical scope
    /// its closures and evals resolve names in.
    pub(crate) fn evals_in_scope(&self) -> bool {
        !self.binding_sites.is_empty()
            || self.instructions().any(|instruction| {
                instruction.op().is_call_with_eval_flags() && (instruction.direct_eval() || instruction.parameter_eval())
            })
    }

    /// The implicit bindings (`new.target`, `super`, the home object, a derived constructor's
    /// `this`) the body reads by a hidden name or a scope operation, which a window activation
    /// leaves to the frame it becomes: not those of a direct eval, which its record serves.
    pub(super) fn reads_implicit_bindings(&self, tables: &impl FactTables) -> bool {
        self.derived_constructor
            // A field initializer or static block keeps its instance's implicit bindings; an
            // arrow inside one (also `class_initializer`) binds none of its own.
            || (self.class_initializer && !self.is_arrow)
            || self.instance_initializer.is_some()
            || self.inherited_with_scope
            || self.instructions_of(name_bindings::may_read_binding).any(|instruction| {
                name_bindings::reads_hidden_name(instruction, tables)
                    || matches!(
                        instruction.op(),
                        Op::DeleteName | Op::StoreResolvedName | Op::StoreVarBinding | Op::InitializeThis | Op::SuperCallCheck
                    )
            })
            // A direct eval beside names resolved through a scope may declare dynamic bindings, which only
            // a frame (or a suspended record) keeps: the scope's other names the window's handlers serve.
            || self.evals_in_scope() && self.instructions().any(|instruction| matches!(instruction.op(), Op::ResolveName | Op::LoadResolvedName))
    }

    /// Every instruction in fixed-width form.
    pub(crate) fn instructions(&self) -> impl Iterator<Item = WideInstruction> + '_ {
        self.code.iter().map(|packed| {
            if packed.is_wide() {
                self.wide[packed.wide_index()]
            } else {
                packed.as_wide()
            }
        })
    }

    /// Whether a closure environment may hold local `slot`: the selective layout names it or,
    /// without one, the function captures its whole frame by creating closures or reaching its
    /// environment.
    pub(super) fn local_slot_captured(&self, slot: usize) -> bool {
        match &self.selective_capture_slots {
            Some(captured) => {
                u16::try_from(slot).is_ok_and(|slot| captured.binary_search(&slot).is_ok())
            }
            None => self
                .instructions_of(|op| matches!(op, Op::MakeClosure | Op::LoadEnvLocal | Op::StoreEnvLocal))
                .next()
                .is_some(),
        }
    }

    /// Whether legacy `f.arguments` reads each passed parameter's current frame value, as V8
    /// does for parameters that live only in the frame: no code or closure uses an `arguments`
    /// binding, the parameters are simple, and each is a plain or register-promoted local.
    /// Otherwise the arguments are reflected as passed.
    pub(crate) fn reflects_live_parameters(&self, tables: &impl FactTables) -> bool {
        *self.live_parameter_reflection.get_or_init(|| self.derive_reflects_live_parameters(tables))
    }

    pub(super) fn derive_reflects_live_parameters(&self, tables: &impl FactTables) -> bool {
        {
            let arguments_used = self.local_atoms.iter().enumerate().any(|(slot, &atom)| {
                usize::try_from(atom).is_ok_and(|atom| tables.atom_name(atom) == Some("arguments"))
                    && (self.local_slot_captured(slot)
                        || self.instructions().any(|instruction| {
                            instruction.op().immediate_role() == ImmediateRole::LocalSlot
                                && instruction.local_slot() == slot
                        }))
            });
            self.arguments_slot.is_none()
                && !arguments_used
                && self.simple_parameters
                && !self.rest
                && (0..self.params).all(|parameter| {
                    self.plain_local(tables, usize::from(parameter))
                        || self
                            .local_registers
                            .iter()
                            .any(|entry| entry.local == parameter)
                })
        }
    }

    /// Whether an activation is fully described by the function's static layout: a nested,
    /// non-derived function whose parameters all live in registers, with no rest
    /// array, self binding, `arguments` object, parameter-scope `arguments` error, inherited
    /// `with` scope or observable implicit bindings. Such a frame needs only its storage sized
    /// (cleared when no root map lets the collector skip stale registers), its parameters
    /// placed and its scalar state set; see `activate_plain_frame`.
    pub(crate) fn plain_activation(&self, program: &ResidualProgram) -> bool {
        self.call_plan(program).plain
    }

    /// Whether every parameter slot is named by its binding rather than a hidden parameter
    /// name, as a list that binds directly names them (`FunctionCompiler::binds_parameters_directly`):
    /// a rest list of identifiers then fills its last slot with the rest array and binds
    /// nothing else.
    /// `name` spells an atom of the function's table (the program's, or the compiler's while
    /// it lays the function out).
    pub(super) fn parameters_bind_directly<'n>(&self, name: impl Fn(usize) -> Option<&'n str>) -> bool {
        self.local_atoms[..usize::from(self.params).min(self.local_atoms.len())]
            .iter()
            .all(|atom| usize::try_from(*atom).ok().and_then(&name).is_some_and(|name| !name.starts_with('\0')))
    }

    /// Whether each parameter lives in its own named slot: a simple list, or a rest list that
    /// binds directly (`parameters_bind_directly`). Selective capture layouts, environment
    /// activations and promoted locals all require it, and the compiler and the validator
    /// read this one rule.
    pub(crate) fn parameters_own_their_slots<'n>(&self, name: impl Fn(usize) -> Option<&'n str>) -> bool {
        self.simple_parameters || self.rest && self.parameters_bind_directly(name)
    }

    /// Whether each argument arrives in a slot of its own that only the activation's own
    /// loads and stores reach: `parameters_own_their_slots`, or a list without a rest
    /// parameter whose patterns or defaults bind its names apart, each argument then in its
    /// hidden parameter slot (`HIDDEN_PARAMETER_PREFIX`). Promoted locals require it.
    pub(crate) fn arguments_own_their_slots<'n>(&self, name: impl Fn(usize) -> Option<&'n str>) -> bool {
        self.parameters_own_their_slots(&name)
            || !self.rest
                && self.local_atoms[..usize::from(self.params).min(self.local_atoms.len())].iter().all(|atom| {
                    usize::try_from(*atom)
                        .ok()
                        .and_then(&name)
                        .is_some_and(|name| name.starts_with(HIDDEN_PARAMETER_PREFIX))
                })
    }

    /// Whether some jump of the body targets its own or an earlier instruction: a loop.
    pub(crate) fn loops(&self) -> bool {
        has_backward_branch(&self.code, &self.wide)
    }

    /// Whether an activation can suspend: an `await`, or a `yield` of a generator.
    pub(crate) fn suspends(&self) -> bool {
        // By opcode alone: a wide record's opcode is in the side table.
        self.code.iter().any(|packed| {
            if packed.is_wide() { self.wide[packed.wide_index()].op().suspends() } else { packed.op().suspends() }
        })
    }

    /// Whether a suspension of the function parks its frame in a record a lane resumes from: a
    /// generator or an async function. An arrow's `this` is its environment's, which each
    /// resumption reads again (`Vm::window_this`).
    pub(super) fn suspends_in_record(&self) -> bool {
        self.is_generator || self.is_async
    }

    /// Whether its locals and the environment its closures capture can live in a window past
    /// its registers: the layout an environment activation (`CallPlan::environment_activation`)
    /// and a suspended generator's window (`CallPlan::suspended_environment`) share. Each
    /// local that is not promoted is a frame local, the captured ones owned by the environment,
    /// and every instruction that addresses one is a record the window's handlers read
    /// (`window_reads_locals`); the promoted ones are registers, which the window's register
    /// file holds as it holds every register. A function's own name binding is a local too,
    /// which the entry initializes with the callee and whose stores leave the lane
    /// (`specialize_environment_window`).
    pub(super) fn locals_ride_window(&self, tables: &impl FactTables) -> bool {
        self.arguments_own_their_slots(|atom| tables.atom_name(atom))
            && (self.arguments_slot.is_none() || self.reads_passed_arguments_only() || self.arguments_ride_window())
            // The implicit bindings `activation_binding_reads` names, but for clones, are a frame's:
            // a suspended function's record keeps them (and gives them to the environment its
            // window makes, `window_environment`), and the opcodes that read them
            // exit the lane to the frame it becomes.
            && (self.names_resolve_in_window() || self.holds_scope())
            && (!self.reads_implicit_bindings(tables) || self.suspends_in_record())
            && self.instructions().all(|instruction| {
                instruction.op().immediate_role() != ImmediateRole::SuperinstructionIndex
                    && self.window_reads_locals(tables, instruction)
            })
    }

    /// Whether the handlers of an environment window read every local `instruction` addresses:
    /// the local loads, stores and dead-zone openings, a field read from a plain local
    /// (`lane_plain_local_operand`), a binary operator of a suspending function over plain
    /// locals, whose operands and numeric target the window holds (`lane_store_numeric_local`),
    /// the reads of the values
    /// passed, which the window keeps, and a function's store to a `var` binding its Annex B
    /// block function declares (`StoreVarBinding`), which is a store to the slot: only a
    /// program's root stores through the global object or an eval's record.
    pub(super) fn window_reads_locals(&self, tables: &impl FactTables, instruction: WideInstruction) -> bool {
        let op = instruction.op();
        instruction.addressed_local_slots().next().is_none()
            || match op {
                Op::LoadLocal | Op::LoadLocalPlain | Op::LoadEnvLocal | Op::StoreLocal | Op::StoreLocalPlain | Op::StoreEnvLocal | Op::InitializeTdz => true,
                Op::StoreVarBinding => self.parent.is_some(),
                Op::GetField => instruction.addressed_local_slots().eq(instruction.local_field_base()),
                Op::Binary | Op::NumericAdd | Op::NumericMultiply if self.suspends_in_record() => {
                    instruction.addressed_local_slots().all(|slot| self.plain_local(tables, usize::from(slot)))
                }
                _ => op.reads_arguments(),
            }
    }

    /// Whether every name of the function resolves by slot, which an environment window answers: none
    /// goes through a `with` object, a binding site or a dynamic binding (`has_dynamic_scope_access`).
    pub(super) fn names_resolve_in_window(&self) -> bool {
        !self.inherited_with_scope && self.binding_sites.is_empty() && !self.has_dynamic_scope_access()
    }

    /// Whether a function may lay out an environment window though it resolves names through `with`
    /// objects, binding sites or dynamic bindings, which its activation's scope chain and, for a
    /// function that suspends, its record carry: one that makes no iteration copy of its environment
    /// (`CloneEnv`), which would have to share them (`Vm::iteration_environment`).
    pub(super) fn holds_scope(&self) -> bool {
        !self.instructions().any(|instruction| instruction.op() == Op::CloneEnv)
    }

    /// Whether the `arguments` object a suspending function made on entry rides its window as
    /// the local it was stored in, which the lane reads in place (`lane_frame_arguments`): a
    /// window local, or, for a binding a closure captures, its slot in the environment the
    /// window makes (`mapped_windows::window_binding`). A mapped object (`arguments_alias_parameters`) aliases
    /// parameters that live in the window as well, the captured ones among them, which ride the
    /// window's locals until a closure makes its environment and its slots after
    /// (`ParameterHome`): the window's parameter reads and writes go through the object
    /// (`lane_load_mapped_parameter`), whose element writes reach the binding
    /// (`LaneStack::window_parameter`).
    pub(crate) fn arguments_ride_window(&self) -> bool {
        self.arguments_slot.is_some() && self.suspends_in_record()
    }
}
