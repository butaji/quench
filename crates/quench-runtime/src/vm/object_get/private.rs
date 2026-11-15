//! Private names and brands: the home of a class element, the brand a target carries, and the
//! reads and writes that check them.

use super::*;

impl<H: Host> Vm<H> {
    /// The value of private field `atom` of `target` under the running frame's home for the
    /// name. A target without the field, or whose element is a method or accessor, answers
    /// `None`, leaving the access to the general path, which throws or calls it.
    pub(in crate::vm) fn private_field_value(&mut self, p: &ResidualProgram, target: Value, atom: Atom) -> Option<Value> {
        let home = self.private_name_home(p, atom)?;
        self.private_field(target, PrivateBrand { home, name: atom })
    }

    /// The private brand of `atom` that function `function` of the running program, whose
    /// frame would capture from `environment`, names when it has no binding sites: the home a
    /// run of it with a frame found (`private_name_home`'s memo), else the initialized home
    /// binding its static name binding reads from `environment` (`direct_capture`'s read), as
    /// an arrow field's closure of each new instance finds its class's. `None` leaves the
    /// access to the general path, which finds the home.
    pub(in crate::vm) fn memoized_private_brand(
        &self,
        p: &ResidualProgram,
        environment: Value,
        function: u32,
        atom: Atom,
    ) -> Option<PrivateBrand> {
        let body = p.function_get(function as usize).filter(|body| body.binding_sites.is_empty())?;
        let key = PrivateHomeKey { environment, program: self.active_program, function, name: atom };
        if let Some(home) = self.private_homes.get(&key) {
            return Some(PrivateBrand { home: *home, name: atom });
        }
        let binding = body
            .name_bindings
            .get(atom)
            .filter(|binding| binding.kind == crate::bytecode::LexicalBindingKind::PrivateName)?;
        let crate::bytecode::EvalBindingLocation::Capture { depth, slot } = binding.location else {
            return None;
        };
        let (slots, root) = self.environment_capture_slots(environment, false, depth)?;
        Some(PrivateBrand { home: self.owned_capture(slots, root, slot)?, name: atom })
    }

    /// The private method or accessor that `object`, which carries `brand`, reads at lane site
    /// `site`: the element the site recorded under the brand's home (`PrivateSiteHomes::element`), else the one `[[Get]]`'s walk of the
    /// object's chain meets, recorded for the next read. `None` leaves the read to the general
    /// path: a missing getter, a proxy on the chain, or a site without a recorded home.
    pub(in crate::vm) fn site_private_element(
        &self,
        site: usize,
        brand: PrivateBrand,
        object: Value,
    ) -> Option<super::private_site_home::PrivateElement> {
        use super::private_site_home::PrivateElement;
        if let Some(element) = self.private_site_homes.element(site, brand.home) {
            return Some(element);
        }
        let key = PropertyKey::string(brand.name);
        let mut link = object;
        let element = loop {
            if matches!(self.heap.get(link), Some(Cell::Proxy { .. })) {
                return None;
            }
            if let Some(attributes) = self.property_attributes(link, key) {
                break if attributes.accessor {
                    PrivateElement::Getter(attributes.getter?)
                } else {
                    PrivateElement::Method(self.own_property(link, brand.name)?)
                };
            }
            link = self.object_data(link)?.proto();
            if link.is_null() {
                return None;
            }
        };
        self.private_site_homes.record_element(site, brand.home, element);
        Some(element)
    }

    /// `private_field_value` through `memoized_private_brand`.
    pub(in crate::vm) fn memoized_private_field_value(
        &self,
        p: &ResidualProgram,
        environment: Value,
        function: u32,
        target: Value,
        atom: Atom,
    ) -> Option<Value> {
        let brand = self.memoized_private_brand(p, environment, function, atom)?;
        self.private_field(target, brand)
    }

