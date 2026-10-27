use super::records::RECORD_SPACE_CAPACITY;
use super::root::WeakHandle;
use crate::bytecode::Atom;
use crate::value::{HeapReference, Value};
use crate::value_vec::{
    INLINE_PROPERTY_COUNT, VALUE_VEC_INTEGRITY_BITS, VALUE_VEC_START_BITS, ValueArena, ValueVec,
};
use crate::vm::program_store::ProgramId;
use crate::vm::wtf16::JsString;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

mod snapshot;

/// Changes whenever the layout an object used as a prototype offers its inheritors can have
/// changed: its shape, prototype or integrity changes, and at every collection, after which a
/// dead prototype's record may be reused. A cache that proved a prototype chain under one value
/// proves it again while the value holds (`FieldAddCache::proved_epoch`). One counter for the
/// process: a change another heap made only sends caches back to the walk that re-proves them.
static PROTOTYPE_LAYOUT_EPOCH: AtomicU64 = AtomicU64::new(0);

/// The current `PROTOTYPE_LAYOUT_EPOCH`.
#[inline(always)]
pub(crate) fn prototype_layout_epoch() -> u64 {
    PROTOTYPE_LAYOUT_EPOCH.load(Ordering::Relaxed)
}

/// Retires every proof made under the current `PROTOTYPE_LAYOUT_EPOCH`.
pub(crate) fn advance_prototype_layout_epoch() {
    PROTOTYPE_LAYOUT_EPOCH.fetch_add(1, Ordering::Relaxed);
}

#[derive(Clone, Debug, Default)]
pub(crate) struct WeakMapEntries {
    ordered: Vec<(Value, Value)>,
    indices: FxHashMap<Value, usize>,
}

#[cfg(test)]
mod cell_layout_tests {
    use super::*;

    /// Every heap slot stores an `Option<Cell>`. The largest payload is an array: its object
    /// and its element backing.
    const CELL_TAG_WORD_BYTES: usize = std::mem::size_of::<Value>();
    const MAX_CELL_BYTES: usize = std::mem::size_of::<ArrayObject>() + CELL_TAG_WORD_BYTES;

    #[test]
    fn cells_stay_compact() {
        assert_eq!(std::mem::size_of::<Option<Cell>>(), MAX_CELL_BYTES);
    }

    #[test]
    fn native_array_layout_is_derived_from_the_current_storage_types() {
        assert_eq!(NATIVE_ARRAY_RECORD_SIZE, std::mem::size_of::<ArrayObject>());
        assert_eq!(NATIVE_ARRAY_RECORD_ALIGN, std::mem::align_of::<ArrayObject>());
        assert_eq!(
            NATIVE_ARRAY_ELEMENTS_OFFSET,
            std::mem::offset_of!(ArrayObject, elements)
        );
        let layout = super::super::NATIVE_ARRAY_LAYOUT;
        assert_eq!(layout.record_size, std::mem::size_of::<ArrayObject>());
        assert_eq!(layout.record_align, std::mem::align_of::<ArrayObject>());
        assert_eq!(layout.elements_offset, NATIVE_ARRAY_ELEMENTS_OFFSET);
    }

