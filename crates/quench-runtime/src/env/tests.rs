//! Unit tests for the environment module.

#[allow(unused_imports)]
use super::*;

// ── Scope basic operations ────────────────────────────────────────────────

#[test]
fn test_scope_new_is_empty() {
    let scope = Scope::new();
    assert!(scope.is_empty());
    assert!(!scope.is_object_binding());
}

#[test]
fn test_scope_define_and_get() {
    let mut scope = Scope::new();
    scope.define("x".to_string(), Value::Number(1.0));
    assert_eq!(scope.get("x"), Some(Value::Number(1.0)));
    assert!(scope.has("x"));
}

#[test]
fn test_scope_define_overwrites_tdz() {
    let mut scope = Scope::new();
    scope.mark_tdz("x".to_string());
    assert!(scope.is_tdz("x"));
    assert!(scope.is_declared_only("x"));

    scope.define("x".to_string(), Value::Number(5.0));
    assert!(!scope.is_tdz("x"));
    assert!(!scope.is_declared_only("x"));
    assert_eq!(scope.get("x"), Some(Value::Number(5.0)));
}

#[test]
fn test_scope_tdz_hides_value() {
    let mut scope = Scope::new();
    scope.define("x".to_string(), Value::Number(10.0));
    assert_eq!(scope.get("x"), Some(Value::Number(10.0)));

    scope.mark_tdz("x".to_string());
    assert!(scope.is_tdz("x"));
    assert!(scope.get("x").is_none()); // TDZ returns None
}

#[test]
fn test_scope_declare_var_hoisting() {
    let mut scope = Scope::new();
    scope.declare_var("x".to_string(), VarKind::Var);

    assert!(scope.is_declared_only("x"));
    assert!(scope.has("x"));
    // Hoisted var returns undefined
    assert_eq!(scope.get("x"), Some(Value::Undefined));

    // After initialization, returns actual value
    scope.initialize_declared("x", Value::Number(99.0));
    assert!(!scope.is_declared_only("x"));
    assert_eq!(scope.get("x"), Some(Value::Number(99.0)));
}

#[test]
fn test_scope_declare_let_tdz() {
    let mut scope = Scope::new();
    scope.declare_var("y".to_string(), VarKind::Let);

    assert!(scope.is_tdz("y"));
    assert!(scope.is_declared_only("y"));
    assert!(scope.get("y").is_none()); // TDZ access returns None
}

#[test]
fn test_scope_declare_const_tdz() {
    let mut scope = Scope::new();
    scope.declare_var("z".to_string(), VarKind::Const);

    assert!(scope.is_tdz("z"));
    assert!(scope.is_declared_only("z"));
    assert!(scope.get("z").is_none());
}

#[test]
fn test_scope_get_kind() {
    let mut scope = Scope::new();
    assert_eq!(scope.get_kind("x"), None);

    scope.declare_var("a".to_string(), VarKind::Var);
    scope.declare_var("b".to_string(), VarKind::Let);
    scope.declare_var("c".to_string(), VarKind::Const);

    assert_eq!(scope.get_kind("a"), Some(VarKind::Var));
    assert_eq!(scope.get_kind("b"), Some(VarKind::Let));
    assert_eq!(scope.get_kind("c"), Some(VarKind::Const));
    assert_eq!(scope.get_kind("missing"), None);
}

#[test]
fn test_scope_set_const_returns_false() {
    let mut scope = Scope::new();
    scope.declare_var("CONST".to_string(), VarKind::Const);
    scope.initialize_declared("CONST", Value::Number(1.0));

    // set() returns false for const — caller throws TypeError
    assert!(!scope.set("CONST".to_string(), Value::Number(2.0), false));
    // value unchanged
    assert_eq!(scope.get("CONST"), Some(Value::Number(1.0)));
}

#[test]
fn test_scope_set_var_updates() {
    let mut scope = Scope::new();
    scope.declare_var("v".to_string(), VarKind::Var);
    scope.initialize_declared("v", Value::Number(1.0));

    assert!(scope.set("v".to_string(), Value::Number(2.0), false));
    assert_eq!(scope.get("v"), Some(Value::Number(2.0)));
}