    /// The store of private field `atom` of `target` under the running frame's home for the
    /// name: `false` for a target without the field, or whose element is a method or
    /// accessor, which the general path throws for or calls.
    pub(in crate::vm) fn set_private_field_value(&mut self, p: &ResidualProgram, target: Value, atom: Atom, value: Value) -> bool {
        let Some(home) = self.private_name_home(p, atom) else { return false; };
        self.set_private_field(target, PrivateBrand { home, name: atom }, value)
    }

    /// Whether `object` may gain a private element: always, unless the host applies
    /// non-extensibility to private elements (`Host::nonextensible_applies_to_private`).
    pub(in crate::vm) fn private_element_addable(&self, object: &Object) -> bool {
        object.is_extensible() || !self.host.nonextensible_applies_to_private()
    }

    pub(in crate::vm) fn initialize_private_field(&mut self, p: &ResidualProgram, target: Value, home: Value, atom: Atom, value: Value) -> Result<(), JsError> {
        let Some(addable) = self.object_data(target).map(|object| self.private_element_addable(object)) else {
            return Err(self.type_error(p, "Cannot add private field to a non-object".into()));
        };
        if !addable {
            return Err(self.type_error(p, "Cannot add private field to a non-extensible object".into()));
        }
        if self.add_private_field(target, PrivateBrand { home, name: atom }, value) {
            return Ok(());
        }
        let name = crate::bytecode::private_identity_label(self.atom_name(atom)).unwrap_or_else(|| self.atom_name(atom)).to_owned();
        Err(self.type_error(p, format!("Cannot initialize #{name} twice on the same object")))
    }

