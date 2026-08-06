use crate::{Context, Value};

fn eval(src: &str) -> Result<Value, crate::value::JsError> {
    Context::new().unwrap().eval(src)
}

#[test]
fn try_without_catch_finally_preserves_try_completion() {
    assert_eq!(eval("1; try { } finally { }").unwrap(), Value::Undefined);
    assert_eq!(
        eval("2; try { 3; } finally { }").unwrap(),
        Value::Number(3.0)
    );
    assert_eq!(eval("4; try { } finally { 5; }").unwrap(), Value::Undefined);
    assert_eq!(
        eval("6; try { 7; } finally { 8; }").unwrap(),
        Value::Number(7.0)
    );
}

#[test]
fn global_environment_ignores_symbol_unscopables() {
    assert_eq!(
        eval("var callCount = 0; Object.defineProperty(this, Symbol.unscopables, { get: function() { callCount++; } }); this.test262 = true; test262; callCount"),
        Ok(Value::Number(0.0))
    );
}

#[test]
fn strict_delete_this_returns_true() {
    assert_eq!(eval("'use strict'; delete this"), Ok(Value::Boolean(true)));
}

#[test]
fn arrow_typeof_arguments_uses_local_function_binding() {
    assert_eq!(
        eval("const f = () => { function arguments() {} return typeof arguments; }; f()"),
        Ok(Value::String("function".to_string()))
    );
}

#[test]
fn direct_eval_resolves_identifier_through_with_environment() {
    assert_eq!(
        eval("var result; (function() { var o = { value: 'str2' }; with (o) { result = eval(\"'str2' === value\"); } }()); result"),
        Ok(Value::Boolean(true))
    );
}

#[test]
fn eval_function_updates_writable_nonconfigurable_global() {
    assert_eq!(
        eval("var initial; Object.defineProperty(this, 'f', { enumerable: true, writable: true, configurable: false }); eval('initial = f; function f() { return 2222; }'); [typeof initial, initial(), Object.getOwnPropertyDescriptor(this, 'f').configurable].join('|')"),
        Ok(Value::String("function|2222|false".to_string()))
    );
}

#[test]
fn direct_eval_in_function_accepts_new_target() {
    assert_eq!(
        eval("var seen = null; var f = function() { seen = eval('new.target;'); }; f(); seen"),
        Ok(Value::Undefined)
    );
}

#[test]
fn switch_case_still_matches_when_default_calls_function() {
    assert_eq!(
        eval("function boxed(value) { return false; } function classify(value) { switch (typeof value) { case 'string': return true; default: return boxed(value); } } classify('a')").unwrap(),
        Value::Boolean(true)
    );
}

#[test]
fn generator_assignment_defers_write_until_yield_resumes() {
    let value = eval("var obj = {foo: 'initial'}; function* g() { obj.foo = yield; } var iter = g(); iter.next(); var before = obj.foo; iter.next('resumed'); [before, obj.foo].join('|')").unwrap();
    assert_eq!(value, Value::String("initial|resumed".into()));
}

#[test]
fn optional_chain_for_update_short_circuits_computed_key() {
    let value = eval("var count = 0; var touched = 0; var obj = { get a() { count++; return undefined; } }; for (count = 0; true; obj?.a?.[touched++]) { if (count > 0) break; } String(count) + ',' + String(touched)").unwrap();
    assert_eq!(value, Value::String("1,0".into()));
}

#[test]
fn dynamic_import_returns_promise_for_missing_module() {
    let value = eval("typeof import('missing-module').then").unwrap();
    assert_eq!(value, Value::String("function".into()));
}

#[test]
fn calling_dynamic_import_promise_throws_type_error() {
    assert!(eval("import('missing-module')()").is_err());
}

#[test]
fn async_await_dynamic_import_rejection_reaches_catch() {
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    ctx.eval(
        "var result = 'pending'; (async function() { await import('missing-module'); })()\
         .catch(error => result = error.name);",
    )
    .unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(
        ctx.eval("result").unwrap(),
        Value::String("TypeError".into())
    );
}

#[test]
fn async_arrow_await_dynamic_import_rejection_reaches_catch() {
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    ctx.eval(
        "var result = 'pending'; (async () => { await import('missing-module'); })()\
         .catch(error => result = error.name);",
    )
    .unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(
        ctx.eval("result").unwrap(),
        Value::String("TypeError".into())
    );
}

#[test]
fn dynamic_import_reuses_module_namespace_object() {
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    ctx.register_module(
        "module-name",
        crate::value::Object::new(crate::value::ObjectKind::Ordinary),
    );
    ctx.eval(
        "var first, second; import('module-name').then(ns => first = ns); import('module-name').then(ns => second = ns);",
    )
    .unwrap();
    assert_eq!(ctx.eval("first === second").unwrap(), Value::Boolean(true));
}

#[test]
fn dynamic_import_of_current_module_exposes_default_function() {
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    ctx.set_global(
        "__quench_current_module__".to_string(),
        Value::String("./self.js".into()),
    );
    ctx.eval_es_module(
        "export default (function() { return 99; }); import('./self.js').then(ns => globalThis.result = ns.default()).catch(error => globalThis.result = error.name);",
    )
    .unwrap();
    assert_eq!(ctx.eval("result").unwrap(), Value::Number(99.0));
}

#[test]
fn dynamic_import_of_current_module_exposes_module_namespace_tag() {
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    ctx.set_global(
        "__quench_current_module__".to_string(),
        Value::String("./self.js".into()),
    );
    ctx.eval_es_module(
        "import * as ns from './self.js'; globalThis.result = Symbol.toStringTag in ns;",
    )
    .unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(ctx.eval("result").unwrap(), Value::Boolean(true));
}

#[test]
fn deferred_dynamic_import_is_not_source_phase_import() {
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    ctx.register_module(
        "module-name",
        crate::value::Object::new(crate::value::ObjectKind::Ordinary),
    );
    ctx.eval("var result; import.defer('module-name').then(() => result = 'ok', error => result = error.name);")
        .unwrap();
    assert_eq!(ctx.eval("result").unwrap(), Value::String("ok".into()));
}

#[test]
fn non_strict_super_set_ignores_failed_receiver_set() {
    let value = eval("var obj = { method() { super.x = 8; Object.freeze(obj); super.y = 9; } }; obj.method(); Object.prototype.hasOwnProperty.call(obj, 'y')").unwrap();
    assert_eq!(value, Value::Boolean(false));
}

#[test]
fn direct_eval_new_target_in_class_field_is_runtime_undefined() {
    let value = eval("var executed = false; var C = class { x = eval('executed = true; new.target;'); }; var c = new C(); [executed, c.x].join('|')").unwrap();
    assert_eq!(value, Value::String("true|".into()));
}

#[test]
fn derived_constructor_arrow_this_before_super_throws() {
    let value = eval("var probe, result; class Base { constructor() { try { probe(); result = false; } catch (e) { result = e instanceof ReferenceError; } } } class C extends Base { field = 1; constructor() { probe = () => this; try { probe(); } catch (e) {} super(); } } new C(); result").unwrap();
    assert_eq!(value, Value::Boolean(true));
}

#[test]
fn setter_function_length_stops_at_default_parameter() {
    assert_eq!(
        eval("Object.getOwnPropertyDescriptor({ set m(x = 42) {} }, 'm').set.length").unwrap(),
        Value::Number(0.0)
    );
}

#[test]
fn anonymous_class_assignment_infers_property_name() {
    assert_eq!(
        eval("var o = { id: class {} }; o.id.name").unwrap(),
        Value::String("id".into())
    );
}

#[test]
fn dynamic_import_missing_module_rejects_with_type_error() {
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    ctx.eval("var result; import('missing-module').then(() => { result = 'fulfilled'; }, error => { result = error.name; });")
        .unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(
        ctx.eval("result").unwrap(),
        Value::String("TypeError".into())
    );
}

#[test]
fn dynamic_import_missing_script_fixture_rejects_with_syntax_error() {
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    ctx.eval("var result; import('./script-code_FIXTURE.js').then(() => { result = 'fulfilled'; }, error => { result = error.name; });")
        .unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(
        ctx.eval("result").unwrap(),
        Value::String("SyntaxError".into())
    );
}

#[test]
fn dynamic_import_specifier_error_rejects_with_original_value() {
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    ctx.eval(
        "var result; import({toString(){throw 'custom error';}}).catch(error => result = error);",
    )
    .unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(
        ctx.eval("result").unwrap(),
        Value::String("custom error".into())
    );
}

#[test]
fn dynamic_import_uses_function_to_string_for_specifier() {
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    crate::builtins::bootstrap::bootstrap_js_builtins(&mut ctx).unwrap();
    let mut exports = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
    exports.set("x", Value::Number(1.0));
    ctx.register_module("./module.js", exports);
    ctx.eval("var result; Function.prototype.toString = () => './module.js'; import(() => {}).then(ns => result = ns.x)")
        .unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(ctx.eval("result").unwrap(), Value::Number(1.0));
}

#[test]
fn dynamic_import_namespace_to_string_tag_has_spec_attributes() {
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    ctx.register_module(
        "module-name",
        crate::value::Object::new(crate::value::ObjectKind::Ordinary),
    );
    ctx.eval("var result; import('module-name').then(ns => { var d = Object.getOwnPropertyDescriptor(ns, Symbol.toStringTag); result = [d.writable, d.enumerable, d.configurable].join('|'); });")
        .unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(
        ctx.eval("result").unwrap(),
        Value::String("false|false|false".into())
    );
}

#[test]
fn dynamic_import_namespace_has_symbol_to_string_tag() {
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    ctx.register_module(
        "module-name",
        crate::value::Object::new(crate::value::ObjectKind::Ordinary),
    );
    ctx.eval(
        "var result; import('module-name').then(ns => { result = Symbol.toStringTag in ns; });",
    )
    .unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(ctx.eval("result").unwrap(), Value::Boolean(true));
}

#[test]
fn dynamic_import_namespace_deletes_non_exported_properties() {
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    ctx.register_module(
        "module-name",
        crate::value::Object::new(crate::value::ObjectKind::Ordinary),
    );
    ctx.eval("var result; import('module-name').then(ns => { result = [Reflect.deleteProperty(ns, 'undef'), Reflect.deleteProperty(ns, 'default'), Reflect.deleteProperty(ns, Symbol.toStringTag), Reflect.deleteProperty(ns, Symbol('x'))].join('|'); });")
        .unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(
        ctx.eval("result").unwrap(),
        Value::String("true|true|false|true".into())
    );
}

#[test]
fn dynamic_import_source_rejects_with_syntax_error() {
    let mut ctx = Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    ctx.eval("var result; import.source('module-name').then(() => { result = 'fulfilled'; }, error => { result = error.name; });")
        .unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(
        ctx.eval("result").unwrap(),
        Value::String("SyntaxError".into())
    );
}

#[test]
fn dynamic_import_with_non_object_options_rejects() {
    let mut ctx = crate::Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    let mut exports = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
    exports.set("default", Value::Number(42.0));
    ctx.register_module("module-name", exports);
    ctx.eval("var result; import('module-name', null).then(() => { result = 'fulfilled'; }, (err) => { result = err instanceof TypeError; })")
        .unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(ctx.eval("result").unwrap(), Value::Boolean(true));
}

#[test]
fn dynamic_import_with_undefined_options_uses_default_attributes() {
    let mut ctx = crate::Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    let mut exports = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
    exports.set("default", Value::Number(42.0));
    ctx.register_module("module-name", exports);
    ctx.eval("var result; import('module-name', undefined).then((ns) => { result = ns.default; })")
        .unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(ctx.eval("result").unwrap(), Value::Number(42.0));
}

