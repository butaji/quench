//! The per-native facts the closed world reads (C5 C2c): what a native does with its arguments and
//! its receiver, decided here once for every native. `reachable_protocol` opens the protocol gates a
//! native's facts name; a native the closure never calls opens none of them.
//!
//! `coerces_args` is true for every native except the runtime's own promise and generator
//! resumption, which pass values without converting them. A native that is not listed is treated as
//! coercing (sound by default).

use super::*;

/// What a native does, as the closed world needs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct NativeEffects {
    /// Converts an argument or its receiver to a primitive (`ToPrimitive`, `ToString`, `ToNumber`).
    pub(super) coerces_args: bool,
    /// Runs the iteration protocol over an argument or a step of an iterator (`next`, `return`, `throw`).
    pub(super) iterates: bool,
    /// Constructs its receiver or a species, so reads a constructor's `prototype`.
    pub(super) constructs: bool,
    /// Resolves or chains a promise, so reads a value's `then`.
    pub(super) resolves: bool,
    /// Reads the clock or the random source (a build-time input, `clock::note_input`): the input
    /// edge captures the caller chain at its entry (`Vm::capture_input_native`).
    pub(super) reads_input: bool,
}

/// The effects of `native`. Each fact is its own list: a native may have several.
pub(super) fn native_effects(native: Native) -> NativeEffects {
    NativeEffects {
        coerces_args: !matches!(
            native,
            Native::PromiseResolve | Native::PromiseReject | Native::PromiseAggregateJob |
            Native::PromiseCapabilityExecutor | Native::PromiseFinallyHandler |
            Native::PromiseFinallyContinuationHandler | Native::AsyncFromSyncValue |
            Native::AsyncFromSyncValueRejected | Native::AsyncGeneratorReturnFulfilled |
            Native::AsyncGeneratorReturnRejected | Native::AsyncGeneratorDelegateFulfilled |
            Native::AsyncGeneratorDelegateRejected | Native::GeneratorNext | Native::GeneratorReturn |
            Native::GeneratorThrow | Native::AsyncGeneratorNext | Native::AsyncGeneratorReturn |
            Native::AsyncGeneratorThrow
        ),
        iterates: matches!(
            native,
            Native::ObjectFromEntries | Native::ObjectGroupBy | Native::TypedArrayFrom |
            Native::ArrayFromAsync | Native::ArrayFromAsyncFulfilled | Native::ArrayFromAsyncRejected |
            Native::ArrayFrom | Native::Map | Native::MapGroupBy | Native::Set | Native::SetIsDisjointFrom |
            Native::SetIsSubsetOf | Native::SetIsSupersetOf | Native::Iterator | Native::IteratorFrom |
            Native::IteratorConcat | Native::IteratorZip | Native::IteratorZipKeyed | Native::IteratorMap |
            Native::IteratorFilter | Native::IteratorTake | Native::IteratorDrop | Native::IteratorFlatMap |
            Native::IteratorReduce | Native::IteratorToArray | Native::IteratorForEach |
            Native::IteratorEvery | Native::IteratorFind | Native::IteratorSome | Native::IteratorDispose |
            Native::IteratorProtocolNext | Native::IteratorProtocolReturn | Native::IteratorHelperNext |
            Native::IteratorHelperReturn | Native::IteratorPrototypeConstructorGetter |
            Native::IteratorPrototypeConstructorSetter | Native::IteratorPrototypeToStringTagGetter |
            Native::IteratorPrototypeToStringTagSetter | Native::IteratorNext | Native::IteratorClose |
            Native::IteratorSelf | Native::IteratorReturn | Native::IteratorThrow | Native::WeakMap |
            Native::WeakSet | Native::PromiseAll | Native::PromiseAllKeyed | Native::PromiseRace |
            Native::PromiseAllSettled | Native::PromiseAllSettledKeyed | Native::PromiseAny |
            Native::AsyncFromSyncValue | Native::AsyncFromSyncValueRejected
        ),
        constructs: matches!(
            native,
            Native::ReflectGet | Native::ReflectHas | Native::ReflectApply |
            Native::ReflectGetOwnPropertyDescriptor | Native::ReflectDefineProperty |
            Native::ReflectDeleteProperty | Native::ReflectPreventExtensions | Native::ReflectIsExtensible |
            Native::ReflectSet | Native::ReflectOwnKeys | Native::ReflectGetPrototypeOf |
            Native::ReflectSetPrototypeOf | Native::ReflectConstruct | Native::Array | Native::TypedArray |
            Native::TypedArrayFrom | Native::TypedArrayOf | Native::TypedArrayAt |
            Native::TypedArrayLastIndexOf | Native::TypedArraySort | Native::TypedArrayToReversed |
            Native::TypedArrayToSorted | Native::TypedArrayWith | Native::TypedArrayToLocaleString |
            Native::TypedArrayBufferGetter | Native::TypedArrayByteLengthGetter |
            Native::TypedArrayByteOffsetGetter | Native::TypedArrayLengthGetter |
            Native::TypedArrayToStringTag | Native::ArrayToLocaleString | Native::ArraySpecies |
            Native::ArrayFromAsync | Native::ArrayFromAsyncFulfilled | Native::ArrayFromAsyncRejected |
            Native::ArrayIsArray | Native::ArrayPush | Native::ArrayPop | Native::ArraySlice |
            Native::ArrayIncludes | Native::ArrayJoin | Native::ArrayConcat | Native::ArrayFlat |
            Native::ArrayReverse | Native::ArrayShift | Native::ArrayUnshift | Native::ArraySplice |
            Native::ArrayFill | Native::ArrayAt | Native::ArrayLastIndexOf | Native::ArrayIndexOf |
            Native::ArrayCopyWithin | Native::ArrayWith | Native::ArrayForEach | Native::ArrayMap |
            Native::ArrayFilter | Native::ArraySome | Native::ArrayEvery | Native::ArrayFind |
            Native::ArrayFindIndex | Native::ArrayFindLast | Native::ArrayFindLastIndex | Native::ArrayGroup |
            Native::ArrayGroupToMap | Native::ArrayFlatMap | Native::ArrayReduce | Native::ArrayReduceRight |
            Native::TypedArrayForEach | Native::TypedArrayMap | Native::TypedArrayFilter |
            Native::TypedArraySome | Native::TypedArrayEvery | Native::TypedArrayFind |
            Native::TypedArrayFindIndex | Native::TypedArrayFindLast | Native::TypedArrayFindLastIndex |
            Native::TypedArrayReduce | Native::TypedArrayReduceRight | Native::ArrayToReversed |
            Native::ArrayToSpliced | Native::ArraySort | Native::ArrayToSorted | Native::ArrayToString |
            Native::ArrayKeys | Native::ArrayValues | Native::ArrayEntries | Native::ArrayFrom |
            Native::ArrayOf | Native::ArrayBuffer | Native::ArrayBufferSpecies | Native::ArrayBufferSlice |
            Native::ArrayBufferTransfer | Native::ArrayBufferResize |
            Native::ArrayBufferTransferToFixedLength | Native::ArrayBufferTransferToImmutable |
            Native::ArrayBufferSliceToImmutable | Native::ArrayBufferByteLengthGetter |
            Native::ArrayBufferDetachedGetter | Native::ArrayBufferImmutableGetter |
            Native::ArrayBufferMaxByteLengthGetter | Native::ArrayBufferResizableGetter |
            Native::SharedArrayBufferByteLengthGetter | Native::SharedArrayBufferGrowableGetter |
            Native::SharedArrayBufferMaxByteLengthGetter | Native::ArrayBufferIsView |
            Native::ArrayIteratorNext | Native::SharedArrayBuffer | Native::SharedArrayBufferGrow |
            Native::SharedArrayBufferSlice | Native::RegExpStringIteratorNext | Native::RegExp |
            Native::RegExpCompile | Native::RegExpEscape | Native::RegExpLegacyGetter |
            Native::RegExpLegacySetter | Native::RegExpToString | Native::RegExpSymbolMatch |
            Native::RegExpSymbolSearch | Native::RegExpSymbolReplace | Native::RegExpSymbolMatchAll |
            Native::RegExpSymbolSplit | Native::RegExpSpecies | Native::RegExpExec | Native::RegExpTest |
            Native::RegExpGlobal | Native::RegExpIgnoreCase | Native::RegExpMultiline | Native::RegExpDotAll |
            Native::RegExpUnicode | Native::RegExpUnicodeSets | Native::RegExpSticky |
            Native::RegExpHasIndices | Native::RegExpSource | Native::RegExpFlags | Native::Promise |
            Native::PromiseSpeciesGetter | Native::PromiseResolve | Native::PromiseReject |
            Native::PromiseTry | Native::PromiseWithResolvers | Native::PromiseCapabilityExecutor |
            Native::PromiseThen | Native::PromiseCatch | Native::PromiseFinally |
            Native::PromiseFinallyHandler | Native::PromiseFinallyContinuationHandler | Native::PromiseAll |
            Native::PromiseAllKeyed | Native::PromiseRace | Native::PromiseAllSettled |
            Native::PromiseAllSettledKeyed | Native::PromiseAny | Native::PromiseAggregateJob
        ),
        reads_input: matches!(native, Native::Date | Native::DateNow | Native::MathRandom),
        resolves: matches!(
            native,
            Native::AsyncIteratorSelf | Native::AsyncGeneratorNext | Native::AsyncGeneratorReturn |
            Native::AsyncGeneratorThrow | Native::AsyncGeneratorReturnFulfilled |
            Native::AsyncGeneratorReturnRejected | Native::AsyncIteratorDispose |
            Native::AsyncIteratorDisposeFulfilled | Native::AsyncDisposableStack |
            Native::AsyncDisposableStackUse | Native::AsyncDisposableStackAdopt |
            Native::AsyncDisposableStackDefer | Native::AsyncDisposableStackMove |
            Native::AsyncDisposableStackDisposeAsync | Native::AsyncDisposableStackDisposed |
            Native::AsyncFunction | Native::AsyncGeneratorFunction |
            Native::AsyncGeneratorDelegateReturnStart | Native::Promise | Native::PromiseSpeciesGetter |
            Native::PromiseResolve | Native::PromiseReject | Native::PromiseTry |
            Native::PromiseWithResolvers | Native::PromiseCapabilityExecutor | Native::PromiseThen |
            Native::PromiseCatch | Native::PromiseFinally | Native::PromiseFinallyHandler |
            Native::PromiseFinallyContinuationHandler | Native::PromiseAll | Native::PromiseAllKeyed |
            Native::PromiseRace | Native::PromiseAllSettled | Native::PromiseAllSettledKeyed |
            Native::PromiseAny | Native::PromiseAggregateJob | Native::AsyncFromSyncValue |
            Native::AsyncFromSyncValueRejected | Native::AsyncGeneratorDelegateFulfilled |
            Native::AsyncGeneratorDelegateRejected
        ),
    }
}
