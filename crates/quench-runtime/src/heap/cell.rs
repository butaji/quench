use super::records::RECORD_SPACE_CAPACITY;
use super::root::WeakHandle;
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
pub(crate) use await_state::{AwaitKind, AwaitRole, AwaitState};
pub(crate) use wasm_fields::{TableTarget, TableWords, WasmGcFields, WasmTableElements};
pub(crate) use typed_array_kind::{TYPED_ARRAYS, TypedArrayKind, TypedArrayRow};
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

#[cfg(test)]
mod cell_layout_tests {
    use super::*;

    /// Every heap slot stores an `Option<Cell>`: a tag word and a payload of at most three
    /// words. No variant may grow past that; an array record is smaller.
    const MAX_CELL_BYTES: usize = 32;

    #[test]
    fn cells_stay_compact() {
        assert!(std::mem::size_of::<Option<Cell>>() <= MAX_CELL_BYTES);
        assert!(std::mem::size_of::<ArrayObject>() + std::mem::size_of::<Value>() <= MAX_CELL_BYTES);
    }

    #[test]
    fn native_array_layout_is_derived_from_the_current_storage_types() {
        assert_eq!(NATIVE_ARRAY_RECORD_SIZE, std::mem::size_of::<ArrayObject>());
        assert_eq!(NATIVE_ARRAY_RECORD_ALIGN, std::mem::align_of::<ArrayObject>());
        assert_eq!(
            NATIVE_ARRAY_ELEMENTS_OFFSET,
            std::mem::offset_of!(ArrayObject, object) + std::mem::offset_of!(Object, payload)
        );
        assert_eq!(NATIVE_ARRAY_EXTRAS_ELEMENTS_OFFSET, std::mem::offset_of!(ObjectExtras, elements));
        let layout = super::super::NATIVE_ARRAY_LAYOUT;
        assert_eq!(layout.extras_elements_offset, NATIVE_ARRAY_EXTRAS_ELEMENTS_OFFSET);
        assert_eq!(layout.record_size, std::mem::size_of::<ArrayObject>());
        assert_eq!(layout.record_align, std::mem::align_of::<ArrayObject>());
        assert_eq!(layout.elements_offset, NATIVE_ARRAY_ELEMENTS_OFFSET);
    }

    #[test]
    fn native_object_layout_is_derived_from_the_current_storage_types() {
        assert_eq!(NATIVE_OBJECT_RECORD_SIZE, std::mem::size_of::<Object>());
        assert_eq!(NATIVE_OBJECT_RECORD_ALIGN, std::mem::align_of::<Object>());
        assert_eq!(
            NATIVE_OBJECT_LAYOUT_OFFSET,
            std::mem::offset_of!(Object, layout)
        );
        assert_eq!(
            NATIVE_OBJECT_PAYLOAD_OFFSET,
            std::mem::offset_of!(Object, payload)
        );
        assert_eq!(
            NATIVE_OBJECT_EXTRAS_ARENA_START_OFFSET,
            std::mem::offset_of!(ObjectExtras, arena_start)
        );

        let layout = super::super::NATIVE_OBJECT_LAYOUT;
        assert_eq!(layout.extras_inline_values_offset,
            std::mem::offset_of!(ObjectExtras, inline_properties) + std::mem::offset_of!(DisplacedInline, 0));
        assert_eq!(layout.record_size, std::mem::size_of::<Object>());
        assert_eq!(layout.record_align, std::mem::align_of::<Object>());
        assert_eq!(layout.meta_offset, NATIVE_OBJECT_META_OFFSET);
        assert_eq!(layout.auxiliary_offset, NATIVE_OBJECT_AUXILIARY_OFFSET);
        assert_eq!(layout.payload_offset, NATIVE_OBJECT_PAYLOAD_OFFSET);
        assert_eq!(
            layout.extras_arena_start_offset,
            NATIVE_OBJECT_EXTRAS_ARENA_START_OFFSET
        );
    }
}

#[rustfmt::skip]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
pub(crate) enum Native {
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
    LazyDataProperty, TemporalInitialize,
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
    WasmHost,
    WebAssemblyValidate, WebAssemblyCompile, WebAssemblyInstantiate,
    WasmModule, WasmModuleExports, WasmModuleImports, WasmModuleCustomSections,
    WasmInstance, WasmInstanceExportsGetter,
    WasmMemory, WasmMemoryGrow, WasmMemoryBufferGetter,
    WasmTable, WasmTableLengthGetter, WasmTableGrow, WasmTableGet, WasmTableSet,
    WasmGlobal, WasmGlobalValueGetter, WasmGlobalValueSetter, WasmGlobalValueOf, WasmTag,
    WasmCompileError, WasmLinkError, WasmRuntimeError,
    /// A Wasm function as JavaScript sees it; its environment is the Wasm closure.
    WasmExportedFunction,
    HostFunction,
    CallSiteConstructor,
    CallSiteGetFunction,
    MathInitialize,
}

/// A native's traits, each derived once at compile time from its defining predicate
/// (`Native::derive_*`) and read with one load: the call protocol asks several per call.
mod native_traits {
    pub(super) const RUNS_FROM_LANE: u8 = 1;
    pub(super) const RUNS_BESIDE_ACTIVATIONS: u8 = 2;
    pub(super) const HOST_CONTROL: u8 = 4;
    pub(super) const MATH_FUNCTION: u8 = 8;
    pub(super) const INTRINSIC_SPECIES_GETTER: u8 = 16;
}

const NATIVE_TRAITS: [u8; Native::COUNT as usize] = {
    use native_traits::*;
    let mut table = [0u8; Native::COUNT as usize];
    let mut index = 0;
    while index < Native::COUNT {
        let Some(native) = Native::from_discriminant(index) else { panic!("discriminants are dense") };
        let mut traits = 0;
        if native.derive_runs_from_lane() { traits |= RUNS_FROM_LANE; }
        if native.derive_runs_beside_activations() { traits |= RUNS_BESIDE_ACTIVATIONS; }
        if native.derive_host_control_native() { traits |= HOST_CONTROL; }
        if native.derive_math_function() { traits |= MATH_FUNCTION; }
        if native.derive_intrinsic_species_getter() { traits |= INTRINSIC_SPECIES_GETTER; }
        table[index as usize] = traits;
        index += 1;
    }
    table
};

impl Native {
    #[inline(always)]
    const fn has_trait(self, bit: u8) -> bool {
        NATIVE_TRAITS[self as usize] & bit != 0
    }

    /// `derive_runs_from_lane`, read from the derived table.
    #[inline(always)]
    pub(crate) const fn runs_from_lane(self) -> bool { self.has_trait(native_traits::RUNS_FROM_LANE) }

    /// `derive_runs_beside_activations`, read from the derived table.
    #[inline(always)]
    pub(crate) const fn runs_beside_activations(self) -> bool { self.has_trait(native_traits::RUNS_BESIDE_ACTIVATIONS) }

    /// `derive_host_control_native`, read from the derived table.
    #[inline(always)]
    pub(crate) const fn is_host_control_native(self) -> bool { self.has_trait(native_traits::HOST_CONTROL) }

    /// `derive_math_function`, read from the derived table.
    #[inline(always)]
    pub(crate) const fn is_math_function(self) -> bool { self.has_trait(native_traits::MATH_FUNCTION) }

    /// `derive_intrinsic_species_getter`, read from the derived table.
    #[inline(always)]
    pub(crate) const fn is_intrinsic_species_getter(self) -> bool { self.has_trait(native_traits::INTRINSIC_SPECIES_GETTER) }
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

