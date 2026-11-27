//! String methods that call a RegExp's well-known symbol (`match`, `replace`) made in place,
//! in the lane, when the symbol method, `exec`, the flags and `lastIndex` are the builtins'.
use super::*;
use super::replacement::{ReplaceResult, has_substitution};
use super::ascii::AsciiReplace;

impl Vm {
    /// `String.prototype.match(regexp)` of a string under `steady_string_replace`'s proofs
    /// for `@@match`: a non-global match is the builtin exec of the string
    /// (`steady_regexp_call`), a global one `regexp_symbol_match_global_fast`.
    pub fn steady_string_match(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        search: Value,
        result_used: bool,
    ) -> Option<Result<Value, JsError>> {
        let (matcher, flags) = self.steady_symbol_matcher(this, search, WellKnownSymbol::Match, Native::RegExpSymbolMatch)?;
        if !flags.contains(RegExpFlags::GLOBAL) {
            return self.steady_regexp_call(p, Native::RegExpExec, search, this, result_used);
        }
        let input = self.heap.string(this)?.clone();
        Some(self.with_call_roots([this, search], |vm| vm.regexp_symbol_match_builtin(p, search, this, input, &matcher, flags)))
    }

    /// The matcher and flags of the RegExp `search` when a string method of the string
    /// `this` would call `search[symbol]`, this realm's builtin `native`, and that builtin's
    /// exec, flags and lastIndex reads are the builtins' (`regexp_builtin_receiver_matcher`).
    fn steady_symbol_matcher(
        &mut self,
        this: Value,
        search: Value,
        symbol: WellKnownSymbol,
        native: Native,
    ) -> Option<(Rc<quench_regexp::Regex>, RegExpFlags)> {
        if !self.is_string(this) {
            return None;
        }
        let symbol = self.well_known_symbols.get(symbol)?;
        if let Some(proven) = self.steady_symbol_matcher_proven(search, symbol, native) {
            return proven;
        }
        let method = self.regexp_proven_symbol_method(search, symbol)?;
        match self.heap.function(method) {
            Some(record @ FunctionRecord { kind: FunctionKind::Native(found), .. })
                if *found == native && self.record_realm(record) == self.realm.globals => {}
            _ => return None,
        }
        self.regexp_builtin_receiver_matcher(search)
    }

    /// `steady_symbol_matcher` where both of its proofs are recorded for the receiver's and
    /// the prototype's shapes: `Some(None)` where a recorded proof's value no longer holds (the
    /// method or `exec` was replaced), `None` where a proof is not recorded yet, which the
    /// general route records.
    #[inline(always)]
    fn steady_symbol_matcher_proven(
        &mut self,
        search: Value,
        symbol: Value,
        native: Native,
    ) -> Option<Option<(Rc<quench_regexp::Regex>, RegExpFlags)>> {
        let Some(Cell::RegExp { object, meta }) = self.heap.get(search) else {
            return None;
        };
        let (flags, proto, receiver_shape) = (meta.flags, object.proto(), object.shape());
        let global = flags.contains(RegExpFlags::GLOBAL);
        if !global && flags.contains(RegExpFlags::STICKY) {
            return None;
        }
        if !self.steady_layouts_proven(proto, receiver_shape, symbol, native)? {
            return Some(None);
        }
        if !global && self.regexp_last_index_value(search).and_then(|(_, value)| value.as_number()).is_none() {
            return Some(None);
        }
        let Some(Cell::RegExp { meta, .. }) = self.heap.get(search) else {
            return Some(None);
        };
        Some(Self::regexp_matcher(meta).ok().map(|matcher| (matcher, flags)))
    }

    /// Whether the method for `symbol` and `exec` of a RegExp of `receiver_shape` and prototype
    /// `proto` are the builtins': one flag read while the prototype-values watch the layout
    /// holds is published, else both slots read, which restarts the watch. `None` where a shape
    /// proof is not recorded.
    fn steady_layouts_proven(&mut self, proto: Value, receiver_shape: u32, symbol: Value, native: Native) -> Option<bool> {
        let realm = self.realm.globals;
        let prototype = self.heap.object(proto)?;
        let prototype_shape = prototype.shape();
        // The layout last proven here is kept last, where the search starts: a program's
        // steady string method calls meet it at the first compare.
        let layouts = &mut self.regexp_symbol_layouts;
        let found = layouts.iter().rposition(|layout| {
            layout.symbol == symbol && layout.realm == realm && layout.prototype_object == proto
                && layout.receiver == receiver_shape && layout.prototype == prototype_shape
        })?;
        // Moving it last keeps the others' order, so the first entry is still the one least
        // recently met, which `remember_shape_proof` evicts.
        layouts[found..].rotate_left(1);
        let position = layouts.len() - 1;
        let layout = &self.regexp_symbol_layouts[position];
        if layout.steady.as_ref().is_some_and(|watch| watch.is_published()) {
            return Some(true);
        }
        let method_slot = layout.slot as usize;
        let exec_slot = self.regexp_builtin_layouts.iter().find(|layout| {
            layout.realm == realm && layout.prototype_object == proto
                && layout.receiver == receiver_shape && layout.prototype == prototype_shape
        })?.exec_slot as usize;
        let proven = self.builtin_values_at(proto, method_slot, exec_slot, native)?;
        if proven {
            self.regexp_symbol_layouts[position].steady = crate::heap::watch_prototype_values(prototype_shape);
        }
        Some(proven)
    }

