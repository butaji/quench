//! `step` taken apart by operation kind (`step_apart`): one out-of-line instance of `step_of` per kind, for a tier
//! that runs every operation through one call site. Inlined there, the whole of `step` was one 367 KB function
//! (`compiled::lane_region::Lowering::step_with`, the u-jit's lowering), compiled by one LLVM thread on every edit to
//! any staged rule; apart, an edit recompiles the kinds whose arms reach it, spread over `SHARDS` codegen units.
use super::*;
use super::operation::kind;

/// The operation's kind (`operation::kind`): the variant's place in the enum, counted from 1.
pub const fn operation_kind<W, P>(operation: &Operation<W, P>) -> u16 {
    match operation {
        Operation::Move { .. } => kind::MOVE,
        Operation::LoadConst { .. } => kind::LOAD_CONST,
        Operation::LoadLocalPlain { .. } => kind::LOAD_LOCAL_PLAIN,
        Operation::StoreLocalPlain { .. } => kind::STORE_LOCAL_PLAIN,
        Operation::LoadLocalChecked { .. } => kind::LOAD_LOCAL_CHECKED,
        Operation::StoreLocalChecked { .. } => kind::STORE_LOCAL_CHECKED,
        Operation::InitializeLocal { .. } => kind::INITIALIZE_LOCAL,
        Operation::Jump { .. } => kind::JUMP,
        Operation::JumpIf { .. } => kind::JUMP_IF,
        Operation::LoadThis { .. } => kind::LOAD_THIS,
        Operation::LoadCallee { .. } => kind::LOAD_CALLEE,
        Operation::Number { .. } => kind::NUMBER,
        Operation::RequireObjectCoercible { .. } => kind::REQUIRE_OBJECT_COERCIBLE,
        Operation::RequireIteratorResult { .. } => kind::REQUIRE_ITERATOR_RESULT,
        Operation::LogicalNot { .. } => kind::LOGICAL_NOT,
        Operation::TypeofIndex { .. } => kind::TYPEOF_INDEX,
        Operation::TypeofTest { .. } => kind::TYPEOF_TEST,
        Operation::TypeofString { .. } => kind::TYPEOF_STRING,
        Operation::IndexWrite { .. } => kind::INDEX_WRITE,
        Operation::InstanceOf { .. } => kind::INSTANCE_OF,
        Operation::DirectArray { .. } => kind::DIRECT_ARRAY,
        Operation::ArrayLength { .. } => kind::ARRAY_LENGTH,
        Operation::TypedLength { .. } => kind::TYPED_LENGTH,
        Operation::ArrayAppend { .. } => kind::ARRAY_APPEND,
        Operation::Allocate { .. } => kind::ALLOCATE,
        Operation::SuperConstArrayObject2 { .. } => kind::SUPER_CONST_ARRAY_OBJECT2,
        Operation::DefineElement { .. } => kind::DEFINE_ELEMENT,
        Operation::Safepoint { .. } => kind::SAFEPOINT,
        Operation::GuardIdentity { .. } => kind::GUARD_IDENTITY,
        Operation::GuardLive { .. } => kind::GUARD_LIVE,
        Operation::GuardRealm { .. } => kind::GUARD_REALM,
        Operation::GuardDeclared { .. } => kind::GUARD_DECLARED,
        Operation::GuardPrototype { .. } => kind::GUARD_PROTOTYPE,
        Operation::GuardShape { .. } => kind::GUARD_SHAPE,
        Operation::GuardClass { .. } => kind::GUARD_CLASS,
        Operation::GuardStores { .. } => kind::GUARD_STORES,
        Operation::BackEdge { .. } => kind::BACK_EDGE,
        Operation::IndexSetHelper { .. } => kind::INDEX_SET_HELPER,
        Operation::LoadName { .. } => kind::LOAD_NAME,
        Operation::Void { .. } => kind::VOID,
        Operation::Unary { .. } => kind::UNARY,
        Operation::PropertyKey { .. } => kind::PROPERTY_KEY,
        Operation::Equality { .. } => kind::EQUALITY,
        Operation::Binary { .. } => kind::BINARY,
        Operation::LoadEnvLocal { .. } => kind::LOAD_ENV_LOCAL,
        Operation::StoreEnvLocal { .. } => kind::STORE_ENV_LOCAL,
        Operation::ArgumentsLength { .. } => kind::ARGUMENTS_LENGTH,
        Operation::ArgumentGet { .. } => kind::ARGUMENT_GET,
        Operation::CloneEnv { .. } => kind::CLONE_ENV,
        Operation::GlobalTypeof { .. } => kind::GLOBAL_TYPEOF,
        Operation::SetFunctionName { .. } => kind::SET_FUNCTION_NAME,
        Operation::GlobalWrite { .. } => kind::GLOBAL_WRITE,
        Operation::GlobalLexicalLoad { .. } => kind::GLOBAL_LEXICAL_LOAD,
        Operation::GlobalLexicalStore { .. } => kind::GLOBAL_LEXICAL_STORE,
        Operation::Protocol { .. } => kind::PROTOCOL,
        Operation::ForInNext { .. } => kind::FOR_IN_NEXT,
        Operation::EnumeratedGet { .. } => kind::ENUMERATED_GET,
        Operation::FieldStoreMiss { .. } => kind::FIELD_STORE_MISS,
        Operation::IteratorCleanupPush { .. } => kind::ITERATOR_CLEANUP_PUSH,
        Operation::IteratorCleanupPop { .. } => kind::ITERATOR_CLEANUP_POP,
        Operation::StringCaseIndex { .. } => kind::STRING_CASE_INDEX,
        Operation::Suspend { .. } => kind::SUSPEND,
        Operation::Nop { .. } => kind::NOP,
        Operation::GetField { .. } => kind::GET_FIELD,
        Operation::SetField { .. } => kind::SET_FIELD,
        Operation::StoreField { .. } => kind::STORE_FIELD,
        Operation::FieldGetHost { .. } => kind::FIELD_GET_HOST,
        Operation::FieldAdd { .. } => kind::FIELD_ADD,
        Operation::Delete { .. } => kind::DELETE,
        Operation::IndexGetHost { .. } => kind::INDEX_GET_HOST,
        Operation::Call { .. } => kind::CALL,
        Operation::SpreadCall { .. } => kind::SPREAD_CALL,
        Operation::SuperCall { .. } => kind::SUPER_CALL,
        Operation::Return { .. } => kind::RETURN,
        Operation::Throw { .. } => kind::THROW,
        Operation::ElementRead { .. } => kind::ELEMENT_READ,
        Operation::ElementReadOperand { .. } => kind::ELEMENT_READ_OPERAND,
        Operation::IncDecLocal { .. } => kind::INC_DEC_LOCAL,
        Operation::ElementWrite { .. } => kind::ELEMENT_WRITE,
        Operation::LoadCapture { .. } => kind::LOAD_CAPTURE,
        Operation::StoreCapture { .. } => kind::STORE_CAPTURE,
        Operation::CompareJump { .. } => kind::COMPARE_JUMP,
        Operation::WasmFloat { .. } => kind::WASM_FLOAT,
        Operation::WasmInt { .. } => kind::WASM_INT,
        Operation::WasmJump { .. } => kind::WASM_JUMP,
        Operation::BranchTable { .. } => kind::BRANCH_TABLE,
        Operation::WasmState { .. } => kind::WASM_STATE,
        Operation::WasmAccess { .. } => kind::WASM_ACCESS,
        Operation::WasmVector { .. } => kind::WASM_VECTOR,
        Operation::Prologue { .. } => kind::PROLOGUE,
        Operation::BodyEntry { .. } => kind::BODY_ENTRY,
    }
}

