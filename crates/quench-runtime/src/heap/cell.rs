use super::root::WeakHandle;
use crate::staged::record_heads::ObjectHead;
use crate::bytecode::Atom;
use crate::value::{HeapReference, Value};
use crate::value_vec::{
    INLINE_PROPERTY_COUNT, VALUE_VEC_INTEGRITY_BITS, VALUE_VEC_START_BITS, ValueArena, ValueVec,
};
use crate::vm::program_store::ProgramId;
use crate::vm::wtf16::JsString;
use super::{WeakMapEntries, WeakSetEntries};
use super::{ArrayElements, BufferBytes, PromiseCell, array_elements, array_proofs, dependencies, layouts, thin_slice};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

mod await_state;
mod global_values;
mod modes;
mod prototype_values;
mod shape_id;
mod snapshot;
mod typed_array_kind;
mod wasm_fields;
mod array_object;
#[cfg(test)]
mod array_record_tests;
#[cfg(test)]
mod cell_layout_tests;
mod cell_variants;
pub use cell_variants::*;
mod environment_slots;
mod native_dispatch;
mod native_kinds;
mod object_layout;
#[cfg(test)]
mod object_payload_tests;
mod object_state;
mod object_storage;
pub use await_state::{AwaitKind, AwaitRole, AwaitState};
pub use wasm_fields::{TableTarget, TableWords, WasmGcFields, WasmGcRecord, WasmTableElements};
pub use typed_array_kind::{TYPED_ARRAYS, TypedArrayKind, TypedArrayRow};
pub(crate) use typed_array_kind::for_each_typed_array;
pub(crate) use modes::{ElementFacts, HelperPhase, ObjectBrand, ViewLength};
#[allow(unused_imports)]
pub use prototype_values::{PrototypeValueWatch, hold_prototype_values, subscribe_prototype_layouts, subscribe_prototype_values, watch_prototype_values};
#[allow(unused_imports)]
pub use global_values::{GlobalCell, GlobalFact, GlobalLayoutFact, global_layout_fact, global_value_fact, note_global_shadowed, note_global_write, subscribe_global_layout, subscribe_global_value};
#[cfg(any(test, feature = "test-support"))]
#[quench_macros::test_visible]
pub(crate) use global_values::new_table;

/// Changes at the prototype-layout events no single prototype names: a collection frees a
/// prototype whose record may then be reused (`Heap::retire_dead_prototypes`), shape
/// compaction renumbers the shapes proofs name, or a prototype with no subject in a dependency
/// table (a restored heap's, or a copy's) changes its shape, prototype or integrity. A layout
/// change of a watched prototype retires only the proofs whose chain holds it (its subject's
/// `heap::ChainCell`s) and does not advance it. One counter for the process: a change another
/// heap made only sends caches back to the walk that re-proves them.
static PROTOTYPE_LAYOUT_EPOCH: AtomicU64 = AtomicU64::new(0);

/// The current `PROTOTYPE_LAYOUT_EPOCH`.
#[inline(always)]
pub(crate) fn prototype_layout_epoch() -> u64 {
    PROTOTYPE_LAYOUT_EPOCH.load(Ordering::Relaxed)
}

/// Retires every proof made under the current `PROTOTYPE_LAYOUT_EPOCH`, and all code and
/// watches that hold a prototype's value as a constant: a layout change moves what a slot
/// holds. A heap retires its chain cells with it (`Dependencies::retire_cells`) where the
/// event is its own.
pub(crate) fn advance_prototype_layout_epoch() {
    PROTOTYPE_LAYOUT_EPOCH.fetch_add(1, Ordering::Relaxed);
    prototype_values::reset();
}

/// Changes whenever a builtin a fast path assumes may no longer be the intrinsic: an intact
/// fuse pops or must be proved again (`code_dependencies::Fuses`), or a heap forgets its
/// proofs. A memo that found a native intrinsic under one value keeps the answer while the
/// value holds: one compare instead of re-proving the builtin's identity. One counter for the
/// process, like `PROTOTYPE_LAYOUT_EPOCH`; a change another heap made only sends memos back
/// to the proof.
static INTRINSIC_EPOCH: AtomicU64 = AtomicU64::new(0);

/// The current `INTRINSIC_EPOCH`.
#[inline(always)]
pub(crate) fn intrinsic_epoch() -> u64 {
    INTRINSIC_EPOCH.load(Ordering::Relaxed)
}

/// Retires every memo made under the current `INTRINSIC_EPOCH`.
#[cold]
pub(crate) fn note_intrinsic_change() {
    INTRINSIC_EPOCH.fetch_add(1, Ordering::Relaxed);
}

