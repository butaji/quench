//! Lazy data properties: host-defined data properties whose values one initializer computes
//! when script first observes any of them, the way V8's lazy data properties let Node expose
//! its web globals without building them at startup.
//!
//! Until its group materializes, a lazy property is an accessor whose getter and setter are
//! one `Native::LazyDataProperty` function; that function's identity is the mark. Reading the
//! property materializes its group and returns the value, assignment replaces it with a data
//! property holding the assigned value, and the reflective operations (`[[GetOwnProperty]]`,
//! `[[DefineOwnProperty]]`) materialize it first, so script only ever observes a writable
//! data property at the position the host defined it.
//!
//! The initializer is host bootstrap code: it runs with the startup intrinsics in place and
//! defines the group's members, which materialization has first turned into writable
//! `undefined` data properties with their enumerable and configurable attributes.
//!
//! A member may instead be one the initializer defines itself, with its own attributes and
//! at its own position (`LazyMember::redefined`): materialization removes it before the
//! initializer runs, so a holder the initializer builds property by property (a prototype
//! a library fills in) ends up exactly as the library leaves it. Script's writes and deletes
//! of such a group's members materialize it first, as for watched members below.
//!
//! A binding may stand for a member that is still pending, such as a builtin module's ESM
//! export (Node's facades bind each export's value when the facade is created). The binding
//! then holds the member's accessor function, which script never observes otherwise
//! (`resolve_pending_binding` reads it as the member), and the member keeps a listener that
//! gives the binding the member's value when its group materializes. While a member has
//! listeners, script's writes and deletes materialize the group first, so the binding keeps
//! the value the member had, as Node's snapshot does.

use super::object_descriptors::PropertyDescriptorRecord;
use super::*;

/// The slots of the environment one group's members share.
#[repr(usize)]
enum GroupSlot {
    Holder,
    Initializer,
    State,
    /// Whether the initializer adds own keys beside the members (`Value::TRUE`), so the
    /// holder's key lists are complete only once the group has materialized.
    CompletesKeys,
    /// Whether some member is redefined by the initializer (`Value::TRUE`), so script's
    /// writes and deletes materialize the group first.
    Redefines,
}

impl GroupSlot {
    const COUNT: usize = Self::Redefines as usize + 1;
}

/// The slots of one member's own environment.
#[repr(usize)]
enum MemberSlot {
    Group,
    Key,
    /// The bindings standing for the member while it is pending: an array of
    /// `listener, index` pairs, each called with `index` and the member's value when its
    /// group materializes (`UNDEFINED` while there are none).
    Listeners,
    /// `Value::TRUE` when the initializer defines the member itself
    /// (`LazyMember::redefined`).
    Redefined,
}

impl MemberSlot {
    const COUNT: usize = Self::Redefined as usize + 1;
}

/// One member of a lazy group: its key (a string or symbol), whether it is enumerable while
/// pending, and whether the initializer defines it itself rather than assigning the data
/// property materialization leaves in its place.
#[derive(Clone, Copy)]
pub(crate) struct LazyMember {
    pub(crate) key: Value,
    pub(crate) enumerable: bool,
    pub(crate) redefined: bool,
}

/// A group's lifecycle: the initializer runs at most once.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum GroupState {
    Pending,
    Running,
    Done,
}

impl GroupState {
    fn value(self) -> Value {
        Value::number(f64::from(self as u8))
    }

    fn of(value: Value) -> Self {
        match value.as_number() {
            Some(state) if state == f64::from(Self::Pending as u8) => Self::Pending,
            Some(state) if state == f64::from(Self::Running as u8) => Self::Running,
            _ => Self::Done,
        }
    }
}

