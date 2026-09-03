//! Facts shared by frontend queries and residualization.

use oxc::span::Span;
use std::collections::HashMap;
use std::rc::Rc;

/// Arithmetic operations whose semantics are shared by the JavaScript and
/// Wasm frontends.  Frontends retain their physical instruction types, while
/// this fact keeps the overlapping meaning declared exactly once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SharedBinaryFact {
    Add,
    Subtract,
    Multiply,
}

impl SharedBinaryFact {
    pub const fn from_js(operator: crate::ops::BinaryOp) -> Option<Self> {
        match operator {
            crate::ops::BinaryOp::Add => Some(Self::Add),
            crate::ops::BinaryOp::Subtract => Some(Self::Subtract),
            crate::ops::BinaryOp::Multiply => Some(Self::Multiply),
            _ => None,
        }
    }

    pub const fn to_js(self) -> crate::ops::BinaryOp {
        match self {
            Self::Add => crate::ops::BinaryOp::Add,
            Self::Subtract => crate::ops::BinaryOp::Subtract,
            Self::Multiply => crate::ops::BinaryOp::Multiply,
        }
    }

    pub const fn to_wasm_i32(self) -> crate::native::BinI32 {
        match self {
            Self::Add => crate::native::BinI32::Add,
            Self::Subtract => crate::native::BinI32::Sub,
            Self::Multiply => crate::native::BinI32::Mul,
        }
    }
}

/// Observable effects attached to a generated VM operation.
///
/// This is deliberately a small, data-only vocabulary.  The interpreter may
/// choose a physical implementation from these facts, but the facts never
/// replace the complete semantic fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationEffect {
    Pure,
    ReadHeap,
    WriteHeap,
    Allocate,
    MayThrow,
    Control,
    Observable,
}

/// Result shape emitted by an operation before completion wrapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResultShape {
    None,
    Value,
}

/// Physical word facts used by generated guarded views. `Any` means the
/// register remains a canonical tagged word and may require the ordinary
/// semantic decoder; the other variants name the cheap tag/identity probe that
/// a guarded operation is allowed to use. `None` describes an absent result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WordKind {
    None,
    Any,
    Number,
    Boolean,
    Object,
    Array,
    Callable,
}

/// Control-flow exit owned by an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlFlow {
    Next,
    Branch,
    Jump,
    Return,
    Loop,
}

impl ControlFlow {
    pub const fn is_next(self) -> bool {
        matches!(self, Self::Next)
    }
}

/// A fact required before a physical variant may be selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationGuard {
    Number,
    Shape,
    DenseArray,
    Callable,
}

impl OperationGuard {
    const fn code(self) -> u8 {
        match self {
            Self::Number => 0,
            Self::Shape => 1,
            Self::DenseArray => 2,
            Self::Callable => 3,
        }
    }
}

/// Mechanical metadata generated alongside the canonical opcode enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationSpec {
    pub opcode: u8,
    pub name: &'static str,
    pub operand_width: u8,
    pub effects: &'static [OperationEffect],
    pub fallback: &'static str,
    pub result: ResultShape,
    pub control: ControlFlow,
    pub guards: &'static [OperationGuard],
}

impl OperationEffect {
    pub const fn code(self) -> u8 {
        match self {
            Self::Pure => 0,
            Self::ReadHeap => 1,
            Self::WriteHeap => 2,
            Self::Allocate => 3,
            Self::MayThrow => 4,
            Self::Control => 5,
            Self::Observable => 6,
        }
    }

    pub const fn is_observable(self) -> bool {
        matches!(self, Self::Observable | Self::MayThrow)
    }
}

impl OperationSpec {
    pub const fn validate(self) -> bool {
        if self.opcode == 0
            || self.name.is_empty()
            || self.fallback.is_empty()
            || self.operand_width > 3
        {
            return false;
        }
        if (!self.control.is_next()) != self.has_effect(OperationEffect::Control) {
            return false;
        }
        self.effects_are_unique() && self.guards_are_unique()
    }

    const fn effects_are_unique(self) -> bool {
        let mut left = 0;
        while left < self.effects.len() {
            let mut right = left + 1;
            while right < self.effects.len() {
                if self.effects[left].code() == self.effects[right].code() {
                    return false;
                }
                right += 1;
            }
            left += 1;
        }
        true
    }

    const fn guards_are_unique(self) -> bool {
        let mut left = 0;
        while left < self.guards.len() {
            let mut right = left + 1;
            while right < self.guards.len() {
                if self.guards[left].code() == self.guards[right].code() {
                    return false;
                }
                right += 1;
            }
            left += 1;
        }
        true
    }

    pub const fn has_effect(self, effect: OperationEffect) -> bool {
        let mut index = 0;
        while index < self.effects.len() {
            if self.effects[index].code() == effect.code() {
                return true;
            }
            index += 1;
        }
        false
    }

    pub const fn has_guard(self, guard: OperationGuard) -> bool {
        let mut index = 0;
        while index < self.guards.len() {
            if self.guards[index].code() == guard.code() {
                return true;
            }
            index += 1;
        }
        false
    }

    pub const fn is_control(self) -> bool {
        !self.control.is_next()
    }

    pub const fn is_observable(self) -> bool {
        let mut index = 0;
        while index < self.effects.len() {
            if self.effects[index].is_observable() {
                return true;
            }
            index += 1;
        }
        false
    }

    /// An operation can use a bounded physical cache only when its declaration
    /// names a runtime guard. The cache selects an implementation; its miss
    /// edge remains the complete fallback.
    pub const fn is_quickenable(self) -> bool {
        !self.is_control() && !self.guards.is_empty()
    }

