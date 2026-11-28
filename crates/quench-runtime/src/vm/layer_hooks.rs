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
            #[allow(unused_imports)]
            use super::*;
            $(#[allow(non_camel_case_types)] pub type $name = fn($($ty),*) -> $ret;)*
        }
    };
}

/// The Intl and Temporal entries, called through the layer the binary installs
/// (`install_intl`), so an image whose program cannot reach Intl or Temporal links none of the
/// layer: an entry it never installed throws.
macro_rules! intl_hooks {
    ($($(#[$meta:meta])* fn $name:ident => $symbol:ident($vm:ident: $vm_ty:ty, $p:ident: $p_ty:ty $(, $arg:ident: $ty:ty)* $(,)?) -> Result<$ok:ty, JsError>;)*) => {
        unsafe extern "Rust" {
            $(safe fn $symbol($vm: $vm_ty, $p: $p_ty $(, $arg: $ty)*) -> Result<$ok, JsError>;)*
        }
        /// The layer's entries, by name.
        struct IntlLayer {
            $($name: fn($vm_ty, $p_ty $(, $ty)*) -> Result<$ok, JsError>,)*
        }
        static INTL: std::sync::OnceLock<IntlLayer> = std::sync::OnceLock::new();
        /// Links the Intl and Temporal layer into this binary and makes its entries callable;
        /// the first installation holds.
        pub fn install_intl() {
            INTL.get_or_init(|| IntlLayer { $($name: $symbol,)* });
        }
        $(
            $(#[$meta])*
            #[inline]
            pub fn $name($vm: $vm_ty, $p: $p_ty $(, $arg: $ty)*) -> Result<$ok, JsError> {
                match INTL.get() {
                    Some(layer) => (layer.$name)($vm, $p $(, $arg)*),
                    None => Err($vm.type_error($p, "Intl and Temporal are not linked into this image".into())),
                }
            }
        )*
        /// The type of each Intl entry, which its definition is checked against.
        pub mod intl_signatures {
            use super::*;
            $(#[allow(non_camel_case_types)] pub type $name = fn($vm_ty, $p_ty $(, $ty)*) -> Result<$ok, JsError>;)*
        }
    };
}

layer_hooks! {
    /// Installs the JIT's compile routes (the engine's `jit_routes`) and every lane handler, as
    /// `Vm::new` makes a runtime: a VM only restored at rest (`Vm::new_at_rest`) names neither.
    fn enable_jit => quench_layer_enable_jit() -> ();
    /// The creation edge of body `body`'s one closure in `program`, once its closure cell is written (`Vm::note_closure_created`):
    /// the layer binds the proven calls of the image that fold that body's closure.
    fn closure_created => quench_layer_closure_created(vm: &mut Vm, program: ProgramId, body: u32) -> ();
    /// Resolves the planned arms of `class` (a class id: a constructor's function id or a synthetic class) in `program` once its header is recorded:
    /// `prototype` is the prototype its instances hold, which holds the arms' methods. Called after the header is set,
    /// so a planned row that reads the header reads a target already resolved. The class-header edge makes the call
    /// for a construction and for a restored snapshot alike (`Vm::record_class_edge`).
    fn planned_class_recorded => quench_layer_planned_class_recorded(vm: &mut Vm, program: ProgramId, class: u32, prototype: Value) -> ();
    /// The entry of an Intl or Temporal native (`None` for any other native), which the core's
    /// `native_table` holds. A build without the layer claims none.
    fn intl_entry => quench_layer_intl_entry(native: Native) -> Option<native_table::NativeFn>;
}

intl_hooks! {
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