/// Declares `Native`, one entry per builtin function kind, in discriminant order. The order fixes
/// the discriminants that tables index and snapshots serialize, so entries never move. An entry
/// marked `: unshared` is not a shared function object: `Vm::native_value` never builds it.
/// `Native::COUNT` (`heap/cell/snapshot.rs`) is the list's length: `ALL`'s type states it, so a
/// list of another length does not compile.
macro_rules! natives {
    ($( $(#[$meta:meta])* $variant:ident $(: $flag:ident)? ),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[repr(u16)]
        pub enum Native {
            $( $(#[$meta])* $variant, )*
        }

        impl Native {
            /// Every native in discriminant order: `ALL[n as usize]` has discriminant `n`.
            pub const ALL: [Native; Native::COUNT as usize] = [$(Native::$variant),*];

            /// Whether this native is a shared function object, which `Vm::native_value` builds on
            /// first read and every realm shares.
            pub(crate) const fn has_shared_function(self) -> bool {
                /// Each native's `unshared` flag, by discriminant.
                const UNSHARED: [bool; Native::COUNT as usize] = [$(natives!(@unshared $($flag)?)),*];
                !UNSHARED[self as usize]
            }
        }
    };
    (@unshared) => { false };
    (@unshared unshared) => { true };
}

#[rustfmt::skip]
natives! {
    Print, HostDone, CreateRealm, IsHTMLDDA, EvalScript, CollectGarbage, RealmTypeError, Eval, Function, FunctionPrototype, FunctionPrototypeHasInstance, FunctionCaller, DynamicImport, AbstractModuleSource, AbstractModuleSourceToStringTag, ShadowRealm, ShadowRealmEvaluate, ShadowRealmImportValue, ShadowRealmImportValueFulfilled, ShadowRealmWrappedFunction,
    Object,
    ObjectKeys, ObjectValues, ObjectEntries, ObjectGetOwnPropertyNames, ObjectGetOwnPropertySymbols, ObjectGetOwnPropertyDescriptor, ObjectGetOwnPropertyDescriptors, ObjectFromEntries, ObjectIs,
    ObjectCreate, ObjectAssign, ObjectDefineProperty, ObjectDefineProperties, ObjectGetPrototypeOf, ObjectPreventExtensions, ObjectIsExtensible, ObjectSeal, ObjectIsSealed, ObjectFreeze, ObjectIsFrozen, ObjectGroupBy,
    ObjectSetPrototypeOf, ObjectHasOwn, ObjectPrototypeHasOwnProperty, ObjectPrototypePropertyIsEnumerable, ObjectPrototypeIsPrototypeOf, ObjectPrototypeLookupGetter, ObjectPrototypeLookupSetter, ObjectPrototypeDefineGetter, ObjectPrototypeDefineSetter, ObjectPrototypeProtoGetter, ObjectPrototypeProtoSetter, ObjectPrototypeToLocaleString, ObjectPrototypeToString, ObjectPrototypeValueOf,
    ReflectGet, ReflectHas, ReflectApply, ReflectGetOwnPropertyDescriptor, ReflectDefineProperty, ReflectDeleteProperty, ReflectPreventExtensions, ReflectIsExtensible,
    ReflectSet,
    SuperSet,
    ObjectLiteralPrototype,
    ObjectLiteralCreate,
    ReflectOwnKeys,
    ReflectGetPrototypeOf,
    ReflectSetPrototypeOf,
    ReflectConstruct,
    Proxy, ProxyRevocable, ProxyRevoke,
    LazyDataProperty: unshared, TemporalInitialize: unshared,
    JsonParse,
    JsonStringify,
    JsonRawJson,
    JsonIsRawJson,
    Array, TypedArray, TypedArrayFrom, TypedArrayOf, TypedArrayAt, TypedArrayLastIndexOf, TypedArraySort, TypedArrayToReversed, TypedArrayToSorted, TypedArrayWith, TypedArrayToLocaleString, TypedArrayBufferGetter, TypedArrayByteLengthGetter, TypedArrayByteOffsetGetter, TypedArrayLengthGetter, TypedArrayToStringTag, ArrayToLocaleString, ArraySpecies, ArrayFromAsync,
    ArrayFromAsyncFulfilled, ArrayFromAsyncRejected,
    ArrayIsArray,
    ArrayPush,
    ArrayPop,
    ArraySlice,
    ArrayIncludes,
    ArrayJoin,
    ArrayConcat,
    ArrayFlat,
    ArrayReverse,
    ArrayShift,
    ArrayUnshift,
    ArraySplice,
    ArrayFill,
    ArrayAt,
    ArrayLastIndexOf,
    ArrayIndexOf,
    ArrayCopyWithin,
    ArrayWith,
    ArrayForEach,
    ArrayMap,
    ArrayFilter,
    ArraySome,
    ArrayEvery,
    ArrayFind,
    ArrayFindIndex,
    ArrayFindLast,
    ArrayFindLastIndex,
    ArrayGroup,
    ArrayGroupToMap,
    ArrayFlatMap,
    ArrayReduce,
    ArrayReduceRight,
    TypedArrayForEach,
    TypedArrayMap,
    TypedArrayFilter,
    TypedArraySome,
    TypedArrayEvery,
    TypedArrayFind,
    TypedArrayFindIndex,
    TypedArrayFindLast,
    TypedArrayFindLastIndex,
    TypedArrayReduce,
    TypedArrayReduceRight,
    ArrayToReversed,
    ArrayToSpliced,
    ArraySort,
    ArrayToSorted,
    ArrayToString,
    ArrayKeys,
    ArrayValues,
    ArrayEntries,
    ArrayFrom,
    ArrayOf,
    ArrayBuffer,
    ArrayBufferSpecies,
    ArrayBufferSlice,
    ArrayBufferTransfer,
    ArrayBufferResize,
    ArrayBufferTransferToFixedLength,
    ArrayBufferTransferToImmutable,
    ArrayBufferSliceToImmutable,
    ArrayBufferByteLengthGetter,
    ArrayBufferDetachedGetter,
    ArrayBufferImmutableGetter,
    ArrayBufferMaxByteLengthGetter,
    ArrayBufferResizableGetter,
    SharedArrayBufferByteLengthGetter,
    SharedArrayBufferGrowableGetter,
    SharedArrayBufferMaxByteLengthGetter,
    ArrayBufferIsView,
    DetachArrayBuffer,
    ArrayIteratorNext,
    SharedArrayBuffer,
    SharedArrayBufferGrow,
    SharedArrayBufferSlice,
    AtomicsLoad,
    AtomicsStore,
    AtomicsAdd,
    AtomicsSub,
    AtomicsAnd,
    AtomicsOr,
    AtomicsXor,
    AtomicsExchange,
    AtomicsCompareExchange,
    AtomicsIsLockFree,
    AtomicsNotify,
    AtomicsWait,
    AtomicsWaitAsync,
    AtomicsPause,
    Uint8Array,
    Uint8ClampedArray,
    Uint16Array,
    Uint32Array,
    Int8Array,
    Int16Array,
    Int32Array,
    BigInt64Array,
    BigUint64Array,
    Float16Array,
    Float32Array,
    Float64Array,
    Uint8ArraySet,
    Uint8ArrayReverse,
    Uint8ArrayFill,
    Uint8ArrayCopyWithin,
    Uint8ArraySubarray,
    Uint8ArraySlice,
    Uint8ArrayIncludes,
    Uint8ArrayIndexOf,
    Uint8ArrayJoin,
    Uint8ArrayToString,
    Uint8ArrayKeys,
    Uint8ArrayValues,
    Uint8ArrayEntries,
    Uint8ArrayFromBase64,
    Uint8ArrayFromHex,
    Uint8ArraySetFromBase64,
    Uint8ArraySetFromHex,
    Uint8ArrayToBase64,
    Uint8ArrayToHex,
    DataView,
    DataViewGetUint8,
    DataViewSetUint8,
    DataViewGetInt8,
    DataViewSetInt8,
    DataViewGetUint16,
    DataViewSetUint16,
    DataViewGetInt16,
    DataViewSetInt16,
    DataViewGetUint32,
    DataViewSetUint32,
    DataViewGetInt32,
    DataViewSetInt32,
    DataViewGetFloat32,
    DataViewSetFloat32,
    DataViewGetFloat64,
    DataViewSetFloat64,
    DataViewGetFloat16,
    DataViewSetFloat16,
    DataViewGetBigInt64,
    DataViewSetBigInt64,
    DataViewGetBigUint64,
    DataViewSetBigUint64,
    DataViewBufferGetter,
    DataViewByteLengthGetter,
    DataViewByteOffsetGetter,
    Map,
    MapGet,
    MapSet,
    MapHas,
    MapDelete,
    MapClear,
    MapKeys,
    MapValues,
    MapEntries,
    MapForEach,
    MapGetOrInsert,
    MapGetOrInsertComputed,
    MapGroupBy,
    MapSizeGetter,
    Set,
    SetAdd,
    SetHas,
    SetDelete,
    SetClear,
    SetKeys,
    SetValues,
    SetEntries,
    SetForEach,
    SetSizeGetter,
    SetDifference,
    SetIntersection,
    SetSymmetricDifference,
    SetUnion,
    SetIsDisjointFrom,
    SetIsSubsetOf,
    SetIsSupersetOf,
    SetSpeciesGetter,
    Iterator,
    IteratorFrom,
    IteratorConcat,
    IteratorZip,
    IteratorZipKeyed,
    IteratorMap,
    IteratorFilter,
    IteratorTake,
    IteratorDrop,
    IteratorFlatMap,
    IteratorReduce,
    IteratorToArray,
    IteratorForEach,
    IteratorEvery,
    IteratorFind,
    IteratorSome,
    IteratorDispose,
    IteratorProtocolNext,
    IteratorProtocolReturn,
    IteratorHelperNext,
    IteratorHelperReturn,
    IteratorPrototypeConstructorGetter,
    IteratorPrototypeConstructorSetter,
    IteratorPrototypeToStringTagGetter,
    IteratorPrototypeToStringTagSetter,
    IteratorNext, StringIteratorNext, RegExpStringIteratorNext, IteratorClose, IteratorSelf, AsyncIteratorSelf,
    IteratorReturn, IteratorThrow,
    GeneratorNext, GeneratorReturn, GeneratorThrow,
    AsyncGeneratorNext, AsyncGeneratorReturn, AsyncGeneratorThrow,
    AsyncGeneratorReturnFulfilled, AsyncGeneratorReturnRejected,
    AsyncIteratorDispose, AsyncIteratorDisposeFulfilled,
    Test262Agent,
    WeakMap,
    WeakMapGet,
    WeakMapSet,
    WeakMapHas,
    WeakMapDelete,
    WeakMapGetOrInsert,
    WeakMapGetOrInsertComputed,
    WeakSet,
    WeakSetAdd,
    WeakSetHas,
    WeakSetDelete,
    WeakRef,
    WeakRefDeref,
    FinalizationRegistry,
    FinalizationRegistryRegister,
    FinalizationRegistryUnregister,
    DisposableStack,
    AsyncDisposableStack,
    AsyncDisposableStackUse,
    AsyncDisposableStackAdopt,
    AsyncDisposableStackDefer,
    AsyncDisposableStackMove,
    AsyncDisposableStackDisposeAsync,
    AsyncDisposableStackDisposed,
    DisposableStackMove,
    DisposableStackDisposed,
    DisposableStackUse,
    DisposableStackAdopt,
    DisposableStackDefer,
    DisposableStackDispose,
    DisposableStackUseAsync,
    DisposableStackDisposeAsync,
    DisposableStackDisposeWithCompletion,
    DisposableStackDisposeAsyncWithCompletion,
    DisposableStackAsyncDisposalFulfilled,
    DisposableStackAsyncDisposalRejected,
    FunctionCall, FunctionApply, FunctionBind, FunctionBoundCall, FunctionToString, AsyncFunction, GeneratorFunction, AsyncGeneratorFunction, AsyncGeneratorDelegateReturnStart,
    Date,
    DateNow,
    DateGetTime, DateValueOf, DateGetTimezoneOffset, DateGetFullYear, DateGetMonth, DateGetDate, DateGetDay,
    DateGetHours, DateGetMinutes, DateGetSeconds, DateGetMilliseconds, DateGetUTCFullYear,
    DateGetUTCMonth, DateGetUTCDate, DateGetUTCDay, DateGetUTCHours, DateGetUTCMinutes,
    DateGetUTCSeconds, DateGetUTCMilliseconds, DateGetYear,
    DateSetTime, DateSetFullYear, DateSetMonth, DateSetUTCMonth, DateSetDate, DateSetUTCDate,
    DateSetUTCFullYear, DateSetHours, DateSetMinutes, DateSetSeconds, DateSetMilliseconds,
    DateSetUTCHours, DateSetUTCMinutes, DateSetUTCSeconds, DateSetUTCMilliseconds, DateSetYear,
    DateToString, DateToDateString, DateToTimeString, DateToUTCString,
    DateToLocaleString, DateToLocaleDateString, DateToLocaleTimeString, DateToISOString,
    DateToJSON, DateToPrimitive, DateToTemporalInstant, DateParse, DateUTC,
    Error, ErrorToString, ErrorIsError, ErrorCaptureStackTrace, ErrorStackGetter, ErrorStackSetter,
    CallSiteGetFileName, CallSiteGetThis, CallSiteGetFunctionName, CallSiteGetLineNumber, CallSiteGetColumnNumber,
    CallSiteGetTypeName, CallSiteGetMethodName, CallSiteIsEval, CallSiteGetEvalOrigin,
    CallSiteIsConstructor,
    // Named for `CallSite.prototype.isNative`, like every `<Builtin><Method>` variant.
    #[allow(clippy::enum_variant_names)]
    CallSiteIsNative,
    CallSiteToString,
    AggregateError, SuppressedError, EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError, ThrowTypeError,
    RegExp, RegExpCompile, RegExpEscape, RegExpLegacyGetter, RegExpLegacySetter, RegExpToString, RegExpSymbolMatch, RegExpSymbolSearch, RegExpSymbolReplace,
    RegExpSymbolMatchAll, RegExpSymbolSplit, RegExpSpecies,
    RegExpExec,
    RegExpTest,
    RegExpGlobal,
    RegExpIgnoreCase,
    RegExpMultiline,
    RegExpDotAll,
    RegExpUnicode,
    RegExpUnicodeSets,
    RegExpSticky,
    RegExpHasIndices,
    RegExpSource,
    RegExpFlags,
    String, Boolean, BooleanToString, BooleanValueOf,
    Symbol, SymbolToString, SymbolToPrimitive, SymbolValueOf, SymbolDescriptionGetter,
    BigInt, BigIntValueOf, BigIntToString, BigIntToLocaleString, BigIntAsIntN, BigIntAsUintN,
    IntlNumberFormat, IntlNumberFormatSupportedLocalesOf,
    IntlNumberFormatFormatGetter, IntlNumberFormatFormat,
    IntlNumberFormatFormatToParts, IntlNumberFormatFormatRange,
    IntlNumberFormatFormatRangeToParts,
    IntlNumberFormatResolvedOptions,
    IntlGetCanonicalLocales, IntlSupportedValuesOf, IntlLocale,
    IntlLocaleToString, IntlLocaleMaximize, IntlLocaleMinimize,
    IntlLocaleGetCalendars, IntlLocaleGetCollations, IntlLocaleGetHourCycles,
    IntlLocaleGetNumberingSystems, IntlLocaleGetTimeZones, IntlLocaleGetTextInfo,
    IntlLocaleGetWeekInfo, IntlLocaleBaseNameGetter, IntlLocaleLanguageGetter,
    IntlLocaleScriptGetter, IntlLocaleRegionGetter, IntlLocaleVariantsGetter,
    IntlLocaleCalendarGetter, IntlLocaleCollationGetter, IntlLocaleHourCycleGetter,
    IntlLocaleCaseFirstGetter, IntlLocaleFirstDayOfWeekGetter,
    IntlLocaleNumberingSystemGetter, IntlLocaleNumericGetter,
    IntlCollator, IntlCollatorSupportedLocalesOf, IntlCollatorCompareGetter, IntlCollatorCompare, IntlCollatorResolvedOptions,
    IntlPluralRules, IntlPluralRulesSupportedLocalesOf, IntlPluralRulesSelect,
    IntlPluralRulesSelectRange, IntlPluralRulesResolvedOptions,
    IntlDateTimeFormat, IntlDateTimeFormatFormatGetter, IntlDateTimeFormatFormat,
    IntlDateTimeFormatFormatToParts, IntlDateTimeFormatFormatRange,
    IntlDateTimeFormatFormatRangeToParts, IntlDateTimeFormatSupportedLocalesOf,
    IntlDateTimeFormatResolvedOptions,
    IntlDisplayNames, IntlDisplayNamesOf, IntlDisplayNamesResolvedOptions, IntlDisplayNamesSupportedLocalesOf,
    IntlDurationFormat, IntlDurationFormatFormatGetter, IntlDurationFormatFormat,
    IntlDurationFormatFormatToParts, IntlDurationFormatResolvedOptions,
    IntlDurationFormatSupportedLocalesOf,
    IntlListFormat, IntlListFormatFormatGetter, IntlListFormatFormat, IntlListFormatFormatToParts,
    IntlListFormatResolvedOptions, IntlListFormatSupportedLocalesOf,
    IntlRelativeTimeFormat, IntlRelativeTimeFormatSupportedLocalesOf,
    IntlRelativeTimeFormatFormat, IntlRelativeTimeFormatFormatToParts,
    IntlRelativeTimeFormatResolvedOptions,
    IntlSegmenter, IntlSegmenterSupportedLocalesOf, IntlSegmenterSegment,
    IntlSegmenterResolvedOptions, IntlSegmenterSegmentsIterator,
    IntlSegmenterSegmentsContaining, IntlSegmenterIteratorNext,
    SymbolFor,
    SymbolKeyFor,
    StringCharCodeAt, StringSlice,
    StringCharAt,
    StringSubstring,
    StringSubstr,
    StringIncludes,
    StringStartsWith,
    StringEndsWith,
    StringIndexOf, StringLastIndexOf,
    StringToString, StringValueOf, StringLocaleCompare, StringToLocaleLowerCase, StringToLocaleUpperCase,
    StringReplace, StringSplit, StringTrim, StringTrimStart, StringTrimEnd,
    StringRepeat, StringPadStart, StringPadEnd, StringMatch, StringMatchAll, StringSearch,
    StringReplaceAll, StringAt, StringCodePointAt, StringToUpperCase, StringToLowerCase, StringConcat, StringNormalize, StringValues,
    StringAnchor, StringBig, StringBlink, StringBold, StringFixed, StringFontcolor, StringFontsize, StringItalics, StringLink, StringSmall, StringStrike, StringSub, StringSup,
    EncodeUri, EncodeUriComponent,
    DecodeUri, DecodeUriComponent,
    GlobalEscape, GlobalUnescape,
    StringFromCharCode, StringFromCodePoint, StringRaw, StringIsWellFormed,
    StringToWellFormed, ParseInt,
    TemporalToLocaleString,
    TemporalDuration, TemporalDurationFrom, TemporalDurationCompare,
    TemporalDurationAdd, TemporalDurationSubtract, TemporalDurationWith,
    TemporalDurationAbs, TemporalDurationNegated, TemporalDurationTotal,
    TemporalDurationRound,
    TemporalDurationToString, TemporalDurationToJSON, TemporalDurationValueOf,
    TemporalDurationYearsGetter, TemporalDurationMonthsGetter,
    TemporalDurationWeeksGetter, TemporalDurationDaysGetter,
    TemporalDurationHoursGetter, TemporalDurationMinutesGetter,
    TemporalDurationSecondsGetter, TemporalDurationMillisecondsGetter,
    TemporalDurationMicrosecondsGetter, TemporalDurationNanosecondsGetter,
    TemporalDurationSignGetter, TemporalDurationBlankGetter,
    TemporalPlainDate, TemporalPlainDateFrom, TemporalPlainDateCompare,
    TemporalPlainDateCalendarIdGetter, TemporalPlainDateYearGetter,
    TemporalPlainDateMonthGetter, TemporalPlainDateMonthCodeGetter,
    TemporalPlainDateDayGetter, TemporalPlainDateEraGetter,
    TemporalPlainDateEraYearGetter, TemporalPlainDateDayOfWeekGetter,
    TemporalPlainDateDayOfYearGetter, TemporalPlainDateWeekOfYearGetter,
    TemporalPlainDateYearOfWeekGetter, TemporalPlainDateDaysInWeekGetter,
    TemporalPlainDateDaysInMonthGetter, TemporalPlainDateDaysInYearGetter,
    TemporalPlainDateMonthsInYearGetter, TemporalPlainDateInLeapYearGetter,
    TemporalPlainDateToString,
    TemporalPlainDateToJSON, TemporalPlainDateToLocaleString,
    TemporalPlainDateToPlainDateTime, TemporalPlainDateToPlainMonthDay,
    TemporalPlainDateToPlainYearMonth, TemporalPlainDateToZonedDateTime,
    TemporalPlainDateEquals, TemporalPlainDateWith, TemporalPlainDateValueOf,
    TemporalPlainDateWithCalendar,
    TemporalPlainDateAdd,
    TemporalPlainDateSubtract, TemporalPlainDateUntil, TemporalPlainDateSince,
    TemporalPlainTime, TemporalPlainTimeFrom, TemporalPlainTimeCompare,
    TemporalPlainTimeAdd, TemporalPlainTimeSubtract,
    TemporalPlainTimeEquals,
    TemporalPlainTimeHourGetter, TemporalPlainTimeMinuteGetter,
    TemporalPlainTimeSecondGetter, TemporalPlainTimeMillisecondGetter,
    TemporalPlainTimeMicrosecondGetter, TemporalPlainTimeNanosecondGetter,
    TemporalPlainTimeValueOf,
    TemporalPlainTimeRound,
    TemporalPlainTimeUntil, TemporalPlainTimeSince,
    TemporalPlainTimeToString, TemporalPlainTimeToJSON,
    TemporalPlainTimeWith,
    TemporalPlainMonthDay, TemporalPlainMonthDayFrom, TemporalPlainMonthDayCompare,
    TemporalPlainMonthDayCalendarIdGetter, TemporalPlainMonthDayDayGetter,
    TemporalPlainMonthDayMonthCodeGetter, TemporalPlainMonthDayEquals,
    TemporalPlainMonthDayToString, TemporalPlainMonthDayToJSON,
    TemporalPlainMonthDayToLocaleString, TemporalPlainMonthDayToPlainDate,
    TemporalPlainMonthDayWith, TemporalPlainMonthDayValueOf,
    TemporalPlainYearMonth, TemporalPlainYearMonthFrom, TemporalPlainYearMonthCompare,
    TemporalPlainYearMonthCalendarIdGetter, TemporalPlainYearMonthYearGetter,
    TemporalPlainYearMonthMonthGetter, TemporalPlainYearMonthMonthCodeGetter,
    TemporalPlainYearMonthEraGetter, TemporalPlainYearMonthEraYearGetter,
    TemporalPlainYearMonthReferenceISODayGetter, TemporalPlainYearMonthDaysInMonthGetter,
    TemporalPlainYearMonthDaysInYearGetter, TemporalPlainYearMonthMonthsInYearGetter,
    TemporalPlainYearMonthInLeapYearGetter, TemporalPlainYearMonthEquals,
    TemporalPlainYearMonthToString, TemporalPlainYearMonthToJSON,
    TemporalPlainYearMonthToLocaleString, TemporalPlainYearMonthAdd,
    TemporalPlainYearMonthSubtract, TemporalPlainYearMonthUntil,
    TemporalPlainYearMonthSince, TemporalPlainYearMonthWith,
    TemporalPlainYearMonthValueOf, TemporalPlainYearMonthToPlainDate,
    TemporalPlainDateTime, TemporalPlainDateTimeFrom, TemporalPlainDateTimeCompare,
    TemporalPlainDateTimeAdd, TemporalPlainDateTimeSubtract, TemporalPlainDateTimeRound,
    TemporalPlainDateTimeUntil, TemporalPlainDateTimeSince,
    TemporalPlainDateTimeToString, TemporalPlainDateTimeToJSON,
    TemporalPlainDateTimeToPlainDate, TemporalPlainDateTimeToPlainTime,
    TemporalPlainDateTimeValueOf,
    TemporalPlainDateTimeToZonedDateTime,
    TemporalPlainDateTimeWith,
    TemporalPlainDateTimeWithCalendar,
    TemporalPlainDateTimeWithPlainTime,
    TemporalPlainDateTimeEquals,
    TemporalPlainDateTimeCalendarIdGetter,
    TemporalPlainDateTimeYearGetter, TemporalPlainDateTimeMonthGetter,
    TemporalPlainDateTimeMonthCodeGetter, TemporalPlainDateTimeDayGetter,
    TemporalPlainDateTimeEraGetter, TemporalPlainDateTimeEraYearGetter,
    TemporalPlainDateTimeDayOfWeekGetter, TemporalPlainDateTimeDayOfYearGetter,
    TemporalPlainDateTimeWeekOfYearGetter, TemporalPlainDateTimeYearOfWeekGetter,
    TemporalPlainDateTimeDaysInWeekGetter, TemporalPlainDateTimeDaysInMonthGetter,
    TemporalPlainDateTimeDaysInYearGetter, TemporalPlainDateTimeMonthsInYearGetter,
    TemporalPlainDateTimeInLeapYearGetter,
    TemporalPlainDateTimeHourGetter, TemporalPlainDateTimeMinuteGetter,
    TemporalPlainDateTimeSecondGetter, TemporalPlainDateTimeMillisecondGetter,
    TemporalPlainDateTimeMicrosecondGetter, TemporalPlainDateTimeNanosecondGetter,
    TemporalZonedDateTime, TemporalZonedDateTimeEpochNanosecondsGetter,
    TemporalZonedDateTimeFrom, TemporalZonedDateTimeCompare,
    TemporalZonedDateTimeEquals,
    TemporalZonedDateTimeWithTimeZone, TemporalZonedDateTimeWith,
    TemporalZonedDateTimeWithCalendar,
    TemporalZonedDateTimeWithPlainTime,
    TemporalZonedDateTimeToInstant, TemporalZonedDateTimeToPlainDate,
    TemporalZonedDateTimeToPlainDateTime, TemporalZonedDateTimeToPlainTime,
    TemporalZonedDateTimeValueOf, TemporalZonedDateTimeAdd, TemporalZonedDateTimeSubtract,
    TemporalZonedDateTimeGetTimeZoneTransition,
    TemporalZonedDateTimeStartOfDay,
    TemporalZonedDateTimeRound,
    TemporalZonedDateTimeUntil, TemporalZonedDateTimeSince,
    TemporalZonedDateTimeToString,
    TemporalZonedDateTimeToJSON,
    TemporalZonedDateTimeToLocaleString,
    TemporalZonedDateTimeTimeZoneIdGetter, TemporalZonedDateTimeCalendarIdGetter,
    TemporalZonedDateTimeYearGetter, TemporalZonedDateTimeMonthGetter,
    TemporalZonedDateTimeMonthCodeGetter,
    TemporalZonedDateTimeDayGetter, TemporalZonedDateTimeHourGetter,
    TemporalZonedDateTimeMinuteGetter, TemporalZonedDateTimeSecondGetter,
    TemporalZonedDateTimeMillisecondGetter, TemporalZonedDateTimeMicrosecondGetter,
    TemporalZonedDateTimeNanosecondGetter,
    TemporalZonedDateTimeEraGetter, TemporalZonedDateTimeEraYearGetter,
    TemporalZonedDateTimeDayOfWeekGetter, TemporalZonedDateTimeDayOfYearGetter,
    TemporalZonedDateTimeWeekOfYearGetter, TemporalZonedDateTimeYearOfWeekGetter,
    TemporalZonedDateTimeDaysInWeekGetter, TemporalZonedDateTimeDaysInMonthGetter,
    TemporalZonedDateTimeDaysInYearGetter, TemporalZonedDateTimeMonthsInYearGetter,
    TemporalZonedDateTimeInLeapYearGetter,
    TemporalZonedDateTimeEpochMillisecondsGetter, TemporalZonedDateTimeHoursInDayGetter,
    TemporalZonedDateTimeOffsetGetter, TemporalZonedDateTimeOffsetNanosecondsGetter,
    TemporalInstant, TemporalInstantFrom, TemporalInstantCompare,
    TemporalInstantFromEpochMilliseconds, TemporalInstantFromEpochNanoseconds,
    TemporalInstantEpochNanosecondsGetter, TemporalInstantEpochMillisecondsGetter,
    TemporalInstantToString, TemporalInstantToJSON, TemporalInstantValueOf,
    TemporalInstantEquals, TemporalInstantAdd, TemporalInstantSubtract, TemporalInstantRound,
    TemporalInstantSince, TemporalInstantUntil,
    TemporalInstantToZonedDateTimeISO,
    TemporalNowInstant, TemporalNowPlainDateISO, TemporalNowPlainDateTimeISO,
    TemporalNowPlainTimeISO, TemporalNowTimeZoneId, TemporalNowZonedDateTimeISO,
    MathLog, MathPow, MathFloor, MathMin, MathMax, MathRandom,
    MathAbs, MathCeil, MathRound, MathTrunc, MathSqrt, MathSign, MathAcos, MathAsin, MathAtan, MathCos, MathExp, MathSin, MathTan, MathAtan2, NumberString, NumberToLocaleString, Number, NumberValueOf,
    MathAcosh, MathAsinh, MathAtanh, MathCbrt, MathCosh, MathExpm1, MathFround,
    MathHypot, MathImul, MathLog10, MathLog1p, MathLog2, MathSinh, MathTanh,
    MathClz32, MathF16Round, MathSumPrecise,
    NumberIsNaN, NumberIsFinite, NumberIsInteger, NumberIsSafeInteger, NumberParseFloat, GlobalIsNaN, GlobalIsFinite,
    NumberFixed,
    NumberExponential,
    NumberPrecision,
    Promise,
    PromiseSpeciesGetter,
    PromiseResolve,
    PromiseReject,
    PromiseTry,
    PromiseWithResolvers,
    PromiseCapabilityExecutor,
    PromiseThen,
    PromiseCatch,
    PromiseFinally,
    PromiseFinallyHandler,
    PromiseFinallyContinuationHandler,
    PromiseAll,
    PromiseAllKeyed,
    PromiseRace,
    PromiseAllSettled,
    PromiseAllSettledKeyed,
    PromiseAny,
    PromiseAggregateJob,
    AsyncFromSyncValue, AsyncFromSyncValueRejected, AsyncGeneratorDelegateFulfilled, AsyncGeneratorDelegateRejected,
    WithEnter, WithExit,
    WasmHost: unshared,
    WebAssemblyValidate, WebAssemblyCompile, WebAssemblyInstantiate,
    WasmModule, WasmModuleExports, WasmModuleImports, WasmModuleCustomSections,
    WasmInstance, WasmInstanceExportsGetter,
    WasmMemory, WasmMemoryGrow, WasmMemoryBufferGetter,
    WasmTable, WasmTableLengthGetter, WasmTableGrow, WasmTableGet, WasmTableSet,
    WasmGlobal, WasmGlobalValueGetter, WasmGlobalValueSetter, WasmGlobalValueOf, WasmTag,
    WasmCompileError, WasmLinkError, WasmRuntimeError,
    /// A Wasm function as JavaScript sees it; its environment is the Wasm closure.
    WasmExportedFunction: unshared,
    HostFunction: unshared,
    CallSiteConstructor,
    CallSiteGetFunction,
    MathInitialize: unshared,
    /// Initializers of the lazily installed namespaces and groups (`install_lazy_builtin_namespace`).
    AtomicsInitialize: unshared,
    WebAssemblyInitialize: unshared,
    IntlInitialize: unshared,
    ReflectInitialize: unshared,
    ProxyInitialize: unshared,
    WeakRefInitialize: unshared,
    FinalizationRegistryInitialize: unshared,
    DataViewInitialize: unshared,
    /// The `console` family's stderr writer (`warn`/`error`).
    PrintError,
}

/// A global's cold facts, boxed with its object part so the cell stays compact.
#[derive(Clone, Debug)]
pub(crate) struct WasmGlobalHeader {
    pub(crate) object: Object,
    pub(crate) declarations: crate::WasmTypes,
}

/// A memory's cold facts, boxed with its object part so the cell stays compact.
#[derive(Clone, Debug)]
pub struct WasmMemoryHeader {
    pub(crate) object: Object,
    pub ty: wasmparser::MemoryType,
}

/// The internal slot of a `Cell::WasmObject`.
#[derive(Clone)]
pub(crate) enum WasmObjectSlot {
    Module(Box<crate::WasmModule>),
    /// The frozen exports object of an instance; its bindings stay alive through it.
    Instance {
        exports: Value,
    },
}

impl std::fmt::Debug for WasmObjectSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Module(_) => f.write_str("Module"),
            Self::Instance { exports } => f
                .debug_struct("Instance")
                .field("exports", exports)
                .finish(),
        }
    }
}

/// What a Wasm host function runs: an embedder operation, or a JavaScript callable
/// imported through the `WebAssembly` namespace.
#[derive(Clone, Debug)]
pub(crate) enum WasmHostTarget {
    Host(crate::WasmHostFunctionId),
    Js(Value),
    /// Exact Emscripten `HEAPU8.copyWithin(dest, src, src + num)` body, with the
    /// mutable captured view location retained so each call observes memory growth.
    JsEmscriptenMemcpy(Box<WasmHostMemcpyTarget>),
}

#[derive(Clone, Debug)]
pub(crate) struct WasmHostMemcpyTarget {
    pub(crate) function: Value,
    pub(crate) view_environment: Value,
    pub(crate) view_slot: usize,
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum ArrayFromAsyncAwait {
    IteratorStep,
    ArrayLikeValue,
    MapperResult,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ArrayFromAsyncState {
    pub(crate) output: Value,
    pub(crate) iterator: Option<Value>,
    pub(crate) result: Value,
    pub(crate) mapper: Option<Value>,
    pub(crate) this_arg: Value,
    pub(crate) index: usize,
    pub(crate) array_like: Option<(Value, usize)>,
    pub(crate) awaiting: ArrayFromAsyncAwait,
}

#[derive(Clone, Copy, Debug)]
pub enum FunctionKind {
    User(ProgramId, u32),
    Native(Native),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IteratorKind {
    Array,
    ArrayKeys,
    ArrayValues,
    ArrayEntries,
    String,
    MapKeys,
    MapValues,
    MapEntries,
    SetValues,
    SetEntries,
    Protocol,
    RegExpStringMatchAll,
    IntlSegments,
    Map,
    Filter,
    Take,
    Drop,
    FlatMap,
    Concat,
    Zip,
    Generator,
    AsyncFromSync,
    AsyncGenerator,
}
impl IteratorKind {
    #[inline]
    pub(crate) fn is_array_iterator(self) -> bool {
        matches!(
            self,
            Self::Array | Self::ArrayKeys | Self::ArrayValues | Self::ArrayEntries
        )
    }

    #[inline]
    pub(crate) fn is_collection_iterator(self) -> bool {
        matches!(
            self,
            Self::MapKeys | Self::MapValues | Self::MapEntries | Self::SetValues | Self::SetEntries
        )
    }
}
#[derive(Clone, Debug)]
pub enum IteratorHelper {
    RegExpStringMatchAll {
        input: Value, // The string matched, the value each step passes to `exec`.
        global: bool,
        unicode: bool,
    },
    Map {
        callback: Value,
        index: usize,
    },
    Filter {
        callback: Value,
        index: usize,
    },
    Take {
        remaining: f64,
    },
    Drop {
        remaining: f64,
    },
    FlatMap {
        callback: Value,
        index: usize,
        inner: Option<Value>,
    },
    Concat {
        items: Vec<Value>,
        methods: Vec<Value>,
        opened: Vec<Option<Value>>,
        next_item: usize,
        active: Option<Value>,
    },
    Zip {
        iterators: Vec<Value>,
        padding: Vec<Value>,
        mode: IteratorZipMode,
        keys: Option<Vec<Value>>,
        opened: Vec<bool>,
        done: bool,
    },
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum IteratorZipMode {
    Shortest,
    Longest,
    Strict,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IteratorConsumer {
    Reduce,
    ToArray,
    ForEach,
    Every,
    Find,
    Some,
}
/// The header words of an object with prototype `proto` and inline storage of `shape`, as
/// `Object::with_property_storage` sets them: derived once by a construct plan, so each
/// planned construction allocates without encoding the prototype or the storage again.
#[derive(Clone, Copy)]
pub struct InlineObjectHeader {
    /// The interned header (`Object::layout`).
    layout: u32,
}

impl InlineObjectHeader {
    /// The inline shape this header stores.
    #[inline] pub(crate) fn shape(self) -> u32 {
        ValueVec::from_parts(0, super::layouts::word(self.layout) as u32).auxiliary()
    }

    /// No receiver's header: an unplanned entry's, which no construction reads.
    pub(crate) const EMPTY: Self = Self { layout: 0 };

    pub fn of(proto: Value, shape: u32) -> Self {
        Self::of_storage(proto, ValueVec::inline_property_storage(shape))
    }

    /// The header words of an object with prototype `proto` whose `shape`'s values live in an
    /// arena run (`Object::from_run_header`), as `Object::with_property_storage` sets them
    /// apart from the run's position.
    pub fn of_run(proto: Value, shape: u32) -> Self {
        Self::of_storage(proto, ValueVec::from_parts(0, shape))
    }

    fn of_storage(proto: Value, storage: ValueVec) -> Self {
        // Its inline values are `undefined`, which the payload word holds, so the object has
        // no extras and nothing to drop.
        let object = std::mem::ManuallyDrop::new(Object::with_property_storage(proto, storage, [Value::UNDEFINED; INLINE_PROPERTY_COUNT]));
        Self { layout: object.layout }
    }
}

#[repr(C)]
/// Where an object outside dictionary storage keeps its values (`Object::exact_storage`).
#[derive(Clone, Copy)]
pub(crate) enum ExactStorage {
    /// No extras and no inline storage: an arena run from this start.
    Plain(usize),
    /// No extras: the inline slots, narrow, in the payload word.
    PackedInline,
    /// Behind extras, as a prototype's or an object with a wide inline value: the inline
    /// slots there, or an arena run (`Object::properties`).
    Extended { inline: bool },
    /// No extras: a narrow run (`value_vec::NARROW_FORM`) from this start.
    Narrow,
}

#[repr(C, packed(4))]
/// An ordinary object's record. Its words are `ObjectHead` (`staged::record_heads`), which the
/// stencils read by offset: the payload word (`ObjectHead::payload`: with `ObjectExtras`,
/// `EXTRAS_TAG` and a pointer to them; without, the inline values, the storage position, or an
/// array's element backing) and the layout id (`ObjectHead::layout`, interned in `heap::layouts`,
/// which keeps the prototype, the `OBJECT_*` flags and the shape, so the record keeps four bytes
/// for them where it kept eight). Field accesses reach the head through `Deref`.
pub struct Object {
    head: ObjectHead,
}

impl std::ops::Deref for Object {
    type Target = ObjectHead;
    #[inline(always)]
    fn deref(&self) -> &ObjectHead {
        &self.head
    }
}

impl std::ops::DerefMut for Object {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut ObjectHead {
        &mut self.head
    }
}

/// Bits of `Object::meta` holding the prototype's heap reference. The field is partitioned
/// by reference family: legacy slot indices, then one range per direct record space, then
/// the null and undefined sentinels at the top.
const PROTO_NULL: u32 = PROTO_INDEX_BITS;
const PROTO_UNDEFINED: u32 = PROTO_INDEX_BITS - 1;
/// Cells a heap may hold so that any of them can be a prototype (`staged::object_encoding`), and
/// the link layout generated code reads: the legacy slots, then the object window, then the array
/// window, each `PROTO_RECORD_WINDOW` links and aligned to it.
pub(crate) use crate::staged::object_encoding::{MAX_HEAP_CELLS, PROTO_ARRAY_RECORD_BASE, PROTO_INDEX_BITS, PROTO_OBJECT_RECORD_BASE};
use crate::staged::object_encoding::PROTO_RECORD_WINDOW;
/// The legacy slots a heap reserves, every one of which a window link names (`heap::slots`).
pub(crate) const LEGACY_SLOT_CAPACITY: usize = MAX_HEAP_CELLS;
/// Window sizes of the prototype link space, one per family. A record space may grow past its
/// window (`records.rs` capacities); a record past it that becomes a prototype takes an overflow
/// link (`heap::prototypes`), codes from `PROTO_RECORD_END` up to the sentinels.
const OBJECT_PROTO_WINDOW: usize = PROTO_RECORD_WINDOW;
const ARRAY_PROTO_WINDOW: usize = PROTO_RECORD_WINDOW;
const FUNCTION_PROTO_WINDOW: usize = PROTO_RECORD_WINDOW;
const PROMISE_PROTO_WINDOW: usize = PROTO_RECORD_WINDOW;
const ITERATOR_PROTO_WINDOW: usize = PROTO_RECORD_WINDOW;
const COLLECTION_PROTO_WINDOW: usize = PROTO_RECORD_WINDOW;
const PROTO_FUNCTION_RECORD_BASE: u32 = PROTO_ARRAY_RECORD_BASE + ARRAY_PROTO_WINDOW as u32;
const PROTO_PROMISE_RECORD_BASE: u32 = PROTO_FUNCTION_RECORD_BASE + FUNCTION_PROTO_WINDOW as u32;
const PROTO_ITERATOR_RECORD_BASE: u32 = PROTO_PROMISE_RECORD_BASE + PROMISE_PROTO_WINDOW as u32;
const PROTO_COLLECTION_RECORD_BASE: u32 = PROTO_ITERATOR_RECORD_BASE + ITERATOR_PROTO_WINDOW as u32;
const PROTO_RECORD_END: u32 = PROTO_COLLECTION_RECORD_BASE + COLLECTION_PROTO_WINDOW as u32;
const _: () = assert!(PROTO_ARRAY_RECORD_BASE == PROTO_OBJECT_RECORD_BASE + OBJECT_PROTO_WINDOW as u32);
const _: () = assert!(PROTO_RECORD_END < PROTO_UNDEFINED);
const _: () = assert!(PROTO_OBJECT_RECORD_BASE.is_multiple_of(OBJECT_PROTO_WINDOW as u32));
const _: () = assert!(PROTO_ARRAY_RECORD_BASE.is_multiple_of(ARRAY_PROTO_WINDOW as u32));
const _: () = assert!(PROTO_FUNCTION_RECORD_BASE.is_multiple_of(FUNCTION_PROTO_WINDOW as u32));
const _: () = assert!(PROTO_PROMISE_RECORD_BASE.is_multiple_of(PROMISE_PROTO_WINDOW as u32));
const _: () = assert!(PROTO_ITERATOR_RECORD_BASE.is_multiple_of(ITERATOR_PROTO_WINDOW as u32));
const _: () = assert!(PROTO_COLLECTION_RECORD_BASE.is_multiple_of(COLLECTION_PROTO_WINDOW as u32));
const _: () = assert!(LEGACY_SLOT_CAPACITY <= PROTO_OBJECT_RECORD_BASE as usize);

/// The link of `prototype`, record `id` of a family whose window starts at `base` and holds
/// `window` links: its window link while `id` fits, else an overflow link (`heap::prototypes`),
/// so a record space may grow past its window and its records still serve as prototypes.
#[inline]
fn windowed(prototype: Value, base: u32, window: usize, id: u32) -> u32 {
    if (id as usize) < window {
        base + id
    } else {
        overflow_link(prototype)
    }
}

/// The overflow link of `prototype` (`heap::prototypes`): past every window, below the sentinels.
#[cold]
#[inline(never)]
fn overflow_link(prototype: Value) -> u32 {
    let codes = (PROTO_UNDEFINED - PROTO_RECORD_END) as usize;
    let index = super::prototypes::intern(prototype.to_bits(), codes)
        .expect("prototype links exhausted: every out-of-window code is taken");
    PROTO_RECORD_END + index
}

/// An object's encoded prototype reference (`PROTO_INDEX_BITS`). Equal links name the same
/// prototype, so a chain walk compares links instead of decoding each to a `Value`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProtoLink(u32);

impl ProtoLink {
    pub(crate) const NULL: Self = Self(PROTO_NULL);

    /// The link naming `prototype`, an object or null (`Object::encode_proto`).
    #[inline(always)]
    pub fn of(prototype: Value) -> Self {
        Self(Object::encode_proto(prototype))
    }

    /// The link whose encoding is `bits` (`bits`'s inverse).
    pub(crate) fn from_bits(bits: u32) -> Self { Self(bits) }

    /// The link as the object header encodes it (`OBJECT.proto_mask`).
    #[inline(always)]
    pub fn bits(self) -> u32 {
        self.0
    }

    /// The window link naming object record `id`; `None` past the object window, where the
    /// record's link (if it is a prototype) is an overflow link (`heap::prototypes`).
    #[inline(always)]
    pub(crate) fn object_record(id: u32) -> Option<Self> {
        ((id as usize) < OBJECT_PROTO_WINDOW).then_some(Self(PROTO_OBJECT_RECORD_BASE + id))
    }

    /// The object record this link names, if it names one.
    #[inline(always)]
    pub fn object_record_id(self) -> Option<u32> {
        // The object range starts at a power of two above its own power-of-two span, so a
        // link in it holds the record id in its low bits: a mask, which addresses the record
        // without the subtracted base as a 64-bit displacement.
        // `staged::object::proto_object_record_id` over u32, which keeps hot readers' registers free.
        let span = OBJECT_PROTO_WINDOW as u32;
        (self.0 & !(span - 1) == PROTO_OBJECT_RECORD_BASE).then_some(self.0 & (span - 1))
    }

    /// The prototype this link names, decoded (`Object::encode_proto`'s inverse).
    #[inline(always)]
    pub(crate) fn prototype(self) -> Value {
        match self.0 {
            PROTO_NULL => Value::NULL,
            PROTO_UNDEFINED => Value::UNDEFINED,
            index if index < PROTO_OBJECT_RECORD_BASE => Value::heap(index),
            index if index < PROTO_ARRAY_RECORD_BASE => {
                Value::direct_object(index - PROTO_OBJECT_RECORD_BASE)
            }
            index if index < PROTO_FUNCTION_RECORD_BASE => {
                Value::direct_array(index - PROTO_ARRAY_RECORD_BASE)
            }
            index if index < PROTO_PROMISE_RECORD_BASE => {
                Value::direct_function(index - PROTO_FUNCTION_RECORD_BASE)
            }
            index if index < PROTO_ITERATOR_RECORD_BASE => {
                Value::direct_promise(index - PROTO_PROMISE_RECORD_BASE)
            }
            index if index < PROTO_COLLECTION_RECORD_BASE => {
                Value::direct_iterator(index - PROTO_ITERATOR_RECORD_BASE)
            }
            index if index < PROTO_RECORD_END => Value::direct_collection(index - PROTO_COLLECTION_RECORD_BASE),
            index => Value::from_bits(super::prototypes::bits(index - PROTO_RECORD_END)),
        }
    }
}

use crate::staged::object_encoding::{EXTRAS_TAG, EXTRAS_TAG_MASK, NARROW_RUN_TAG, OBJECT_ARRAY, OBJECT_INLINE_STORAGE};
const _: () = assert!(
    PROTO_INDEX_BITS & (OBJECT_ARRAY | OBJECT_INLINE_STORAGE | VALUE_VEC_INTEGRITY_BITS) == 0
        && (OBJECT_ARRAY | OBJECT_INLINE_STORAGE) & VALUE_VEC_INTEGRITY_BITS == 0
);
// The auxiliary word's inline form bit is the header's inline flag at the same position, so
// `Object::storage_form` moves it without a shift.
const _: () = assert!(OBJECT_INLINE_STORAGE == crate::value_vec::INLINE_FORM);
/// Inline property values held by `ObjectExtras`; undefined until an object moves its own in.
#[derive(Clone, Copy, Debug)]
struct DisplacedInline([Value; INLINE_PROPERTY_COUNT]);

impl Default for DisplacedInline {
    fn default() -> Self {
        Self([Value::UNDEFINED; INLINE_PROPERTY_COUNT])
    }
}

/// The storage position of an object without property values (`ValueVec::new`): the only
/// one an array without extras has, as its payload word holds its element backing instead.
const EMPTY_POSITION: u32 = ValueVec::new().into_parts().0 & VALUE_VEC_START_BITS;

/// Storage forms whose complete layout is in the ordinary object's record.
/// This is an observation until a native receiver guard establishes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlainPropertyStorage {
    PackedInline,
    Arena,
}

/// Layout facts for the generated read-only ordinary-object field path. Keep these derived
/// from the authoritative storage declarations; native code checks the object and extras
/// representation bits before using any offset below.
pub(super) const NATIVE_OBJECT_RECORD_SIZE: usize = std::mem::size_of::<Object>();
pub(super) const NATIVE_OBJECT_RECORD_ALIGN: usize = std::mem::align_of::<Object>();
/// Where a record keeps its layout id; the header words are in the layout's table entry
/// (`heap::layouts::table_base`), `meta` at `NATIVE_OBJECT_META_OFFSET` and `auxiliary` at
/// `NATIVE_OBJECT_AUXILIARY_OFFSET` within it.
pub(super) const NATIVE_OBJECT_LAYOUT_OFFSET: usize = std::mem::offset_of!(Object, head) + std::mem::offset_of!(ObjectHead, layout);
pub(super) const NATIVE_OBJECT_META_OFFSET: usize = crate::staged::object_encoding::META_OFFSET;
pub(super) const NATIVE_OBJECT_AUXILIARY_OFFSET: usize = 0;

/// An object's two header words as its layout's entry holds them (`heap::layouts`): the
/// auxiliary word low, `meta` high.
#[inline(always)]
const fn header_word(meta: u32, auxiliary: u32) -> u64 {
    auxiliary as u64 | (meta as u64) << u32::BITS
}
pub(super) const NATIVE_OBJECT_PAYLOAD_OFFSET: usize = std::mem::offset_of!(Object, head) + std::mem::offset_of!(ObjectHead, payload);
/// Where the extras head's fields are: the head is the record's first field (`ObjectExtras::head`).
pub(super) const NATIVE_OBJECT_EXTRAS_ARENA_START_OFFSET: usize = std::mem::offset_of!(ObjectExtras, head)
    + std::mem::offset_of!(ObjectExtrasHead<DisplacedInline, super::ArrayElements>, arena_start);
pub(super) const NATIVE_OBJECT_EXTRAS_INLINE_VALUES_OFFSET: usize = std::mem::offset_of!(ObjectExtras, head)
    + std::mem::offset_of!(ObjectExtrasHead<DisplacedInline, super::ArrayElements>, inline_properties)
    + std::mem::offset_of!(DisplacedInline, 0);
pub(super) const NATIVE_OBJECT_INLINE_STORAGE_MASK: u32 = OBJECT_INLINE_STORAGE;
pub(super) const NATIVE_OBJECT_INLINE_FORM: u32 = crate::value_vec::INLINE_FORM;
pub(super) const NATIVE_OBJECT_NARROW_RUN_TAG: u64 = NARROW_RUN_TAG;
pub(super) const NATIVE_OBJECT_EXTRAS_FORM: u32 = crate::value_vec::EXTRAS_FORM;
pub(super) const NATIVE_OBJECT_ARRAY_MASK: u32 = OBJECT_ARRAY;
pub(super) const NATIVE_OBJECT_EXTRAS_TAG: u64 = EXTRAS_TAG;
pub(super) const NATIVE_OBJECT_EXTRAS_TAG_MASK: u64 = EXTRAS_TAG_MASK;
pub(super) const NATIVE_OBJECT_PROTO_MASK: u32 = PROTO_INDEX_BITS;
pub(super) const NATIVE_OBJECT_PROTO_RECORD_BASE: u32 = PROTO_OBJECT_RECORD_BASE;
pub(super) const NATIVE_OBJECT_PROTO_RECORD_END: u32 = PROTO_ARRAY_RECORD_BASE;

const _: () =
    assert!(NATIVE_OBJECT_META_OFFSET + std::mem::size_of::<u32>() <= NATIVE_OBJECT_RECORD_SIZE);
const _: () = assert!(
    NATIVE_OBJECT_AUXILIARY_OFFSET + std::mem::size_of::<u32>() <= NATIVE_OBJECT_RECORD_SIZE
);
const _: () =
    assert!(NATIVE_OBJECT_PAYLOAD_OFFSET + std::mem::size_of::<u64>() <= NATIVE_OBJECT_RECORD_SIZE);
const _: () = assert!(
    NATIVE_OBJECT_EXTRAS_ARENA_START_OFFSET + std::mem::size_of::<u32>()
        <= std::mem::size_of::<ObjectExtras>()
);

const _: () = assert!(NATIVE_OBJECT_EXTRAS_INLINE_VALUES_OFFSET
    + INLINE_PROPERTY_COUNT * std::mem::size_of::<Value>() <= std::mem::size_of::<ObjectExtras>());

impl Clone for Object {
    fn clone(&self) -> Self {
        // The word copies bit for bit; a copy with extras gets its own.
        let payload = match self.extras() {
            Some(extras) => Self::tag_extras(Box::new(extras.clone())),
            // An array's word is its element backing, which the object alone does not own
            // (`ArrayObject::clone` copies it).
            None if self.meta() & OBJECT_ARRAY != 0 => u64::from(EMPTY_POSITION),
            None => self.payload,
        };
        Self { head: ObjectHead { payload, layout: self.layout } }
    }
}

impl Drop for Object {
    fn drop(&mut self) {
        if let Some(extras) = self.extras_pointer() {
            // SAFETY: the pointer came from `Box::into_raw` in `extras_mut` and is owned by
            // this object alone.
            drop(unsafe { Box::from_raw(extras) });
        }
    }
}

/// An array: its object, which keeps named properties in the arena, and its dense element
/// backing. The backing has no field of its own: without extras, the object's payload word
/// is its header pointer (`ArrayElements::into_raw`), as an array without extras holds no
/// property store (`Object::set_storage_position`); with extras, they hold it
/// (`ObjectExtras::elements`). `OBJECT_ARRAY` tells the object's own readers which.
#[repr(C)]
pub struct ArrayObject {
    object: Object,
}

/// Native read-only layout facts for `ArrayObject`, kept beside its field declarations.
pub(super) const NATIVE_ARRAY_RECORD_SIZE: usize = std::mem::size_of::<ArrayObject>();
pub(super) const NATIVE_ARRAY_RECORD_ALIGN: usize = std::mem::align_of::<ArrayObject>();
/// Where an array record without extras keeps its backing's header pointer: its object's
/// payload word. With extras (`NATIVE_OBJECT_EXTRAS_TAG`), the word points at them, and
/// they hold it at `NATIVE_ARRAY_EXTRAS_ELEMENTS_OFFSET`.
pub(super) const NATIVE_ARRAY_ELEMENTS_OFFSET: usize =
    std::mem::offset_of!(ArrayObject, object) + NATIVE_OBJECT_PAYLOAD_OFFSET;
pub(super) const NATIVE_ARRAY_EXTRAS_ELEMENTS_OFFSET: usize = std::mem::offset_of!(ObjectExtras, head)
    + std::mem::offset_of!(ObjectExtrasHead<DisplacedInline, super::ArrayElements>, elements);
/// Where an array record's object part (`Object`: meta, auxiliary, payload) starts.
pub(super) const NATIVE_ARRAY_OBJECT_OFFSET: usize = std::mem::offset_of!(ArrayObject, object);
pub(super) const NATIVE_FUNCTION_RECORD_SIZE: usize = std::mem::size_of::<FunctionRecord>();
pub(super) const NATIVE_FUNCTION_OBJECT_OFFSET: usize = std::mem::offset_of!(FunctionRecord, object);

const _: () = assert!(
    NATIVE_ARRAY_ELEMENTS_OFFSET + std::mem::size_of::<super::ArrayElements>()
        <= NATIVE_ARRAY_RECORD_SIZE
);
const _: () = assert!(std::mem::size_of::<ArrayObject>() == 12);
const _: () = assert!(std::mem::size_of::<ArrayObject>() == std::mem::size_of::<Object>());
const _: () = assert!(NATIVE_ARRAY_ELEMENTS_OFFSET == NATIVE_ARRAY_OBJECT_OFFSET + NATIVE_OBJECT_PAYLOAD_OFFSET);
const _: () = assert!(
    NATIVE_ARRAY_EXTRAS_ELEMENTS_OFFSET + std::mem::size_of::<super::ArrayElements>()
        <= std::mem::size_of::<ObjectExtras>()
);

#[derive(Clone, Debug)]
pub enum CallSiteLocation {
    Source {
        file_name: String,
        line: u32,
        column: u32,
    },
    Native,
}

#[derive(Clone, Debug)]
pub enum CallSiteName {
    /// Resolve the safe debug name when a CallSite is read, not at capture.
    Callable(Value),
    Static(JsString),
    Anonymous,
}

#[derive(Clone, Debug)]
pub struct CallSiteRecord {
    pub(crate) location: CallSiteLocation,
    pub(crate) name: CallSiteName,
    pub(crate) this_value: Value,
    pub(crate) hide_identity: bool,
    pub(crate) is_constructor: bool,
    pub(crate) eval_origin: Option<JsString>,
}

impl CallSiteRecord {
    fn visit_roots(&self, visit: &mut impl FnMut(Value)) {
        visit(self.this_value);
        if let CallSiteName::Callable(function) = self.name {
            visit(function);
        }
    }
}

#[derive(Clone, Debug)]
pub enum StackData {
    Captured {
        records: Vec<CallSiteRecord>,
        source: Option<crate::bytecode::ProgramSource>,
    },
    Formatted(Value),
    CallSite(CallSiteRecord),
}

/// A prototype starts in setup mode; named native-info lookup makes its
/// accessor layout observable through getter-free diagnostic reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PrototypeLookupState {
    #[default]
    Ordinary,
    Setup,
    Ready,
}

/// An object's extras. The head native code reads comes first (`staged::object_extras`), and `Deref`
/// forwards its fields, so `extras.arena_start` and the rest read as before.
#[derive(Clone, Debug, Default)]
#[repr(C)]
struct ObjectExtras {
    head: ObjectExtrasHead<DisplacedInline, super::ArrayElements>,
    prototype_lookup: PrototypeLookupState,
    /// What has been recorded about the object's properties beside its shape; bits are set
    /// once and never cleared (`ElementFacts`).
    element_facts: ElementFacts,
    /// A prototype's subject in its heap's dependency table, which its layout and element
    /// changes retire (`Object::note_prototype_layout_change`).
    watch: super::dependencies::Watch,
    /// What few objects carry (`RareExtras`), behind one more pointer: most extras belong to
    /// arrays with named properties (typescript-lib's NodeArrays with `pos` and `end`), which
    /// carry none of it, and the whole set inline made each such array's extras 368 bytes.
    rare: Option<Box<RareExtras>>,
}

impl std::ops::Deref for ObjectExtras {
    type Target = ObjectExtrasHead<DisplacedInline, super::ArrayElements>;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.head
    }
}

impl std::ops::DerefMut for ObjectExtras {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.head
    }
}

use crate::staged::object_extras::ObjectExtrasHead;

/// The extras of exotic, branded, host and module objects (`ObjectExtras::rare`).
#[derive(Clone, Debug, Default)]
struct RareExtras {
    arguments_map: Option<Vec<u16>>,
    /// The depth of the lane activation whose window holds this arguments object, as its latest
    /// push recorded it (`Vm::record_window_owner`); stale once that activation ends, which its
    /// reader checks against the activation's own binding.
    window_depth: Option<u32>,
    brand: ObjectBrand,
    external_address: Option<usize>,
    boxed_primitive: Option<Value>,
    /// A function's creation realm when it differs from the VM's main realm (`FunctionRecord`
    /// keeps no realm of its own); `Some(Value::NULL)` is an explicit null realm.
    function_realm: Option<Value>,
    structured_serialization: StructuredSerialization,
    structured_transfer_forbidden: bool,
    module_bindings: Vec<(Atom, ProgramId, u16)>,
    deferred_module: Option<crate::ModuleSource>,
    stack_data: Option<StackData>,
    /// A global object's table of the values code read from it by name (`global_values`).
    global_values: Option<Box<global_values::GlobalValues>>,
}

impl ObjectExtras {
    #[inline] fn rare(&self) -> Option<&RareExtras> {
        self.rare.as_deref()
    }

    #[inline] fn rare_mut_existing(&mut self) -> Option<&mut RareExtras> {
        self.rare.as_deref_mut()
    }

    fn rare_mut(&mut self) -> &mut RareExtras {
        self.rare.get_or_insert_with(Box::default)
    }
}
/// How structured serialization treats an object (HTML [[Serializable]] and
/// [[Transferable]] host objects).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum StructuredSerialization {
    /// Serialized by its own properties or builtin data.
    #[default]
    Ordinary,
    /// Never serializable (host-marked uncloneable, or an external).
    Forbidden,
    /// Serializable only when listed for transfer (a MessagePort).
    TransferRequired,
}

