//! The C twin of a staged row (S1.17, TOD-2193): the C-shaped kernel of the benchmark corpus a row is
//! measured against, or an explicit, reasoned `None`. The kernels are the measured functions of
//! `quench-bench/primitives/ops.c`, each named here as its registry entry there (`{"name", op_name}`), so a
//! variant of `CKernel` is that entry's name. Every row-defining list carries its twin as a required field:
//! the catalog's heap rows and Number kinds (`row_catalog.rs`, a line without `twin:` matches no arm of any
//! consumer) and the staged `Operation` (`quench-runtime` `staged::c_twins`, an exhaustive match).
//!
//! The bound, `default Ir/iter <= 3 * C + 6` for every `CTwin::Kernel` row (C the kernel's callgrind count), is
//! section `c-bound` of `quench-gate --only ir-budget`, held by `quench-tier-corpus/c_bound_allowance.txt`;
//! `CTwin::None` rows are listed there as unmeasured.

/// The kernel a row is measured against, or why it has none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CTwin {
    /// The `ops.c` kernel whose loop body is the row's operation.
    Kernel(CKernel),
    /// No benchmark kernel isolates the row; the reason must not be empty.
    None(&'static str),
}

macro_rules! kernels {
    ($($kernel:ident),* $(,)?) => {
        /// A measured function of `quench-bench/primitives/ops.c`, by its registry name there.
        #[allow(non_camel_case_types)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum CKernel { $($kernel),* }

        impl CKernel {
            /// Every kernel a row names.
            pub const ALL: &'static [CKernel] = &[$(CKernel::$kernel),*];

            /// The kernel's registry name in `ops.c` (and its benchmark op in `ops.js`).
            pub const fn name(self) -> &'static str {
                match self { $(CKernel::$kernel => stringify!($kernel)),* }
            }
        }
    };
}

kernels! {
    alloc_literal, arr_length, arr_load_int, arr_push, arr_store_int, bit_not, bitwise, c_bit_test,
    c_bool_flag, c_class_ctor_fields, c_class_method, c_closure_read, c_locals_rotate, c_switch_dense,
    call_direct, call_spread, cmp_dbl, cmp_int, dbl_div, empty, eq_int, eq_undef_null, field_read,
    field_store, inc_local, int_add, int_add_noor, int_mod, int_mul, int_sub, int_sub_noor, math_abs,
    math_ceil, math_clz32, math_floor, math_fround, math_imul, math_minmax, math_round, math_sign,
    math_sqrt, math_trunc, neg_op, not_op, parse_int, shl_shr, str_charcodeat, str_compare, truthy_int,
    typed_length, typed_load, typed_store_f64, typeof_op, ushr, v_alloc_array4, v_alloc_closure,
    v_arguments_length, v_array_push_grow, v_closure_counter, v_destructure_obj, v_for_in, v_for_of_array,
    v_global_var, v_instanceof, v_let_block, v_new_point, v_obj_add, v_proto_chain3_field, v_super_call,
    v_throw_catch,
}

/// A row's `twin:` field as its `CTwin`: `kernel(name)` or `none("reason")`. Any other spelling matches no arm.
#[macro_export]
macro_rules! c_twin {
    (kernel($kernel:ident)) => { $crate::c_twin::CTwin::Kernel($crate::c_twin::CKernel::$kernel) };
    (none($reason:literal)) => { $crate::c_twin::CTwin::None($reason) };
}

/// The catalog's twins (`row_catalog.rs`): each heap row's, chunk by chunk, and each Number kind's, by row name.
macro_rules! catalog_twins {
    (heap { $([$end:ident $table:ident: $($body:tt)*])* } number { $($kinds:tt)* }) => {
        /// Each heap row's twin, by row name, in the catalog's chunks.
        pub const HEAP_ROW_TWINS: &[&[(&str, CTwin)]] = &[$(catalog_twins!(@heap []; $($body)*)),*];
        /// Each Number row kind's twin, by kind name.
        pub const NUMBER_ROW_TWINS: &[(&str, CTwin)] = catalog_twins!(@number []; $($kinds)*);
    };
    (@heap [$($acc:tt)*];) => { &[$($acc)*] };
    (@heap [$($acc:tt)*]; reserve $n:expr $(, $($rest:tt)*)?) => {
        catalog_twins!(@heap [$($acc)*]; $($($rest)*)?)
    };
    (@heap [$($acc:tt)*]; $(#[$m:meta])* $name:ident : unlisted twin: $kind:ident($what:tt) $(, $($rest:tt)*)?) => {
        catalog_twins!(@heap [$($acc)* (stringify!($name), $crate::c_twin!($kind($what))),]; $($($rest)*)?)
    };
    (@heap [$($acc:tt)*]; $(#[$m:meta])* $name:ident : uses($($u:tt)*) $(x $n:ident)? twin: $kind:ident($what:tt)
        $(= $pat:pat $(=> $off:expr)?)? $(, $($rest:tt)*)?) => {
        catalog_twins!(@heap [$($acc)* (stringify!($name), $crate::c_twin!($kind($what))),]; $($($rest)*)?)
    };
    (@number [$($acc:tt)*];) => { &[$($acc)*] };
    (@number [$($acc:tt)*]; $(#[$m:meta])* $name:ident twin: $kind:ident($what:tt) $(, $($rest:tt)*)?) => {
        catalog_twins!(@number [$($acc)* (stringify!($name), $crate::c_twin!($kind($what))),]; $($($rest)*)?)
    };
}

catalog!(catalog_twins());