#[test]
fn dynamic_import_with_non_string_attribute_value_rejects() {
    let mut ctx = crate::Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    let mut exports = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
    exports.set("default", Value::Number(42.0));
    ctx.register_module("module-name", exports);
    ctx.eval("var result; import('module-name', { with: { type: 7 } }).then(() => { result = 'fulfilled'; }, (err) => { result = err instanceof TypeError; })")
        .unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(ctx.eval("result").unwrap(), Value::Boolean(true));
}

#[test]
fn dynamic_import_with_non_enumerable_type_and_proxy_own_keys_ignores_attribute() {
    let mut ctx = crate::Context::new().unwrap();
    crate::builtins::register_builtins(&mut ctx);
    let mut exports = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
    exports.set("default", Value::Number(42.0));
    ctx.register_module("module-name", exports);

    let script = "var result;\n\
var withProxy = new Proxy({}, {\n\
  ownKeys() { return ['type']; },\n\
  get() { return 'text'; },\n\
  getOwnPropertyDescriptor(_target, _key) { return { configurable: true, enumerable: false }; }\n\
});\n\
import('module-name', { with: withProxy }).then((ns) => { result = ns.default; }, () => { result = 'rejected'; });";
    ctx.eval(script).unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(ctx.eval("result").unwrap(), Value::Number(42.0));
}

#[test]
fn dynamic_import_namespace_rejects_set() {
    let mut ctx = crate::Context::new().unwrap();
    let mut exports = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
    exports.set("x", Value::Number(1.0));
    ctx.register_module("module-name", exports);
    ctx.eval("var result; import('module-name').then(ns => result = Reflect.set(ns, 'x', 2));")
        .unwrap();
    assert_eq!(ctx.eval("result").unwrap(), Value::Boolean(false));
    ctx.eval("var deleted; import('module-name').then(ns => deleted = Reflect.deleteProperty(ns, 'x')); ")
        .unwrap();
    assert_eq!(ctx.eval("deleted").unwrap(), Value::Boolean(false));
    ctx.eval("var after; import('module-name').then(ns => { ns.x = 2; after = ns.x; });")
        .unwrap();
    assert_eq!(ctx.eval("after").unwrap(), Value::Number(1.0));
}

#[test]
fn dynamic_import_namespace_strict_assignment_throws() {
    let mut ctx = crate::Context::new().unwrap();
    let mut exports = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
    exports.set("x", Value::Number(1.0));
    ctx.register_module("module-name", exports);
    ctx.eval("'use strict'; var result; import('module-name').then(ns => { try { ns.x = 2; result = false; } catch (e) { result = e instanceof TypeError; } });")
        .unwrap();
    assert_eq!(ctx.eval("result").unwrap(), Value::Boolean(true));
}

#[test]
fn dynamic_import_namespace_descriptor_has_spec_fields() {
    let mut ctx = crate::Context::new().unwrap();
    let mut exports = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
    exports.set("x", Value::String("value".into()));
    ctx.register_module("module-name", exports);
    ctx.eval("var result; import('module-name').then(ns => { var d = Object.getOwnPropertyDescriptor(ns, 'x'); result = [d.value, d.enumerable, d.writable, d.configurable]; });")
        .unwrap();
    assert_eq!(
        ctx.eval("JSON.stringify(result)").unwrap(),
        Value::String("[\"value\",true,true,false]".into())
    );
}

#[test]
fn dynamic_import_namespace_descriptor_reads_live_binding() {
    let mut ctx = crate::Context::new().unwrap();
    let mut exports = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
    let getter = crate::value::Value::NativeFunction(std::rc::Rc::new(
        crate::value::NativeFunction::new(|_| Err(crate::value::error::JsError::new("boom"))),
    ));
    exports.define_accessor(
        "x",
        Some(getter),
        None,
        crate::value::PropertyFlags {
            value: None,
            writable: false,
            enumerable: true,
            configurable: false,
        },
    );
    ctx.register_module("module-name", exports);
    ctx.eval("var result; import('module-name').then(ns => { try { Object.getOwnPropertyDescriptor(ns, 'x'); result = false; } catch (_) { result = true; } });").unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(
        ctx.eval("result").unwrap(),
        crate::value::Value::Boolean(true)
    );
}

#[test]
fn dynamic_import_namespace_has_own_descriptor_reads_live_binding() {
    let mut ctx = crate::Context::new().unwrap();
    let mut exports = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
    let getter = crate::value::Value::NativeFunction(std::rc::Rc::new(
        crate::value::NativeFunction::new(|_| Err(crate::value::error::JsError::new("boom"))),
    ));
    exports.define_accessor(
        "x",
        Some(getter),
        None,
        crate::value::PropertyFlags {
            value: None,
            writable: false,
            enumerable: true,
            configurable: false,
        },
    );
    ctx.register_module("module-name", exports);
    ctx.eval("var result; import('module-name').then(ns => { try { Object.prototype.hasOwnProperty.call(ns, 'x'); result = false; } catch (_) { result = true; } });").unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(
        ctx.eval("result").unwrap(),
        crate::value::Value::Boolean(true)
    );
}

#[test]
fn dynamic_import_namespace_own_keys_are_sorted_and_immutable() {
    let mut ctx = crate::Context::new().unwrap();
    let mut exports = crate::value::Object::new(crate::value::ObjectKind::Ordinary);
    exports.set("z", Value::Number(1.0));
    exports.set("a", Value::Number(2.0));
    ctx.register_module("module-name", exports);
    ctx.eval("var result; import('module-name').then(ns => { result = [Object.keys(ns).join(','), Reflect.ownKeys(ns).map(String).join(','), Reflect.deleteProperty(ns, 'z'), Reflect.set(ns, 'z', 3)].join('|'); });").unwrap();
    crate::builtins::promise::execute_pending_microtasks().unwrap();
    assert_eq!(
        ctx.eval("result").unwrap(),
        Value::String("a,z|a,z,Symbol(Symbol.toStringTag)|false|false".into())
    );
}

mod return_statement {
    use super::*;

    #[test]
    fn return_with_value() {
        assert_eq!(
            eval("function f() { return 42; } f()").unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn return_without_value() {
        assert_eq!(
            eval("function f() { return; } f()").unwrap(),
            Value::Undefined
        );
    }

    #[test]
    fn static_getter_tail_return_super_not_deferred_without_trampoline() {
        assert_eq!(
            eval(
                "class B { static m() { return 1; } } \
                 class C extends B { static get x() { 0; return super.m(); } } \
                 C.x"
            )
            .unwrap(),
            Value::Number(1.0)
        );
    }
}

mod const_decl {
    use super::*;

    #[test]
    fn const_fn_cover_grammar_does_not_set_function_name() {
        let r = eval(
            "const xCover = (0, function() {}); \
             const cover = (function() {}); \
             cover.name + '|' + xCover.name",
        )
        .unwrap();
        assert_eq!(r, Value::String("cover|".into()));
    }

    #[test]
    fn for_of_const_increment_throws_type_error() {
        let r =
            eval("try { for (const x of [1, 2, 3]) { x++ } 'ok'; } catch (e) { e.name }").unwrap();
        assert_eq!(r, Value::String("TypeError".into()));
    }
}

mod labeled_continue {
    use super::*;

    #[test]
    fn labeled_continue_to_for_from_inner_while() {
        let r = eval(
            "var count = 0; \
             label: for (let x = 0; x < 10;) { \
               while (true) { x++; count++; continue label; } \
             } \
             count",
        )
        .unwrap();
        assert_eq!(r, Value::Number(10.0));
    }
}

mod throw_statement {
    use super::*;

    #[test]
    fn throw_propagates() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("throw 42");
        assert!(result.is_err());
    }
}

mod var_declarations_misc {
    use super::*;

    #[test]
    fn var_declaration_with_resolves_to_binding_not_with_object() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(
            "var obj = { test262id: 1 }; \
             with (obj) { var test262id = delete obj.test262id; } \
             if (obj.test262id !== true || test262id !== undefined) { throw new Error('binding mismatch'); }",
        );
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn var_decl_no_initializer_preserves_existing_global_property() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval(
            "this['__declared__var'] = 'baloon'; \
             var __declared__var; \
             __declared__var",
        );
        assert_eq!(result.unwrap(), Value::String("baloon".into()));

        let result = ctx.eval(
            "\"use strict\"; \
             this['__declared__var'] = 'baloon'; \
             var __declared__var; \
             __declared__var",
        );
        assert_eq!(result.unwrap(), Value::String("baloon".into()));
    }

    #[test]
    fn var_decl_assignment_class_name_is_inferred_on_assignment() {
        let value =
            eval("cls = class {}; Object.getOwnPropertyDescriptor(cls, \"name\").value").unwrap();
        assert_eq!(value, Value::String("cls".to_string()));

        let writable =
            eval("cls = class {}; Object.getOwnPropertyDescriptor(cls, \"name\").writable")
                .unwrap();
        assert_eq!(writable, Value::Boolean(false));
    }

    #[test]
    fn var_decl_named_class_keeps_explicit_name() {
        assert_eq!(
            eval("var xCls = class xCls {}; xCls.name").unwrap(),
            Value::String("xCls".into())
        );
    }

    #[test]
    fn const_init_reads_previous_const_binding_without_tdz() {
        let result =
            eval("const first = [1, 2, 3];\nconst second = first.concat(4);\nsecond.length");
        assert_eq!(result.unwrap(), Value::Number(4.0));
    }
}

mod with_statement {
    use super::*;

    #[test]
    fn with_object_non_object_throws_type_error() {
        let result = eval("try { with(null) x = 2; } catch (e) { e.name }");
        assert_eq!(result.unwrap(), Value::String("TypeError".to_string()));
    }

    #[test]
    fn with_property_lookup_is_dynamic_and_delete_updates_object() {
        let result = eval(
            "var myObj = { p1: 'a' };\n            with(myObj) {\n              delete p1;\n            }\n            myObj.p1;",
        );
        assert_eq!(result.unwrap(), Value::Undefined);
    }

    #[test]
    fn with_var_declarations_are_hoisted_to_function_scope() {
        let result = eval(
            "function f(){\n\
             try {\n\
             with ({}) { throw 0; var p4 = 'x4'; }\n\
             } catch (e) {}\n\
             return p4;\n\
             }\n\
             f();",
        );
        assert_eq!(result.unwrap(), Value::Undefined);
    }

    #[test]
    fn with_var_declarations_are_hoisted_to_global_scope() {
        let result = eval(
            "try {\n\
             with ({}) { throw 0; var p4 = 'x4'; }\n\
             } catch (e) {}\n\
             p4;",
        );
        assert_eq!(result.unwrap(), Value::Undefined);
    }

