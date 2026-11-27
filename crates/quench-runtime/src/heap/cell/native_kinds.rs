//! The kinds of a native: its predicates and the traits each derives from its defining predicate.

use super::*;

impl Native {
    /// The constructor intrinsic (`Function`, `AsyncFunction`, ...) of a function of this kind.
    pub(crate) fn function_constructor(is_async: bool, is_generator: bool) -> Self {
        match (is_async, is_generator) {
            (true, true) => Self::AsyncGeneratorFunction,
            (false, true) => Self::GeneratorFunction,
            (true, false) => Self::AsyncFunction,
            (false, false) => Self::Function,
        }
    }

    /// `derive_intrinsic_species_getter`: a `get [Symbol.species]` accessor that returns its receiver.
    #[inline(always)]
    pub(crate) const fn is_intrinsic_species_getter(self) -> bool {
        self.derive_intrinsic_species_getter()
    }

    /// The `get [Symbol.species]` accessors, each of which returns its receiver.
    pub(super) const fn derive_intrinsic_species_getter(self) -> bool {
        matches!(
            self,
            Self::ArraySpecies
                | Self::ArrayBufferSpecies
                | Self::PromiseSpeciesGetter
                | Self::RegExpSpecies
                | Self::SetSpeciesGetter
        )
    }

    pub(crate) const fn is_function_native(self) -> bool {
        matches!(
            self,
            Self::Function
                | Self::AsyncFunction
                | Self::GeneratorFunction
                | Self::AsyncGeneratorFunction
        )
    }

    /// The natives whose searches run a RegExp's matcher, after which a search that ran out of
    /// backtracking stack raises (`Vm::regexp_search_outcome`).
    pub(crate) const fn searches_regexps(self) -> bool {
        matches!(
            self,
            Self::RegExpExec
                | Self::RegExpTest
                | Self::RegExpSymbolMatch
                | Self::RegExpSymbolSearch
                | Self::RegExpSymbolReplace
                | Self::RegExpSymbolMatchAll
                | Self::RegExpSymbolSplit
                | Self::StringReplace
                | Self::StringReplaceAll
                | Self::StringMatch
                | Self::StringMatchAll
                | Self::StringSearch
                | Self::StringSplit
                | Self::RegExpStringIteratorNext
        )
    }

    /// The functions of the `Math` object (`MATH_FUNCTIONS`, which its test keeps equal).
    pub(super) const fn derive_math_function(self) -> bool {
        matches!(
            self,
            Self::MathAbs
                | Self::MathAcos
                | Self::MathAcosh
                | Self::MathAsin
                | Self::MathAsinh
                | Self::MathAtan
                | Self::MathAtan2
                | Self::MathAtanh
                | Self::MathCbrt
                | Self::MathCeil
                | Self::MathClz32
                | Self::MathCos
                | Self::MathCosh
                | Self::MathExp
                | Self::MathExpm1
                | Self::MathF16Round
                | Self::MathFloor
                | Self::MathFround
                | Self::MathHypot
                | Self::MathImul
                | Self::MathLog
                | Self::MathLog10
                | Self::MathLog1p
                | Self::MathLog2
                | Self::MathMax
                | Self::MathMin
                | Self::MathPow
                | Self::MathRandom
                | Self::MathRound
                | Self::MathSign
                | Self::MathSin
                | Self::MathSinh
                | Self::MathSqrt
                | Self::MathSumPrecise
                | Self::MathTan
                | Self::MathTanh
                | Self::MathTrunc
        )
    }

    /// The string functions, which `call_native_table` dispatches directly.
    pub(crate) fn is_string_regexp_or_math(self) -> bool {
        self.is_pure_string_method()
            || matches!(
                self,
                Self::StringReplace
                    | Self::StringReplaceAll
                    | Self::StringMatch
                    | Self::StringMatchAll
                    | Self::StringSearch
            )
    }

    /// The `Reflect` functions that run an internal method of their target (`Reflect.apply`,
    /// `construct` and the like call functions, and take frames).
    pub(super) const fn is_reflect_operation(self) -> bool {
        matches!(
            self,
            Self::ReflectGetPrototypeOf
                | Self::ReflectSetPrototypeOf
                | Self::ReflectGet
                | Self::ReflectSet
                | Self::ReflectHas
                | Self::ReflectDefineProperty
                | Self::ReflectGetOwnPropertyDescriptor
                | Self::ReflectDeleteProperty
                | Self::ReflectOwnKeys
        )
    }

    /// `Array.prototype.forEach` and the other array methods that call a callback per index
    /// (`Vm::array_callback_native`), the reducers (`Native::is_array_reducer`) among them.
    pub fn is_array_callback(self) -> bool {
        matches!(
            self,
            Self::ArrayForEach
                | Self::ArrayMap
                | Self::ArrayFilter
                | Self::ArraySome
                | Self::ArrayEvery
                | Self::ArrayFind
                | Self::ArrayFindIndex
                | Self::ArrayFindLast
                | Self::ArrayFindLastIndex | Self::ArrayReduce | Self::ArrayReduceRight
        )
    }

