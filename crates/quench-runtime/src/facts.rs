//! Facts shared by frontend queries and residualization.

use oxc::span::Span;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Fact<T> {
    Proven(T),
    Guarded { value: T, guard: Guard },
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReduceContext {
    Value,
    Place,
    Effect,
    Control,
    Define,
}

#[derive(Debug, PartialEq)]
pub struct SpanFacts<T> {
    entries: HashMap<(Span, ReduceContext), Fact<T>>,
}

impl<T> Default for SpanFacts<T> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

impl<T: Clone> SpanFacts<T> {
    pub fn insert(&mut self, span: Span, fact: Fact<T>) {
        self.insert_in_context(span, ReduceContext::Value, fact);
    }

    pub fn insert_in_context(&mut self, span: Span, context: ReduceContext, fact: Fact<T>) {
        self.entries.insert((span, context), fact);
    }

    pub fn query(&self, span: Span) -> Fact<T> {
        self.query_in_context(span, ReduceContext::Value)
    }

    pub fn query_in_context(&self, span: Span, context: ReduceContext) -> Fact<T> {
        self.entries
            .get(&(span, context))
            .cloned()
            .unwrap_or(Fact::Unknown)
    }

    pub fn merge(&mut self, other: Self) {
        for ((span, context), fact) in other.entries {
            self.insert_in_context(span, context, fact);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Guard {
    PlainObject,
    Number,
}

impl<T> Fact<T> {
    pub fn proven(value: T) -> Self {
        Self::Proven(value)
    }

    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown)
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct ProgramDb {
    pub constants: Vec<ConstantFact>,
    pub(crate) span_facts: SpanFacts<Constant>,
    pub scope_count: usize,
    pub symbol_count: usize,
    pub(crate) private_names: HashMap<Span, PrivateNameId>,
    pub(crate) strict: bool,
    pub(crate) in_function: bool,
    pub(crate) tail_calls: bool,
    pub(crate) eval_var_barrier: Vec<String>,
    pub(crate) eval_deletable: Vec<(String, u16)>,
    pub(crate) epochs: Epochs,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Epochs {
    shape: u32,
    prototype: u32,
    realm: u32,
    global: u32,
}

impl Epochs {
    pub fn shape(&self) -> u32 {
        self.shape
    }

    pub fn prototype(&self) -> u32 {
        self.prototype
    }

    pub fn realm(&self) -> u32 {
        self.realm
    }

    pub fn global(&self) -> u32 {
        self.global
    }

    pub fn bump_shape(&mut self) {
        self.shape = self.shape.wrapping_add(1);
    }

    pub fn bump_prototype(&mut self) {
        self.prototype = self.prototype.wrapping_add(1);
    }

    pub fn bump_realm(&mut self) {
        self.realm = self.realm.wrapping_add(1);
    }

    pub fn bump_global(&mut self) {
        self.global = self.global.wrapping_add(1);
    }
}

impl ProgramDb {
    pub(crate) fn record_fact_in_context(
        &mut self,
        span: Span,
        context: ReduceContext,
        fact: Fact<Constant>,
    ) {
        self.span_facts.insert_in_context(span, context, fact);
    }

    pub fn query_fact(&self, span: Span) -> Fact<Constant> {
        self.span_facts.query(span)
    }

    pub fn query_fact_in_context(&self, span: Span, context: ReduceContext) -> Fact<Constant> {
        self.span_facts.query_in_context(span, context)
    }

    pub fn insert_private_name(&mut self, span: Span, id: PrivateNameId) {
        self.private_names.insert(span, id);
    }

    pub fn private_name(&self, span: Span) -> Option<PrivateNameId> {
        self.private_names.get(&span).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PrivateNameId(pub u32);

#[derive(Debug, Clone, PartialEq)]
pub struct ConstantFact {
    pub value: Constant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Number(f64),
    Boolean(bool),
    String(String),
    BigInt(String),
    Null,
    Undefined,
}

#[cfg(test)]
mod tests {
    use super::Epochs;

    #[test]
    fn epochs_are_independent_invalidation_dimensions() {
        let mut epochs = Epochs::default();
        epochs.bump_shape();
        epochs.bump_global();
        epochs.bump_global();
        assert_eq!(epochs.shape(), 1);
        assert_eq!(epochs.prototype(), 0);
        assert_eq!(epochs.realm(), 0);
        assert_eq!(epochs.global(), 2);
    }
}