    #[test]
    fn with_empty_object_falls_through_to_outer_binding() {
        let result = eval("var count = 0; with ({}) { count++; } count").unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn strict_closure_inside_with_falls_through_to_outer_binding() {
        let result =
            eval("var count = 0; with ({}) { (function() { 'use strict'; count++; })(); } count")
                .unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn getter_can_delete_its_own_property() {
        let result =
            eval("var o = { get x() { delete this.x; return 2; } }; o.x; o.hasOwnProperty('x');");
        assert_eq!(result, Ok(Value::Boolean(false)));
    }

    #[test]
    fn with_getter_receives_with_object_as_this() {
        let result = eval(
            "var o = { get x() { delete this.x; return 2; } }; with (o) { x; } o.hasOwnProperty('x');",
        );
        assert_eq!(result, Ok(Value::Boolean(false)));
    }

    #[test]
    fn strict_compound_assignment_after_with_getter_deletion_throws() {
        let result = eval(
            "var count = 0; var scope = { get x() { delete this.x; return 2; } }; with (scope) { (function() { 'use strict'; try { count++; x += 1; count++; } catch (e) { if (!(e instanceof ReferenceError)) throw e; } count++; })(); } count",
        )
        .unwrap();
        assert_eq!(result, Value::Number(2.0));
    }

    #[test]
    fn strict_arrow_callback_inside_with_preserves_outer_count() {
        let result = eval(
            "var count = 0; var scope = { get x() { delete this.x; return 2; } }; with (scope) { (function() { 'use strict'; try { (() => { count++; x += 1; count++; })(); } catch (e) { if (!(e instanceof ReferenceError)) throw e; } count++; })(); } count",
        )
        .unwrap();
        assert_eq!(result, Value::Number(2.0));
    }

    #[test]
    fn with_assignment_retains_deleted_object_reference() {
        let result = eval(
            "function f() { var x = 0; var scope = { x: 1 }; with (scope) { x = (delete scope.x, 2); } return [scope.x, x]; } f().join(',');",
        );
        assert_eq!(result, Ok(Value::String("2,0".to_string())));
    }

    #[test]
    fn sloppy_unresolvable_assignment_creates_global_object_property() {
        let result = eval(
            "function f() { implicit_global_for_test = 42; } f(); Object.getOwnPropertyDescriptor(this, 'implicit_global_for_test').value;",
        );
        assert_eq!(result, Ok(Value::Number(42.0)));
    }

    #[test]
    fn object_spread_preserves_own_key_order() {
        let result = eval(
            "var calls = []; var o = { get z() { calls.push('z'); }, get a() { calls.push('a'); } }; Object.defineProperty(o, 1, { get: () => { calls.push(1); }, enumerable: true }); Object.defineProperty(o, Symbol('foo'), { get: () => { calls.push('Symbol(foo)'); }, enumerable: true }); ({ ...o }); calls.join(',');",
        );
        assert_eq!(result, Ok(Value::String("1,z,a,Symbol(foo)".to_string())));
    }

    #[test]
    fn sloppy_named_function_name_assignment_is_ignored() {
        let result = eval(
            "var ref = function named() { (() => { named = 1; })(); return named; }; ref() === ref",
        );
        assert_eq!(result, Ok(Value::Boolean(true)));
    }

    #[test]
    fn object_literal_arrow_functions_receive_property_names() {
        let result = eval(
            "var s = Symbol('test'); var a = Symbol(); var o = { id: () => {}, [a]: () => {}, [s]: () => {} }; [o.id.name, o[a].name, o[s].name].join('|');",
        );
        assert_eq!(result, Ok(Value::String("id||[test]".to_string())));
    }

    #[test]
    fn object_literal_proto_property_sets_internal_prototype() {
        let result = eval(
            "var proto = {}; var o = { __proto__: proto }; Object.getPrototypeOf(o) === proto;",
        );
        assert_eq!(result, Ok(Value::Boolean(true)));
    }

    #[test]
    fn computed_proto_property_does_not_set_internal_prototype() {
        let result = eval(
            "var proto = {}; var o = { ['__proto__']: proto }; [o.hasOwnProperty('__proto__'), o['__proto__'] === proto].join(',');",
        );
        assert_eq!(result, Ok(Value::String("true,true".to_string())));
    }

    #[test]
    fn strict_assignment_to_global_undefined_throws() {
        let result = eval("'use strict'; var global = this; global.undefined = 42;");
        assert!(result.is_err());
    }

    #[test]
    fn strict_unresolvable_assignment_throws_reference_error() {
        let mut ctx = Context::new().unwrap();
        crate::interpreter::set_strict_mode(true);
        let result = ctx.eval("'use strict'; undeclared = (this.undeclared = 5);");
        crate::interpreter::set_strict_mode(false);
        assert!(result.is_err());
    }

    #[test]
    fn strict_assignment_to_global_constant_throws_type_error() {
        let result = eval("'use strict'; undefined = 12;");
        assert!(result.unwrap_err().0.contains("TypeError"));
    }

    #[test]
    fn object_spread_copies_symbol_value() {
        let result =
            eval("var s = Symbol('s'); var o = {}; o[s] = 1; var copy = { ...o }; copy[s];");
        assert_eq!(result, Ok(Value::Number(1.0)));
    }

    #[test]
    fn with_compound_assignment_retains_deleted_object_reference() {
        let result = eval(
            "var x = 0; var scope = { get x() { delete this.x; return 2; } }; with (scope) { x ^= 3; } [scope.x, x].join(',');",
        );
        assert_eq!(result, Ok(Value::String("1,0".to_string())));
    }

    #[test]
    fn with_var_assignment_after_unscopables_side_effect_recreates_binding_as_configurable_property(
    ) {
        let result = eval(
            "var env = { binding: 0 };\n\
             Object.defineProperty(env, Symbol.unscopables, {\n\
               get() {\n\
                 delete env.binding;\n\
                 return {};\n\
               }\n\
             });\n\
             with (env) {\n\
               binding = 123;\n\
             }\n\
             Object.getOwnPropertyDescriptor(env, 'binding').configurable",
        );
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn with_unscopables_blocks_truthy_non_boolean_values() {
        let result = eval(
            "var x = 1;\n\
             var env = { x: 2 };\n\
             env[Symbol.unscopables] = { x: true };\n\
             with (env) { x = 0; }\n\
             var first = x === 0;\n\
             env[Symbol.unscopables].x = 'string';\n\
             with (env) { x = 0; }\n\
             var second = x === 0;\n\
             env[Symbol.unscopables].x = 86;\n\
             with (env) { x = 0; }\n\
             var third = x === 0;\n\
             first && second && third",
        );
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn with_proxy_compound_assignment_uses_one_has_for_get_binding_value() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let result = ctx.eval(
            r#"var log=[]; var env={p:0}; var proxy=new Proxy(env,{has(t,k){log.push('has:'+String(k));return Reflect.has(t,k)},get(t,k,r){log.push('get:'+String(k));return Reflect.get(t,k,r)},set(t,k,v,r){log.push('set:'+String(k));return Reflect.set(t,k,v,r)},getOwnPropertyDescriptor(t,k){log.push('getOwnPropertyDescriptor:'+String(k));return Reflect.getOwnPropertyDescriptor(t,k)},defineProperty(t,k,d){log.push('defineProperty:'+String(k));return Reflect.defineProperty(t,k,d)}}); with(proxy){p+=1} log.join(',')"#,
        );
        assert_eq!(
            result.unwrap(),
            Value::String("has:p,get:Symbol(Symbol.unscopables),has:p,get:p,has:p,set:p,getOwnPropertyDescriptor:p,defineProperty:p".into()),
        );
    }

    #[test]
    fn with_deleted_typed_array_binding_assigns_without_recreating_property() {
        let result = eval(
            "var typedArray = new Int32Array(10); var env = Object.create(typedArray); Object.defineProperty(env, 'NaN', { configurable: true, value: 100 }); with (env) { NaN = (delete env.NaN, 0); } Object.getOwnPropertyDescriptor(env, 'NaN') === undefined",
        );
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn strict_with_deleted_typed_array_binding_throws_reference_error() {
        let result = eval(
            "var typedArray = new Int32Array(10); var env = Object.create(typedArray); Object.defineProperty(env, 'NaN', { configurable: true, value: 100 }); with (env) { (function() { 'use strict'; NaN = (delete env.NaN, 0); })(); }",
        );
        assert!(result.is_err_and(|error| error.0.contains("ReferenceError")));
    }

    #[test]
    fn with_proxy_binding_object_lookup_follows_proxy_get_for_call_expression() {
        let result = eval(
            "var log = [];\n\
             var env = { Object };\n\
             var proxy = new Proxy(env, {\n\
              has(t, pk) { log.push('has:' + String(pk)); return Reflect.has(t, pk); },\n\
              get(t, pk, r) { log.push('get:' + String(pk)); return t[pk]; },\n\
             });\n\
             with (proxy) { Object(); }\n\
             log.join(',');",
        );
        assert_eq!(
            result.unwrap(),
            Value::String(
                "has:Object,get:Symbol(Symbol.unscopables),has:Object,get:Object".to_string(),
            ),
        );
    }

    #[test]
    fn with_proxy_binding_missing_name_still_performs_has_binding() {
        let result = eval(
            "var log = [];\n\
             var env = {};\n\
             var proxy = new Proxy(env, {\n\
              has(t, pk) { log.push('has:' + String(pk)); return pk in t; },\n\
              get(t, pk, r) { log.push('get:' + String(pk)); return t[pk]; },\n\
             });\n\
             with (proxy) { Object; }\n\
             log.join(',');",
        );
        assert_eq!(result.unwrap(), Value::String("has:Object".to_string()));
    }

    #[test]
    fn with_unscopable_name_falls_through_to_initialized_var() {
        let result = eval(
            "globalThis.v = 1; globalThis[Symbol.unscopables] = { v: true }; \
             var r; \
             function p() { var v = 10; with (globalThis) { r = v; } } \
             p(); r",
        );
        assert_eq!(result.unwrap(), Value::Number(10.0));
    }

    #[test]
    fn with_unscopable_name_falls_through_to_declared_only_var() {
        let result = eval(
            "globalThis.v = 1; globalThis[Symbol.unscopables] = { v: true }; \
             var observed = 1; \
             function p() { with (globalThis) { observed = v; } var v = 10; } \
             p(); observed",
        );
        assert_eq!(result.unwrap(), Value::Undefined);
    }

    #[test]
    fn with_unscopables_false_falls_through_to_global_property() {
        let result = eval(
            "globalThis.w = 7; globalThis[Symbol.unscopables] = { w: false }; \
             var rw; \
             function q() { with (globalThis) { rw = w; } } \
             q(); rw",
        );
        assert_eq!(result.unwrap(), Value::Number(7.0));
    }
}

mod class_static_properties {
    use super::*;

    #[test]
    fn class_name_property_is_not_writable_in_strict_mode() {
        let err = eval("\"use strict\"; var cls = class {}; cls.name = 'q';");
        assert!(err.is_err(), "strict class static name write must throw");
    }

    #[test]
    fn class_name_property_is_not_writable_in_sloppy_mode() {
        let result = eval("var cls = class {}; cls.name = 'q'; cls.name;");
        assert_eq!(result.unwrap(), Value::String("cls".to_string()));
    }
}

mod try_statement {
    use super::*;

    #[test]
    fn catch_array_destructuring_default_unresolvable_reference_throws() {
        let err = eval("try { throw []; } catch ([x = unresolvableReference]) {}");
        assert!(err.is_err());
        let message = format!("{err:?}");
        assert!(message.contains("ReferenceError"));
    }

    #[test]
    fn catch_object_destructuring_default_unresolvable_reference_throws() {
        let err = eval("try { throw {}; } catch ({x = unresolvableReference}) {}");
        assert!(err.is_err());
        let message = format!("{err:?}");
        assert!(message.contains("ReferenceError"));
    }

    #[test]
    fn catch_throw_runs_finally() {
        let value = eval(
            "var count = { catch: 0, finally: 0 };\n\
             var fn = function() {\n\
               try {\n\
                 throw 'try';\n\
               } catch (e) {\n\
                 count.catch += 1;\n\
                 throw 'catch';\n\
               } finally {\n\
                 count.finally += 1;\n\
                 'finally';\n\
               }\n\
             };\n\
             try {\n\
               fn();\n\
             } catch (_) {}\n\
             count.finally",
        )
        .unwrap();
        assert_eq!(value, Value::Number(1.0));
    }

    #[test]
    fn catch_array_destructuring_binds_values() {
        let value = eval("var x = 0; try { throw [42]; } catch ([y]) { x = y; } x").unwrap();
        assert_eq!(value, Value::Number(42.0));
    }
}

mod break_continue {
    use super::*;

    #[test]
    fn break_exits_loop() {
        assert_eq!(
            eval("let i = 0; while (true) { i++; if (i > 2) break; } i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn continue_skips_iteration() {
        assert_eq!(
            eval("let i = 0, j = 0; while (i < 3) { i++; if (i === 2) continue; j++; } j").unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn break_with_label_exits_labeled_loop() {
        assert_eq!(
            eval("let i = 0; LABEL: while (true) { i++; if (i > 2) break LABEL; } i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn break_label_in_eval_throws_when_label_is_outer() {
        // eval("break LABEL") where LABEL is defined OUTSIDE the eval should throw
        // SyntaxError per ES §13.12.1 (BreakStatement evaluation).
        let result =
            eval("var x = 0, y = 0; LABEL: do { x++; eval('break LABEL'); y++; } while(false); x");
        assert!(
            result.is_err(),
            "break LABEL in eval pointing to outer label should throw SyntaxError"
        );
    }

    #[test]
    fn continue_label_in_eval_throws_when_label_is_outer() {
        // eval("continue LABEL") where LABEL is defined OUTSIDE the eval should throw.
        let result = eval("var x = 0; LABEL: while (x < 3) { x++; eval('continue LABEL'); }");
        assert!(
            result.is_err(),
            "continue LABEL in eval pointing to outer label should throw SyntaxError"
        );
    }

    #[test]
    fn labeled_continue_to_outer_loop() {
        // Bug fix: continue LABEL should break out of inner loop, not continue it infinitely
        assert_eq!(
            eval(
                "let i = 0; OUTER: while (i < 3) {
                   i++;
                   INNER: while (true) { continue OUTER; }
                 } i"
            )
            .unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn labeled_continue_with_for_loop() {
        // continue LABEL targeting outer for loop should work
        assert_eq!(
            eval(
                "let result = 0; OUTER: for (let i = 0; i < 3; i++) {
                   INNER: for (let j = 0; j < 3; j++) {
                     if (j === 1) continue OUTER;
                     result++;
                   }
                 } result"
            )
            .unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn labeled_continue_in_do_while() {
        // continue LABEL targeting outer do-while should work
        assert_eq!(
            eval(
                "let i = 0; OUTER: do {
                   i++;
                   INNER: while (true) { continue OUTER; }
                 } while (i < 3); i"
            )
            .unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn unlabeled_continue_still_works() {
        // Regular (unlabeled) continue must still work
        assert_eq!(
            eval("let i = 0, j = 0; while (i < 3) { i++; if (i === 2) continue; j++; } j").unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn break_label_in_eval_works_when_label_is_inner() {
        // break LABEL where LABEL is defined WITHIN the eval should work.
        assert_eq!(
            eval("eval('LABEL: while(true) { break LABEL; }')").unwrap(),
            Value::Undefined
        );
    }

    #[test]
    fn break_unknown_label_in_eval_throws() {
        // break to a label that doesn't exist anywhere should throw SyntaxError.
        let result = eval("eval('break NOSUCH')");
        assert!(
            result.is_err(),
            "break to undefined label should throw SyntaxError"
        );
    }

    #[test]
    fn continue_unknown_label_in_eval_throws() {
        // continue to a label that doesn't exist anywhere should throw SyntaxError.
        let result = eval("eval('continue NOSUCH')");
        assert!(
            result.is_err(),
            "continue to undefined label should throw SyntaxError"
        );
    }

    #[test]
    fn catch_binding_does_not_alias_outer_scope_this_target() {
        let r = eval(
            "var res1 = false;
            var res2 = false;
            var res3 = false;
            (function() {
              var x_12_14_13 = 'local';
              function foo() { this.x_12_14_13 = 'instance'; }
              try {
                throw foo;
              } catch (e) {
                res1 = (x_12_14_13 === 'local');
                e();
                res2 = (x_12_14_13 === 'local');
              }
              res3 = (x_12_14_13 === 'local');
            })();
            res1 && res2 && res3",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }
}

mod block_statement {
    use super::*;

    #[test]
    fn block_creates_scope() {
        assert_eq!(
            eval("{ let x = 1; } typeof x").unwrap(),
            Value::String("undefined".into())
        );
    }

    #[test]
    fn var_hoisted_outside_block() {
        assert_eq!(eval("{ var x = 1; } x").unwrap(), Value::Number(1.0));
    }
}

mod switch_statement {
    use super::*;

    #[test]
    fn switch_scope_lex_default_async_function_is_block_scoped() {
        assert!(eval("switch (0) { default: async function x() {} } x").is_err());
    }

    #[test]
    fn switch_scope_lex_open_case_uses_outer_lexical_for_discriminant() {
        let src = "let x = 'outside';\nvar probeExpr, probeSelector, probeStmt;\n".to_owned()
            + "switch (probeExpr = function() { return x; }, null) {\n"
            + "  case probeSelector = function() { return x; }, null:\n"
            + "    probeStmt = function() { return x; };\n"
            + "    let x = 'inside';\n"
            + "}\n"
            + "probeExpr() + '|' + probeSelector() + '|' + probeStmt();";
        let r = eval(&src).unwrap();
        assert_eq!(r, Value::String("outside|inside|inside".into()));
    }

    #[test]
    fn switch_scope_lex_open_default_uses_outer_lexical_for_discriminant() {
        let src = "let x = 'outside';\nvar probeExpr, probeStmt;\n".to_owned()
            + "switch (probeExpr = function() { return x; }) {\n"
            + "  default:\n"
            + "    probeStmt = function() { return x; };\n"
            + "    let x = 'inside';\n"
            + "}\n"
            + "probeExpr() + '|' + probeStmt();";
        let r = eval(&src).unwrap();
        assert_eq!(r, Value::String("outside|inside".into()));
    }

    #[test]
    fn switch_tail_call_can_run_many_iterations() {
        let src = "var callCount = 0;\n".to_owned()
            + "(function f(n) {\n"
            + "  if (n === 0) {\n"
            + "    callCount += 1;\n"
            + "    return;\n"
            + "  }\n"
            + "  switch(0) { case 0: return f(n - 1); }\n"
            + "})(1000);\n"
            + "callCount;";
        let r = eval(&src).unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn switch_tail_call_with_default_can_run_many_iterations() {
        let src = "var callCount = 0;\n".to_owned()
            + "(function f(n) {\n"
            + "  if (n === 0) { callCount += 1; return; }\n"
            + "  switch(0) { case 0: return f(n - 1); default: }\n"
            + "})(10000); callCount;";
        assert_eq!(eval(&src).unwrap(), Value::Number(1.0));
    }

    #[test]
    fn switch_default_abrupt_empty_does_not_preserve_prior_completion() {
        assert_eq!(
            eval(
                "1; \
                 switch ('a') {\n\
                   case 'a':\n\
                     break;\n\
                   default:\n\
                 }",
            )
            .unwrap(),
            Value::Undefined
        );
    }

    #[test]
    fn nested_switch_statements_follow_case_completion_rules() {
        let src = "function SwitchTest(value){
  var result = 0;
  switch(value) {
    case 0:
      switch(value) {
        case 0:
         result += 3;
        break;
        default:
          result += 32;
          break;
        }
      result *= 2;
      break;
    default:
      result += 32;
      break;
  }
  return result;
}

SwitchTest(0);";
        assert_eq!(eval(src).unwrap(), Value::Number(6.0));
    }

    #[test]
    fn nested_switch_12_11_a4_t1_matches_expected_result() {
        let src = "function SwitchTest(value){\n".to_owned()
            + "  var result = 0;\n"
            + "  switch(value) {\n"
            + "    case 0:\n"
            + "      switch(value) {\n"
            + "        case 0:\n"
            + "         result += 3;\n"
            + "        break;\n"
            + "        default:\n"
            + "          result += 32;\n"
            + "          break;\n"
            + "        }\n"
            + "      result *= 2;\n"
            + "      break;\n"
            + "    default:\n"
            + "      result += 32;\n"
            + "      break;\n"
            + "  }\n"
            + "  return result;\n"
            + "}\n"
            + "var x = SwitchTest(0);\n"
            + "if (x !== 6) { throw x; }\n"
            + "x;";
        let value = eval(&src);
        assert_eq!(value.unwrap(), Value::Number(6.0));
    }
}

mod empty_statement {
    use super::*;

    #[test]
    fn empty_returns_undefined() {
        assert_eq!(eval(";").unwrap(), Value::Undefined);
    }

    #[test]
    fn empty_does_not_override_previous_completion() {
        assert_eq!(eval("2;;").unwrap(), Value::Number(2.0));
        assert_eq!(eval("3;;;").unwrap(), Value::Number(3.0));
    }
}

mod var_declarations {
    use super::*;

    #[test]
    fn let_declaration() {
        assert_eq!(eval("let x = 5; x").unwrap(), Value::Number(5.0));
    }

    #[test]
    fn const_declaration() {
        assert_eq!(eval("const x = 7; x").unwrap(), Value::Number(7.0));
    }

    #[test]
    fn var_declaration() {
        assert_eq!(eval("var x = 3; x").unwrap(), Value::Number(3.0));
    }
}

mod if_statement {
    use super::*;

    #[test]
    fn if_branch_taken() {
        assert_eq!(eval("if (true) 1").unwrap(), Value::Number(1.0));
    }

    #[test]
    fn else_branch_taken() {
        assert_eq!(eval("if (false) 0; else 2").unwrap(), Value::Number(2.0));
    }

    #[test]
    fn if_without_else_returns_undefined() {
        assert_eq!(eval("if (false) 1").unwrap(), Value::Undefined);
    }
}

mod while_statement {
    use super::*;

    #[test]
    fn basic_while_loop() {
        assert_eq!(
            eval("let i = 0; while (i < 3) { i++; } i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn while_with_break() {
        assert_eq!(
            eval("let i = 0; while (true) { i++; if (i >= 2) break; } i").unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn while_with_continue() {
        assert_eq!(
            eval("let i = 0, c = 0; while (i < 3) { i++; if (i < 2) continue; c++; } c").unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn while_never_executes() {
        assert_eq!(
            eval("let x = 5; while (false) { x = 10; } x").unwrap(),
            Value::Number(5.0)
        );
    }
}

mod for_statement {
    use super::*;

    #[test]
    fn for_with_var_init() {
        assert_eq!(
            eval("for (var i = 0; i < 3; i++); i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn for_with_let_init() {
        // Verify loop body executes
        assert_eq!(
            eval("let sum = 0; for (let j = 0; j < 3; j++) { sum++; } sum").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn for_let_prefix_inc_terminates() {
        // Regression: `for (let i = 0; i < 1; ++i)` must terminate
        assert_eq!(
            eval("let sum = 0; for (let i = 0; i < 1; ++i) { sum++; } sum").unwrap(),
            Value::Number(1.0)
        );
    }

    #[test]
    fn for_let_prefix_inc_three_iter() {
        // Regression: `for (let i = 0; i < 3; ++i)` must terminate
        assert_eq!(
            eval("let sum = 0; for (let i = 0; i < 3; ++i) { sum++; } sum").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn for_with_expression_init() {
        assert_eq!(
            eval("let i = 0; for (i++; i < 3; i++); i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn for_without_condition() {
        assert_eq!(
            eval("let i = 0; for (;;) { i++; if (i > 2) break; } i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn for_with_break_continue() {
        assert_eq!(
            eval("let sum = 0; for (let i = 0; i < 5; i++) { if (i === 2) continue; sum++; } sum")
                .unwrap(),
            Value::Number(4.0)
        );
    }
}

mod try_catch_statement {
    use super::*;

    #[test]
    fn try_succeeds() {
        assert_eq!(
            eval("try { 1 } catch (e) { 2 }").unwrap(),
            Value::Number(1.0)
        );
    }

    #[test]
    fn catch_binds_error() {
        assert_eq!(
            eval("try { throw 42; } catch (e) { e }").unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn catch_guards_body() {
        // Verify catch runs after throw and can modify outer scope
        assert_eq!(
            eval("let x = 1; try { throw 2; } catch (e) { x = e; } x").unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn catch_param_shadows() {
        assert_eq!(
            eval("let x = 1; try { throw 2; } catch (x) { x }").unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn catch_with_undefined() {
        assert_eq!(
            eval("try { throw undefined; } catch (e) { e }").unwrap(),
            Value::Undefined
        );
    }
}

mod try_catch_finally_statement {
    use super::*;

    #[test]
    fn try_catch_works() {
        let r = eval("try { throw 42; } catch (e) { e }").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn try_return_finally_side_effect_runs() {
        assert_eq!(
            eval(
                "var n=0; class C { constructor(){ try { return; } finally { n=1; } } } \
                  try { new C(); } catch(e) {} n"
            )
            .unwrap(),
            Value::Number(1.0)
        );
    }
}

mod for_in_statement {
    use super::*;

    #[test]
    fn for_in_iterates_keys() {
        assert_eq!(
            eval("let keys = []; for (let k in {a: 1, b: 2}) { keys.push(k); } keys.length")
                .unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn for_in_with_break() {
        assert_eq!(eval("let keys = []; for (let k in {a: 1, b: 2, c: 3}) { keys.push(k); if (keys.length >= 2) break; } keys.length").unwrap(), Value::Number(2.0));
    }

    #[test]
    fn for_in_var_redecl_in_body_preserves_binding() {
        assert_eq!(
            eval(
                "var iterCount = 0; for (var x in { attr: null }) { var x; \
                 if (x !== 'attr') throw new Error('bad'); iterCount++; } iterCount",
            )
            .unwrap(),
            Value::Number(1.0)
        );
    }

    #[test]
    fn for_in_break_returns_body_completion() {
        assert_eq!(
            eval("var b; for (b in { x: 0 }) { 3; break; }").unwrap(),
            Value::Number(3.0)
        );
    }
}

// ─── Function declaration (eval_func_decl) ────────────────────────────────

mod function_declaration {
    use super::*;

    #[test]
    fn declaration_returns_undefined() {
        assert_eq!(eval("function f() {}").unwrap(), Value::Undefined);
    }

    #[test]
    fn hoisting_before_declaration() {
        assert_eq!(
            eval("f(); function f() { return 42; }").unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn hoisting_among_vars() {
        assert_eq!(
            eval("var x = f(); function f() { return 10; } x").unwrap(),
            Value::Number(10.0)
        );
    }

    #[test]
    fn multiple_function_declarations() {
        assert_eq!(
            eval("function a() { return 1; } function b() { return 2; } a() + b()").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn function_inside_block() {
        assert_eq!(
            eval("{ function f() { return 7; } } f()").unwrap(),
            Value::Number(7.0)
        );
    }
}

// ─── Class declaration (eval_class_decl) ──────────────────────────────────

mod class_declaration {
    use super::*;

    #[test]
    fn declaration_returns_undefined() {
        assert_eq!(eval("class C {}").unwrap(), Value::Undefined);
    }

    #[test]
    fn class_invalid_heritage_throws() {
        assert!(eval("class C extends 42 {}").is_err());
        assert!(eval("(function() { class C extends 42 {} })()").is_err());
        assert_eq!(
            eval("try { (function() { class C extends 42 {} })(); } catch (e) { typeof e + ':' + e.constructor.name }").unwrap(),
            Value::String("object:TypeError".into())
        );
        assert!(eval("class C extends function() {}.bind() {}").is_err());
        assert!(crate::value::take_thrown_value().is_some());
        assert_eq!(
            eval("var D = class extends function() { arguments.callee; } {}; try { Object.getPrototypeOf(D).arguments; } catch (e) { typeof e + ':' + (e && e.name) }").unwrap(),
            Value::String("object:TypeError".into())
        );
    }

    #[test]
    fn direct_eval_class_decl_preserves_prior_completion() {
        assert_eq!(eval("eval('1; class C {}')").unwrap(), Value::Number(1.0));
    }

    #[test]
    fn direct_eval_class_decl_after_prior_class_eval() {
        assert_eq!(
            eval("eval('class C {}'); eval('1; class C {}')").unwrap(),
            Value::Number(1.0)
        );
    }

    #[test]
    fn class_with_method() {
        assert_eq!(
            eval("class C { method() { return 42; } } new C().method()").unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn class_with_constructor() {
        assert_eq!(
            eval("class C { constructor(x) { this.x = x; } } new C(99).x").unwrap(),
            Value::Number(99.0)
        );
    }

    #[test]
    fn class_computed_getter_name_throws_on_abrupt() {
        // Computed property name in accessor should evaluate the expression.
        // If it throws, the class declaration should throw.
        let r = eval("var t = function() { throw new Error(); }; class C { get [t()]() {} }");
        assert!(
            r.is_err(),
            "computed getter name throwing should propagate to class decl, got {:?}",
            r
        );
    }

    #[test]
    fn class_computed_setter_name_throws_on_abrupt() {
        let r = eval("var t = function() { throw new Error(); }; class C { set [t()](_) {} }");
        assert!(
            r.is_err(),
            "computed setter name throwing should propagate to class decl, got {:?}",
            r
        );
    }
}

// ─── Var declaration without initializer (eval_var_decl) ──────────────────

mod var_without_init {
    use super::*;

    #[test]
    fn var_without_init_is_undefined() {
        assert_eq!(eval("var x; x").unwrap(), Value::Undefined);
    }

    #[test]
    fn let_without_init_is_undefined() {
        assert_eq!(eval("let x; x").unwrap(), Value::Undefined);
    }

    #[test]
    fn var_redeclaration_without_init_resets_to_undefined() {
        assert_eq!(eval("var x = 5; var x; x").unwrap(), Value::Number(5.0));
    }
}

// ─── Expression statement ─────────────────────────────────────────────────

mod expression_statement {
    use super::*;

    #[test]
    fn number_literal() {
        assert_eq!(eval("42").unwrap(), Value::Number(42.0));
    }

    #[test]
    fn string_literal() {
        assert_eq!(eval("'hello'").unwrap(), Value::String("hello".into()));
    }

    #[test]
    fn boolean_literal() {
        assert_eq!(eval("true").unwrap(), Value::Boolean(true));
    }

    #[test]
    fn null_literal() {
        assert_eq!(eval("null").unwrap(), Value::Null);
    }

    #[test]
    fn assignment_expression() {
        assert_eq!(eval("var x; x = 10").unwrap(), Value::Number(10.0));
    }

    #[test]
    fn call_expression() {
        assert_eq!(eval("Math.max(3, 7)").unwrap(), Value::Number(7.0));
    }
}

// ─── Multiple statements / eval_statements ────────────────────────────────

mod multiple_statements {
    use super::*;

    #[test]
    fn last_expression_is_completion_value() {
        assert_eq!(eval("1; 2; 3").unwrap(), Value::Number(3.0));
    }

    #[test]
    fn var_declaration_does_not_override_completion() {
        assert_eq!(eval("1; var x = 2; 3").unwrap(), Value::Number(3.0));
    }

    #[test]
    fn function_declaration_does_not_override_completion() {
        assert_eq!(eval("1; function f() {}; 3").unwrap(), Value::Number(3.0));
    }

    #[test]
    fn class_declaration_does_not_override_completion() {
        assert_eq!(eval("1; class C {}; 3").unwrap(), Value::Number(3.0));
    }

    #[test]
    fn using_block_does_not_override_completion() {
        assert_eq!(
            eval("4; { using resource = null; }").unwrap(),
            Value::Number(4.0)
        );
    }

    #[test]
    fn sequence_of_expression_statements() {
        assert_eq!(eval("1 + 1; 2 + 2; 3 + 3").unwrap(), Value::Number(6.0));
    }
}

// ─── Return in more contexts (eval_function_body) ─────────────────────────

mod function_body {
    use super::*;

    /// Empty block: undefined per ES §13.2.1.
    #[test]
    fn return_async_call_yields_promise_not_tco() {
        assert_eq!(
            eval("function g() { return (async function() {})(); } typeof g()").unwrap(),
            Value::String("object".into())
        );
    }

    /// Empty block: undefined per ES §13.2.1.
    #[test]
    fn empty_block_is_undefined() {
        assert_eq!(eval("function f() {} f()").unwrap(), Value::Undefined);
    }

    /// Expression statement at end: its completion value is the return value.
    /// Per ES spec, the completion value of the last statement becomes the
    /// function's return value when no explicit return is present.
    #[test]
    fn expression_completion_not_return() {
        assert_eq!(eval("function f() { 42; } f()").unwrap(), Value::Undefined);
    }

    /// Postfix increment: x++ evaluates to the original value (1), then increments.
    /// But function with no explicit return returns undefined.
    #[test]
    fn postfix_increment_completion_not_return() {
        assert_eq!(
            eval("function f() { var x = 1; x++; } f()").unwrap(),
            Value::Undefined
        );
    }

    /// Multiple statements: last statement's completion is not the return value
    /// when there's no explicit return.
    #[test]
    fn last_statement_completion_not_return() {
        assert_eq!(
            eval("function f() { var x = 1; var y = 2; x + y; } f()").unwrap(),
            Value::Undefined
        );
    }
}

// ─── If/else edge cases ───────────────────────────────────────────────────

mod if_edge_cases {
    use super::*;

    #[test]
    fn chained_else_if() {
        assert_eq!(
            eval("function f(n) { if (n > 5) return 'big'; else if (n > 2) return 'mid'; else return 'small'; } f(3)")
                .unwrap(),
            Value::String("mid".into())
        );
    }

    #[test]
    fn nested_if_else() {
        assert_eq!(
            eval("function f(a, b) { if (a) if (b) return 'ab'; else return 'a'; else return 'none'; } f(true, false)")
                .unwrap(),
            Value::String("a".into())
        );
    }

    #[test]
    fn if_with_block_body() {
        assert_eq!(
            eval("var x = 0; if (true) { x = 1; x++; } x").unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn if_with_complex_condition() {
        assert_eq!(
            eval("if (1 + 1 === 2) 10; else 20").unwrap(),
            Value::Number(10.0)
        );
    }
}

// ─── While loop edge cases ────────────────────────────────────────────────

mod while_edge_cases {
    use super::*;

    #[test]
    fn while_tail_call_body() {
        let src = "var callCount = 0;\n".to_owned()
            + "function f(n) {\n"
            + "  if (n === 0) {\n"
            + "    callCount += 1;\n"
            + "    return;\n"
            + "  }\n"
            + "  while (true) {\n"
            + "    return f(n - 1);\n"
            + "  }\n"
            + "}\n"
            + "f(1000);\n"
            + "callCount";
        let result = eval(src.as_str()).unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn while_with_return() {
        assert_eq!(
            eval("function f() { while (true) { return 42; } return 0; } f()").unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn while_break_in_inner_loop() {
        // break only exits the inner loop, outer continues
        assert_eq!(
            eval("let i = 0; while (i < 3) { i++; let j = 0; while (j < 3) { j++; break; } } i")
                .unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn while_with_complex_condition() {
        assert_eq!(
            eval("let i = 0; while ((i++) < 3) {} i").unwrap(),
            Value::Number(4.0)
        );
    }
}

// ─── For loop edge cases ──────────────────────────────────────────────────

mod for_edge_cases {
    use super::*;

    #[test]
    fn for_with_return() {
        assert_eq!(
            eval("function f() { for (var i = 0; i < 10; i++) { if (i === 3) return i; } return -1; } f()").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn for_with_no_init() {
        assert_eq!(
            eval("var i = 0; for (; i < 3; i++); i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn for_with_no_update() {
        assert_eq!(
            eval("let i = 0; for (; i < 3;) { i++; } i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn for_with_empty_body() {
        assert_eq!(
            eval("let s = 0; for (let i = 0; i < 5; i++, s++); s").unwrap(),
            Value::Number(5.0)
        );
    }

    #[test]
    fn for_without_body_block() {
        assert_eq!(
            eval("let i = 0; for (; i < 3; i++); i").unwrap(),
            Value::Number(3.0)
        );
    }
}

// ─── Try/catch edge cases ─────────────────────────────────────────────────

mod try_catch_edge_cases {
    use super::*;

    #[test]
    fn return_in_try() {
        assert_eq!(
            eval("function f() { try { return 42; } catch (e) { return 0; } } f()").unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn return_in_catch() {
        assert_eq!(
            eval("function f() { try { throw 1; } catch (e) { return e + 10; } } f()").unwrap(),
            Value::Number(11.0)
        );
    }

    #[test]
    fn nested_try_catch() {
        assert_eq!(
            eval("try { try { throw 1; } catch (e) { throw e + 1; } } catch (f) { f }").unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn try_catch_with_var_in_body() {
        // var in try body is hoisted and visible in catch
        assert_eq!(
            eval("var x = 'outer'; try { throw 1; } catch (e) { x = 'caught'; } x").unwrap(),
            Value::String("caught".into())
        );
    }

    #[test]
    fn try_catch_no_throw_skips_catch() {
        assert_eq!(
            eval("try { 99 } catch (e) { 0 }").unwrap(),
            Value::Number(99.0)
        );
    }

    #[test]
    fn try_catch_with_object_error() {
        assert_eq!(
            eval("try { throw { code: 500 }; } catch (e) { e.code }").unwrap(),
            Value::Number(500.0)
        );
    }

    #[test]
    fn try_catch_with_string_error() {
        assert_eq!(
            eval("try { throw 'error'; } catch (e) { e }").unwrap(),
            Value::String("error".into())
        );
    }
}

#[test]
fn throw_function_call_sequence_preserves_parameter_value() {
    let value = eval(
        "var i=0; function adding1(){ i++; return 1; } \
         try { throw (adding1()); } catch(e) {} \
         var i=0; function adding2(){ i++; return i; } \
         try { throw adding2(); } catch(e) {} \
         var i=0; function adding3(){ i++; } \
         try { throw adding3(); } catch(e) {} \
         function adding4(i){ i++; return i; } \
         try { throw (adding4(1)); } catch(e) { e }",
    )
    .unwrap();
    assert_eq!(value, Value::Number(2.0));
}

// ─── For-in edge cases ────────────────────────────────────────────────────

mod for_in_edge_cases {
    use super::*;

    #[test]
    fn for_in_with_continue() {
        assert_eq!(
            eval("let keys = []; for (let k in {a:1, b:2, c:3}) { if (k === 'b') continue; keys.push(k); } keys.length")
                .unwrap(),
            Value::Number(2.0)
        );
    }

    #[test]
    fn for_in_with_return() {
        assert_eq!(
            eval("function f() { for (let k in {a:1, b:2, c:3}) { if (k === 'b') return k; } return 'none'; } f()")
                .unwrap(),
            Value::String("b".into())
        );
    }

    #[test]
    fn for_in_on_array() {
        assert_eq!(
            eval("let keys = []; for (let k in ['a', 'b', 'c']) { keys.push(k); } keys.length")
                .unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn for_in_on_empty_object() {
        assert_eq!(
            eval("let count = 0; for (let k in {}) { count++; } count").unwrap(),
            Value::Number(0.0)
        );
    }

    #[test]
    fn for_in_with_var_declaration() {
        assert_eq!(
            eval("let keys = []; for (var k in {x:1, y:2}) { keys.push(k); } keys.length").unwrap(),
            Value::Number(2.0)
        );
    }
}

// ─── Break/continue edge cases ────────────────────────────────────────────

mod break_continue_edge_cases {
    use super::*;

    #[test]
    fn break_in_for() {
        assert_eq!(
            eval("let s = 0; for (let i = 0; i < 10; i++) { if (i === 5) break; s = i; } s")
                .unwrap(),
            Value::Number(4.0)
        );
    }

    #[test]
    fn continue_in_nested_loops() {
        assert_eq!(
            eval("let acc = 0; for (let i = 0; i < 3; i++) { for (let j = 0; j < 3; j++) { if (j === 1) continue; acc++; } } acc")
                .unwrap(),
            Value::Number(6.0)
        );
    }

    #[test]
    fn break_in_nested_loops() {
        assert_eq!(
            eval("let acc = 0; for (let i = 0; i < 3; i++) { for (let j = 0; j < 3; j++) { if (j === 1) break; acc++; } } acc")
                .unwrap(),
            Value::Number(3.0)
        );
    }
}

// ─── Throw edge cases ─────────────────────────────────────────────────────

mod throw_edge_cases {
    use super::*;

    #[test]
    fn throw_type_error() {
        let result = eval("throw new TypeError('bad')");
        assert!(result.is_err());
    }

    #[test]
    fn throw_and_catch_string() {
        assert_eq!(
            eval("try { throw 'msg'; } catch (e) { e }").unwrap(),
            Value::String("msg".into())
        );
    }

    #[test]
    fn throw_prevents_subsequent_code() {
        let mut ctx = Context::new().unwrap();
        let result = ctx.eval("throw 1; 42");
        assert!(result.is_err());
    }
}

// ─── Sequence declarations (multiple var decls at once) ───────────────────

mod sequence_decls {
    use super::*;

    #[test]
    fn multiple_var_in_one_statement() {
        assert_eq!(
            eval("var a = 1, b = 2, c = 3; a + b + c").unwrap(),
            Value::Number(6.0)
        );
    }

    #[test]
    fn multiple_let_in_one_statement() {
        assert_eq!(eval("let a = 4, b = 5; a + b").unwrap(), Value::Number(9.0));
    }

    #[test]
    fn mixed_init_and_uninit() {
        assert_eq!(
            eval("var a = 1, b; a + (b === undefined ? 2 : 0)").unwrap(),
            Value::Number(3.0)
        );
    }
}

// ─── Block statement edge cases ───────────────────────────────────────────

mod block_edge_cases {
    use super::*;

    #[test]
    fn block_returns_last_expression() {
        assert_eq!(eval("{ 1; 2; 3 }").unwrap(), Value::Number(3.0));
    }

    #[test]
    fn nested_blocks_create_scopes() {
        assert_eq!(
            eval("{ let x = 1; { let x = 2; } x }").unwrap(),
            Value::Number(1.0)
        );
    }

    #[test]
    fn empty_block_returns_undefined() {
        assert_eq!(eval("{}").unwrap(), Value::Undefined);
    }

    #[test]
    fn block_with_var_hoists_out() {
        assert_eq!(eval("{ var x = 10; } x").unwrap(), Value::Number(10.0));
    }

    #[test]
    fn block_scope_visible_inside_for_let_loop() {
        // Regression: let bindings in outer blocks must be visible inside
        // for (let i = 0; ...) loops (before TDZ fix).
        let result = eval(
            "{ let x = 10; let r = []; for (let i = 0; i < 3; ++i) { r.push(x); } r.length; }",
        );
        match &result {
            Ok(Value::Number(n)) if *n == 3.0 => {} // pass
            other => panic!("Expected 3, got {:?}", other),
        }
    }
}

// ─── Throw with different types ───────────────────────────────────────────

mod throw_types {
    use super::*;

    #[test]
    fn throw_number_caught() {
        assert_eq!(
            eval("try { throw 99; } catch (e) { e }").unwrap(),
            Value::Number(99.0)
        );
    }

    #[test]
    fn throw_boolean_caught() {
        assert_eq!(
            eval("try { throw true; } catch (e) { e }").unwrap(),
            Value::Boolean(true)
        );
    }

    #[test]
    fn throw_object_caught() {
        assert_eq!(
            eval("var o = { msg: 'err' }; try { throw o; } catch (e) { e.msg }").unwrap(),
            Value::String("err".into())
        );
    }

    #[test]
    fn throw_caught_does_not_propagate() {
        assert_eq!(
            eval("var x = 0; try { throw 1; } catch (e) { x = e; } x").unwrap(),
            Value::Number(1.0)
        );
    }
}

// ─── is_tail_expr ─────────────────────────────────────────────────────────

mod is_tail_expr {
    use super::super::is_tail_expr;
    use crate::ast::Expression;

    #[test]
    fn call_expression_is_tail() {
        let expr = Expression::Call {
            callee: Box::new(Expression::Identifier("f".into())),
            arguments: vec![],
        };
        assert!(is_tail_expr(&expr));
    }

    #[test]
    fn identifier_is_not_tail() {
        let expr = Expression::Identifier("x".into());
        assert!(!is_tail_expr(&expr));
    }

    #[test]
    fn binary_add_is_not_tail() {
        let expr = Expression::Binary {
            left: Box::new(Expression::Identifier("x".into())),
            op: crate::ast::BinaryOp::Add,
            right: Box::new(Expression::Number(1.0)),
        };
        assert!(!is_tail_expr(&expr));
    }

    #[test]
    fn eval_call_is_tail() {
        let expr = Expression::Call {
            callee: Box::new(Expression::Identifier("eval".into())),
            arguments: vec![Expression::String("1".into())],
        };
        assert!(is_tail_expr(&expr));
    }

    #[test]
    fn conditional_and_sequence_tail_calls_are_tail() {
        let conditional = Expression::Conditional {
            condition: Box::new(Expression::Boolean(true)),
            consequent: Box::new(Expression::Call {
                callee: Box::new(Expression::Identifier("f".into())),
                arguments: vec![],
            }),
            alternate: Box::new(Expression::Call {
                callee: Box::new(Expression::Identifier("g".into())),
                arguments: vec![],
            }),
        };
        assert!(is_tail_expr(&conditional));
        let sequence = Expression::Sequence(vec![Expression::Number(0.0), conditional]);
        assert!(is_tail_expr(&sequence));
    }
}

// ─── acc_stack thread-local ────────────────────────────────────────────────

mod acc_stack {
    use super::super::{
        acc_stack_len, acc_stack_pop_to, acc_stack_push, acc_stack_top, acc_stack_update_last,
    };
    use crate::value::Value;
    use crate::Context;

    fn sym(desc: &'static str) -> Value {
        crate::builtins::symbol::new_symbol(Some(desc))
    }

    /// Drain the thread-local stack to a clean state.
    fn drain() {
        acc_stack_pop_to(0);
    }

    #[test]
    fn empty_stack_returns_none() {
        drain();
        assert_eq!(acc_stack_len(), 0);
        assert!(acc_stack_top().is_none());
    }

    #[test]
    fn push_increases_len() {
        drain();
        acc_stack_push(sym("A"));
        assert_eq!(acc_stack_len(), 1);
        assert!(acc_stack_top().is_some());
    }

    #[test]
    fn pop_to_restores_to_target() {
        drain();
        acc_stack_push(sym("X"));
        acc_stack_push(sym("Y"));
        acc_stack_push(sym("Z"));
        assert_eq!(acc_stack_len(), 3);
        // Pop back to depth 1: removes Y and Z.
        acc_stack_pop_to(1);
        assert_eq!(acc_stack_len(), 1);
        assert!(acc_stack_top().is_some_and(|v| v.is_symbol_with("X")));
    }

    #[test]
    fn pop_to_zero_clears() {
        drain();
        acc_stack_push(sym("A"));
        acc_stack_push(sym("B"));
        acc_stack_push(sym("C"));
        acc_stack_pop_to(0);
        assert_eq!(acc_stack_len(), 0);
        assert!(acc_stack_top().is_none());
    }

    #[test]
    fn update_last_replaces_top() {
        drain();
        acc_stack_push(sym("BOTTOM"));
        acc_stack_push(sym("OLD"));
        // update_last replaces the most-recently-pushed item (the top).
        acc_stack_update_last(sym("NEW"));
        assert!(acc_stack_top().is_some_and(|v| v.is_symbol_with("NEW")));
        acc_stack_pop_to(0);
    }

    #[test]
    fn update_last_only_affects_top() {
        drain();
        acc_stack_push(sym("A"));
        acc_stack_push(sym("B"));
        acc_stack_push(sym("C"));
        // Update top only.
        acc_stack_update_last(sym("X"));
        // Stack: A, B, X  (top replaced, A and B unchanged)
        let top = acc_stack_top();
        assert!(
            top.as_ref().is_some_and(|v| v.is_symbol_with("X")),
            "top should be X, got {:?}",
            top
        );
        drain();
    }

    #[test]
    fn nested_push_pop() {
        drain();
        acc_stack_push(sym("L1"));
        acc_stack_push(sym("L2"));
        acc_stack_pop_to(1);
        assert_eq!(acc_stack_len(), 1);
        acc_stack_push(sym("L2a"));
        assert_eq!(acc_stack_len(), 2);
        assert!(acc_stack_top().is_some_and(|v| v.is_symbol_with("L2a")));
        drain();
    }

    #[test]
    fn acc_stack_empty_after_proxy_class_construct() {
        drain();
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        ctx.eval(
            "class P { constructor() { return new Proxy(this, { get(o,k){ 1; return o[k]; } }); } } \
             class T extends P { method() { return 1; } } \
             new T()",
        )
        .unwrap();
        assert_eq!(
            acc_stack_len(),
            0,
            "class construction must not leak acc_stack entries"
        );
        drain();
    }
}

// ─── tail_call_signal thread-local ─────────────────────────────────────────

mod tail_signal {
    use super::super::{set_tail_call_signal, take_tail_call_signal, TailCallSignal};
    use crate::env::Environment;
    use crate::value::Value;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn make_fn() -> crate::value::ValueFunction {
        crate::value::ValueFunction::new(
            None,
            vec![],
            vec![],
            Rc::new(RefCell::new(Environment::new())),
            false,
            false,
        )
    }

    #[test]
    fn set_and_take() {
        assert!(take_tail_call_signal().is_none());

        let sig = TailCallSignal {
            function: make_fn(),
            arguments: vec![Value::Number(42.0)],
            this_val: Value::Undefined,
        };
        set_tail_call_signal(sig);
        let taken = take_tail_call_signal();
        assert!(taken.is_some());
        assert_eq!(taken.unwrap().arguments, vec![Value::Number(42.0)]);
    }

    #[test]
    fn take_clears() {
        let sig = TailCallSignal {
            function: make_fn(),
            arguments: vec![],
            this_val: Value::Undefined,
        };
        set_tail_call_signal(sig);
        assert!(take_tail_call_signal().is_some());
        assert!(take_tail_call_signal().is_none());
    }
}

// ─── Non-tail call: caller executes remaining statements ─────────────────────

/// Regression test: var + return expression (no inner function call).
/// This tests the basic var scoping + return expression evaluation.
mod var_and_return {
    use super::*;

    #[test]
    fn var_then_return_expression() {
        // function f() { var x = 10; return x + 1; }
        // x is 10, return x+1 → 11
        assert_eq!(
            eval(
                r#""use strict";
                function f() { var x = 10; return x + 1; }
                f()"#,
            )
            .unwrap(),
            Value::Number(11.0)
        );
    }

    #[test]
    fn var_from_expression_then_return() {
        // function f() { var x = 5 * 2; return x + 1; }
        assert_eq!(
            eval(
                r#""use strict";
                function f() { var x = 5 * 2; return x + 1; }
                f()"#,
            )
            .unwrap(),
            Value::Number(11.0)
        );
    }

    #[test]
    fn multiple_var_then_return() {
        assert_eq!(
            eval(
                r#""use strict";
                function f() { var a = 1; var b = 2; return a + b + 3; }
                f()"#,
            )
            .unwrap(),
            Value::Number(6.0)
        );
    }

    /// Simple function call with return value used in expression.
    #[test]
    fn call_return_used_in_expression() {
        assert_eq!(
            eval(
                r#""use strict";
                function g() { return 10; }
                var x = g();
                x + 1"#,
            )
            .unwrap(),
            Value::Number(11.0)
        );
    }

    /// Same pattern but INSIDE a function body — this is the failing case.
    #[test]
    fn call_return_used_in_function_body() {
        assert_eq!(
            eval(
                r#""use strict";
                function g() { return 10; }
                function f() { var x = g(); return x + 1; }
                f()"#,
            )
            .unwrap(),
            Value::Number(11.0)
        );
    }
}

mod non_tail_call {
    use super::*;

    /// The canonical non-tail call: var x = f(); return x + 1;
    /// f() returns 10, caller adds 1 → 11.
    #[test]
    fn caller_executes_remaining_after_non_tail_call() {
        assert_eq!(
            eval(
                r#""use strict";
                function g() { return 10; }
                function f() { var x = g(); return x + 1; }
                f()"#,
            )
            .unwrap(),
            Value::Number(11.0)
        );
    }

    /// Multiple non-tail calls in sequence.
    #[test]
    fn multiple_non_tail_calls() {
        assert_eq!(
            eval(
                r#""use strict";
                function a() { return 1; }
                function b() { return a() + 10; }
                function c() { return b() + 100; }
                c()"#,
            )
            .unwrap(),
            Value::Number(111.0)
        );
    }

    /// Deep non-tail chain: f → g (tail) → h (tail), h returns, g adds, f adds.
    #[test]
    fn deep_non_tail_chain() {
        assert_eq!(
            eval(
                r#""use strict";
                function h() { return 100; }
                function g() { var y = h(); return y + 10; }
                function f() { var x = g(); return x + 1; }
                f()"#,
            )
            .unwrap(),
            Value::Number(111.0)
        );
    }

    /// Non-tail call where caller discards the value.
    #[test]
    fn non_tail_call_result_discarded() {
        assert_eq!(
            eval(
                r#""use strict";
                function g() { return 99; }
                function f() { g(); return 42; }
                f()"#,
            )
            .unwrap(),
            Value::Number(42.0)
        );
    }

    /// Non-tail call with side-effect in expression.
    #[test]
    fn non_tail_call_with_side_effect() {
        assert_eq!(
            eval(
                r#""use strict";
                var calls = 0;
                function g() { calls += 1; return 10; }
                function f() { var x = g(); return calls; }
                f()"#,
            )
            .unwrap(),
            Value::Number(1.0)
        );
    }
}

// ─── try-finally ────────────────────────────────────────────────────────
// Note: try-finally (try without catch) is not yet implemented.
// These tests are ignored until the feature is added.

// ─── optional chaining ──────────────────────────────────────────────────
// Note: optional chaining on null/undefined is not fully implemented.
// These tests cover the supported cases.

mod optional_chaining {
    use super::*;

    #[test]
    fn optional_member_on_object() {
        let r = eval("var o = {a: 1}; o?.a").unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn optional_member_on_missing_property() {
        let r = eval("({})?.missing").unwrap();
        assert_eq!(r, Value::Undefined);
    }

    #[test]
    fn optional_call_on_function() {
        let r = eval("var f = () => 42; f?.()").unwrap();
        assert_eq!(r, Value::Number(42.0));
    }

    #[test]
    fn optional_call_supports_string_constructor() {
        let r = eval("String?.(42)").unwrap();
        assert_eq!(r, Value::String("42".to_string()));
    }

    #[test]
    fn optional_chain_with_method() {
        let r = eval("var o = {m() { return 5; }}; o.m?.()").unwrap();
        assert_eq!(r, Value::Number(5.0));
    }

    #[test]
    fn optional_chain_on_array() {
        let r = eval("[1,2,3]?.[1]").unwrap();
        assert_eq!(r, Value::Number(2.0));
    }

    #[test]
    fn optional_super_member_chain_returns_undefined() {
        let value = eval("class A { undf () { return super.a?.c; } } new A().undf()").unwrap();
        assert_eq!(value, Value::Undefined);
    }

    #[test]
    fn optional_member_skips_computed_key_after_nullish_chain() {
        let r = eval("var count = 0; function key() { count++; } undefined?.arr[key()]; count")
            .unwrap();
        assert_eq!(r, Value::Number(0.0));
    }

    #[test]
    fn optional_super_method_call_preserves_this() {
        let r = eval("let called = false; let context; class B { method() { called = true; context = this; } } class F extends B { method() { super.method?.(); } } let f = new F(); f.method(); [called, context === f]").unwrap();
        let object = match r {
            Value::Object(object) => object,
            _ => panic!(),
        };
        assert_eq!(object.borrow().get("0"), Some(Value::Boolean(true)));
        assert_eq!(object.borrow().get("1"), Some(Value::Boolean(true)));
    }

    #[test]
    fn optional_member_after_super_call_evaluates_super_once() {
        let r =
            eval("class A {} class B extends A { constructor() { return super()?.a; } } new B()")
                .unwrap();
        assert!(matches!(r, Value::Object(_)));
    }

    #[test]
    fn optional_chain_private_field_continuation_short_circuits() {
        let mut ctx = crate::Context::new().unwrap();
        ctx.eval(
            "class C { #f = 'ok'; method(o) { return o?.c.#f; } } let c = new C(); c.method(null)",
        )
        .map(|value| assert_eq!(value, crate::Value::Undefined))
        .unwrap();
    }
}

// ─── array spread in literals ──────────────────────────────────────────

mod array_spread {
    use super::*;

    #[test]
    fn spread_in_array_literal() {
        let r = eval("[1, ...[2, 3], 4]").unwrap();
        match r {
            Value::Object(ref o) => {
                let arr = o.borrow();
                assert_eq!(arr.elements.len(), 4);
                assert_eq!(arr.elements.first(), Some(&Value::Number(1.0)));
                assert_eq!(arr.elements.get(1), Some(&Value::Number(2.0)));
                assert_eq!(arr.elements.get(2), Some(&Value::Number(3.0)));
                assert_eq!(arr.elements.get(3), Some(&Value::Number(4.0)));
            }
            _ => panic!("expected array"),
        }
    }

    #[test]
    fn spread_empty_array() {
        let r = eval("[1, ...[], 2]").unwrap();
        match r {
            Value::Object(ref o) => {
                let arr = o.borrow();
                assert_eq!(arr.elements.len(), 2);
                assert_eq!(arr.elements.first(), Some(&Value::Number(1.0)));
                assert_eq!(arr.elements.get(1), Some(&Value::Number(2.0)));
            }
            _ => panic!("expected array"),
        }
    }

    #[test]
    fn spread_string() {
        let r = eval("[...'abc']").unwrap();
        match r {
            Value::Object(ref o) => {
                let arr = o.borrow();
                assert_eq!(arr.elements.len(), 3);
                assert_eq!(arr.elements.first(), Some(&Value::String("a".to_string())));
                assert_eq!(arr.elements.get(1), Some(&Value::String("b".to_string())));
                assert_eq!(arr.elements.get(2), Some(&Value::String("c".to_string())));
            }
            _ => panic!("expected array"),
        }
    }

    #[test]
    fn spread_nested() {
        let r = eval("[1, ...['a', ...['b', 'c']], 2]").unwrap();
        match r {
            Value::Object(ref o) => {
                let arr = o.borrow();
                assert_eq!(arr.elements.len(), 5);
                assert_eq!(arr.elements.first(), Some(&Value::Number(1.0)));
                assert_eq!(arr.elements.get(1), Some(&Value::String("a".to_string())));
                assert_eq!(arr.elements.get(2), Some(&Value::String("b".to_string())));
                assert_eq!(arr.elements.get(3), Some(&Value::String("c".to_string())));
                assert_eq!(arr.elements.get(4), Some(&Value::Number(2.0)));
            }
            _ => panic!("expected array"),
        }
    }
}

mod do_while_statement {
    use super::*;

    #[test]
    fn do_while_returns_body_completion_value() {
        // do-while body completes with a value when condition is false
        assert_eq!(eval("do { 1; } while (false)").unwrap(), Value::Number(1.0));
    }

    #[test]
    fn do_while_returns_last_body_value() {
        // When condition becomes false, return the body completion value
        assert_eq!(
            eval("let x = 0; do { x++; } while (x < 3); x").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn do_while_returns_undefined_when_body_no_value() {
        // Body with no completion value returns undefined
        let result = eval("do ; while (false)").unwrap();
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn do_while_returns_expression_value() {
        assert_eq!(
            eval("do { 42; } while (false)").unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn do_while_break_exits_loop() {
        assert_eq!(
            eval("let i = 0; do { i++; if (i > 2) break; } while (true); i").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn do_while_break_returns_undefined() {
        // break does not provide a value; loop returns undefined
        let result = eval("let i = 0; do { i++; if (i > 2) break; } while (true)").unwrap();
        assert_eq!(result, Value::Undefined);
    }

    #[test]
    fn do_while_return_interrupts() {
        assert_eq!(
            eval("function f() { do { return 99; } while (true); } f()").unwrap(),
            Value::Number(99.0)
        );
    }

    #[test]
    fn do_while_continue_restarts() {
        // continue in do-while jumps back to condition check, skipping j++.
        // i=1,2: continue skips j++. i=3,4: j++ runs. i=5: exit.
        // j ends at 3 (j=1 at i=3, j=2 at i=4, j=3 at i=5).
        assert_eq!(
            eval("let i = 0, j = 0; do { i++; if (i < 3) continue; j++; } while (i < 5); j")
                .unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn do_while_with_nested_labeled_break() {
        // break inside a nested labeled do-while should exit only that do-while,
        // not the outer one. Variables declared after break inside the inner
        // do-while should still be accessible (var hoisting).
        let mut ctx = Context::new().unwrap();
        ctx.eval("var result = ''").unwrap();
        assert_eq!(
            ctx.eval(
                r#"
                var result = "";
                do_out: do {
                    result += "A";
                    do_in: do {
                        result += "B";
                        break do_in;
                        result += "FAIL";
                    } while (0);
                    result += "C";
                } while (2==1);
                result;
            "#
            )
            .unwrap(),
            Value::String("ABC".to_string())
        );
    }

    #[test]
    fn do_while_body_completion_from_expression() {
        // The test S12.6.1_A3: do __in__do=1; while(false) should return 1
        assert_eq!(
            eval("var x = 0; do x = 1; while (false); x").unwrap(),
            Value::Number(1.0)
        );
    }

    #[test]
    fn do_while_break_returns_block_completion() {
        assert_eq!(
            eval("eval('2; do { 3; break; } while (false)')").unwrap(),
            Value::Number(3.0)
        );
    }

    #[test]
    fn direct_eval_do_while_continue_loops() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let r = ctx
            .eval("var x = 0; eval(\"do { x++; continue; } while (x < 3)\"); x")
            .unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn direct_eval_do_while_increments_outer_var() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let r = ctx
            .eval("var x = 0; eval(\"do { x++; } while (x < 3)\"); x")
            .unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn direct_eval_do_while_continue_with_if_skips_body() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let r = ctx
            .eval(
                "var c = 0, o = 0; \
                 eval(\"do { c++; if (c % 2 === 1) continue; o++; } while (c < 6)\"); \
                 o",
            )
            .unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn string_split_on_decimal_detection() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let s = ctx.eval("(''+1/2)").unwrap();
        assert_eq!(s, Value::String("0.5".into()));
        let r = ctx.eval("('0.5').split('.').length").unwrap();
        assert_eq!(r, Value::Number(2.0));
        let r = ctx.eval("(''+1/2).split('.').length").unwrap();
        assert_eq!(r, Value::Number(2.0));
        let r2 = ctx.eval("(''+2/2).split('.').length").unwrap();
        assert_eq!(r2, Value::Number(1.0));
    }

    #[test]
    fn direct_eval_do_while_split_decimal_continue() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let r = ctx
            .eval(
                "var c = 0, o = 0; \
                 eval(\"do { c++; if (((''+c/2).split('.')).length>1) continue; o++; } while (c < 6)\"); \
                 o",
            )
            .unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn do_while_continue_in_direct_eval_updates_outer_bindings() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let r = ctx
            .eval(
                "var __condition = 0, __odds = 0; \
                 eval(\"do { __condition++; if (((''+__condition/2).split('.')).length>1) continue; __odds++;} while(__condition < 10)\"); \
                 __odds",
            )
            .unwrap();
        assert_eq!(r, Value::Number(5.0));
    }
    #[test]
    fn for_init_object_destructure_binds_pattern() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let r = ctx
            .eval(
                "var iterCount = 0; \
                 for (let { x: y } = { x: 23 }; iterCount < 1; ) { iterCount++; } \
                 iterCount",
            )
            .unwrap();
        assert_eq!(r, Value::Number(1.0));
    }

    #[test]
    fn for_var_in_init_is_hoisted_before_loop() {
        let r = eval(
            "var ok = true; \
             try { index = index; } catch (e) { ok = false; } \
             for (var index = 0; index < 1; index++) {} \
             ok",
        )
        .unwrap();
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn for_var_in_init_visible_after_loop() {
        let r = eval("for (var i = 0; i < 10; i++) {} i").unwrap();
        assert_eq!(r, Value::Number(10.0));
    }

    #[test]
    fn for_completion_value_from_body() {
        let r = eval("eval('for (var run = true; run; run = false) { 3; }')").unwrap();
        assert_eq!(r, Value::Number(3.0));
    }

    #[test]
    fn for_let_body_per_iteration_closure() {
        let r = eval(
            "var probeFirst; \
             var probeSecond = null; \
             for (let x = 'first'; probeSecond === null; x = 'second') \
               if (!probeFirst) probeFirst = function() { return x; }; \
               else probeSecond = function() { return x; }; \
             probeFirst() + '|' + probeSecond()",
        )
        .unwrap();
        assert_eq!(r, Value::String("first|second".into()));
    }

    #[test]
    fn for_head_multi_decl_init() {
        let r = eval(
            "var probeDecl; \
             for (let x = 'inside', _ = (probeDecl = function() { return x; }); false; ) {} \
             probeDecl()",
        )
        .unwrap();
        assert_eq!(r, Value::String("inside".into()));
    }

    #[test]
    fn for_init_null_destructure_throws_when_for_is_last_stmt() {
        let mut ctx = Context::new().unwrap();
        crate::builtins::register_builtins(&mut ctx);
        let r = ctx.eval("(function() { for (let {} = null; ; ) { return; } })()");
        assert!(r.is_err());
    }
}

mod labeled_statement {
    use super::*;

    #[test]
    fn eval_block_block_with_labels() {
        // eval('{}{x: 42};') should return 42, not the object {x: 42}.
        // {x: 42} in statement context is a Block with a labeled statement,
        // not an object literal. The completion value should be 42.
        assert_eq!(eval("eval('{}{x: 42};')").unwrap(), Value::Number(42.0));
    }
}

mod class_name_in_const {
    use super::*;

    #[test]
    fn const_class_gets_binding_name() {
        // Bug fix: const cls = class {}; should set cls.name = "cls"
        assert_eq!(
            eval("const cls = class {}; cls.name").unwrap(),
            Value::String("cls".to_string())
        );
    }

    #[test]
    fn let_class_gets_binding_name() {
        // Same for let declarations
        assert_eq!(
            eval("let cls = class {}; cls.name").unwrap(),
            Value::String("cls".to_string())
        );
    }

    #[test]
    fn var_class_gets_binding_name() {
        // Same for var declarations
        assert_eq!(
            eval("var cls = class {}; cls.name").unwrap(),
            Value::String("cls".to_string())
        );
    }

    #[test]
    fn named_class_not_overridden() {
        // Named class expressions should keep their own name
        assert_eq!(
            eval("var cls = class MyClass {}; cls.name").unwrap(),
            Value::String("MyClass".to_string())
        );
    }

    #[test]
    fn global_var_strict_function_can_read_var() {
        // Regression: strict-mode script global `var` bindings must be
        // accessible from function bodies.
        assert_eq!(
            eval(
                "\"use strict\";
                var x = 42;
                function f() { return x; }
                f();"
            )
            .unwrap(),
            Value::Number(42.0)
        );
    }

    #[test]
    fn tail_call_in_if_does_not_evaluate_following_statement() {
        assert_eq!(
            eval(
                "var after = false; \
                 function call(strings) { return 'done'; } \
                 function f(value) { \
                   if (typeof value !== 'object') { return call`${value}`; } \
                   after = true; \
                   return 'wrong'; \
                 } \
                 f(Symbol()); \
                 after"
            )
            .unwrap(),
            Value::Boolean(false)
        );
    }

    #[test]
    fn arrow_default_closure_uses_parameter_environment() {
        assert_eq!(
            eval(
                "const f = (p = eval(\"var arguments = 'param'\"), q = () => arguments) => { function arguments() {} return q(); }; f()"
            )
            .unwrap(),
            Value::String("param".to_string())
        );
    }

    #[test]
    fn direct_eval_existing_local_var_preserves_value() {
        assert_eq!(
            eval(
                "var initial; (function() { var x = 44443; eval('initial = x; var x;'); }()); initial"
            )
            .unwrap(),
            Value::Number(44443.0)
        );
    }
}
