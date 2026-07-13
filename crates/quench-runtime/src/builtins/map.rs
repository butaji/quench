//! Map and Set built-ins

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::{JsError, NativeFunction, Object, ObjectKind, Value};
use crate::Context;

// ============================================================================
// Map and Set
// ============================================================================

/// SameValueZero key equality: NaN equals NaN, +0 and -0 are the same key
fn same_value_zero(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => crate::value::strict_eq(a, b),
    }
}

/// Get the internal entries array (`_entries`: array of [key, value] pairs)
fn map_entries(this: &Value) -> Option<Rc<RefCell<Object>>> {
    if let Value::Object(o) = this {
        if let Some(Value::Object(entries)) = o.borrow().get("_entries") {
            return Some(Rc::clone(&entries));
        }
    }
    None
}

/// Find the pair array holding `key`, or None
fn map_find_pair(entries: &Rc<RefCell<Object>>, key: &Value) -> Option<Rc<RefCell<Object>>> {
    let elements = entries.borrow().elements.clone();
    for elem in elements {
        if let Value::Object(pair) = elem {
            let k = pair.borrow().elements.first().cloned();
            if let Some(k) = k {
                if same_value_zero(&k, key) {
                    return Some(pair);
                }
            }
        }
    }
    None
}

/// Store the current entry count in the map's `size` property
fn map_update_size(this: &Value, entries: &Rc<RefCell<Object>>) {
    let size = entries.borrow().elements.len() as f64;
    if let Value::Object(o) = this {
        o.borrow_mut().set("size", Value::Number(size));
    }
}

fn map_set_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let key = args.first().cloned().unwrap_or(Value::Undefined);
    let value = args.get(1).cloned().unwrap_or(Value::Undefined);
    let Some(entries) = map_entries(&this) else {
        return Err(JsError::from(
            "TypeError: Map.prototype.set called on non-Map",
        ));
    };
    if let Some(pair) = map_find_pair(&entries, &key) {
        pair.borrow_mut().elements[1] = value;
    } else {
        let pair = Object::new_array_from(vec![key, value]);
        let idx = entries.borrow().elements.len().to_string();
        entries
            .borrow_mut()
            .set(&idx, Value::Object(Rc::new(RefCell::new(pair))));
        map_update_size(&this, &entries);
    }
    Ok(this)
}

fn map_get_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let key = args.first().cloned().unwrap_or(Value::Undefined);
    if let Some(entries) = map_entries(&this) {
        if let Some(pair) = map_find_pair(&entries, &key) {
            let v = pair
                .borrow()
                .elements
                .get(1)
                .cloned()
                .unwrap_or(Value::Undefined);
            return Ok(v);
        }
    }
    Ok(Value::Undefined)
}

fn map_has_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let key = args.first().cloned().unwrap_or(Value::Undefined);
    let found = map_entries(&this)
        .map(|entries| map_find_pair(&entries, &key).is_some())
        .unwrap_or(false);
    Ok(Value::Boolean(found))
}

fn map_delete_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let key = args.first().cloned().unwrap_or(Value::Undefined);
    let Some(entries) = map_entries(&this) else {
        return Ok(Value::Boolean(false));
    };
    let pos = {
        let entries_ref = entries.borrow();
        entries_ref.elements.iter().position(|elem| {
            if let Value::Object(pair) = elem {
                if let Some(k) = pair.borrow().elements.first() {
                    return same_value_zero(k, &key);
                }
            }
            false
        })
    };
    if let Some(pos) = pos {
        entries.borrow_mut().elements.remove(pos);
        let len = entries.borrow().elements.len() as f64;
        entries.borrow_mut().set("length", Value::Number(len));
        map_update_size(&this, &entries);
        return Ok(Value::Boolean(true));
    }
    Ok(Value::Boolean(false))
}

