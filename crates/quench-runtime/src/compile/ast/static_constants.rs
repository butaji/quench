//! Ershov's division for `const` bindings: a `const` whose initializer is static holds that
//! value at every read its declaration precedes in the binding's scope, so the static
//! expressions reading it fold at parse (`binding_time::expression_in`), once, and every tier
//! reads the residual constant. A computed property key whose value is static names its
//! property the same way (`static_property_key`).

use super::*;
use super::destructure::AssignmentReference;

impl FunctionCompiler<'_, '_> {
    /// The property a computed key names where the key's value is static (Ershov's division,
    /// `static_value`): the array index it converts to, else its name. However the program
    /// spells a static key (`x.a`, `x["a"]`, `` x[`a`] ``, a `const`), it compiles to the
    /// one access that key names.
    pub(super) fn static_property_key(&mut self, key: &Expression<'_>) -> Option<StaticKey> {
        let name = match self.static_value(key)? {
            Constant::Number(_) => return Self::literal_index_key(key).map(StaticKey::Index),
            Constant::String(name) => name,
            _ => return None,
        };
        Some(match crate::vm::array_index(&name).and_then(|index| i32::try_from(index).ok()) {
            Some(index) => StaticKey::Index(index),
            None => StaticKey::Name(name),
        })
    }

    /// The assignment target `object[key]`: where `key` is a static name, the field it names,
    /// as `object.name` would; else the key's value.
    pub(super) fn computed_reference(&mut self, member: &ComputedMemberExpression<'_>) -> AssignmentReference {
        if let Some(StaticKey::Name(name)) = self.static_property_key(&member.expression) {
            let atom = self.owner.atom(&name);
            if matches!(&member.object, Expression::ThisExpression(_)) {
                return AssignmentReference::ThisField(atom);
            }
            return AssignmentReference::Field { object: self.expression(&member.object), atom };
        }
        let object = self.expression(&member.object);
        let key = self.expression(&member.expression);
        AssignmentReference::Index { object, key }
    }

    /// Emits the read of `key` off `object` into `dst` where `key` is a static name (the
    /// member's static spelling's `GetField`); `false`, emitting nothing, for any other key.
    pub(super) fn static_field_read(&mut self, dst: Register, object: Register, key: &Expression<'_>) -> bool {
        let Some(StaticKey::Name(name)) = self.static_property_key(key) else { return false };
        let (atom, cache) = (self.owner.atom(&name), self.owner.cache_site());
        self.emit(Op::GetField, dst, FieldBase::register(object).0, cache, atom);
        true
    }

    /// Reads the callee `receiver[key]` into `callee`: the named field for a static name,
    /// else the key's value.
    pub(super) fn computed_callee_read(&mut self, callee: Register, receiver: Register, key: &Expression<'_>) {
        if !self.static_field_read(callee, receiver, key) {
            let key = self.expression_outside_optional_chain(key);
            self.emit(Op::GetIndex, callee, receiver, key, 0);
        }
    }

    /// The static value of `value`, reading the `const` bindings folded so far.
    pub(in crate::compile) fn static_value(&mut self, value: &Expression<'_>) -> Option<Constant> {
        binding_time::expression_in(value, &mut |name| self.static_constant(name))
    }

    /// The exits of a test whose truthiness is known before run time (`binding_time`): none
    /// where it is true, so control falls through, else the one jump that always leaves.
    /// `None` where the test is dynamic.
    pub(super) fn constant_exits(&mut self, value: &Expression<'_>) -> Option<super::expression::ConditionExits> {
        let truthy = binding_time::to_boolean(&self.static_value(value)?)?;
        Some(if truthy { Default::default() } else { smallvec::smallvec![self.emit(Op::Jump, 0, 0, 0, 0)] })
    }

    pub(super) fn static_binary(&mut self, value: &BinaryExpression<'_>) -> Option<Constant> {
        binding_time::binary_in(value, &mut |name| self.static_constant(name))
    }

    /// Records `const name = init` after its initialization: the later reads in the binding's
    /// scope read `init`'s static value. A direct `eval` may observe the binding, so a function
    /// that has one records nothing.
    pub(super) fn record_static_constant(
        &mut self,
        kind: VariableDeclarationKind,
        pattern: &BindingPattern<'_>,
        init: &Expression<'_>,
    ) {
        self.record_literal_singleton(pattern, init);
        let BindingPattern::BindingIdentifier(name) = pattern else { return };
        if kind != VariableDeclarationKind::Const || self.dynamic_eval {
            return;
        }
        let Some(constant) = self.static_value(init) else { return };
        let atom = self.owner.atom(name.name.as_str());
        if let Some(scope) = self.binding_scope(atom)
            && scope.kinds.get(&atom) == Some(&LexicalBindingKind::Immutable)
            && let Some(constants) = &mut scope.constants
        {
            constants.insert(atom, constant);
        }
    }

    /// The static value of the binding `name` reads here, if its scope folded one. An
    /// uninitialized shadowing binding is found first, and holds none.
    fn static_constant(&mut self, name: &str) -> Option<Constant> {
        if self.dynamic_eval || self.arguments_alias.as_deref() == Some(name) {
            return None;
        }
        let atom = self.owner.atom(name);
        if let Some(scope) = self.binding_scope(atom) {
            return scope.constants.as_ref()?.get(&atom).cloned();
        }
        self.folded_capture(atom)
    }

