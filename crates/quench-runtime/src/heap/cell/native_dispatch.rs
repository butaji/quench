//! The dispatch classes of a native: lane and constructor predicates, typed array, Atomics, BigInt and promise natives.

use super::*;

impl Native {
    /// Natives a lane `Call` makes in place from the running frame (`lane_call_native`),
    /// as the general loop's native branch makes them once lane activations are frames:
    /// every native but those whose call the general loop takes as its own step. The
    /// call-forwarding natives push their target as a frame there (and run under a
    /// forwarding boundary), direct `eval` reads the calling instruction, and `with` entry
    /// and exit change the running frame's object environment.
    pub(super) const fn derive_runs_from_lane(self) -> bool {
        !matches!(
            self,
            Self::FunctionCall
                | Self::FunctionApply
                | Self::FunctionBoundCall
                | Self::ReflectApply
                | Self::ReflectConstruct
                | Self::Eval
                | Self::WithEnter
                | Self::WithExit
        )
    }

    pub(super) const fn derive_host_control_native(self) -> bool {
        matches!(
            self,
            Self::HostDone | Self::CreateRealm | Self::Boolean | Self::BigInt
        ) || self.is_function_native()
    }

    /// Natives a lane `new` constructs in place from the running frame
    /// (`lane_construct_native`): the keyed collections, `Date`, `Promise`, `Object`,
    /// `RegExp`, the buffers, typed arrays and errors and the primitive wrappers and `Proxy`, whose
    /// [[Construct]] reads no caller state beyond the frame's published PC and runs guest
    /// code only through ordinary nested calls.
    pub fn constructs_from_lane(self) -> bool {
        self.is_typed_array_constructor()
            || self.is_error_constructor()
            || matches!(
                self,
                Self::Map
                    | Self::Set
                    | Self::WeakMap
                    | Self::WeakSet
                    | Self::Date
                    | Self::Promise
                    | Self::Object
                    | Self::RegExp
                    | Self::ArrayBuffer
                    | Self::DataView
                    | Self::Boolean
                    | Self::Number
                    | Self::String
                    | Self::Proxy
            )
    }

    /// Whether its [[Construct]] makes the object from the newTarget it is passed
    /// (GetPrototypeFromConstructor), so the construction leaves its prototype as made.
    pub(crate) fn constructs_from_new_target(self) -> bool {
        self.is_typed_array_constructor()
            || self.is_error_constructor()
            || matches!(
                self,
                Self::Array
                    | Self::ArrayBuffer
                    | Self::SharedArrayBuffer
                    | Self::RegExp
                    | Self::Date
                    | Self::Map
                    | Self::Set
                    | Self::WeakMap
                    | Self::WeakSet
                    | Self::WeakRef
                    | Self::FinalizationRegistry
                    | Self::DisposableStack
                    | Self::AsyncDisposableStack
                    | Self::ShadowRealm
                    | Self::TemporalPlainDate
                    | Self::TemporalPlainDateTime
                    | Self::TemporalPlainMonthDay
                    | Self::TemporalPlainYearMonth
                    | Self::TemporalZonedDateTime
                    | Self::TemporalInstant
            )
    }

    pub fn is_error_constructor(self) -> bool {
        matches!(
            self,
            Self::Error
                | Self::AggregateError
                | Self::SuppressedError
                | Self::EvalError
                | Self::RangeError
                | Self::ReferenceError
                | Self::SyntaxError
                | Self::TypeError
                | Self::URIError
                | Self::RealmTypeError
                | Self::WasmCompileError
                | Self::WasmLinkError
                | Self::WasmRuntimeError
        )
    }

    pub(crate) fn is_typed_array_constructor(self) -> bool {
        matches!(
            self,
            Self::TypedArray
                | Self::Uint8Array
                | Self::Uint8ClampedArray
                | Self::Uint16Array
                | Self::Uint32Array
                | Self::Int8Array
                | Self::Int16Array
                | Self::Int32Array
                | Self::BigInt64Array
                | Self::BigUint64Array
                | Self::Float16Array
                | Self::Float32Array
                | Self::Float64Array
        )
    }

    pub(crate) fn is_bigint_native(self) -> bool {
        matches!(
            self,
            Self::BigIntValueOf
                | Self::BigIntToString
                | Self::BigIntToLocaleString
                | Self::BigIntAsIntN
                | Self::BigIntAsUintN
        )
    }
}