    /// The result remains a word even when its semantic value is dynamic. A
    /// missing result is represented explicitly so generated consumers do not
    /// infer it from an opcode name or handler.
    pub const fn result_word_kind(self) -> WordKind {
        match self.result {
            ResultShape::None => WordKind::None,
            ResultShape::Value => WordKind::Any,
        }
    }

    /// Map a declared guard to the physical word probe it permits. The guard
    /// is the source of truth; no second table may claim a stronger type.
    pub const fn guarded_word_kind(self, guard: OperationGuard) -> Option<WordKind> {
        if !self.has_guard(guard) {
            return None;
        }
        Some(match guard {
            OperationGuard::Number => WordKind::Number,
            OperationGuard::Shape => WordKind::Object,
            OperationGuard::DenseArray => WordKind::Array,
            OperationGuard::Callable => WordKind::Callable,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DirectConstructorField {
    pub(crate) name: String,
    pub(crate) source: DirectConstructorSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ComposedConstructorStep {
    Field(DirectConstructorField),
    SuperCall {
        owner_slot: u16,
        arguments: Rc<[ForwardValueSource]>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirectConstructorSource {
    Argument(u16),
    Boolean(bool),
    Integer(i32),
    Null,
    EmptyArray,
    FalsyArgumentOrInteger {
        argument: u16,
        fallback: i32,
    },
    ConstructCapture {
        constructor_slot: u16,
        arguments: Rc<[ForwardValueSource]>,
    },
    CaptureProperty {
        owner_slot: u16,
        property: String,
    },
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
    ReceiverProperty(String),
    Argument(u16),
    Integer(i32),
    Capture(u16),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FunctionFacts {
    pub(crate) direct_constructor: Rc<[DirectConstructorField]>,
    pub(crate) composed_constructor: Rc<[ComposedConstructorStep]>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Fact<T> {
    Proven(T),
    Guarded { value: T, guard: Guard },
    Unknown,
}

/// Certainty is the derived view used by physical execution policy. The
/// payload and guard remain owned by [`Fact`], so certainty cannot drift into
/// a second fact table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Certainty {
    Proven,
    Guarded,
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

    pub const fn certainty(&self) -> Certainty {
        match self {
            Self::Proven(_) => Certainty::Proven,
            Self::Guarded { .. } => Certainty::Guarded,
            Self::Unknown => Certainty::Unknown,
        }
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
    use super::{
        Certainty, ControlFlow, Epochs, Fact, FactSiteId, Guard, OperationEffect, OperationSpec,
        ReduceContext, ResultShape, SharedBinaryFact, WordKind,
    };

    #[test]
    fn shared_binary_fact_adapts_js_and_wasm_without_duplicate_meaning() {
        for (js, wasm) in [
            (crate::ops::BinaryOp::Add, crate::native::BinI32::Add),
            (crate::ops::BinaryOp::Subtract, crate::native::BinI32::Sub),
            (crate::ops::BinaryOp::Multiply, crate::native::BinI32::Mul),
        ] {
            let fact = SharedBinaryFact::from_js(js).expect("shared arithmetic fact");
            assert_eq!(fact.to_js(), js);
            assert_eq!(fact.to_wasm_i32(), wasm);
        }
    }

    #[test]
    fn certainty_is_derived_from_the_single_fact_record() {
        assert_eq!(Fact::Proven(1).certainty(), Certainty::Proven);
        assert_eq!(
            Fact::Guarded {
                value: 1,
                guard: Guard::Number,
            }
            .certainty(),
            Certainty::Guarded
        );
        assert_eq!(Fact::<i32>::Unknown.certainty(), Certainty::Unknown);
    }

    #[test]
    fn word_kinds_are_derived_from_result_and_guard_facts() {
        let get = crate::ir::Opcode::GetProperty.spec();
        assert_eq!(get.result_word_kind(), WordKind::Any);
        assert_eq!(
            get.guarded_word_kind(super::OperationGuard::Shape),
            Some(WordKind::Object)
        );
        assert_eq!(
            crate::ir::Opcode::Call.guarded_word_kind(super::OperationGuard::Callable),
            Some(WordKind::Callable)
        );
        assert_eq!(crate::ir::Opcode::Jump.result_word_kind(), WordKind::None);
        assert_eq!(
            crate::ir::Opcode::Move.guarded_word_kind(super::OperationGuard::Shape),
            None
        );
    }

    #[test]
    fn operation_spec_validation_rejects_duplicate_effects_bad_widths_and_inconsistent_control() {
        let duplicate = OperationSpec {
            opcode: 1,
            name: "duplicate",
            operand_width: 1,
            effects: &[OperationEffect::Pure, OperationEffect::Pure],
            fallback: "fallback",
            result: ResultShape::None,
            control: ControlFlow::Next,
            guards: &[],
        };
        assert!(!duplicate.validate());

        let too_wide = OperationSpec {
            opcode: 1,
            name: "wide",
            operand_width: 4,
            effects: &[OperationEffect::Pure],
            fallback: "fallback",
            result: ResultShape::None,
            control: ControlFlow::Next,
            guards: &[],
        };
        assert!(!too_wide.validate());

        let inconsistent_control = OperationSpec {
            opcode: 1,
            name: "branch",
            operand_width: 1,
            effects: &[OperationEffect::Pure],
            fallback: "fallback",
            result: ResultShape::None,
            control: ControlFlow::Branch,
            guards: &[],
        };
        assert!(!inconsistent_control.validate());

        let duplicate_guard = OperationSpec {
            opcode: 1,
            name: "guarded",
            operand_width: 1,
            effects: &[OperationEffect::Control],
            fallback: "fallback",
            result: ResultShape::None,
            control: ControlFlow::Branch,
            guards: &[super::OperationGuard::Shape, super::OperationGuard::Shape],
        };
        assert!(!duplicate_guard.validate());
    }

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
