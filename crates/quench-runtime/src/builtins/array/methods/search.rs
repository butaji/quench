//! Array search methods (indexOf, includes, find, findLast, findLastIndex)
//! are all self-hosted in JS (builtins/core/array_statics.js). Only the
//! regression-guard unit tests live here.

#[cfg(test)]
mod tests {
    fn create_test_context() -> crate::Context {
        crate::Context::new().unwrap()
    }

    #[test]
    fn test_includes_nan() {
        // Bug fix: includes uses SameValueZero, so [NaN].includes(NaN) is true
        let mut ctx = create_test_context();
        let result = ctx.eval("[NaN].includes(NaN)");
        assert_eq!(result.unwrap(), crate::value::Value::Boolean(true));
    }

    #[test]
    fn test_index_of_negative_from_index() {
        // Bug fix: negative fromIndex counts back from the end
        let mut ctx = create_test_context();
        let result = ctx.eval("[1,2,3].indexOf(2, -2)");
        assert_eq!(result.unwrap(), crate::value::Value::Number(1.0));
        let result = ctx.eval("[1,2,3].includes(1, -3)");
        assert_eq!(result.unwrap(), crate::value::Value::Boolean(true));
    }
}