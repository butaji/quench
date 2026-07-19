//! Map and Set built-ins

use std::cell::RefCell;
use std::rc::Rc;

use crate::eval::call_value_with_this;
use crate::eval::member::eval_object_member;
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
        Some(Value::Symbol(payload)) => Some(
            payload
                .desc
                .clone()
                .map(|s| s.to_string())
                .unwrap_or_default(),
        ),
        _ => None,
    }
}

/// Populate a Map from an iterable source. Per spec, `new Map(iterable)`:
/// 1. Get adder = Map.prototype.set (this may throw via getter)
/// 2. For each entry [k, v] in iterable, call adder(k, v)
fn map_populate(map: &Rc<RefCell<Object>>, src: &Value) -> Result<(), JsError> {
    // Per spec, we must GET the adder BEFORE iterating
    // This is step 8b: "Let adder be Get(map, "set")"
    let adder = eval_object_member(map, "set", None)?;

    let pairs: Vec<Value> = match src {
        Value::Object(o) => match map_entries(src) {
            // src is another Map - copy its entries
            Some(src_entries) => src_entries.borrow().elements.clone(),
            // src is an array-like object - use its elements as pairs
            None => o.borrow().elements.clone(),
        },
        _ => Vec::new(),
    };
    for pair in pairs {
        if let Value::Object(p) = pair {
            let elems = p.borrow().elements.clone();
            if elems.len() >= 2 {
                let k = elems[0].clone();
                let v = elems[1].clone();
                call_value_with_this(adder.clone(), vec![k, v], Value::Object(Rc::clone(map)))?;
            }
        }
    }
    Ok(())
}