fn map_clear_impl(_args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let store = map_entries(&this).or_else(|| set_values(&this));
    if let Some(store) = store {
        store.borrow_mut().elements.clear();
        store.borrow_mut().set("length", Value::Number(0.0));
        map_update_size(&this, &store);
    }
    Ok(Value::Undefined)
}

/// Get the internal values array (`_values`) of a Set
fn set_values(this: &Value) -> Option<Rc<RefCell<Object>>> {
    if let Value::Object(o) = this {
        if let Some(Value::Object(values)) = o.borrow().get("_values") {
            return Some(Rc::clone(&values));
        }
    }
    None
}

fn set_has_value(values: &Rc<RefCell<Object>>, value: &Value) -> bool {
    values
        .borrow()
        .elements
        .iter()
        .any(|v| same_value_zero(v, value))
}

fn set_add_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let Some(values) = set_values(&this) else {
        return Err(JsError::from(
            "TypeError: Set.prototype.add called on non-Set",
        ));
    };
    if !set_has_value(&values, &value) {
        let idx = values.borrow().elements.len().to_string();
        values.borrow_mut().set(&idx, value);
        map_update_size(&this, &values);
    }
    Ok(this)
}

fn set_has_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let found = set_values(&this)
        .map(|values| set_has_value(&values, &value))
        .unwrap_or(false);
    Ok(Value::Boolean(found))
}

fn set_delete_impl(args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let Some(values) = set_values(&this) else {
        return Ok(Value::Boolean(false));
    };
    let pos = values
        .borrow()
        .elements
        .iter()
        .position(|v| same_value_zero(v, &value));
    if let Some(pos) = pos {
        values.borrow_mut().elements.remove(pos);
        let len = values.borrow().elements.len() as f64;
        values.borrow_mut().set("length", Value::Number(len));
        map_update_size(&this, &values);
        return Ok(Value::Boolean(true));
    }
    Ok(Value::Boolean(false))
}

fn native_fn(f: impl Fn(Vec<Value>) -> Result<Value, JsError> + 'static) -> Value {
    Value::NativeFunction(Rc::new(NativeFunction::new(f)))
}

/// Build an iterator object over a snapshot of values (`{ next() }` protocol).
fn make_iterator(items: Vec<Value>) -> Value {
    let items = Rc::new(items);
    let index = Rc::new(RefCell::new(0usize));
    let next_fn = NativeFunction::new(move |_args| {
        let mut obj = Object::new(ObjectKind::Ordinary);
        let mut i = index.borrow_mut();
        if *i < items.len() {
            obj.set("value", items[*i].clone());
            obj.set("done", Value::Boolean(false));
            *i += 1;
        } else {
            obj.set("value", Value::Undefined);
            obj.set("done", Value::Boolean(true));
        }
        Ok(Value::Object(Rc::new(RefCell::new(obj))))
    });
    let mut iter = Object::new(ObjectKind::Ordinary);
    iter.set("next", Value::NativeFunction(Rc::new(next_fn)));
    Value::Object(Rc::new(RefCell::new(iter)))
}

fn map_iterator_impl(_args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let items = map_entries(&this)
        .map(|e| e.borrow().elements.clone())
        .unwrap_or_default();
    Ok(make_iterator(items))
}

fn set_iterator_impl(_args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let items = set_values(&this)
        .map(|v| v.borrow().elements.clone())
        .unwrap_or_default();
    Ok(make_iterator(items))
}

/// Property key for the Symbol.iterator method: computed member access with a
/// symbol evaluates to the symbol payload, so the method is stored under that
/// exact key.
fn iterator_prop_key() -> Option<String> {
    match crate::builtins::symbol::get_well_known_symbol_no_ctx("iterator") {
        Some(Value::Symbol(payload)) => Some(payload.desc.clone().unwrap_or_default()),
        _ => None,
    }
}

