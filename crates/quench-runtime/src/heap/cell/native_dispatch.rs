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

    #[rustfmt::skip]
    pub(crate) fn is_object_static(self) -> bool { matches!(self, Native::Object | Native::ObjectKeys | Native::ObjectValues | Native::ObjectEntries | Native::ObjectGetOwnPropertyNames | Native::ObjectGetOwnPropertySymbols | Native::ObjectGetOwnPropertyDescriptor | Native::ObjectGetOwnPropertyDescriptors | Native::ObjectFromEntries | Native::ObjectIs | Native::ObjectCreate | Native::ObjectAssign | Native::ObjectDefineProperty | Native::ObjectDefineProperties | Native::ObjectGetPrototypeOf | Native::ObjectSetPrototypeOf | Native::ObjectHasOwn | Native::ObjectPreventExtensions | Native::ObjectIsExtensible | Native::ObjectSeal | Native::ObjectIsSealed | Native::ObjectFreeze | Native::ObjectIsFrozen | Native::ObjectGroupBy) }
    pub(crate) fn is_typed_array_method(self) -> bool {
        matches!(
            self,
            Self::Uint8ArraySet
                | Self::Uint8ArrayReverse
                | Self::Uint8ArrayFill
                | Self::Uint8ArrayCopyWithin
                | Self::Uint8ArraySubarray
                | Self::Uint8ArraySlice
                | Self::Uint8ArrayIncludes
                | Self::Uint8ArrayIndexOf
                | Self::Uint8ArrayJoin
                | Self::Uint8ArrayToString
                | Self::TypedArrayForEach
                | Self::TypedArrayMap
                | Self::TypedArrayFilter
                | Self::TypedArraySome
                | Self::TypedArrayEvery
                | Self::TypedArrayFind
                | Self::TypedArrayFindIndex
                | Self::TypedArrayFindLast
                | Self::TypedArrayFindLastIndex
                | Self::TypedArrayReduce
                | Self::TypedArrayReduceRight
                | Self::TypedArrayLastIndexOf
                | Self::TypedArraySort
                | Self::TypedArrayAt
                | Self::TypedArrayToReversed
                | Self::TypedArrayToSorted
                | Self::TypedArrayWith
                | Self::TypedArrayToLocaleString
                | Self::ArrayBufferIsView
                | Self::Uint8ArrayFromBase64
                | Self::Uint8ArrayFromHex
                | Self::Uint8ArraySetFromBase64
                | Self::Uint8ArraySetFromHex
                | Self::Uint8ArrayToBase64
                | Self::Uint8ArrayToHex
        )
    }
    pub(crate) fn is_uint8_array_base64_method(self) -> bool {
        matches!(
            self,
            Self::Uint8ArrayFromBase64
                | Self::Uint8ArrayFromHex
                | Self::Uint8ArraySetFromBase64
                | Self::Uint8ArraySetFromHex
                | Self::Uint8ArrayToBase64
                | Self::Uint8ArrayToHex
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
    pub(crate) fn is_typed_array_iterator(self) -> bool {
        matches!(
            self,
            Self::Uint8ArrayKeys | Self::Uint8ArrayValues | Self::Uint8ArrayEntries
        )
    }
    pub(crate) fn is_atomics_native(self) -> bool {
        matches!(
            self,
            Self::AtomicsLoad
                | Self::AtomicsStore
                | Self::AtomicsAdd
                | Self::AtomicsSub
                | Self::AtomicsAnd
                | Self::AtomicsOr
                | Self::AtomicsXor
                | Self::AtomicsExchange
                | Self::AtomicsCompareExchange
                | Self::AtomicsIsLockFree
                | Self::AtomicsNotify
                | Self::AtomicsWait
                | Self::AtomicsWaitAsync
                | Self::AtomicsPause
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
    pub(crate) fn is_data_view_native(self) -> bool {
        matches!(
            self,
            Self::DataViewGetUint8
                | Self::DataViewSetUint8
                | Self::DataViewGetInt8
                | Self::DataViewSetInt8
                | Self::DataViewGetUint16
                | Self::DataViewSetUint16
                | Self::DataViewGetInt16
                | Self::DataViewSetInt16
                | Self::DataViewGetUint32
                | Self::DataViewSetUint32
                | Self::DataViewGetInt32
                | Self::DataViewSetInt32
                | Self::DataViewGetFloat32
                | Self::DataViewSetFloat32
                | Self::DataViewGetFloat64
                | Self::DataViewSetFloat64
                | Self::DataViewGetFloat16
                | Self::DataViewSetFloat16
                | Self::DataViewGetBigInt64
                | Self::DataViewSetBigInt64
                | Self::DataViewGetBigUint64
                | Self::DataViewSetBigUint64
                | Self::DataViewBufferGetter
                | Self::DataViewByteLengthGetter
                | Self::DataViewByteOffsetGetter
        )
    }
}
