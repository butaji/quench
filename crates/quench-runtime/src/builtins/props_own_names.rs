pub(crate) fn own_property_names(builtin: Builtin) -> &'static [&'static str] {
    match builtin {
        Builtin::BigInt => &["length", "name", "prototype", "asIntN", "asUintN"],
        Builtin::BigIntPrototype => {
            &["constructor", "toString", "valueOf", "Symbol.toStringTag"]
        }
        Builtin::DisposableStack => &["length", "name", "prototype"],
        Builtin::DisposableStackPrototype => &[
            "constructor", "use", "adopt", "defer", "move", "dispose", "disposed",
            "Symbol.dispose", "Symbol.toStringTag",
        ],
        _ => &[],
    }
}