    /// `MarkPrivateName`: brands `object` with private method name `atom` of the class home
    /// `home` (PrivateMethodOrAccessorAdd), which raises for a non-extensible object or one
    /// already branded; the class itself, as its own home, takes its static methods' brand.
    pub(in crate::vm) fn mark_private_name(&mut self, p: &ResidualProgram, object: Value, home: Value, atom: Atom) -> Result<(), JsError> {
        let brand = PrivateBrand { home, name: atom };
        if object != home {
            let Some((addable, branded)) = self.object_data(object).map(|data| (self.private_element_addable(data), self.has_private_name(object, brand))) else {
                return Err(self.type_error(p, "Cannot add private method to a non-extensible object".into()));
            };
            if !addable {
                return Err(self.type_error(p, "Cannot add private method to a non-extensible object".into()));
            }
            if branded {
                return Err(self.type_error(p, "Cannot add private method to an object that already has it".into()));
            }
        }
        self.add_private_name(object, brand);
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
    pub(super) fn undeclared_private_read_error(&mut self, p: &ResidualProgram, atom: Atom) -> JsError {
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
        self.has_private_name(target, PrivateBrand { home, name: atom }).then_some(home)
    }

    /// The home object that declares private name `atom` for the running frame: the nearest
    /// eval-introduced private binding, otherwise the first visible class home declaring the
    /// name. It depends on the frame's function and environment chain, never on the target,
    /// so a found home is memoized while that environment lives.
    pub(in crate::vm) fn private_name_home(&mut self, p: &ResidualProgram, atom: Atom) -> Option<Value> {
        let frame_index = self.frames.len().checked_sub(1)?;
        let owner = VarOwner::Frame(frame_index);
        let start = self.captured_parent_environment(self.owner_view(owner));
        let key = PrivateHomeKey {
            environment: start,
            program: self.frames[frame_index].program,
            function: self.frames[frame_index].function,
            name: atom,
        };
        // Without binding sites no eval-introduced private binding is in reach, so the name
        // bindings resolve the same initialized home whenever the closure's environment chain
        // is the same.
        let static_bindings = p
            .function_get(key.function as usize)
            .is_some_and(|function| function.binding_sites.is_empty());
        if let Some(answer) = self.bound_or_cached_home(p, owner, atom, key, static_bindings) {
            return answer;
        }
        let homes = match self.scan_private_homes(p, frame_index, start, atom)? {
            ControlFlow::Break(home) => return Some(home),
            ControlFlow::Continue(homes) => homes,
        };
        let home = homes
            .into_iter()
            .map(|(_, home)| home)
            .find(|home| self.has_private_name(*home, PrivateBrand { home: *home, name: atom }))?;
        // Eval-introduced private bindings live only in environments made for that eval, so
        // a cached chain never gains a nearer one.
        self.private_homes.insert(key, home);
        Some(home)
    }

    /// The home a frame's own name binding for `atom` holds, or a cached home for `key`: `Some`
    /// ends the lookup with that answer, `None` leaves it to the environment chain. A binding's
    /// home is cached when `static_bindings` holds, as the cache key says it may be.
    fn bound_or_cached_home(
        &mut self,
        p: &ResidualProgram,
        owner: VarOwner,
        atom: Atom,
        key: PrivateHomeKey,
        static_bindings: bool,
    ) -> Option<Option<Value>> {
        if static_bindings && let Some(home) = self.private_homes.get(&key) {
            return Some(Some(*home));
        }
        if let Some(binding) = self.name_binding(self.owner_view(owner), atom)
            && binding.kind == crate::bytecode::LexicalBindingKind::PrivateName
        {
            let home = self.load_name_binding(p, owner, binding).ok();
            if static_bindings && let Some(home) = home {
                self.private_homes.insert(key, home);
            }
            return Some(home);
        }
        self.private_homes.get(&key).map(|home| Some(*home))
    }

    /// Walks the environment chain from `start` for the frame's private name `atom`: the home
    /// an eval-introduced binding holds (`Break`), else each class home the chain's functions
    /// declare for the name, nearest first (`Continue`). `None` when the heap cannot answer.
    fn scan_private_homes(
        &mut self,
        p: &ResidualProgram,
        frame_index: usize,
        start: Value,
        atom: Atom,
    ) -> Option<ControlFlow<Value, Vec<(Atom, Value)>>> {
        let private_home_binding = self.private_home_binding_atom(atom);
        let home_atoms = super_home_atoms(p, self.frames[frame_index].function);
        let mut environment = start;
        let mut homes = Vec::with_capacity(home_atoms.len());
        while let Some(Cell::Environment {
            parent,
            function,
            slots,
            scope,
        }) = self.heap.get(environment)
        {
            let dynamic_bindings = self.heap.environment_bindings(environment)?;
            if let Some((_, home)) = dynamic_bindings
                .iter()
                .rev()
                .find(|(binding, _)| *binding == private_home_binding)
            {
                return Some(ControlFlow::Break(*home));
            }
            if *function != u32::MAX {
                self.environment_homes(p, environment, scope.program(), *function, slots.len(), &home_atoms, &mut homes)?;
            }
            environment = *parent;
        }
        Some(ControlFlow::Continue(homes))
    }

    /// The homes an environment's function declares for `home_atoms` and its own home bindings,
    /// appended to `homes` once per atom, the first one found.
    fn environment_homes(
        &self,
        p: &ResidualProgram,
        environment: Value,
        program: Option<u32>,
        function: u32,
        slot_count: usize,
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
            if let Some(slot) = self
                .local_binding_slot(owner, function, *atom)
                .filter(|slot| *slot < slot_count)
            {
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

/// The `super` home atoms of a function and its enclosing functions, each once, innermost first.
fn super_home_atoms(p: &ResidualProgram, function: u32) -> Vec<Atom> {
    let mut next = Some(function);
    let mut home_atoms = Vec::new();
    while let Some(id) = next
        && let Some(metadata) = p.function_get(id as usize)
    {
        if let Some(atom) = metadata.super_home_atom
            && !home_atoms.contains(&atom)
        {
            home_atoms.push(atom);
        }
        next = metadata.parent;
    }
    home_atoms
}