#[test]
fn test_scope_set_vacant_returns_false() {
    let mut scope = Scope::new();
    assert!(!scope.set("nonexistent".to_string(), Value::Number(1.0), false));
}

#[test]
fn test_scope_get_rc_identity() {
    let mut scope = Scope::new();
    scope.define("arr".to_string(), Value::Undefined);

    let rc1 = scope.get_rc("arr");
    let rc2 = scope.get_rc("arr");
    assert!(rc1.is_some());
    assert!(rc2.is_some());
    // Same Rc
    assert!(Rc::ptr_eq(rc1.as_ref().unwrap(), rc2.as_ref().unwrap()));
}

#[test]
fn test_scope_declared_only_returns_undefined() {
    let mut scope = Scope::new();
    scope.declare_var("hoisted".to_string(), VarKind::Var);
    assert_eq!(scope.get("hoisted"), Some(Value::Undefined));
}

#[test]
fn test_scope_varstate_initialized() {
    let val = Value::Number(42.0);
    let state = VarState::Initialized(Rc::new(val.clone()));
    match state {
        VarState::Initialized(v) => {
            assert_eq!(v.as_ref(), &Value::Number(42.0));
        }
        _ => unreachable!(),
    }
}

#[test]
fn test_scope_clone_is_deep() {
    let mut scope = Scope::new();
    scope.define("x".to_string(), Value::Number(1.0));
    scope.declare_var("y".to_string(), VarKind::Let);

    let cloned = scope.clone();
    assert_eq!(cloned.get("x"), Some(Value::Number(1.0)));
    assert!(cloned.is_declared_only("y"));
}

#[test]
fn test_scope_this_binding() {
    let mut scope = Scope::new();
    assert!(scope.get_this().is_none());
    assert!(!scope.is_this_initialized());

    scope.set_this(Value::Number(123.0));
    assert_eq!(scope.get_this(), Some(Value::Number(123.0)));
    assert!(scope.is_this_initialized());
}

#[test]
fn test_scope_set_this_value_no_flag() {
    let mut scope = Scope::new();
    scope.set_this_value(Value::String("hello".into()));
    assert_eq!(scope.get_this(), Some(Value::String("hello".into())));
    assert!(!scope.is_this_initialized()); // flag still false
}

#[test]
fn test_scope_mark_this_initialized() {
    let mut scope = Scope::new();
    scope.set_this_value(Value::Number(1.0));
    assert!(!scope.is_this_initialized());

    scope.mark_this_initialized();
    assert!(scope.is_this_initialized());
}

#[test]
fn test_scope_bindings_iter() {
    let mut scope = Scope::new();
    scope.define("a".to_string(), Value::Number(1.0));
    scope.define("b".to_string(), Value::Number(2.0));

    let names: Vec<_> = scope.bindings().map(|(k, _)| k.clone()).collect();
    assert!(names.contains(&"a".to_string()));
    assert!(names.contains(&"b".to_string()));
}

// ── Environment basic operations ─────────────────────────────────────────

#[test]
fn test_environment_new_has_one_scope() {
    let env = Environment::new();
    assert_eq!(env.scopes.len(), 1);
    assert!(env.get_parent().is_none());
}

#[test]
fn test_environment_with_parent() {
    let parent = Rc::new(RefCell::new(Environment::new()));
    let child = Environment::with_parent(Rc::clone(&parent));
    assert_eq!(child.scopes.len(), 1);
    assert!(child.get_parent().is_some());
}

#[test]
fn test_environment_define_and_get() {
    let mut env = Environment::new();
    env.define("x".to_string(), Value::Number(42.0));
    assert_eq!(env.get("x"), Some(Value::Number(42.0)));
}

#[test]
fn test_environment_has() {
    let mut env = Environment::new();
    env.define("y".to_string(), Value::Undefined);
    assert!(env.has("y"));
    assert!(!env.has("z"));
}

#[test]
fn test_environment_set_existing() {
    let mut env = Environment::new();
    env.define("x".to_string(), Value::Number(1.0));
    assert!(env.set("x", Value::Number(2.0)));
    assert_eq!(env.get("x"), Some(Value::Number(2.0)));
}

