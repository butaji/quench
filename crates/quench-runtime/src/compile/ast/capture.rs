//! The bindings a closure created at a position of a function can capture, derived once
//! per lexical state, per local count and per lexical scope.
use super::super::capture_scope::{BaseBindings, CaptureBase, CaptureLayer, CaptureScope, ScopeChain};
use super::{FunctionCompiler, LexicalBindingKind};
use crate::bytecode::Atom;
use rustc_hash::FxHashSet;
use std::rc::Rc;

/// The state a capture scope is derived from. Locals and lexical pushes only grow, so equal
/// states name the same locals and lexical stack.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct CaptureScopeState {
    local_count: usize,
    lexical_depth: usize,
    lexical_pushes: usize,
    parameter_context: bool,
}

/// The facts capture scopes and lexical slot sets are derived from, kept between positions.
#[derive(Default)]
pub(super) struct CaptureMemos {
    /// The capture scope of one lexical state: closures created in the same state (hoisted
    /// declarations, sibling callbacks) share it.
    scope: Option<(CaptureScopeState, Rc<CaptureScope>)>,
    /// The capture base as of a local count and parameter context (`capture_base`).
    base: Option<((usize, bool), Rc<BaseBindings>)>,
    /// Lexical targets a cached layer or slot set found without a local slot; one gaining a
    /// slot advances `unslotted_epoch`, retiring every cached layer and slot set.
    pub(super) unslotted_targets: FxHashSet<Atom>,
    pub(super) unslotted_epoch: u32,
}

impl CaptureMemos {
    /// `hidden_local` gave `atom` a slot.
    pub(super) fn slot_given(&mut self, atom: Atom) {
        if self.unslotted_targets.remove(&atom) {
            self.unslotted_epoch += 1;
        }
    }
}

impl FunctionCompiler<'_, '_> {
    pub(in crate::compile) fn capture_scopes(&mut self) -> ScopeChain {
        let state = CaptureScopeState {
            local_count: self.local_slots.len(),
            lexical_depth: self.lexical_scopes.len(),
            lexical_pushes: self.lexical_scope_pushes,
            parameter_context: self.parameter_context,
        };
        let scope = match &self.capture.scope {
            Some((memo_state, scope)) if *memo_state == state => Rc::clone(scope),
            _ => {
                let scope = Rc::new(self.current_capture_scope());
                self.capture.scope = Some((state, Rc::clone(&scope)));
                scope
            }
        };
        let mut scopes = vec![scope];
        // Eval can introduce vars that shadow inherited bindings. Current lexical
        // bindings cannot be replaced by those declarations and remain capturable.
        if !self.dynamic_eval {
            scopes.extend(self.scopes.levels().iter().cloned());
        } else {
            // Source eval cannot introduce compiler bindings. Keep their scope
            // depths while dropping source-name projections and capture markers.
            scopes.extend(self.scopes.levels().iter().map(|scope| {
                Rc::new(
                    scope
                        .iter()
                        .filter_map(|(atom, slot)| {
                            let name = &self.owner.atoms[atom as usize];
                            (self.is_compiler_binding(atom)
                                && !LexicalBindingKind::ALL
                                    .into_iter()
                                    .any(|kind| kind != LexicalBindingKind::PrivateName && name.starts_with(kind.capture_prefix()))
                                && !name.starts_with(super::CATCH_CAPTURE_PREFIX))
                            .then_some((atom, slot))
                        })
                        .collect(),
                )
            }));
        }
        ScopeChain::new(scopes)
    }

