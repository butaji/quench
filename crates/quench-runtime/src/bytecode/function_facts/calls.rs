//! The call plan and the facts a function derives from its code and its layout.

use super::super::*;

impl Function {
    pub(super) fn derive_call_plan(&self, tables: &impl FactTables) -> CallPlan {
        {
            let promoted_parameters = self.local_registers.iter().filter(|entry| entry.local < self.params).count();
            // Laid out as its registers whatever kind of function it is: an ordinary
            // body's layout, or a non-suspending async body's (`settles_registers_only`).
            let registers_layout_of_any_kind = self.parent.is_some()
                && !self.derived_constructor
                && self.self_binding_slot.is_none()
                && !self.inherited_with_scope
                && promoted_parameters == usize::from(self.params)
                // A plain activation makes no closure, whichever bindings it would see.
                && !self.instructions().any(|instruction| instruction.op() == Op::MakeClosure);
            let registers_layout = registers_layout_of_any_kind && !self.is_generator && !self.is_async;
            // Plain but for an `arguments` binding.
            // An arrow's `this` is its environment's, which its activation binds
            // (`Vm::lexical_activation_this`). One that reaches `super` reads a hidden name
            // (`activation_binding_reads`) or its home as a capture, as any closure reads one.
            // Ordinary but for a rest parameter, which a frameless activation gathers
            // (`CallEntry::Gathers`) and a plain frame does not.
            let ordinary_with_rest = registers_layout && !self.activation_binding_reads(tables);
            // A base class constructor laid out as an ordinary function runs frameless when
            // constructed (`lane_script_construct`); its calls, which throw, keep a frame.
            let constructs_frameless = registers_layout
                && self.is_class_constructor
                && !self.rest
                && !self.binding_reads_beside_class(tables);
            let ordinary = ordinary_with_rest && !self.rest;
            // A non-suspending async body laid out as an ordinary one's, which keeps no
            // passed values: its activation settles its call's promise.
            let settles_with_registers = registers_layout_of_any_kind
                && !self.body_binding_reads(tables)
                && !self.rest
                && self.arguments_slot.is_none();
            // A field initializer is an ordinary method call on the instance whose
            // activation keeps its implicit bindings (`activation_binding_reads`); only the
            // derived instance initializer, which `super()` runs, is entered natively.
            let in_loop = !self.is_async
                && !self.is_generator
                && !self.is_class_constructor
                && !self.derived_constructor;
            let settles_in_loop = self.is_async
                && !self.is_generator
                && !self.is_class_constructor
                && !self.derived_constructor
                && !self.suspends();
            let legacy_reflection = !self.has_restricted_legacy_caller_access();
            let live_parameters = self.reflects_live_parameters(tables);
            let kept_arguments = self.kept_arguments();
            let forwards_arguments = kept_arguments == Some(KeptArguments::Forwards);
            let addresses_locals = self.instructions().any(|instruction| {
                instruction.op().captures_environment()
                    || instruction.addressed_local_slots().next().is_some()
            });
            let reaches_only_forwarded_arguments = kept_arguments.is_some()
                && self.instructions().all(|instruction| {
                    instruction
                        .addressed_local_slots()
                        .all(|slot| Some(slot) == self.arguments_slot)
                });
            // An ordinary call places its arguments in the window's locals, which a promoted
            // parameter has none of, and fills the registers with `undefined`; a suspended
            // frame is made with both, and no suspending function starts its registers apart
            // from `undefined` (Wasm's i32 zero belongs to its bodies, which never suspend).
            let environment_activation = in_loop
                && self.parent.is_some()
                && self.local_registers.is_empty()
                && self.initial_register == InitialRegister::Undefined
                && self.locals_ride_window(tables);
            let suspendable = self.suspends_in_record();
            let suspended_registers = suspendable
                && registers_layout_of_any_kind
                && !self.rest
                && self.arguments_slot.is_none()
                && !addresses_locals
                && !self.has_dynamic_scope_access();
            // Its locals and closures ride an environment window as an ordinary body's do, and so
            // does the `arguments` object it made on entry (`arguments_ride_window`); so do its
            // `with` objects and dynamic bindings, which its record and activation carry.
            // An async module's body is a program's root, which no function encloses: its slots
            // that stand for imports and global bindings are read through the activation
            // (`Vm::root_binding`).
            let suspended_environment = suspendable
                && !suspended_registers
                && (self.parent.is_some() || self.is_async)
                && self.locals_ride_window(tables);
            CallPlan {
                environment_activation,
                suspended_registers,
                suspended_environment,
                admits_direct_eval: self.evals_in_scope() && (environment_activation || suspended_environment),
                plain: ordinary && self.arguments_slot.is_none(),
                in_loop,
                settles_in_loop,
                legacy_reflection,
                live_parameters,
                addresses_locals,
                forwards_arguments,
                keeps_arguments: kept_arguments.is_some(),
                reads_passed_arguments: self.reads_passed_arguments_only(),
                registers_only: (ordinary_with_rest && in_loop || constructs_frameless || settles_in_loop && settles_with_registers)
                    && self.initial_register == InitialRegister::Undefined
                    && if self.arguments_slot.is_none() {
                        // A rest body's activation keeps every value passed
                        // (`CallEntry::Gathers`), which legacy reflection reads.
                        !addresses_locals && (!legacy_reflection || live_parameters || self.rest)
                    } else {
                        reaches_only_forwarded_arguments
                    },

            }
        }
    }

