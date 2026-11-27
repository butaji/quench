//! The calls a staged row leaves to the VM (`staged::CallOut::Spread`, `SuperCall`): a call with a
//! spread argument and `super(...)`. The argument values are gathered once from the operand
//! registers the tier read (`staged::Passing`), then the VM's own call or construction runs, as
//! the interpreter's `CallSpread`, `CallArray` and super `Construct` do, so every tier shares the
//! one definition.

use super::*;
use crate::staged::Passing;
use std::borrow::Cow;

impl Vm {
    /// `callee` called with `this` and the arguments `passing` makes of `list`, the values of the
    /// call's argument registers.
    pub(super) fn spread_call(&mut self, p: &ResidualProgram, site: super::call_out::CallSite, passing: Passing,
        [callee, this]: [Option<Value>; 2], list: &[Value]) -> Result<Value, JsError> {
        let (callee, this) = (callee.expect("a spread call names its callee"), this.expect("a spread call names its receiver"));
        let kept;
        let (passing, list) = match passing {
            // The kept values are the rest array's elements: passed as they are while spreading
            // an array runs no guest code, else spread from the array they make.
            Passing::Rest if list.first() == Some(&Value::UNDEFINED) => {
                let values = self.with_call_roots([callee, this], |vm| vm.kept_rest_values(p, site))?;
                if self.fresh_array_iteration_intact() {
                    kept = values;
                    (Passing::Values, kept.as_slice())
                } else {
                    kept = vec![self.rest_parameter_array(&values)];
                    (Passing::Iterable, kept.as_slice())
                }
            }
            Passing::Rest => (Passing::Iterable, list),
            passing => (passing, list),
        };
        let args = self.with_call_roots([callee, this], move |vm| vm.gathered_arguments(p, passing, list))?;
        self.with_call_roots([callee, this].into_iter().chain(args.iter().copied()),
            |vm| vm.call_value_from_frame(p, callee, this, &args))
    }

    /// `callee` constructed by `super(...)` with the arguments `passing` makes of `list`. The
    /// construction reads the running activation's `new.target`, which only a frame holds, so
    /// activations a lane runs frameless become frames first.
    pub(super) fn super_call(&mut self, p: &ResidualProgram, passing: Passing, callee: Option<Value>, list: &[Value])
        -> Result<Value, JsError> {
        let callee = callee.expect("a super construction names its callee");
        if self.lane_deferred.is_some() {
            self.materialize_deferred_lane();
        }
        let args = match passing {
            Passing::Array => Cow::Owned(self.super_argument_list(p, list)?),
            Passing::Values | Passing::Iterable | Passing::Rest => self.with_call_roots([callee], move |vm| vm.gathered_arguments(p, passing, list))?,
        };
        self.with_call_roots([callee].into_iter().chain(args.iter().copied()), |vm| vm.construct_super_value(p, callee, &args))
    }

    /// The argument values of `list` as `passing` holds them: the values themselves, borrowed, or
    /// the elements of the array in the one register, or the values an iterable in it spreads into
    /// a new array.
    fn gathered_arguments<'a>(&mut self, p: &ResidualProgram, passing: Passing, list: &'a [Value]) -> Result<Cow<'a, [Value]>, JsError> {
        if passing == Passing::Values { return Ok(Cow::Borrowed(list)); }
        let argument = list.first().copied().expect("a spread names its argument register");
        if passing == Passing::Array { return Ok(Cow::Owned(self.array_values(argument)?)); }
        let array = self.hole_array(0);
        self.with_call_roots([array, argument], |vm| vm.spread_into_array(p, array, argument))?;
        Ok(Cow::Owned(self.array_values(array)?))
    }

    /// The values a rest view's activation kept past its fixed parameters (`Function::rest_view`),
    /// for `CallRest` at `site` whose rest register holds `undefined`: the activation's frame
    /// keeps every value passed (`original_arguments`); activations a lane runs frameless become
    /// frames first, as `super_call` makes them.
    fn kept_rest_values(&mut self, p: &ResidualProgram, site: super::call_out::CallSite) -> Result<Vec<Value>, JsError> {
        let frame = match site.owner {
            VarOwner::Frame(frame) => frame,
            _ => {
                if self.lane_deferred.is_some() {
                    self.materialize_deferred_lane();
                }
                self.frames.len().checked_sub(1).ok_or_else(|| JsError("a rest spread runs in no activation".into()))?
            }
        };
        let function = p.function(site.function as usize);
        let frame = &self.frames[frame];
        if frame.function != site.function || !function.rest_view() {
            return Err(JsError("a rest spread runs outside its rest view's activation".into()));
        }
        let fixed = usize::from(function.params) - 1;
        Ok(frame.original_arguments.get(fixed..).unwrap_or_default().to_vec())
    }

    /// `RestLength` (`get` false) or `RestGet` of a rest view (`Protocol::RestView`) at `site`:
    /// while the rest register holds `undefined` (`held`), the values its activation kept
    /// (`kept_rest_values`) answer the length and an index they hold; any other key makes the
    /// rest array, stored into the rest register, which then answers as any array does.
    pub(super) fn rest_view_step(&mut self, p: &ResidualProgram, site: super::call_out::CallSite,
        (get, _fixed, rest): (bool, usize, crate::bytecode::Register), key: Option<Value>, mut held: Value) -> Result<Value, JsError> {
        let key = key.unwrap_or(Value::UNDEFINED);
        if held == Value::UNDEFINED {
            let passed = self.kept_rest_values(p, site)?;
            if !get {
                return Ok(Value::length(passed.len()));
            }
            if let Some(value) = key.as_int().and_then(|index| usize::try_from(index).ok()).and_then(|index| passed.get(index)) {
                return Ok(*value);
            }
            held = self.rest_parameter_array(&passed);
            self.set_owner_register(site.owner, rest, held);
        }
        if get { self.get_index(p, held, key) } else { self.get_property(p, held, self.length_atom) }
    }

    /// The arguments a super construction applies from the array in `list`'s one register.
    fn super_argument_list(&mut self, p: &ResidualProgram, list: &[Value]) -> Result<Vec<Value>, JsError> {
        let array = list.first().copied().expect("an applied super construction names its array register");
        if self.heap.array(array).is_none() {
            return Err(JsError("constructor arguments are not an array".into()));
        }
        self.call_argument_list(p, array, false)
    }
}
