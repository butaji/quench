//! The natives every tier runs on a staged row, one line each: the row, its stencil symbol and the
//! arguments it reads. This file depends on nothing, so the b-jit's planner (`quench_bjit::call`)
//! and its stencils read the same table the runtime does (`staged::native_row`); which `Native` a
//! row serves is stated beside it there (`native_row::native_row_of`). A new row is one line here, its arm of
//! `native_row_of` and its body in `staged::native_row::step_native_row`.

/// Declares `NativeRow` from `Variant = "symbol", stencil, arguments;` lines, with `ALL`, `symbol` and `arguments`.
macro_rules! native_rows {
    ($( $(#[$doc:meta])* $row:ident = $symbol:literal, $stencil:ident, $arguments:literal; )*) => {
        /// A native run on its staged row by every tier.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum NativeRow {
            $( $(#[$doc])* $row, )*
        }

        impl NativeRow {
            /// Every row, each with its own stencil.
            pub const ALL: &'static [NativeRow] = &[$(NativeRow::$row),*];

            /// The row's name in the stencil object, after `builtin_`.
            pub const fn symbol(self) -> &'static str {
                match self { $(NativeRow::$row => $symbol,)* }
            }

            /// The arguments the row reads after its receiver: a call passing another count takes the native.
            pub const fn arguments(self) -> u16 {
                match self { $(NativeRow::$row => $arguments,)* }
            }
        }
    };
}

/// Hands every row line to the macro `$then`: `NativeRow` here (`native_rows`), and the b-jit's
/// stencils, one per row (`stencils/builtin_rows.rs`), so a row is stated once.
#[allow(unused_macros)]
macro_rules! with_native_rows {
    ($then:ident) => {
        $then! {
            /// `String.prototype.charCodeAt` of a flat string.
            CharCodeAt = "char_code_at", builtin_char_code_at, 1;
            /// `String.prototype.charAt` of a flat string, at an index of an ASCII unit.
            CharAt = "char_at", builtin_char_at, 1;
            /// `Number.isInteger` of a Number.
            IsInteger = "is_integer", builtin_is_integer, 1;
            /// `Math.imul` of two int32 operands.
            Imul = "imul", builtin_imul, 2;
            /// `Array.prototype.push` of one value onto a dense array (`staged::builtins::array_push_one`).
            Push = "push", builtin_push, 1;
            /// `Array.prototype.pop` of a dense array (`staged::builtins::array_pop`).
            Pop = "pop", builtin_pop, 0;
        }
    };
}

with_native_rows!(native_rows);
#[allow(unused_imports)]
pub(crate) use with_native_rows;
