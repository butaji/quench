use super::{Class, Op, Traits};

#[test]
fn catalog_tables_are_compact_and_the_runtime_uses_direct_predicates() {
    // One byte per opcode's class and sixteen per traits: about 5 KiB for the whole catalog,
    // read only by planning code; the per-instruction loop calls the generated
    // `is_*` predicates, which compile to direct enum comparisons.
    assert_eq!(core::mem::size_of::<Class>(), 1);
    assert_eq!(core::mem::size_of::<Traits>(), 32);
    let bytes = Op::COUNT * (core::mem::size_of::<Class>() + core::mem::size_of::<Traits>());
    assert!(bytes <= 16384, "catalog tables take {bytes} bytes");
    assert!(Op::Call.class().eq(Class::Call) && !Op::Return.class().eq(Class::Call));
    assert!(Op::Return.class().eq(Class::Return) && !Op::Call.class().eq(Class::Return));
}

#[test]
fn role_traits_select_exactly_the_ops_they_replaced() {
    use crate::compile::liveness::raw_webs::raw_form;
    let carriers = |role: Traits| {
        let mut ops: Vec<Op> = (0..Op::COUNT).filter_map(Op::from_index).filter(|op| op.traits().has(role)).collect();
        ops.sort_by_key(|op| *op as u16);
        ops
    };
    let expect = |role: Traits, replaced: &[Op]| {
        let mut ops = replaced.to_vec();
        ops.sort_by_key(|op| *op as u16);
        assert_eq!(carriers(role), ops);
    };
    expect(Traits::MAKES_CLOSURE, &[Op::MakeClosure]);
    expect(Traits::NO_OP, &[Op::Nop]);
    expect(Traits::REGISTER_COPY, &[Op::Move]);
    expect(Traits::RETURN_VALUE, &[Op::Return]);
    expect(Traits::FIELD_LEAF, &[Op::GetField]);
    expect(Traits::FRAME_LOCAL_LOAD, &[Op::LoadLocal, Op::LoadLocalPlain]);
    expect(Traits::ENVIRONMENT_CLONE, &[Op::CloneEnv]);
    expect(Traits::ENV_LOCAL_LOAD, &[Op::LoadEnvLocal]);
    expect(Traits::ENV_LOCAL_STORE, &[Op::StoreEnvLocal]);
    expect(Traits::CAPTURE_LOAD, &[Op::LoadCapture]);
    expect(Traits::CAPTURE_STORE, &[Op::StoreCapture]);
    expect(Traits::ELEMENT_LOAD, &[Op::GetIndex]);
    expect(Traits::SPREAD_CALL, &[Op::CallSpread]);
    expect(Traits::REST_SPREAD_CALL, &[Op::CallRest]);
    expect(Traits::REST_INDEX_READ, &[Op::RestGet]);
    expect(Traits::REST_LENGTH_READ, &[Op::RestLength]);
    expect(Traits::PLAIN_CALL, &[Op::Call]);
    expect(
        Traits::DISTANT_PRODUCER,
        &[
            Op::GetIndex,
            Op::ToNumeric,
            Op::ToPropertyKey,
            Op::MakeArray,
            Op::MakeConstArray,
            Op::MakeObject,
            Op::MakeObjectLiteral,
            Op::MakeLiteralTemplate,
            Op::CreateRegExpLiteral,
            Op::LoadCallee,
            Op::LoadNameTypeof,
            Op::LoadCachedTemplateObject,
            Op::StringConcat,
            Op::ArgumentsLength,
            Op::ArgumentGet,
            Op::NumericAdd,
            Op::NumericMultiply,
            Op::PrivateIn,
        ],
    );
    expect(Traits::I64_BINARY_FAMILY, &[Op::WasmI64Binary, Op::WasmI64BinaryRaw]);
    expect(Traits::I64_BINARY_IMMEDIATE_FAMILY, &[Op::WasmI64BinaryImmediate, Op::WasmI64BinaryImmediateRaw]);
    expect(Traits::I64_UNARY_FAMILY, &[Op::WasmI64Unary, Op::WasmI64UnaryRaw]);
    expect(Traits::F64_BINARY_FAMILY, &[Op::WasmF64Binary, Op::WasmF64BinaryRaw]);
    expect(Traits::F64_UNARY_FAMILY, &[Op::WasmF64Unary, Op::WasmF64UnaryRaw]);
    expect(Traits::RETURN_FAMILY, &[Op::Return, Op::WasmReturn64]);
    expect(Traits::STRUCT_GET_FAMILY, &[Op::WasmStructGet, Op::WasmStructGetRaw]);
    expect(Traits::STRUCT_SET_FAMILY, &[Op::WasmStructSet, Op::WasmStructSetRaw]);
    expect(Traits::ARRAY_GET_FAMILY, &[Op::WasmArrayGet, Op::WasmArrayGetRaw]);
    expect(Traits::ARRAY_SET_FAMILY, &[Op::WasmArraySet, Op::WasmArraySetRaw]);
    expect(
        Traits::RAW_FORM,
        &[
            Op::WasmI64BinaryRaw, Op::WasmI64BinaryImmediateRaw, Op::WasmI64UnaryRaw, Op::WasmF64BinaryRaw, Op::WasmF64UnaryRaw, Op::WasmReturn64,
            Op::WasmStructGetRaw, Op::WasmStructSetRaw, Op::WasmArrayGetRaw, Op::WasmArraySetRaw,
        ],
    );
    assert_eq!(raw_form(Op::WasmStructGet), Some(Op::WasmStructGetRaw));
    assert_eq!(raw_form(Op::WasmStructSet), Some(Op::WasmStructSetRaw));
    assert_eq!(raw_form(Op::WasmArrayGet), Some(Op::WasmArrayGetRaw));
    assert_eq!(raw_form(Op::WasmArraySet), Some(Op::WasmArraySetRaw));
    assert_eq!(raw_form(Op::WasmStructGetS), None);
    assert_eq!(raw_form(Op::WasmI64Binary), Some(Op::WasmI64BinaryRaw));
    assert_eq!(raw_form(Op::WasmI64BinaryImmediate), Some(Op::WasmI64BinaryImmediateRaw));
    assert_eq!(raw_form(Op::WasmI64Unary), Some(Op::WasmI64UnaryRaw));
    assert_eq!(raw_form(Op::WasmF64Binary), Some(Op::WasmF64BinaryRaw));
    assert_eq!(raw_form(Op::WasmF64Unary), Some(Op::WasmF64UnaryRaw));
    assert_eq!(raw_form(Op::Return), Some(Op::WasmReturn64));
    assert_eq!(raw_form(Op::WasmReturn64), None);
    assert_eq!(raw_form(Op::Move), None);
}
