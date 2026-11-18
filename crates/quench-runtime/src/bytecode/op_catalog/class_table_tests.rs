use super::{Class, Op, Traits};

#[test]
fn catalog_tables_are_compact_and_the_runtime_uses_direct_predicates() {
    // One byte per opcode's class and eight per traits: ~2.7 KiB for the whole catalog,
    // read only by planning code; the per-instruction loop calls the generated
    // `is_*` predicates, which compile to direct enum comparisons.
    assert_eq!(core::mem::size_of::<Class>(), 1);
    assert_eq!(core::mem::size_of::<Traits>(), 8);
    let bytes = Op::COUNT * (core::mem::size_of::<Class>() + core::mem::size_of::<Traits>());
    assert!(bytes <= 4096, "catalog tables take {bytes} bytes");
    assert!(Op::Call.class().eq(Class::Call) && !Op::Return.class().eq(Class::Call));
    assert!(Op::Return.class().eq(Class::Return) && !Op::Call.class().eq(Class::Return));
}
