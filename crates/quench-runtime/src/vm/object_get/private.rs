//! Private names on objects: reading, writing and initializing private fields, methods and accessors,
//! the brand checks, and the home object a private name resolves to.

use super::super::*;
use crate::heap::PrivateAtom;
use super::PropertyKey;

/// Where a private name's home was found walking the environment chain.
enum ChainHome {
    /// Bound for the name by an eval: not memoized.
    Bound(Value),
    /// The first home object declaring the name: memoized with the chain.
    Found(Value),
}

impl<H: Host> Vm<H> {
    /// The value of private field `atom` of `target` under the running frame's home for the
    /// name. A target without the field, or whose element is a method or accessor, answers
    /// `None`, leaving the access to the general path, which throws or calls it.
    pub(in crate::vm) fn private_field_value(&mut self, p: &ResidualProgram, target: Value, atom: Atom) -> Option<Value> {
        let atom = self.private_name_atom(p, atom)?;
        self.private_field_in(target, atom)
    }

    /// The private atom of `atom` that function `function` of the running program, whose
    /// frame would capture from `environment`, names when it has no binding sites: the atom of
    /// the home a run of it with a frame found (`private_name_home`'s memo), else of the
    /// initialized home binding its static name binding reads from `environment`
    /// (`direct_capture`'s read), as an arrow field's closure of each new instance finds its
    /// class's. `None` leaves the access to the general path, which finds the home.
    pub(in crate::vm) fn memoized_private_atom(
        &self,
        p: &ResidualProgram,
        environment: Value,
        function: u32,
        atom: Atom,
    ) -> Option<PrivateAtom> {
        let body = p.function_get(function as usize).filter(|body| body.binding_sites.is_empty())?;
        let key = PrivateHomeKey { environment, program: self.active_program, function, name: atom };
        if let Some(home) = self.private_homes.get(&key) {
            return self.home_private_atom(*home, atom);
        }
        let binding = body
            .name_bindings
            .get(atom)
            .filter(|binding| binding.kind == crate::bytecode::LexicalBindingKind::PrivateName)?;
        let crate::bytecode::EvalBindingLocation::Capture { depth, slot } = binding.location else {
            return None;
        };
        let (slots, root) = self.environment_capture_slots(environment, false, depth)?;
        self.home_private_atom(self.owned_capture(slots, root, slot)?, atom)
    }

    /// The private method or accessor that `object`, which carries private atom `atom` of the
    /// name `name`, reads at lane site `site`: the element the site recorded under the atom
    /// (`PrivateSiteHomes::element`), else the one `[[Get]]`'s walk of the object's chain meets,
    /// recorded for the next read. `None` leaves the read to the general path: a missing getter,
    /// a proxy on the chain, or a site without a recorded atom.
    pub(in crate::vm) fn site_private_element(
        &self,
        site: usize,
        (atom, name): (PrivateAtom, Atom),
        object: Value,
    ) -> Option<super::private_site_home::PrivateElement> {
        use super::private_site_home::PrivateElement;
        if let Some(element) = self.private_site_homes.element(site, atom) {
            return Some(element);
        }
        let key = PropertyKey::string(name);
        let mut link = object;
        let element = loop {
            if matches!(self.heap.get(link), Some(Cell::Proxy { .. })) {
                return None;
            }
            if let Some(attributes) = self.property_attributes(link, key) {
                break if attributes.accessor {
                    PrivateElement::Getter(attributes.getter?)
                } else {
                    PrivateElement::Method(self.own_property(link, name)?)
                };
            }
            link = self.object_data(link)?.proto();
            if link.is_null() {
                return None;
            }
        };
        self.private_site_homes.record_element(site, atom, element);
        Some(element)
    }

    /// `private_field_value` through `memoized_private_atom`.
    pub(in crate::vm) fn memoized_private_field_value(
        &self,
        p: &ResidualProgram,
        environment: Value,
        function: u32,
        target: Value,
        atom: Atom,
    ) -> Option<Value> {
        let atom = self.memoized_private_atom(p, environment, function, atom)?;
        self.private_field_in(target, atom)
    }

