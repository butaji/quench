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
pub(crate) use cell_variants::*;
mod environment_slots;
mod native_dispatch;
mod native_kinds;
mod object_layout;
#[cfg(test)]
mod object_payload_tests;
mod object_state;
mod object_storage;
pub(crate) use await_state::{AwaitKind, AwaitRole, AwaitState};
pub(crate) use wasm_fields::{TableTarget, TableWords, WasmGcFields, WasmTableElements};
pub(crate) use typed_array_kind::{TYPED_ARRAYS, TypedArrayKind, TypedArrayRow, for_each_typed_array};
pub(crate) use modes::{ElementFacts, HelperPhase, ObjectBrand, ViewLength};
pub(crate) use prototype_values::{PrototypeValueWatch, subscribe_prototype_values, watch_prototype_values};
pub(crate) use global_values::{GlobalCell, GlobalFact, GlobalLayoutFact, global_layout_fact, global_value_fact, note_global_shadowed, note_global_write, subscribe_global_layout, subscribe_global_value};
#[cfg(test)]
pub(crate) use global_values::new_table;

/// Changes whenever the layout an object used as a prototype offers its inheritors can have
/// changed: its shape, prototype or integrity changes; a collection frees a prototype, whose
/// record may then be reused (`Heap::retire_dead_prototypes`); or shape compaction renumbers
/// the shapes proofs name. A cache that proved a prototype chain under one value
/// proves it again while the value holds (`FieldAddCache::proved_epoch`). One counter for the
/// process: a change another heap made only sends caches back to the walk that re-proves them.
static PROTOTYPE_LAYOUT_EPOCH: AtomicU64 = AtomicU64::new(0);

/// The current `PROTOTYPE_LAYOUT_EPOCH`.
#[inline(always)]
pub(crate) fn prototype_layout_epoch() -> u64 {
    PROTOTYPE_LAYOUT_EPOCH.load(Ordering::Relaxed)
}

/// Retires every proof made under the current `PROTOTYPE_LAYOUT_EPOCH`, and all code that
/// holds a prototype's value as a constant: a layout change moves what a slot holds.
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
}

/// A global's cold facts, boxed with its object part so the cell stays compact.
#[derive(Clone, Debug)]
pub(crate) struct WasmGlobalHeader {
    pub(crate) object: Object,
    pub(crate) declarations: crate::WasmTypes,
}

