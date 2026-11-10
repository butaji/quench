//! Global matches of a pattern that is a literal run (`quench_regexp::RangeSearcher::literal_matches`).
use super::*;

impl<H: Host> Vm<H> {
    /// The result of a global builtin exec loop that found `count` matches of `length` units,
    /// the last at `last`: lastIndex stored 0 as the loop's first step stores it, the legacy
    /// statics left pending on the last match, and the array of the matches, `null` without
    /// one. Every match is the same string, so the array holds one value `count` times.
    pub(super) fn regexp_literal_matches(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        input: &JsString,
        matcher: &Rc<quench_regexp::Regex>,
        quench_regexp::LiteralMatches { length, count, last }: quench_regexp::LiteralMatches,
    ) -> Result<Value, JsError> {
        self.store_regexp_last_index(p, receiver, Value::number(0.0))?;
        let Some(last) = last else {
            return Ok(Value::NULL);
        };
        self.record_regexp_legacy_pending(receiver, input.clone(), matcher, last)?;
        let matched = self.heap.alloc_unit_run(input.units().slice(last..last + length));
        Ok(self.new_array(vec![matched; count]))
    }
}

#[cfg(test)]
#[path = "literal_matches_tests.rs"]
mod tests;
