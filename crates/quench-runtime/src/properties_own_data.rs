// Allocation-free facts about ordinary own properties.
//
// Public descriptor objects remain observable API values. Internal property
// routing consumes this projection of the same canonical storage instead of
// constructing those objects merely to rediscover data/accessor attributes.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlainOwnProperty {
    Missing,
    Data { writable: bool },
    Accessor,
}

impl PlainOwnProperty {
    const fn is_data(self) -> bool {
        matches!(self, Self::Data { .. })
    }

    const fn is_writable_data(self) -> bool {
        matches!(self, Self::Data { writable: true })
    }
}

fn plain_own_property(
    properties: &crate::value::ObjectData,
    key: &str,
) -> Option<PlainOwnProperty> {
    if has_special_descriptor_semantics(properties) {
        return None;
    }
    let (own, metadata) = own_and_metadata_slots(properties, key);
    if let Some(slot) = metadata {
        return properties
            .hot_properties()
            .slot_value(slot)
            .as_ref()
            .and_then(descriptor_kind);
    }
    Some(if own {
        PlainOwnProperty::Data { writable: true }
    } else {
        PlainOwnProperty::Missing
    })
}

fn plain_own_property_value(value: &crate::value::Value, key: &str) -> Option<PlainOwnProperty> {
    match value {
        crate::value::Value::Object(properties) => plain_own_property(properties, key),
        crate::value::Value::ObjectAlias(alias) => {
            plain_own_property(alias.target()?.as_ref(), key)
        }
        _ => None,
    }
}

#[cfg(feature = "execution-trace")]
fn record_named_set_fact(target: &crate::value::Value, key: &str, assigned: &crate::value::Value) {
    let fact = plain_own_property_value(target, key);
    let name = match target {
        crate::value::Value::Object(_) => object_set_fact(fact),
        crate::value::Value::ObjectAlias(_) => alias_set_fact(fact),
        crate::value::Value::Array(array) => array_set_fact(array, key, assigned),
        _ => "other",
    };
    crate::execution_trace::named_set_fact(name);
}

#[cfg(feature = "execution-trace")]
fn array_set_fact(
    array: &crate::value::ArrayData,
    key: &str,
    assigned: &crate::value::Value,
) -> &'static str {
    if key == "length" {
        return "array:length";
    }
    let Some(index) = crate::arrays::array_index(key).map(|value| value as usize) else {
        return "array:nonindex";
    };
    if !crate::locals::array_word_is_current(array) {
        return "array:index-stale";
    }
    use crate::value::PlainDenseIndexFact as Fact;
    match array.plain_dense_index_fact(index) {
        Fact::Available => "array:index-dense",
        Fact::Arguments => "array:index-arguments",
        Fact::NamedDescriptor => "array:index-named-descriptor",
        Fact::IndexedDescriptor => "array:index-indexed-descriptor",
        Fact::ReadonlyLength => "array:index-readonly-length",
        Fact::Deleted => "array:index-deleted",
        Fact::Mapped => "array:index-mapped",
        Fact::BeyondPhysicalLength if index == array.physical_len() => {
            if matches!(assigned, crate::value::Value::Number(_)) {
                "array:index-preallocated-number"
            } else {
                "array:index-preallocated-other"
            }
        }
        Fact::BeyondPhysicalLength => "array:index-physical-hole",
        Fact::BeyondLogicalLength if index == array.header_length() => "array:index-append",
        Fact::BeyondLogicalLength => "array:index-gap",
    }
}

#[cfg(feature = "execution-trace")]
fn object_set_fact(fact: Option<PlainOwnProperty>) -> &'static str {
    match fact {
        Some(PlainOwnProperty::Missing) => "object:missing",
        Some(PlainOwnProperty::Data { writable: true }) => "object:writable",
        Some(PlainOwnProperty::Data { writable: false }) => "object:readonly",
        Some(PlainOwnProperty::Accessor) => "object:accessor",
        None => "object:special",
    }
}

#[cfg(feature = "execution-trace")]
fn alias_set_fact(fact: Option<PlainOwnProperty>) -> &'static str {
    match fact {
        Some(PlainOwnProperty::Missing) => "alias:missing",
        Some(PlainOwnProperty::Data { writable: true }) => "alias:writable",
        Some(PlainOwnProperty::Data { writable: false }) => "alias:readonly",
        Some(PlainOwnProperty::Accessor) => "alias:accessor",
        None => "alias:special",
    }
}

#[cfg(not(feature = "execution-trace"))]
#[inline(always)]
fn record_named_set_fact(_: &crate::value::Value, _: &str, _: &crate::value::Value) {}

fn own_and_metadata_slots(
    properties: &crate::value::ObjectData,
    key: &str,
) -> (bool, Option<usize>) {
    let mut own = false;
    let mut metadata = None;
    for (slot, name) in properties.hot_properties().names().enumerate().rev() {
        if crate::builtins::is_deleted_key_for(name, key) {
            return (false, None);
        }
        own |= name == key;
        if metadata.is_none() && crate::builtins::is_descriptor_key_for(name, key) {
            metadata = Some(slot);
        }
        if own && metadata.is_some() {
            break;
        }
    }
    (own, metadata)
}

fn has_special_descriptor_semantics(properties: &crate::value::ObjectData) -> bool {
    properties.is_realm_global()
        || properties.is_script_global_view()
        || properties.has_regexp_internal_slot()
        || properties.hot_properties().position_rev("_value").is_some()
}