/// Populate a Map's entries from a constructor argument: an array of [k, v]
/// pairs or another Map (mirrors `new Map(iterable)`).
fn map_populate(entries: &Rc<RefCell<Object>>, src: &Value) {
    let pairs: Vec<Value> = match src {
        Value::Object(o) => match map_entries(src) {
            Some(src_entries) => src_entries.borrow().elements.clone(),
            None => o.borrow().elements.clone(),
        },
        _ => Vec::new(),
    };
    for pair in pairs {
        if let Value::Object(p) = pair {
            let elems = p.borrow().elements.clone();
            if elems.len() >= 2 {
                let (k, v) = (elems[0].clone(), elems[1].clone());
                if let Some(existing) = map_find_pair(entries, &k) {
                    existing.borrow_mut().elements[1] = v;
                } else {
                    let pair_obj = Object::new_array_from(vec![k, v]);
                    let idx = entries.borrow().elements.len().to_string();
                    entries
                        .borrow_mut()
                        .set(&idx, Value::Object(Rc::new(RefCell::new(pair_obj))));
                }
            }
        }
    }
}

/// Populate a Set's values from a constructor argument: an array or another
/// Set (mirrors `new Set(iterable)`).
fn set_populate(values: &Rc<RefCell<Object>>, src: &Value) {
    let items: Vec<Value> = match src {
        Value::Object(o) => match set_values(src) {
            Some(src_values) => src_values.borrow().elements.clone(),
            None => o.borrow().elements.clone(),
        },
        _ => Vec::new(),
    };
    for item in items {
        if !set_has_value(values, &item) {
            let idx = values.borrow().elements.len().to_string();
            values.borrow_mut().set(&idx, item);
        }
    }
}

pub fn register_map_and_set(ctx: &mut Context) {
    let object_proto = crate::builtins::get_object_prototype();

    // ---- Map ----
    let map_proto = Object::new(ObjectKind::Ordinary);
    let map_proto = Rc::new(RefCell::new(map_proto));
    if let Some(ref op) = object_proto {
        map_proto.borrow_mut().prototype = Some(Rc::clone(op));
    }
    {
        let mut p = map_proto.borrow_mut();
        p.set("set", native_fn(map_set_impl));
        p.set("get", native_fn(map_get_impl));
        p.set("has", native_fn(map_has_impl));
        p.set("delete", native_fn(map_delete_impl));
        p.set("clear", native_fn(map_clear_impl));
        if let Some(key) = iterator_prop_key() {
            p.set(&key, native_fn(map_iterator_impl));
        }
    }
    let map_proto_for_ctor = Rc::clone(&map_proto);
    let map_constructor = native_fn(move |args| {
        let map_obj = Object::with_prototype(ObjectKind::Map, Rc::clone(&map_proto_for_ctor));
        let map = Rc::new(RefCell::new(map_obj));
        {
            let mut m = map.borrow_mut();
            let entries = Object::new_array(0);
            m.set("_entries", Value::Object(Rc::new(RefCell::new(entries))));
            m.set("size", Value::Number(0.0));
        }
        if let Some(src) = args.first() {
            if !matches!(src, Value::Undefined | Value::Null) {
                let entries_val = map.borrow().get("_entries");
                if let Some(Value::Object(entries)) = entries_val {
                    map_populate(&entries, src);
                    map_update_size(&Value::Object(Rc::clone(&map)), &entries);
                }
            }
        }
        Ok(Value::Object(map))
    });
    if let Value::NativeFunction(nf) = &map_constructor {
        nf.set_property("prototype", Value::Object(map_proto));
        nf.set_property("name", Value::String("Map".to_string()));
    }
    ctx.set_global("Map".to_string(), map_constructor);

    // ---- Set ----
    let set_proto = Object::new(ObjectKind::Ordinary);
    let set_proto = Rc::new(RefCell::new(set_proto));
    if let Some(ref op) = object_proto {
        set_proto.borrow_mut().prototype = Some(Rc::clone(op));
    }
    {
        let mut p = set_proto.borrow_mut();
        p.set("add", native_fn(set_add_impl));
        p.set("has", native_fn(set_has_impl));
        p.set("delete", native_fn(set_delete_impl));
        p.set("clear", native_fn(map_clear_impl));
        if let Some(key) = iterator_prop_key() {
            p.set(&key, native_fn(set_iterator_impl));
        }
    }
    let set_proto_for_ctor = Rc::clone(&set_proto);
    let set_constructor = native_fn(move |args| {
        let set_obj = Object::with_prototype(ObjectKind::Set, Rc::clone(&set_proto_for_ctor));
        let set = Rc::new(RefCell::new(set_obj));
        {
            let mut s = set.borrow_mut();
            let values = Object::new_array(0);
            s.set("_values", Value::Object(Rc::new(RefCell::new(values))));
            s.set("size", Value::Number(0.0));
        }
        if let Some(src) = args.first() {
            if !matches!(src, Value::Undefined | Value::Null) {
                let values_val = set.borrow().get("_values");
                if let Some(Value::Object(values)) = values_val {
                    set_populate(&values, src);
                    map_update_size(&Value::Object(Rc::clone(&set)), &values);
                }
            }
        }
        Ok(Value::Object(set))
    });
    if let Value::NativeFunction(nf) = &set_constructor {
        nf.set_property("prototype", Value::Object(set_proto));
        nf.set_property("name", Value::String("Set".to_string()));
    }
    ctx.set_global("Set".to_string(), set_constructor);
}