    /// The store of private field `atom` of `target` under the running frame's home for the
    /// name: `false` for a target without the field, or whose element is a method or
    /// accessor, which the general path throws for or calls.
    pub(in crate::vm) fn set_private_field_value(&mut self, p: &ResidualProgram, target: Value, atom: Atom, value: Value) -> bool {
        let Some(atom) = self.private_name_atom(p, atom) else { return false; };
        self.set_private_field_in(target, atom, value)
    }

    /// Whether `object` may gain a private element: always, unless the host applies
    /// non-extensibility to private elements (`Host::nonextensible_applies_to_private`).
    pub(in crate::vm) fn private_element_addable(&self, object: &Object) -> bool {
        object.is_extensible() || !self.host.nonextensible_applies_to_private()
    }

    /// `DefinePrivateField`: `target` gains private field `name` of the class evaluation `home`
    /// declared, a shape transition by its private atom with `value` at the new slot.
    pub(in crate::vm) fn initialize_private_field(&mut self, p: &ResidualProgram, target: Value, home: Value, atom: Atom, value: Value) -> Result<(), JsError> {
        let Some(addable) = self.object_data(target).map(|object| self.private_element_addable(object)) else {
            return Err(self.type_error(p, "Cannot add private field to a non-object".into()));
        };
        if !addable {
            return Err(self.type_error(p, "Cannot add private field to a non-extensible object".into()));
        }
        let private = self.home_private_atom(home, atom).ok_or_else(|| JsError("private name is not declared by its class".into()))?;
        if !self.has_private_slot(target, private) {
            self.add_private_slot(target, private, value, true);
            return Ok(());
        }
        let name = crate::bytecode::private_identity_label(self.atom_name(atom)).unwrap_or_else(|| self.atom_name(atom)).to_owned();
        Err(self.type_error(p, format!("Cannot initialize #{name} twice on the same object")))
    }

    /// `MarkPrivateName`: brands `object` with private method name `atom` of the class home
    /// `home` (PrivateMethodOrAccessorAdd), which raises for a non-extensible object or one
    /// already branded; the class itself, as its own home, takes its static methods' brand.
    pub(in crate::vm) fn mark_private_name(&mut self, p: &ResidualProgram, object: Value, home: Value, atom: Atom) -> Result<(), JsError> {
        let private = self.home_private_atom(home, atom).ok_or_else(|| JsError("private name is not declared by its class".into()))?;
        if object != home {
            let Some((addable, branded)) = self.object_data(object).map(|data| (self.private_element_addable(data), self.has_private_slot(object, private))) else {
                return Err(self.type_error(p, "Cannot add private method to a non-extensible object".into()));
            };
            if !addable {
                return Err(self.type_error(p, "Cannot add private method to a non-extensible object".into()));
            }
            if branded {
                return Err(self.type_error(p, "Cannot add private method to an object that already has it".into()));
            }
        }
        if !self.has_private_slot(object, private) && self.object_data(object).is_some() {
            self.add_private_slot(object, private, Value::UNDEFINED, false);
        }
        Ok(())
    }

    pub(in crate::vm) fn check_private_brand(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        atom: Atom,
    ) -> Result<(), JsError> {
        if self.has_private_brand(p, target, atom) {
            return Ok(());
        }
        Err(self.type_error(p, "private member is not present on this object".into()))
    }

    /// Report a failed private read with V8/Node's public error wording. Keep
    /// `check_private_brand` unchanged for writes and brand checks whose error
    /// semantics are distinct from reading a private member.
    pub(in crate::vm) fn check_private_brand_for_read(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        atom: Atom,
    ) -> Result<(), JsError> {
        if self.has_private_brand(p, target, atom) {
            return Ok(());
        }
        Err(self.undeclared_private_read_error(p, atom))
    }

