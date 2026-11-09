//! A property descriptor record (ECMA-262 PropertyDescriptor) and its field algebra:
//! completion, compatibility and the conflicts a redefinition may raise.
use super::*;

#[derive(Clone, Copy)]
pub(in crate::vm) struct PropertyDescriptorRecord {
    pub(in crate::vm) value: Option<Value>,
    pub(in crate::vm) writable: Option<bool>,
    pub(in crate::vm) enumerable: Option<bool>,
    pub(in crate::vm) configurable: Option<bool>,
    pub(in crate::vm) getter: Option<Value>,
    pub(in crate::vm) setter: Option<Value>,
}

impl PropertyDescriptorRecord {
    pub(in crate::vm) fn data(value: Value) -> Self {
        Self {
            value: Some(value),
            writable: Some(true),
            enumerable: Some(true),
            configurable: Some(true),
            getter: None,
            setter: None,
        }
    }

    pub(in crate::vm) fn for_definition(mode: PropertyDefinitionMode, value: Value) -> Self {
        use PropertyDefinitionMode::*;
        match mode {
            Method | ReadonlyMethod => Self {
                writable: Some(mode == Method),
                enumerable: Some(false),
                ..Self::data(value)
            },
            Getter | Setter | EnumerableGetter | EnumerableSetter => Self {
                value: None,
                writable: None,
                enumerable: Some(matches!(mode, EnumerableGetter | EnumerableSetter)),
                configurable: Some(true),
                getter: matches!(mode, Getter | EnumerableGetter).then_some(value),
                setter: matches!(mode, Setter | EnumerableSetter).then_some(value),
            },
        }
    }

    pub(in crate::vm) fn value(value: Value) -> Self {
        Self {
            value: Some(value),
            writable: None,
            enumerable: None,
            configurable: None,
            getter: None,
            setter: None,
        }
    }

    pub(in crate::vm) fn compatible_with(
        self,
        current: Option<Self>,
        extensible: bool,
        same_value: impl Fn(Value, Value) -> bool,
    ) -> bool {
        let Some(current) = current else {
            return extensible;
        };
        let attributes = current.fold_attributes(DEFAULT_PROPERTY_ATTRIBUTES, true);
        if attributes.configurable {
            return true;
        }
        let next = self.fold_attributes(attributes, false);
        if self.non_configurable_conflict(attributes, next).is_some()
            || self.changes_non_configurable_accessor(attributes, &same_value)
        {
            return false;
        }
        attributes.accessor
            || attributes.writable
            || self
                .value
                .is_none_or(|value| same_value(value, current.value.unwrap_or(Value::UNDEFINED)))
    }

    pub(in crate::vm) fn from_attributes(value: Value, attributes: PropertyAttributes) -> Self {
        Self {
            value: (!attributes.accessor).then_some(value),
            writable: (!attributes.accessor).then_some(attributes.writable),
            enumerable: Some(attributes.enumerable),
            configurable: Some(attributes.configurable),
            getter: attributes
                .accessor
                .then_some(attributes.getter.unwrap_or(Value::UNDEFINED)),
            setter: attributes
                .accessor
                .then_some(attributes.setter.unwrap_or(Value::UNDEFINED)),
        }
    }

    pub(in crate::vm) fn has_accessor_fields(self) -> bool {
        self.getter.is_some() || self.setter.is_some()
    }

    /// Whether this descriptor gives every field of its kind other than the shared
    /// `[[Enumerable]]` and `[[Configurable]]`, so the property it defines keeps nothing of
    /// the current one but those.
    pub(in crate::vm) fn replaces_kind_fields(self) -> bool {
        (self.value.is_some() && self.writable.is_some())
            || (self.getter.is_some() && self.setter.is_some())
    }

    pub(in crate::vm) fn has_data_fields(self) -> bool {
        self.value.is_some() || self.writable.is_some()
    }

    pub(in crate::vm) fn fold_attributes(self, current: PropertyAttributes, is_new: bool) -> PropertyAttributes {
        let mut next = if is_new {
            PropertyAttributes { writable: false, enumerable: false, configurable: false, accessor: false, getter: None, setter: None }
        } else {
            current
        };
        next.writable = self.writable.unwrap_or(next.writable);
        next.enumerable = self.enumerable.unwrap_or(next.enumerable);
        next.configurable = self.configurable.unwrap_or(next.configurable);
        if self.has_accessor_fields() {
            next.accessor = true;
            next.writable = false;
            next.getter = Self::folded_accessor(self.getter, current, current.getter);
            next.setter = Self::folded_accessor(self.setter, current, current.setter);
        } else if self.has_data_fields() {
            next.accessor = false;
            next.getter = None;
            next.setter = None;
        }
        next
    }

    /// A getter or setter `field` folded over the current property's `held` one: `undefined`
    /// clears it, and an absent field keeps a current accessor's.
    fn folded_accessor(field: Option<Value>, current: PropertyAttributes, held: Option<Value>) -> Option<Value> {
        field.map(|value| (!value.is_undefined()).then_some(value)).or_else(|| current.accessor.then_some(held)).flatten()
    }

    pub(in crate::vm) fn non_configurable_conflict(
        self,
        current: PropertyAttributes,
        next: PropertyAttributes,
    ) -> Option<DescriptorConflict> {
        if current.configurable {
            return None;
        }
        if next.configurable != current.configurable
            || next.enumerable != current.enumerable
            || next.writable && !current.writable
        {
            return Some(DescriptorConflict::Attributes);
        }
        let changes_kind = (self.has_accessor_fields() || self.has_data_fields())
            && self.has_accessor_fields() != current.accessor;
        changes_kind.then_some(DescriptorConflict::Kind)
    }

    pub(in crate::vm) fn changes_non_configurable_accessor(
        self,
        current: PropertyAttributes,
        same_value: impl Fn(Value, Value) -> bool,
    ) -> bool {
        !current.configurable
            && current.accessor
            && [(self.getter, current.getter), (self.setter, current.setter)]
                .into_iter()
                .any(|(requested, existing)| {
                    requested.is_some_and(|requested| {
                        !(requested.is_undefined() && existing.is_none())
                            && !existing.is_some_and(|existing| same_value(existing, requested))
                    })
                })
    }
}

#[derive(Clone, Copy)]
pub(in crate::vm) enum DescriptorConflict {
    Attributes,
    Kind,
}