    /// The constant a read of `atom` here takes where it resolves to an enclosing function's
    /// literal singleton (`early::literal_singleton_vars`) that this closure was created after
    /// the initialization of: no binding of this function shadows it, and no `with` or eval
    /// can rebind it.
    pub(super) fn folded_capture(&mut self, atom: Atom) -> Option<Constant> {
        if self.with_depth != 0 || self.dynamic_eval || self.is_compiler_binding(atom) {
            return None;
        }
        let atom = self.parameter_binding_atom(atom);
        if self.active_lexical_binding(atom).is_some()
            || self.local_slots.contains_key(&atom)
            || self.function_scope.contains(&atom)
        {
            return None;
        }
        self.scopes.fold(atom)
    }

    /// Reads the capture `atom` names at `depth` and `slot` into a new register: its constant
    /// where the binding is a literal singleton this closure was created after
    /// (`folded_capture`), else a `LoadCapture`.
    pub(super) fn capture_read(&mut self, atom: Atom, depth: usize, slot: u16) -> Register {
        if let Some(constant) = self.folded_capture(atom) {
            return self.literal(constant);
        }
        let dst = self.reg();
        self.emit(Op::LoadCapture, dst, 0, 0, crate::bytecode::ImmediateLayout::capture_immediate(depth, slot));
        dst
    }

    /// Records `var name = literal` after its initialization where it is one of this
    /// function's literal singletons: closures created from here on fold its reads.
    fn record_literal_singleton(&mut self, pattern: &BindingPattern<'_>, init: &Expression<'_>) {
        let BindingPattern::BindingIdentifier(name) = pattern else { return };
        if self.literal_singletons.binary_search(&name.span.start).is_err() {
            return;
        }
        let Some(constant) = binding_time::expression(init) else { return };
        let atom = self.owner.atom(name.name.as_str());
        let mut folds = self.capture_folds.to_vec();
        folds.push((atom, constant));
        self.capture_folds = folds.into();
    }

    /// The innermost lexical scope binding `atom` that a read here resolves to, as
    /// `active_lexical_binding` resolves it.
    fn binding_scope(&mut self, atom: Atom) -> Option<&mut LexicalScope> {
        let with_depth = self.with_depth;
        self.lexical_scopes
            .iter_mut()
            .rev()
            .filter(|scope| with_depth == 0 || scope.with_depth >= with_depth)
            .find(|scope| scope.bindings.contains_key(&atom))
    }
}

/// The property a static computed key names (`FunctionCompiler::static_property_key`).
pub(super) enum StaticKey {
    Index(i32),
    Name(std::rc::Rc<str>),
}

#[cfg(test)]
mod tests {
    use crate::bytecode::Op;
    use crate::vm::EngineVm;
    use crate::test_host::TestHost;
    use crate::vm::Vm;
    use crate::Engine;
    use std::{cell::RefCell, rc::Rc};

    /// What `source` prints, the same interpreted and compiled.
    fn answer(source: &str) -> String {
        let program = Engine::specialize(source, "static-constants.js").unwrap();
        let mut answers = Vec::new();
        for mode in [crate::ExecutionMode::Interpreted, crate::ExecutionMode::Compiled] {
            let output = Rc::new(RefCell::new(Vec::new()));
            let mut vm = Vm::new(TestHost::writing_to(output.clone()));
            vm.set_execution_mode(mode);
            vm.execute(&program).unwrap();
            answers.push(output.borrow().join(","));
        }
        assert_eq!(answers[0], answers[1], "{source}");
        answers.swap_remove(0)
    }

    #[test]
    fn static_const_reads_fold_at_parse() {
        let source = "{ const A = 1 << 4; const B = A * 2; print(B + A); }";
        assert_eq!(answer(source), "48");
        let program = Engine::specialize(source, "static-constants.js").unwrap();
        assert!(program.functions[0].instructions().all(|instruction| instruction.op() != Op::Binary),
            "{}", program.disassemble());
    }

    #[test]
    fn static_const_folding_keeps_scoping_and_temporal_dead_zones() {
        for (source, expected) in [
            // A case entered past the declaration reads the binding uninitialized.
            ("function f(v){ switch(v){ case 0: const x = 1; case 1: return x + 1; } } \
              try { f(1); print(0) } catch (e) { print(e instanceof ReferenceError ? 1 : 2) }", "1"),
            ("function f(){ const x = 1; try { { const r = x + 1; const x = 2; return r } } \
              catch (e) { return e instanceof ReferenceError ? 7 : 8 } } print(f());", "7"),
            ("function f(){ const x = 1; with ({ x: 5 }) { return x + 1 } } print(f());", "6"),
            ("function f(){ let s = 0; for (let i = 0; i < 3; i++) { const k = 2; s += k * i } return s } print(f());", "6"),
            ("function f(){ const s = 'a' + 'b'; const t = s + 'c'; return t === 'abc' ? 1 : 0 } print(f());", "1"),
            ("function f(){ const x = 3; return eval('x') * 2 } print(f());", "6"),
            // Literal singletons: a hoisted closure reads the binding before its declaration
            // ran, a later one the literal; reassigned, shadowed or evaled names stay reads.
            ("function f(){ function e(){ return X } var a = e(); var X = 4; var g = function(){ return X + 1 }; \
              return [a, e(), g()].join() } print(f() + f());", ",4,5,4,5"),
            ("function f(){ var Z = 1; Z = 2; var Q = 3; return function(){ return Z + function(Q){ return Q }(9) }() } print(f());", "11"),
            ("function f(){ var Y = 1; return function(){ return eval('var Y = 7'), Y }() } print(f());", "7"),
        ] {
            assert_eq!(answer(source), expected, "{source}");
        }
    }
}