    /// The `get [Symbol.species]` accessors, each of which returns its receiver.
    const fn derive_intrinsic_species_getter(self) -> bool {
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

    /// `String.prototype` methods that, for a string primitive receiver and arguments that
    /// are not objects, run no guest code and have no effect beyond allocating their result.
    /// `split` looks `Symbol.split` up only on an object separator (ES2025), so it is among
    /// them; `match` and `search`, which make a RegExp of a string, are not.
    #[inline]
    pub(crate) const fn is_pure_string_method(self) -> bool {
        matches!(
            self,
            Self::StringCharCodeAt
                | Self::StringCharAt
                | Self::StringSlice
                | Self::StringSubstring
                | Self::StringSubstr
                | Self::StringIncludes
                | Self::StringStartsWith
                | Self::StringEndsWith
                | Self::StringIndexOf
                | Self::StringLastIndexOf
                | Self::StringTrim
                | Self::StringTrimStart
                | Self::StringTrimEnd
                | Self::StringAt
                | Self::StringCodePointAt
                | Self::StringToUpperCase
                | Self::StringToLowerCase
                | Self::StringConcat
                | Self::StringRepeat
                | Self::StringPadStart
                | Self::StringPadEnd
                | Self::StringSplit
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
    const fn derive_math_function(self) -> bool {
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

    /// The string, RegExp and `Math` functions, which `call_native_table` dispatches
    /// directly.
    pub(crate) fn is_string_regexp_or_math(self) -> bool {
        self.is_pure_string_method()
            || self.is_math_function()
            || matches!(
                self,
                Self::RegExpExec
                    | Self::RegExpTest
                    | Self::StringReplace
                    | Self::StringReplaceAll
                    | Self::StringMatch
                    | Self::StringMatchAll
                    | Self::StringSearch
            )
    }

    /// The `Reflect` functions that run an internal method of their target (`Reflect.apply`,
    /// `construct` and the like call functions, and take frames).
    const fn is_reflect_operation(self) -> bool {
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
    pub(crate) fn is_array_callback(self) -> bool {
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
    /// `print` and `String` convert their argument with a coercion and `print` hands the text to
    /// the host's line writer, which reads no frame: a generator's or async function's window
    /// that prints stays frameless.
    const fn derive_runs_beside_activations(self) -> bool {
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
                | Self::Print
                | Self::String
        )
    }

    /// Natives a lane `Call` makes in place from the running frame (`lane_call_native`),
    /// as the general loop's native branch makes them once lane activations are frames:
    /// every native but those whose call the general loop takes as its own step. The
    /// call-forwarding natives push their target as a frame there (and run under a
    /// forwarding boundary), direct `eval` reads the calling instruction, and `with` entry
    /// and exit change the running frame's object environment.
    const fn derive_runs_from_lane(self) -> bool {
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

    const fn derive_host_control_native(self) -> bool {
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
    pub(crate) fn constructs_from_lane(self) -> bool {
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

    pub(crate) fn is_error_constructor(self) -> bool {
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

    pub(crate) fn is_promise_native(self) -> bool {
        matches!(
            self,
            Self::Promise
                | Self::PromiseSpeciesGetter
                | Self::PromiseResolve
                | Self::PromiseReject
                | Self::PromiseTry
                | Self::PromiseWithResolvers
                | Self::PromiseCapabilityExecutor
                | Self::PromiseThen
                | Self::PromiseCatch
                | Self::PromiseFinally
                | Self::PromiseFinallyHandler
                | Self::PromiseFinallyContinuationHandler
                | Self::PromiseAll
                | Self::PromiseAllKeyed
                | Self::PromiseRace
                | Self::PromiseAllSettled
                | Self::PromiseAllSettledKeyed
                | Self::PromiseAny
                | Self::PromiseAggregateJob
                | Self::ArrayFromAsyncFulfilled
                | Self::ArrayFromAsyncRejected
                | Self::DynamicImport
                | Self::AsyncGeneratorDelegateFulfilled
                | Self::AsyncGeneratorDelegateRejected
        )
    }
}
#[derive(Clone, Copy, Debug)]
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
pub(crate) struct Object {
    /// One word: with `ObjectExtras`, `EXTRAS_TAG` and a pointer to them, and they then hold
    /// the inline values and the storage position. Extras are rare, so the common object pays
    /// no word for them. Without extras: the inline values as narrow values
    /// (`Value::to_narrow`), the first slot in the lower half; or, without inline storage,
    /// the storage position (`ValueVec`'s start). An inline value that has no narrow form
    /// moves the values into extras, so every value stays storable.
    payload: u64,
    /// The object's layout (`heap::layouts`): the interned pair of its header words, `meta`
    /// (the prototype's heap index, `PROTO_INDEX_BITS`, with null and undefined sentinels, and
    /// the `OBJECT_*` flags: array, inline storage, and the integrity bits `ValueVec` keeps) and
    /// `auxiliary` (`ValueVec::auxiliary`: the shape, the dictionary, exotic and storage-form
    /// bits). Those facts repeat across every object of a kind, so they live once in the layout
    /// table, and the record keeps four bytes for them where it kept eight.
    layout: u32,
}

/// Bits of `Object::meta` holding the prototype's heap reference. The field is partitioned
/// by reference family: legacy slot indices, then one range per direct record space, then
/// the null and undefined sentinels at the top.
const PROTO_INDEX_BITS: u32 = (1 << 28) - 1;
const PROTO_NULL: u32 = PROTO_INDEX_BITS;
const PROTO_UNDEFINED: u32 = PROTO_INDEX_BITS - 1;
/// Cells a heap may hold so that any of them can be a prototype.
pub(crate) const MAX_HEAP_CELLS: usize = 1 << 27;
const PROTO_OBJECT_RECORD_BASE: u32 = MAX_HEAP_CELLS as u32;
const PROTO_ARRAY_RECORD_BASE: u32 = PROTO_OBJECT_RECORD_BASE + RECORD_SPACE_CAPACITY as u32;
const PROTO_FUNCTION_RECORD_BASE: u32 = PROTO_ARRAY_RECORD_BASE + RECORD_SPACE_CAPACITY as u32;
const PROTO_PROMISE_RECORD_BASE: u32 = PROTO_FUNCTION_RECORD_BASE + RECORD_SPACE_CAPACITY as u32;
const PROTO_ITERATOR_RECORD_BASE: u32 = PROTO_PROMISE_RECORD_BASE + RECORD_SPACE_CAPACITY as u32;
const PROTO_COLLECTION_RECORD_BASE: u32 = PROTO_ITERATOR_RECORD_BASE + RECORD_SPACE_CAPACITY as u32;
const PROTO_RECORD_END: u32 = PROTO_COLLECTION_RECORD_BASE + RECORD_SPACE_CAPACITY as u32;
const _: () = assert!(PROTO_RECORD_END <= PROTO_UNDEFINED);

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

    /// The link naming object record `id`.
    #[inline(always)]
    pub(crate) fn object_record(id: u32) -> Self {
        Self(PROTO_OBJECT_RECORD_BASE + id)
    }

    /// The object record this link names, if it names one.
    #[inline(always)]
    pub(crate) fn object_record_id(self) -> Option<u32> {
        // The object range starts at a power of two above its own power-of-two span, so a
        // link in it holds the record id in its low bits: a mask, which addresses the record
        // without the subtracted base as a 64-bit displacement.
        // `staged::object::proto_object_record_id` over u32, which keeps hot readers' registers free.
        let span = RECORD_SPACE_CAPACITY as u32;
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
            index => {
                debug_assert!(index < PROTO_RECORD_END);
                Value::direct_collection(index - PROTO_COLLECTION_RECORD_BASE)
            }
        }
    }
}

const OBJECT_ARRAY: u32 = 1 << 28;
const OBJECT_INLINE_STORAGE: u32 = 1 << 29;
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

const EXTRAS_TAG: u64 = 0x7fff_0000_0000_0000;
/// The payload bit of a plain object whose values are a narrow run (`value_vec::NARROW_FORM`):
/// the word is then the run's start beside it, so the readers that take a payload with a zero
/// upper half as a plain arena start (`plain_storage_start`, `plain_arena_start`) refuse it,
/// and it is no extras tag.
const NARROW_RUN_TAG: u64 = 1 << u32::BITS;
const EXTRAS_TAG_MASK: u64 = 0xffff_0000_0000_0000;
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
pub(super) const NATIVE_OBJECT_LAYOUT_OFFSET: usize = std::mem::offset_of!(Object, layout);
pub(super) const NATIVE_OBJECT_META_OFFSET: usize = std::mem::size_of::<u32>();
pub(super) const NATIVE_OBJECT_AUXILIARY_OFFSET: usize = 0;

/// An object's two header words as its layout's entry holds them (`heap::layouts`): the
/// auxiliary word low, `meta` high.
#[inline(always)]
const fn header_word(meta: u32, auxiliary: u32) -> u64 {
    auxiliary as u64 | (meta as u64) << u32::BITS
}
pub(super) const NATIVE_OBJECT_PAYLOAD_OFFSET: usize = std::mem::offset_of!(Object, payload);
pub(super) const NATIVE_OBJECT_EXTRAS_ARENA_START_OFFSET: usize =
    std::mem::offset_of!(ObjectExtras, arena_start);
pub(super) const NATIVE_OBJECT_EXTRAS_INLINE_VALUES_OFFSET: usize =
    std::mem::offset_of!(ObjectExtras, inline_properties) + std::mem::offset_of!(DisplacedInline, 0);
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
        Self { payload, layout: self.layout }
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
pub(super) const NATIVE_ARRAY_EXTRAS_ELEMENTS_OFFSET: usize = std::mem::offset_of!(ObjectExtras, elements);
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

impl ArrayObject {
    /// A new array of `prototype` with no properties: `new` of `Object::new(prototype)`,
    /// made with its arena storage at once.
    #[inline(always)]
    pub(crate) fn fresh(prototype: Value, elements: super::ArrayElements) -> Self {
        // The layout depends on the prototype alone: the last one's is kept, so a run of new
        // arrays (every literal of one realm) interns nothing.
        #[thread_local]
        static mut LAST: [(u32, u32); 16] = [(u32::MAX, 0); 16];
        let meta = Object::encode_array_proto(prototype) | OBJECT_ARRAY;
        let at = (meta.wrapping_mul(0x9e37_79b9) >> 28) as usize;
        // SAFETY: this thread's own memo; no reference to it outlives the read.
        let (key, layout) = unsafe { (*std::ptr::addr_of!(LAST))[at] };
        let object = if key == meta {
            Object { payload: u64::from(EMPTY_POSITION), layout }
        } else {
            let mut object = Object::from_header(meta, 0, 0);
            object.initialize_properties(ValueVec::new());
            // SAFETY: as above.
            unsafe { (*std::ptr::addr_of_mut!(LAST))[at] = (meta, object.layout) };
            object
        };
        let mut object = object;
        debug_assert!(object.proto() == prototype);
        debug_assert!(object.extras_pointer().is_none() && object.payload == u64::from(EMPTY_POSITION));
        // No extras and no property store: the payload word takes the backing.
        object.payload = elements.into_raw();
        Self { object }
    }

    pub(crate) fn new(mut object: Object, elements: impl Into<super::ArrayElements>) -> Self {
        let elements = elements.into();
        // Without extras before or after, the array form is a function of the object's layout
        // alone (its payload ends as the empty position): the last ones are kept.
        #[thread_local]
        static mut LAST: [(u32, u32); 16] = [(u32::MAX, 0); 16];
        let input = object.layout;
        let at = (input.wrapping_mul(0x9e37_79b9) >> 28) as usize;
        let plain = object.extras_pointer().is_none();
        // SAFETY: this thread's own memo; no reference to it outlives the read.
        let (key, layout) = unsafe { (*std::ptr::addr_of!(LAST))[at] };
        if plain && key == input {
            debug_assert!(object.inline_properties().is_none_or(|values| values.iter().all(|value| value.is_undefined())));
            debug_assert!(object.inline_properties().is_some() || object.payload == u64::from(EMPTY_POSITION));
            object.layout = layout;
            object.payload = u64::from(EMPTY_POSITION);
            return Self::attach(object, elements);
        }
        let object = Self::array_form(object);
        if plain && object.extras_pointer().is_none() {
            debug_assert_eq!({ object.payload }, u64::from(EMPTY_POSITION));
            // SAFETY: as above.
            unsafe { (*std::ptr::addr_of_mut!(LAST))[at] = (input, object.layout) };
        }
        Self::attach(object, elements)
    }

    /// `object` in the array form `new` gives it.
    fn array_form(mut object: Object) -> Object {
        // Read as a plain object until its storage takes the array form below.
        object.set_meta(object.meta() & !OBJECT_ARRAY);
        // Array objects keep named properties in the arena; a fresh object has none inline.
        let current = object.properties();
        if current.has_inline_property_storage() {
            debug_assert!(
                object
                    .inline_properties()
                    .is_some_and(|values| values.iter().all(|value| value.is_undefined()))
            );
            let mut properties = ValueVec::new();
            properties.set_auxiliary(current.auxiliary());
            properties.preserve_integrity_from(current);
            object.initialize_properties(properties);
        }
        // An array without extras holds no property store (`set_storage_position`).
        if object.extras_pointer().is_none() && object.array_store_needs_extras(object.payload as u32) {
            object.extras_mut();
        }
        object.set_meta(object.meta() | OBJECT_ARRAY);
        object
    }

    /// The array of `object`, an array's object in its array form (`new`), and `elements`:
    /// they go to its extras, or to its payload word where it has none.
    fn attach(mut object: Object, elements: super::ArrayElements) -> Self {
        debug_assert!(object.is_array());
        match object.extras_mut_existing() {
            Some(extras) => extras.elements = elements,
            None => {
                debug_assert!(object.payload == u64::from(EMPTY_POSITION));
                object.payload = elements.into_raw();
            }
        }
        Self { object }
    }

    #[inline(always)]
    pub(crate) fn elements(&self) -> &super::ArrayElements {
        match self.object.extras() {
            Some(extras) => &extras.elements,
            // SAFETY: without extras, the payload word is the backing this array owns.
            None => unsafe { super::ArrayElements::from_raw_ref(std::ptr::addr_of!(self.object.payload)) },
        }
    }

    /// The elements for any change, including their backing's: the running lane's array
    /// storage proofs retire first (`retire_array_storage_proofs`).
    #[inline(always)]
    pub(crate) fn elements_mut(&mut self) -> &mut super::ArrayElements {
        self.announce_shrinking();
        self.elements_raw_mut()
    }

    /// The elements for a change no proof observes (`elements_mut` announces one first).
    #[inline(always)]
    fn elements_raw_mut(&mut self) -> &mut super::ArrayElements {
        match self.object.extras_pointer() {
            // SAFETY: owned by this array's object, and `&mut self` makes the borrow unique.
            Some(extras) => unsafe { &mut (*extras).elements },
            // SAFETY: as in `elements`, with `&mut self` making the borrow unique.
            None => unsafe { super::ArrayElements::from_raw_mut(std::ptr::addr_of_mut!(self.object.payload)) },
        }
    }

    /// What a change to this array's elements that a proof may not bound retires: the proofs
    /// that read a prototype's elements, and the running lane's array storage proofs. A change
    /// the staged `array_pop` makes in place announces itself here, as `elements_mut` does.
    #[inline(always)]
    pub(crate) fn announce_shrinking(&self) {
        // A prototype's elements answer index reads of the objects it is the prototype of.
        self.object.note_prototype_elements_change();
        super::array_proofs::retire_array_storage_proofs();
    }

    /// Whether it owns nothing at all: no extras, so no property store, and the shared empty
    /// header as its elements, one compare of its payload word (`[]` until it grows).
    #[inline(always)]
    pub(crate) fn owns_nothing(&self) -> bool {
        self.object.payload == super::array_elements::empty_header_word()
    }

    /// Whether a dead array owns anything its drop frees: extras, or an element allocation
    /// it counts among its backing's owners. Young runs and the empty header need none.
    #[inline] pub(crate) fn needs_drop(&self) -> bool {
        self.object.extras_pointer().is_some() || self.elements().owner_count() != 0
    }

    /// Copies a young run of elements to an allocation of the array's own
    /// (`ArrayElements::leave_nursery`): the same values, so no proof or dependent retires.
    pub(crate) fn leave_nursery(&mut self) {
        self.elements_raw_mut().leave_nursery();
    }

    /// `ArrayElements::survive_nursery`: the same values, so no proof or dependent retires.
    pub(crate) fn survive_nursery(&mut self) {
        self.elements_raw_mut().survive_nursery();
    }

    /// `ArrayElements::widen`: the same values, so no dependent retires; a proof of the
    /// narrow backing proves no index (`ArrayStorageProofs::prove`), so none reads it.
    #[inline]
    pub(crate) fn widen_elements(&mut self) {
        if self.elements().is_encoded() {
            self.elements_raw_mut().widen();
        }
    }

    /// `ArrayElements::narrow_in_place`: the same values, so no proof or dependent retires.
    pub(crate) fn narrow_elements(&mut self) -> usize {
        self.elements_raw_mut().narrow_in_place()
    }

    /// Replaces the present element at `index` in place where this array alone owns its
    /// backing (`ArrayElements::replace_in_place`); `false` changes nothing. A replacement
    /// changes no backing, so no array storage proof retires; a prototype's dependents do.
    #[inline(always)]
    pub(crate) fn replace_in_place(&mut self, index: usize, value: Value) -> bool {
        if !self.elements_raw_mut().replace_in_place(index, value) {
            return false;
        }
        self.object.note_prototype_elements_change();
        true
    }

    /// The first value of the backing this array alone owns, which an array storage proof
    /// replaces elements through (`ArrayStorageProofs::replace`); `None` where it is shared.
    #[inline(always)]
    pub(crate) fn unique_values(&mut self) -> Option<std::ptr::NonNull<Value>> {
        self.elements_raw_mut().unique_values()
    }
}

impl Clone for ArrayObject {
    fn clone(&self) -> Self {
        // `Object::clone` copies extras, their backing with them, and leaves the word of an
        // array without extras empty.
        let mut object = self.object.clone();
        if object.extras_pointer().is_none() {
            object.payload = self.elements().clone().into_raw();
        }
        Self { object }
    }
}

impl Drop for ArrayObject {
    fn drop(&mut self) {
        // Extras drop their backing with the object; without them, the payload word owns it.
        if self.object.extras_pointer().is_none() {
            // SAFETY: the payload word is the backing this array owns, taken once here.
            drop(unsafe { super::ArrayElements::from_raw(self.object.payload) });
            self.object.payload = u64::from(EMPTY_POSITION);
        }
    }
}

impl std::ops::Deref for ArrayObject {
    type Target = Object;
    #[inline] fn deref(&self) -> &Object {
        &self.object
    }
}

impl std::ops::DerefMut for ArrayObject {
    fn deref_mut(&mut self) -> &mut Object {
        &mut self.object
    }
}

impl std::fmt::Debug for ArrayObject {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ArrayObject")
            .field("object", &self.object)
            .field("elements", self.elements())
            .finish()
    }
}

impl std::fmt::Debug for Object {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Object")
            .field("proto", &self.proto())
            .field("properties", &self.properties())
            .field("inline_properties", &self.inline_values())
            .field("extras", &self.extras())
            .finish()
    }
}

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

#[derive(Clone, Debug, Default)]
#[repr(C)]
struct ObjectExtras {
    prototype_lookup: PrototypeLookupState,
    /// The object's inline property values, displaced from its payload by the extras pointer.
    inline_properties: DisplacedInline,
    /// An array's storage position, displaced from its first payload word.
    arena_start: u32,
    /// An array's element backing, displaced from its payload word (`ArrayObject`); empty for
    /// any other object. Native code reads it at a fixed offset, so it follows the fields that
    /// native code reads, which keep theirs.
    elements: super::ArrayElements,
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
            Some(HeapReference::CollectionRecord(id)) => PROTO_COLLECTION_RECORD_BASE + id,
            Some(HeapReference::AwaitRecord(_)) => unreachable!("an await state is never a prototype"),
            Some(HeapReference::StringRecord(_)) => unreachable!("a string is never a prototype"),
            Some(HeapReference::V128Record(_)) => unreachable!("a Wasm vector is never a prototype"),
            Some(HeapReference::EnvironmentRecord(_)) => unreachable!("an environment is never a prototype"),
            Some(HeapReference::SymbolRecord(_)) => unreachable!("a symbol is never a prototype"),
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

    /// A function's creation realm when it is not the main realm (`FunctionRecord::realm_override`).
    pub(crate) fn function_realm_override(&self) -> Option<Value> {
        self.extras()?.rare()?.function_realm
    }

    pub(crate) fn set_function_realm_override(&mut self, realm: Value) {
        self.extras_mut().rare_mut().function_realm = Some(realm);
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
        if let Some(realm) = extras.function_realm {
            visit(realm);
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

#[cfg(test)]
mod object_payload_tests {
    use super::Object;
    use crate::value::Value;
    use crate::value_vec::{INLINE_PROPERTY_COUNT, ValueVec, ValueArena};

    #[test]
    fn out_of_line_extras_preserve_inline_values_and_clone_state() {
        let inline: [Value; INLINE_PROPERTY_COUNT] =
            std::array::from_fn(|slot| Value::integer(11 + 2 * slot as i32));
        let mut object =
            Object::with_property_storage(Value::NULL, ValueVec::inline_property_storage(7), inline);
        object.set_arguments_object();
        assert!(object.is_arguments_object());
        assert_eq!(object.shape(), 7);
        assert_eq!(object.inline_properties(), Some(inline));

        let mut cloned = object.clone();
        cloned.set_inline_property(0, Value::integer(17));
        assert_eq!(cloned.inline_properties().unwrap()[0], Value::integer(17));
        assert_eq!(object.inline_properties().unwrap()[0], Value::integer(11));
        assert!(cloned.is_arguments_object());

        object.set_shape(19, &mut ValueArena::default());
        assert_eq!(object.shape(), 19);
        assert!(object.is_arguments_object());
    }

    #[test]
    fn prototype_references_round_trip_through_every_family() {
        for proto in [
            Value::NULL,
            Value::UNDEFINED,
            Value::heap(0),
            Value::heap(super::MAX_HEAP_CELLS as u32 - 1),
            Value::direct_object(0),
            Value::direct_object(super::RECORD_SPACE_CAPACITY as u32 - 1),
            Value::direct_array(0),
            Value::direct_array(super::RECORD_SPACE_CAPACITY as u32 - 1),
        ] {
            let mut object = Object::new(proto);
            assert_eq!(object.proto(), proto);
            object.set_proto(Value::NULL);
            object.set_proto(proto);
            assert_eq!(object.proto(), proto);
        }
    }

    #[test]
    fn out_of_line_extras_keep_arena_storage_mode_and_integrity() {
        let mut properties = ValueVec::new();
        properties.set_extensible(false);
        let mut object =
            Object::with_property_storage(Value::NULL, properties, [Value::UNDEFINED; INLINE_PROPERTY_COUNT]);
        object.set_module_namespace();
        assert!(object.is_module_namespace());
        assert!(!object.properties().has_inline_property_storage());
        assert!(!object.is_extensible());
    }
}

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

impl EnvironmentSlots {
    /// Dense backing storage for a cache that already owns the environment's lifetime.
    /// The pointer expires when the storage is replaced or collected.
    #[inline(always)]
    pub(crate) fn dense_storage(&self) -> Option<*const usize> {
        Some(self.dense()?.0.storage().as_ptr().cast_const())
    }

    /// # Safety
    /// A non-null pointer came from `dense_storage`, whose owner and dense layout remain
    /// live. The returned descriptor must stay within that lifetime; its elements obey
    /// normal aliasing rules. Only the scoped read/write/bridge operations below use it.
    #[inline(always)]
    unsafe fn borrow_storage(storage: *const usize) -> Option<std::mem::ManuallyDrop<Self>> {
        let header = std::ptr::NonNull::new(storage.cast_mut())?;
        // SAFETY: the caller keeps the original storage live. Rewrap the descriptor
        // immediately so neither wrapper can destroy the original owner's elements.
        let slots = unsafe { super::thin_slice::ThinBoxSlice::borrow_storage(header) };
        Some(std::mem::ManuallyDrop::new(Self(std::mem::ManuallyDrop::into_inner(slots))))
    }

    /// Records `parent`, the dense storage of the parent environment of the environment whose
    /// dense storage is at `storage`, for collection `epoch` (`linked_parent`). Within one
    /// collection a live environment's storage never moves, and each collection names a new
    /// epoch, so a link is read only while both storages it joins stand.
    ///
    /// # Safety
    /// As `linked_parent`, for both storages.
    #[inline(always)]
    pub(crate) unsafe fn link(storage: *const usize, parent: *const usize, epoch: u64) {
        type Slice = super::thin_slice::ThinBoxSlice<EnvironmentSlot, ENVIRONMENT_PREFIX_WORDS>;
        // SAFETY: as the caller guarantees; an empty storage shares the static header, whose
        // prefix is never written.
        unsafe {
            if *storage & !super::thin_slice::MARK_BIT == 0 {
                return;
            }
            let prefix = Slice::prefix(std::ptr::NonNull::new_unchecked(storage.cast_mut()));
            *prefix.add(1) = parent as usize;
            *prefix = epoch.wrapping_add(1) as usize;
        }
    }

    /// # Safety
    /// `storage` is null or live as for `dense_storage`; no element is mutably borrowed.
    #[inline(always)]
    pub(crate) unsafe fn read_storage(storage: *const usize, slot: u16) -> Option<Value> {
        // SAFETY: as guaranteed by the caller; each slot is a `Value` (`repr(transparent)`).
        unsafe { let (values, len) = Self::borrow_storage(storage)?.0.run(); crate::staged::environment::concrete_dense_capture(values.cast(), len, slot) }
    }

    /// Write through a cache whose caller has already admitted the store and taken its barrier:
    /// `staged::environment::dense_capture_store`, as the native tiers store a capture.
    /// # Safety
    /// As `read_storage`, with exclusive access to the element for this operation and the
    /// environment's write authorization/barrier satisfied. No reference escapes the write.
    #[inline(always)]
    pub(crate) unsafe fn write_storage(storage: *const usize, slot: usize, value: Value) -> Option<()> {
        // SAFETY: as guaranteed by the caller; the view's values are live for this write only.
        let (values, len) = unsafe { Self::native_storage_view(storage) }?;
        // SAFETY: the caller's exclusive access and authorization, over the view's `len` values.
        unsafe { crate::staged::environment::concrete_dense_capture_store(values.cast_mut(), len, u16::try_from(slot).ok()?, value) }
            .then_some(())
    }

    /// Native entry's values/count projection of the same cached storage.
    /// # Safety
    /// As `read_storage`; the returned pointer expires with the original storage.
    #[inline(always)]
    pub(crate) unsafe fn native_storage_view(storage: *const usize) -> Option<(*const Value, usize)> {
        // SAFETY: the original owner, not this temporary descriptor, owns the result.
        unsafe { Self::borrow_storage(storage) }?.native_dense_view()
    }

    /// A dense slot view for native reads. The caller must keep the environment live and
    /// discard the pointer at a safepoint, where collection or mutation can replace storage.
    #[inline]
    pub(crate) fn native_dense_view(&self) -> Option<(*const Value, usize)> {
        let dense = self.dense()?;
        const { assert!(std::mem::size_of::<EnvironmentSlot>() == std::mem::size_of::<Value>()) };
        const { assert!(std::mem::align_of::<EnvironmentSlot>() == std::mem::align_of::<Value>()) };
        Some((dense.0.as_ptr().cast::<Value>(), dense.0.len()))
    }

    /// The logical length and default owner of sparse storage.
    #[inline(always)]
    fn sparse(&self) -> Option<(usize, Value)> {
        if !self.0.marked() {
            return None;
        }
        let length = self.0.first()?.0.as_environment_index()?;
        let owner = self.0.get(1)?.0.shared_environment_owner()?;
        Some((length as usize, owner))
    }

    /// The listed sparse entry for `slot`, as its storage position.
    fn sparse_position(&self, slot: usize) -> Option<usize> {
        let slot = u32::try_from(slot).ok()?;
        let entries = self.0.len().saturating_sub(SPARSE_ENTRIES_START) / SPARSE_ENTRY_WORDS;
        let position = |entry: usize| SPARSE_ENTRIES_START + entry * SPARSE_ENTRY_WORDS;
        let (mut low, mut high) = (0, entries);
        while low < high {
            let middle = (low + high) / 2;
            match self.0[position(middle)].0.cmp_environment_index(slot) {
                std::cmp::Ordering::Equal => return Some(position(middle) + 1),
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
            }
        }
        None
    }

    pub(crate) fn len(&self) -> usize {
        self.sparse().map_or(self.0.len(), |(length, _)| length)
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// What `slot` holds: a value this environment owns, or the owner it shares the slot with.
    #[inline(always)]
    pub(crate) fn state(&self, slot: usize) -> Option<EnvironmentSlotState> {
        if self.0.marked() {
            return self.sparse_state(slot);
        }
        Some(self.0.get(slot)?.state())
    }
    /// `state` of sparse storage, kept out of the dense read's line.
    #[inline(never)]
    fn sparse_state(&self, slot: usize) -> Option<EnvironmentSlotState> {
        let (length, owner) = self.sparse()?;
        if slot >= length {
            return None;
        }
        Some(match self.sparse_position(slot) {
            Some(position) => self.0[position].state(),
            None => EnvironmentSlotState::Shared(owner),
        })
    }
    /// This storage when it is dense, which a caller may then read with `dense_plain`.
    #[inline(always)]
    pub(crate) fn dense(&self) -> Option<&Self> {
        (!self.0.marked()).then_some(self)
    }
    /// A dense slot's ordinary value (storage `dense` returned): `None` past the storage or
    /// for any internal marker it holds (`Value::is_internal_marker`: a shared slot, an
    /// uninitialized binding), which the caller resolves the long way. One tag test answers
    /// both the shared-slot and the initialization check.
    #[inline(always)]
    pub(crate) fn dense_plain(&self, slot: usize) -> Option<Value> {
        debug_assert!(!self.0.marked());
        let value = self.0.get(slot)?.0;
        (!value.is_internal_marker()).then_some(value)
    }
    /// The value of a slot this environment owns; a shared slot lives in its owner.
    #[inline(always)]
    pub(crate) fn owned(&self, slot: usize) -> Option<Value> {
        match self.state(slot)? {
            EnvironmentSlotState::Owned(value) => Some(value),
            EnvironmentSlotState::Shared(_) => None,
        }
    }
    pub(crate) fn owned_mut(&mut self, slot: usize) -> Option<&mut Value> {
        let position = match self.sparse() {
            None => slot,
            Some((length, _)) if slot < length => self.sparse_position(slot)?,
            Some(_) => return None,
        };
        self.0.get_mut(position)?.owned_mut()
    }
    /// Appends the heap values `roots` yields (owned heap values, and the owner of each
    /// shared slot) to `out`. A dense storage takes a branch-free compaction over its words:
    /// a loop's per-iteration copy repeats every binding of its function, which the filtered
    /// iterator decoded at ~20 Ir a slot.
    #[inline]
    pub(crate) fn extend_heap_roots(&self, out: &mut Vec<Value>) {
        if self.0.marked() {
            out.extend(self.roots().filter(|value| value.is_heap()));
            return;
        }
        let slots: &[EnvironmentSlot] = &self.0;
        out.reserve(slots.len());
        let start = out.len();
        let mut kept = 0;
        for slot in slots {
            let (value, heap) = slot.0.environment_slot_edge();
            // SAFETY: `kept` never passes the slots read so far, so every write lands within
            // the `slots.len()` values reserved past `start`.
            unsafe { out.as_mut_ptr().add(start + kept).write(value) };
            kept += usize::from(heap);
        }
        // SAFETY: the first `kept` values past `start` were written above.
        unsafe { out.set_len(start + kept) };
    }

    pub(crate) fn roots(&self) -> impl Iterator<Item = Value> + '_ {
        // A sparse storage's index words are not values; its owner and slots are.
        let sparse = self.0.marked();
        self.0
            .iter()
            .filter(move |slot| !sparse || slot.0.as_environment_index().is_none())
            .map(|slot| match slot.state() {
                EnvironmentSlotState::Owned(value) | EnvironmentSlotState::Shared(value) => value,
            })
    }

    /// The owner a sparse storage's unlisted slots share.
    pub(crate) fn default_owner(&self) -> Option<Value> {
        self.sparse().map(|(_, owner)| owner)
    }

    /// Clears every slot whose value no code reads, as `use_of` tells: an unread slot becomes
    /// an uninitialized binding (`Value::DELETED`) this environment owns, and the value it owned
    /// or owner it shared may be swept. A slot read only for whether it is initialized keeps
    /// that state: an initialized value it owns becomes `undefined`, and a shared slot stays
    /// shared with its owner, which the caller keeps. A sparse storage's unlisted slots stay
    /// shared with its default owner, which the caller keeps too.
    pub(crate) fn clear_unread(&mut self, use_of: impl Fn(usize) -> SlotUse) {
        let clear = |slot: &mut EnvironmentSlot, index: usize| match use_of(index) {
            SlotUse::Value => {}
            SlotUse::Unread => *slot = EnvironmentSlot::owned(Value::DELETED),
            SlotUse::Initialization => {
                if matches!(slot.state(), EnvironmentSlotState::Owned(value) if !value.is_deleted()) {
                    *slot = EnvironmentSlot::owned(Value::UNDEFINED);
                }
            }
        };
        if self.0.marked() {
            let entries = self.0.len().saturating_sub(SPARSE_ENTRIES_START) / SPARSE_ENTRY_WORDS;
            for entry in 0..entries {
                let position = SPARSE_ENTRIES_START + entry * SPARSE_ENTRY_WORDS;
                if let Some(index) = self.0[position].0.as_environment_index() {
                    clear(&mut self.0[position + 1], index as usize);
                }
            }
        } else {
            for (index, slot) in self.0.iter_mut().enumerate() {
                clear(slot, index);
            }
        }
    }

    /// The indexes a sparse storage lists, ascending; none for dense storage.
    pub(crate) fn sparse_indexes(&self) -> impl Iterator<Item = usize> + '_ {
        let entries = if self.0.marked() {
            self.0.len().saturating_sub(SPARSE_ENTRIES_START) / SPARSE_ENTRY_WORDS
        } else {
            0
        };
        (0..entries).filter_map(move |entry| {
            self.0[SPARSE_ENTRIES_START + entry * SPARSE_ENTRY_WORDS]
                .0
                .as_environment_index()
                .map(|index| index as usize)
        })
    }

    /// `dense` stored sparsely around the owner most of its slots share, when it reaches
    /// `SPARSE_CLONE_MIN_SLOTS` and the sparse words are fewer; a copy that owns most of its
    /// slots stays dense, whose reads need no search.
    fn compacted(dense: super::thin_slice::ThinBoxSlice<EnvironmentSlot, ENVIRONMENT_PREFIX_WORDS>) -> Option<Self> {
        let len = dense.len();
        if len < SPARSE_CLONE_MIN_SLOTS {
            return Some(Self(dense));
        }
        let mut owners: Vec<(Value, usize)> = Vec::new();
        for slot in dense.iter() {
            if let EnvironmentSlotState::Shared(owner) = slot.state() {
                match owners.iter_mut().find(|(candidate, _)| *candidate == owner) {
                    Some((_, count)) => *count += 1,
                    None => owners.push((owner, 1)),
                }
            }
        }
        let Some((owner, sharing)) = owners.into_iter().max_by_key(|(_, count)| *count) else {
            return Some(Self(dense));
        };
        let listed = len - sharing;
        if SPARSE_ENTRIES_START + listed * SPARSE_ENTRY_WORDS >= len {
            return Some(Self(dense));
        }
        let shared = Value::shared_environment_slot(owner);
        let mut words = Vec::with_capacity(SPARSE_ENTRIES_START + listed * SPARSE_ENTRY_WORDS);
        words.push(EnvironmentSlot(Value::environment_index(u32::try_from(len).ok()?)));
        words.push(EnvironmentSlot(shared));
        for (index, slot) in dense.iter().enumerate() {
            if slot.0 != shared {
                words.push(EnvironmentSlot(Value::environment_index(u32::try_from(index).ok()?)));
                words.push(*slot);
            }
        }
        let mut storage = super::thin_slice::ThinBoxSlice::from_exact_iter(words.into_iter());
        storage.mark();
        Some(Self(storage))
    }

    /// The slots of a per-iteration copy of `environment`, which holds these slots: a `reset`
    /// slot is `undefined`, a `fresh` slot owns its current value (`owned_value` reads a
    /// shared slot from its owner), and every other slot shares with the slot's current
    /// owner. The copy's default owner is the one most slots share, so a copy of a copy
    /// stays sparse, and a sparse source is copied through the slots it lists and the
    /// fresh and reset ones alone. `fresh` and `reset` are sorted.
    /// `iteration_copy` of this dense storage as sparse storage around `environment`: `None`
    /// when the listed slots would not save words (the caller then copies densely), and
    /// `Some(None)` when a fresh slot's owner has no value.
    fn sparse_copy_of_dense(
        &self,
        environment: Value,
        fresh: &[u16],
        reset: crate::bytecode::Resets<'_>,
        owned_value: &impl Fn(Value, usize) -> Option<Value>,
    ) -> Option<Option<Self>> {
        let len = self.0.len();
        let default = Value::shared_environment_slot(environment);
        let fits = |listed: usize| SPARSE_ENTRIES_START + listed * SPARSE_ENTRY_WORDS < len;
        let mut resets = reset.iter().peekable();
        let mut fresh = fresh.iter().copied().peekable();
        let mut listed: smallvec::SmallVec<[(u32, EnvironmentSlot); 8]> = smallvec::SmallVec::new();
        for (index, word) in self.0.iter().enumerate() {
            let slot = u16::try_from(index).ok()?;
            let is_reset = resets.next_if_eq(&slot).is_some();
            let is_fresh = fresh.next_if_eq(&slot).is_some();
            let copied = if is_reset {
                EnvironmentSlot::owned(Value::UNDEFINED)
            } else if is_fresh {
                EnvironmentSlot::owned(match word.state() {
                    EnvironmentSlotState::Owned(value) => value,
                    EnvironmentSlotState::Shared(owner) => match owned_value(owner, index) {
                        Some(value) => value,
                        None => return Some(None),
                    },
                })
            } else if word.0.is_shared_environment_slot() && word.0 != default {
                *word
            } else {
                continue;
            };
            listed.push((u32::from(slot), copied));
            if !fits(listed.len()) {
                return None;
            }
        }
        let header = Value::environment_index(u32::try_from(len).ok()?);
        let mut storage = super::thin_slice::ThinBoxSlice::try_from_fn(
            SPARSE_ENTRIES_START + listed.len() * SPARSE_ENTRY_WORDS,
            |word| {
                Some(match word {
                    0 => EnvironmentSlot(header),
                    1 => EnvironmentSlot(default),
                    _ => {
                        let entry = word - SPARSE_ENTRIES_START;
                        let (index, slot) = listed[entry / SPARSE_ENTRY_WORDS];
                        if entry % SPARSE_ENTRY_WORDS == 0 { EnvironmentSlot(Value::environment_index(index)) } else { slot }
                    }
                })
            },
        )?;
        storage.mark();
        Some(Some(Self(storage)))
    }

    pub(crate) fn iteration_copy(
        &self,
        environment: Value,
        fresh: &[u16],
        reset: crate::bytecode::Resets<'_>,
        owned_value: impl Fn(Value, usize) -> Option<Value>,
    ) -> Option<Self> {
        let len = self.len();
        let copy_slot = |index: usize| -> Option<EnvironmentSlot> {
            let slot = u16::try_from(index).ok()?;
            if reset.contains(slot) {
                return Some(EnvironmentSlot::owned(Value::UNDEFINED));
            }
            let fresh = fresh.binary_search(&slot).is_ok();
            Some(match self.state(index)? {
                EnvironmentSlotState::Owned(value) if fresh => EnvironmentSlot::owned(value),
                EnvironmentSlotState::Owned(_) => EnvironmentSlot::shared(environment),
                EnvironmentSlotState::Shared(owner) if fresh => {
                    EnvironmentSlot::owned(owned_value(owner, index)?)
                }
                EnvironmentSlotState::Shared(owner) => EnvironmentSlot::shared(owner),
            })
        };
        let Some((_, owner)) = self.sparse() else {
            // A wide dense source (a function's environment, copied for each iteration of a
            // loop whose closures escape) is copied sparsely around `environment` in one pass:
            // its own slots become shared with it, the copy's default owner, so only the
            // fresh, reset and otherwise-shared slots are listed. The copy then costs a scan of
            // the words and one allocation of its listed slots, not a dense copy, a select per
            // word, an owner census and a second allocation (a 66-binding function's loop:
            // the copy was most of 7,000 instructions per iteration).
            if len >= SPARSE_CLONE_MIN_SLOTS
                && let Some(copy) = self.sparse_copy_of_dense(environment, fresh, reset, &owned_value)
            {
                return copy;
            }
            // Every slot shares with its current owner, then the fresh and reset ones own: one
            // copy of the words, then a select per word (a shared word names its owner already).
            let shared = EnvironmentSlot::shared(environment);
            let mut copy = super::thin_slice::ThinBoxSlice::from_slice(&self.0);
            for slot in copy.iter_mut() {
                if !slot.0.is_shared_environment_slot() {
                    *slot = shared;
                }
            }
            for &slot in fresh {
                let index = usize::from(slot);
                let Some(source) = self.0.get(index) else {
                    continue;
                };
                copy[index] = EnvironmentSlot::owned(match source.state() {
                    EnvironmentSlotState::Owned(value) => value,
                    EnvironmentSlotState::Shared(owner) => owned_value(owner, index)?,
                });
            }
            for slot in reset.iter() {
                if let Some(target) = copy.get_mut(usize::from(slot)) {
                    *target = EnvironmentSlot::owned(Value::UNDEFINED);
                }
            }
            return Self::compacted(copy);
        };
        // Every slot the source does not list shares with `owner`, as the copy's do unless
        // they are fresh or reset.
        // Each list is ascending (sparse entries by position, `fresh` and `reset` sorted), so
        // their union is merged, never sorted.
        let mut indexes = merge_ascending(
            merge_ascending(self.sparse_indexes(), fresh.iter().map(|&slot| usize::from(slot)))
                .into_iter(),
            reset.iter().map(usize::from),
        );
        indexes.truncate(indexes.partition_point(|&index| index < len));
        let shared = Value::shared_environment_slot(owner);
        let mut listed: smallvec::SmallVec<[(u32, EnvironmentSlot); 16]> = smallvec::SmallVec::new();
        for index in indexes {
            let slot = copy_slot(index)?;
            if slot.0 != shared {
                listed.push((u32::try_from(index).ok()?, slot));
            }
        }
        if SPARSE_ENTRIES_START + listed.len() * SPARSE_ENTRY_WORDS >= len {
            return super::thin_slice::ThinBoxSlice::try_from_fn(len, copy_slot).map(Self);
        }
        // Written straight into the copy's one allocation: the length, the default owner, then
        // each listed index and slot.
        let header = Value::environment_index(u32::try_from(len).ok()?);
        let mut storage = super::thin_slice::ThinBoxSlice::try_from_fn(
            SPARSE_ENTRIES_START + listed.len() * SPARSE_ENTRY_WORDS,
            |word| {
                Some(match word {
                    0 => EnvironmentSlot(header),
                    1 => EnvironmentSlot(shared),
                    _ => {
                        let entry = word - SPARSE_ENTRIES_START;
                        let (index, slot) = listed[entry / SPARSE_ENTRY_WORDS];
                        if entry % SPARSE_ENTRY_WORDS == 0 { EnvironmentSlot(Value::environment_index(index)) } else { slot }
                    }
                })
            },
        )?;
        storage.mark();
        Some(Self(storage))
    }

    /// Slots that own `values`, in order.
    /// Dense storage of the `len` owned slots that follow its `IN_PLACE_SLOTS_WORDS` words
    /// at `words`, as `ThinBoxSlice::lay_out_in_place`: a view its caller never drops.
    ///
    /// # Safety
    /// As `ThinBoxSlice::lay_out_in_place`; each slot is an owned value.
    #[inline(always)]
    pub(crate) unsafe fn lay_out_in_place(words: *mut Value, len: usize) -> *const Self {
        // SAFETY: as the caller guarantees; a slot is a `Value`, a word.
        unsafe { super::thin_slice::ThinBoxSlice::<EnvironmentSlot, ENVIRONMENT_PREFIX_WORDS>::lay_out_in_place(words.cast(), len).cast() }
    }

    /// `len` slots, each `DELETED` but slot `k` of each bit `k` of `captured`, which holds
    /// `values[k]`: one fill of the storage, then one write per captured slot.
    ///
    /// # Safety
    /// `values` holds a readable value at every index `captured` names, each below `len`,
    /// and none of them shares a slot (`Value::shared_environment_owner`).
    pub(crate) unsafe fn from_captured(len: usize, captured: u64, values: *const Value) -> Self {
        let mut slots = super::thin_slice::ThinBoxSlice::filled(len, EnvironmentSlot(Value::DELETED));
        let mut remaining = captured;
        while remaining != 0 {
            let slot = remaining.trailing_zeros() as usize;
            debug_assert!(slot < len);
            // SAFETY: as the caller guarantees.
            slots[slot] = EnvironmentSlot::owned(unsafe { *values.add(slot) });
            remaining &= remaining - 1;
        }
        Self(slots)
    }

    pub(crate) fn from_values(values: impl ExactSizeIterator<Item = Value>) -> Self {
        Self(super::thin_slice::ThinBoxSlice::from_exact_iter(
            values.map(EnvironmentSlot::owned),
        ))
    }
}

impl From<Vec<Value>> for EnvironmentSlots {
    fn from(values: Vec<Value>) -> Self {
        Self::from_values(values.into_iter())
    }
}

impl From<Box<[Value]>> for EnvironmentSlots {
    fn from(values: Box<[Value]>) -> Self {
        Self(super::thin_slice::ThinBoxSlice::from_exact_iter(
            values.iter().copied().map(EnvironmentSlot::owned),
        ))
    }
}

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

/// A function: its ordinary object, code and closure environment. The heap keeps it in the
/// function record family (`Heap::function`), where a dead closure costs its occupancy bit.
///
/// The creation realm is not a field: it is the VM's main realm unless the object's rare
/// extras carry an override (`realm_override`), so a closure of the main realm costs no word.
#[derive(Clone, Debug)]
#[repr(C)]
pub(crate) struct FunctionRecord {
    pub(crate) object: Object,
    /// Beside the object, whose 12 bytes it fills to the words after: after them, its 12
    /// bytes and two of padding made the record 48 bytes (jsdom-d3: 114K records at its peak).
    pub(crate) kind: FunctionKind,
    pub(crate) env: Value,
}

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

/// A legacy heap slot's cell. Kinds with a typed record family (`for_each_record_space!` in
/// `heap.rs`) are allocated in that family. A variant such as `Promise` or `AwaitState`
/// remains here only as the fallback for a full record space, and a variant with no family
/// (for example `Proxy`) lives here by design.
///
/// The variant for ordinary objects is gone. Every ordinary object lives only in the object record
/// space: `Heap::alloc_object` (and the `alloc_object_*` family) insert it through
/// `Heap::insert_object_record`, and a full space panics ("object records exhausted"), as functions
/// and iterators do. Snapshot restore reads object records directly (`Heap::read_snapshot`); the
/// snapshot cell tag for objects is gone too, and its number is not reused.
///
/// Brands are not kinds. Arguments, module namespace, raw JSON, Error and boxed primitives are
/// `ObjectBrand` values (or the `Object` payload) on an ordinary object record, so they need no
/// cell of their own.
///
/// Still needed, and what blocks each:
///
/// - Exotic kinds that carry an `Object` next to their own payload keep a legacy cell because the
///   payload does not fit an object record: `Proxy` (target and handler), `TypedArray`,
///   `DataView`, `ArrayBuffer` (bytes), `Date`, `RegExp` (shared meta), `ShadowRealm`, `WeakMap`,
///   `WeakSet`, `WeakRef`, `FinalizationRegistry` (entries), `Temporal*` (fields), `Wasm*`
///   (handles and headers), `Error(String)`. Each needs its own record family before it can move.
#[derive(Clone, Debug)]
#[rustfmt::skip]
pub(crate) enum Cell {
    ArrayBuffer {
        object: Box<Object>,
        bytes: super::BufferBytes,
        shared: bool,
        detached: bool,
        max_byte_length: usize,
        resizable: bool,
        immutable: bool,
    },
    TypedArray {
        object: Box<BufferViewObject>,
        buffer: Value,
        length: ViewLength,
    },
    DataView {
        object: Box<BufferViewObject>,
        buffer: Value,
        length: ViewLength,
    },
    ShadowRealm {
        object: Box<Object>,
        caller_global: Value,
        realm_global: Value,
    },
    WeakMap {
        object: Box<Object>,
        entries: Box<WeakMapEntries>,
    },
    WeakSet {
        object: Box<Object>,
        entries: Box<WeakSetEntries>,
    },
    WeakRef {
        object: Box<Object>,
        target: Option<WeakHandle>,
    },
    FinalizationRegistry {
        object: Box<Object>,
        callback: Value,
        entries: Box<FinalizationEntries>,
    },
    ArrayFromAsyncState(Box<ArrayFromAsyncState>),
    AwaitState(Box<AwaitState>),
    Proxy {
        object: Box<Object>,
        kind: ProxyKind,
        target: Value,
        handler: Value,
    },
    BindingReference {
        environment: Value,
        slot: u16,
        kind: crate::bytecode::LexicalBindingKind,
    },
    // Immutable raw 64-bit Wasm scalars cannot fit the tagged Value payload.
    WasmBits64(u64),
    WasmV128([u8; crate::wasm::V128_BYTES]),
    /// Opaque external payload in the internal anyref hierarchy, outside eqref.
    WasmExtern(Value),
    /// The getter and setter of an accessor property whose shape keeps them in its slot
    /// (`ShapeAttributes::SLOT_ACCESSOR_PAIR`), so objects that each define their own pair, as
    /// an object literal's `get x() {}` and `set x(v) {}` do, share one shape. Only that slot
    /// holds one; no guest code sees it as a value.
    AccessorPair { getter: Value, setter: Value },
    /// One exception identity owns its original tag and traced payload.
    WasmException {
        tag: Value,
        // Boxed so this payload keeps `Cell` narrow; the heap stores cells inline.
        #[allow(clippy::box_collection)]
        payload: Box<Vec<Value>>,
    },
    /// A tag retains its original declaration; identity is independent of type equality.
    WasmTag { object: Box<Object>, declarations: crate::WasmTypes, ty: u32 },
    /// GC object identity owns its original declaration and traced field values.
    WasmGc { declarations: crate::WasmTypes, ty: u32, fields: WasmGcFields },
    /// The native callable's immutable host operation and structural signature.
    WasmHostFunction {
        target: WasmHostTarget,
        signature: Rc<crate::wasm::WasmHostSignature>,
    },
    /// Immutable references available until an element segment drops.
    WasmElements(Vec<Value>),
    /// A `WebAssembly.Module` or `WebAssembly.Instance`.
    WasmObject { object: Box<Object>, slot: WasmObjectSlot },
    /// A global, memory or table is also its `WebAssembly.Global`, `.Memory` or `.Table`
    /// object, so every export of it is the same JavaScript object. `bits` is its one native
    /// slot, which compiled code reads and writes in place: the raw payload of an i32, i64, f32
    /// or f64 (a 32-bit one zero-extended), else the bits of the reference or vector handle,
    /// the only kind the collector follows (`Storage`).
    WasmGlobal { header: Box<WasmGlobalHeader>, bits: u64, ty: crate::WasmType, mutable: bool },
    /// The persistent context of a Wasm instance, which its environment holds in a slot of its
    /// own: native code reads the instance's words in place and rows name the instance by it.
    WasmInstance(crate::vm::InstanceRecord),
    /// One memory identity owns its bytes and original optional maximum. `buffer` is the
    /// ArrayBuffer currently viewing the bytes, or undefined until one is requested; growth
    /// detaches it.
    WasmMemory { header: Box<WasmMemoryHeader>, bytes: std::rc::Rc<crate::wasm::memory::MemoryStorage>, buffer: Value },
    /// Typed references owned by a Wasm instance, traced like other heap edges.
    WasmTable {
        object: Box<Object>,
        table64: bool,
        elements: Box<WasmTableElements>,
        element_type: wasmparser::RefType,
        declarations: crate::WasmTypes,
    },
    /// Digits inline up to `bigint::INLINE_DIGITS`, within the slot (`cells_stay_compact`).
    BigInt(crate::bigint::JsBigInt),
    Date { milliseconds: f64, object: Box<Object> },
    /// `meta` is shared: every evaluation of one literal site takes the site's record
    /// (`ProgramStore::regexp_literal_meta`), and `compile` replaces the receiver's copy.
    RegExp {
        object: Object,
        meta: Rc<RegExpMeta>,
    },
    Error(String),
    /// A promise: an ordinary object with its [[PromiseState]] and reactions.
    Promise(Box<super::PromiseCell>),
    PromiseResolvingState {
        promise: Value,
        already_resolved: bool,
    },
    TemporalDuration {
        object: Box<Object>,
        fields: Box<[f64; 10]>,
    },
    TemporalPlainDate {
        object: Box<Object>,
        year: i32,
        month: u32,
        day: u32,
        // Boxed so this payload keeps `Cell` narrow; the heap stores cells inline.
        #[allow(clippy::box_collection)]
        calendar: Box<String>,
    },
    TemporalPlainDateTime {
        object: Box<Object>,
        date_time: Box<IsoDateTimeFields>,
        // Boxed so this payload keeps `Cell` narrow; the heap stores cells inline.
        #[allow(clippy::box_collection)]
        calendar: Box<String>,
    },
    TemporalPlainMonthDay {
        object: Box<Object>,
        month: u32,
        day: u32,
        // Boxed so this payload keeps `Cell` narrow; the heap stores cells inline.
        #[allow(clippy::box_collection)]
        calendar: Box<String>,
        reference_iso_year: i32,
    },
    TemporalPlainYearMonth {
        object: Box<Object>,
        year: i32,
        month: u32,
        // Boxed so this payload keeps `Cell` narrow; the heap stores cells inline.
        #[allow(clippy::box_collection)]
        calendar: Box<String>,
        reference_iso_day: u32,
    },
    TemporalZonedDateTime {
        object: Box<Object>,
        instant: Box<ZonedInstant>,
        // Boxed so this payload keeps `Cell` narrow; the heap stores cells inline.
        #[allow(clippy::box_collection)]
        calendar: Box<String>,
    },
    TemporalInstant {
        object: Box<Object>,
        epoch_nanoseconds: Box<i128>,
    },
}

impl Cell {
    pub(crate) fn bigint(value: num_bigint::BigInt) -> Self {
        Self::BigInt(crate::bigint::JsBigInt::from(&value))
    }

    /// Whether a collection prunes this cell's entries by the liveness of their targets
    /// (`Heap::prune_weak_cell`).
    pub(crate) fn has_weak_entries(&self) -> bool {
        matches!(
            self,
            Self::WeakMap { .. }
                | Self::WeakSet { .. }
                | Self::WeakRef { .. }
                | Self::FinalizationRegistry { .. }
        )
    }

    pub(crate) fn profile_variant_name(&self) -> &'static str {
        match self {
            Self::WasmObject { .. } => "wasm_object",
            Self::ArrayBuffer { .. } => "array_buffer",
            Self::TypedArray { .. } => "typed_array",
            Self::DataView { .. } => "data_view",
            Self::ShadowRealm { .. } => "shadow_realm",
            Self::WeakMap { .. } => "weak_map",
            Self::WeakSet { .. } => "weak_set",
            Self::WeakRef { .. } => "weak_ref",
            Self::FinalizationRegistry { .. } => "finalization_registry",
            Self::ArrayFromAsyncState(_) => "array_from_async_state",
            Self::AwaitState(_) => "await_state",
            Self::Proxy { .. } => "proxy",
            Self::BindingReference { .. } => "binding_reference",
            Self::WasmBits64(_) => "wasm_bits64",
            Self::WasmInstance(_) => "wasm_instance",
            Self::WasmV128(_) => "wasm_v128",
            Self::WasmExtern(_) => "wasm_extern",
            Self::AccessorPair { .. } => "accessor_pair",
            Self::WasmException { .. } => "wasm_exception",
            Self::WasmTag { .. } => "wasm_tag",
            Self::WasmGc { .. } => "wasm_gc",
            Self::WasmHostFunction { .. } => "wasm_host_function",
            Self::WasmElements(_) => "wasm_elements",
            Self::WasmGlobal { .. } => "wasm_global",
            Self::WasmMemory { .. } => "wasm_memory",
            Self::WasmTable { .. } => "wasm_table",
            Self::BigInt(_) => "bigint",
            Self::Date { .. } => "date",
            Self::RegExp { .. } => "regexp",
            Self::Error(_) => "error",
            Self::PromiseResolvingState { .. } => "promise_resolving_state",
            Self::Promise(_) => "promise",
            Self::TemporalDuration { .. } | Self::TemporalPlainDate { .. } | Self::TemporalPlainDateTime { .. }
            | Self::TemporalPlainMonthDay { .. } | Self::TemporalPlainYearMonth { .. }
            | Self::TemporalZonedDateTime { .. } | Self::TemporalInstant { .. } => self.temporal_variant_name(),
        }
    }

    /// `profile_variant_name` of a Temporal cell.
    fn temporal_variant_name(&self) -> &'static str {
        match self {
            Self::TemporalDuration { .. } => "temporal_duration",
            Self::TemporalPlainDate { .. } => "temporal_plain_date",
            Self::TemporalPlainDateTime { .. } => "temporal_plain_date_time",
            Self::TemporalPlainMonthDay { .. } => "temporal_plain_month_day",
            Self::TemporalPlainYearMonth { .. } => "temporal_plain_year_month",
            Self::TemporalZonedDateTime { .. } => "temporal_zoned_date_time",
            _ => "temporal_instant",
        }
    }
}

impl Cell {
    /// Each program whose code this cell can still run or resolve names in:
    /// a user function's code, an environment's scope, a suspended generator.
    pub(crate) fn program_references(&self, mut visit: impl FnMut(ProgramId)) {
        match self {
            Self::AwaitState(state) => state.program_references(&mut visit),
            _ => {}
        }
    }
}

/// The union of two ascending index sequences, ascending and without repeats.
fn merge_ascending(
    left: impl Iterator<Item = usize>,
    right: impl Iterator<Item = usize>,
) -> smallvec::SmallVec<[usize; 16]> {
    // Inline for the few slots a per-iteration copy lists: no allocation per copy.
    let (mut left, mut right) = (left.peekable(), right.peekable());
    let mut merged = smallvec::SmallVec::new();
    loop {
        let next = match (left.peek().copied(), right.peek().copied()) {
            (Some(l), Some(r)) if l <= r => {
                left.next();
                l
            }
            (_, Some(r)) => {
                right.next();
                r
            }
            (Some(l), None) => {
                left.next();
                l
            }
            (None, None) => return merged,
        };
        if merged.last() != Some(&next) {
            merged.push(next);
        }
    }
}

#[cfg(test)]
mod array_record_tests {
    use super::*;
    use super::super::ArrayElements;

    fn values(array: &ArrayObject) -> Vec<Value> {
        array.elements().as_slice().to_vec()
    }

    /// The backing header the staged reader finds for `array` (`staged::elements::array_header`).
    fn staged_header(array: &ArrayObject) -> u64 {
        let mut stage = crate::staged::Concrete::new(std::ptr::null());
        crate::staged::elements::array_header(&mut stage, array as *const ArrayObject as u64)
    }

    #[test]
    fn an_array_without_extras_keeps_its_backing_in_its_payload_word() {
        let elements = ArrayElements::from_slice(&[Value::integer(1), Value::integer(2)]);
        let array = ArrayObject::new(Object::new(Value::NULL), elements.clone());
        assert!(!array.has_extras());
        assert_eq!({ array.object.payload }, elements.as_ptr() as u64);
        assert_eq!(staged_header(&array), elements.as_ptr() as u64);
        assert_eq!(array.properties().into_parts().0 & VALUE_VEC_START_BITS, EMPTY_POSITION);
        assert_eq!(array.plain_storage_start(), None);
        assert_eq!(elements.owner_count(), 2);
        let copy = array.clone();
        assert_eq!(elements.owner_count(), 3);
        assert_eq!(values(&copy), values(&array));
        // The object alone owns no backing.
        let object = Object::clone(&array);
        assert_eq!({ object.payload }, u64::from(EMPTY_POSITION));
        drop(object);
        assert_eq!(elements.owner_count(), 3);
        drop(copy);
        drop(array);
        assert_eq!(elements.owner_count(), 1);
        // The empty backing and a fresh array own nothing.
        let empty = ArrayObject::fresh(Value::NULL, ArrayElements::new());
        assert!(!empty.needs_drop());
        assert_eq!(empty.elements().len(), 0);
    }

    #[test]
    fn extras_take_an_arrays_backing_with_them() {
        let elements = ArrayElements::from_slice(&[Value::integer(3)]);
        let mut array = ArrayObject::fresh(Value::NULL, elements.clone());
        array.watch_as(Default::default());
        assert!(array.has_extras() && array.needs_drop());
        assert_eq!(values(&array), [Value::integer(3)]);
        assert_eq!(staged_header(&array), elements.as_ptr() as u64);
        assert_eq!(elements.owner_count(), 2);
        let copy = array.clone();
        assert!(copy.has_extras());
        assert_eq!(elements.owner_count(), 3);
        array.elements_mut().make_mut().push(Value::integer(4));
        assert_eq!(values(&array), [Value::integer(3), Value::integer(4)]);
        assert_eq!(values(&copy), [Value::integer(3)]);
        assert_eq!(staged_header(&array), array.elements().as_ptr() as u64);
        assert_eq!(elements.owner_count(), 2);
        // A new array of an array's object with extras takes the new backing in them.
        let rebuilt = ArrayObject::new(Object::clone(&copy), ArrayElements::from_slice(&[Value::integer(5)]));
        assert!(rebuilt.has_extras());
        assert_eq!(values(&rebuilt), [Value::integer(5)]);
        assert_eq!(elements.owner_count(), 2);
        drop((array, copy, rebuilt));
        assert_eq!(elements.owner_count(), 1);
    }

    #[test]
    fn array_records_round_trip_through_a_snapshot_with_and_without_extras() {
        for extras in [false, true] {
            let mut array = ArrayObject::fresh(Value::NULL, ArrayElements::from_slice(&[Value::integer(6), Value::NULL]));
            if extras {
                array.mark_indexed_descriptors();
            }
            let mut writer = crate::snapshot::Writer::new();
            array.write_snapshot(&mut writer).unwrap();
            let bytes = writer.finish();
            let mut reader = crate::snapshot::Reader::new(&bytes, None).unwrap();
            let restored = ArrayObject::read_snapshot(&mut reader).unwrap();
            assert_eq!(restored.has_extras(), extras);
            assert_eq!(restored.has_indexed_descriptors(), extras);
            assert_eq!(values(&restored), [Value::integer(6), Value::NULL]);
            assert_eq!(restored.elements().owner_count(), 1);
            assert_eq!(restored.properties().into_parts().0 & VALUE_VEC_START_BITS, EMPTY_POSITION);
        }
    }
}