impl ObjectExtras {
    fn allocated_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.rare().map_or(0, RareExtras::allocated_bytes)
    }
}

impl RareExtras {
    fn allocated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .arguments_map
                .as_ref()
                .map_or(0, |mapping| mapping.capacity() * std::mem::size_of::<u16>())
            + self.module_bindings.capacity() * std::mem::size_of::<(Atom, ProgramId, u16)>()
            + self.deferred_module.as_ref().map_or(0, |module| {
                module.name.capacity()
                    + module.source.len()
                    + module.original_bytes.as_ref().map_or(0, |bytes| bytes.len())
            })
            + self
                .stack_data
                .as_ref()
                .map_or(0, StackData::allocated_bytes)
    }
}
impl StackData {
    fn allocated_bytes(&self) -> usize {
        fn record_bytes(record: &CallSiteRecord) -> usize {
            (match &record.location {
                CallSiteLocation::Source { file_name, .. } => file_name.capacity(),
                CallSiteLocation::Native => 0,
            }) + match &record.name {
                CallSiteName::Static(name) => name.capacity(),
                _ => 0,
            }
        }
        match self {
            Self::Captured { records, .. } => {
                records.capacity() * std::mem::size_of::<CallSiteRecord>()
                    + records.iter().map(record_bytes).sum::<usize>()
            }
            Self::CallSite(record) => record_bytes(record),
            Self::Formatted(_) => 0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivateBrand {
    pub home: Value,
    pub name: Atom,
}

#[derive(Clone, Debug)]
pub(crate) struct FinalizationEntry {
    pub target: WeakHandle,
    pub held: Value,
    pub token: Option<WeakHandle>,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct FinalizationEntries(pub(crate) Vec<FinalizationEntry>);
impl std::ops::Deref for FinalizationEntries {
    type Target = Vec<FinalizationEntry>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for FinalizationEntries {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
/// An object is its header word and one payload word.
const _: () = assert!(std::mem::size_of::<Object>() == 12);
/// The payload word holds every inline slot as a narrow value.
const _: () = assert!(INLINE_PROPERTY_COUNT * u32::BITS as usize == u64::BITS as usize);

/// Internal methods installed by ProxyCreate, retained after target release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProxyKind {
    Object,
    Callable,
    Constructor,
}

/// One environment slot in one word: a value the environment owns, or the direct owner of
/// this slot index (`Value::shared_environment_slot`), whose slot is always owned.
#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub(crate) struct EnvironmentSlot(Value);

/// What the code that can still run reads of an environment slot (`EnvironmentSlots::clear_unread`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SlotUse {
    /// Its value.
    Value,
    /// Only whether it is initialized: a projection, whose reads go through it.
    Initialization,
    /// Nothing.
    Unread,
}

/// What an `EnvironmentSlot` holds.
pub(crate) enum EnvironmentSlotState {
    Owned(Value),
    Shared(Value),
}

impl EnvironmentSlot {
    #[inline]
    pub(crate) fn owned(value: Value) -> Self {
        debug_assert!(value.shared_environment_owner().is_none());
        Self(value)
    }

    pub(crate) fn shared(owner: Value) -> Self {
        Self(Value::shared_environment_slot(owner))
    }

    #[inline(always)]
    pub(crate) fn state(self) -> EnvironmentSlotState {
        match self.0.shared_environment_owner() {
            Some(owner) => EnvironmentSlotState::Shared(owner),
            None => EnvironmentSlotState::Owned(self.0),
        }
    }

    /// The owned value, for writing in place.
    pub(crate) fn owned_mut(&mut self) -> Option<&mut Value> {
        self.0.shared_environment_owner().is_none().then_some(&mut self.0)
    }
}

/// An environment: a function activation's captured bindings (`slots`), its parent's link,
/// its function and its scope. A record of its own family (`Value::direct_environment`).
#[derive(Debug)]
pub struct EnvironmentRecord {
    pub parent: Value,
    pub function: u32,
    pub slots: EnvironmentSlots,
    pub scope: EnvironmentScopeRef,
}

// Size pins. The record is 32 bytes: parent, slots and scope are one word each, and the u32
// `function` leaves 4 bytes of padding to the 8-byte alignment. Dropping `function` from the
// record would make it 24 bytes. A dense slot storage of `n` slots is `24 + 8n` bytes: a
// 16-byte prefix, the 8-byte header, then the slots. A change to any of these is a deliberate
// memory-layout change.
const _: () = assert!(std::mem::size_of::<EnvironmentRecord>() == 32);
const _: () = assert!(std::mem::size_of::<EnvironmentSlots>() == std::mem::size_of::<usize>());
const _: () = assert!(std::mem::size_of::<EnvironmentScopeRef>() == std::mem::size_of::<usize>());

/// An environment's provenance and dynamic-scope state: consulted for eval, `with` and
/// name resolution, while slot reads only need `parent`, `function` and `slots`.
#[derive(Debug, Clone)]
pub struct EnvironmentScope {
    pub program: Option<u32>,
    pub root_eval_scope: bool,
    // A captured lexical scope selects its names from the owning function's
    // binding-site table; slot values remain shared with that activation.
    pub binding_site_pc: Option<u32>,
    pub dynamic_bindings: EnvironmentBindings,
    pub with_objects: Box<[Value]>,
}

impl EnvironmentScope {
    /// Whether this scope names its program and nothing else, as a function activation's does.
    fn is_program_only(&self) -> bool {
        !self.root_eval_scope
            && self.binding_site_pc.is_none()
            && matches!(&self.dynamic_bindings, EnvironmentBindings::Owned(bindings) if bindings.is_empty())
            && self.with_objects.is_empty()
    }
}

/// An environment's `EnvironmentScope` in one word. Most environments are a function's
/// activation, whose scope names only its program: that scope is the word itself, tagged by
/// `INLINE_SCOPE_TAG`, and needs no allocation. Any other scope is a box the word points to.
pub struct EnvironmentScopeRef(std::num::NonZeroUsize);

/// The low bit, never set in a box pointer, marks an inline program-only scope.
const INLINE_SCOPE_TAG: usize = 1;
/// An inline scope holds its program plus one (zero for none) above the tag bit.
const INLINE_PROGRAM_SHIFT: u32 = INLINE_SCOPE_TAG.trailing_ones();
const _: () = assert!(align_of::<EnvironmentScope>() > INLINE_SCOPE_TAG);
static NO_DYNAMIC_BINDINGS: EnvironmentBindings = EnvironmentBindings::Owned(Vec::new());

impl EnvironmentScopeRef {
    pub fn new(scope: EnvironmentScope) -> Self {
        if scope.is_program_only()
            && let Some(inline) = Self::inline(scope.program)
        {
            return inline;
        }
        Self::boxed(scope)
    }

    /// The scope of a function's activation, which names only its `program`: `new` of that
    /// scope, without building it first.
    pub fn of_program(program: u32) -> Self {
        Self::inline(Some(program)).unwrap_or_else(|| {
            Self::boxed(EnvironmentScope {
                program: Some(program),
                root_eval_scope: false,
                binding_site_pc: None,
                dynamic_bindings: EnvironmentBindings::Owned(Vec::new()),
                with_objects: Box::default(),
            })
        })
    }

    fn inline(program: Option<u32>) -> Option<Self> {
        let payload = program.map_or(Some(0), |program| usize::try_from(program).ok()?.checked_add(1))?;
        (payload.leading_zeros() >= INLINE_PROGRAM_SHIFT).then(|| {
            Self(
                std::num::NonZeroUsize::new(payload << INLINE_PROGRAM_SHIFT | INLINE_SCOPE_TAG)
                    .expect("the tag bit is set"),
            )
        })
    }

    fn boxed(scope: EnvironmentScope) -> Self {
        let pointer = Box::into_raw(Box::new(scope));
        Self(std::num::NonZeroUsize::new(pointer as usize).expect("a box is never null"))
    }

    #[inline(always)]
    fn scope(&self) -> Option<&EnvironmentScope> {
        // SAFETY: an untagged word is the pointer `boxed` leaked, owned by `self` until `Drop`.
        (self.0.get() & INLINE_SCOPE_TAG == 0)
            .then(|| unsafe { &*(self.0.get() as *const EnvironmentScope) })
    }

    #[inline(always)]
    pub fn program(&self) -> Option<u32> {
        match self.scope() {
            Some(scope) => scope.program,
            None => (self.0.get() >> INLINE_PROGRAM_SHIFT)
                .checked_sub(1)
                .map(|program| u32::try_from(program).expect("inline programs fit u32")),
        }
    }

    pub(crate) fn root_eval_scope(&self) -> bool {
        self.scope().is_some_and(|scope| scope.root_eval_scope)
    }

    pub(crate) fn binding_site_pc(&self) -> Option<u32> {
        self.scope().and_then(|scope| scope.binding_site_pc)
    }

    pub(crate) fn dynamic_bindings(&self) -> &EnvironmentBindings {
        self.scope().map_or(&NO_DYNAMIC_BINDINGS, |scope| &scope.dynamic_bindings)
    }

    pub(crate) fn with_objects(&self) -> &[Value] {
        self.scope().map_or(&[], |scope| &scope.with_objects)
    }

    /// The scope, boxed first when it was inline.
    pub(crate) fn make_mut(&mut self) -> &mut EnvironmentScope {
        if self.scope().is_none() {
            *self = Self::boxed(self.to_scope());
        }
        // SAFETY: `self` now holds the box `boxed` leaked, and `&mut self` borrows it uniquely.
        unsafe { &mut *(self.0.get() as *mut EnvironmentScope) }
    }

    pub(crate) fn to_scope(&self) -> EnvironmentScope {
        self.scope().cloned().unwrap_or_else(|| EnvironmentScope {
            program: self.program(),
            root_eval_scope: false,
            binding_site_pc: None,
            dynamic_bindings: EnvironmentBindings::Owned(Vec::new()),
            with_objects: Box::default(),
        })
    }
}

impl Drop for EnvironmentScopeRef {
    fn drop(&mut self) {
        if self.scope().is_some() {
            // SAFETY: the untagged word is the box `boxed` leaked, dropped exactly once here.
            drop(unsafe { Box::from_raw(self.0.get() as *mut EnvironmentScope) });
        }
    }
}

impl Clone for EnvironmentScopeRef {
    fn clone(&self) -> Self {
        match self.scope() {
            Some(scope) => Self::boxed(scope.clone()),
            None => Self(self.0),
        }
    }
}

impl std::fmt::Debug for EnvironmentScopeRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.to_scope().fmt(formatter)
    }
}

#[derive(Clone, Debug)]
/// An environment's slots behind a one-word pointer, which keeps the environment cell within
/// the common cell size.
///
/// Storage is dense, one `EnvironmentSlot` per slot, or sparse for a per-iteration copy of a
/// wide environment (`SPARSE_CLONE_MIN_SLOTS`): `[environment_index(len), shared(owner),
/// (environment_index(slot), slot)...]`. The listed slots are those that differ from sharing
/// with the default owner, sorted by index; every other slot is shared with that owner. The
/// slice header's mark tells the two apart, so a dense read tests one bit it loads anyway.
#[repr(transparent)]
pub struct EnvironmentSlots(pub(super) super::thin_slice::ThinBoxSlice<EnvironmentSlot, ENVIRONMENT_PREFIX_WORDS>);

/// Words an `EnvironmentSlots::lay_out_in_place` storage takes before its slots.
pub const IN_PLACE_SLOTS_WORDS: usize =
    super::thin_slice::ThinBoxSlice::<EnvironmentSlot, ENVIRONMENT_PREFIX_WORDS>::IN_PLACE_WORDS;

/// Words before an environment's slot storage header (`ThinBoxSlice`'s prefix): the capture
/// link (`EnvironmentSlots::link`). A storage laid out in place has none it may read.
pub(crate) const ENVIRONMENT_PREFIX_WORDS: usize = 2;

/// A per-iteration copy of an environment with at least this many slots stores only the
/// slots it does not share with one owner. A module root with thousands of bindings would
/// otherwise copy every one into each loop iteration's environment a closure keeps; smaller
/// environments stay dense, whose slot reads need no search.
pub(crate) const SPARSE_CLONE_MIN_SLOTS: usize = 64;
/// Where a sparse storage's listed pairs start: after its length and default owner.
const SPARSE_ENTRIES_START: usize = 2;
/// Words per listed sparse slot: its index, then the slot.
const SPARSE_ENTRY_WORDS: usize = 2;

#[derive(Clone, Debug)]
pub enum EnvironmentBindings {
    Owned(Vec<(Atom, Value)>),
    Shared(Value),
}

impl From<Vec<(Atom, Value)>> for EnvironmentBindings {
    fn from(bindings: Vec<(Atom, Value)>) -> Self {
        Self::Owned(bindings)
    }
}

/// An iterator's protocol caches, helper state and generator record: needed by
/// helpers, wrapped iterators and generators, while built-in stepping only reads
/// `source`, `kind`, `index` and `done`.
#[derive(Debug, Clone, Default)]
pub struct IteratorExt {
    /// Upper half of the iterator position; nonzero only for array-likes iterated past
    /// 2^32 elements, whose lengths reach 2^53 - 1.
    pub(crate) position_high: u32,
    pub(crate) next_method: Option<Value>,
    pub(crate) helper: Option<Box<IteratorHelper>>,
    /// Where `helper` is in its life; the record's `done` ends it.
    pub(crate) helper_phase: HelperPhase,
    pub generator: Option<Box<crate::vm::activation::GeneratorRecord>>,
}

impl IteratorExt {
    /// The full position whose low half is the cell's `index`.
    #[inline]
    pub fn position(&self, low: u32) -> usize {
        ((u64::from(self.position_high) << u32::BITS) | u64::from(low)) as usize
    }

    /// Splits `position` between the cell's `index` and this box.
    #[inline]
    pub fn store_position(&mut self, low: &mut u32, position: usize) {
        let position = position as u64;
        *low = position as u32;
        self.position_high = (position >> u32::BITS) as u32;
    }
}

/// A Map or a Set: its ordinary object and its entries. The heap keeps it in the collection
/// record family (`Heap::map_entries`, `Heap::set_entries`).
#[derive(Clone, Debug)]
pub(crate) struct CollectionRecord {
    pub(crate) object: Object,
    pub(crate) entries: CollectionStorage,
}

/// The entries of a `CollectionRecord`: a Map's key-value pairs or a Set's values. Most
/// collections stay empty (jsdom-d3: 107K of 132K Maps, 152K of 153K Sets), so the record
/// holds the entries inline beside its object.
#[derive(Clone, Debug)]
pub enum CollectionStorage {
    Map(super::Entries<(Value, Value)>),
    Set(super::Entries<Value>),
}

impl CollectionStorage {
    /// The entry storage this collection owns, which allocation and live weight charge
    /// (`Cell::payload_bytes`).
    pub(crate) fn payload_bytes(&self) -> usize {
        match self {
            Self::Map(entries) => entries.payload_bytes(),
            Self::Set(entries) => entries.payload_bytes(),
        }
    }
}

impl CollectionRecord {
    /// The kind a Map or Set reports as.
    pub(crate) fn cell_kind(&self) -> super::CellKind {
        match self.entries {
            CollectionStorage::Map(_) => super::CellKind::Map,
            CollectionStorage::Set(_) => super::CellKind::Set,
        }
    }
}

/// An iterator object: built-in iterators, iterator helpers and generators. The heap keeps
/// it in the iterator record family (`Heap::iterator`).
#[derive(Clone, Debug)]
pub struct IteratorRecord {
    pub object: Object,
    pub source: Value,
    pub kind: IteratorKind,
    /// Low half of the position within `source`; `IteratorExt::position` joins it with
    /// the upper half.
    pub index: u32,
    pub done: bool,
    pub ext: IteratorExt,
}

impl IteratorRecord {
    /// A suspended generator's program, which stays alive with it.
    pub(crate) fn program_references(&self, mut visit: impl FnMut(ProgramId)) {
        if let Some(continuation) = self
            .ext
            .generator
            .as_ref()
            .and_then(|generator| generator.continuation())
        {
            visit(continuation.program());
        }
    }
}

/// A function: its ordinary object, creation realm, code and closure environment. The
/// heap keeps it in the function record family (`Heap::function`), where a dead closure
/// costs its occupancy bit; a heap whose record range is unavailable boxes it in a
/// `Cell::Function` slot. Its layout is `staged::record_heads::FunctionRecord`, instantiated with the
/// runtime's `Object`, `FunctionKind` and `Value`, so its fields are named there.
pub type FunctionRecord = crate::staged::record_heads::FunctionRecord<Object, FunctionKind, Value>;

const _: () = assert!(size_of::<FunctionRecord>() == 32);

impl FunctionRecord {
    /// The program whose code this function runs, which keeps that program alive.
    pub(crate) fn program(&self) -> Option<ProgramId> {
        match self.kind {
            FunctionKind::User(program, _) => Some(program),
            FunctionKind::Native(_) => None,
        }
    }

    /// The creation realm this function carries when it is not the VM's main realm; `None`
    /// means the main realm (`Vm::record_realm`).
    pub(crate) fn realm_override(&self) -> Option<Value> {
        self.object.function_realm_override()
    }

    /// The record with `realm` stored as its creation realm, whatever `realm` is (`Value::NULL`
    /// included). `Vm::alloc_function_in` stores one only off the main realm.
    pub(crate) fn with_realm(mut self, realm: Value) -> Self {
        self.object.set_function_realm_override(realm);
        self
    }
}

/// A typed array's or DataView's ordinary object and byte offset into its buffer, boxed
/// together so the view cell keeps four words. The native prefix comes first: a typed array's
/// table entry (`TypedViewTable`) addresses it, and `NATIVE_TYPED_ARRAY_LAYOUT` names its fields.
#[derive(Clone, Debug)]
#[repr(C)]
pub(crate) struct BufferViewObject {
    pub(crate) native: super::typed_view::NativeView,
    pub(crate) object: Object,
    pub(crate) offset: usize,
}

/// Where the native prefix starts in a view record: the record's address itself.
pub(super) const NATIVE_VIEW_OFFSET: usize = std::mem::offset_of!(BufferViewObject, native);

impl std::ops::Deref for BufferViewObject {
    type Target = Object;
    fn deref(&self) -> &Object {
        &self.object
    }
}

impl std::ops::DerefMut for BufferViewObject {
    fn deref_mut(&mut self) -> &mut Object {
        &mut self.object
    }
}


/// A PlainDateTime's ISO year, month and day with its hour-through-nanosecond fields, boxed
/// together so the cell keeps four words.
#[derive(Clone, Copy, Debug)]
pub struct IsoDateTimeFields {
    pub date: (i32, u32, u32),
    pub time: [u32; 6],
}

/// A ZonedDateTime's exact time and time-zone identifier, boxed together so the cell keeps
/// four words.
#[derive(Clone, Debug)]
pub struct ZonedInstant {
    pub epoch_nanoseconds: i128,
    pub time_zone: String,
}

/// A RegExp's source text, flags, legacy-constructor owner and compiled matcher: all fixed
/// at creation and replaced together by `RegExp.prototype.compile`.
#[derive(Clone)]
pub(crate) struct RegExpMeta {
    pub(crate) source: JsString,
    pub(crate) flags: crate::vm::RegExpFlags,
    // One constructor identity owns creation realm and legacy eligibility.
    pub(crate) legacy_constructor: RegExpLegacyOwner,
    pub(crate) matcher: RegExpMatcher,
}

/// A RegExp's compiled matcher, or none yet: a RegExp restored from a snapshot compiles its
/// pattern on first use (`Vm::regexp_matcher`). A snapshot holds every pattern its builtins
/// create, which a run seldom executes; compiling them all was 5% of restoring the preloaded
/// startup snapshot (x86_64, 2026-10-22).
#[derive(Clone, Default)]
pub(crate) struct RegExpMatcher(pub(crate) std::cell::OnceCell<Rc<quench_regexp::Regex>>);

impl RegExpMatcher {
    pub(crate) fn compiled(matcher: Rc<quench_regexp::Regex>) -> Self {
        Self(std::cell::OnceCell::from(matcher))
    }
}

impl std::fmt::Debug for RegExpMeta {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RegExpMeta")
            .field("source", &self.source)
            .field("flags", &self.flags)
            .field("legacy_constructor", &self.legacy_constructor)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum RegExpLegacyOwner {
    Enabled(Value),
    Disabled(Value),
}
impl RegExpLegacyOwner {
    pub(crate) fn constructor(self) -> Value {
        match self {
            Self::Enabled(owner) | Self::Disabled(owner) => owner,
        }
    }
}

/// Native host-private keys remain symbol primitives, but do not participate in
/// ordinary property reflection, inheritance, or proxy interception.
#[derive(Clone, Debug)]
pub struct SymbolData {
    // Descriptions are immutable; a slice avoids String capacity metadata so
    // the private-kind tag fits without growing every arena cell.
    pub(crate) description: Option<Box<str>>,
    pub(crate) kind: SymbolKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SymbolKind {
    Public,
    HostPrivate,
}
impl SymbolData {
    pub fn public(description: Option<String>) -> Self {
        Self {
            description: description.map(String::into_boxed_str),
            kind: SymbolKind::Public,
        }
    }
    pub(crate) fn host_private(description: Option<String>) -> Self {
        Self {
            description: description.map(String::into_boxed_str),
            kind: SymbolKind::HostPrivate,
        }
    }
}
impl std::ops::Deref for SymbolData {
    type Target = Option<Box<str>>;
    fn deref(&self) -> &Self::Target {
        &self.description
    }
}
