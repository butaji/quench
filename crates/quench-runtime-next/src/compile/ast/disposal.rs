use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn push_disposal_scope(&mut self) {
        self.disposal_scopes.push(DisposalScope::default());
    }

    pub(super) fn pop_disposal_scope(&mut self) {
        self.disposal_scopes.pop();
    }

    pub(super) fn has_disposal_stack(&self) -> bool {
        self.disposal_scopes
            .last()
            .is_some_and(|scope| scope.stack.is_some())
    }

    pub(super) fn emit_disposal_scope_exit(&mut self, start: u32, end: u32, error: Atom) {
        let (stack_atom, asynchronous) = self
            .disposal_scopes
            .last()
            .and_then(|scope| scope.stack.map(|stack| (stack, scope.asynchronous)))
            .expect("disposal handler has a stack local");
        self.emit_disposal();
        let normal_exit = self.emit(Op::Jump, 0, 0, 0, 0);
        let exceptional_target = self.code.len() as u32;
        let original_error = self.load_atom(error);
        let stack = self.load_atom(stack_atom);
        let skip_uninitialized_stack = self.emit(Op::JumpFalse, stack, 0, 0, 0);
        if asynchronous {
            self.emit_disposal();
        } else {
            self.emit_disposal_with_completion(stack, original_error);
        }
        self.patch(skip_uninitialized_stack);
        self.emit(Op::Throw, original_error, 0, 0, 0);
        let end_target = self.code.len() as u32;
        self.patch_to(normal_exit, end_target);
        self.handlers.push(crate::bytecode::Handler {
            start,
            end,
            target: exceptional_target,
            slot: self.local_slot(error),
            return_target: None,
            return_slot: None,
            with_depth: self.with_depth,
        });
    }

    fn emit_disposal_with_completion(&mut self, stack: Register, completion: Register) {
        let method_atom = self.owner.atom("\0rqj:disposeWithCompletion");
        let cache = self.owner.cache_site();
        let site = self.owner.method_sites.len() as u32;
        self.owner
            .method_sites
            .push((method_atom, cache, vec![completion], None));
        let result = self.reg();
        self.emit(Op::CallMethod, result, stack, 0, site);
    }

    pub(crate) fn emit_function_disposal_scope_exit(&mut self, start: u32, end: u32) -> bool {
        if !self.has_disposal_stack() {
            return false;
        }
        let error = self.hidden_local("\0rqj:function-using-error");
        self.emit_disposal_scope_exit(start, end, error);
        self.pop_disposal_scope();
        true
    }

    pub(super) fn ensure_disposable_stack(&mut self, asynchronous: bool) -> Register {
        let scope = self
            .disposal_scopes
            .last_mut()
            .expect("function disposal scope is present");
        scope.asynchronous |= asynchronous;
        if let Some(atom) = scope.stack {
            return self.load_atom(atom);
        }
        let atom = self.hidden_local("\0rqj:disposable-stack");
        self.disposal_scopes
            .last_mut()
            .expect("function disposal scope is present")
            .stack = Some(atom);
        let constructor = self.load_name("DisposableStack");
        let stack = self.reg();
        self.emit(Op::Construct, stack, constructor, 0, 0);
        self.store_atom(atom, stack);
        self.load_atom(atom)
    }

    pub(crate) fn emit_disposal(&mut self) {
        let Some(scope) = self.disposal_scopes.last_mut() else {
            return;
        };
        let Some(atom) = scope.stack else {
            return;
        };
        let asynchronous = scope.asynchronous;
        let stack = self.load_atom(atom);
        let method_atom = self.owner.atom(if asynchronous {
            "disposeAsync"
        } else {
            "dispose"
        });
        let cache = self.owner.cache_site();
        let site = self.owner.method_sites.len() as u32;
        self.owner
            .method_sites
            .push((method_atom, cache, Vec::new(), None));
        let result = self.reg();
        self.emit(Op::CallMethod, result, stack, 0, site);
        if asynchronous {
            let awaited = self.reg();
            self.emit(Op::Await, awaited, result, 0, 0);
        }
    }

    pub(super) fn call_disposable_method(
        &mut self,
        stack: Register,
        name: &str,
        value: Register,
    ) -> Register {
        let atom = self.owner.atom(name);
        let cache = self.owner.cache_site();
        let site = self.owner.method_sites.len() as u32;
        self.owner
            .method_sites
            .push((atom, cache, vec![value], None));
        let result = self.reg();
        self.emit(Op::CallMethod, result, stack, 0, site);
        result
    }
}
