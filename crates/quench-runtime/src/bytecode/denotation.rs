/// The guarded JavaScript denotation available to the shared compiled region
/// analyzer. This describes an instruction operation, never a source pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JsDenotation {
    Unsupported,
    Noop,
    Jump,
    LoadConst,
    Move,
    GetField,
    FieldStore,
    NumericBinary,
    NumericUnary,
    /// ToString of each register of a window, concatenated in order.
    StringConcatenation,
    CapturedValue,
    PlainLocalLoad,
    PlainLocalStore,
    Return,
    /// A fresh ordinary object with the realm's `Object.prototype` (`Vm::object`).
    ObjectAllocation,
    /// A literal's own data property definition, by name, through its cache site.
    FieldDefinition,
}
