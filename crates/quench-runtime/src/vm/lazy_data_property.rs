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

use super::object_descriptors::PropertyDescriptorRecord;
use super::*;

/// The slots of the environment one group's members share.
#[repr(usize)]
enum GroupSlot {
    Holder,
    Initializer,
    State,
}

impl GroupSlot {
    const COUNT: usize = Self::State as usize + 1;
}

/// The slots of one member's own environment.
#[repr(usize)]
enum MemberSlot {
    Group,
    Key,
}

impl MemberSlot {
    const COUNT: usize = Self::Key as usize + 1;
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
    /// Defines each of `keys` (string property keys) on `holder` as one lazy group of
    /// configurable data properties that `initializer` defines when any of them is observed.
    pub(crate) fn define_lazy_data_properties(
        &mut self,
        p: &ResidualProgram,
        holder: Value,
        keys: &[Value],
        enumerable: bool,
        initializer: Value,
    ) -> Result<(), JsError> {
        let mut slots = vec![Value::UNDEFINED; GroupSlot::COUNT];
        slots[GroupSlot::Holder as usize] = holder;
        slots[GroupSlot::Initializer as usize] = initializer;
        slots[GroupSlot::State as usize] = GroupState::Pending.value();
        let group = self.native_environment(slots);
        self.with_call_roots([group], |vm| {
            for &key in keys {
                let mut slots = vec![Value::UNDEFINED; MemberSlot::COUNT];
                slots[MemberSlot::Group as usize] = group;
                slots[MemberSlot::Key as usize] = key;
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

    /// The group of `holder`'s own property `key` while it is still a lazy data property.
    fn lazy_data_group(&self, holder: Value, key: PropertyKey) -> Option<Value> {
        let attributes = self.property_attributes(holder, key)?;
        let getter = attributes.getter.filter(|_| attributes.accessor)?;
        let Some(Cell::Function {
            kind: FunctionKind::Native(Native::LazyDataProperty),
            env,
            ..
        }) = self.heap.get(getter)
        else {
            return None;
        };
        self.heap.environment_slot(*env, MemberSlot::Group as usize)
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
            vm.enter_startup_intrinsics(p)?;
            let initialized = (|| {
                // Entering the startup intrinsics put back any member script had replaced.
                let keys = vm
                    .lazy_data_properties(holder)
                    .filter_map(|(key, candidate)| (candidate == group).then_some(key))
                    .collect::<Vec<_>>();
                for key in keys {
                    vm.replace_lazy_data_property(holder, key, Value::UNDEFINED)?;
                }
                vm.call_value(p, initializer, Value::UNDEFINED, &[])
                    .map(|_| ())
            })();
            vm.set_lazy_group_state(group, GroupState::Done);
            let left = vm.leave_startup_intrinsics(p);
            initialized.and(left)
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
        let Some(Cell::String(name)) = self.heap.get(slot(self, MemberSlot::Key)).cloned() else {
            return Err(JsError("lazy data property key is a string".into()));
        };
        let key = self.intern_js_atom(&name);
        let holder = self
            .heap
            .environment_slot(group, GroupSlot::Holder as usize)
            .unwrap_or(Value::UNDEFINED);
        let still_lazy =
            |vm: &Self| vm.lazy_data_group(holder, PropertyKey::string(key)) == Some(group);
        match args.first() {
            Some(&value) if receiver == holder && still_lazy(self) => {
                self.replace_lazy_data_property(holder, PropertyKey::string(key), value)?;
                Ok(Value::UNDEFINED)
            }
            Some(&value) => {
                self.materialize_lazy_group(p, group)?;
                self.set_property_with_receiver(p, holder, key, value, receiver)?;
                Ok(Value::UNDEFINED)
            }
            None => {
                self.materialize_lazy_group(p, group)?;
                // Still lazy only while its group's initialization is under way: the member
                // reads as `undefined` instead of re-entering it.
                if still_lazy(self) {
                    return Ok(Value::UNDEFINED);
                }
                self.get_property_with_receiver(p, holder, key, receiver)
            }
        }
    }
}