    /// V8/Node's TypeError for reading private member `atom` from an object without its brand.
    fn undeclared_private_read_error(&mut self, p: &ResidualProgram, atom: Atom) -> JsError {
        let name = self.atom_name(atom);
        let member = if name.starts_with('#') {
            name.to_owned()
        } else {
            format!("#{name}")
        };
        self.type_error(
            p,
            format!(
                "Cannot read private member {member} from an object whose class did not declare it"
            ),
        )
    }

    pub(in crate::vm) fn has_private_brand(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        atom: Atom,
    ) -> bool {
        self.private_brand_home(p, target, atom).is_some()
    }

    /// The home object of `target`'s private name `atom` as seen from the running frame,
    /// if `target` carries it.
    pub(in crate::vm) fn private_brand_home(
        &mut self,
        p: &ResidualProgram,
        target: Value,
        atom: Atom,
    ) -> Option<Value> {
        let home = self.private_name_home(p, atom)?;
        let private = self.home_private_atom(home, atom)?;
        self.has_private_slot(target, private).then_some(home)
    }

    /// The private atom of `atom` as seen from the running frame: the one the home that declares
    /// the name (`private_name_home`, the private environment chain, eval-introduced bindings
    /// included) made for its class evaluation.
    pub(in crate::vm) fn private_name_atom(&mut self, p: &ResidualProgram, atom: Atom) -> Option<PrivateAtom> {
        let home = self.private_name_home(p, atom)?;
        self.home_private_atom(home, atom)
    }

    /// The home object that declares private name `atom` for the running frame: the nearest
    /// eval-introduced private binding, otherwise the first visible class home declaring the
    /// name. It depends on the frame's function and environment chain, never on the target,
    /// so a found home is memoized while that environment lives.
    pub(in crate::vm) fn private_name_home(&mut self, p: &ResidualProgram, atom: Atom) -> Option<Value> {
        let frame_index = self.frames.len().checked_sub(1)?;
        let start = self.captured_parent_environment(self.frame_caller(frame_index));
        let (key, static_bindings) = self.private_home_key(p, frame_index, start, atom);
        if static_bindings && let Some(home) = self.private_homes.get(&key) {
            return Some(*home);
        }
        if let Some(binding) = self.name_binding(self.frame_caller(frame_index), atom)
            && binding.kind == crate::bytecode::LexicalBindingKind::PrivateName
        {
            let home = self.load_name_binding(p, VarOwner::Frame(frame_index), binding).ok();
            if static_bindings && let Some(home) = home {
                self.private_homes.insert(key, home);
            }
            return home;
        }
        if let Some(home) = self.private_homes.get(&key) {
            return Some(*home);
        }
        let home = match self.private_home_in_chain(p, (frame_index, start), atom)? {
            ChainHome::Bound(home) => return Some(home),
            ChainHome::Found(home) => home,
        };
        // Eval-introduced private bindings live only in environments made for that eval, so
        // a cached chain never gains a nearer one.
        self.private_homes.insert(key, home);
        Some(home)
    }

    /// The memo key of private name `atom`'s home for the frame `frame_index` whose captured
    /// environment is `start`, and whether the name bindings resolve it from the key alone: without
    /// binding sites no eval-introduced private binding is in reach, so they resolve the same
    /// initialized home whenever the closure's environment chain is the same.
    fn private_home_key(&self, p: &ResidualProgram, frame_index: usize, start: Value, atom: Atom) -> (PrivateHomeKey, bool) {
        let key = PrivateHomeKey {
            environment: start,
            program: self.frames[frame_index].program,
            function: self.frames[frame_index].function,
            name: atom,
        };
        let static_bindings = p
            .function_get(key.function as usize)
            .is_some_and(|function| function.binding_sites.is_empty());
        (key, static_bindings)
    }

