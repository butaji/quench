//! The calls a staged row leaves to the VM (`staged::CallOut::Spread`, `SuperCall`): a call with a
//! spread argument and `super(...)`. The argument values are gathered once from the operand
//! registers the tier read (`staged::Passing`), then the VM's own call or construction runs, as
//! the interpreter's `CallSpread`, `CallArray` and super `Construct` do, so every tier shares the
//! one definition.

use super::*;
use crate::staged::Passing;

impl<H: Host> Vm<H> {
    /// `callee` called with `this` and the arguments `passing` makes of `list`, the values of the
    /// call's argument registers.
    pub(super) fn spread_call(&mut self, p: &ResidualProgram, passing: Passing, [callee, this]: [Option<Value>; 2],
        list: &[Value]) -> Result<Value, JsError> {
        let (callee, this) = (callee.expect("a spread call names its callee"), this.expect("a spread call names its receiver"));
        let args = self.with_call_roots([callee, this], |vm| vm.gathered_arguments(p, passing, list))?;
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
            Passing::Array => self.super_argument_list(p, list)?,
            Passing::Values | Passing::Iterable => self.with_call_roots([callee], |vm| vm.gathered_arguments(p, passing, list))?,
        };
        self.with_call_roots([callee].into_iter().chain(args.iter().copied()), |vm| vm.construct_super_value(p, callee, &args))
    }

    /// The argument values of `list` as `passing` holds them: the values themselves, the elements of
    /// the array in the one register, or the values an iterable in it spreads into a new array.
    fn gathered_arguments(&mut self, p: &ResidualProgram, passing: Passing, list: &[Value]) -> Result<Vec<Value>, JsError> {
        if passing == Passing::Values { return Ok(list.to_vec()); }
        let argument = list.first().copied().expect("a spread names its argument register");
        if passing == Passing::Array { return self.array_values(argument); }
        let array = self.hole_array(0);
        self.with_call_roots([array, argument], |vm| vm.spread_into_array(p, array, argument))?;
        self.array_values(array)
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