    #[test]
    fn native_object_layout_is_derived_from_the_current_storage_types() {
        assert_eq!(NATIVE_OBJECT_RECORD_SIZE, std::mem::size_of::<Object>());
        assert_eq!(NATIVE_OBJECT_RECORD_ALIGN, std::mem::align_of::<Object>());
        assert_eq!(
            NATIVE_OBJECT_META_OFFSET,
            std::mem::offset_of!(Object, meta)
        );
        assert_eq!(
            NATIVE_OBJECT_AUXILIARY_OFFSET,
            std::mem::offset_of!(Object, auxiliary)
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

impl WeakMapEntries {
    pub(crate) fn get(&self, key: Value) -> Option<Value> {
        self.indices
            .get(&key)
            .and_then(|index| self.ordered.get(*index))
            .map(|(_, value)| *value)
    }

    pub(crate) fn insert(&mut self, key: Value, value: Value) {
        if let Some(index) = self.indices.get(&key).copied() {
            self.ordered[index].1 = value;
        } else {
            self.indices.insert(key, self.ordered.len());
            self.ordered.push((key, value));
        }
    }

    pub(crate) fn remove(&mut self, key: Value) -> bool {
        let Some(index) = self.indices.remove(&key) else {
            return false;
        };
        self.ordered.swap_remove(index);
        if let Some((moved_key, _)) = self.ordered.get(index) {
            self.indices.insert(*moved_key, index);
        }
        true
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &(Value, Value)> {
        self.ordered.iter()
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&(Value, Value)) -> bool) {
        self.ordered.retain(|entry| keep(entry));
        self.indices.clear();
        self.indices.extend(
            self.ordered
                .iter()
                .enumerate()
                .map(|(index, (key, _))| (*key, index)),
        );
    }

    pub(crate) fn allocated_bytes(&self) -> usize {
        self.ordered.capacity() * std::mem::size_of::<(Value, Value)>()
            + self.indices.capacity() * std::mem::size_of::<(Value, usize)>()
    }
}
#[rustfmt::skip]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
pub(crate) enum Native {
    Print, HostDone, CreateRealm, IsHTMLDDA, EvalScript, CollectGarbage, RealmTypeError, Eval, Function, FunctionPrototype, FunctionPrototypeHasInstance, FunctionCaller, DynamicImport, AbstractModuleSource, AbstractModuleSourceToStringTag, ShadowRealm, ShadowRealmEvaluate, ShadowRealmImportValue, ShadowRealmImportValueFulfilled, ShadowRealmWrappedFunction,
    Object,
    ObjectKeys, ForInKeys, ForInKeyIsEnumerable, ObjectValues, ObjectEntries, ObjectGetOwnPropertyNames, ObjectGetOwnPropertySymbols, ObjectGetOwnPropertyDescriptor, ObjectGetOwnPropertyDescriptors, ObjectFromEntries, ObjectIs,
    ObjectCreate, ObjectAssign, ObjectDefineProperty, ObjectDefineProperties, ObjectGetPrototypeOf, ObjectPreventExtensions, ObjectIsExtensible, ObjectSeal, ObjectIsSealed, ObjectFreeze, ObjectIsFrozen, ObjectGroupBy,
    ObjectSetPrototypeOf, ObjectHasOwn, ObjectPrototypeHasOwnProperty, ObjectPrototypePropertyIsEnumerable, ObjectPrototypeIsPrototypeOf, ObjectPrototypeLookupGetter, ObjectPrototypeLookupSetter, ObjectPrototypeDefineGetter, ObjectPrototypeDefineSetter, ObjectPrototypeProtoGetter, ObjectPrototypeProtoSetter, ObjectPrototypeToLocaleString, ObjectPrototypeToString, ObjectPrototypeValueOf,
    ReflectGet, ReflectHas, ReflectApply, ReflectGetOwnPropertyDescriptor, ReflectDefineProperty, ReflectDeleteProperty, ReflectPreventExtensions, ReflectIsExtensible,
    ReflectSet,
    SuperSet,
    ObjectLiteralPrototype,
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
    CallSiteIsConstructor, CallSiteIsNative, CallSiteToString,
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
    WasmGlobal, WasmGlobalValueGetter, WasmGlobalValueSetter, WasmGlobalValueOf,
    WasmCompileError, WasmLinkError, WasmRuntimeError,
    /// A Wasm function as JavaScript sees it; its environment is the Wasm closure.
    WasmExportedFunction,
    HostFunction,
    CallSiteConstructor,
    CallSiteGetFunction,
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
    Instance { exports: Value },
}

impl std::fmt::Debug for WasmObjectSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Module(_) => f.write_str("Module"),
            Self::Instance { exports } => f.debug_struct("Instance").field("exports", exports).finish(),
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TypedArrayKind {
    Uint8,
    Uint8Clamped,
    Uint16,
    Uint32,
    Int8,
    Int16,
    Int32,
    BigInt64,
    BigUint64,
    Float16,
    Float32,
    Float64,
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

/// One `await`'s suspended activation. The cell is both handlers of the await's promise
/// reaction, so the continuation is traced while the reaction can still run, and the run
/// takes it. The continuation is boxed apart so taking it frees its storage at once: the
/// spent state waits for a sweep.
#[derive(Clone, Debug)]
pub(crate) struct AwaitState {
    pub(crate) continuation: Option<Box<crate::vm::activation::Continuation>>,
    /// The realm global that awaited, which the continuation resumes in.
    pub(crate) realm: Value,
    /// The async function's promise, or the async generator request's promise.
    pub(crate) promise: Value,
    pub(crate) generator: Option<Value>,
    /// The await of an async generator's `yield`, which settles the request with the value.
    pub(crate) yielded: bool,
}
impl AwaitState {
    /// The program its suspended continuation resumes in, which stays alive with it.
    pub(crate) fn program_references(&self, mut visit: impl FnMut(ProgramId)) {
        if let Some(continuation) = &self.continuation {
            visit(continuation.program());
        }
    }

    /// Its edges: the realm and promise it resumes, the generator, the suspended frame.
    pub(crate) fn extend_roots(&self, out: &mut Vec<Value>) {
        out.extend([self.realm, self.promise]);
        out.extend(self.generator);
        if let Some(continuation) = &self.continuation {
            continuation.extend_roots(out);
        }
    }
}
impl TypedArrayKind {
    pub(crate) const fn is_bigint(self) -> bool {
        matches!(self, Self::BigInt64 | Self::BigUint64)
    }

    pub(crate) const fn width(self) -> usize {
        match self {
            Self::Uint8 => 1,
            Self::Uint8Clamped => 1,
            Self::Uint16 => 2,
            Self::Uint32 => 4,
            Self::Int8 => 1,
            Self::Int16 => 2,
            Self::Int32 => 4,
            Self::BigInt64 => 8,
            Self::BigUint64 => 8,
            Self::Float16 => 2,
            Self::Float32 => 4,
            Self::Float64 => 8,
        }
    }
}
impl Native {
    /// The `get [Symbol.species]` accessors, each of which returns its receiver.
    pub(crate) fn is_intrinsic_species_getter(self) -> bool {
        matches!(
            self,
            Self::ArraySpecies
                | Self::ArrayBufferSpecies
                | Self::PromiseSpeciesGetter
                | Self::RegExpSpecies
                | Self::SetSpeciesGetter
        )
    }

    pub(crate) fn is_function_native(self) -> bool {
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
    /// The methods that look up a `Symbol.split`-like method on an argument (`split`,
    /// `replace`, `match` and their kin) are not among them.
    pub(crate) fn is_pure_string_method(self) -> bool {
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
        )
    }

    /// The functions of the `Math` object (`MATH_FUNCTIONS`, which its test keeps equal).
    pub(crate) fn is_math_function(self) -> bool {
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
                    | Self::StringSplit
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
    /// query value with `decodeURIComponent` (one per request).
    pub(crate) fn runs_beside_activations(self) -> bool {
        matches!(
            self,
            Self::ObjectLiteralPrototype
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
                | Self::ReflectGetPrototypeOf
                | Self::ForInKeys
                | Self::ArrayJoin
                | Self::ArraySplice
                | Self::StringSplit
                | Self::StringMatch
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
                | Self::FunctionBind
                | Self::Array
                | Self::PromiseResolve
                | Self::PromiseWithResolvers
        )
    }

    /// Natives a lane `Call` makes in place from the running frame (`lane_call_native`),
    /// as the general loop's native branch makes them once lane activations are frames:
    /// every native but those whose call the general loop takes as its own step. The
    /// call-forwarding natives push their target as a frame there (and run under a
    /// forwarding boundary), direct `eval` reads the calling instruction, and `with` entry
    /// and exit change the running frame's object environment.
    pub(crate) fn runs_from_lane(self) -> bool {
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

    /// Natives a lane `new` constructs in place from the running frame
    /// (`lane_construct_native`): the keyed collections, `Date`, `Promise`, `Object`,
    /// `RegExp`, the buffers, typed arrays and errors and the primitive wrappers, whose
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
    pub(crate) fn is_object_static(self) -> bool { matches!(self, Native::Object | Native::ObjectKeys | Native::ForInKeys | Native::ForInKeyIsEnumerable | Native::ObjectValues | Native::ObjectEntries | Native::ObjectGetOwnPropertyNames | Native::ObjectGetOwnPropertySymbols | Native::ObjectGetOwnPropertyDescriptor | Native::ObjectGetOwnPropertyDescriptors | Native::ObjectFromEntries | Native::ObjectIs | Native::ObjectCreate | Native::ObjectAssign | Native::ObjectDefineProperty | Native::ObjectDefineProperties | Native::ObjectGetPrototypeOf | Native::ObjectSetPrototypeOf | Native::ObjectHasOwn | Native::ObjectPreventExtensions | Native::ObjectIsExtensible | Native::ObjectSeal | Native::ObjectIsSealed | Native::ObjectFreeze | Native::ObjectIsFrozen | Native::ObjectGroupBy) }
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
    pub(crate) fn is_array_iterator(self) -> bool {
        matches!(
            self,
            Self::Array | Self::ArrayKeys | Self::ArrayValues | Self::ArrayEntries
        )
    }

    pub(crate) fn is_collection_iterator(self) -> bool {
        matches!(
            self,
            Self::MapKeys
                | Self::MapValues
                | Self::MapEntries
                | Self::SetValues
                | Self::SetEntries
        )
    }
}
#[derive(Clone, Debug)]
pub(crate) enum IteratorHelper {
    RegExpStringMatchAll {
        input: JsString,
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
    meta: u32,
    auxiliary: u32,
}

impl InlineObjectHeader {
    /// The inline shape this header stores.
    pub(crate) fn shape(self) -> u32 {
        ValueVec::from_parts(0, self.auxiliary).auxiliary()
    }

    /// No receiver's header: an unplanned entry's, which no construction reads.
    pub(crate) const EMPTY: Self = Self {
        meta: 0,
        auxiliary: 0,
    };

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
        let object = Object::with_property_storage(proto, storage, [Value::UNDEFINED; INLINE_PROPERTY_COUNT]);
        Self {
            meta: object.meta,
            auxiliary: object.auxiliary,
        }
    }
}

#[repr(C)]
pub(crate) struct Object {
    /// The prototype's heap index (`PROTO_INDEX_BITS`, with null and undefined sentinels) and
    /// the `OBJECT_*` flags: array, inline storage, and the integrity bits `ValueVec` keeps.
    meta: u32,
    /// `ValueVec::auxiliary`: the shape and the dictionary flag. Property names live once in
    /// the VM's shape table; objects keep only the data the shape selects.
    auxiliary: u32,
    /// One word: with `ObjectExtras`, `EXTRAS_TAG` and a pointer to them, and they then hold
    /// the inline values and the storage position. Extras are rare, so the common object pays
    /// no word for them. Without extras: the inline values as narrow values
    /// (`Value::to_narrow`), the first slot in the lower half; or, without inline storage,
    /// the storage position (`ValueVec`'s start). An inline value that has no narrow form
    /// moves the values into extras, so every value stays storable.
    payload: u64,
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
const PROTO_RECORD_END: u32 = PROTO_ITERATOR_RECORD_BASE + RECORD_SPACE_CAPACITY as u32;
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
        const _: () = assert!(
            PROTO_OBJECT_RECORD_BASE.is_power_of_two()
                && RECORD_SPACE_CAPACITY.is_power_of_two()
                && RECORD_SPACE_CAPACITY as u32 <= PROTO_OBJECT_RECORD_BASE
        );
        (PROTO_OBJECT_RECORD_BASE..PROTO_ARRAY_RECORD_BASE)
            .contains(&self.0)
            .then(|| self.0 & (RECORD_SPACE_CAPACITY as u32 - 1))
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
            index => {
                debug_assert!(index < PROTO_RECORD_END);
                Value::direct_iterator(index - PROTO_ITERATOR_RECORD_BASE)
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
/// Inline property values held by `ObjectExtras`; undefined until an object moves its own in.
#[derive(Clone, Copy, Debug)]
struct DisplacedInline([Value; INLINE_PROPERTY_COUNT]);

impl Default for DisplacedInline {
    fn default() -> Self {
        Self([Value::UNDEFINED; INLINE_PROPERTY_COUNT])
    }
}

const EXTRAS_TAG: u64 = 0x7fff_0000_0000_0000;
const EXTRAS_TAG_MASK: u64 = 0xffff_0000_0000_0000;

/// Layout facts for the generated read-only ordinary-object field path. Keep these derived
/// from the authoritative storage declarations; native code checks the object and extras
/// representation bits before using any offset below.
pub(super) const NATIVE_OBJECT_RECORD_SIZE: usize = std::mem::size_of::<Object>();
pub(super) const NATIVE_OBJECT_RECORD_ALIGN: usize = std::mem::align_of::<Object>();
pub(super) const NATIVE_OBJECT_META_OFFSET: usize = std::mem::offset_of!(Object, meta);
pub(super) const NATIVE_OBJECT_AUXILIARY_OFFSET: usize = std::mem::offset_of!(Object, auxiliary);
pub(super) const NATIVE_OBJECT_PAYLOAD_OFFSET: usize = std::mem::offset_of!(Object, payload);
pub(super) const NATIVE_OBJECT_EXTRAS_ARENA_START_OFFSET: usize =
    std::mem::offset_of!(ObjectExtras, arena_start);
pub(super) const NATIVE_OBJECT_INLINE_STORAGE_MASK: u32 = OBJECT_INLINE_STORAGE;
pub(super) const NATIVE_OBJECT_ARRAY_MASK: u32 = OBJECT_ARRAY;
pub(super) const NATIVE_OBJECT_EXTRAS_TAG: u64 = EXTRAS_TAG;
pub(super) const NATIVE_OBJECT_EXTRAS_TAG_MASK: u64 = EXTRAS_TAG_MASK;

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

impl Clone for Object {
    fn clone(&self) -> Self {
        // The word copies bit for bit; a copy with extras gets its own.
        let payload = match self.extras() {
            Some(extras) => Self::tag_extras(Box::new(extras.clone())),
            None => self.payload,
        };
        Self {
            meta: self.meta,
            auxiliary: self.auxiliary,
            payload,
        }
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
/// backing.
#[derive(Clone)]
#[repr(C)]
pub(crate) struct ArrayObject {
    object: Object,
    elements: super::ArrayElements,
}

/// Native read-only layout facts for `ArrayObject`, kept beside its field declarations.
pub(super) const NATIVE_ARRAY_RECORD_SIZE: usize = std::mem::size_of::<ArrayObject>();
pub(super) const NATIVE_ARRAY_RECORD_ALIGN: usize = std::mem::align_of::<ArrayObject>();
pub(super) const NATIVE_ARRAY_ELEMENTS_OFFSET: usize =
    std::mem::offset_of!(ArrayObject, elements);

const _: () = assert!(
    NATIVE_ARRAY_ELEMENTS_OFFSET + std::mem::size_of::<super::ArrayElements>()
        <= NATIVE_ARRAY_RECORD_SIZE
);

impl ArrayObject {
    pub(crate) fn new(mut object: Object, elements: impl Into<super::ArrayElements>) -> Self {
        let elements = elements.into();
        // Array objects keep named properties in the arena; a fresh object has none inline.
        let current = object.properties();
        object.meta |= OBJECT_ARRAY;
        if current.has_inline_property_storage() {
            debug_assert!(
                object
                    .inline_properties()
                    .is_some_and(|values| values.iter().all(|value| value.is_undefined()))
            );
            let mut properties = ValueVec::new();
            properties.set_auxiliary(current.auxiliary());
            properties.preserve_integrity_from(current);
            object.set_properties(properties);
        }
        Self { object, elements }
    }

    pub(crate) fn elements(&self) -> &super::ArrayElements {
        &self.elements
    }

    pub(crate) fn elements_mut(&mut self) -> &mut super::ArrayElements {
        &mut self.elements
    }
}

impl std::ops::Deref for ArrayObject {
    type Target = Object;
    fn deref(&self) -> &Object {
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
        if let CallSiteName::Callable(function) = self.name { visit(function); }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum StackData {
    Captured { records: Vec<CallSiteRecord>, source: Option<crate::bytecode::ProgramSource> },
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
    arguments_map: Option<Vec<u16>>,
    arguments_object: bool,
    raw_json: bool,
    error_data: bool,
    module_namespace: bool,
    external_address: Option<usize>,
    boxed_primitive: Option<Value>,
    structured_serialization: StructuredSerialization,
    structured_transfer_forbidden: bool,
    module_bindings: Vec<(Atom, ProgramId, u16)>,
    deferred_module: Option<crate::ModuleSource>,
    private_elements: Vec<PrivateElement>,
    stack_data: Option<StackData>,
    /// An index-keyed property descriptor has been recorded for this object; never cleared.
    indexed_descriptors: bool,
    /// A `length` descriptor has been recorded for this object, as a non-writable array
    /// length is; never cleared.
    length_descriptor: bool,
    /// Any property descriptor beside the shape has been recorded for this object
    /// (`Vm::insert_descriptor`); never cleared.
    side_descriptors: bool,
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
            + self.private_elements.capacity() * std::mem::size_of::<PrivateElement>()
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
            }) + match &record.name { CallSiteName::Static(name) => name.capacity(), _ => 0 }
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
/// A name's actual class home and its data share one lifetime-owned entry.
#[derive(Clone, Copy, Debug)]
struct PrivateElement {
    brand: PrivateBrand,
    // Brand-only entries name methods/accessors or reserve a class-home declaration.
    data: Option<Value>,
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
    #[inline(always)]
    pub(crate) fn new(proto: Value) -> Self {
        Self::with_property_storage(
            proto,
            ValueVec::inline_property_storage(0),
            [Value::UNDEFINED; INLINE_PROPERTY_COUNT],
        )
    }

    fn encode_proto(proto: Value) -> u32 {
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
        ProtoLink(self.meta & PROTO_INDEX_BITS)
    }

    #[inline(always)]
    pub(crate) fn proto(&self) -> Value {
        self.proto_link().prototype()
    }

    pub(crate) fn set_proto(&mut self, proto: Value) {
        self.note_prototype_layout_change();
        self.meta = (self.meta & !PROTO_INDEX_BITS) | Self::encode_proto(proto);
    }

    /// A change to what this object offers its inheritors, if it has any.
    #[inline(always)]
    fn note_prototype_layout_change(&self) {
        if self.was_used_as_prototype() {
            advance_prototype_layout_epoch();
        }
    }

    /// Whether this object is an array's, which keeps named properties in the arena.
    pub(crate) fn is_array(&self) -> bool {
        self.meta & OBJECT_ARRAY != 0
    }

    /// The property storage descriptor, assembled from the header and, without inline
    /// storage, the payload.
    #[inline(always)]
    pub(crate) fn properties(&self) -> ValueVec {
        let integrity = self.meta & VALUE_VEC_INTEGRITY_BITS;
        let position = if self.meta & OBJECT_INLINE_STORAGE != 0 {
            ValueVec::inline_property_storage(0).into_parts().0 & VALUE_VEC_START_BITS
        } else {
            self.storage_position()
        };
        ValueVec::from_parts(integrity | position, self.auxiliary)
    }

    /// Stores `properties`. A change from inline to other storage overwrites the payload
    /// word that held an inline value, so callers move inline values out first.
    pub(crate) fn set_properties(&mut self, properties: ValueVec) {
        let (start, auxiliary) = properties.into_parts();
        self.auxiliary = auxiliary;
        let flags = self.meta & !(VALUE_VEC_INTEGRITY_BITS | OBJECT_INLINE_STORAGE);
        let position = start & VALUE_VEC_START_BITS;
        if properties.has_inline_property_storage() {
            self.meta = flags | (start & VALUE_VEC_INTEGRITY_BITS) | OBJECT_INLINE_STORAGE;
        } else {
            self.meta = flags | (start & VALUE_VEC_INTEGRITY_BITS);
            self.set_storage_position(position);
        }
    }

    /// The storage start of an object whose values live in the arena and that has no
    /// extras: its payload word is then that start alone (`set_properties`), and its upper
    /// half is zero, which an extras word (`EXTRAS_TAG`) never is. `None` for inline storage
    /// or extras, whose start `storage_position` decodes.
    #[inline(always)]
    pub(crate) fn plain_storage_start(&self) -> Option<usize> {
        let plain = self.meta & OBJECT_INLINE_STORAGE == 0 && self.payload >> u32::BITS == 0;
        debug_assert!(!plain || self.extras_pointer().is_none());
        plain.then_some(self.payload as usize)
    }

    /// `plain_storage_start` of an object outside dictionary storage, whose start bits are
    /// then an arena start rather than a dictionary id (`ValueVec::is_dictionary`): the
    /// fast case of a read that also serves dictionaries (`Heap::property_get_shaped`).
    #[inline(always)]
    pub(crate) fn plain_shaped_start(&self) -> Option<usize> {
        if self.auxiliary & crate::value_vec::DICTIONARY_STORAGE != 0 {
            return None;
        }
        self.plain_storage_start()
    }

    #[inline(always)]
    fn storage_position(&self) -> u32 {
        match self.extras() {
            Some(extras) => extras.arena_start,
            None => self.payload as u32,
        }
    }

    fn set_storage_position(&mut self, position: u32) {
        match self.extras_mut_existing() {
            Some(extras) => extras.arena_start = position,
            None => self.payload = u64::from(position),
        }
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

    fn extras_pointer(&self) -> Option<*mut ObjectExtras> {
        let word = self.payload;
        (word & EXTRAS_TAG_MASK == EXTRAS_TAG)
            .then_some((word & !EXTRAS_TAG_MASK) as usize as *mut ObjectExtras)
    }

    fn extras(&self) -> Option<&ObjectExtras> {
        // SAFETY: a tagged payload owns a live `ObjectExtras` for the object's lifetime.
        self.extras_pointer().map(|extras| unsafe { &*extras })
    }

    fn extras_mut(&mut self) -> &mut ObjectExtras {
        let extras = match self.extras_pointer() {
            Some(extras) => extras,
            None => {
                // The payload word moves into the extras: the inline values, or the storage
                // position.
                let (inline, arena_start) = if self.meta & OBJECT_INLINE_STORAGE != 0 {
                    (Self::unpack_inline(self.payload), 0)
                } else {
                    ([Value::UNDEFINED; INLINE_PROPERTY_COUNT], self.payload as u32)
                };
                self.payload = Self::tag_extras(Box::new(ObjectExtras {
                    inline_properties: DisplacedInline(inline),
                    arena_start,
                    ..ObjectExtras::default()
                }));
                self.extras_pointer().expect("extras just installed")
            }
        };
        // SAFETY: owned by this object, and `&mut self` makes the borrow unique.
        unsafe { &mut *extras }
    }

    fn extras_mut_existing(&mut self) -> Option<&mut ObjectExtras> {
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
        let mut object = Self {
            meta: Self::encode_proto(proto),
            auxiliary: 0,
            payload,
        };
        // Inline storage leaves the payload word alone.
        object.set_properties(ValueVec::inline_property_storage(shape));
        Some(object)
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
        let mut object = Self {
            meta: Self::encode_proto(proto),
            auxiliary: 0,
            payload: 0,
        };
        object.set_properties(properties);
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
        Some(Self {
            meta: header.meta,
            auxiliary: header.auxiliary,
            payload: Self::pack_inline(inline)?,
        })
    }

    /// An object of a run header (`InlineObjectHeader::of_run`) whose values start at arena
    /// position `start`: what `with_property_storage` makes of that run, which has no extras.
    pub(crate) fn from_run_header(header: InlineObjectHeader, start: u32) -> Self {
        Self {
            meta: header.meta,
            auxiliary: header.auxiliary,
            payload: u64::from(start),
        }
    }

    #[inline(always)]
    pub(crate) fn inline_properties(&self) -> Option<[Value; INLINE_PROPERTY_COUNT]> {
        (self.meta & OBJECT_INLINE_STORAGE != 0).then(|| self.inline_values())
    }

    /// The inline property at `slot`, when the object stores its properties inline.
    #[inline(always)]
    pub(crate) fn inline_property(&self, slot: usize) -> Option<Value> {
        if self.meta & OBJECT_INLINE_STORAGE == 0 {
            return None;
        }
        debug_assert!(slot < INLINE_PROPERTY_COUNT);
        Some(match self.extras() {
            Some(extras) => extras.inline_properties.0[slot],
            None => Value::from_narrow((self.payload >> (slot as u32 * u32::BITS)) as u32),
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
        if self.meta & OBJECT_INLINE_STORAGE != 0 {
            return slot < INLINE_PROPERTY_COUNT && value.to_narrow().is_some();
        }
        ValueArena::appends_in_place(self.properties(), slot)
    }

    /// Appends `value` at `slot` and moves to `shape`, the add `admits_plain_append` admitted.
    /// Without extras the object was never a prototype, so the move retires no proof.
    #[inline(always)]
    pub(crate) fn append_plain(&mut self, properties: &mut ValueArena, slot: usize, value: Value, shape: u32) {
        debug_assert!(self.admits_plain_append(slot, value));
        match value.to_narrow().filter(|_| self.meta & OBJECT_INLINE_STORAGE != 0) {
            Some(narrow) => self.store_payload_narrow(slot, narrow),
            // SAFETY: `admits_plain_append` found the arena range's room for the value.
            None => unsafe { properties.push_in_place(self.properties(), slot, value) },
        }
        self.set_shape(shape);
    }

    /// Stores the narrow form `narrow` at inline `slot` of the payload word, which holds the
    /// inline values.
    #[inline(always)]
    fn store_payload_narrow(&mut self, slot: usize, narrow: u32) {
        let shift = slot as u32 * u32::BITS;
        self.payload = self.payload & !(u64::from(u32::MAX) << shift) | u64::from(narrow) << shift;
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
    ) -> ValueVec {
        let previous = self.properties();
        properties.preserve_integrity_from(previous);
        self.set_properties(properties);
        // Arena storage keeps its position in the payload word (or the extras) instead.
        if properties.has_inline_property_storage() {
            self.store_inline_values(inline_properties);
        }
        previous
    }

    /// Takes `source`'s property values, given `properties`, the arena's duplicate of
    /// `source.properties()`, and returns the storage this object held before.
    pub(crate) fn copy_property_storage_from(&mut self, source: &Self, properties: ValueVec) -> ValueVec {
        let previous = self.properties();
        self.set_properties(properties);
        if properties.has_inline_property_storage() {
            self.store_inline_values(source.inline_values());
        }
        previous
    }

    /// Presence of [[ErrorData]] is the unforgeable Error brand.
    pub(crate) fn error(proto: Value) -> Self {
        let mut object = Self::new(proto);
        object.extras_mut().error_data = true;
        object
    }

    /// How structured serialization treats this object.
    pub(crate) fn set_structured_serialization(&mut self, policy: StructuredSerialization) {
        self.extras_mut().structured_serialization = policy;
    }

    pub(crate) fn structured_serialization(&self) -> StructuredSerialization {
        self.extras().map_or(StructuredSerialization::Ordinary, |extras| extras.structured_serialization)
    }

    pub(crate) fn forbid_structured_transfer(&mut self) {
        self.extras_mut().structured_transfer_forbidden = true;
    }

    pub(crate) fn is_structured_transfer_forbidden(&self) -> bool {
        self.extras()
            .is_some_and(|extras| extras.structured_transfer_forbidden)
    }

    pub(crate) fn has_error_data(&self) -> bool {
        self.extras().is_some_and(|extras| extras.error_data)
    }

    /// Genuine primitive-wrapper data; guest properties cannot supply this slot.
    pub(crate) fn boxed_primitive(&self) -> Option<Value> {
        self.extras()?.boxed_primitive
    }

    pub(crate) fn set_boxed_primitive(&mut self, value: Value) {
        self.extras_mut().boxed_primitive = Some(value);
    }

    pub(crate) fn stack_data(&self) -> Option<&StackData> {
        self.extras()?.stack_data.as_ref()
    }

    pub(crate) fn set_stack_data(&mut self, data: StackData) {
        self.extras_mut().stack_data = Some(data);
    }

    /// Visit internal primitive data, private-name homes and captured stack roots.
    pub(crate) fn visit_extras_roots(&self, mut visit: impl FnMut(Value)) {
        let Some(extras) = self.extras() else {
            return;
        };
        if let Some(value) = extras.boxed_primitive { visit(value); }
        for element in &extras.private_elements {
            visit(element.brand.home);
            if let Some(value) = element.data { visit(value); }
        }
        match extras.stack_data.as_ref() {
            Some(StackData::Captured { records, .. }) => {
                for record in records {
                    record.visit_roots(&mut visit);
                }
            }
            Some(StackData::CallSite(record)) => {
                record.visit_roots(&mut visit);
            },
            Some(StackData::Formatted(value)) => visit(*value),
            None => {}
        }
    }

    pub(crate) fn was_used_as_prototype(&self) -> bool {
        self.extras().is_some_and(|extras| extras.prototype_lookup != PrototypeLookupState::Ordinary)
    }

    pub(crate) fn mark_as_prototype(&mut self) {
        if !self.was_used_as_prototype() { self.extras_mut().prototype_lookup = PrototypeLookupState::Setup; }
    }

    pub(crate) fn prototype_lookup_ready(&self) -> bool {
        self.extras().is_some_and(|extras| extras.prototype_lookup == PrototypeLookupState::Ready)
    }

    pub(crate) fn prepare_prototype_lookup(&mut self) {
        if self.was_used_as_prototype() { self.extras_mut().prototype_lookup = PrototypeLookupState::Ready; }
    }

    pub(crate) fn shape(&self) -> u32 {
        ValueVec::from_parts(0, self.auxiliary).auxiliary()
    }
    /// The shape with its dictionary bit (`ValueVec::exact_auxiliary`): a cache entry,
    /// which records only non-dictionary shapes, matches it exactly when it matches the
    /// shape of an object outside dictionary storage.
    #[inline(always)]
    pub(crate) fn exact_shape(&self) -> u32 {
        ValueVec::from_parts(0, self.auxiliary).exact_auxiliary()
    }
    #[inline(always)]
    pub(crate) fn set_shape(&mut self, shape: u32) {
        self.note_prototype_layout_change();
        let mut properties = ValueVec::from_parts(0, self.auxiliary);
        properties.set_auxiliary(shape);
        self.auxiliary = properties.into_parts().1;
    }
    pub(crate) fn is_extensible(&self) -> bool {
        self.properties().is_extensible()
    }
    /// Whether a cached add transition (`FieldAddCache`) applies to this object as to the
    /// ordinary object that recorded it: it is extensible, and no arguments object or module
    /// namespace, whose exotic own properties its shape does not describe.
    #[inline(always)]
    pub(crate) fn admits_cached_add(&self) -> bool {
        self.is_extensible()
            && self.extras().is_none_or(|extras| !extras.arguments_object && !extras.module_namespace)
    }
    pub(crate) fn set_extensible(&mut self, value: bool) {
        let mut properties = ValueVec::from_parts(self.meta & VALUE_VEC_INTEGRITY_BITS, 0);
        properties.set_extensible(value);
        self.meta = (self.meta & !VALUE_VEC_INTEGRITY_BITS) | properties.into_parts().0;
    }
    pub(crate) fn is_frozen(&self) -> bool {
        self.properties().is_frozen()
    }
    pub(crate) fn set_frozen(&mut self, value: bool) {
        self.note_prototype_layout_change();
        let mut properties = ValueVec::from_parts(self.meta & VALUE_VEC_INTEGRITY_BITS, 0);
        properties.set_frozen(value);
        self.meta = (self.meta & !VALUE_VEC_INTEGRITY_BITS) | properties.into_parts().0;
    }
    pub(crate) fn arguments_map(&self) -> Option<&[u16]> {
        self.extras()?.arguments_map.as_deref()
    }
    pub(crate) fn arguments_map_mut(&mut self) -> Option<&mut Vec<u16>> {
        self.extras_mut_existing()?.arguments_map.as_mut()
    }
    pub(crate) fn mapped_argument_slot(&self, index: usize) -> Option<usize> {
        self.arguments_map()?
            .get(index)
            .copied()
            .filter(|slot| *slot != u16::MAX)
            .map(usize::from)
    }
    pub(crate) fn set_arguments_map(&mut self, mapping: Vec<u16>) {
        self.extras_mut().arguments_map = Some(mapping);
    }
    pub(crate) fn set_arguments_object(&mut self) {
        self.extras_mut().arguments_object = true;
    }
    pub(crate) fn is_arguments_object(&self) -> bool {
        self.extras().is_some_and(|extras| extras.arguments_object)
    }
    pub(crate) fn has_indexed_descriptors(&self) -> bool {
        self.extras()
            .is_some_and(|extras| extras.indexed_descriptors)
    }
    pub(crate) fn mark_indexed_descriptors(&mut self) {
        self.note_prototype_layout_change();
        self.extras_mut().indexed_descriptors = true;
    }
    pub(crate) fn has_length_descriptor(&self) -> bool {
        self.extras().is_some_and(|extras| extras.length_descriptor)
    }
    pub(crate) fn mark_length_descriptor(&mut self) {
        self.extras_mut().length_descriptor = true;
    }
    /// Whether the VM's descriptor table may hold an entry for this object: lookups of an
    /// object without one need not probe the table.
    #[inline(always)]
    pub(crate) fn has_side_descriptors(&self) -> bool {
        self.extras().is_some_and(|extras| extras.side_descriptors)
    }
    /// A prototype's side descriptors can name keys its shape lacks, which retires the
    /// proofs (`prototype_layout_epoch`) that found them absent.
    pub(crate) fn mark_side_descriptors(&mut self) {
        self.note_prototype_layout_change();
        self.extras_mut().side_descriptors = true;
    }
    pub(crate) fn is_raw_json(&self) -> bool {
        self.extras().is_some_and(|extras| extras.raw_json)
    }
    pub(crate) fn set_raw_json(&mut self) {
        self.extras_mut().raw_json = true;
    }

    pub(crate) fn external_address(&self) -> Option<usize> {
        self.extras()?.external_address
    }
    pub(crate) fn set_external_address(&mut self, address: usize) {
        let extras = self.extras_mut();
        extras.external_address = Some(address);
        extras.structured_serialization = StructuredSerialization::Forbidden;
    }

    pub(crate) fn is_module_namespace(&self) -> bool {
        self.extras().is_some_and(|extras| extras.module_namespace)
    }
    pub(crate) fn set_module_namespace(&mut self) {
        self.extras_mut().module_namespace = true;
    }
    pub(crate) fn module_bindings(&self) -> &[(Atom, ProgramId, u16)] {
        self.extras().map_or(&[], |extras| &extras.module_bindings)
    }
    pub(crate) fn set_module_bindings(&mut self, bindings: Vec<(Atom, ProgramId, u16)>) {
        self.extras_mut().module_bindings = bindings;
    }
    pub(crate) fn deferred_module(&self) -> Option<&crate::ModuleSource> {
        self.extras()?.deferred_module.as_ref()
    }
    pub(crate) fn set_deferred_module(&mut self, module: Option<crate::ModuleSource>) {
        self.extras_mut().deferred_module = module;
    }
    pub(crate) fn private_names(&self) -> impl Iterator<Item = PrivateBrand> + '_ {
        self.extras().into_iter().flat_map(|extras| extras.private_elements.iter().map(|element| element.brand))
    }
    pub(crate) fn has_private_name(&self, brand: PrivateBrand) -> bool {
        self.private_names().any(|candidate| candidate == brand)
    }
    pub(crate) fn add_private_name(&mut self, brand: PrivateBrand) {
        if !self.has_private_name(brand) {
            self.extras_mut().private_elements.push(PrivateElement { brand, data: None });
        }
    }
    pub(crate) fn private_field(&self, brand: PrivateBrand) -> Option<Value> {
        self.extras()?.private_elements.iter().find(|element| element.brand == brand)?.data
    }
    pub(crate) fn set_private_field(&mut self, brand: PrivateBrand, value: Value) -> bool {
        let Some(element) = self.extras_mut_existing().and_then(|extras| extras.private_elements.iter_mut().find(|element| element.brand == brand && element.data.is_some())) else { return false; };
        element.data = Some(value);
        true
    }
    pub(crate) fn initialize_private_field(&mut self, brand: PrivateBrand, value: Value) -> bool {
        let elements = &mut self.extras_mut().private_elements;
        if let Some(element) = elements.iter_mut().find(|element| element.brand == brand) {
            if element.data.is_some() { return false; }
            element.data = Some(value);
        } else {
            elements.push(PrivateElement { brand, data: Some(value) });
        }
        true
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
const _: () = assert!(std::mem::size_of::<Object>() == 2 * std::mem::size_of::<u64>());
/// The payload word holds every inline slot as a narrow value.
const _: () = assert!(INLINE_PROPERTY_COUNT * u32::BITS as usize == u64::BITS as usize);

#[cfg(test)]
mod object_payload_tests {
    use super::Object;
    use crate::value::Value;
    use crate::value_vec::{INLINE_PROPERTY_COUNT, ValueVec};

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

        object.set_shape(19);
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

    fn scope(&self) -> Option<&EnvironmentScope> {
        // SAFETY: an untagged word is the pointer `boxed` leaked, owned by `self` until `Drop`.
        (self.0.get() & INLINE_SCOPE_TAG == 0)
            .then(|| unsafe { &*(self.0.get() as *const EnvironmentScope) })
    }

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

    #[cfg(feature = "profile-memory")]
    pub(crate) fn boxed_bytes(&self) -> usize {
        self.scope().map_or(0, |_| size_of::<EnvironmentScope>())
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
pub(crate) struct EnvironmentSlots(pub(super) super::thin_slice::ThinBoxSlice<EnvironmentSlot>);

/// Words an `EnvironmentSlots::lay_out_in_place` storage takes before its slots.
pub(crate) const IN_PLACE_SLOTS_WORDS: usize =
    super::thin_slice::ThinBoxSlice::<EnvironmentSlot>::IN_PLACE_WORDS;

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
    /// `dense` for writing.
    #[inline(always)]
    pub(crate) fn dense_mut(&mut self) -> Option<&mut Self> {
        (!self.0.marked()).then_some(self)
    }
    /// `dense_plain`'s slot to write: an ordinary value's place, which a store replaces.
    #[inline(always)]
    pub(crate) fn dense_plain_mut(&mut self, slot: usize) -> Option<&mut Value> {
        debug_assert!(!self.0.marked());
        let value = &mut self.0.get_mut(slot)?.0;
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
    fn compacted(dense: super::thin_slice::ThinBoxSlice<EnvironmentSlot>) -> Option<Self> {
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
    pub(crate) fn iteration_copy(
        &self,
        environment: Value,
        fresh: &[u16],
        reset: &[u16],
        owned_value: impl Fn(Value, usize) -> Option<Value>,
    ) -> Option<Self> {
        let len = self.len();
        let copy_slot = |index: usize| -> Option<EnvironmentSlot> {
            let slot = u16::try_from(index).ok()?;
            if reset.binary_search(&slot).is_ok() {
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
            // Every slot shares with its current owner, then the fresh and reset ones own.
            let mut copy = super::thin_slice::ThinBoxSlice::from_exact_iter(self.0.iter().map(
                |slot| match slot.state() {
                    EnvironmentSlotState::Owned(_) => EnvironmentSlot::shared(environment),
                    EnvironmentSlotState::Shared(owner) => EnvironmentSlot::shared(owner),
                },
            ));
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
            for &slot in reset {
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
            reset.iter().map(|&slot| usize::from(slot)),
        );
        indexes.truncate(indexes.partition_point(|&index| index < len));
        let shared = Value::shared_environment_slot(owner);
        let mut listed = Vec::with_capacity(indexes.len());
        for index in indexes {
            let slot = copy_slot(index)?;
            if slot.0 != shared {
                listed.push((index, slot));
            }
        }
        if SPARSE_ENTRIES_START + listed.len() * SPARSE_ENTRY_WORDS >= len {
            return super::thin_slice::ThinBoxSlice::try_from_fn(len, copy_slot).map(Self);
        }
        let mut words = Vec::with_capacity(SPARSE_ENTRIES_START + listed.len() * SPARSE_ENTRY_WORDS);
        words.push(EnvironmentSlot(Value::environment_index(u32::try_from(len).ok()?)));
        words.push(EnvironmentSlot(shared));
        for (index, slot) in listed {
            words.push(EnvironmentSlot(Value::environment_index(u32::try_from(index).ok()?)));
            words.push(slot);
        }
        let mut storage = super::thin_slice::ThinBoxSlice::from_exact_iter(words.into_iter());
        storage.mark();
        Some(Self(storage))
    }

    /// Slots that own `values`, in order.
    /// Dense storage of the `len` owned slots that follow its `IN_PLACE_SLOTS_WORDS` words
    /// at `words`, as `ThinBoxSlice::lay_out_in_place`: a view its caller never drops.
    ///
    /// # Safety
    /// As `ThinBoxSlice::lay_out_in_place`; each slot is an owned value.
    pub(crate) unsafe fn lay_out_in_place(words: *mut Value, len: usize) -> *const Self {
        // SAFETY: as the caller guarantees; a slot is a `Value`, a word.
        unsafe { super::thin_slice::ThinBoxSlice::<EnvironmentSlot>::lay_out_in_place(words.cast(), len).cast() }
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
#[derive(Debug, Clone)]
pub(crate) struct IteratorExt {
    /// Upper half of the iterator position; nonzero only for array-likes iterated past
    /// 2^32 elements, whose lengths reach 2^53 - 1.
    pub(crate) position_high: u32,
    pub(crate) next_method: Option<Value>,
    pub(crate) helper: Option<Box<IteratorHelper>>,
    pub(crate) helper_running: bool,
    pub(crate) helper_started: bool,
    pub(crate) generator: Option<Box<crate::vm::activation::GeneratorRecord>>,
}

impl IteratorExt {
    /// The full position whose low half is the cell's `index`.
    pub(crate) fn position(&self, low: u32) -> usize {
        ((u64::from(self.position_high) << u32::BITS) | u64::from(low)) as usize
    }

    /// Splits `position` between the cell's `index` and this box.
    pub(crate) fn store_position(&mut self, low: &mut u32, position: usize) {
        let position = position as u64;
        *low = position as u32;
        self.position_high = (position >> u32::BITS) as u32;
    }
}

/// An iterator object: built-in iterators, iterator helpers and generators. The heap keeps
/// it in the iterator record family (`Heap::iterator`); a heap whose record range is
/// unavailable boxes it in a `Cell::Iterator` slot.
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
            .and_then(|generator| generator.continuation.as_ref())
        {
            visit(continuation.program());
        }
    }
}

/// A function: its ordinary object, creation realm, code and closure environment. The
/// heap keeps it in the function record family (`Heap::function`), where a dead closure
/// costs its occupancy bit; a heap whose record range is unavailable boxes it in a
/// `Cell::Function` slot.
#[derive(Clone, Debug)]
pub(crate) struct FunctionRecord {
    pub(crate) object: Object,
    pub(crate) realm: Value,
    pub(crate) env: Value,
    pub(crate) kind: FunctionKind,
}

impl FunctionRecord {
    /// The program whose code this function runs, which keeps that program alive.
    pub(crate) fn program(&self) -> Option<ProgramId> {
        match self.kind {
            FunctionKind::User(program, _) => Some(program),
            FunctionKind::Native(_) => None,
        }
    }
}

/// A typed array's or DataView's ordinary object and byte offset into its buffer, boxed
/// together so the view cell keeps four words.
#[derive(Clone, Debug)]
pub(crate) struct BufferViewObject {
    pub(crate) object: Object,
    pub(crate) offset: usize,
}

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

/// A Wasm GC object's field values and optional descriptor, boxed together so the cell
/// keeps four words; field access reads the values through `Deref`.
#[derive(Clone, Debug, Default)]
pub(crate) struct WasmGcFields {
    pub(crate) values: Vec<Value>,
    pub(crate) descriptor: Option<Value>,
}

/// A Wasm table's references and optional maximum, boxed together so the cell keeps four
/// words; element access reads the references through `Deref`.
#[derive(Clone, Debug, Default)]
pub(crate) struct WasmTableElements {
    pub(crate) values: Vec<Value>,
    pub(crate) maximum: Option<u64>,
}

macro_rules! deref_to_values {
    ($($owner:ty),*) => {$(
        impl std::ops::Deref for $owner {
            type Target = Vec<Value>;
            fn deref(&self) -> &Vec<Value> {
                &self.values
            }
        }

        impl std::ops::DerefMut for $owner {
            fn deref_mut(&mut self) -> &mut Vec<Value> {
                &mut self.values
            }
        }
    )*};
}
deref_to_values!(WasmGcFields, WasmTableElements);

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
pub(crate) enum SymbolKind { Public, HostPrivate }
impl SymbolData {
    pub(crate) fn public(description: Option<String>) -> Self {
        Self { description: description.map(String::into_boxed_str), kind: SymbolKind::Public }
    }
    pub(crate) fn host_private(description: Option<String>) -> Self {
        Self { description: description.map(String::into_boxed_str), kind: SymbolKind::HostPrivate }
    }
}
impl std::ops::Deref for SymbolData {
    type Target = Option<Box<str>>;
    fn deref(&self) -> &Self::Target { &self.description }
}

#[derive(Clone, Debug)]
#[rustfmt::skip]
pub(crate) enum Cell {
    Object(Object),
    Array {
        object: ArrayObject,
    },
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
        kind: TypedArrayKind,
        object: Box<BufferViewObject>,
        buffer: Value,
        length: usize,
        length_tracking: bool,
    },
    DataView {
        object: Box<BufferViewObject>,
        buffer: Value,
        length: usize,
        length_tracking: bool,
    },
    Map {
        object: Box<Object>,
        entries: Box<super::CollectionEntries<(Value, Value)>>,
    },
    Set {
        object: Box<Object>,
        entries: Box<super::CollectionEntries<Value>>,
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
        entries: Box<Vec<Value>>,
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
    /// An iterator where no iterator record is available (`IteratorRecord`).
    Iterator(Box<IteratorRecord>),
    ArrayFromAsyncState(Box<ArrayFromAsyncState>),
    AwaitState(Box<AwaitState>),
    Proxy {
        object: Box<Object>,
        kind: ProxyKind,
        target: Value,
        handler: Value,
    },
    /// A function where no function record is available (`FunctionRecord`).
    Function(Box<FunctionRecord>),
    BindingReference {
        environment: Value,
        slot: u16,
        kind: crate::bytecode::LexicalBindingKind,
    },
    Environment {
        parent: Value,
        function: u32,
        slots: EnvironmentSlots,
        scope: EnvironmentScopeRef,
    },
    // Immutable raw 64-bit Wasm scalars cannot fit the tagged Value payload.
    WasmBits64(u64),
    WasmV128([u8; crate::wasm::V128_BYTES]),
    /// Opaque external payload in the internal anyref hierarchy, outside eqref.
    WasmExtern(Value),
    /// One exception identity owns its original tag and traced payload.
    WasmException { tag: Value, payload: Box<Vec<Value>> },
    /// A tag retains its original declaration; identity is independent of type equality.
    WasmTag { declarations: crate::WasmTypes, ty: u32 },
    /// GC object identity owns its original declaration and traced field values.
    WasmGc { declarations: Box<crate::WasmTypes>, ty: u32, fields: Box<WasmGcFields> },
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
    /// object, so every export of it is the same JavaScript object.
    WasmGlobal { header: Box<WasmGlobalHeader>, value: Value, ty: crate::WasmType, mutable: bool },
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
        declarations: Box<crate::WasmTypes>,
    },
    /// Boxed so a slot stays one tag word plus the object layout (`cells_stay_compact`).
    BigInt(Box<num_bigint::BigInt>),
    Symbol(SymbolData),
    Date { milliseconds: f64, object: Box<Object> },
    /// `meta` is shared: every evaluation of one literal site takes the site's record
    /// (`ProgramStore::regexp_literal_meta`), and `compile` replaces the receiver's copy.
    RegExp {
        object: Box<Object>,
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
        calendar: Box<String>,
    },
    TemporalPlainDateTime {
        object: Box<Object>,
        date_time: Box<IsoDateTimeFields>,
        calendar: Box<String>,
    },
    TemporalPlainMonthDay {
        object: Box<Object>,
        month: u32,
        day: u32,
        calendar: Box<String>,
        reference_iso_year: i32,
    },
    TemporalPlainYearMonth {
        object: Box<Object>,
        year: i32,
        month: u32,
        calendar: Box<String>,
        reference_iso_day: u32,
    },
    TemporalZonedDateTime {
        object: Box<Object>,
        instant: Box<ZonedInstant>,
        calendar: Box<String>,
    },
    TemporalInstant {
        object: Box<Object>,
        epoch_nanoseconds: Box<i128>,
    },
}

impl Cell {
    pub(crate) fn bigint(value: num_bigint::BigInt) -> Self {
        Self::BigInt(Box::new(value))
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
            Self::Object(_) => "object",
            Self::WasmObject { .. } => "wasm_object",
            Self::Array { .. } => "array",
            Self::ArrayBuffer { .. } => "array_buffer",
            Self::TypedArray { .. } => "typed_array",
            Self::DataView { .. } => "data_view",
            Self::Map { .. } => "map",
            Self::Set { .. } => "set",
            Self::ShadowRealm { .. } => "shadow_realm",
            Self::WeakMap { .. } => "weak_map",
            Self::WeakSet { .. } => "weak_set",
            Self::WeakRef { .. } => "weak_ref",
            Self::FinalizationRegistry { .. } => "finalization_registry",
            Self::Iterator(_) => "iterator",
            Self::ArrayFromAsyncState(_) => "array_from_async_state",
            Self::AwaitState(_) => "await_state",
            Self::Proxy { .. } => "proxy",
            Self::Function(_) => "function",
            Self::BindingReference { .. } => "binding_reference",
            Self::Environment { .. } => "environment",
            Self::WasmBits64(_) => "wasm_bits64",
            Self::WasmV128(_) => "wasm_v128",
            Self::WasmExtern(_) => "wasm_extern",
            Self::WasmException { .. } => "wasm_exception",
            Self::WasmTag { .. } => "wasm_tag",
            Self::WasmGc { .. } => "wasm_gc",
            Self::WasmHostFunction { .. } => "wasm_host_function",
            Self::WasmElements(_) => "wasm_elements",
            Self::WasmGlobal { .. } => "wasm_global",
            Self::WasmMemory { .. } => "wasm_memory",
            Self::WasmTable { .. } => "wasm_table",
            Self::BigInt(_) => "bigint",
            Self::Symbol(_) => "symbol",
            Self::Date { .. } => "date",
            Self::RegExp { .. } => "regexp",
            Self::Error(_) => "error",
            Self::PromiseResolvingState { .. } => "promise_resolving_state",
            Self::Promise(_) => "promise",
            Self::TemporalDuration { .. } => "temporal_duration",
            Self::TemporalPlainDate { .. } => "temporal_plain_date",
            Self::TemporalPlainDateTime { .. } => "temporal_plain_date_time",
            Self::TemporalPlainMonthDay { .. } => "temporal_plain_month_day",
            Self::TemporalPlainYearMonth { .. } => "temporal_plain_year_month",
            Self::TemporalZonedDateTime { .. } => "temporal_zoned_date_time",
            Self::TemporalInstant { .. } => "temporal_instant",
        }
    }
}

impl Cell {
    /// Each program whose code this cell can still run or resolve names in:
    /// a user function's code, an environment's scope, a suspended generator.
    pub(crate) fn program_references(&self, mut visit: impl FnMut(ProgramId)) {
        match self {
            Self::Function(function) => function.program().into_iter().for_each(&mut visit),
            Self::Environment { scope, .. } => {
                if let Some(program) = scope.program() {
                    visit(ProgramId::from_raw(program));
                }
            }
            Self::Iterator(iterator) => iterator.program_references(&mut visit),
            Self::AwaitState(state) => state.program_references(&mut visit),
            _ => {}
        }
    }
}

/// The union of two ascending index sequences, ascending and without repeats.
fn merge_ascending(
    left: impl Iterator<Item = usize>,
    right: impl Iterator<Item = usize>,
) -> Vec<usize> {
    let (mut left, mut right) = (left.peekable(), right.peekable());
    let mut merged = Vec::with_capacity(left.size_hint().0 + right.size_hint().0);
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
