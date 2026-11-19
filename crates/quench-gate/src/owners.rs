//! Which thread owns a test's code, by the longest matching module prefix (the ownership map of
//! Linear TOD-1743): `J` the JIT tiers and the tools that gate them, `R` the runtime (interpreter,
//! heap, compile pipeline), `I` the trunk (host and everything else).

/// Module prefix and owning thread; the longest matching prefix decides.
const OWNERS: &[(&str, &str)] = &[
    ("vm::compiled", "J"),
    ("staged", "J"),
    ("code_dependencies", "J"),
    ("api_native_tests", "J"),
    ("vm::jit_backend", "J"),
    ("vm::tier_machine", "J"),
    ("vm::dispatch_fast", "R"),
    ("vm::dispatch", "R"),
    ("vm::tests", "R"),
    ("vm::wasm", "R"),
    ("vm::regexp", "R"),
    ("vm::object", "R"),
    ("heap", "R"),
    ("value_vec", "R"),
    ("compile", "R"),
    ("bytecode", "R"),
    ("snapshot", "R"),
];

/// The thread that owns the test named `test`.
pub fn of(test: &str) -> &'static str {
    OWNERS.iter().filter(|(prefix, _)| test.starts_with(prefix)).max_by_key(|(prefix, _)| prefix.len()).map_or("I", |(_, owner)| owner)
}
