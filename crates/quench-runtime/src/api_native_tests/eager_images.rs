//! Eager native images: the publisher's tiers and the frozen whole-program facts it folds
//! (method atoms, globals, prototype values, heap-string constants) with their retirement.
use super::*;

impl<H: crate::Host> crate::Runtime<H> {
    /// Records a method-cache name on the runtime's VM.
    fn name_method_atom(&mut self, atom: crate::bytecode::Atom) {
        self.vm.name_method_atom(atom);
    }
}

#[cfg(feature = "aot")]
#[test]
fn eager_publication_reports_general_dispatch_and_typed_regions() {
    // A loop the static typed lowering admits: the eager publisher must select
    // the general-dispatch baseline and inline a typed region, not fall back to
    // a per-PC call-threaded body for the hot function.
    let source = r#"
        function sum(n) { let s = 0; for (let i = 0; i < n; i++) { s += i * 2; } return s; }
        globalThis.answer = sum(1000);
    "#;
    let program = Engine::specialize_unspecialized(source, "eager-report.js").unwrap();
    // The counts describe this publication's lowering; stencil publication has
    // its own shapes and is measured separately.
    let options = crate::NativeJsCodegenOptions { backend: crate::NativeJsBackend::Cranelift, ..Default::default() };
    let native = crate::NativeJsProgram::compile_eager_with_options(program.clone(), options).unwrap();
    let counts = native.publication_counts();
    assert!(counts.functions >= 2, "program functions: {counts:?}");
    assert!(counts.general_dispatch >= 1, "no function used general dispatch: {counts:?}");
    assert!(counts.typed_regions >= 1, "no typed region was inlined: {counts:?}");
    assert!(counts.tier_specialized >= 1, "no function reached the specialized tier: {counts:?}");
    assert!(counts.tier_call_threaded >= 1, "the root stays on tier zero: {counts:?}");
    assert!(counts.semantic_entry_sites > 0, "a fallback must still exist: {counts:?}");
    let mut runtime = Runtime::new(TestHost::default());
    runtime.set_execution_mode(crate::ExecutionMode::NativeOnly);
    runtime.execute_native(&native).unwrap();
    assert_eq!(runtime.native_execution_rejections(), 0, "counts: {counts:?}");
    let global = runtime.global_root().unwrap();
    let key = runtime.string_rooted("answer");
    let answer = runtime.get_property_rooted(global, key).unwrap();
    assert_eq!(runtime.rooted_value(answer).unwrap().as_number(), Some(999_000.0));
}

#[cfg(feature = "aot")]
fn compile_frozen(source: &str) -> (ResidualProgram, crate::NativeJsProgram<TestHost>) {
    let program = Engine::specialize(source, "frozen.js").unwrap();
    let options = crate::NativeJsCodegenOptions { backend: crate::NativeJsBackend::Cranelift, ..Default::default() };
    let native = crate::NativeJsProgram::compile_eager_with_options(program.clone(), options).unwrap();
    (program, native)
}

#[cfg(feature = "aot")]
#[test]
fn a_frozen_image_folds_the_unnamed_method_atoms_of_its_stores() {
    let (program, native) = compile_frozen(
        "var o={a:1,b:2}; function write(x){for(var i=0;i<3;i++){o.a=x}} write(2); write(3);");
    let a = program.atoms.iter().position(|name| name == "a").unwrap() as crate::bytecode::Atom;
    assert!(native.frozen_atoms().contains(&a), "held atoms: {:?}", native.frozen_atoms());
}

#[cfg(feature = "aot")]
#[test]
fn a_named_method_atom_retires_the_image_and_the_shared_body_runs() {
    let (program, native) = compile_frozen(
        "var o={a:1,b:2}; var owner={a(){return 1},m(){return 2}}; \
         function write(x){for(var i=0;i<3;i++){o.a=x} return o.a} \
         globalThis.answer = write(2) + write(3) + owner.a(); \
         if (globalThis.answer !== 6) throw Error('answer ' + globalThis.answer);");
    let a = program.atoms.iter().position(|name| name == "a").unwrap() as crate::bytecode::Atom;
    assert!(native.frozen_atoms().contains(&a), "held atoms: {:?}", native.frozen_atoms());
    let mut runtime = Runtime::new(TestHost::default());
    runtime.name_method_atom(a);
    runtime.set_execution_mode(crate::ExecutionMode::NativeOnly);
    runtime.execute_native(&native).unwrap();
    assert!(native.frozen_invalidated(), "a named atom retires the image");
    assert_eq!(runtime.native_execution_rejections(), 0, "the deopt stays inside the image");
}

