//! The C twin of every staged `Operation` (S1.17, TOD-2193): the `ops.c` kernel the operation is measured
//! against, or an explicit `CTwin::None` with its reason (`quench_bjit::c_twin`, which the catalog's rows share).
//! `row_twins!` writes one list of `Variant => twin` arms as both an exhaustive match with no catch-all
//! (`c_twin`: a new variant without an arm does not compile) and the rows it names (`c_twin_rows`), which
//! `tests/c_twin.rs` checks against the C corpus. Twins are the inventory's name matches
//! (`s117/c-twins.md`, table A); the 3C+6 bound itself is TOD-2193 step 3 (`quench_bjit::c_twin`).

pub use quench_bjit::c_twin::{CKernel, CTwin};

use super::numeric::NumberOperation;
use super::Operation;

/// `trait NAME for TYPE, <GENERICS> as "LABEL" { Variant => twin, .. }`, its rows named `LABEL::Variant`: `twin` is `kernel(name)`,
/// `none("reason")`, or `by(expr, rows)` for a variant whose payload decides it (`expr` over the variant's
/// bindings, `rows` the twins it can take).
macro_rules! row_twins {
    (trait $name:ident $(<$($tgen:ident),*>)? for $type:ty $(, <$($generic:ident),*>)? as $label:literal {
        $($variant:ident $(($bound:ident))? $({ $($field:ident),* })? => $kind:ident($($what:tt)*),)*
    }) => {
        /// The C twins of this type's rows, as an extension trait: quench-staged owns the type.
        pub trait $name $(<$($tgen),*>)? {
            /// The C kernel this operation is measured against, or why none is.
            fn c_twin(&self) -> CTwin;

            /// Every row this type's operations make, by name, with its twin.
            fn c_twin_rows() -> Vec<(&'static str, CTwin)>;
        }

        impl $(<$($generic),*>)? $name $(<$($tgen),*>)? for $type {
            fn c_twin(&self) -> CTwin {
                match self {
                    $(row_twins!(@pattern $variant $(($bound))? $({ $($field),* })?) => row_twins!(@twin $kind($($what)*)),)*
                }
            }

            fn c_twin_rows() -> Vec<(&'static str, CTwin)> {
                let mut rows = Vec::new();
                $(rows.extend(row_twins!(@rows $label, $variant $kind($($what)*)));)*
                rows
            }
        }
    };
    (@pattern $variant:ident) => { Self::$variant { .. } };
    (@pattern $variant:ident ($bound:ident)) => { Self::$variant($bound) };
    (@pattern $variant:ident { $($field:ident),* }) => { Self::$variant { $($field,)* .. } };
    (@twin by($decided:expr, $rows:expr)) => { $decided };
    (@twin $kind:ident($what:tt)) => { quench_bjit::c_twin!($kind($what)) };
    (@rows $label:literal, $variant:ident by($decided:expr, $rows:expr)) => { $rows };
    (@rows $label:literal, $variant:ident $kind:ident($what:tt)) => {
        [(concat!($label, "::", stringify!($variant)), quench_bjit::c_twin!($kind($what)))]
    };
}

/// `Equality`'s rows: a strict equality is measured against `eq_int` (`eq_str` measures the string case of the
/// same row), a loose one against `eq_undef_null`, its nullish fast case.
const EQUALITY_TWINS: [(&str, CTwin); 2] = [
    ("Operation::Equality(strict)", quench_bjit::c_twin!(kernel(eq_int))),
    ("Operation::Equality(loose)", quench_bjit::c_twin!(kernel(eq_undef_null))),
];

fn equality_twin(strict: bool) -> CTwin {
    EQUALITY_TWINS[usize::from(!strict)].1
}

row_twins! {
    trait NumberOperationCTwinsExt for NumberOperation as "NumberOperation" {
        Arithmetic => kernel(int_add),
        Relation => kernel(cmp_int),
        RelationValue => kernel(cmp_dbl),
        Bitwise => kernel(bitwise),
        Remainder => kernel(int_mod),
        IncDec => kernel(inc_local),
        Unary => kernel(neg_op),
        BitwiseNot => kernel(bit_not),
        ToNumeric => none("ToNumeric of a Number: no benchmark op isolates it"),
        Math => kernel(math_sqrt),
        Extreme => kernel(math_minmax),
        Imul => kernel(math_imul),
        IntCase => kernel(c_switch_dense),
        Power => none("Math.pow of a constant exponent: no benchmark op isolates it"),
    }
}

row_twins! {
    trait OperationCTwinsExt<W, P> for Operation<W, P>, <W, P> as "Operation" {
        Move => kernel(c_locals_rotate),
        LoadConst => none("constant load: no benchmark op isolates it"),
        LoadLocalPlain => kernel(c_locals_rotate),
        StoreLocalPlain => kernel(c_locals_rotate),
        LoadLocalChecked => kernel(v_let_block),
        StoreLocalChecked => kernel(v_let_block),
        InitializeLocal => kernel(v_let_block),
        Jump => kernel(empty),
        JumpIf => kernel(truthy_int),
        LoadThis => kernel(c_class_method),
        LoadCallee => none("callee read: no benchmark op isolates it"),
        Number(number) => by(number.c_twin(), NumberOperation::c_twin_rows()),
        RequireObjectCoercible => kernel(v_destructure_obj),
        RequireIteratorResult => none("iterator result check: no benchmark op isolates it"),
        LogicalNot => kernel(not_op),
        TypeofIndex => kernel(typeof_op),
        TypeofTest => kernel(typeof_op),
        TypeofString => kernel(typeof_op),
        IndexWrite => kernel(arr_store_int),
        AddSite => none("stencil row only: no opcode decodes to it"),
        NamingSite => none("stencil row only: no opcode decodes to it"),
        InstanceOf => kernel(v_instanceof),
        DirectArray => none("stencil guard: its cost is the guarded operation's"),
        ArrayLength => kernel(arr_length),
        TypedLength => kernel(typed_length),
        ArrayAppend => kernel(arr_push),
        Allocate => kernel(alloc_literal),
        SuperConstArrayObject2 => none("superinstruction: its parts' twins measure it"),
        DefineElement => kernel(v_alloc_array4),
        Safepoint => kernel(empty),
        GuardIdentity => none("stencil guard: its cost is the guarded operation's"),
        GuardLive => none("stencil guard: its cost is the guarded operation's"),
        GuardRealm => none("stencil guard: its cost is the guarded operation's"),
        GuardDeclared => none("stencil guard: its cost is the guarded operation's"),
        GuardPrototype => none("stencil guard: its cost is the guarded operation's"),
        GuardShape => none("stencil guard: its cost is the guarded operation's"),
        GuardStores => none("stencil guard: its cost is the guarded operation's"),
        BackEdge => kernel(empty),
        IndexSetHelper => none("helper store: no benchmark op isolates it"),
        LoadName => none("lane_load_name_resolve runs inside the math_* ops, not as a twin"),
        Void => none("void: no benchmark op isolates it"),
        Unary => none("VM callout: no benchmark op isolates it"),
        PropertyKey => none("ToPropertyKey: no benchmark op isolates it"),
        Equality { strict } => by(equality_twin(*strict), EQUALITY_TWINS.to_vec()),
        Binary => kernel(str_compare),
        LoadEnvLocal => kernel(c_closure_read),
        StoreEnvLocal => kernel(v_closure_counter),
        ArgumentsLength => kernel(v_arguments_length),
        ArgumentGet => kernel(v_arguments_length),
        CloneEnv => kernel(v_alloc_closure),
        GlobalTypeof => none("global typeof: no benchmark op isolates it"),
        SetFunctionName => none("function naming: no benchmark op isolates it"),
        GlobalWrite => kernel(v_global_var),
        GlobalLexicalLoad => none("global lexical read: no benchmark op isolates it"),
        GlobalLexicalStore => none("global lexical write: no benchmark op isolates it"),
        Protocol => kernel(v_for_of_array),
        ForInNext => kernel(v_for_in),
        EnumeratedGet => kernel(v_for_in),
        FieldStoreMiss => none("miss edge: no benchmark op isolates it"),
        IteratorCleanupPush => kernel(v_for_of_array),
        IteratorCleanupPop => kernel(v_for_of_array),
        StringCaseIndex => kernel(str_charcodeat),
        Suspend => none("generator suspension: no benchmark op isolates it"),
        Nop => kernel(empty),
        GetField => kernel(field_read),
        SetField => kernel(field_store),
        StoreField => kernel(field_store),
        FieldGetHost => none("host helper: no benchmark op isolates it"),
        FieldAdd => kernel(v_obj_add),
        Delete => none("delete: no benchmark op isolates it"),
        IndexGetHost => none("host helper: no benchmark op isolates it"),
        Call => kernel(call_direct),
        SpreadCall => kernel(call_spread),
        SuperCall => kernel(v_super_call),
        Return => kernel(call_direct),
        Throw => kernel(v_throw_catch),
        ElementRead => kernel(arr_load_int),
        ElementReadOperand => kernel(arr_load_int),
        IncDecLocal => kernel(inc_local),
        ElementWrite => kernel(arr_store_int),
        LoadCapture => kernel(c_closure_read),
        StoreCapture => kernel(v_closure_counter),
        CompareJump => kernel(cmp_int),
        WasmFloat => none("no wasm ops in ops.js"),
        WasmInt => none("no wasm ops in ops.js"),
        WasmJump => none("no wasm ops in ops.js"),
        BranchTable => none("no wasm ops in ops.js"),
        WasmState => none("no wasm ops in ops.js"),
        WasmAccess => none("no wasm ops in ops.js"),
        WasmVector => none("no wasm ops in ops.js"),
        Prologue => kernel(empty),
        BodyEntry => none("specialized body entry: no benchmark op isolates it"),
    }
}