    /// The current function's bindings any closure created inside it sees before lexical
    /// scopes apply: its declared names and compiler bindings with local slots. Locals are
    /// only appended (`hidden_local`), so a later base is an earlier one plus the locals added
    /// since, under the same parameter context.
    fn capture_base(&mut self) -> CaptureBase {
        let (count, parameter_context) = (self.locals.len(), self.parameter_context);
        let arguments = self
            .parameter_arguments_slot
            .filter(|_| parameter_context)
            .map(|slot| (self.owner.atom("arguments"), slot));
        let (bindings, first_new): (Rc<BaseBindings>, _) = match self.capture.base.take() {
            Some(((memo_count, context), bindings)) if context == parameter_context && memo_count <= count => (bindings, memo_count),
            _ => (Rc::default(), 0),
        };
        for index in first_new..count {
            let atom = self.locals[index];
            let Some(&slot) = self.local_slots.get(&atom) else {
                continue;
            };
            let declared = !self.dynamic_eval && self.function_scope.contains(&atom);
            let visible = !self.parameter_context
                || usize::from(slot) < self.parameter_local_count
                || Some(slot) == self.parameter_arguments_slot;
            // The parameter context's `arguments` binding is its own, whatever local shares
            // the name.
            let shadowed = arguments.is_some_and(|(name, _)| name == atom);
            if (declared || self.is_compiler_binding(atom))
                && visible
                && !shadowed
                && !bindings.bind(atom, index as u32, slot)
            {
                // A name rebound to another slot: earlier positions keep what they saw; start over.
                self.capture.base = None;
                return self.capture_base();
            }
        }
        self.capture.base = Some(((count, parameter_context), Rc::clone(&bindings)));
        CaptureBase { bindings, visible: count as u32, arguments }
    }

    /// The current function's bindings visible to a closure created at this position: the
    /// base under each lexical scope's layer, built once per scope (`capture_layer`).
    fn current_capture_scope(&mut self) -> CaptureScope {
        let mut scope = CaptureScope::new(self.capture_base());
        for index in 0..self.lexical_scopes.len() {
            let layer = match &self.lexical_scopes[index].capture_layer {
                Some((epoch, layer)) if *epoch == self.capture.unslotted_epoch => Rc::clone(layer),
                _ => self.capture_layer(index, &scope),
            };
            scope.push_layer(layer);
        }
        scope
    }

    /// The bindings lexical scope `index` puts over `outer`, the scopes around it: each
    /// source at its target's slot with its kind's capture marker, hiding the markers of the
    /// same source outside. Cached unless a private name's identity reads `outer`.
    fn capture_layer(&mut self, index: usize, outer: &CaptureScope) -> Rc<CaptureLayer> {
        let lexical = &self.lexical_scopes[index];
        let mut layer = CaptureLayer::default();
        let mut private = false;
        for (source, target) in &lexical.bindings {
            let Some(slot) = self.local_slots.get(target).copied() else {
                self.capture.unslotted_targets.insert(*target);
                continue;
            };
            layer.insert(*source, Some(slot));
            let kind = lexical.kinds.get(source).copied().unwrap_or(LexicalBindingKind::Mutable);
            if kind == LexicalBindingKind::PrivateName {
                private = true;
                self.hide_stale_private_names(*source, outer, &mut layer);
                layer.insert(*source, Some(slot));
            }
            // Markers exist only as `capture_marker` made them, which records each.
            for marker in LexicalBindingKind::ALL
                .map(Some)
                .into_iter()
                .chain([None])
                .filter_map(|kind| self.owner.capture_markers.get(&(kind, *source)).copied())
            {
                layer.insert(marker, None);
            }
            layer.insert(self.owner.capture_marker(Some(kind), *source), Some(slot));
            if lexical.catch_parameter {
                layer.insert(self.owner.capture_marker(None, *source), Some(slot));
            }
        }
        let layer = Rc::new(layer);
        if !private {
            self.lexical_scopes[index].capture_layer = Some((self.capture.unslotted_epoch, Rc::clone(&layer)));
        }
        layer
    }

    /// Hides every binding visible over `outer` and in `layer` whose private identity is
    /// private name `source`'s: an inner class's private name replaces an outer one's.
    fn hide_stale_private_names(&self, source: Atom, outer: &CaptureScope, layer: &mut CaptureLayer) {
        let label = crate::bytecode::private_identity_label(&self.owner.atoms[source as usize]);
        let visible = outer.keys().chain(layer.iter().filter(|(_, slot)| slot.is_some()).map(|(atom, _)| *atom));
        let stale = visible.filter(|atom| {
            let text: &str = self.owner.atoms[*atom as usize].as_ref();
            let identity = text.strip_prefix(LexicalBindingKind::PrivateName.capture_prefix()).unwrap_or(text);
            label.is_some() && crate::bytecode::private_identity_label(identity) == label
        }).collect::<Vec<_>>();
        for atom in stale {
            layer.insert(atom, None);
        }
    }
}
