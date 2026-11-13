//! The engine-internal binding names compiled code loads by name, and the natives the
//! intrinsic ones stand for.

pub(crate) const INTRINSIC_REGEXP_BINDING: &str = "\0quench:intrinsic-regexp";
/// Internal global bindings, which compiled code loads by name and no source text can
/// declare: iterator closing and `with` scope exit (`WITH_ENTER_BINDING` enters).
pub(crate) const ITERATOR_CLOSE_BINDING: &str = "\0quench:iterator-close";
/// The intrinsic a `super` access reads its home's parent with (`Native::ReflectGetPrototypeOf`).
pub(crate) const SUPER_BASE_BINDING: &str = "\0quench:super-base";
pub(crate) const WITH_EXIT_BINDING: &str = "\0quench:with-exit";
/// Every internal global the realm installs on its global object.
pub(crate) const INTERNAL_GLOBAL_BINDINGS: [&str; 3] = [
    ITERATOR_CLOSE_BINDING,
    WITH_ENTER_BINDING,
    WITH_EXIT_BINDING,
];

/// The native an engine-internal intrinsic name stands for: a by-name read of it resolves to
/// that native before consulting any binding (`Vm::intrinsic_name_value`), and no source
/// identifier can spell it, so the read observes no scope.
pub(crate) fn intrinsic_name_native(name: &str) -> Option<crate::heap::Native> {
    use crate::heap::Native;
    Some(match name {
        "\0quench:dynamic-import" => Native::DynamicImport,
        "\0quench:intrinsic-promise" => Native::Promise,
        "\0quench:super-get" => Native::ReflectGet,
        "\0quench:super-set" => Native::SuperSet,
        SUPER_BASE_BINDING => Native::ReflectGetPrototypeOf,
        "\0quench:object-literal-prototype" => Native::ObjectLiteralPrototype,
        "\0quench:object-literal-create" => Native::ObjectLiteralCreate,
        "\0quench:object-define-property" => Native::ObjectDefineProperty,
        "\0quench:object-freeze" => Native::ObjectFreeze,
        // What the compiler's own throws, class setup and `using` declarations call: the
        // realm's intrinsics, which a binding of the same name in scope does not replace.
        INTRINSIC_TYPE_ERROR_BINDING => Native::TypeError,
        INTRINSIC_REFERENCE_ERROR_BINDING => Native::ReferenceError,
        INTRINSIC_SET_PROTOTYPE_OF_BINDING => Native::ObjectSetPrototypeOf,
        INTRINSIC_DISPOSABLE_STACK_BINDING => Native::DisposableStack,
        // The iteration helpers each realm also installs on its global object.
        ITERATOR_CLOSE_BINDING => Native::IteratorClose,
        _ => return None,
    })
}

/// Whether a by-name read of `name` is an intrinsic's (`intrinsic_name_native` or the RegExp
/// intrinsic), which reads no binding.
pub(crate) fn is_intrinsic_name(name: &str) -> bool {
    name == INTRINSIC_REGEXP_BINDING || intrinsic_name_native(name).is_some()
}
pub(crate) const INTRINSIC_TYPE_ERROR_BINDING: &str = "\0quench:intrinsic-type-error";
pub(crate) const INTRINSIC_REFERENCE_ERROR_BINDING: &str = "\0quench:intrinsic-reference-error";
pub(crate) const INTRINSIC_SET_PROTOTYPE_OF_BINDING: &str = "\0quench:intrinsic-set-prototype-of";
pub(crate) const INTRINSIC_DISPOSABLE_STACK_BINDING: &str = "\0quench:intrinsic-disposable-stack";
/// The engine-internal global a `with` statement calls to enter its object scope.
pub(crate) const WITH_ENTER_BINDING: &str = "\0quench:with-enter";