#[cfg(feature = "aot")]
#[test]
fn a_later_method_cache_name_retires_the_image() {
    let (program, native) = compile_frozen(
        "var o={a:1}; function write(x){for(var i=0;i<3;i++){o.a=x}} write(2);");
    let a = program.atoms.iter().position(|name| name == "a").unwrap() as crate::bytecode::Atom;
    let mut runtime = Runtime::new(TestHost::default());
    runtime.set_execution_mode(crate::ExecutionMode::NativeOnly);
    runtime.execute_native(&native).unwrap();
    assert!(!native.frozen_invalidated(), "the image holds while no cache names `a`");
    runtime.name_method_atom(a);
    assert!(native.frozen_invalidated(), "the subscription retires the image");
}

#[cfg(feature = "aot")]
#[test]
fn a_frozen_image_folds_whole_program_globals_and_guards_a_change() {
    // `SCALE` is written once at its declaration, so the closed program gives it 3 at
    // every read. The second call changes it through the global object (not a `StoreName`),
    // so the read-site guard fails and the shared body computes the guest answer.
    let source = r#"
        var SCALE = 3;
        function scale(x) { let s = 0; for (let i = 0; i < 4; i++) { s = (s + x * SCALE) | 0; } return s; }
        globalThis.answer = scale(2);
        globalThis.SCALE = 5;
        globalThis.answer2 = scale(2);
        if (globalThis.answer !== 24 || globalThis.answer2 !== 40) throw Error(globalThis.answer + "," + globalThis.answer2);
    "#;
    let (program, native) = compile_frozen(source);
    let counts = native.publication_counts();
    assert!(counts.frozen_globals >= 1, "no whole-program global folded: {counts:?}");
    assert!(counts.tier_specialized >= 1, "no specialized tier: {counts:?}");
    let scale = program.atoms.iter().position(|name| name == "SCALE").unwrap() as crate::bytecode::Atom;
    assert!(native.frozen_global_atoms_for_test().contains(&scale), "atoms: {:?}", native.frozen_global_atoms_for_test());
    let mut runtime = Runtime::new(TestHost::default());
    runtime.set_execution_mode(crate::ExecutionMode::NativeOnly);
    runtime.execute_native(&native).unwrap();
    assert_eq!(runtime.native_execution_rejections(), 0, "counts: {counts:?}");
}

#[cfg(feature = "aot")]
#[test]
fn a_frozen_image_folds_a_prototype_value_and_guards_a_redefinition() {
    // `C.prototype.k` is stored once with an immediate, so the closed program gives every
    // read of `k` the value 5. Redefining it through `defineProperty` (a call, not a store)
    // makes the read-site guard fail; the shared body reads 7 instead.
    let source = r#"
        function C() { this.x = 1; }
        C.prototype.k = 5;
        var c = new C();
        function read(n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + c.k) | 0; } return s; }
        globalThis.answer = read(3);
        Object.defineProperty(C.prototype, "k", { value: 7, configurable: true });
        globalThis.answer2 = read(3);
        if (globalThis.answer !== 15 || globalThis.answer2 !== 21) throw Error(globalThis.answer + "," + globalThis.answer2);
    "#;
    let (program, native) = compile_frozen(source);
    let counts = native.publication_counts();
    assert!(counts.frozen_fields >= 1, "no prototype value folded: {counts:?}");
    assert!(counts.tier_specialized >= 1, "no specialized tier: {counts:?}");
    let k = program.atoms.iter().position(|name| name == "k").unwrap() as crate::bytecode::Atom;
    assert!(native.frozen_field_atoms_for_test().contains(&k), "atoms: {:?}", native.frozen_field_atoms_for_test());
    let mut runtime = Runtime::new(TestHost::default());
    runtime.set_execution_mode(crate::ExecutionMode::NativeOnly);
    runtime.execute_native(&native).unwrap();
    assert_eq!(runtime.native_execution_rejections(), 0, "counts: {counts:?}");
}