/// `step` for any operation, as the out-of-line `step_of` instance of its kind.
#[inline(always)]
pub fn step_apart<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let kinds: &[fn(&mut S, &mut F, Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control>] = const { &KindTable::<S, F>::STEPS };
    kinds[usize::from(operation_kind(&operation)) - 1](stage, frame, operation)
}

/// Kind `K`'s instance, in shard module `K % SHARDS`: instances of one generic function share a codegen unit.
const fn shard<const K: u16, S: NumericStage, F: NumericFrame<S>>() -> fn(&mut S, &mut F, Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match K % SHARDS { 0 => s0::step::<K, S, F>, 1 => s1::step::<K, S, F>, 2 => s2::step::<K, S, F>, 3 => s3::step::<K, S, F>,
        4 => s4::step::<K, S, F>, 5 => s5::step::<K, S, F>, 6 => s6::step::<K, S, F>, _ => s7::step::<K, S, F> }
}

const SHARDS: u16 = 8;

macro_rules! shards { ($($shard:ident)*) => { $(mod $shard {
    use super::*;
    #[inline(never)]
    pub(super) fn step<const K: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
        super::step_of::<K, S, F>(stage, frame, operation)
    }
})* }; }
shards!(s0 s1 s2 s3 s4 s5 s6 s7);