    /// The facts below and the call plan, each derived now and packed as an encoding keeps
    /// them (`binary::write_function`): derived lazily, they had cost 3-7% of the instructions
    /// of cached npm runs (axios, playwright), which each run derived again.
    /// `names_resolve_statically` is given: the one fact that reads the enclosing functions,
    /// which a compiler finishing this function early has not finished yet
    /// (`binary::LoadEncoding`).
    pub(crate) fn derived_facts(&self, tables: &impl FactTables, names_resolve_statically: bool) -> u32 {
        let facts = [
            (DerivedFact::ActivationBindings, self.activation_bindings_observable(tables)),
            (DerivedFact::LiveParameters, self.reflects_live_parameters(tables)),
            (DerivedFact::StaticNameScope, names_resolve_statically),
            (DerivedFact::DynamicScopeAccess, self.has_dynamic_scope_access()),
        ];
        let flags = facts.iter().fold(0, |bits, &(fact, holds)| if holds { bits | fact.bit() } else { bits });
        flags | (self.call_plan(tables).to_bits() << DERIVED_FACT_COUNT)
    }

    /// Seeds the facts `derived_facts` packed; a fact already derived keeps its value.
    pub(crate) fn seed_derived_facts(&self, bits: u32) {
        let fact = |fact: DerivedFact| bits & fact.bit() != 0;
        let _ = self.activation_bindings.set(fact(DerivedFact::ActivationBindings));
        let _ = self.live_parameter_reflection.set(fact(DerivedFact::LiveParameters));
        let _ = self.static_name_scope.set(fact(DerivedFact::StaticNameScope));
        let _ = self.dynamic_scope_access.set(fact(DerivedFact::DynamicScopeAccess));
        let _ = self.call_plan.set(u16::try_from(bits >> DERIVED_FACT_COUNT).expect("the plan's flags fit 16 bits"));
    }

    /// Whether each fact `derived_facts` packs is this function's own derivation, whether it
    /// was derived or seeded (`ResidualProgram::validate`).
    pub(crate) fn derived_facts_hold(&self, program: &ResidualProgram) -> bool {
        self.activation_bindings_observable(program)
            == self.derive_activation_bindings_observable(program)
            && self.reflects_live_parameters(program) == self.derive_reflects_live_parameters(program)
            && self.names_resolve_statically(program) == self.derive_names_resolve_statically(program)
            && self.has_dynamic_scope_access() == self.derive_dynamic_scope_access()
            && self.call_plan(program).to_bits() == self.derive_call_plan(program).to_bits()
    }

    /// Whether the body's whole effect is storing parameters into fields of `this`, each as
    /// passed or a Number in its place where it is falsy, then returning `undefined`
    /// (`initializer_stores`). `constants` is the owning program's table.
    pub(crate) fn only_initializes_this_fields(&self, constants: &[Constant]) -> bool {
        self.initializer_stores(constants).is_some()
    }
}