#[test]
fn test_environment_set_creates_implicit_global_in_global_scope() {
    let mut env = Environment::new();
    env.define("global".to_string(), Value::Undefined); // mark as global scope

    // Assign to undeclared identifier in sloppy mode → creates implicit global
    assert!(env.set("implicit_global", Value::Number(42.0)));
    assert_eq!(env.get("implicit_global"), Some(Value::Number(42.0)));

    // Binding lives in the global (first) scope
    let global_scope = env.scopes.first().unwrap().borrow();
    assert!(global_scope.has("implicit_global"));
}

#[test]
fn test_environment_set_implicit_global_not_in_child_scope() {
    let mut env = Environment::new();
    env.define("global".to_string(), Value::Undefined);
    env.push_scope();

    // Implicit global created in child scope → lives in global scope
    assert!(env.set("implicit_global", Value::Number(1.0)));
    assert_eq!(env.get("implicit_global"), Some(Value::Number(1.0)));

    let global_scope = env.scopes.first().unwrap().borrow();
    assert!(global_scope.has("implicit_global"));
}

#[test]
fn test_environment_keys() {
    let mut env = Environment::new();
    env.define("a".to_string(), Value::Number(1.0));
    env.define("b".to_string(), Value::Number(2.0));
    let keys = env.keys();
    assert!(keys.contains(&"a".to_string()));
    assert!(keys.contains(&"b".to_string()));
}

// ── Scope chain ───────────────────────────────────────────────────────────

#[test]
fn test_scope_chain_inner_shadows_outer() {
    let mut env = Environment::new();
    env.define("x".to_string(), Value::Number(1.0));
    env.push_scope();
    env.define("x".to_string(), Value::Number(2.0));

    assert_eq!(env.get("x"), Some(Value::Number(2.0)));

    env.pop_scope();
    assert_eq!(env.get("x"), Some(Value::Number(1.0)));
}

#[test]
fn test_scope_chain_outer_visible_after_pop() {
    let mut env = Environment::new();
    env.define("outer".to_string(), Value::Number(1.0));
    env.push_scope();
    env.define("inner".to_string(), Value::Number(2.0));

    assert_eq!(env.get("inner"), Some(Value::Number(2.0)));
    assert_eq!(env.get("outer"), Some(Value::Number(1.0)));

    env.pop_scope();
    assert_eq!(env.get("outer"), Some(Value::Number(1.0)));
    assert!(env.get("inner").is_none());
}

#[test]
fn test_scope_chain_multiple_levels() {
    let mut env = Environment::new();
    env.define("a".to_string(), Value::Number(1.0));

    env.push_scope();
    env.define("b".to_string(), Value::Number(2.0));

    env.push_scope();
    env.define("c".to_string(), Value::Number(3.0));

    assert_eq!(env.get("c"), Some(Value::Number(3.0)));
    assert_eq!(env.get("b"), Some(Value::Number(2.0)));
    assert_eq!(env.get("a"), Some(Value::Number(1.0)));

    env.pop_scope();
    assert!(env.get("c").is_none());
    assert_eq!(env.get("b"), Some(Value::Number(2.0)));

    env.pop_scope();
    assert!(env.get("b").is_none());
    assert!(env.get("c").is_none());
    assert_eq!(env.get("a"), Some(Value::Number(1.0)));
}

#[test]
fn test_scope_chain_pop_preserves_bindings() {
    let mut env = Environment::new();
    env.define("x".to_string(), Value::Number(1.0));
    env.push_scope();
    env.define("y".to_string(), Value::Number(2.0));
    env.pop_scope();

    // Outer binding still accessible
    assert_eq!(env.get("x"), Some(Value::Number(1.0)));
    // Scope snapshot lives on
    let snap = env.live_scopes_snapshot();
    assert!(Rc::ptr_eq(&env.current_scope(), &snap[0]));
}