    /// Whether the prototype `proto`'s values at `method_slot` and `exec_slot` are `native` of
    /// this realm and the builtin `exec`.
    fn builtin_values_at(&mut self, proto: Value, method_slot: usize, exec_slot: usize, native: Native) -> Option<bool> {
        let prototype = self.heap.object(proto)?;
        let method = self.heap.property_get_ref(prototype, method_slot)?;
        let exec = self.heap.property_get_ref(prototype, exec_slot)?;
        let realm = self.realm.globals;
        let method_is_builtin = matches!(
            self.heap.function(method),
            Some(record @ FunctionRecord { kind: FunctionKind::Native(found), .. }) if *found == native && self.record_realm(record) == realm
        );
        Some(method_is_builtin && self.is_native_function(exec, Native::RegExpExec))
    }

    /// `String.prototype.replace(regexp, string)` when the call runs no guest code, so it
    /// observes no frame (`regexp_call_in_lane`): a string receiver and replacement, and a
    /// RegExp whose `@@replace` is this realm's builtin (`regexp_proven_symbol_method`) and
    /// whose exec, flags and lastIndex are builtin (`regexp_builtin_receiver_matcher`).
    /// Those make the native's steps the builtin `@@replace`'s fast path, which this runs;
    /// `None` leaves any other call to the general path.
    pub fn steady_string_replace(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        arguments: &[Value],
        result_used: bool,
    ) -> Option<Result<Value, JsError>> {
        self.steady_string_replace_built(p, native, this, arguments, ReplaceResult::of(result_used))
    }

    /// `steady_string_replace` when the result is read only by its `.length`
    /// (`String.prototype.replace(...).length`): the replacement's length, its string never
    /// built. The same proofs and effects; a template with `$` substitutes, which takes the
    /// string's build, so it is `None` as any other call the lane leaves to the native.
    pub fn steady_string_replace_length(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        arguments: &[Value],
    ) -> Option<Result<Value, JsError>> {
        let [_, replacement] = *arguments.get(..2)?.as_array::<2>()?;
        if has_substitution(self.heap.string(replacement)?) {
            return None;
        }
        self.steady_string_replace_built(p, native, this, arguments, ReplaceResult::Length)
    }

    /// `steady_string_replace` building what `build` says its result is.
    fn steady_string_replace_built(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        arguments: &[Value],
        build: ReplaceResult,
    ) -> Option<Result<Value, JsError>> {
        let [search, replacement] = *arguments.get(..2)?.as_array::<2>()?;
        if native != Native::StringReplace || !self.is_string(replacement) {
            return None;
        }
        let (matcher, flags) = self.steady_symbol_matcher(this, search, WellKnownSymbol::Replace, Native::RegExpSymbolReplace)?;
        if let Some(replaced) = self.replace_ascii_input(p, search, this, replacement, &matcher, flags, build) {
            return Some(replaced);
        }
        let input = self.heap.string(this)?.clone();
        let template = self.heap.string(replacement)?.clone();
        Some(self.with_call_roots([this, search, replacement], |vm| {
            vm.regexp_symbol_replace_fast(
                p,
                search,
                &input,
                this,
                FastReplacement::Template(&template),
                &matcher,
                flags,
                build,
            )
        }))
    }

    /// `steady_string_replace` of a flat ASCII subject by a flat ASCII template with no `$`:
    /// GetSubstitution returns the template, so the match ranges are all the replacement
    /// reads. The subject and template are borrowed from the heap while the ranges and the
    /// result are made, and only the subject is kept (by the legacy statics). `None` leaves
    /// any other call, or a pattern with no compiled matcher over bytes, to the general
    /// route (`regexp_replace_with_literal`).
    #[allow(clippy::too_many_arguments)]
    fn replace_ascii_input(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        this: Value,
        replacement: Value,
        matcher: &Rc<quench_regexp::Regex>,
        flags: RegExpFlags,
        build: ReplaceResult,
    ) -> Option<Result<Value, JsError>> {
        let global = flags.contains(RegExpFlags::GLOBAL);
        let mut ranges = smallvec::SmallVec::new();
        let AsciiReplace { output, length } = AsciiReplace::plan(&self.heap, (this, replacement), matcher, flags, build, &mut ranges)?;
        // The exec loop of a global replace starts from lastIndex 0 and leaves it 0.
        let holds_zero = |vm: &mut Self| vm.regexp_last_index_value(receiver).is_some_and(|(_, held)| held.as_number() == Some(0.0));
        if global && !holds_zero(self) && let Err(error) = self.store_regexp_last_index(p, receiver, Value::number(0.0)) {
            return Some(Err(error));
        }
        let Some(last) = ranges.last() else {
            return Some(Ok(build.unchanged(this, self.heap.string(this)?.units().len())));
        };
        let input = self.heap.string(this)?.clone();
        let owner = match self.regexp_legacy_owner(receiver) {
            Ok(owner) => owner,
            Err(error) => return Some(Err(error)),
        };
        self.record_regexp_legacy_pending(owner, input, Rc::clone(matcher), last.start);
        Some(match length {
            None => Err(self.range_error(p, "Invalid string length".into())),
            Some(length) => Ok(match output {
                Some(replaced) => self.heap.alloc_string(replaced),
                None => build.value(length),
            }),
        })
    }
}