/// Every kind after `ANY`, in kind order: `step_apart` indexes its table by `kind - 1`, and the
/// assertion below holds the order to the numbering, so a kind added out of place fails to compile.
macro_rules! kind_table { ($($kind:ident)*) => {
    const KIND_ORDER: &[u16] = &[$(kind::$kind),*];
    const _: () = {
        let mut at = 0;
        while at < KIND_ORDER.len() { assert!(KIND_ORDER[at] as usize == at + 1, "step_apart's table is out of kind order"); at += 1; }
    };
    struct KindTable<S, F>(core::marker::PhantomData<(S, F)>);
    impl<S: NumericStage, F: NumericFrame<S>> KindTable<S, F> {
        const STEPS: [fn(&mut S, &mut F, Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control>; KIND_ORDER.len()] = [$(shard::<{ kind::$kind }, S, F>()),*];
    }
}; }
kind_table! {
    MOVE LOAD_CONST LOAD_LOCAL_PLAIN STORE_LOCAL_PLAIN LOAD_LOCAL_CHECKED STORE_LOCAL_CHECKED
    INITIALIZE_LOCAL JUMP JUMP_IF LOAD_THIS LOAD_CALLEE NUMBER
    REQUIRE_OBJECT_COERCIBLE REQUIRE_ITERATOR_RESULT LOGICAL_NOT TYPEOF_INDEX TYPEOF_TEST TYPEOF_STRING
    INDEX_WRITE ADD_SITE NAMING_SITE INSTANCE_OF DIRECT_ARRAY ARRAY_LENGTH
    TYPED_LENGTH ARRAY_APPEND ALLOCATE SUPER_CONST_ARRAY_OBJECT2 DEFINE_ELEMENT SAFEPOINT
    GUARD_IDENTITY GUARD_LIVE GUARD_REALM GUARD_DECLARED GUARD_PROTOTYPE GUARD_SHAPE
    GUARD_STORES BACK_EDGE INDEX_SET_HELPER LOAD_NAME VOID UNARY
    PROPERTY_KEY EQUALITY BINARY LOAD_ENV_LOCAL STORE_ENV_LOCAL ARGUMENTS_LENGTH
    ARGUMENT_GET CLONE_ENV GLOBAL_TYPEOF SET_FUNCTION_NAME GLOBAL_WRITE GLOBAL_LEXICAL_LOAD
    GLOBAL_LEXICAL_STORE PROTOCOL FOR_IN_NEXT ENUMERATED_GET FIELD_STORE_MISS ITERATOR_CLEANUP_PUSH
    ITERATOR_CLEANUP_POP STRING_CASE_INDEX SUSPEND NOP GET_FIELD SET_FIELD
    STORE_FIELD FIELD_GET_HOST FIELD_ADD DELETE INDEX_GET_HOST CALL
    SPREAD_CALL SUPER_CALL RETURN THROW ELEMENT_READ ELEMENT_READ_OPERAND
    INC_DEC_LOCAL ELEMENT_WRITE LOAD_CAPTURE STORE_CAPTURE COMPARE_JUMP WASM_FLOAT
    WASM_INT WASM_JUMP BRANCH_TABLE WASM_STATE WASM_ACCESS WASM_VECTOR
    PROLOGUE BODY_ENTRY GUARD_CLASS
}