impl<H: Host> Vm<H> {
    /// Defines each of `members` on `holder` as one lazy group of configurable properties
    /// that `initializer` defines when any of them is observed. With `completes_keys`, the
    /// initializer also adds keys the members do not name, and listing the holder's keys
    /// materializes the group first (`complete_lazy_keys`).
    pub(crate) fn define_lazy_data_properties(
        &mut self,
        p: &ResidualProgram,
        holder: Value,
        members: &[LazyMember],
        completes_keys: bool,
        initializer: Value,
    ) -> Result<(), JsError> {
        let flag = |set: bool| if set { Value::TRUE } else { Value::FALSE };
        let mut slots = vec![Value::UNDEFINED; GroupSlot::COUNT];
        slots[GroupSlot::Holder as usize] = holder;
        slots[GroupSlot::Initializer as usize] = initializer;
        slots[GroupSlot::State as usize] = GroupState::Pending.value();
        slots[GroupSlot::CompletesKeys as usize] = flag(completes_keys);
        slots[GroupSlot::Redefines as usize] = flag(members.iter().any(|member| member.redefined));
        let group = self.native_environment(slots);
        self.with_call_roots([group], |vm| {
            for &LazyMember {
                key,
                enumerable,
                redefined,
            } in members
            {
                let mut slots = vec![Value::UNDEFINED; MemberSlot::COUNT];
                slots[MemberSlot::Group as usize] = group;
                slots[MemberSlot::Key as usize] = key;
                slots[MemberSlot::Redefined as usize] = flag(redefined);
                vm.lazy_member_key(key)?;
                let member = vm.native_environment(slots);
                let accessor = vm.native_with_env(Native::LazyDataProperty, member);
                let record = PropertyDescriptorRecord {
                    value: None,
                    writable: None,
                    enumerable: Some(enumerable),
                    configurable: Some(true),
                    getter: Some(accessor),
                    setter: Some(accessor),
                };
                if !vm.define_own_property_record(p, holder, key, record)? {
                    return Err(vm.type_error(p, "cannot define lazy data property".into()));
                }
            }
            Ok(())
        })
    }

    /// The accessor of `holder`'s own property `key` while it is still a lazy data property.
    fn lazy_data_accessor(&self, holder: Value, key: PropertyKey) -> Option<Value> {
        // A lazy data property's accessor is its getter and its setter, so a holder whose
        // shape holds no such accessor, and no indexed descriptor, has none.
        if let Some(object) = self.object_data(holder)
            && !object.has_indexed_descriptors()
            && !self.shape_may_have_table_accessors(object.shape())
        {
            return None;
        }
        let attributes = self.property_attributes(holder, key)?;
        let getter = attributes.getter.filter(|_| attributes.accessor)?;
        self.lazy_member(getter).map(|_| getter)
    }

    /// Whether `holder`'s own property `key` is still a lazy data property: its group has not
    /// materialized and script has not replaced it.
    pub(super) fn is_pending_lazy_data_property(&self, holder: Value, key: PropertyKey) -> bool {
        self.lazy_data_accessor(holder, key).is_some()
    }

    /// The member environment of `value` when it is a lazy data property's accessor.
    pub(super) fn lazy_member(&self, value: Value) -> Option<Value> {
        match self.heap.function(value) {
            Some(FunctionRecord {
                kind: FunctionKind::Native(Native::LazyDataProperty),
                env,
                ..
            }) => Some(*env),
            _ => None,
        }
    }

    fn member_slot(&self, member: Value, slot: MemberSlot) -> Value {
        self.heap
            .environment_slot(member, slot as usize)
            .unwrap_or(Value::UNDEFINED)
    }

    /// A member's key as a property key: its string's atom, or the symbol itself.
    fn lazy_member_key(&mut self, key: Value) -> Result<PropertyKey, JsError> {
        match self.heap.get(key).cloned() {
            _ if let Some(atom) = self.string_key_atom(key) => Ok(PropertyKey::string(atom)),
            Some(Cell::Symbol(_)) => Ok(PropertyKey::symbol(key)),
            _ => Err(JsError(
                "lazy data property key is a string or symbol".into(),
            )),
        }
    }

    fn group_slot(&self, group: Value, slot: GroupSlot) -> Value {
        self.heap
            .environment_slot(group, slot as usize)
            .unwrap_or(Value::UNDEFINED)
    }

