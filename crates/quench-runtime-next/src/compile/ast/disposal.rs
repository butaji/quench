use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn push_disposal_scope(&mut self) {
        self.disposal_scopes.push(DisposalScope::default());
    }

    pub(super) fn pop_disposal_scope(&mut self) {
        self.disposal_scopes.pop();
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