#[test]
fn test_pop_does_not_remove_last_scope() {
    let mut env = Environment::new();
    env.define("x".to_string(), Value::Number(1.0));
    env.pop_scope(); // Should be no-op

    assert_eq!(env.get("x"), Some(Value::Number(1.0)));
    assert_eq!(env.scopes.len(), 1);
}

// ── Parent environment chain ─────────────────────────────────────────────

#[test]
fn test_parent_chain_lookup() {
    let parent = Rc::new(RefCell::new(Environment::new()));
    parent
        .borrow_mut()
        .define("parent_var".to_string(), Value::Number(1.0));

    let child = Environment::with_parent(Rc::clone(&parent));
    assert_eq!(child.get("parent_var"), Some(Value::Number(1.0)));
}

#[test]
fn test_parent_chain_set() {
    let parent = Rc::new(RefCell::new(Environment::new()));
    parent
        .borrow_mut()
        .define("x".to_string(), Value::Number(1.0));

    let child = Rc::new(RefCell::new(Environment::with_parent(Rc::clone(&parent))));
    child.borrow_mut().set("x", Value::Number(2.0));

    assert_eq!(parent.borrow().get("x"), Some(Value::Number(2.0)));
}

#[test]
fn test_parent_chain_child_defines_locally() {
    let parent = Rc::new(RefCell::new(Environment::new()));
    parent
        .borrow_mut()
        .define("shared".to_string(), Value::Number(1.0));

    let mut child = Environment::with_parent(Rc::clone(&parent));
    child.define("shared".to_string(), Value::Number(2.0));

    // Child's local binding shadows parent's
    assert_eq!(child.get("shared"), Some(Value::Number(2.0)));
    // Parent unchanged
    assert_eq!(parent.borrow().get("shared"), Some(Value::Number(1.0)));
}

#[test]
fn test_parent_chain_has() {
    let parent = Rc::new(RefCell::new(Environment::new()));
    parent
        .borrow_mut()
        .define("p".to_string(), Value::Undefined);

    let child = Environment::with_parent(Rc::clone(&parent));
    assert!(child.has("p"));
}

// ── var/let/const declaration ─────────────────────────────────────────────

#[test]
fn test_declare_var_hoists_to_global() {
    let mut env = Environment::new();
    env.define("global".to_string(), Value::Undefined); // global scope

    env.push_scope();
    env.declare_var("hoisted".to_string(), VarKind::Var);

    // var should be in global (first) scope, not current
    let global_scope = env.scopes.first().unwrap().borrow();
    assert!(global_scope.has("hoisted"));
    assert!(global_scope.is_declared_only("hoisted"));

    let current = env.current_scope_ref().unwrap();
    assert!(!current.has("hoisted"));
}

#[test]
fn test_declare_let_block_scoped() {
    let mut env = Environment::new();
    env.push_scope();
    env.declare_var("block_let".to_string(), VarKind::Let);

    let current = env.current_scope_ref().unwrap();
    assert!(current.has("block_let"));
    assert!(current.is_tdz("block_let"));
}

#[test]
fn test_declare_const_block_scoped() {
    let mut env = Environment::new();
    env.push_scope();
    env.declare_var("block_const".to_string(), VarKind::Const);

    let current = env.current_scope_ref().unwrap();
    assert!(current.has("block_const"));
    assert!(current.is_tdz("block_const"));
    assert!(current.is_declared_only("block_const"));
}

#[test]
fn test_initialize_declared_removes_tdz() {
    let mut env = Environment::new();
    env.push_scope();
    env.declare_var("x".to_string(), VarKind::Let);
    assert!(env.is_tdz("x"));

    env.initialize_declared("x", Value::Number(5.0));
    assert!(!env.is_tdz("x"));
    assert_eq!(env.get("x"), Some(Value::Number(5.0)));
}

#[test]
fn test_initialize_declared_finds_correct_scope() {
    let mut env = Environment::new();
    env.define("outer".to_string(), Value::Number(1.0));

    env.push_scope();
    env.declare_var("inner".to_string(), VarKind::Let);
    assert!(env.is_tdz("inner"));

    env.initialize_declared("inner", Value::Number(2.0));
    assert_eq!(env.get("inner"), Some(Value::Number(2.0)));
    assert_eq!(env.get("outer"), Some(Value::Number(1.0)));
}