    /// `[[Get]]` of `key` on `holder` with `receiver`, for either kind of member key.
    fn get_lazy_key(
        &mut self,
        p: &ResidualProgram,
        holder: Value,
        key: PropertyKey,
        receiver: Value,
    ) -> Result<Value, JsError> {
        match key {
            PropertyKey::String(atom) => self.get_property_with_receiver(p, holder, atom, receiver),
            PropertyKey::Symbol(symbol) => {
                self.get_symbol_property_with_receiver(p, holder, symbol, receiver)
            }
            PropertyKey::Private(_) => Err(JsError("lazy data property key is not private".into())),
        }
    }

    /// The group of `holder`'s own property `key` while it is still a lazy data property.
    fn lazy_data_group(&self, holder: Value, key: PropertyKey) -> Option<Value> {
        let member = self.lazy_member(self.lazy_data_accessor(holder, key)?)?;
        Some(self.member_slot(member, MemberSlot::Group))
    }

    /// Whether script's writes and deletes of `holder`'s still-pending member `key` must
    /// materialize its group first: a binding stands for it, or its group's initializer
    /// redefines members.
    pub(super) fn watched_lazy_member(&self, holder: Value, key: PropertyKey) -> bool {
        self.lazy_data_accessor(holder, key)
            .and_then(|accessor| self.lazy_member(accessor))
            .is_some_and(|member| {
                !self
                    .member_slot(member, MemberSlot::Listeners)
                    .is_undefined()
                    || self.group_slot(
                        self.member_slot(member, MemberSlot::Group),
                        GroupSlot::Redefines,
                    ) == Value::TRUE
            })
    }

    /// The pending value a binding of `holder`'s member `key` holds while the member is still a
    /// lazy data property, registering `listener` to receive `index` and the member's value when
    /// its group materializes. `None` when the member is not pending.
    pub(super) fn pending_lazy_data_property(
        &mut self,
        holder: Value,
        key: PropertyKey,
        listener: Value,
        index: u32,
    ) -> Option<Value> {
        let accessor = self.lazy_data_accessor(holder, key)?;
        let member = self.lazy_member(accessor)?;
        let mut entries = match self.member_slot(member, MemberSlot::Listeners) {
            listeners if listeners.is_undefined() => Vec::new(),
            listeners => self.heap.array_elements(listeners).map(|elements| elements.to_vec()).unwrap_or_default(),
        };
        entries.extend([listener, Value::number(f64::from(index))]);
        let listeners = self.with_call_roots([accessor, member, listener], |vm| vm.new_array(entries));
        if let Some(slot) = self
            .heap
            .environment_slot_mut(member, MemberSlot::Listeners as usize)
        {
            *slot = listeners;
        }
        Some(accessor)
    }

    /// The value a binding holds, reading a pending lazy member (`pending_lazy_data_property`)
    /// as the member: its group materializes, and the binding then holds the member's value.
    pub(super) fn resolve_pending_binding(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        let Some(member) = self.lazy_member(value) else {
            return Ok(value);
        };
        let group = self.member_slot(member, MemberSlot::Group);
        let holder = self
            .heap
            .environment_slot(group, GroupSlot::Holder as usize)
            .unwrap_or(Value::UNDEFINED);
        self.with_call_roots([value, member, group, holder], |vm| {
            vm.materialize_lazy_group(p, group)?;
            let key = vm.lazy_member_key(vm.member_slot(member, MemberSlot::Key))?;
            vm.get_lazy_key(p, holder, key, holder)
        })
    }

    /// Materializes `holder`'s member `key` when a binding stands for it, before script
    /// writes or deletes it (Node's snapshot keeps the member's value).
    pub(super) fn materialize_watched_lazy_member(
        &mut self,
        p: &ResidualProgram,
        holder: Value,
        key: PropertyKey,
    ) -> Result<(), JsError> {
        if self.watched_lazy_member(holder, key) {
            self.materialize_lazy_data_property(p, holder, key)?;
        }
        Ok(())
    }

