//! RegExp calls made in place, in the lane, when they run no guest code and raise nothing.
use super::*;

impl<H: Host> Vm<H> {
    /// `RegExp.prototype.exec` or `test` when the call runs no guest code and raises
    /// nothing, so it observes no frame (`lane_regexp_call`): a RegExp receiver, a string
    /// subject, an own writable data `lastIndex` holding a Number (`regexp_last_index_slot`,
    /// so its conversion and any store run no guest code), and for `test` the builtin `exec`
    /// (`regexp_builtin_exec_matcher`). The call is the native's own; `None` leaves any other
    /// call to the general path.
    pub(in crate::vm) fn steady_regexp_call(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        subject: Value,
        result_used: bool,
    ) -> Option<Result<Value, JsError>> {
        if !matches!(native, Native::RegExpExec | Native::RegExpTest)
            || self.method_caches_may_name(self.runtime_atoms.last_index)
        {
            return None;
        }
        let (slot, last_index, rejected) = self.regexp_steady_state(receiver, subject)?;
        if native == Native::RegExpTest && self.regexp_builtin_exec_matcher(receiver).is_none() {
            return None;
        }
        // A search the prefilter excludes fails as the matcher's would: `null` from exec and
        // `false` from test, and a global or sticky regexp's `lastIndex` reset through the
        // slot `regexp_match_from` stores through.
        if let Some(stateful) = rejected {
            if stateful {
                self.heap.property_set(receiver, slot, Value::number(0.0));
            }
            return Some(Ok(if native == Native::RegExpExec { Value::NULL } else { Value::FALSE }));
        }
        // `test` with the builtin exec is that exec whose result array never escapes.
        let exec = native == Native::RegExpExec;
        let result = self.regexp_steady_exec(p, receiver, subject, slot, last_index, result_used && exec);
        Some(if exec { result } else { result.map(|result| Value::boolean(!result.is_null())) })
    }

    /// `String.prototype.match(regexp)` of a string under `steady_string_replace`'s proofs
    /// for `@@match`: a non-global match is the builtin exec of the string
    /// (`steady_regexp_call`), a global one `regexp_symbol_match_global_fast`.
    pub(in crate::vm) fn steady_string_match(
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
        let method = self.regexp_proven_symbol_method(search, symbol)?;
        match self.heap.function(method) {
            Some(FunctionRecord { kind: FunctionKind::Native(found), realm, .. })
                if *found == native && *realm == self.realm.globals => {}
            _ => return None,
        }
        self.regexp_builtin_receiver_matcher(search)
    }

    /// `String.prototype.replace(regexp, string)` when the call runs no guest code, so it
    /// observes no frame (`lane_regexp_call`): a string receiver and replacement, and a
    /// RegExp whose `@@replace` is this realm's builtin (`regexp_proven_symbol_method`) and
    /// whose exec, flags and lastIndex are builtin (`regexp_builtin_receiver_matcher`).
    /// Those make the native's steps the builtin `@@replace`'s fast path, which this runs;
    /// `None` leaves any other call to the general path.
    pub(in crate::vm) fn steady_string_replace(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        arguments: &[Value],
        result_used: bool,
    ) -> Option<Result<Value, JsError>> {
        let [search, replacement] = *arguments.get(..2)?.as_array::<2>()?;
        if native != Native::StringReplace || !self.is_string(replacement) {
            return None;
        }
        let (matcher, flags) = self.steady_symbol_matcher(this, search, WellKnownSymbol::Replace, Native::RegExpSymbolReplace)?;
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
                result_used,
            )
        }))
    }

    /// `regexp_builtin_exec` of `steady_regexp_call`'s case: a string subject, whose
    /// conversion runs no guest code, and `lastIndex` the Number in the own data `slot`, so
    /// neither its conversion nor the store runs guest code or allocates. A failed match is
    /// `null` with nothing to root; a match builds its result as the builtin does.
    fn regexp_steady_exec(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        subject: Value,
        slot: usize,
        last_index: Value,
        result_used: bool,
    ) -> Result<Value, JsError> {
        let input = self.coerce_js_string(p, subject)?;
        let last_index = self.regexp_to_length_value(p, last_index)?;
        let Some(matched) = self.regexp_match_from(p, receiver, &input, last_index, Some(slot))? else {
            return Ok(Value::NULL);
        };
        // Nothing reads the result: building it allocates and runs no guest code, so only
        // the match's own effect, the legacy statics, remains.
        if !result_used {
            self.record_regexp_legacy_match(receiver, input, matched)?;
            return Ok(Value::UNDEFINED);
        }
        self.with_call_roots([receiver, subject], |vm| vm.regexp_exec_result(receiver, subject, input, matched))
    }

    /// What a steady call (`steady_regexp_call`) of the RegExp `receiver` over the string
    /// `subject` reads, in one look at the regexp: the slot and Number value of its own
    /// writable data `lastIndex` (`regexp_last_index_slot`), and whether its compiled
    /// regexp's prefilter excludes `regexp_match_from`'s search (`Regex::excludes_search`),
    /// decided where neither conversion has an effect, over a flat subject. That `Some`
    /// carries whether the regexp is global or sticky; `None` leaves the search to the
    /// matcher, and a `None` result the call to the general path.
    #[inline(always)]
    fn regexp_steady_state(&mut self, receiver: Value, subject: Value) -> Option<(usize, Value, Option<bool>)> {
        let Some(Cell::RegExp { object, .. }) = self.heap.get(receiver) else {
            return None;
        };
        let shape = object.shape();
        let slot = match self.regexp_last_index_layouts.iter().find(|(cached, _)| *cached == shape) {
            Some(&(_, slot)) => slot as usize,
            None => self.regexp_last_index_slot_miss(receiver, shape)?,
        };
        let Some(Cell::RegExp { object, meta }) = self.heap.get(receiver) else {
            return None;
        };
        // A RegExp's `lastIndex`, made first, usually lies in its inline storage.
        let inline = (slot < crate::value_vec::INLINE_PROPERTY_COUNT).then(|| object.inline_property(slot)).flatten();
        let last_index = match inline {
            Some(value) => value,
            None => self.heap.property_get(object, slot)?,
        };
        // A deleted slot is no Number either.
        let index = last_index.as_number()?;
        let input = self.heap.string(subject)?;
        let (Some(regex), Some(units)) = (meta.matcher.0.get(), input.existing_flat()) else {
            return Some((slot, last_index, None));
        };
        let stateful = meta.flags.intersects(RegExpFlags::GLOBAL.with(RegExpFlags::STICKY));
        let start = if stateful { regexp_to_length(index) } else { 0 };
        let length = input.len();
        let rejected = (stateful && start > length) || regex.excludes_search(length, start, meta.flags.contains(RegExpFlags::STICKY), || units.unit(start));
        Some((slot, last_index, rejected.then_some(stateful)))
    }
}
