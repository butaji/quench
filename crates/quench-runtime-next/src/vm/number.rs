use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn call_number_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        Ok(match native {
            Native::Number => Value::number(self.to_number(p, value)?),
            Native::NumberIsNaN => {
                if value.as_number().is_some_and(f64::is_nan) {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            Native::NumberIsFinite => {
                if value.as_number().is_some_and(f64::is_finite) {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            Native::NumberIsInteger => {
                if value
                    .as_number()
                    .is_some_and(|value| value.is_finite() && value.fract() == 0.0)
                {
                    Value::TRUE
                } else {
                    Value::FALSE
                }
            }
            _ => return Err(JsError("invalid Number native".into())),
        })
    }
}