    fn lazy_data_properties(
        &self,
        holder: Value,
    ) -> impl Iterator<Item = (PropertyKey, Value)> + '_ {
        let object = self.object_data(holder);
        let named = object
            .map(|object| self.shape_entries(object.shape()))
            .unwrap_or(&[])
            .iter()
            .map(|(key, _)| *key);
        let indexed = object
            .filter(|object| object.has_indexed_descriptors())
            .into_iter()
            .flat_map(move |_| {
                self.descriptors
                    .keys()
                    .filter_map(move |(owner, key)| (*owner == holder).then_some(*key))
            });
        named
            .chain(indexed)
            .filter_map(move |key| self.lazy_data_group(holder, key).map(|group| (key, group)))
    }

    pub(super) fn has_lazy_data_properties(&self, holder: Value) -> bool {
        self.lazy_data_properties(holder).next().is_some()
    }

    /// Materializes the pending groups whose initializers add keys to `holder`, before an
    /// operation that observes its complete key list (`GroupSlot::CompletesKeys`).
    pub(super) fn complete_lazy_keys(
        &mut self,
        p: &ResidualProgram,
        holder: Value,
    ) -> Result<(), JsError> {
        let completes = |vm: &Self, group: Value| {
            vm.heap.environment_slot(group, GroupSlot::CompletesKeys as usize) == Some(Value::TRUE)
                && vm.heap.environment_slot(group, GroupSlot::State as usize)
                    .map(GroupState::of)
                    == Some(GroupState::Pending)
        };
        let groups = self
            .lazy_data_properties(holder)
            .map(|(_, group)| group)
            .filter(|&group| completes(self, group))
            .collect::<Vec<_>>();
        for group in groups {
            self.materialize_lazy_group(p, group)?;
        }
        Ok(())
    }

    /// Materializes `holder`'s own property `key` when it is a lazy data property, so a
    /// reflective operation observes its data descriptor.
    pub(super) fn materialize_lazy_data_property(
        &mut self,
        p: &ResidualProgram,
        holder: Value,
        key: PropertyKey,
    ) -> Result<(), JsError> {
        match self.lazy_data_group(holder, key) {
            Some(group) => self.materialize_lazy_group(p, group),
            None => Ok(()),
        }
    }

    fn materialize_lazy_group(&mut self, p: &ResidualProgram, group: Value) -> Result<(), JsError> {
        let slot = |vm: &Self, slot: GroupSlot| {
            vm.heap
                .environment_slot(group, slot as usize)
                .unwrap_or(Value::UNDEFINED)
        };
        if GroupState::of(slot(self, GroupSlot::State)) != GroupState::Pending {
            return Ok(());
        }
        self.set_lazy_group_state(group, GroupState::Running);
        let holder = slot(self, GroupSlot::Holder);
        let initializer = slot(self, GroupSlot::Initializer);
        self.with_call_roots([group, holder, initializer], |vm| {
            // The bindings standing for pending members receive their values once the
            // group is defined.
            let watched = vm
                .lazy_data_properties(holder)
                .filter(|(_, candidate)| *candidate == group)
                .filter_map(|(key, _)| {
                    let member = vm.lazy_member(vm.lazy_data_accessor(holder, key)?)?;
                    let listeners = vm.member_slot(member, MemberSlot::Listeners);
                    (!listeners.is_undefined()).then_some((key, listeners))
                })
                .collect::<Vec<_>>();
            let roots = watched.iter().map(|(_, listeners)| *listeners).collect::<Vec<_>>();
            vm.with_call_roots(roots, |vm| {
                vm.enter_startup_intrinsics(p)?;
                let initialized = (|| {
                    // Entering the startup intrinsics put back any member script had replaced.
                    let keys = vm
                        .lazy_data_properties(holder)
                        .filter_map(|(key, candidate)| (candidate == group).then_some(key))
                        .collect::<Vec<_>>();
                    for key in keys {
                        let redefined = vm
                            .lazy_data_accessor(holder, key)
                            .and_then(|accessor| vm.lazy_member(accessor))
                            .is_some_and(|member| {
                                vm.member_slot(member, MemberSlot::Redefined) == Value::TRUE
                            });
                        if redefined {
                            vm.remove_lazy_data_property(holder, key);
                        } else {
                            vm.replace_lazy_data_property(holder, key, Value::UNDEFINED)?;
                        }
                    }
                    vm.call_value(p, initializer, Value::UNDEFINED, &[])
                        .map(|_| ())
                })();
                vm.set_lazy_group_state(group, GroupState::Done);
                let left = vm.leave_startup_intrinsics(p);
                initialized.and(left)?;
                for (key, listeners) in watched {
                    let PropertyKey::String(atom) = key else { continue };
                    let value = vm.get_property(p, holder, atom)?;
                    let entries = vm.heap.array_elements(listeners).map(|elements| elements.to_vec()).unwrap_or_default();
                    for entry in entries.chunks_exact(2) {
                        vm.with_call_roots([value], |vm| {
                            vm.call_value(p, entry[0], Value::UNDEFINED, &[entry[1], value])
                        })?;
                    }
                }
                Ok(())
            })
        })
    }

    fn set_lazy_group_state(&mut self, group: Value, state: GroupState) {
        if let Some(slot) = self
            .heap
            .environment_slot_mut(group, GroupSlot::State as usize)
        {
            *slot = state.value();
        }
    }

    /// Removes a pending member the initializer redefines, as `[[Delete]]` of a configurable
    /// own property does.
    fn remove_lazy_data_property(&mut self, holder: Value, key: PropertyKey) {
        if let Some((_, slot)) = self.object_property_slot(holder, key) {
            self.heap.property_set(holder, slot, Value::DELETED);
            self.delete_shape_property(holder, key);
        }
    }

    /// Turns a lazy member into a writable data property holding `value` in place.
    fn replace_lazy_data_property(
        &mut self,
        holder: Value,
        key: PropertyKey,
        value: Value,
    ) -> Result<bool, JsError> {
        let record = PropertyDescriptorRecord {
            value: Some(value),
            writable: Some(true),
            enumerable: None,
            configurable: None,
            getter: None,
            setter: None,
        };
        if self.heap.array(holder).is_some()
            && let PropertyKey::String(atom) = key
            && let Some(index) = super::object_static::array_index(self.atom_name(atom))
        {
            return self.define_array_property(holder, index as usize, record);
        }
        self.define_ordinary_property_key(holder, key, record)
    }

    /// The getter (no argument) and setter (one argument) of a lazy data property.
    pub(super) fn lazy_data_property_access(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let member = self
            .active_native_env()
            .ok_or_else(|| JsError("lazy data property requires its environment".into()))?;
        let slot = |vm: &Self, slot: MemberSlot| {
            vm.heap
                .environment_slot(member, slot as usize)
                .unwrap_or(Value::UNDEFINED)
        };
        let group = slot(self, MemberSlot::Group);
        let key = self.lazy_member_key(slot(self, MemberSlot::Key))?;
        let holder = self.group_slot(group, GroupSlot::Holder);
        let still_lazy = |vm: &Self| vm.lazy_data_group(holder, key) == Some(group);
        let watched = self.watched_lazy_member(holder, key);
        match args.first() {
            Some(&value) if receiver == holder && still_lazy(self) && !watched => {
                self.replace_lazy_data_property(holder, key, value)?;
                Ok(Value::UNDEFINED)
            }
            Some(&value) => {
                self.materialize_lazy_group(p, group)?;
                match key {
                    PropertyKey::String(atom) => {
                        self.set_property_with_receiver(p, holder, atom, value, receiver)?;
                    }
                    PropertyKey::Symbol(symbol) => {
                        self.set_symbol_property_with_receiver(p, holder, symbol, value, receiver)?;
                    }
                    PropertyKey::Private(_) => {
                        return Err(JsError("lazy data property key is not private".into()));
                    }
                }
                Ok(Value::UNDEFINED)
            }
            None => {
                self.materialize_lazy_group(p, group)?;
                // Still lazy only while its group's initialization is under way: the member
                // reads as `undefined` instead of re-entering it.
                if still_lazy(self) {
                    return Ok(Value::UNDEFINED);
                }
                self.get_lazy_key(p, holder, key, receiver)
            }
        }
    }
}