#[cfg(test)]
mod tests {
    use crate::value::Value;
    use crate::Context;

    #[test]
    fn test_map_set_get_has_size() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var m = new Map(); m.set('a', 1); m.set('b', 2);")
            .unwrap();
        assert_eq!(ctx.eval("m.get('a')").unwrap(), Value::Number(1.0));
        assert_eq!(ctx.eval("m.get('missing')").unwrap(), Value::Undefined);
        assert_eq!(ctx.eval("m.has('b')").unwrap(), Value::Boolean(true));
        assert_eq!(ctx.eval("m.size").unwrap(), Value::Number(2.0));
    }

    #[test]
    fn test_map_nan_key() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var m = new Map(); m.set(NaN, 42);").unwrap();
        assert_eq!(ctx.eval("m.get(NaN)").unwrap(), Value::Number(42.0));
        assert_eq!(ctx.eval("m.has(NaN)").unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_map_delete_clear() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var m = new Map(); m.set('a', 1); m.set('b', 2);")
            .unwrap();
        assert_eq!(ctx.eval("m.delete('a')").unwrap(), Value::Boolean(true));
        assert_eq!(ctx.eval("m.delete('a')").unwrap(), Value::Boolean(false));
        assert_eq!(ctx.eval("m.size").unwrap(), Value::Number(1.0));
        ctx.eval("m.clear();").unwrap();
        assert_eq!(ctx.eval("m.size").unwrap(), Value::Number(0.0));
    }

    #[test]
    fn test_set_basics() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var s = new Set(); s.add(1); s.add(2); s.add(2);")
            .unwrap();
        assert_eq!(ctx.eval("s.size").unwrap(), Value::Number(2.0));
        assert_eq!(ctx.eval("s.has(2)").unwrap(), Value::Boolean(true));
        assert_eq!(ctx.eval("s.delete(1)").unwrap(), Value::Boolean(true));
        assert_eq!(ctx.eval("s.size").unwrap(), Value::Number(1.0));
        ctx.eval("s.clear();").unwrap();
        assert_eq!(ctx.eval("s.has(2)").unwrap(), Value::Boolean(false));
    }

    #[test]
    fn test_set_nan_value() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var s = new Set(); s.add(NaN); s.add(NaN);")
            .unwrap();
        assert_eq!(ctx.eval("s.size").unwrap(), Value::Number(1.0));
        assert_eq!(ctx.eval("s.has(NaN)").unwrap(), Value::Boolean(true));
    }
}
