fn collections_prop(builtin: Builtin, key: &str) -> Option<Value> {
    crate::builtin_meta::collections::collections_property(builtin, key)
}