    /// Whether the native observes the frame stack only by calling guest code, capturing an
    /// error's stack or making a collection due (`Host::host_function_runs_beside_activations`
    /// for a host's), so a lane calls it with its activations frameless. Each of these reads
    /// its arguments and the heap, and reaches guest code only through getters, coercions,
    /// proxy traps, species constructors or a `JSON.parse` reviver, whose calls make the activations frames first
    /// (`Vm::materialize_deferred_lane`): `assert.deepEqual` calls `Object.prototype.toString`
    /// on every value it compares (10,210 materializations in the color-convert npm scenario),
    /// and TypeScript's CommonJS re-export helper (`__createBinding`) reads each export's
    /// descriptor (208,883 in the typescript-eslint npm scenario), and Express decodes each
    /// query value with `decodeURIComponent` (one per request). A Wasm export converts its
    /// arguments with coercions, reaches JavaScript only through its imports' calls and runs
    /// its own code natively, or pushes a frame for it only after making the activations
    /// frames (`Vm::call_wasm_exported_function`); its trap becomes an error object where the
    /// general path raises it (JetStream richards-wasm calls an export 2.1 million times an
    /// iteration from a frameless activation). A keyed collection's or array's iterator
    /// creation, `%IteratorPrototype%[@@iterator]`, `concat`, `Array.from` and `Object.isExtensible` belong
    /// too (MobX's observable set reads `values()` 4,087 times per three proxy-mobx iterations).
    pub(super) const fn derive_runs_beside_activations(self) -> bool {
        self.is_reflect_operation() || matches!(
            self,
            Self::ObjectLiteralPrototype
                | Self::ObjectLiteralCreate
                | Self::ObjectKeys
                | Self::ObjectAssign
                | Self::ObjectDefineProperty
                | Self::ObjectGetPrototypeOf
                | Self::ObjectSetPrototypeOf
                | Self::ObjectEntries
                | Self::ObjectGetOwnPropertyNames
                | Self::ObjectHasOwn
                | Self::ObjectGetOwnPropertyDescriptor
                | Self::ObjectPrototypeHasOwnProperty
                | Self::ObjectPrototypeToString
                | Self::ArrayJoin
                | Self::ArraySplice
                | Self::StringSplit
                | Self::StringMatch
                | Self::StringReplace
                | Self::StringReplaceAll
                | Self::NumberString
                | Self::MapEntries
                | Self::MapKeys
                | Self::MapValues
                | Self::SetValues
                | Self::SetKeys
                | Self::SetEntries
                | Self::IteratorSelf
                | Self::AsyncGeneratorNext
                | Self::ArrayConcat
                | Self::ArrayFrom
                | Self::ObjectIsExtensible
                | Self::ArrayReverse
                | Self::MathRandom
                | Self::ArraySlice
                | Self::ArrayMap
                | Self::ArrayForEach
                | Self::ArrayFilter
                | Self::ArraySome
                | Self::ArrayEvery
                | Self::ArrayFind
                | Self::ArrayFindIndex
                | Self::ArrayFindLast
                | Self::ArrayFindLastIndex
                | Self::ArrayReduce
                | Self::ArrayReduceRight
                | Self::ArrayFlatMap
                | Self::ArraySort
                | Self::MapForEach
                | Self::SetForEach
                | Self::RegExpTest
                | Self::JsonParse
                | Self::JsonStringify
                | Self::ParseInt
                | Self::EncodeUri
                | Self::EncodeUriComponent
                | Self::DecodeUri
                | Self::DecodeUriComponent
                | Self::ArrayShift
                | Self::WeakMapSet
                | Self::FinalizationRegistryRegister
                | Self::TypedArrayToStringTag
                | Self::TypedArrayBufferGetter
                | Self::TypedArrayByteLengthGetter
                | Self::TypedArrayByteOffsetGetter
                | Self::TypedArrayLengthGetter
                | Self::FunctionPrototypeHasInstance
                | Self::FunctionBind
                | Self::Array
                | Self::PromiseResolve
                | Self::PromiseWithResolvers
                | Self::WasmExportedFunction
                // A stack capture reads the activations from the lane's frame walk
                // (`Vm::lane_stack_sites`, frame-switches.md §5): chai's AssertionError calls
                // `Error.captureStackTrace` on every failure. An error constructor called
                // without `new` constructs (`Vm::call_native`'s error arm).
                | Self::ErrorCaptureStackTrace
                | Self::Error
                | Self::EvalError
                | Self::RangeError
                | Self::ReferenceError
                | Self::SyntaxError
                | Self::TypeError
                | Self::URIError
        )
    }

