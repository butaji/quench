use super::*;

mod activation;
mod calls;
mod dynamic;
mod initializers;

impl Function {
    /// Releases the spare capacity of the tables a compilation grew, once it is finished.
    pub(crate) fn shrink_to_fit(&mut self) {
        self.code.shrink_to_fit();
        self.wide.shrink_to_fit();
        self.source_positions.shrink_to_fit();
    }

    pub(super) fn global_declarations_or_none(&self) -> Option<&GlobalDeclarations> {
        self.global_declarations.as_deref()
    }

    /// The slots a module body's environment holds for its importers and linker: its imports
    /// (whatever their phase), its exported locals and its hoisted functions, which `link_plan`
    /// names and the linker addresses by slot. The body's other bindings are its own.
    pub(crate) fn module_environment_slots<'a>(
        &'a self,
        link_plan: &'a ModuleLinkPlan,
        imports: &'a [ModuleImportBinding],
        atom_text: impl Fn(Atom) -> Option<&'a str> + 'a,
    ) -> impl Iterator<Item = u16> + 'a {
        // A set, once: a large module's locals against its whole link plan was quadratic.
        let linked: rustc_hash::FxHashSet<&str> = imports
            .iter()
            .map(|import| import.local.as_str())
            .chain(link_plan.locals.iter().chain(&link_plan.hoisted_functions).map(|(local, _)| local.as_str()))
            .collect();
        self.local_atoms
            .iter()
            .enumerate()
            .filter(move |(_, atom)| atom_text(**atom).is_some_and(|name| linked.contains(name)))
            .filter_map(|(slot, _)| u16::try_from(slot).ok())
    }

    /// Reusing an activation is observable while its call is protected by a handler.
    /// A terminal return may skip its following Return instruction without being a
    /// proper tail call: the caller must still catch the callee's abrupt completion.
    pub(crate) fn permits_js_tail_call_at(&self, pc: u32) -> bool {
        self.strict && !self.class_initializer && self.handler_at(pc).is_none()
    }

    pub(crate) fn handler_at(&self, pc: u32) -> Option<Handler> {
        self.handlers
            .iter()
            .find(|handler| pc >= handler.start && pc < handler.end)
            .copied()
    }

    pub(crate) fn promoted_register(&self, local: u16) -> Option<Register> {
        self.local_registers
            .binary_search_by_key(&local, |entry| entry.local)
            .ok()
            .map(|index| self.local_registers[index].register)
    }

    /// Whether the instruction at `pc` returns `register`. A call is in tail position only
    /// when the instruction after it returns that call's own result.
    pub(crate) fn has_restricted_legacy_caller_access(&self) -> bool {
        self.strict || !self.constructible || self.is_class_constructor
    }

    pub(crate) fn arguments_are_mapped(&self) -> bool {
        !self.is_arrow && !self.strict && self.simple_parameters
    }

    /// Whether the `arguments` object's indices alias the parameters: a mapped object
    /// whose mapping some write could observe (`unaliased_arguments`).
    pub(crate) fn arguments_alias_parameters(&self) -> bool {
        self.arguments_are_mapped() && !self.unaliased_arguments
    }

    pub(crate) fn plain_local_context_is_safe(&self) -> bool {
        self.plain_local_context_ineligibility().is_none()
    }

    pub(crate) fn plain_local_context_ineligibility(
        &self,
    ) -> Option<PlainLocalContextIneligibility> {
        let selective = self.selective_capture_slots.is_some();
        // A root's locals are its script's global bindings, unless it is a module body whose
        // activation splits (only a module body's may): then its uncaptured locals are its own.
        if self.parent.is_none() && !selective {
            return Some(PlainLocalContextIneligibility::RootFunction);
        }
        let has_legacy_environment_access = self.code.iter().any(|instruction| {
            instruction.op() == Op::MakeClosure && !selective
                || instruction.op().addresses_env_local() && !selective
        }) || self.wide.iter().any(|instruction| {
            instruction.op() == Op::MakeClosure && !selective
                || instruction.op().addresses_env_local() && !selective
        });
        if has_legacy_environment_access {
            return Some(PlainLocalContextIneligibility::LegacyCaptureLayout);
        }
        let has_dynamic_name_resolution = self.code.iter().any(|instruction| {
            instruction.op() == Op::ResolveName
                || instruction.op().is_call_with_eval_flags() && instruction.direct_eval()
        }) || self.wide.iter().any(|instruction| {
            instruction.op() == Op::ResolveName
                || instruction.op().is_call_with_eval_flags()
                    && ImmediateLayout::direct_eval(instruction.imm())
        });
        has_dynamic_name_resolution.then_some(PlainLocalContextIneligibility::DynamicNameResolution)
    }

    /// Which local slots are plain: read and written only through the frame's local array,
    /// with no environment, mapped-argument or dynamic-scope observer, and no read inside a
    /// temporal dead zone (`initialization`). Compilation specializes these slots'
    /// operations, validation re-derives the same proof, and the interpreter memoizes it in
    /// `plain_locals` to read plain local operands directly.
    pub(crate) fn plain_local_slots(&self, superinstructions: &[Superinstruction]) -> crate::bitset::BitSet {
        let locals = usize::from(self.locals);
        if !self.plain_local_context_is_safe() {
            return crate::bitset::BitSet::new(locals);
        }
        let plain = crate::bitset::BitSet::from_fn(locals, |slot| self.plain_local_slot_is_safe(slot));
        let mut dead_zone = crate::bitset::BitSet::new(locals);
        for instruction in self.instructions_of(|op| op == Op::InitializeTdz) {
            if plain.contains(instruction.local_slot()) {
                dead_zone.insert(instruction.local_slot());
            }
        }
        let initialized =
            initialization::initialized_before_reads(self, superinstructions, &dead_zone, &Default::default());
        crate::bitset::BitSet::from_fn(locals, |slot| {
            plain.contains(slot) && (!dead_zone.contains(slot) || initialized.contains(slot))
        })
    }

    pub(super) fn declared_atoms(&self) -> &DeclaredAtomSets {
        self.declared_atom_sets.get_or_init(|| {
            let global_lexical: rustc_hash::FxHashSet<Atom> =
                self.global_lexical_atoms().iter().copied().collect();
            let global_var: rustc_hash::FxHashSet<Atom> =
                self.global_var_atoms().iter().copied().collect();
            let slots = self
                .local_atoms
                .iter()
                .map(|atom| SlotDeclaration {
                    global_var: global_var.contains(atom),
                    global_lexical: global_lexical.contains(atom),
                })
                .collect();
            Box::new(DeclaredAtomSets {
                global_lexical,
                global_var,
                global_immutable: self.global_immutable_atoms().iter().copied().collect(),
                environment: self.environment_atoms.iter().copied().collect(),
                slots,
                local_slots: (self.local_atoms.len() > LOCAL_SLOT_SCAN).then(|| {
                    let mut slots = rustc_hash::FxHashMap::default();
                    for (slot, atom) in self.local_atoms.iter().enumerate().rev() {
                        slots.insert(*atom, slot as u32);
                    }
                    slots
                }),
            })
        })
    }

    /// The first local slot named `atom`: one probe in a function with many locals, where a
    /// scan per global write made a script of N declarations cost N^2.
    pub(crate) fn local_slot(&self, atom: Atom) -> Option<usize> {
        if self.local_atoms.len() <= LOCAL_SLOT_SCAN {
            return self.local_atoms.iter().position(|candidate| *candidate == atom);
        }
        self.declared_atoms().local_slots.as_ref()?.get(&atom).map(|slot| *slot as usize)
    }

    /// The declaration kinds of local `slot`'s name (none for a slot outside the function).
    pub(crate) fn slot_declaration(&self, slot: usize) -> SlotDeclaration {
        let (var, lexical) = (self.global_var_atoms(), self.global_lexical_atoms());
        if var.len() <= LOCAL_SLOT_SCAN && lexical.len() <= LOCAL_SLOT_SCAN {
            return self.local_atoms.get(slot).map_or_else(SlotDeclaration::default, |atom| SlotDeclaration {
                global_var: var.contains(atom),
                global_lexical: lexical.contains(atom),
            });
        }
        self.declared_atoms()
            .slots
            .get(slot)
            .copied()
            .unwrap_or_default()
    }

    /// Whether `atom` is among `global_lexical_atoms`.
    pub(crate) fn declares_global_lexical(&self, atom: Atom) -> bool {
        let atoms: &[Atom] = self.global_lexical_atoms();
        if atoms.len() <= LOCAL_SLOT_SCAN {
            return atoms.contains(&atom);
        }
        self.declared_atoms().global_lexical.contains(&atom)
    }

    /// Whether `atom` is among `global_var_atoms`.
    pub(crate) fn declares_global_var(&self, atom: Atom) -> bool {
        let atoms: &[Atom] = self.global_var_atoms();
        if atoms.len() <= LOCAL_SLOT_SCAN {
            return atoms.contains(&atom);
        }
        self.declared_atoms().global_var.contains(&atom)
    }

    /// Whether `atom` is among `global_immutable_atoms`.
    pub(crate) fn declares_global_immutable(&self, atom: Atom) -> bool {
        let atoms: &[Atom] = self.global_immutable_atoms();
        if atoms.len() <= LOCAL_SLOT_SCAN {
            return atoms.contains(&atom);
        }
        self.declared_atoms().global_immutable.contains(&atom)
    }

    /// Whether `atom` is among `environment_atoms`.
    pub(crate) fn declares_environment_atom(&self, atom: Atom) -> bool {
        let atoms: &[Atom] = &self.environment_atoms[..];
        if atoms.len() <= LOCAL_SLOT_SCAN {
            return atoms.contains(&atom);
        }
        self.declared_atoms().environment.contains(&atom)
    }

    /// Whether anything can read an activation's implicit `new.target` and lexical-this
    /// bindings: by-name resolution or direct eval in the body, closures or cloned scopes
    /// that can capture the activation, `with` scopes, and constructors, initializers,
    /// generators or async bodies whose `this` or activation outlives ordinary dispatch.
    /// Otherwise the frame's own `this` is authoritative and the bindings are never read.
    /// By-name loads of source names resolve only their own atom; the implicit `this` and
    /// `new.target` bindings are reachable by name only through engine-internal atoms.
    /// Whether the activation's implicit `this` and `new.target` bindings can be read: by
    /// its own name lookups (`activation_binding_reads`) or by an arrow function it makes,
    /// which inherits them. Any other closure has its own. `functions` is the owning
    /// program's table.
    pub(crate) fn activation_bindings_observable(&self, tables: &impl FactTables) -> bool {
        *self.activation_bindings.get_or_init(|| self.derive_activation_bindings_observable(tables))
    }

    pub(super) fn derive_activation_bindings_observable(&self, tables: &impl FactTables) -> bool {
        self.activation_binding_reads(tables)
            || self.instructions().any(|instruction| {
                instruction.op() == Op::MakeClosure
                    && tables
                        .function_is_arrow(instruction.closure_function_index() as usize)
                        .is_none_or(|is_arrow| is_arrow)
            })
    }

    /// Whether the activation itself may read its implicit bindings through a name or scope
    /// operation, or must keep them for its kind of activation.
    pub(super) fn activation_binding_reads(&self, tables: &impl FactTables) -> bool {
        self.is_class_constructor || self.binding_reads_beside_class(tables)
    }

    /// `activation_binding_reads` but for a class constructor's own kind: a base class
    /// constructor's body that reaches none of its implicit bindings (`new.target`, `super`
    /// and the like read hidden names) needs none to run frameless when constructed.
    pub(super) fn binding_reads_beside_class(&self, tables: &impl FactTables) -> bool {
        self.is_async || self.is_generator || self.body_binding_reads(tables)
    }

    /// `binding_reads_beside_class` but for a suspendable kind's own: what the body itself
    /// reads. A non-suspending async body reaching none runs as an ordinary one would
    /// (`CallPlan::settles_in_loop`), its promise held by its call.
    pub(super) fn body_binding_reads(&self, tables: &impl FactTables) -> bool {
        self.body_binding_reads_beside_clones(tables)
            || self.instructions().any(|instruction| instruction.op() == Op::CloneEnv)
    }

    /// `body_binding_reads` but for the iteration copies of its environment (`CloneEnv`),
    /// which share the implicit bindings the environment holds rather than read them: an
    /// environment activation's window keeps those its closures may observe in the
    /// environment it makes (`window_environment`), which a copy shares as a frame's does.
    pub(super) fn body_binding_reads_beside_clones(&self, tables: &impl FactTables) -> bool {
        self.evals_in_scope() || self.reads_implicit_bindings(tables)
    }

    pub fn global_lexical_atoms(&self) -> &[Atom] {
        self.global_declarations_or_none().map_or(&[], |declarations| &declarations.lexical)
    }

    pub fn global_var_atoms(&self) -> &[Atom] {
        self.global_declarations_or_none().map_or(&[], |declarations| &declarations.var)
    }

    pub fn global_function_atoms(&self) -> &[Atom] {
        self.global_declarations_or_none().map_or(&[], |declarations| &declarations.function)
    }

    pub fn global_annex_b_var_atoms(&self) -> &[Atom] {
        self.global_declarations_or_none().map_or(&[], |declarations| &declarations.annex_b_var)
    }

    pub fn global_immutable_atoms(&self) -> &[Atom] {
        self.global_declarations_or_none().map_or(&[], |declarations| &declarations.immutable)
    }

    /// Parameters an activation binds into frame locals.
    pub(crate) const fn local_parameter_count(&self) -> u16 {
        if self.parameter_registers.is_some() {
            0
        } else {
            self.params
        }
    }

    /// The function's `CallPlan`, derived on first use.
    #[inline(always)] pub(crate) fn call_plan(&self, tables: &impl FactTables) -> CallPlan {
        CallPlan::from_bits(u32::from(*self.call_plan.get_or_init(|| u16::try_from(self.derive_call_plan(tables).to_bits()).expect("the plan's flags fit 16 bits"))))
    }

    /// The function's `initializer_stores`, derived on first use.
    #[inline(always)] pub(crate) fn initializer_stores(&self, constants: &[Constant]) -> Option<&[InitializerStore]> {
        self.initializer
            .get_or_init(|| self.derive_initializer_stores(constants).map(Into::into))
            .as_deref()
    }

    /// The memoized `plain_local_slots` projection for this residual's atoms.
    #[inline(always)] pub(crate) fn plain_local(&self, tables: &impl FactTables, slot: usize) -> bool {
        let words =
            self.plain_locals.get_or_init(|| self.plain_local_slots(tables.superinstructions()).words().to_vec().into());
        crate::bitset::words_contain(words, slot)
    }
}