/// Populate a Set from an iterable source. Per spec, `new Set(iterable)`:
/// 1. Get adder = Set.prototype.add (this may throw via getter)
/// 2. For each value in iterable, call adder(value)
fn set_populate(set: &Rc<RefCell<Object>>, src: &Value) -> Result<(), JsError> {
    // Per spec, we must GET the adder BEFORE iterating
    let adder = eval_object_member(set, "add", None)?;

    let items: Vec<Value> = match src {
        Value::Object(o) => match set_values(src) {
            // src is another Set - copy its values
            Some(src_values) => src_values.borrow().elements.clone(),
            // src is an array-like object - use its elements
            None => o.borrow().elements.clone(),
        },
        _ => Vec::new(),
    };
    for item in items {
        call_value_with_this(adder.clone(), vec![item], Value::Object(Rc::clone(set)))?;
    }
    Ok(())
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
                map_populate(&map, src)?;
            }
        }
        Ok(Value::Object(map))
    });
    if let Value::NativeFunction(nf) = &map_constructor {
        let _ = nf.set_property("prototype", Value::Object(map_proto));
        let _ = nf.set_property("name", Value::String("Map".to_string()));
        // Set up Symbol.species getter - returns this (the constructor) by default
        if let Some(species_sym) = crate::builtins::symbol::get_well_known_symbol_no_ctx("species")
        {
            let map_ctor = map_constructor.clone();
            let species_getter = NativeFunction::new(move |_args| {
                // Return this (the Map constructor)
                let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
                // If called as a getter, 'this' should be Map constructor
                // If called with explicit this, use it; otherwise use map_ctor
                if matches!(this_val, Value::Undefined) {
                    Ok(map_ctor.clone())
                } else {
                    Ok(this_val)
                }
            });
            // Store the getter - the key is the symbol's string representation
            let species_key = format!("{}", species_sym);
            let _ = nf.set_property(&species_key, Value::NativeFunction(Rc::new(species_getter)));
        }
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
                set_populate(&set, src)?;
            }
        }
        Ok(Value::Object(set))
    });
    if let Value::NativeFunction(nf) = &set_constructor {
        let _ = nf.set_property("prototype", Value::Object(set_proto));
        let _ = nf.set_property("name", Value::String("Set".to_string()));
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

    #[test]
    fn test_map_set_getter_override() {
        // Test that overriding Map.prototype.set with a getter throws
        // when new Map([[...]]) tries to call it (ES spec §24.1.1)
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            r#"
            Object.defineProperty(Map.prototype, 'set', {
              get: function() {
                throw new Error("getter was called");
              }
            });
            "#,
        )
        .unwrap();
        // new Map() with empty arg should NOT throw
        ctx.eval("new Map();").unwrap();
        // new Map with non-empty iterable SHOULD throw
        let result = ctx.eval("new Map([[1, 2]]);");
        assert!(
            result.is_err(),
            "new Map([[1, 2]]) should throw when Map.prototype.set getter throws"
        );
    }

    #[test]
    fn test_set_add_getter_override() {
        // Test that overriding Set.prototype.add with a getter throws
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            r#"
            Object.defineProperty(Set.prototype, 'add', {
              get: function() {
                throw new Error("getter was called");
              }
            });
            "#,
        )
        .unwrap();
        // new Set() with empty arg should NOT throw
        ctx.eval("new Set();").unwrap();
        // new Set with non-empty iterable SHOULD throw
        let result = ctx.eval("new Set([1]);");
        assert!(
            result.is_err(),
            "new Set([1]) should throw when Set.prototype.add getter throws"
        );
    }

    #[test]
    fn test_map_direct_getter() {
        // Direct test: get Map.prototype.set - should it be a getter?
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            r#"
            Object.defineProperty(Map.prototype, 'set', {
              get: function() {
                return 42;
              }
            });
            "#,
        )
        .unwrap();
        let result = ctx.eval("Map.prototype.set");
        // If getter works, this should be 42
        assert_eq!(
            result.unwrap(),
            Value::Number(42.0),
            "Map.prototype.set getter should return 42"
        );
    }

    #[test]
    fn test_map_getter_called_when_populating() {
        // Test that the getter IS called when populating from iterable
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            r#"
            var getterCalled = false;
            Object.defineProperty(Map.prototype, 'set', {
              get: function() {
                getterCalled = true;
                // Return a function that throws to simulate getter throwing
                throw new Error("getter was called");
              }
            });
            "#,
        )
        .unwrap();
        // new Map() should NOT call the getter (empty iterable)
        ctx.eval("new Map();").unwrap();
        let getter_called = ctx.eval("getterCalled").unwrap();
        assert_eq!(
            getter_called,
            Value::Boolean(false),
            "getter should not be called for empty Map()"
        );

        // new Map([[1, 2]]) SHOULD call the getter
        let result = ctx.eval("new Map([[1, 2]]);");
        assert!(
            result.is_err(),
            "new Map([[1, 2]]) should throw when getter throws"
        );

        // Verify getter WAS called
        let getter_called = ctx.eval("getterCalled").unwrap();
        assert_eq!(
            getter_called,
            Value::Boolean(true),
            "getter should be called for non-empty Map()"
        );
    }

    #[test]
    fn test_map_call_getter_directly() {
        // Test that calling the getter directly works
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            r#"
            Object.defineProperty(Map.prototype, 'set', {
              get: function() {
                return function(k, v) { return k + v; };
              }
            });
            "#,
        )
        .unwrap();
        // Call the getter and use the returned function
        let result = ctx.eval("var m = new Map(); var s = Map.prototype.set; s.call(m, 1, 2);");
        assert!(result.is_ok(), "calling getter should work: {:?}", result);
        assert_eq!(
            ctx.eval("var m = new Map(); Map.prototype.set.call(m, 1, 2);")
                .unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn test_map_getter_vs_own_property() {
        // Test: does the Map have its own 'set' property that shadows the getter?
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            r#"
            Object.defineProperty(Map.prototype, 'set', {
              get: function() {
                throw new Error("prototype getter called");
              }
            });
            "#,
        )
        .unwrap();
        // Check if Map has own 'set' property
        let has_own = ctx
            .eval("Object.prototype.hasOwnProperty.call(Map.prototype, 'set');")
            .unwrap();
        assert_eq!(
            has_own,
            Value::Boolean(true),
            "Map.prototype should have own 'set' property"
        );

        // Now test with a map instance
        let result = ctx.eval("var m = new Map(); m.set(1, 2);");
        assert!(
            result.is_err(),
            "calling m.set should throw (getter on prototype): {:?}",
            result
        );
    }

    #[test]
    fn test_map_iterable_parsing() {
        // Test that new Map with iterable populates correctly
        let mut ctx = Context::new().unwrap();

        // Test with a normal iterable (no override)
        ctx.eval("var m = new Map([[1, 2], [3, 4]]);").unwrap();
        assert_eq!(ctx.eval("m.get(1)").unwrap(), Value::Number(2.0));
        assert_eq!(ctx.eval("m.get(3)").unwrap(), Value::Number(4.0));
        assert_eq!(ctx.eval("m.size").unwrap(), Value::Number(2.0));
    }

    #[test]
    fn test_map_with_override() {
        // Test the exact test262 scenario: override Map.prototype.set, then new Map(iterable)
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            r#"
            var mapSet = Map.prototype.set;
            var counter = 0;
            var iterable = [["foo", 1], ["bar", 2]];
            
            Map.prototype.set = function(k, v) {
              counter++;
              mapSet.call(this, k, v);
            };
            
            var map = new Map(iterable);
            "#,
        )
        .unwrap();

        // Verify counter is 2 (called twice)
        let counter = ctx.eval("counter").unwrap();
        assert_eq!(
            counter,
            Value::Number(2.0),
            "Map.prototype.set should be called twice"
        );

        // Verify map has the values
        assert_eq!(ctx.eval("map.get('foo')").unwrap(), Value::Number(1.0));
        assert_eq!(ctx.eval("map.get('bar')").unwrap(), Value::Number(2.0));
    }

    #[test]
    fn test_map_override_is_found() {
        // Check if overridden Map.prototype.set is found
        let mut ctx = Context::new().unwrap();

        // Override Map.prototype.set
        ctx.eval("Map.prototype.set = function() { return 42; };")
            .unwrap();

        // Check if a map instance sees the override
        ctx.eval("var m = new Map();").unwrap();
        let result = ctx.eval("m.set(1, 2);").unwrap();
        assert_eq!(
            result,
            Value::Number(42.0),
            "m.set should return 42 from override"
        );
    }

    #[test]
    fn test_map_empty_iterable_no_getter() {
        // Test that new Map([]) does NOT call the getter (empty array)
        // This is the test262 test scenario
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            r#"
            Object.defineProperty(Map.prototype, 'set', {
              get: function() {
                throw new Error("getter called");
              }
            });
            "#,
        )
        .unwrap();

        // Empty iterable should NOT throw
        let result = ctx.eval("new Map([]);");
        assert!(result.is_ok(), "new Map([]) should not throw: {:?}", result);
    }

    #[test]
    fn test_map_test262_scenario() {
        // Exactly replicate the test262 test scenario
        let mut ctx = Context::new().unwrap();

        // Step 1: Define getter
        ctx.eval(
            r#"
            Object.defineProperty(Map.prototype, 'set', {
              get: function() {
                throw new Test262Error();
              }
            });
            "#,
        )
        .unwrap();

        // Step 2: new Map() should NOT throw
        ctx.eval("new Map();").unwrap();

        // Step 3: new Map([]) SHOULD throw
        let result = ctx.eval("new Map([]);");
        assert!(
            result.is_err(),
            "new Map([]) should throw when getter throws: {:?}",
            result
        );
    }

    #[test]
    fn test_map_is_constructor() {
        // Test that Map is recognized as a constructor
        let mut ctx = Context::new().unwrap();
        // Note: isConstructor is only available in test262 harness
        // Just test that new Map() works
        let result = ctx.eval("new Map()");
        assert!(result.is_ok(), "new Map() should work: {:?}", result);

        // Test that Map() without new also works (implicit call)
        let result = ctx.eval("Map()");
        assert!(result.is_ok(), "Map() should work: {:?}", result);
    }
}