    /// Whether the native observes the frame stack only by calling guest code, capturing an
    /// error's stack or making a collection due (`Host::host_function_runs_beside_activations`
    /// for a host's), so a lane calls it with its activations frameless. Each of these reads
    /// its arguments and the heap, and reaches guest code only through getters, coercions,
    /// proxy traps, species constructors or a `JSON.parse` reviver, whose calls make the activations frames first
    /// (`Vm::materialize_deferred_lane`): `assert.deepEqual` calls `Object.prototype.toString`
    /// on every value it compares (10,210 materializations in the color-convert npm scenario),
    /// and TypeScript's CommonJS re-export helper (`__createBinding`) reads each export's
    /// descriptor (208,883 in the typescript-eslint npm scenario), and Express decodes each
    /// query value with `decodeURIComponent` (one per request). A Wasm export converts its
    /// arguments with coercions, reaches JavaScript only through its imports' calls and runs
    /// its own code natively, or pushes a frame for it only after making the activations
    /// frames (`Vm::call_wasm_exported_function`); its trap becomes an error object where the
    /// general path raises it (JetStream richards-wasm calls an export 2.1 million times an
    /// iteration from a frameless activation). A keyed collection's or array's iterator
    /// creation, `%IteratorPrototype%[@@iterator]`, `concat`, `Array.from` and `Object.isExtensible` belong
    /// too (MobX's observable set reads `values()` 4,087 times per three proxy-mobx iterations).
    /// `BigInt.asIntN` / `asUintN` coerce only (the data-structure suite's hash map calls them
    /// eight times per lookup).
    pub fn runs_beside_activations(self) -> bool {
        self.is_reflect_operation() || matches!(
            self,
            Self::ObjectLiteralPrototype
                | Self::ObjectLiteralCreate
                | Self::ObjectKeys
                | Self::ObjectAssign
                | Self::ObjectDefineProperty
                | Self::ObjectGetPrototypeOf
                | Self::ObjectSetPrototypeOf
                | Self::ObjectEntries
                | Self::ObjectGetOwnPropertyNames
                | Self::ObjectHasOwn
                | Self::ObjectGetOwnPropertyDescriptor
                | Self::ObjectPrototypeHasOwnProperty
                | Self::ObjectPrototypeToString
                | Self::ArrayJoin
                | Self::ArraySplice
                | Self::StringSplit
                | Self::StringMatch
                | Self::StringReplace
                | Self::StringReplaceAll
                | Self::NumberString
                | Self::MapEntries
                | Self::MapKeys
                | Self::MapValues
                | Self::SetValues
                | Self::SetKeys
                | Self::SetEntries
                | Self::IteratorSelf
                | Self::AsyncGeneratorNext
                | Self::ArrayConcat
                | Self::ArrayFrom
                | Self::ObjectIsExtensible
                | Self::ArrayReverse
                | Self::MathRandom
                | Self::ArraySlice
                | Self::ArrayMap
                | Self::ArrayForEach
                | Self::ArrayFilter
                | Self::ArraySome
                | Self::ArrayEvery
                | Self::ArrayFind
                | Self::ArrayFindIndex
                | Self::ArrayFindLast
                | Self::ArrayFindLastIndex
                | Self::ArrayReduce
                | Self::ArrayReduceRight
                | Self::ArrayFlatMap
                | Self::ArraySort
                | Self::MapForEach
                | Self::SetForEach
                | Self::RegExpTest
                | Self::JsonParse
                | Self::JsonStringify
                | Self::ParseInt
                | Self::EncodeUri
                | Self::EncodeUriComponent
                | Self::DecodeUri
                | Self::DecodeUriComponent
                | Self::ArrayShift
                | Self::WeakMapSet
                | Self::FinalizationRegistryRegister
                | Self::TypedArrayToStringTag
                | Self::TypedArrayBufferGetter
                | Self::TypedArrayByteLengthGetter
                | Self::TypedArrayByteOffsetGetter
                | Self::TypedArrayLengthGetter
                | Self::FunctionPrototypeHasInstance
                | Self::FunctionBind
                | Self::Array
                | Self::PromiseResolve
                | Self::PromiseWithResolvers
                | Self::WasmExportedFunction
                | Self::BigIntAsIntN
                | Self::BigIntAsUintN
        )
    }

    /// Natives a lane `Call` makes in place from the running frame (`lane_call_native`),
    /// as the general loop's native branch makes them once lane activations are frames:
    /// every native but those whose call the general loop takes as its own step. The
    /// call-forwarding natives push their target as a frame there (and run under a
    /// forwarding boundary), direct `eval` reads the calling instruction, and `with` entry
    /// and exit change the running frame's object environment.
    pub fn runs_from_lane(self) -> bool {
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

    pub(crate) fn is_host_control_native(self) -> bool {
        matches!(
            self,
            Self::HostDone | Self::CreateRealm | Self::Boolean | Self::BigInt
        ) || self.is_function_native()
    }
}
