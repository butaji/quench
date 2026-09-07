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
    if properties.has_deleted_key(key) {
        return Some(PlainOwnProperty::Missing);
    }
    let metadata = crate::builtins::descriptor_metadata(properties, key);
    if let Some(value) = metadata {
        return descriptor_kind(&value);
    }
    Some(if properties.hot_properties().position_rev(key).is_some() {
        PlainOwnProperty::Data { writable: true }
    } else {
        PlainOwnProperty::Missing
    })
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
    plain_own_property(properties, key).is_some_and(PlainOwnProperty::is_writable_data)
}

#[cfg(test)]
mod own_data_tests {
    use super::{plain_own_property, plain_writable_own_data, PlainOwnProperty};
    use crate::value::{ObjectData, Value};

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
}
