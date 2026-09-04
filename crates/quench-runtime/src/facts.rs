//! Facts shared by frontend queries and residualization.

use oxc::span::Span;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectConstructorField {
    pub(crate) name: String,
    pub(crate) source: DirectConstructorSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirectConstructorSource {
    Argument(u16),
    Boolean(bool),
    Integer(i32),
    Null,
    GuardedArray {
        length_slot: u16,
    },
    NullishSelectCapture {
        argument: u16,
        nullish_slot: u16,
        other_slot: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ForwardValueSource {
    Receiver,
    Argument(u16),
    Integer(i32),
    Capture(u16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForwardConstructCallFact {
    pub(crate) method: String,
    pub(crate) constructor_slot: u16,
    pub(crate) forwarded_arguments: Rc<[u16]>,
    pub(crate) constructor_arguments: Rc<[ForwardValueSource]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForwardThenCallFact {
    pub(crate) first_method: String,
    pub(crate) nested_property: String,
    pub(crate) nested_method: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CountedMethodLoopFact {
    /// A side-effect-free population-count loop (`n &= n - 1`).  This is a
    /// generic numeric loop fact; callers still fall back to ordinary VM
    /// execution whenever its input is not a plain Number.
    BitCount,
    Visit {
        length_method: String,
        element_method: String,
        body_method: String,
    },
    Filter {
        determining_property: String,
        collection_property: String,
        length_method: String,
        element_method: String,
        predicate_method: String,
        append_method: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirectMethodFact {
    Noop,
    CopyMethodProperty {
        target_method: String,
        source_method: String,
        property: String,
    },
    PropertyLoad {
        property: String,
    },
    PropertyNotEqualCapture {
        property: String,
        capture_slot: u16,
        capture_property: String,
    },
    AppendArray {
        property: String,
    },
    SlotDot3 {
        receiver: [String; 3],
        argument: [String; 3],
    },
    SelectUpdateCall {
        input_method: String,
        output_method: String,
        namespace_slot: u16,
        combine_method: String,
        receiver_value: String,
        input_value: String,
        output_value: String,
        input_flag: String,
        output_flag: String,
        extra_flag_objects: Vec<String>,
        conditional_method: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FunctionFacts {
    pub(crate) direct_constructor: Rc<[DirectConstructorField]>,
    pub(crate) forward_construct_call: Option<Rc<ForwardConstructCallFact>>,
    pub(crate) forward_then_call: Option<Rc<ForwardThenCallFact>>,
    pub(crate) counted_method_loop: Option<Rc<CountedMethodLoopFact>>,
    pub(crate) direct_method: Option<Rc<DirectMethodFact>>,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FactSiteId(pub u32);

#[derive(Debug, PartialEq)]
pub struct SiteFacts<T> {
    entries: HashMap<(FactSiteId, ReduceContext), Fact<T>>,
}

impl<T> Default for SiteFacts<T> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

impl<T: Clone> SiteFacts<T> {
    pub fn insert(&mut self, site: FactSiteId, fact: Fact<T>) {
        self.insert_in_context(site, ReduceContext::Value, fact);
    }

    pub fn insert_in_context(&mut self, site: FactSiteId, context: ReduceContext, fact: Fact<T>) {
        self.entries.insert((site, context), fact);
    }

    pub fn query(&self, site: FactSiteId) -> Fact<T> {
        self.query_in_context(site, ReduceContext::Value)
    }

    pub fn query_in_context(&self, site: FactSiteId, context: ReduceContext) -> Fact<T> {
        self.entries
            .get(&(site, context))
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
    pub(crate) site_facts: SiteFacts<Constant>,
    pub(crate) site_ids: HashMap<Span, FactSiteId>,
    pub scope_count: usize,
    pub symbol_count: usize,
    pub(crate) private_names: HashMap<Span, PrivateNameId>,
    /// Canonical identifier spellings owned by this lowering session.
    ///
    /// IDs are scoped to this `ProgramDb` and are never exposed to runtime
    /// values. The table is dropped with the database, so names cannot leak
    /// across programs or realms.
    pub(crate) identifier_names: IdentifierInterner,
    pub(crate) strict: bool,
    pub(crate) in_function: bool,
    pub(crate) function_name_slot: Option<u16>,
    pub(crate) inferred_name: Option<String>,
    pub(crate) tail_calls: bool,
    pub(crate) eval_var_barrier: Vec<String>,
    pub(crate) eval_formals: Vec<String>,
    pub(crate) eval_var_scope_start: u16,
    pub(crate) function_has_direct_eval: bool,
    pub(crate) eval_arrow_scope: bool,
    pub(crate) eval_deletable: Vec<(String, u16)>,
    pub(crate) epochs: Epochs,
    pub(crate) dynamic_scope_depth: u16,
    pub(crate) function_dynamic_scope_floor: u16,
    pub(crate) reduction_source: String,
}
#[derive(Debug, Default, PartialEq)]
pub(crate) struct IdentifierInterner {
    ids: HashMap<String, u32>,
    names: Vec<String>,
}

impl IdentifierInterner {
    pub(crate) fn intern(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.ids.get(name) {
            return id;
        }
        let id = self.names.len() as u32;
        self.names.push(name.to_owned());
        self.ids.insert(name.to_owned(), id);
        id
    }

    pub(crate) fn resolve(&self, id: u32) -> Option<&str> {
        self.names.get(id as usize).map(String::as_str)
    }
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
    pub(crate) fn binding_is_dynamic(&self, slot: u16) -> bool {
        self.has_dynamic_scope()
            || (self.in_function
                && self.function_has_direct_eval
                && slot < self.eval_var_scope_start)
    }

    pub(crate) fn enter_dynamic_scope(&mut self) {
        self.dynamic_scope_depth = self.dynamic_scope_depth.saturating_add(1);
    }

    pub(crate) fn exit_dynamic_scope(&mut self) {
        self.dynamic_scope_depth = self.dynamic_scope_depth.saturating_sub(1);
    }

    pub(crate) fn has_dynamic_scope(&self) -> bool {
        self.dynamic_scope_depth != 0
    }

    pub(crate) fn has_active_dynamic_scope(&self) -> bool {
        self.dynamic_scope_depth > self.function_dynamic_scope_floor
    }

    pub(crate) fn install_fact_sites(&mut self, sites: HashMap<Span, FactSiteId>) {
        self.site_ids = sites;
    }

    pub(crate) fn install_reduction_source(&mut self, source: &str) {
        self.reduction_source = source.to_string();
    }

    pub(crate) fn is_cover_parenthesized_identifier(&self, span: Span) -> bool {
        let start = usize::try_from(span.start).ok();
        let end = usize::try_from(span.end).ok();
        let (Some(start), Some(end)) = (start, end) else {
            return false;
        };
        let before = self
            .reduction_source
            .get(..start)
            .unwrap_or_default()
            .trim_end();
        let after = self
            .reduction_source
            .get(end..)
            .unwrap_or_default()
            .trim_start();
        before.ends_with('(') && after.starts_with(')')
    }

    pub(crate) fn finish_reduction(&mut self) {
        self.site_ids.clear();
        self.reduction_source.clear();
    }

    pub(crate) fn record_fact_in_context(
        &mut self,
        span: Span,
        context: ReduceContext,
        fact: Fact<Constant>,
    ) {
        let Some(site) = self.site_ids.get(&span).copied() else {
            return;
        };
        self.site_facts.insert_in_context(site, context, fact);
    }

    pub fn query_fact(&self, site: FactSiteId) -> Fact<Constant> {
        self.site_facts.query(site)
    }

    pub fn query_fact_in_context(
        &self,
        site: FactSiteId,
        context: ReduceContext,
    ) -> Fact<Constant> {
        self.site_facts.query_in_context(site, context)
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
    use super::{Epochs, FactSiteId, ReduceContext};

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

    #[test]
    fn reduction_keeps_site_facts_without_retaining_source_spans() {
        let program = crate::reduce::reduce_source("1;").expect("literal source reduces");
        assert!(program.facts.site_ids.is_empty());
        assert!(!program.facts.site_facts.entries.is_empty());
        let (site, _) = program
            .facts
            .site_facts
            .entries
            .keys()
            .next()
            .expect("literal fact");
        assert!(program
            .facts
            .query_fact_in_context(*site, ReduceContext::Value)
            .is_known());
        assert_eq!(
            program.facts.query_fact(FactSiteId(u32::MAX)),
            super::Fact::Unknown
        );
    }
}