/// A memory's cold facts, boxed with its object part so the cell stays compact.
#[derive(Clone, Debug)]
pub(crate) struct WasmMemoryHeader {
    pub(crate) object: Object,
    pub(crate) ty: wasmparser::MemoryType,
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

pub(crate) enum FunctionKind {
    User(ProgramId, u32),
    Native(Native),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IteratorKind {
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
pub(crate) enum IteratorHelper {
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
pub(crate) enum IteratorConsumer {
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
pub(crate) struct InlineObjectHeader {
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

    pub(crate) fn of(proto: Value, shape: u32) -> Self {
        Self::of_storage(proto, ValueVec::inline_property_storage(shape))
    }

    /// The header words of an object with prototype `proto` whose `shape`'s values live in an
    /// arena run (`Object::from_run_header`), as `Object::with_property_storage` sets them
    /// apart from the run's position.
    pub(crate) fn of_run(proto: Value, shape: u32) -> Self {
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
pub(crate) struct Object {
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
/// Cells a heap may hold so that any of them can be a prototype (`staged::object_encoding`).
pub(crate) use crate::staged::object_encoding::{MAX_HEAP_CELLS, PROTO_ARRAY_RECORD_BASE, PROTO_INDEX_BITS, PROTO_OBJECT_RECORD_BASE};
const PROTO_FUNCTION_RECORD_BASE: u32 = PROTO_ARRAY_RECORD_BASE + RECORD_SPACE_CAPACITY as u32;
const PROTO_PROMISE_RECORD_BASE: u32 = PROTO_FUNCTION_RECORD_BASE + RECORD_SPACE_CAPACITY as u32;
const PROTO_ITERATOR_RECORD_BASE: u32 = PROTO_PROMISE_RECORD_BASE + RECORD_SPACE_CAPACITY as u32;
const PROTO_RECORD_END: u32 = PROTO_ITERATOR_RECORD_BASE + RECORD_SPACE_CAPACITY as u32;
const _: () = assert!(PROTO_RECORD_END <= PROTO_UNDEFINED);
const _: () = assert!(PROTO_OBJECT_RECORD_BASE.is_multiple_of(OBJECT_PROTO_WINDOW as u32));
const _: () = assert!(PROTO_ARRAY_RECORD_BASE.is_multiple_of(ARRAY_PROTO_WINDOW as u32));
const _: () = assert!(PROTO_FUNCTION_RECORD_BASE.is_multiple_of(FUNCTION_PROTO_WINDOW as u32));
const _: () = assert!(PROTO_PROMISE_RECORD_BASE.is_multiple_of(PROMISE_PROTO_WINDOW as u32));
const _: () = assert!(PROTO_ITERATOR_RECORD_BASE.is_multiple_of(ITERATOR_PROTO_WINDOW as u32));
const _: () = assert!(PROTO_COLLECTION_RECORD_BASE.is_multiple_of(COLLECTION_PROTO_WINDOW as u32));
const _: () = assert!(LEGACY_SLOT_CAPACITY <= PROTO_ARRAY_RECORD_BASE as usize);

/// An object's encoded prototype reference (`PROTO_INDEX_BITS`). Equal links name the same
/// prototype, so a chain walk compares links instead of decoding each to a `Value`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct ProtoLink(u32);

impl ProtoLink {
    pub(crate) const NULL: Self = Self(PROTO_NULL);

    /// The link naming `prototype`, an object or null (`Object::encode_proto`).
    #[inline(always)]
    pub(crate) fn of(prototype: Value) -> Self {
        Self(Object::encode_proto(prototype))
    }

    #[cfg(test)]
    pub(crate) fn from_bits(bits: u32) -> Self { Self(bits) }

    /// The link as the object header encodes it (`OBJECT.proto_mask`).
    #[inline(always)]
    pub(crate) fn bits(self) -> u32 {
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
    pub(crate) fn object_record_id(self) -> Option<u32> {
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
            index if index < PROTO_ARRAY_RECORD_BASE => Value::heap(index),
            index if index < PROTO_OBJECT_RECORD_BASE => {
                Value::direct_array(index - PROTO_ARRAY_RECORD_BASE)
            }
            index if index < PROTO_FUNCTION_RECORD_BASE => {
                Value::direct_object(index - PROTO_OBJECT_RECORD_BASE)
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
pub(crate) enum PlainPropertyStorage {
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
pub(super) const NATIVE_OBJECT_PROTO_RECORD_END: u32 = PROTO_FUNCTION_RECORD_BASE;

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
pub(crate) struct ArrayObject {
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
pub(crate) enum CallSiteLocation {
    Source {
        file_name: String,
        line: u32,
        column: u32,
    },
    Native,
}

#[derive(Clone, Debug)]
pub(crate) enum CallSiteName {
    /// Resolve the safe debug name when a CallSite is read, not at capture.
    Callable(Value),
    Static(JsString),
    Anonymous,
}

#[derive(Clone, Debug)]
pub(crate) struct CallSiteRecord {
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
pub(crate) enum StackData {
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
pub(crate) struct PrivateBrand {
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
impl Object {
    /// An object of header words `meta` and `auxiliary` (`layout`) and `payload`.
    #[inline(always)]
    fn from_header(meta: u32, auxiliary: u32, payload: u64) -> Self {
        Self { payload, layout: super::layouts::intern(header_word(meta, auxiliary)) }
    }

    /// The header words as one: the auxiliary word low, which shape tests read most, and
    /// `meta` high (`header_word`).
    #[inline(always)]
    fn header(&self) -> u64 {
        super::layouts::word(self.layout)
    }

    #[inline(always)]
    fn meta(&self) -> u32 {
        (self.header() >> u32::BITS) as u32
    }

    #[inline(always)]
    fn aux(&self) -> u32 {
        self.header() as u32
    }

    /// The interned layout id (`heap::layouts`): equal ids mean equal prototype links, flags,
    /// shapes and storage forms, so one compare proves what comparing each would.
    #[inline(always)]
    pub(crate) fn layout_id(&self) -> u32 {
        self.layout
    }

    /// Sets both header words: the layout of the pair.
    #[inline(always)]
    fn set_header(&mut self, meta: u32, auxiliary: u32) {
        let word = header_word(meta, auxiliary);
        if word != self.header() {
            self.layout = super::layouts::intern(word);
        }
    }

    #[inline(always)]
    fn set_meta(&mut self, meta: u32) {
        let auxiliary = self.aux();
        self.set_header(meta, auxiliary);
    }

    #[inline(always)]
    pub(super) fn set_aux(&mut self, auxiliary: u32) {
        let meta = self.meta();
        self.set_header(meta, auxiliary);
    }

    #[inline(always)]
    pub(crate) fn new(proto: Value) -> Self {
        // `with_property_storage` of empty inline storage, written out as `with_inline_values`
        // does: one header word, one intern.
        let object = Self::with_inline_values(proto, 0, &[]).expect("undefined is narrow");
        debug_assert!({
            let slow = std::mem::ManuallyDrop::new(Self::with_property_storage(proto, ValueVec::inline_property_storage(0), [Value::UNDEFINED; INLINE_PROPERTY_COUNT]));
            slow.layout == object.layout && { slow.payload } == { object.payload }
        });
        object
    }

    /// A direct object, the usual prototype, is tested first: its record id and its range's
    /// base alone, before the decode of every other reference family.
    #[inline(always)]
    fn encode_proto(proto: Value) -> u32 {
        match proto.direct_object_index() {
            Some(id) => PROTO_OBJECT_RECORD_BASE + id,
            None => Self::encode_other_proto(proto),
        }
    }

    /// `encode_proto` of a direct array, as every realm's `Array.prototype` is.
    #[inline(always)]
    fn encode_array_proto(proto: Value) -> u32 {
        match proto.direct_array_index() {
            Some(id) => PROTO_ARRAY_RECORD_BASE + id,
            None => Self::encode_proto(proto),
        }
    }

    #[inline]
    fn encode_other_proto(proto: Value) -> u32 {
        match proto.heap_reference() {
            Some(HeapReference::LegacySlot(index)) => {
                assert!(
                    (index as usize) < MAX_HEAP_CELLS,
                    "heap exhausted the prototype index space"
                );
                index
            }
            Some(HeapReference::ObjectRecord(id)) => PROTO_OBJECT_RECORD_BASE + id,
            Some(HeapReference::ArrayRecord(id)) => PROTO_ARRAY_RECORD_BASE + id,
            Some(HeapReference::FunctionRecord(id)) => PROTO_FUNCTION_RECORD_BASE + id,
            Some(HeapReference::PromiseRecord(id)) => PROTO_PROMISE_RECORD_BASE + id,
            Some(HeapReference::IteratorRecord(id)) => PROTO_ITERATOR_RECORD_BASE + id,
            Some(HeapReference::AwaitRecord(_)) => unreachable!("an await state is never a prototype"),
            Some(HeapReference::StringRecord(_)) => unreachable!("a string is never a prototype"),
            Some(HeapReference::V128Record(_)) => unreachable!("a Wasm vector is never a prototype"),
            Some(HeapReference::EnvironmentRecord(_)) => unreachable!("an environment is never a prototype"),
            None if proto.is_null() => PROTO_NULL,
            None => {
                debug_assert!(proto.is_undefined(), "a prototype is an object or null");
                PROTO_UNDEFINED
            }
        }
    }





    /// The prototype reference as stored (`ProtoLink`), undecoded.
    #[inline(always)]
    pub(crate) fn proto_link(&self) -> ProtoLink {
        ProtoLink(self.meta() & PROTO_INDEX_BITS)
    }

    #[inline(always)]
    pub(crate) fn proto(&self) -> Value {
        self.proto_link().prototype()
    }

    pub(crate) fn set_proto(&mut self, proto: Value) {
        self.note_constant_write();
        self.note_prototype_layout_change();
        self.set_meta((self.meta() & !PROTO_INDEX_BITS) | Self::encode_proto(proto));
    }

    /// A change to what this object offers its inheritors, if it has any.
    #[inline(always)]
    fn note_prototype_layout_change(&self) {
        if self.was_used_as_prototype() {
            advance_prototype_layout_epoch();
            self.retire_watch();
        }
    }

    /// Retires the proofs that read this prototype's elements (`Heap::note_prototype_elements`).
    #[inline(always)]
    pub(crate) fn note_prototype_elements_change(&self) {
        if self.was_used_as_prototype() {
            self.retire_watch();
        }
    }

    fn retire_watch(&self) {
        if let Some(extras) = self.extras() {
            extras.watch.retire();
        }
    }

    /// This prototype's subject in its heap's dependency table (`Heap::mark_prototype`).
    pub(crate) fn watch_as(&mut self, watch: super::dependencies::Watch) {
        self.extras_mut().watch = watch;
    }

    /// This watched prototype's subject, if a heap's dependency table gave it one.
    pub(crate) fn watched_subject(&self) -> Option<u32> {
        self.extras().and_then(|extras| extras.watch.subject())
    }

    /// Whether this object is an array's, which keeps named properties in the arena.
    pub(crate) fn is_array(&self) -> bool {
        self.meta() & OBJECT_ARRAY != 0
    }

    /// The property storage descriptor, assembled from the header and, without inline
    /// storage, the payload.
    #[inline(always)]
    pub(crate) fn properties(&self) -> ValueVec {
        let integrity = self.meta() & VALUE_VEC_INTEGRITY_BITS;
        let position = if self.meta() & OBJECT_INLINE_STORAGE != 0 {
            ValueVec::inline_property_storage(0).into_parts().0 & VALUE_VEC_START_BITS
        } else {
            self.storage_position()
        };
        ValueVec::from_parts(integrity | position, self.aux())
    }

    /// Stores `properties`. A change from inline to other storage overwrites the payload
    /// word that held an inline value, so callers move inline values out first.
    pub(crate) fn set_properties(&mut self, properties: ValueVec, arena: &mut ValueArena) {
        arena.note_shape_transition(self.shape(), properties.auxiliary());
        self.initialize_properties(properties);
    }

    /// Construction before publication has no shape dependents.
    #[inline(always)] fn initialize_properties(&mut self, properties: ValueVec) {
        let (start, auxiliary) = properties.into_parts();
        // Exotic access is a fact of the object, not of the storage that replaces its own.
        // A storage change ends the object's constancy (`note_constant_write`): its readers
        // retire here, and the new word carries no mark.
        self.note_constant_write();
        let (meta, current) = (self.meta(), self.aux());
        let auxiliary = (auxiliary & !crate::value_vec::CONSTANT_FORM) | (current & crate::value_vec::EXOTIC_ACCESS);
        let flags = meta & !(VALUE_VEC_INTEGRITY_BITS | OBJECT_INLINE_STORAGE);
        let position = start & VALUE_VEC_START_BITS;
        if properties.has_inline_property_storage() {
            let meta = flags | (start & VALUE_VEC_INTEGRITY_BITS) | OBJECT_INLINE_STORAGE;
            // Inline storage leaves the payload word alone, and its form is decided here.
            let extras = if self.extras_pointer().is_some() { crate::value_vec::EXTRAS_FORM } else { 0 };
            let form = OBJECT_INLINE_STORAGE | extras;
            self.set_header(meta, (auxiliary & !crate::value_vec::STORAGE_FORM) | form);
        } else {
            self.set_header(flags | (start & VALUE_VEC_INTEGRITY_BITS), auxiliary);
            self.set_storage_position(position);
            if properties.is_narrow() {
                // Only a plain object's values take a narrow run: its payload is the position.
                debug_assert!(self.extras_pointer().is_none() && self.meta() & OBJECT_ARRAY == 0);
                self.payload |= NARROW_RUN_TAG;
            }
            let form = self.storage_form();
            self.set_aux((self.aux() & !crate::value_vec::STORAGE_FORM) | form);
        }
    }

    /// The whole auxiliary word, its storage form (`storage_form`) included: what a lane
    /// record's own-field hint holds (`Vm::form_hint`), so that one compare proves the exact
    /// shape, no dictionary, no exotic access and where the values live. Every cache records
    /// `exact_shape`, which leaves the form out.
    #[inline(always)]
    pub(crate) fn guard_word(&self) -> u64 {
        u64::from(self.layout)
    }

    /// What a lane record's own-field hint and a call memo's step hold: the record's layout id
    /// (`layout_id`), so that one compare of the record's own word, with no table load, proves
    /// its exact shape, storage form, prototype link and flags. The auxiliary word a hint names
    /// is `heap::layouts::auxiliary(hint)`.
    #[inline(always)]
    pub(crate) fn hint_shape(&self) -> u32 {
        debug_assert_eq!(self.aux() & crate::value_vec::STORAGE_FORM, self.storage_form());
        self.layout
    }

    /// The storage-form bits of the auxiliary word (`value_vec::STORAGE_FORM`) the header's
    /// inline flag and the payload's extras tag decide. The word keeps them current: every
    /// change of either (`initialize_properties`, `extras_mut`) rewrites them, and the
    /// constructors that write the header words directly derive them the same way.
    /// Whether the payload word names a narrow run (`NARROW_RUN_TAG` beside its start): only
    /// a plain object's payload outside inline storage is a start; inline values, an array's
    /// backing and an extras word are other words.
    #[inline(always)]
    fn holds_narrow_run(&self) -> bool {
        self.meta() & (OBJECT_INLINE_STORAGE | OBJECT_ARRAY) == 0 && self.payload >> u32::BITS == NARROW_RUN_TAG >> u32::BITS
    }

    #[inline(always)]
    fn storage_form(&self) -> u32 {
        let extras = if self.extras_pointer().is_some() { crate::value_vec::EXTRAS_FORM } else { 0 };
        let narrow = if self.holds_narrow_run() { crate::value_vec::NARROW_FORM } else { 0 };
        (self.meta() & OBJECT_INLINE_STORAGE) | extras | narrow
    }


    /// The storage start of an object whose values live in the arena and that has no
    /// extras: its payload word is then that start alone (`set_properties`), and its upper
    /// half is zero, which an extras word (`EXTRAS_TAG`) never is. `None` for inline storage
    /// or extras, whose start `storage_position` decodes.
    #[inline(always)]
    pub(crate) fn plain_storage_start(&self) -> Option<usize> {
        // An array's word is its backing; one with values in the arena has extras.
        let plain = self.meta() & (OBJECT_INLINE_STORAGE | OBJECT_ARRAY) == 0 && self.payload >> u32::BITS == 0;
        debug_assert!(!plain || self.extras_pointer().is_none());
        plain.then_some(self.payload as usize)
    }
    /// `hinted_plain_start` of a narrow run (`value_vec::NARROW_FORM`): the run's first word
    /// where the layout id equals `hint` (`hint_shape`), a narrow form without inline storage
    /// or extras, so slot `i` is half `i` of the run counted in `u32`s from twice that word.
    #[inline(always)]
    pub(crate) fn formed_narrow_start(&self, hint: u32) -> Option<usize> {
        use crate::value_vec::{NARROW_FORM, STORAGE_FORM};
        let narrow = self.layout == hint && super::layouts::auxiliary(hint) & STORAGE_FORM == NARROW_FORM
            && self.payload >> u32::BITS == NARROW_RUN_TAG >> u32::BITS;
        narrow.then_some(self.payload as u32 as usize)
    }


    /// The arena storage start of an object whose layout's form a hint proved the arena form
    /// (`staged::ic::layout_form` 0): its payload word.
    #[inline(always)]
    pub(crate) fn proved_arena_start(&self) -> usize {
        debug_assert_eq!(self.plain_arena_start(), Some(self.payload as usize));
        self.payload as usize
    }

    /// The arena storage start of an object whose auxiliary word is `shape` whole, an exact
    /// shape, which only the arena form's word equals; `None` for any other word or form.
    #[inline(always)]
    pub(crate) fn formed_plain_start(&self, hint: u32) -> Option<usize> {
        // The arena form (`STORAGE_FORM` clear) has neither inline storage nor extras, so the
        // payload is the start (`plain_arena_start`) once the whole word matches.
        let plain = hint & crate::value_vec::STORAGE_FORM == 0 && self.aux() == hint;
        debug_assert!(!plain || self.plain_arena_start() == Some(self.payload as usize));
        plain.then_some(self.payload as usize)
    }

    /// `plain_storage_start` as one compare of the payload: a start never exceeds
    /// `VALUE_VEC_START_BITS`, and an extras word always does, so without inline storage a
    /// payload at most that bound is the start itself.
    #[inline(always)]
    pub(crate) fn plain_arena_start(&self) -> Option<usize> {
        let plain = self.meta() & OBJECT_INLINE_STORAGE == 0 && self.payload <= u64::from(VALUE_VEC_START_BITS);
        debug_assert_eq!(plain.then_some(self.payload as usize), self.plain_storage_start());
        plain.then_some(self.payload as usize)
    }

    pub(crate) fn plain_property_storage(&self) -> Option<PlainPropertyStorage> {
        if self.properties().is_dictionary() || self.extras_pointer().is_some() {
            return None;
        }
        Some(if self.meta() & OBJECT_INLINE_STORAGE != 0 {
            PlainPropertyStorage::PackedInline
        } else {
            PlainPropertyStorage::Arena
        })
    }

    /// The storage start of an object whose values live in the arena behind extras, as a
    /// prototype's do (its `PrototypeLookupState` lives there): the start the extras hold
    /// (`storage_position`). `None` for inline storage or no extras.
    #[inline(always)]
    pub(crate) fn extras_storage_start(&self) -> Option<usize> {
        if self.meta() & OBJECT_INLINE_STORAGE != 0 {
            return None;
        }
        // SAFETY: an extras pointer names the extras this object owns.
        self.extras_pointer().map(|extras| unsafe { (*extras).arena_start } as usize)
    }

    /// `plain_storage_start` of an object outside dictionary storage, whose start bits are
    /// then an arena start rather than a dictionary id (`ValueVec::is_dictionary`): the
    /// fast case of a read that also serves dictionaries (`Heap::property_get_shaped`).
    #[inline(always)]
    pub(crate) fn plain_shaped_start(&self) -> Option<usize> {
        if self.aux() & crate::value_vec::DICTIONARY_STORAGE != 0 {
            return None;
        }
        self.plain_storage_start()
    }

    #[inline(always)]
    fn storage_position(&self) -> u32 {
        match self.extras() {
            Some(extras) => extras.arena_start,
            None => self.payload_position(),
        }
    }

    /// The storage position an object without extras keeps: its payload word, or for an
    /// array, whose word holds its backing, the empty position (`set_storage_position`).
    #[inline(always)]
    fn payload_position(&self) -> u32 {
        if self.meta() & OBJECT_ARRAY != 0 { EMPTY_POSITION } else { self.payload as u32 }
    }

    #[inline]
    fn set_storage_position(&mut self, position: u32) {
        if let Some(extras) = self.extras_mut_existing() {
            extras.arena_start = position;
        } else if self.meta() & OBJECT_ARRAY != 0 && self.array_store_needs_extras(position) {
            self.extras_mut().arena_start = position;
        } else {
            self.payload = u64::from(position);
        }
    }

    /// Whether an array's property store at `position`, under its current auxiliary word,
    /// holds anything: an array keeps its payload word for its elements, so such a store,
    /// or dictionary storage, lives behind extras.
    #[inline(always)]
    fn array_store_needs_extras(&self, position: u32) -> bool {
        position != EMPTY_POSITION || self.aux() & crate::value_vec::DICTIONARY_STORAGE != 0
    }

    fn tag_extras(extras: Box<ObjectExtras>) -> u64 {
        let address = Box::into_raw(extras) as usize as u64;
        assert_eq!(
            address & EXTRAS_TAG_MASK,
            0,
            "object extras outside 48-bit space"
        );
        EXTRAS_TAG | address
    }

    #[inline] fn extras_pointer(&self) -> Option<*mut ObjectExtras> {
        let word = self.payload;
        (word & EXTRAS_TAG_MASK == EXTRAS_TAG)
            .then_some((word & !EXTRAS_TAG_MASK) as usize as *mut ObjectExtras)
    }

    /// Releases a dead object's storage in `arena` (`ValueArena::release`), and answers
    /// whether it owns extras, which its drop frees. Inline values hold no arena words;
    /// without extras the payload word is the storage position alone, upper half zero
    /// (`plain_storage_start`).
    #[inline(always)]
    pub(crate) fn release_storage(&self, arena: &mut ValueArena) -> bool {
        // An array's sweep releases its properties (`ValueArena::release_ordinary`).
        debug_assert!(!self.is_array());
        if self.meta() & OBJECT_INLINE_STORAGE != 0 {
            return self.extras_pointer().is_some();
        }
        if self.payload >> u32::BITS == 0 {
            arena.release(ValueVec::from_parts(self.payload as u32, self.aux()));
            return false;
        }
        arena.release(self.properties());
        true
    }

    /// Whether the object owns extras (`ObjectExtras`): the data a plain object lacks.
    #[inline(always)]
    pub(crate) fn has_extras(&self) -> bool {
        self.extras_pointer().is_some()
    }

    #[inline] fn extras(&self) -> Option<&ObjectExtras> {
        // SAFETY: a tagged payload owns a live `ObjectExtras` for the object's lifetime.
        self.extras_pointer().map(|extras| unsafe { &*extras })
    }

    fn extras_mut(&mut self) -> &mut ObjectExtras {
        let extras = match self.extras_pointer() {
            Some(extras) => extras,
            None => {
                // The payload word moves into the extras: the inline values, or the storage
                // position.
                // Extras hold a wide run's start only: a narrow run's owner widens first
                // (`access::widen_narrow_run`).
                assert!(!self.holds_narrow_run(), "a narrow run widens before its owner takes extras");
                let (inline, arena_start) = if self.meta() & OBJECT_INLINE_STORAGE != 0 {
                    (Self::unpack_inline(self.payload), 0)
                } else {
                    ([Value::UNDEFINED; INLINE_PROPERTY_COUNT], self.payload_position())
                };
                // An array's word is its backing, which moves into the extras with it.
                let elements = if self.meta() & OBJECT_ARRAY != 0 {
                    // SAFETY: without extras, an array's payload word is the backing it owns,
                    // and the word is overwritten below.
                    unsafe { super::ArrayElements::from_raw(self.payload) }
                } else {
                    super::ArrayElements::new()
                };
                self.payload = Self::tag_extras(Box::new(ObjectExtras {
                    inline_properties: DisplacedInline(inline),
                    arena_start,
                    elements,
                    ..ObjectExtras::default()
                }));
                // Extras are never dropped while the object lives, so the bit only sets.
                self.set_aux(self.aux() | crate::value_vec::EXTRAS_FORM);
                self.extras_pointer().expect("extras just installed")
            }
        };
        // SAFETY: owned by this object, and `&mut self` makes the borrow unique.
        unsafe { &mut *extras }
    }

    #[inline] fn extras_mut_existing(&mut self) -> Option<&mut ObjectExtras> {
        // SAFETY: as in `extras`, with `&mut self` making the borrow unique.
        self.extras_pointer().map(|extras| unsafe { &mut *extras })
    }

    /// The payload word of inline `values`, when each has a narrow form.
    #[inline(always)]
    fn pack_inline(values: [Value; INLINE_PROPERTY_COUNT]) -> Option<u64> {
        let mut word = 0;
        for (slot, value) in values.into_iter().enumerate() {
            word |= u64::from(value.to_narrow_reference()?) << (slot as u32 * u32::BITS);
        }
        Some(word)
    }

    #[inline(always)]
    fn unpack_inline(word: u64) -> [Value; INLINE_PROPERTY_COUNT] {
        std::array::from_fn(|slot| Value::from_narrow((word >> (slot as u32 * u32::BITS)) as u32))
    }

    /// An object of `proto` and inline `shape` holding `values`, at most the inline slots and
    /// `undefined` past them; `None` when a value has no narrow form.
    #[inline(always)]
    pub(crate) fn with_inline_values(proto: Value, shape: u32, values: &[Value]) -> Option<Self> {
        debug_assert!(values.len() <= INLINE_PROPERTY_COUNT);
        // A loop over the fixed bound unrolls to element moves, where one over `values`
        // becomes a `memcpy` call.
        let inline = std::array::from_fn(|slot| values.get(slot).copied().unwrap_or(Value::UNDEFINED));
        let payload = Self::pack_inline(inline)?;
        // `initialize_properties` of inline storage on a fresh object, which leaves the payload
        // word alone, written out: the object has no exotic access to keep.
        let (start, auxiliary) = ValueVec::inline_property_storage(shape).into_parts();
        let meta = Self::encode_proto(proto) | (start & VALUE_VEC_INTEGRITY_BITS) | OBJECT_INLINE_STORAGE;
        // Inline and no extras: its storage form (`storage_form`).
        let auxiliary = auxiliary | crate::value_vec::INLINE_FORM;
        Some(Self::from_header(meta, auxiliary, payload))
    }

    /// Whether inline `values` fit the payload word without moving into extras.
    #[inline(always)]
    pub(crate) fn inline_values_narrow(values: &[Value]) -> bool {
        values.iter().all(|value| value.to_narrow().is_some())
    }

    /// The inline property slots, wherever they live.
    #[inline(always)]
    fn inline_values(&self) -> [Value; INLINE_PROPERTY_COUNT] {
        match self.extras() {
            Some(extras) => extras.inline_properties.0,
            None => Self::unpack_inline(self.payload),
        }
    }

    /// Stores the inline property slots: in the payload word when each value has a narrow
    /// form and the object has no extras, else in the extras.
    /// Stores `values` in the inline slots as one payload word, when the object keeps them
    /// there (inline storage, no extras) and each has a narrow form; `false` otherwise,
    /// changing nothing.
    pub(crate) fn store_packed_inline_values(&mut self, values: [Value; INLINE_PROPERTY_COUNT]) -> bool {
        if !self.properties().has_inline_property_storage() || self.extras_pointer().is_some() {
            return false;
        }
        match Self::pack_inline(values) {
            Some(word) => {
                self.payload = word;
                true
            }
            None => false,
        }
    }

    fn store_inline_values(&mut self, values: [Value; INLINE_PROPERTY_COUNT]) {
        // An array keeps no inline storage (`Heap::initialize_object_properties`).
        debug_assert!(!self.is_array());
        match (self.extras_mut_existing(), Self::pack_inline(values)) {
            (Some(extras), _) => extras.inline_properties.0 = values,
            (None, Some(word)) => self.payload = word,
            (None, None) => self.extras_mut().inline_properties.0 = values,
        }
    }

    /// Inlined: most callers pass constant inline values (`Object::new`), which then pack
    /// to a constant payload word.
    #[inline(always)]
    pub(crate) fn with_property_storage(
        proto: Value,
        properties: ValueVec,
        inline_properties: [Value; INLINE_PROPERTY_COUNT],
    ) -> Self {
        let mut object = Self::from_header(Self::encode_proto(proto), 0, 0);
        object.initialize_properties(properties);
        // A new object has no extras: its payload word holds the values when each is narrow.
        if properties.has_inline_property_storage() {
            match Self::pack_inline(inline_properties) {
                Some(word) => object.payload = word,
                None => object.extras_mut().inline_properties.0 = inline_properties,
            }
        }
        object
    }

    /// An object of `header`'s prototype and inline shape holding `inline`: what
    /// `with_property_storage` makes, from header words derived once. `None` when a value
    /// has no narrow form, so the payload word cannot hold it.
    #[inline(always)]
    pub(crate) fn from_inline_header(
        header: InlineObjectHeader,
        inline: [Value; INLINE_PROPERTY_COUNT],
    ) -> Option<Self> {
        Some(Self { payload: Self::pack_inline(inline)?, layout: header.layout })
    }

    /// An object of a run header (`InlineObjectHeader::of_run`) whose values start at arena
    /// position `start`: what `with_property_storage` makes of that run, which has no extras.
    #[inline] pub(crate) fn from_run_header(header: InlineObjectHeader, start: u32) -> Self {
        Self { payload: u64::from(start), layout: header.layout }
    }

    #[inline(always)]
    pub(crate) fn inline_properties(&self) -> Option<[Value; INLINE_PROPERTY_COUNT]> {
        (self.meta() & OBJECT_INLINE_STORAGE != 0).then(|| self.inline_values())
    }

    /// What a mark visit needs of an object that stores its properties inline and has no
    /// extras: its prototype link and its inline values' narrow forms, first slot first. Such
    /// an object holds no extras roots and was never used as a prototype (both live in
    /// extras), so its visit is its prototype, its shape's roots and these values. `None`
    /// for any other object.
    #[inline(always)]
    pub(crate) fn plain_inline_trace(&self) -> Option<(ProtoLink, [u32; INLINE_PROPERTY_COUNT])> {
        let meta = self.meta();
        if meta & OBJECT_INLINE_STORAGE == 0 || self.payload & EXTRAS_TAG_MASK == EXTRAS_TAG {
            return None;
        }
        Some((ProtoLink(meta & PROTO_INDEX_BITS), Self::unpack_narrow(self.payload)))
    }

    /// The narrow inline values of a payload word that holds them (`unpack_inline`).
    #[inline(always)]
    fn unpack_narrow(word: u64) -> [u32; INLINE_PROPERTY_COUNT] {
        std::array::from_fn(|slot| (word >> (slot as u32 * u32::BITS)) as u32)
    }

    /// The inline property at `slot`, when the object stores its properties inline.
    #[inline(always)]
    pub(crate) fn inline_property(&self, slot: usize) -> Option<Value> {
        if self.meta() & OBJECT_INLINE_STORAGE == 0 {
            return None;
        }
        debug_assert!(slot < INLINE_PROPERTY_COUNT);
        Some(match self.extras() {
            Some(extras) => extras.inline_properties.0[slot],
            None => Value::from_narrow(self.payload_narrow(slot)),
        })
    }

    /// Stores `value` at inline `slot`, moving the inline values into extras when the payload
    /// word cannot hold it.
    #[cfg(test)]
    pub(crate) fn set_inline_property(&mut self, slot: usize, value: Value) {
        // A cached slot past the inline values is a caller bug, not a reason to panic here.
        if slot < INLINE_PROPERTY_COUNT && !self.store_inline_narrow(slot, value) {
            self.set_inline_property_wide(slot, value);
        }
    }

    /// Stores `value` at inline `slot` unless the payload word holds the inline values and
    /// cannot hold `value`; `false` leaves the object unchanged for the caller to move its
    /// values to the arena.
    #[inline(always)]
    pub(crate) fn store_inline_narrow(&mut self, slot: usize, value: Value) -> bool {
        debug_assert!(self.properties().has_inline_property_storage());
        debug_assert!(slot < INLINE_PROPERTY_COUNT);
        match (self.extras_pointer(), value.to_narrow()) {
            (None, Some(narrow)) => {
                self.store_payload_narrow(slot, narrow);
                true
            }
            (None, None) => false,
            (Some(_), _) => {
                self.set_inline_property_wide(slot, value);
                true
            }
        }
    }

    /// Whether a cached add of `value` at `slot`, the length of this object's storage under
    /// a shape outside dictionary storage, to this object, which admits it
    /// (`admits_cached_add`), is a plain append (`append_plain`): the object has no extras,
    /// so it was never a prototype, and its storage takes the value without moving: a narrow
    /// value into a free inline slot, or any value into its arena range's spare capacity.
    #[inline(always)]
    pub(crate) fn admits_plain_append(&self, slot: usize, value: Value) -> bool {
        debug_assert!(self.admits_cached_add());
        if self.extras_pointer().is_some() {
            return false;
        }
        if self.meta() & OBJECT_INLINE_STORAGE != 0 {
            return slot < INLINE_PROPERTY_COUNT && value.to_narrow().is_some();
        }
        ValueArena::appends_in_place(self.properties(), slot)
    }

    /// Whether a cached add at `slot`, as for `admits_plain_append`, to this object, which has
    /// no extras, appends into the young run reserved for it past its length's class
    /// (`ValueArena::reserved_room`): the receiver of a constructor whose learned field count
    /// reserved that run (`Heap::reserve_receiver_run`).
    #[inline(always)]
    pub(crate) fn admits_reserved_append(&self, properties: &ValueArena, slot: usize) -> bool {
        debug_assert!(self.admits_cached_add());
        self.extras_pointer().is_none()
            && self.meta() & OBJECT_INLINE_STORAGE == 0
            && properties.reserved_room(self.properties(), slot + 1)
    }

    /// Appends `value` at `slot` and moves to `shape`, the add `admits_reserved_append`
    /// admitted: `append_plain` into the reservation.
    #[inline(always)]
    pub(crate) fn append_reserved(&mut self, properties: &mut ValueArena, slot: usize, value: Value, shape: u32) {
        properties.note_shape_value(shape, slot, value);
        debug_assert!(self.admits_reserved_append(properties, slot));
        // SAFETY: `admits_reserved_append` found the reservation's room for the value.
        unsafe { properties.push_reserved(self.properties(), slot, value, shape) };
        self.set_shape(shape, properties);
    }

    /// Appends `value` at `slot` and moves to `shape`, the add `admits_plain_append` admitted.
    /// Without extras it was never a prototype; the move still retires shape readers.
    #[inline(always)]
    pub(crate) fn append_plain(&mut self, properties: &mut ValueArena, slot: usize, value: Value, shape: u32) {
        properties.note_shape_value(shape, slot, value);
        debug_assert!(self.admits_plain_append(slot, value));
        match value.to_narrow().filter(|_| self.meta() & OBJECT_INLINE_STORAGE != 0) {
            Some(narrow) => self.store_payload_narrow(slot, narrow),
            // SAFETY: `admits_plain_append` found the arena range's room for the value.
            None => unsafe { properties.push_in_place(self.properties(), slot, value, shape) },
        }
        self.set_shape(shape, properties);
    }

    /// The half of the payload word that holds inline `slot`, the first the lower: one
    /// half-word access, which neither shifts nor touches the other slot.
    #[inline(always)]
    fn payload_half(slot: usize) -> usize {
        debug_assert!(slot < INLINE_PROPERTY_COUNT);
        if cfg!(target_endian = "little") { slot } else { INLINE_PROPERTY_COUNT - 1 - slot }
    }

    /// The narrow form at inline `slot` of the payload word, which holds the inline values.
    #[inline(always)]
    fn payload_narrow(&self, slot: usize) -> u32 {
        // SAFETY: `payload` is a `u64`, two in-bounds `u32` halves.
        unsafe { (&raw const self.payload).cast::<u32>().add(Self::payload_half(slot)).read() }
    }

    /// Stores the narrow form `narrow` at inline `slot` of the payload word, which holds the
    /// inline values.
    #[inline(always)]
    fn store_payload_narrow(&mut self, slot: usize, narrow: u32) {
        // SAFETY: `payload` is a `u64`, two in-bounds `u32` halves; `&mut self` is exclusive.
        unsafe { (&raw mut self.payload).cast::<u32>().add(Self::payload_half(slot)).write(narrow) };
    }

    /// Where the values of an object outside dictionary storage live, decoded from its
    /// payload word once (`ExactStorage`).
    #[inline(always)]
    pub(crate) fn exact_storage(&self) -> ExactStorage {
        // The auxiliary word's form bits, which the shape compare already loaded, stand for
        // the header's inline flag and the payload's extras tag (`storage_form`).
        let form = self.aux() & crate::value_vec::STORAGE_FORM;
        debug_assert_eq!(form, self.storage_form());
        match form {
            0 => ExactStorage::Plain(self.payload_position() as usize),
            crate::value_vec::INLINE_FORM => ExactStorage::PackedInline,
            crate::value_vec::NARROW_FORM => ExactStorage::Narrow,
            _ => ExactStorage::Extended { inline: form & crate::value_vec::INLINE_FORM != 0 },
        }
    }

    /// Stores a value's narrow form `narrow` (`Value::to_narrow`) into inline `slot` of a
    /// `PackedInline` object (`exact_storage`), that half of the payload word; `false` for a
    /// value without one, changing nothing.
    #[inline(always)]
    pub(crate) fn store_packed_narrow(&mut self, slot: usize, narrow: Option<u32>) -> bool {
        debug_assert!(matches!(self.exact_storage(), ExactStorage::PackedInline));
        narrow.map(|narrow| self.store_payload_narrow(slot, narrow)).is_some()
    }

    /// Stores `value` into inline `slot` of an `Extended` object (`exact_storage`), whose
    /// extras hold its inline values.
    #[inline(always)]
    pub(crate) fn store_extended_inline(&mut self, slot: usize, value: Value) {
        let extras = self.extras_pointer().expect("an extended object has extras");
        // SAFETY: owned by this object, and `&mut self` makes the borrow unique.
        unsafe { (*extras).inline_properties.0[slot] = value };
    }

    /// `set_inline_property` into extras: the object has them, or `value` has no narrow form.
    #[cold]
    fn set_inline_property_wide(&mut self, slot: usize, value: Value) {
        let mut values = self.inline_values();
        values[slot] = value;
        self.store_inline_values(values);
    }

    pub(crate) fn replace_property_storage(
        &mut self,
        mut properties: ValueVec,
        inline_properties: [Value; INLINE_PROPERTY_COUNT],
        arena: &mut ValueArena,
    ) -> ValueVec {
        let previous = self.properties();
        properties.preserve_integrity_from(previous);
        self.set_properties(properties, arena);
        // Arena storage keeps its position in the payload word (or the extras) instead.
        if properties.has_inline_property_storage() {
            self.store_inline_values(inline_properties);
        }
        previous
    }

    /// Takes `source`'s property values, given `properties`, the arena's duplicate of
    /// `source.properties()`, and returns the storage this object held before.
    pub(crate) fn copy_property_storage_from(&mut self, source: &Self, properties: ValueVec, arena: &mut ValueArena) -> ValueVec {
        let previous = self.properties();
        self.set_properties(properties, arena);
        if properties.has_inline_property_storage() {
            self.store_inline_values(source.inline_values());
        }
        previous
    }

    /// Presence of [[ErrorData]] is the unforgeable Error brand.
    pub(crate) fn error(proto: Value) -> Self {
        let mut object = Self::new(proto);
        object.extras_mut().rare_mut().brand = ObjectBrand::Error;
        object
    }

    /// How structured serialization treats this object.
    pub(crate) fn set_structured_serialization(&mut self, policy: StructuredSerialization) {
        self.extras_mut().rare_mut().structured_serialization = policy;
    }

    pub(crate) fn structured_serialization(&self) -> StructuredSerialization {
        self.extras().and_then(ObjectExtras::rare).map_or(StructuredSerialization::Ordinary, |rare| rare.structured_serialization)
    }

    pub(crate) fn forbid_structured_transfer(&mut self) {
        self.extras_mut().rare_mut().structured_transfer_forbidden = true;
    }

    pub(crate) fn is_structured_transfer_forbidden(&self) -> bool {
        self.extras()
            .and_then(ObjectExtras::rare).is_some_and(|rare| rare.structured_transfer_forbidden)
    }

    pub(crate) fn has_error_data(&self) -> bool {
        self.brand() == ObjectBrand::Error
    }

    /// Genuine primitive-wrapper data; guest properties cannot supply this slot.
    #[inline]
    pub(crate) fn boxed_primitive(&self) -> Option<Value> {
        self.extras()?.rare()?.boxed_primitive
    }

    pub(crate) fn set_boxed_primitive(&mut self, value: Value) {
        self.extras_mut().rare_mut().boxed_primitive = Some(value);
    }

    pub(crate) fn stack_data(&self) -> Option<&StackData> {
        self.extras()?.rare()?.stack_data.as_ref()
    }

    pub(crate) fn set_stack_data(&mut self, data: StackData) {
        self.extras_mut().rare_mut().stack_data = Some(data);
    }

    /// Visit internal primitive data, private-name homes and captured stack roots.
    #[inline(always)] pub(crate) fn visit_extras_roots(&self, visit: impl FnMut(Value)) { if self.extras_pointer().is_some() { self.visit_extras_values(visit) } }

    #[inline(never)] fn visit_extras_values(&self, mut visit: impl FnMut(Value)) {
        let Some(extras) = self.extras().and_then(ObjectExtras::rare) else { return };
        if let Some(value) = extras.boxed_primitive {
            visit(value);
        }
        match extras.stack_data.as_ref() {
            Some(StackData::Captured { records, .. }) => {
                for record in records {
                    record.visit_roots(&mut visit);
                }
            }
            Some(StackData::CallSite(record)) => {
                record.visit_roots(&mut visit);
            }
            Some(StackData::Formatted(value)) => visit(*value),
            None => {}
        }
    }

    #[inline] pub(crate) fn was_used_as_prototype(&self) -> bool {
        self.extras()
            .is_some_and(|extras| extras.prototype_lookup != PrototypeLookupState::Ordinary)
    }

    pub(crate) fn mark_as_prototype(&mut self) {
        if !self.was_used_as_prototype() {
            // A prototype's elements answer its dependents' reads: no proof replaces them.
            super::array_proofs::retire_array_storage_proofs();
            self.note_global_layout_change();
            self.extras_mut().prototype_lookup = PrototypeLookupState::Setup;
        }
    }

    pub(crate) fn prototype_lookup_ready(&self) -> bool {
        self.extras()
            .is_some_and(|extras| extras.prototype_lookup == PrototypeLookupState::Ready)
    }

    pub(crate) fn prepare_prototype_lookup(&mut self) {
        if self.was_used_as_prototype() {
            self.extras_mut().prototype_lookup = PrototypeLookupState::Ready;
        }
    }

    #[inline] pub(crate) fn shape(&self) -> u32 {
        ValueVec::from_parts(0, self.aux()).auxiliary()
    }
    /// The shape with its dictionary and exotic bits (`ValueVec::exact_auxiliary`): a cache
    /// entry, which records only shapes, matches it exactly when it matches the shape of an
    /// ordinary object outside dictionary storage.
    #[inline(always)]
    pub(crate) fn exact_shape(&self) -> u32 {
        ValueVec::from_parts(0, self.aux()).exact_auxiliary()
    }
    /// The shape with its exotic bit and without its dictionary bit: a recorded shape
    /// matches it exactly when it matches the shape of an ordinary object, in whatever
    /// storage (`ValueVec::EXOTIC_ACCESS`).
    #[inline(always)]
    pub(crate) fn ordinary_shape(&self) -> u32 {
        self.aux() & !(crate::value_vec::DICTIONARY_STORAGE | crate::value_vec::STORAGE_FORM | crate::value_vec::CONSTANT_FORM)
    }

    /// Whether code holds this object's values as constants (`value_vec::CONSTANT_FORM`).
    #[inline(always)]
    pub(crate) fn holds_constant_mark(&self) -> bool {
        self.aux() & crate::value_vec::CONSTANT_FORM != 0
    }

    /// Marks this object's values as held by code (`value_vec::CONSTANT_FORM`): until the bit
    /// clears, every raw-word guard misses it and its stores go through the VM, which clears
    /// it (`note_constant_write`). The caller subscribes its reader under `exact_shape`, which
    /// the mark leaves unchanged.
    pub(crate) fn mark_constant(&mut self) {
        self.set_aux(self.aux() | crate::value_vec::CONSTANT_FORM);
    }

    /// A write to one of this object's values, or a change of its layout or storage: where
    /// code holds its values (`holds_constant_mark`), that code retires, by the object's exact
    /// shape as it subscribed (`watch_prototype_values`), and the mark clears, so later stores
    /// take every fast path again. One test of the header word otherwise.
    #[inline(always)]
    pub(crate) fn note_constant_write(&mut self) {
        if self.holds_constant_mark() {
            self.clear_constant_mark();
        }
    }

    #[inline(never)]
    fn clear_constant_mark(&mut self) {
        prototype_values::retire(self.exact_shape());
        self.set_aux(self.aux() & !crate::value_vec::CONSTANT_FORM);
    }
    #[inline(always)]
    pub(crate) fn set_shape(&mut self, shape: u32, arena: &mut ValueArena) {
        self.note_constant_write();
        arena.note_shape_transition(self.shape(), shape);
        self.note_prototype_layout_change();
        self.note_global_layout_change();
        self.renumber_shape(shape);
    }
    #[inline] pub(crate) fn is_extensible(&self) -> bool {
        self.properties().is_extensible()
    }
    /// Whether a cached add transition (`FieldAddCache`) applies to this object as to the
    /// ordinary object that recorded it: it is extensible, and no arguments object or module
    /// namespace, whose exotic own properties its shape does not describe.
    #[inline(always)]
    pub(crate) fn admits_cached_add(&self) -> bool {
        self.is_extensible() && !self.has_exotic_access()
    }
    /// Whether the object's property access is not the ordinary one its shape describes: an
    /// arguments object, a module namespace or a proxy's own object. A header bit, so fast
    /// paths need not read the extras (`ObjectBrand`).
    #[inline(always)]
    pub(crate) fn has_exotic_access(&self) -> bool {
        self.aux() & crate::value_vec::EXOTIC_ACCESS != 0
    }
    #[inline(always)]
    fn brand(&self) -> ObjectBrand {
        self.extras().and_then(ObjectExtras::rare).map_or(ObjectBrand::Ordinary, |rare| rare.brand)
    }
    #[inline(always)]
    fn element_facts(&self) -> ElementFacts {
        self.extras().map_or(ElementFacts(0), |extras| extras.element_facts)
    }
    fn mark_element_fact(&mut self, bit: u8) {
        self.extras_mut().element_facts.0 |= bit;
    }
    pub(crate) fn set_extensible(&mut self, value: bool) {
        let mut properties = ValueVec::from_parts(self.meta() & VALUE_VEC_INTEGRITY_BITS, 0);
        properties.set_extensible(value);
        self.set_meta((self.meta() & !VALUE_VEC_INTEGRITY_BITS) | properties.into_parts().0);
    }
    #[inline]
    pub(crate) fn is_frozen(&self) -> bool {
        self.properties().is_frozen()
    }
    pub(crate) fn set_frozen(&mut self, value: bool) {
        self.note_prototype_layout_change();
        self.note_global_layout_change();
        super::array_proofs::retire_array_storage_proofs();
        let mut properties = ValueVec::from_parts(self.meta() & VALUE_VEC_INTEGRITY_BITS, 0);
        properties.set_frozen(value);
        self.set_meta((self.meta() & !VALUE_VEC_INTEGRITY_BITS) | properties.into_parts().0);
    }
    #[inline]
    pub(crate) fn arguments_map(&self) -> Option<&[u16]> {
        self.extras()?.rare()?.arguments_map.as_deref()
    }
    pub(crate) fn arguments_map_mut(&mut self) -> Option<&mut Vec<u16>> {
        self.extras_mut_existing()?.rare_mut_existing()?.arguments_map.as_mut()
    }
    pub(crate) fn window_depth(&self) -> Option<usize> {
        self.extras()?.rare()?.window_depth.map(|depth| depth as usize)
    }
    pub(crate) fn set_window_depth(&mut self, depth: usize) {
        if let Some(rare) = self.extras_mut_existing().and_then(ObjectExtras::rare_mut_existing) {
            rare.window_depth = u32::try_from(depth).ok();
        }
    }
    pub(crate) fn mapped_argument_slot(&self, index: usize) -> Option<usize> {
        self.arguments_map()?
            .get(index)
            .copied()
            .filter(|slot| *slot != u16::MAX)
            .map(usize::from)
    }
    pub(crate) fn set_arguments_map(&mut self, mapping: Vec<u16>) {
        super::array_proofs::retire_array_storage_proofs();
        self.extras_mut().rare_mut().arguments_map = Some(mapping);
    }
    pub(crate) fn set_arguments_object(&mut self) {
        self.extras_mut().rare_mut().brand = ObjectBrand::Arguments;
        self.set_aux(self.aux() | crate::value_vec::EXOTIC_ACCESS);
    }
    /// Whether the object's elements are ordinary and all dense, as `dense_array_length`
    /// needs them: it is extensible and not frozen, and holds no indexed or `length`
    /// descriptor, no arguments mapping and never held sparse elements
    /// (`mark_sparse_elements`, which every sparse write makes). Read from the header bits
    /// and one mask of the extras' element facts.
    #[inline(always)]
    pub(crate) fn holds_ordinary_elements(&self) -> bool {
        self.meta() & VALUE_VEC_INTEGRITY_BITS == 0
            && !self.has_exotic_access()
            && !self.element_facts().has(ElementFacts::NOT_ORDINARY)
    }
    #[inline] pub(crate) fn is_arguments_object(&self) -> bool {
        self.brand() == ObjectBrand::Arguments
    }
    #[inline]
    pub(crate) fn has_indexed_descriptors(&self) -> bool {
        self.element_facts().has(ElementFacts::INDEXED_DESCRIPTORS)
    }
    pub(crate) fn mark_indexed_descriptors(&mut self) {
        self.note_prototype_layout_change();
        super::array_proofs::retire_array_storage_proofs();
        self.mark_element_fact(ElementFacts::INDEXED_DESCRIPTORS);
    }
    /// Whether the object ever held sparse elements (`mark_sparse_elements`).
    #[inline(always)]
    pub(crate) fn has_sparse_elements(&self) -> bool {
        self.element_facts().has(ElementFacts::SPARSE_ELEMENTS)
    }
    pub(crate) fn mark_sparse_elements(&mut self) {
        self.mark_element_fact(ElementFacts::SPARSE_ELEMENTS);
    }
    pub(crate) fn has_length_descriptor(&self) -> bool {
        self.element_facts().has(ElementFacts::LENGTH_DESCRIPTOR)
    }
    pub(crate) fn mark_length_descriptor(&mut self) {
        self.mark_element_fact(ElementFacts::LENGTH_DESCRIPTOR);
    }
    /// Whether the VM's descriptor table may hold an entry for this object: lookups of an
    /// object without one need not probe the table.
    #[inline(always)]
    pub(crate) fn has_side_descriptors(&self) -> bool {
        self.element_facts().has(ElementFacts::SIDE_DESCRIPTORS)
    }
    /// A prototype's side descriptors can name keys its shape lacks, which retires the
    /// proofs (`prototype_layout_epoch`) that found them absent.
    pub(crate) fn mark_side_descriptors(&mut self) {
        self.note_prototype_layout_change();
        self.mark_element_fact(ElementFacts::SIDE_DESCRIPTORS);
    }
    pub(crate) fn is_raw_json(&self) -> bool {
        self.brand() == ObjectBrand::RawJson
    }
    pub(crate) fn set_raw_json(&mut self) {
        self.extras_mut().rare_mut().brand = ObjectBrand::RawJson;
    }

    pub(crate) fn external_address(&self) -> Option<usize> {
        self.extras()?.rare()?.external_address
    }
    pub(crate) fn set_external_address(&mut self, address: usize) {
        let extras = self.extras_mut().rare_mut();
        extras.external_address = Some(address);
        extras.structured_serialization = StructuredSerialization::Forbidden;
    }

    /// A namespace is exotic (`set_module_namespace`), so the flag in the object's own word
    /// rules out every other object without reading its extras.
    #[inline(always)]
    pub(crate) fn is_module_namespace(&self) -> bool {
        self.has_exotic_access() && self.brand() == ObjectBrand::ModuleNamespace
    }
    /// A proxy's own object: the handler answers every access, so no shape a cache records
    /// may match it.
    pub(crate) fn proxy(proto: Value) -> Self {
        let mut object = Self::new(proto);
        object.set_aux(object.aux() | crate::value_vec::EXOTIC_ACCESS);
        object
    }
    pub(crate) fn set_module_namespace(&mut self) {
        self.extras_mut().rare_mut().brand = ObjectBrand::ModuleNamespace;
        self.set_aux(self.aux() | crate::value_vec::EXOTIC_ACCESS);
    }
    pub(crate) fn module_bindings(&self) -> &[(Atom, ProgramId, u16)] {
        self.extras().and_then(ObjectExtras::rare).map_or(&[], |rare| &rare.module_bindings)
    }
    pub(crate) fn set_module_bindings(&mut self, bindings: Vec<(Atom, ProgramId, u16)>) {
        self.extras_mut().rare_mut().module_bindings = bindings;
    }
    #[inline]
    pub(crate) fn deferred_module(&self) -> Option<&crate::ModuleSource> {
        self.extras()?.rare()?.deferred_module.as_ref()
    }
    pub(crate) fn set_deferred_module(&mut self, module: Option<crate::ModuleSource>) {
        self.extras_mut().rare_mut().deferred_module = module;
    }
    pub(crate) fn allocated_extra_bytes(&self) -> usize {
        self.extras().map_or(0, ObjectExtras::allocated_bytes)
    }

    pub(crate) fn module_binding(&self, atom: Atom) -> Option<(ProgramId, u16)> {
        self.module_bindings()
            .iter()
            .find_map(|(name, program, slot)| (*name == atom).then_some((*program, *slot)))
    }
}

/// An object is its header word and one payload word.
const _: () = assert!(std::mem::size_of::<Object>() == 12);
/// The payload word holds every inline slot as a narrow value.
const _: () = assert!(INLINE_PROPERTY_COUNT * u32::BITS as usize == u64::BITS as usize);

/// Internal methods installed by ProxyCreate, retained after target release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProxyKind {
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
pub(crate) struct EnvironmentRecord {
    pub(crate) parent: Value,
    pub(crate) function: u32,
    pub(crate) slots: EnvironmentSlots,
    pub(crate) scope: EnvironmentScopeRef,
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
pub(crate) struct EnvironmentScope {
    pub(crate) program: Option<u32>,
    pub(crate) root_eval_scope: bool,
    // A captured lexical scope selects its names from the owning function's
    // binding-site table; slot values remain shared with that activation.
    pub(crate) binding_site_pc: Option<u32>,
    pub(crate) dynamic_bindings: EnvironmentBindings,
    pub(crate) with_objects: Box<[Value]>,
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
pub(crate) struct EnvironmentScopeRef(std::num::NonZeroUsize);

/// The low bit, never set in a box pointer, marks an inline program-only scope.
const INLINE_SCOPE_TAG: usize = 1;
/// An inline scope holds its program plus one (zero for none) above the tag bit.
const INLINE_PROGRAM_SHIFT: u32 = INLINE_SCOPE_TAG.trailing_ones();
const _: () = assert!(align_of::<EnvironmentScope>() > INLINE_SCOPE_TAG);
static NO_DYNAMIC_BINDINGS: EnvironmentBindings = EnvironmentBindings::Owned(Vec::new());

impl EnvironmentScopeRef {
    pub(crate) fn new(scope: EnvironmentScope) -> Self {
        if scope.is_program_only()
            && let Some(inline) = Self::inline(scope.program)
        {
            return inline;
        }
        Self::boxed(scope)
    }

    /// The scope of a function's activation, which names only its `program`: `new` of that
    /// scope, without building it first.
    pub(crate) fn of_program(program: u32) -> Self {
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
    pub(crate) fn program(&self) -> Option<u32> {
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
pub(crate) struct EnvironmentSlots(pub(super) super::thin_slice::ThinBoxSlice<EnvironmentSlot, ENVIRONMENT_PREFIX_WORDS>);

/// Words an `EnvironmentSlots::lay_out_in_place` storage takes before its slots.
pub(crate) const IN_PLACE_SLOTS_WORDS: usize =
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
pub(crate) enum EnvironmentBindings {
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
pub(crate) struct IteratorExt {
    /// Upper half of the iterator position; nonzero only for array-likes iterated past
    /// 2^32 elements, whose lengths reach 2^53 - 1.
    pub(crate) position_high: u32,
    pub(crate) next_method: Option<Value>,
    pub(crate) helper: Option<Box<IteratorHelper>>,
    /// Where `helper` is in its life; the record's `done` ends it.
    pub(crate) helper_phase: HelperPhase,
    pub(crate) generator: Option<Box<crate::vm::activation::GeneratorRecord>>,
}

impl IteratorExt {
    /// The full position whose low half is the cell's `index`.
    #[inline]
    pub(crate) fn position(&self, low: u32) -> usize {
        ((u64::from(self.position_high) << u32::BITS) | u64::from(low)) as usize
    }

    /// Splits `position` between the cell's `index` and this box.
    #[inline]
    pub(crate) fn store_position(&mut self, low: &mut u32, position: usize) {
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
pub(crate) enum CollectionStorage {
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
pub(crate) struct IteratorRecord {
    pub(crate) object: Object,
    pub(crate) source: Value,
    pub(crate) kind: IteratorKind,
    /// Low half of the position within `source`; `IteratorExt::position` joins it with
    /// the upper half.
    pub(crate) index: u32,
    pub(crate) done: bool,
    pub(crate) ext: IteratorExt,
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
pub(crate) type FunctionRecord = crate::staged::record_heads::FunctionRecord<Object, FunctionKind, Value>;

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
pub(crate) struct IsoDateTimeFields {
    pub(crate) date: (i32, u32, u32),
    pub(crate) time: [u32; 6],
}

/// A ZonedDateTime's exact time and time-zone identifier, boxed together so the cell keeps
/// four words.
#[derive(Clone, Debug)]
pub(crate) struct ZonedInstant {
    pub(crate) epoch_nanoseconds: i128,
    pub(crate) time_zone: String,
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
pub(crate) struct SymbolData {
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
    pub(crate) fn public(description: Option<String>) -> Self {
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
