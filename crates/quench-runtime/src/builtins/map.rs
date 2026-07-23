//! Map and Set built-ins

pub mod helpers;
pub mod set;

use std::cell::RefCell;
use std::rc::Rc;

use self::helpers::{
    init_map_object, iterator_prop_key, map_entries, map_find_pair, map_populate, map_update_size,
    native_fn, set_values,
};
use crate::value::{JsError, Object, ObjectKind, Value};
use crate::Context;

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
            return Ok(pair
                .borrow()
                .elements
                .get(1)
                .cloned()
                .unwrap_or(Value::Undefined));
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
                    return helpers::same_value_zero(k, &key);
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

fn map_iterator_impl(_args: Vec<Value>) -> Result<Value, JsError> {
    let this = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
    let items = map_entries(&this)
        .map(|e| e.borrow().elements.clone())
        .unwrap_or_default();
    Ok(self::helpers::make_iterator(items))
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
        let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
        let map = if let Value::Object(obj_rc) = this_val {
            init_map_object(&obj_rc);
            obj_rc
        } else {
            let map_obj = Object::with_prototype(ObjectKind::Map, Rc::clone(&map_proto_for_ctor));
            let map = Rc::new(RefCell::new(map_obj));
            init_map_object(&map);
            map
        };
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
        if let Some(species_sym) = crate::builtins::symbol::get_well_known_symbol_no_ctx("species")
        {
            let map_ctor = map_constructor.clone();
            let species_getter = NativeFunction::new(move |_args| {
                let this_val = crate::builtins::get_native_this().unwrap_or(Value::Undefined);
                if matches!(this_val, Value::Undefined) {
                    Ok(map_ctor.clone())
                } else {
                    Ok(this_val)
                }
            });
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
    set::register_set(ctx, set_proto);
}

use crate::value::NativeFunction;

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
        ctx.eval("new Map();").unwrap();
        let result = ctx.eval("new Map([[1, 2]]);");
        assert!(
            result.is_err(),
            "new Map([[1, 2]]) should throw when Map.prototype.set getter throws"
        );
    }

    #[test]
    fn test_set_add_getter_override() {
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
        ctx.eval("new Set();").unwrap();
        let result = ctx.eval("new Set([1]);");
        assert!(
            result.is_err(),
            "new Set([1]) should throw when Set.prototype.add getter throws"
        );
    }

    #[test]
    fn test_map_direct_getter() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            r#"
            Object.defineProperty(Map.prototype, 'set', {
              get: function() { return 42; }
            });
            "#,
        )
        .unwrap();
        let result = ctx.eval("Map.prototype.set");
        assert_eq!(
            result.unwrap(),
            Value::Number(42.0),
            "Map.prototype.set getter should return 42"
        );
    }

    #[test]
    fn test_map_getter_called_when_populating() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            r#"
            var getterCalled = false;
            Object.defineProperty(Map.prototype, 'set', {
              get: function() {
                getterCalled = true;
                throw new Error("getter was called");
              }
            });
            "#,
        )
        .unwrap();
        ctx.eval("new Map();").unwrap();
        let getter_called = ctx.eval("getterCalled").unwrap();
        assert_eq!(
            getter_called,
            Value::Boolean(false),
            "getter should not be called for empty Map()"
        );

        let result = ctx.eval("new Map([[1, 2]]);");
        assert!(
            result.is_err(),
            "new Map([[1, 2]]) should throw when getter throws"
        );
        let getter_called = ctx.eval("getterCalled").unwrap();
        assert_eq!(
            getter_called,
            Value::Boolean(true),
            "getter should be called for non-empty Map()"
        );
    }

    #[test]
    fn test_map_call_getter_directly() {
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
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            r#"
            Object.defineProperty(Map.prototype, 'set', {
              get: function() { throw new Error("prototype getter called"); }
            });
            "#,
        )
        .unwrap();
        let has_own = ctx
            .eval("Object.prototype.hasOwnProperty.call(Map.prototype, 'set');")
            .unwrap();
        assert_eq!(
            has_own,
            Value::Boolean(true),
            "Map.prototype should have own 'set' property"
        );
        let result = ctx.eval("var m = new Map(); m.set(1, 2);");
        assert!(
            result.is_err(),
            "calling m.set should throw (getter on prototype): {:?}",
            result
        );
    }

    #[test]
    fn test_map_iterable_parsing() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("var m = new Map([[1, 2], [3, 4]]);").unwrap();
        assert_eq!(ctx.eval("m.get(1)").unwrap(), Value::Number(2.0));
        assert_eq!(ctx.eval("m.get(3)").unwrap(), Value::Number(4.0));
        assert_eq!(ctx.eval("m.size").unwrap(), Value::Number(2.0));
    }

    #[test]
    fn test_map_with_override() {
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
        let counter = ctx.eval("counter").unwrap();
        assert_eq!(
            counter,
            Value::Number(2.0),
            "Map.prototype.set should be called twice"
        );
        assert_eq!(ctx.eval("map.get('foo')").unwrap(), Value::Number(1.0));
        assert_eq!(ctx.eval("map.get('bar')").unwrap(), Value::Number(2.0));
    }

    #[test]
    fn test_map_override_is_found() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("Map.prototype.set = function() { return 42; };")
            .unwrap();
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
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            r#"
            Object.defineProperty(Map.prototype, 'set', {
              get: function() { throw new Error("getter called"); }
            });
            "#,
        )
        .unwrap();
        // Per ES spec 24.1.1.1 step 5: Let adder be Get(map, "set") is called BEFORE checking if iterable is empty
        let result = ctx.eval("new Map([]);");
        assert!(
            result.is_err(),
            "new Map([]) should trigger getter per spec: {:?}",
            result
        );
    }

    #[test]
    fn test_map_test262_scenario() {
        let mut ctx = Context::new().unwrap();
        ctx.eval(
            r#"
            Object.defineProperty(Map.prototype, 'set', {
              get: function() { throw new Test262Error(); }
            });
            "#,
        )
        .unwrap();
        ctx.eval("new Map();").unwrap();
        let result = ctx.eval("new Map([]);");
        assert!(
            result.is_err(),
            "new Map([]) should throw when getter throws: {:?}",
            result
        );
    }

    #[test]
    fn test_map_is_constructor() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("new Map()");
        assert!(result.is_ok(), "new Map() should work: {:?}", result);
        let result = ctx.eval("Map()");
        assert!(result.is_ok(), "Map() should work: {:?}", result);
    }

    #[test]
    fn test_map_subclass_populate_object_entry() {
        let mut ctx = Context::new().unwrap();
        ctx.eval("class M extends Map {} var map = new M([{ 'foo': 'bar' }]);")
            .unwrap();
        assert_eq!(ctx.eval("map.size").unwrap(), Value::Number(1.0));
        ctx.eval("map.set('bar', 'baz');").unwrap();
        assert_eq!(ctx.eval("map.size").unwrap(), Value::Number(2.0));
    }
}