    /// The home of private name `atom` found by walking the environment chain from `start`: a home
    /// an eval bound for the name, else the first home object declaring it among those the
    /// running function's lexical homes name (`ChainHome`).
    fn private_home_in_chain(&mut self, p: &ResidualProgram, (frame_index, start): (usize, Value), atom: Atom) -> Option<ChainHome> {
        let private_home_binding = self.private_home_binding_atom(atom);
        let home_atoms = self.super_home_atoms(p, self.frames[frame_index].function);
        let mut homes = Vec::with_capacity(home_atoms.len());
        let mut environment = start;
        while let Some(Cell::Environment { parent, function, slots, scope }) = self.heap.get(environment) {
            let dynamic_bindings = self.heap.environment_bindings(environment)?;
            if let Some((_, home)) = dynamic_bindings.iter().rev().find(|(binding, _)| *binding == private_home_binding) {
                return Some(ChainHome::Bound(*home));
            }
            if *function != u32::MAX {
                self.collect_environment_homes(p, environment, (*function, slots.len(), scope.program()), &home_atoms, &mut homes)?;
            }
            environment = *parent;
        }
        homes.into_iter().map(|(_, home)| home).find(|home| self.home_private_atom(*home, atom).is_some()).map(ChainHome::Found)
    }

    /// The home bindings of the functions from `function` outward, each once.
    fn super_home_atoms(&self, p: &ResidualProgram, function: u32) -> Vec<Atom> {
        let mut function = Some(function);
        let mut home_atoms = Vec::new();
        while let Some(id) = function
            && let Some(metadata) = p.function_get(id as usize)
        {
            if let Some(atom) = metadata.super_home_atom
                && !home_atoms.contains(&atom)
            {
                home_atoms.push(atom);
            }
            function = metadata.parent;
        }
        home_atoms
    }

    /// Adds to `homes` the value of each home binding of `home_atoms` and of the function's own
    /// locals that `environment`'s slots hold and `homes` lacks. `function`, the slot count and the
    /// program are the environment's own.
    fn collect_environment_homes(
        &self,
        p: &ResidualProgram,
        environment: Value,
        (function, slot_count, program): (u32, usize, Option<u32>),
        home_atoms: &[Atom],
        homes: &mut Vec<(Atom, Value)>,
    ) -> Option<()> {
        let owner = program.and_then(|program| self.programs.get(super::super::program_store::ProgramId::from_raw(program)));
        let owner = owner.as_deref().unwrap_or(p);
        let visible_homes = owner
            .function_get(function as usize)
            .into_iter()
            .flat_map(|metadata| metadata.local_atoms.iter().copied())
            .filter(|atom| self.atom_name(*atom).starts_with(crate::bytecode::HOME_BINDING_PREFIX))
            .collect::<Vec<_>>();
        for atom in home_atoms.iter().chain(&visible_homes) {
            if homes.iter().any(|(candidate, _)| candidate == atom) {
                continue;
            }
            if let Some(slot) = self.local_binding_slot(owner, function, *atom).filter(|slot| *slot < slot_count) {
                homes.push((*atom, self.heap.environment_slot(environment, slot)?));
            }
        }
        Some(())
    }

    pub(in crate::vm) fn get_private_proxy_field(
        &mut self,
        p: &ResidualProgram,
        proxy: Value,
        atom: Atom,
    ) -> Result<Value, JsError> {
        if let Some(value) = self.private_field_value(p, proxy, atom) {
            return Ok(value);
        }
        let Some(home) = self.private_brand_home(p, proxy, atom) else {
            return Err(self.undeclared_private_read_error(p, atom));
        };
        if let Some(value) = self.own_property(proxy, atom) {
            return Ok(value);
        }
        if let Some(attributes) = self.property_accessor(home, atom) {
            let Some(getter) = attributes.getter else {
                return Err(
                    self.type_error(p, "private member is not present on this object".into())
                );
            };
            return self.call_value(p, getter, proxy, &[]);
        }
        // Methods live on the home object, which does not carry its own brand.
        self.own_property(home, atom).ok_or_else(|| {
            self.type_error(p, "private member is not present on this object".into())
        })
    }
}
