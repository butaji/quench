use crate::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModulePhase {
    Loading,
    Linking,
    Deferred,
    Evaluating,
    EvaluatingAsync,
    Evaluated,
    Errored,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ModuleOutcome {
    Pending(ModulePhase),
    Deferred(Value),
    Evaluated(Value),
    Errored(Value),
}

#[derive(Clone, Debug)]
pub(crate) struct ModuleRecord {
    pub(crate) outcome: ModuleOutcome,
    deferred_namespace: Option<Value>,
    evaluation_promise: Option<Value>,
    waiters: Vec<Value>,
    pending_namespace: Option<Value>,
}

impl ModuleRecord {
    pub(crate) fn loading(import_promise: Value) -> Self {
        Self {
            outcome: ModuleOutcome::Pending(ModulePhase::Loading),
            deferred_namespace: None,
            evaluation_promise: None,
            waiters: vec![import_promise],
            pending_namespace: None,
        }
    }

    pub(crate) fn evaluating_root(namespace: Value, import_promise: Value) -> Self {
        Self {
            outcome: ModuleOutcome::Pending(ModulePhase::Evaluating),
            deferred_namespace: None,
            evaluation_promise: None,
            waiters: vec![import_promise],
            pending_namespace: Some(namespace),
        }
    }

    pub(crate) fn evaluating_static() -> Self {
        Self {
            outcome: ModuleOutcome::Pending(ModulePhase::Evaluating),
            deferred_namespace: None,
            evaluation_promise: None,
            waiters: Vec::new(),
            pending_namespace: None,
        }
    }

    pub(crate) fn materialized(namespace: Value) -> Self {
        Self {
            outcome: ModuleOutcome::Evaluated(namespace),
            deferred_namespace: None,
            evaluation_promise: None,
            waiters: vec![],
            pending_namespace: None,
        }
    }

    pub(crate) fn deferred(namespace: Value) -> Self {
        Self {
            outcome: ModuleOutcome::Deferred(namespace),
            deferred_namespace: Some(namespace),
            evaluation_promise: None,
            waiters: Vec::new(),
            pending_namespace: None,
        }
    }

    pub(crate) fn phase(&self) -> ModulePhase {
        match self.outcome {
            ModuleOutcome::Pending(phase) => phase,
            ModuleOutcome::Deferred(_) => ModulePhase::Deferred,
            ModuleOutcome::Evaluated(_) => ModulePhase::Evaluated,
            ModuleOutcome::Errored(_) => ModulePhase::Errored,
        }
    }

    pub(crate) fn begin_linking(&mut self) -> bool {
        self.transition(ModulePhase::Loading, ModulePhase::Linking)
    }

    pub(crate) fn begin_evaluation(&mut self) -> bool {
        self.transition(ModulePhase::Linking, ModulePhase::Evaluating)
    }

    pub(crate) fn begin_deferred_evaluation(&mut self) -> Option<Value> {
        let ModuleOutcome::Deferred(namespace) = self.outcome else {
            return None;
        };
        self.outcome = ModuleOutcome::Pending(ModulePhase::Evaluating);
        self.pending_namespace = Some(namespace);
        Some(namespace)
    }

    pub(crate) fn deferred_namespace(&self) -> Option<Value> {
        self.deferred_namespace
    }

    pub(crate) fn cache_deferred_namespace(&mut self, namespace: Value) {
        self.deferred_namespace = Some(namespace);
    }

    pub(crate) fn track_evaluation_promise(&mut self, promise: Value) {
        self.evaluation_promise = Some(promise);
    }

    pub(crate) fn evaluation_promise(&self) -> Option<Value> {
        self.evaluation_promise
    }

    pub(crate) fn pending_namespace(&self) -> Option<Value> {
        self.pending_namespace
    }

    pub(crate) fn begin_async_evaluation(&mut self, namespace: Value) {
        self.outcome = ModuleOutcome::Pending(ModulePhase::EvaluatingAsync);
        self.pending_namespace = Some(namespace);
    }

    pub(crate) fn add_waiter(&mut self, import_promise: Value) -> bool {
        if !matches!(self.outcome, ModuleOutcome::Pending(_)) {
            return false;
        }
        self.waiters.push(import_promise);
        true
    }

    pub(crate) fn evaluate(&mut self, namespace: Value) -> Option<Vec<Value>> {
        if !matches!(
            self.phase(),
            ModulePhase::Evaluating | ModulePhase::EvaluatingAsync
        ) {
            return None;
        }
        self.outcome = ModuleOutcome::Evaluated(namespace);
        self.pending_namespace = None;
        self.evaluation_promise = None;
        Some(std::mem::take(&mut self.waiters))
    }

    pub(crate) fn evaluate_root(&mut self) -> Option<(Value, Vec<Value>)> {
        if self.phase() != ModulePhase::Evaluating {
            return None;
        }
        let namespace = self.pending_namespace.take()?;
        self.outcome = ModuleOutcome::Evaluated(namespace);
        Some((namespace, std::mem::take(&mut self.waiters)))
    }

    pub(crate) fn fail(&mut self, reason: Value) -> Option<Vec<Value>> {
        if matches!(self.outcome, ModuleOutcome::Pending(_)) {
            self.outcome = ModuleOutcome::Errored(reason);
            self.pending_namespace = None;
            self.evaluation_promise = None;
            Some(std::mem::take(&mut self.waiters))
        } else {
            None
        }
    }

    pub(crate) fn roots(&self) -> impl Iterator<Item = Value> {
        let outcome = match self.outcome {
            ModuleOutcome::Deferred(value)
            | ModuleOutcome::Evaluated(value)
            | ModuleOutcome::Errored(value) => Some(value),
            ModuleOutcome::Pending(_) => None,
        };
        outcome
            .into_iter()
            .chain(self.deferred_namespace)
            .chain(self.pending_namespace)
            .chain(self.evaluation_promise)
            .chain(self.waiters.iter().copied())
    }

    fn transition(&mut self, from: ModulePhase, to: ModulePhase) -> bool {
        if self.phase() != from {
            return false;
        }
        self.outcome = ModuleOutcome::Pending(to);
        true
    }
}