fn descriptor_kind(value: &crate::value::Value) -> Option<PlainOwnProperty> {
    let crate::value::Value::Object(fields) = value else {
        return None;
    };
    let mut writable = None;
    for (name, value) in fields.iter().rev() {
        if matches!(name.as_str(), "get" | "set") {
            return Some(PlainOwnProperty::Accessor);
        }
        if writable.is_none() && name == "writable" {
            writable = Some(matches!(value, crate::value::Value::Boolean(true)));
        }
    }
    Some(PlainOwnProperty::Data {
        writable: writable.unwrap_or(true),
    })
}

fn plain_writable_own_data(properties: &crate::value::ObjectData, key: &str) -> bool {
    let (own, metadata) = own_and_metadata_slots(properties, key);
    if !own {
        return false;
    }
    metadata.is_none_or(|slot| {
        properties
            .hot_properties()
            .slot_value(slot)
            .as_ref()
            .and_then(descriptor_kind)
            .is_some_and(PlainOwnProperty::is_writable_data)
    })
}

fn store_plain_writable_own_data(
    properties: &crate::value::ObjectData,
    key: &str,
    value: &crate::value::Value,
) -> bool {
    if !plain_writable_own_data(properties, key) {
        return false;
    }
    let Some(slot) = properties.hot_properties().position_rev(key) else {
        return false;
    };
    if let Some(crate::value::Value::BindingCell(cell)) =
        properties.hot_properties().slot_value(slot)
    {
        cell.store(value.clone());
        return true;
    }
    properties.hot_properties().store_slot(slot, value.clone());
    true
}

#[cfg(test)]
mod own_data_tests {
    #[cfg(feature = "execution-trace")]
    use super::{alias_set_fact, array_set_fact, object_set_fact};
    use super::{
        plain_own_property, plain_own_property_value, plain_writable_own_data,
        store_plain_writable_own_data, PlainOwnProperty,
    };
    use crate::value::{ObjectAliasValue, ObjectData, Value};
    use std::{cell::RefCell, rc::Rc};

    fn object(metadata: Option<Value>) -> ObjectData {
        let mut entries = vec![("field".into(), Value::Number(1.0))];
        if let Some(metadata) = metadata {
            entries.push((crate::builtins::descriptor_key("field"), metadata));
        }
        ObjectData::new(entries)
    }

    fn descriptor(fields: Vec<(&str, Value)>) -> Value {
        Value::Object(std::rc::Rc::new(ObjectData::new(
            fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        )))
    }

    #[test]
    fn classifies_plain_writable_and_readonly_data() {
        assert_eq!(
            plain_own_property(&object(None), "field"),
            Some(PlainOwnProperty::Data { writable: true })
        );
        let readonly = descriptor(vec![("writable", Value::Boolean(false))]);
        assert_eq!(
            plain_own_property(&object(Some(readonly)), "field"),
            Some(PlainOwnProperty::Data { writable: false })
        );
    }

    #[test]
    fn alias_view_uses_the_same_fact_and_slot_store() {
        let owner = Rc::new(object(None));
        let alias = Value::ObjectAlias(ObjectAliasValue(Rc::new(RefCell::new(Rc::downgrade(
            &owner,
        )))));

        assert_eq!(
            plain_own_property_value(&alias, "field"),
            Some(PlainOwnProperty::Data { writable: true })
        );
        assert!(store_plain_writable_own_data(
            &owner,
            "field",
            &Value::Number(9.0)
        ));
        assert_eq!(
            owner.hot_properties().slot_value(0),
            Some(Value::Number(9.0))
        );
    }

    #[test]
    fn classifies_accessor_deleted_and_missing_properties() {
        let getter = descriptor(vec![("get", Value::Undefined)]);
        assert_eq!(
            plain_own_property(&object(Some(getter)), "field"),
            Some(PlainOwnProperty::Accessor)
        );
        let mut deleted = object(None);
        deleted.properties.push((
            crate::builtins::deleted_key("field").into(),
            Value::Undefined,
        ));
        assert_eq!(
            plain_own_property(&deleted, "field"),
            Some(PlainOwnProperty::Missing)
        );
        assert!(!plain_writable_own_data(&object(None), "missing"));
    }

    #[cfg(feature = "execution-trace")]
    #[test]
    fn named_set_trace_labels_preserve_receiver_and_property_fact() {
        let facts = [
            (None, "object:special", "alias:special"),
            (
                Some(PlainOwnProperty::Missing),
                "object:missing",
                "alias:missing",
            ),
            (
                Some(PlainOwnProperty::Data { writable: true }),
                "object:writable",
                "alias:writable",
            ),
            (
                Some(PlainOwnProperty::Data { writable: false }),
                "object:readonly",
                "alias:readonly",
            ),
            (
                Some(PlainOwnProperty::Accessor),
                "object:accessor",
                "alias:accessor",
            ),
        ];
        for (fact, object, alias) in facts {
            assert_eq!(object_set_fact(fact), object);
            assert_eq!(alias_set_fact(fact), alias);
        }
    }

    #[cfg(feature = "execution-trace")]
    #[test]
    fn named_set_array_labels_separate_dense_append_gap_and_names() {
        let mut array = crate::value::ArrayData::new(vec![Value::Number(1.0)]);
        let number = Value::Number(2.0);
        assert_eq!(array_set_fact(&array, "0", &number), "array:index-dense");
        assert_eq!(array_set_fact(&array, "1", &number), "array:index-append");
        assert_eq!(array_set_fact(&array, "3", &number), "array:index-gap");
        assert_eq!(array_set_fact(&array, "length", &number), "array:length");
        assert_eq!(array_set_fact(&array, "field", &number), "array:nonindex");
        array.set_length(3);
        assert_eq!(
            array_set_fact(&array, "1", &number),
            "array:index-preallocated-number"
        );
        assert_eq!(
            array_set_fact(&array, "1", &Value::String("x".into())),
            "array:index-preallocated-other"
        );
    }
}
