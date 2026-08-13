pub(crate) fn own_property_names(builtin: Builtin) -> &'static [&'static str] {
    match builtin {
        Builtin::BigInt => &["length", "name", "prototype", "asIntN", "asUintN"],
        Builtin::BigIntPrototype => {
            &["constructor", "toString", "valueOf", "Symbol.toStringTag"]
        }
        Builtin::Error => &[
            "length",
            "name",
            "prototype",
            "isError",
        ],
        Builtin::ErrorPrototype => {
            &["constructor", "name", "message", "cause", "stack", "toString"]
        }
        Builtin::DisposableStack => &["length", "name", "prototype"],
        Builtin::DisposableStackPrototype => &[
            "constructor", "use", "adopt", "defer", "move", "dispose", "disposed",
            "Symbol.dispose", "Symbol.toStringTag",
        ],
        Builtin::AsyncDisposableStack => &["length", "name", "prototype"],
        Builtin::FinalizationRegistry => &["length", "name", "prototype"],
        Builtin::FinalizationRegistryPrototype => &[
            "constructor", "register", "unregister", "Symbol.toStringTag",
        ],
        Builtin::AsyncDisposableStackPrototype => &[
            "constructor", "use", "adopt", "defer", "move", "disposeAsync", "disposed",
            "Symbol.asyncDispose", "Symbol.toStringTag",
        ],
        _ => &[],
    }
}