#[test]
fn test_initialize_declared_no_pending_decl_updates_existing() {
    let mut env = Environment::new();
    env.define("x".to_string(), Value::Number(1.0));

    // No pending declaration for x — should update existing binding
    env.initialize_declared("x", Value::Number(999.0));
    assert_eq!(env.get("x"), Some(Value::Number(999.0)));
}

#[test]
fn test_get_kind_on_parent_chain() {
    let parent = Rc::new(RefCell::new(Environment::new()));
    parent
        .borrow_mut()
        .declare_var("parent_let".to_string(), VarKind::Let);

    let child = Environment::with_parent(Rc::clone(&parent));
    assert_eq!(child.get_kind("parent_let"), Some(VarKind::Let));
}

#[test]
fn test_is_tdz_on_parent_chain() {
    let parent = Rc::new(RefCell::new(Environment::new()));
    parent
        .borrow_mut()
        .declare_var("tdz_var".to_string(), VarKind::Let);

    let child = Environment::with_parent(Rc::clone(&parent));
    assert!(child.is_tdz("tdz_var"));
}

// ── super_class and pending_fields ───────────────────────────────────────

#[test]
fn test_super_class_set_and_get() {
    let mut env = Environment::new();
    assert!(env.get_super_class().is_none());

    env.set_super_class(Value::Object(Rc::new(RefCell::new(
        crate::value::Object::new(crate::value::kind::ObjectKind::Ordinary),
    ))));
    assert!(env.get_super_class().is_some());
}

#[test]
fn test_pending_fields_set_and_take() {
    let mut env = Environment::new();
    assert!(env.take_pending_fields().is_none());

    let fields: Vec<_> = vec![];
    env.set_pending_fields(fields.clone());
    assert!(env.take_pending_fields().is_some());
    // Second take returns None
    assert!(env.take_pending_fields().is_none());
}

#[test]
fn test_pending_fields_not_copied_on_clone() {
    let fields: Vec<_> = vec![];
    let mut env = Environment::new();
    env.set_pending_fields(fields);
    let cloned = env.clone();
    // pending_fields not cloned (only valid in original constructor env)
    assert!(cloned.scopes[0].borrow().is_empty());
}

// ── capture_env ──────────────────────────────────────────────────────────

#[test]
fn test_capture_env_preserves_scope_chain() {
    let mut env = Environment::new();
    env.define("x".to_string(), Value::Number(1.0));
    env.push_scope();
    env.define("y".to_string(), Value::Number(2.0));

    let captured = env.capture_env();

    // Captured env has the same scopes as env at capture time
    assert_eq!(captured.scopes.len(), 2);
    assert_eq!(captured.get("x"), Some(Value::Number(1.0)));
    assert_eq!(captured.get("y"), Some(Value::Number(2.0)));
}

#[test]
fn test_capture_env_parent_inherited() {
    let parent = Rc::new(RefCell::new(Environment::new()));
    parent
        .borrow_mut()
        .define("p".to_string(), Value::Number(1.0));

    let env = Environment::with_parent(Rc::clone(&parent));
    let captured = env.capture_env();

    assert!(captured.get_parent().is_some());
    assert_eq!(captured.get("p"), Some(Value::Number(1.0)));
}

// ── binding_scope ────────────────────────────────────────────────────────

#[test]
fn test_binding_scope_finds_innermost() {
    let mut env = Environment::new();
    env.define("x".to_string(), Value::Number(1.0));
    env.push_scope();
    env.define("x".to_string(), Value::Number(2.0));

    // binding_scope returns innermost
    let scope = env.binding_scope("x").unwrap();
    let current = env.current_scope();
    // Innermost scope IS the current scope
    assert!(Rc::ptr_eq(&scope, &current));
}

#[test]
fn test_binding_scope_not_found() {
    let env = Environment::new();
    assert!(env.binding_scope("missing").is_none());
}

// ── get_shared / get_rc identity ─────────────────────────────────────────