#[cfg(feature = "aot")]
#[test]
fn the_aot_tier_ladder_selects_the_general_body_after_invalidation() {
    // The loop function's entry is a dispatcher over T2 (typed), T1 (general) and T0
    // (call-threaded), all resident. It reads the image's invalidation word: zero selects
    // the specialized body; a retired image selects the general baseline. Guest results
    // are identical either way.
    let source = r#"
        var SCALE = 3;
        function scale(x) { let s = 0; for (let i = 0; i < 4; i++) { s = (s + x * SCALE) | 0; } return s; }
        globalThis.answer = scale(2);
        if (globalThis.answer !== 24) throw Error("answer " + globalThis.answer);
    "#;
    let (_program, native) = compile_frozen(source);
    let counts = native.publication_counts();
    assert!(counts.tier_specialized >= 1, "no specialized tier: {counts:?}");
    let mut runtime = Runtime::new(TestHost::default());
    runtime.set_execution_mode(crate::ExecutionMode::NativeOnly);
    runtime.execute_native(&native).unwrap();
    assert_eq!(native.last_tier(), 2, "the specialized body must run first");
    native.invalidate_frozen();
    assert!(native.frozen_invalidated());
    let mut runtime = Runtime::new(TestHost::default());
    runtime.set_execution_mode(crate::ExecutionMode::NativeOnly);
    runtime.execute_native(&native).unwrap();
    assert_eq!(native.last_tier(), 1, "a retired image must select the general body");
    assert_eq!(runtime.native_execution_rejections(), 0);
}

#[cfg(feature = "aot")]
#[test]
fn a_frozen_image_materializes_heap_string_constants_and_deopts_on_retirement() {
    // The image allocates the literal once at load and the typed region reads the block
    // behind the invalidation word; a retired image takes the general tier and still
    // computes the same answer.
    let source = r#"
        function read(n) { let s = 0; for (let i = 0; i < n; i++) { let k = "abc"; if (k.length === 3) s++; } return s; }
        globalThis.answer = read(4);
        if (globalThis.answer !== 4) throw Error("answer " + globalThis.answer);
    "#;
    let (_program, native) = compile_frozen(source);
    assert!(native.materialized_constants() >= 1, "no heap constant materialized");
    let mut runtime = Runtime::new(TestHost::default());
    runtime.set_execution_mode(crate::ExecutionMode::NativeOnly);
    runtime.execute_native(&native).unwrap();
    assert_eq!(native.last_tier(), 2, "specialized tier first");
    native.invalidate_frozen();
    let mut runtime = Runtime::new(TestHost::default());
    runtime.set_execution_mode(crate::ExecutionMode::NativeOnly);
    runtime.execute_native(&native).unwrap();
    assert_eq!(native.last_tier(), 1, "general tier after retirement");
    assert_eq!(runtime.native_execution_rejections(), 0);
}

#[cfg(feature = "aot")]
#[test]
fn functions_past_the_set_width_keep_a_conservative_map() {
    let locals = crate::compile::liveness::ROOT_MAP_REGISTERS as usize + 1;
    let parameters = (0..locals).map(|index| format!("p{index}")).collect::<Vec<_>>().join(", ");
    let source = format!("function wide({parameters}) {{ return p0; }}\nwide(1);\n");
    let program = Engine::specialize(&source, "wide-frame.js").unwrap();
    let wide = (0..program.function_count()).find(|index| program.function_get(*index)
        .is_some_and(|function| function.registers > crate::compile::liveness::ROOT_MAP_REGISTERS))
        .expect("the wide body");
    let masks = program.register_root_masks(wide as u32).expect("a body past the set width keeps a conservative map");
    assert!(masks.get(0).is_none(), "the conservative map has no per-pc entry");
    let options = crate::NativeJsCodegenOptions {
        backend: crate::NativeJsBackend::Cranelift, ..Default::default() };
    let native = crate::NativeJsProgram::<TestHost>::compile_eager_with_options(program, options).unwrap();
    assert!(native.publication_counts().wide_frame_functions >= 1,
        "{:?}", native.publication_counts());
}

#[cfg(feature = "aot")]
#[test]
fn a_function_past_the_old_width_still_has_a_stack_map() {
    // Between the old 256-register width and the current 512, the root map must exist.
    let parameters = (0..300).map(|index| format!("p{index}")).collect::<Vec<_>>().join(", ");
    let source = format!("function wide({parameters}) {{ return p0; }}\nwide(1);\n");
    let program = Engine::specialize(&source, "wide-frame-half.js").unwrap();
    let options = crate::NativeJsCodegenOptions {
        backend: crate::NativeJsBackend::Cranelift, ..Default::default() };
    let native = crate::NativeJsProgram::<TestHost>::compile_eager_with_options(program, options).unwrap();
    assert_eq!(native.publication_counts().wide_frame_functions, 0,
        "{:?}", native.publication_counts());
}
