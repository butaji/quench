fn is_default_derived_constructor(function: &crate::value::FunctionValue) -> bool {
    function
        .properties
        .borrow()
        .iter()
        .any(|(name, _)| name == "\0default_derived_constructor")
}