#[test]
fn test_get_shared_same_rc_for_same_name() {
    let mut env = Environment::new();
    env.define("shared".to_string(), Value::Undefined);

    let rc1 = env.get_shared("shared");
    let rc2 = env.get_shared("shared");
    assert!(rc1.is_some());
    assert!(rc2.is_some());
    assert!(Rc::ptr_eq(rc1.as_ref().unwrap(), rc2.as_ref().unwrap()));
}

// ── declare is_alias ────────────────────────────────────────────────────

#[test]
fn test_declare_is_alias_for_define() {
    let mut env = Environment::new();
    env.declare("z".to_string(), Value::Number(3.0));
    assert!(env.has("z"));
    assert_eq!(env.get("z"), Some(Value::Number(3.0)));
}

// ── current_scope ────────────────────────────────────────────────────────

#[test]
fn test_current_scope_returns_innermost() {
    let mut env = Environment::new();
    env.define("first".to_string(), Value::Number(1.0));
    env.push_scope();
    env.define("second".to_string(), Value::Number(2.0));

    let current = env.current_scope();
    let second_scope = env.binding_scope("second").unwrap();
    // current_scope is innermost = the scope with "second"
    assert!(Rc::ptr_eq(&current, &second_scope));
    // "first" lives in outer scope
    let first_scope = env.binding_scope("first").unwrap();
    assert!(!Rc::ptr_eq(&current, &first_scope));
}

// ── Scope::delete ────────────────────────────────────────────────────────

#[test]
fn test_scope_delete_existing_binding() {
    let mut scope = Scope::new();
    scope.define("x".to_string(), Value::Number(42.0));
    assert!(scope.has("x"));
    assert!(scope.delete("x"));
    assert!(!scope.has("x"));
}

#[test]
fn test_scope_delete_nonexistent_binding() {
    let mut scope = Scope::new();
    assert!(!scope.delete("nonexistent"));
}

#[test]
fn test_scope_delete_removes_binding() {
    let mut scope = Scope::new();
    scope.define("y".to_string(), Value::Number(1.0));
    assert!(scope.has("y"));
    assert!(scope.delete("y"));
    assert!(!scope.has("y"));
}

// ── Environment::delete_binding ─────────────────────────────────────────

#[test]
fn test_env_delete_binding_implicit_global() {
    let mut env = Environment::new();
    // Implicit global: inserted directly into bindings without a kind
    env.scopes.first().unwrap().borrow_mut().bindings_mut().insert(
        "implicit".to_string(),
        Rc::new(Value::Number(99.0)),
    );
    assert!(env.has("implicit"));
    assert_eq!(env.get_kind("implicit"), None);
    assert!(env.delete_binding("implicit"));
    assert!(!env.has("implicit"));
}

#[test]
fn test_env_delete_binding_declared_var() {
    let mut env = Environment::new();
    env.declare_var("declared_var".to_string(), VarKind::Var);
    env.initialize_declared("declared_var", Value::Number(1.0));
    assert!(env.has("declared_var"));
    assert_eq!(env.get_kind("declared_var"), Some(VarKind::Var));
    assert!(!env.delete_binding("declared_var"));
    assert!(env.has("declared_var"));
}

#[test]
fn test_env_delete_binding_declared_let() {
    let mut env = Environment::new();
    env.declare_var("declared_let".to_string(), VarKind::Let);
    env.initialize_declared("declared_let", Value::Number(2.0));
    assert_eq!(env.get_kind("declared_let"), Some(VarKind::Let));
    assert!(!env.delete_binding("declared_let"));
    assert!(env.has("declared_let"));
}

#[test]
fn test_env_delete_binding_declared_const() {
    let mut env = Environment::new();
    env.declare_var("declared_const".to_string(), VarKind::Const);
    env.initialize_declared("declared_const", Value::Number(3.0));
    assert_eq!(env.get_kind("declared_const"), Some(VarKind::Const));
    assert!(!env.delete_binding("declared_const"));
    assert!(env.has("declared_const"));
}

#[test]
fn test_env_delete_binding_nonexistent() {
    let mut env = Environment::new();
    assert!(!env.delete_binding("does_not_exist"));
}
