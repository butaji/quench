//! Calls from the VM core into its layer crates. A layer crate (`quench-runtime-intl`: Intl
//! and Temporal) sits above the core and adds behaviour the core reaches from a few places (the
//! native dispatch, construction, a handful of builtins). Each such entry is declared once here:
//! the core calls it through a link-time symbol that the layer defines with `#[unsafe(no_mangle)]`
//! and checks against `signatures` (`const _: signatures::name = definition;`), so a signature
//! change fails the layer's build instead of linking a mismatched call. The binary that links
//! the core links its layers too (the `quench-runtime` facade).
use super::*;

macro_rules! layer_hooks {
    ($($(#[$meta:meta])* fn $name:ident => $symbol:ident($($arg:ident: $ty:ty),* $(,)?) -> $ret:ty;)*) => {
        unsafe extern "Rust" {
            $(safe fn $symbol($($arg: $ty),*) -> $ret;)*
        }
        $(
            $(#[$meta])*
            #[inline(always)]
            pub fn $name($($arg: $ty),*) -> $ret {
                $symbol($($arg),*)
            }
        )*
        /// The type of each layer entry, which its definition is checked against.
        pub mod signatures {
            use super::*;
            $(#[allow(non_camel_case_types)] pub type $name = fn($($ty),*) -> $ret;)*
        }
    };
}

layer_hooks! {
    /// The Intl and Temporal natives of `call_native_table`.
    fn intl_temporal_call => quench_layer_intl_temporal_call(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError>;
    /// The Intl and Temporal constructors of `construct_native_with_new_target`.
    fn intl_temporal_construct => quench_layer_intl_temporal_construct(vm: &mut Vm, p: &ResidualProgram, native: Native, args: &[Value], new_target: Value) -> Result<Value, JsError>;
    /// `Number.prototype.toLocaleString` and `BigInt.prototype.toLocaleString` with Intl.
    fn intl_format_primitive => quench_layer_intl_format_primitive(vm: &mut Vm, p: &ResidualProgram, value: Value, args: &[Value]) -> Result<Value, JsError>;
    /// Installs `Intl` on a realm's global object.
    fn install_intl_for_realm => quench_layer_install_intl_for_realm(vm: &mut Vm, p: &ResidualProgram, global: Value, object_prototype: Value) -> Result<(), JsError>;
    /// `Date.prototype.toLocale*String`.
    fn date_to_locale_string => quench_layer_date_to_locale_string(vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError>;
    /// The realm's `Temporal.Instant` constructor.
    fn temporal_instant_constructor => quench_layer_temporal_instant_constructor(vm: &mut Vm, p: &ResidualProgram) -> Result<Value, JsError>;
    /// A `Temporal.Instant` at `epoch` nanoseconds.
    fn make_temporal_instant => quench_layer_make_temporal_instant(vm: &mut Vm, p: &ResidualProgram, epoch: i128, new_target: Value) -> Result<Value, JsError>;
    /// CanonicalizeLocaleList.
    fn canonical_locale_list => quench_layer_canonical_locale_list(vm: &mut Vm, p: &ResidualProgram, locales: Option<Value>) -> Result<Vec<String>, JsError>;
    /// The collation `String.prototype.localeCompare` compares with.
    fn locale_compare_collation => quench_layer_locale_compare_collation(vm: &mut Vm, p: &ResidualProgram, locales: Value, options: Value) -> Result<std::rc::Rc<intl_core::Collation>, JsError>;
    /// `%SegmentIteratorPrototype%.next`.
    fn intl_segment_iterator_next => quench_layer_intl_segment_iterator_next(vm: &mut Vm, p: &ResidualProgram, iterator: Value) -> Result<Value, JsError>;
}
