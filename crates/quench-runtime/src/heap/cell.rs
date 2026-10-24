use super::records::RECORD_SPACE_CAPACITY;
use super::root::WeakHandle;
use crate::bytecode::Atom;
use crate::value::{HeapReference, Value};
use crate::value_vec::{
    INLINE_PROPERTY_COUNT, VALUE_VEC_INTEGRITY_BITS, VALUE_VEC_START_BITS, ValueVec,
};
use crate::vm::program_store::ProgramId;
use crate::vm::wtf16::JsString;
use rustc_hash::FxHashMap;
use std::rc::Rc;

#[derive(Clone, Debug, Default)]
pub(crate) struct WeakMapEntries {
    ordered: Vec<(Value, Value)>,
    indices: FxHashMap<Value, usize>,
}

#[cfg(test)]
mod cell_layout_tests {
    use super::*;

    /// Every heap slot stores an `Option<Cell>`. The M4 object layout includes
    /// two inline properties; an array keeps its element handle in that payload.
    const CELL_TAG_WORD_BYTES: usize = std::mem::size_of::<Value>();
    const MAX_CELL_BYTES: usize = std::mem::size_of::<Object>() + CELL_TAG_WORD_BYTES;

    #[test]
    fn cells_stay_compact() {
        assert_eq!(std::mem::size_of::<Option<Cell>>(), MAX_CELL_BYTES);
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
pub(crate) enum Native {
    Print, HostDone, CreateRealm, IsHTMLDDA, EvalScript, CollectGarbage, RealmTypeError, Eval, ToString, Function, FunctionPrototype, FunctionPrototypeHasInstance, FunctionCaller, DynamicImport, AbstractModuleSource, AbstractModuleSourceToStringTag, ShadowRealm, ShadowRealmEvaluate, ShadowRealmImportValue, ShadowRealmImportValueFulfilled, ShadowRealmWrappedFunction,
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
    PromiseThenableJob,
    PromiseAggregateJob,
    PromiseAsyncResumeJob, AsyncFromSyncValue, AsyncFromSyncValueRejected, AsyncGeneratorDelegateFulfilled, AsyncGeneratorDelegateRejected,
    WithEnter, WithExit,
    WasmHost,
    WebAssemblyValidate, WebAssemblyCompile, WebAssemblyInstantiate,
    WasmModule, WasmModuleExports, WasmModuleImports,
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
#[derive(Clone, Copy, Debug)]
pub(crate) enum WasmHostTarget {
    Host(crate::WasmHostFunctionId),
    Js(Value),
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

    pub(crate) fn is_host_control_native(self) -> bool {
        matches!(
            self,
            Self::HostDone | Self::CreateRealm | Self::Boolean | Self::BigInt
        ) || self.is_function_native()
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
                | Self::PromiseThenableJob
                | Self::PromiseAggregateJob
                | Self::PromiseAsyncResumeJob
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
    NumericUser(ProgramId, u32),
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
            Self::MapKeys | Self::MapValues | Self::MapEntries | Self::SetValues | Self::SetEntries
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
pub(crate) struct Object {
    /// The prototype's heap index (`PROTO_INDEX_BITS`, with null and undefined sentinels) and
    /// the `OBJECT_*` flags: array, inline storage, and the integrity bits `ValueVec` keeps.
    meta: u32,
    /// `ValueVec::auxiliary`: the shape and the dictionary flag. Property names live once in
    /// the VM's shape table; objects keep only the data the shape selects.
    auxiliary: u32,
    /// The inline property values, or, for an object that has `ObjectExtras`, a tagged
    /// pointer to them; they then also hold the inline values. Extras are rare, so the
    /// common object pays no word for them. Without inline storage, the payload also keeps
    /// the storage position (`ValueVec`'s start): the second word for a plain object; for an
    /// array, whose second word is its element backing, the first word under `START_TAG`, or
    /// the extras when it has them.
    payload: ObjectPayload,
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
const PROTO_RECORD_END: u32 = PROTO_ARRAY_RECORD_BASE + RECORD_SPACE_CAPACITY as u32;
const _: () = assert!(PROTO_RECORD_END <= PROTO_UNDEFINED);
const OBJECT_ARRAY: u32 = 1 << 28;
const OBJECT_INLINE_STORAGE: u32 = 1 << 29;
const _: () = assert!(
    PROTO_INDEX_BITS & (OBJECT_ARRAY | OBJECT_INLINE_STORAGE | VALUE_VEC_INTEGRITY_BITS) == 0
        && (OBJECT_ARRAY | OBJECT_INLINE_STORAGE) & VALUE_VEC_INTEGRITY_BITS == 0
);
/// Marks an array's storage position in its first payload word; like `EXTRAS_TAG`, no
/// `Value` carries it.
const START_TAG: u64 = 0x7ffe_8000_0000_0000;
const START_TAG_MASK: u64 = 0xffff_8000_0000_0000;

/// `Object::payload`: inline values, or `EXTRAS_TAG` and an `ObjectExtras` pointer in the first
/// word. No `Value` carries that tag: it is a NaN pattern, and numbers canonicalize NaN. An
/// array's object never stores properties inline, and its second word holds the array's element
/// backing, owned by the `ArrayObject` around it.
#[repr(C)]
union ObjectPayload {
    inline: [Value; INLINE_PROPERTY_COUNT],
    words: [u64; INLINE_PROPERTY_COUNT],
    array: std::mem::ManuallyDrop<ArrayPayload>,
}

#[repr(C)]
struct ArrayPayload {
    extras: u64,
    elements: std::mem::ManuallyDrop<Rc<Vec<Value>>>,
}

const _: () = assert!(size_of::<ArrayPayload>() == size_of::<[Value; INLINE_PROPERTY_COUNT]>());

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

impl Clone for Object {
    fn clone(&self) -> Self {
        // The words copy bit for bit; the copy then gets its own extras. An array's second
        // word is its backing, which `ArrayObject::clone` replaces without reading.
        // SAFETY: both views are plain words.
        let mut words = unsafe { self.payload.words };
        if let Some(extras) = self.extras() {
            words[0] = Self::tag_extras(Box::new(extras.clone()));
        }
        Self {
            meta: self.meta,
            auxiliary: self.auxiliary,
            payload: ObjectPayload { words },
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

/// An array: its object, whose payload also holds the dense element backing.
pub(crate) struct ArrayObject {
    object: Object,
}

impl ArrayObject {
    pub(crate) fn new(mut object: Object, elements: Rc<Vec<Value>>) -> Self {
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
        // SAFETY: the second word is unused by an object without inline storage; this writes
        // the backing there without reading or dropping what it held.
        unsafe {
            std::ptr::write(
                std::ptr::addr_of_mut!((*object.payload.array).elements),
                std::mem::ManuallyDrop::new(elements),
            );
        }
        Self { object }
    }

    pub(crate) fn elements(&self) -> &Rc<Vec<Value>> {
        // SAFETY: `new` stored the backing, which this array owns until it drops.
        unsafe { &self.object.payload.array.elements }
    }

    pub(crate) fn elements_mut(&mut self) -> &mut Rc<Vec<Value>> {
        // SAFETY: as in `elements`, with `&mut self` making the borrow unique.
        unsafe { &mut (*self.object.payload.array).elements }
    }
}

impl Drop for ArrayObject {
    fn drop(&mut self) {
        // SAFETY: the backing was stored by `new` and is dropped only here.
        unsafe { std::mem::ManuallyDrop::drop(&mut (*self.object.payload.array).elements) };
    }
}

impl Clone for ArrayObject {
    fn clone(&self) -> Self {
        Self::new_cloned(self.object.clone(), self.elements().clone())
    }
}

impl ArrayObject {
    /// `new` for an object that already has arena storage and a stale second word.
    fn new_cloned(object: Object, elements: Rc<Vec<Value>>) -> Self {
        Self::new(object, elements)
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
            .field("inline_properties", self.inline_property_values())
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
    structured_clone_forbidden: bool,
    structured_transfer_forbidden: bool,
    module_bindings: Vec<(Atom, ProgramId, u16)>,
    deferred_module: Option<crate::ModuleSource>,
    private_elements: Vec<PrivateElement>,
    stack_data: Option<StackData>,
    /// An index-keyed property descriptor has been recorded for this object; never cleared.
    indexed_descriptors: bool,
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
                module.name.capacity() + module.source.capacity() + module.bytes.capacity()
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
            None if proto.is_null() => PROTO_NULL,
            None => {
                debug_assert!(proto.is_undefined(), "a prototype is an object or null");
                PROTO_UNDEFINED
            }
        }
    }

    pub(crate) fn proto(&self) -> Value {
        match self.meta & PROTO_INDEX_BITS {
            PROTO_NULL => Value::NULL,
            PROTO_UNDEFINED => Value::UNDEFINED,
            index if index < PROTO_OBJECT_RECORD_BASE => Value::heap(index),
            index if index < PROTO_ARRAY_RECORD_BASE => {
                Value::direct_object(index - PROTO_OBJECT_RECORD_BASE)
            }
            index => {
                debug_assert!(index < PROTO_RECORD_END);
                Value::direct_array(index - PROTO_ARRAY_RECORD_BASE)
            }
        }
    }

    pub(crate) fn set_proto(&mut self, proto: Value) {
        self.meta = (self.meta & !PROTO_INDEX_BITS) | Self::encode_proto(proto);
    }

    /// Whether this object is an array's, whose payload holds the element backing.
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

    #[inline(always)]
    fn storage_position(&self) -> u32 {
        if !self.is_array() {
            // SAFETY: both views are plain words; without inline storage the second word
            // holds the position.
            return unsafe { self.payload.words[1] } as u32;
        }
        match self.extras() {
            Some(extras) => extras.arena_start,
            // SAFETY: as above; an array without extras keeps it in the first word.
            None => {
                let word = unsafe { self.payload.words[0] };
                debug_assert_eq!(word & START_TAG_MASK, START_TAG);
                word as u32
            }
        }
    }

    fn set_storage_position(&mut self, position: u32) {
        if !self.is_array() {
            // SAFETY: both views are plain words.
            unsafe { self.payload.words[1] = u64::from(position) };
        } else if let Some(extras) = self.extras_mut_existing() {
            extras.arena_start = position;
        } else {
            // SAFETY: as above.
            unsafe { self.payload.words[0] = START_TAG | u64::from(position) };
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
        // SAFETY: both union views are plain words.
        let word = unsafe { self.payload.words[0] };
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
                // An array moves its storage position from the first word into the extras;
                // any other object's words are inline values (or, without inline storage, a
                // stale first word and the position in the second, which stays).
                let arena_start = if self.is_array() {
                    self.storage_position()
                } else {
                    0
                };
                // SAFETY: an untagged payload holds plain words.
                let inline = unsafe { self.payload.inline };
                let word = Self::tag_extras(Box::new(ObjectExtras {
                    inline_properties: DisplacedInline(inline),
                    arena_start,
                    ..ObjectExtras::default()
                }));
                // Only the first word changes: an array keeps its element backing in the second.
                // SAFETY: both union views are plain words.
                unsafe { self.payload.words[0] = word };
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

    /// The inline property slots, wherever they live.
    fn inline_property_values(&self) -> &[Value; INLINE_PROPERTY_COUNT] {
        match self.extras() {
            Some(extras) => &extras.inline_properties.0,
            // SAFETY: an untagged payload holds the inline values.
            None => unsafe { &self.payload.inline },
        }
    }

    fn inline_property_values_mut(&mut self) -> &mut [Value; INLINE_PROPERTY_COUNT] {
        if let Some(extras) = self.extras_pointer() {
            // SAFETY: a tagged payload owns a live `ObjectExtras` for the object's lifetime,
            // and `&mut self` makes this its only borrow.
            unsafe { &mut (*extras).inline_properties.0 }
        } else {
            // SAFETY: an untagged payload holds the inline values.
            unsafe { &mut self.payload.inline }
        }
    }

    pub(crate) fn with_property_storage(
        proto: Value,
        properties: ValueVec,
        inline_properties: [Value; INLINE_PROPERTY_COUNT],
    ) -> Self {
        let mut object = Self {
            meta: Self::encode_proto(proto),
            auxiliary: 0,
            payload: ObjectPayload {
                inline: inline_properties,
            },
        };
        object.set_properties(properties);
        object
    }

    pub(crate) fn inline_properties(&self) -> Option<&[Value; INLINE_PROPERTY_COUNT]> {
        (self.meta & OBJECT_INLINE_STORAGE != 0).then(|| self.inline_property_values())
    }

    pub(crate) fn set_inline_property(&mut self, slot: usize, value: Value) {
        debug_assert!(self.properties().has_inline_property_storage());
        debug_assert!(slot < INLINE_PROPERTY_COUNT);
        // A cached slot past the inline values is a caller bug, not a reason to panic here.
        if let Some(entry) = self.inline_property_values_mut().get_mut(slot) {
            *entry = value;
        }
    }

    pub(crate) fn replace_property_storage(
        &mut self,
        mut properties: ValueVec,
        inline_properties: [Value; INLINE_PROPERTY_COUNT],
    ) -> ValueVec {
        let previous = self.properties();
        properties.preserve_integrity_from(previous);
        self.set_properties(properties);
        // Arena storage leaves the inline values alone: an array keeps its element backing in
        // the payload.
        if properties.has_inline_property_storage() {
            *self.inline_property_values_mut() = inline_properties;
        }
        previous
    }

    pub(crate) fn copy_property_storage_from(&mut self, source: &Self) {
        let properties = source.properties();
        self.set_properties(properties);
        if properties.has_inline_property_storage() {
            *self.inline_property_values_mut() = *source.inline_property_values();
        }
    }

    /// Presence of [[ErrorData]] is the unforgeable Error brand.
    pub(crate) fn error(proto: Value) -> Self {
        let mut object = Self::new(proto);
        object.extras_mut().error_data = true;
        object
    }

    pub(crate) fn forbid_structured_clone(&mut self) {
        self.extras_mut().structured_clone_forbidden = true;
    }

    pub(crate) fn is_structured_clone_forbidden(&self) -> bool {
        self.extras()
            .is_some_and(|extras| extras.structured_clone_forbidden)
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
        if let Some(value) = extras.boxed_primitive {
            visit(value);
        }
        for element in &extras.private_elements {
            visit(element.brand.home);
            if let Some(value) = element.data {
                visit(value);
            }
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

    pub(crate) fn was_used_as_prototype(&self) -> bool {
        self.extras()
            .is_some_and(|extras| extras.prototype_lookup != PrototypeLookupState::Ordinary)
    }

    pub(crate) fn mark_as_prototype(&mut self) {
        if !self.was_used_as_prototype() {
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

    pub(crate) fn shape(&self) -> u32 {
        ValueVec::from_parts(0, self.auxiliary).auxiliary()
    }
    pub(crate) fn set_shape(&mut self, shape: u32) {
        let mut properties = ValueVec::from_parts(0, self.auxiliary);
        properties.set_auxiliary(shape);
        self.auxiliary = properties.into_parts().1;
    }
    pub(crate) fn is_extensible(&self) -> bool {
        self.properties().is_extensible()
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
        self.extras_mut().indexed_descriptors = true;
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
        extras.structured_clone_forbidden = true;
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
        self.extras()
            .into_iter()
            .flat_map(|extras| extras.private_elements.iter().map(|element| element.brand))
    }
    pub(crate) fn has_private_name(&self, brand: PrivateBrand) -> bool {
        self.private_names().any(|candidate| candidate == brand)
    }
    pub(crate) fn add_private_name(&mut self, brand: PrivateBrand) {
        if !self.has_private_name(brand) {
            self.extras_mut()
                .private_elements
                .push(PrivateElement { brand, data: None });
        }
    }
    pub(crate) fn private_field(&self, brand: PrivateBrand) -> Option<Value> {
        self.extras()?
            .private_elements
            .iter()
            .find(|element| element.brand == brand)?
            .data
    }
    pub(crate) fn set_private_field(&mut self, brand: PrivateBrand, value: Value) -> bool {
        let Some(element) = self
            .extras_mut()
            .private_elements
            .iter_mut()
            .find(|element| element.brand == brand && element.data.is_some())
        else {
            return false;
        };
        element.data = Some(value);
        true
    }
    pub(crate) fn initialize_private_field(&mut self, brand: PrivateBrand, value: Value) -> bool {
        let elements = &mut self.extras_mut().private_elements;
        if let Some(element) = elements.iter_mut().find(|element| element.brand == brand) {
            if element.data.is_some() {
                return false;
            }
            element.data = Some(value);
        } else {
            elements.push(PrivateElement {
                brand,
                data: Some(value),
            });
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

const _: () = assert!(std::mem::size_of::<Object>() == 3 * std::mem::size_of::<Value>());

#[cfg(test)]
mod object_payload_tests {
    use super::Object;
    use crate::value::Value;
    use crate::value_vec::ValueVec;

    #[test]
    fn out_of_line_extras_preserve_inline_values_and_clone_state() {
        let mut object = Object::with_property_storage(
            Value::NULL,
            ValueVec::inline_property_storage(7),
            [Value::integer(11), Value::integer(13)],
        );
        object.set_arguments_object();
        assert!(object.is_arguments_object());
        assert_eq!(object.shape(), 7);
        assert_eq!(
            object.inline_properties().copied(),
            Some([Value::integer(11), Value::integer(13)])
        );

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
            Object::with_property_storage(Value::NULL, properties, [Value::UNDEFINED; 2]);
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

#[derive(Clone, Copy, Debug)]
pub(crate) enum EnvironmentSlot {
    Owned(Value),
    /// Direct owner of this slot index; the referenced slot is always Owned.
    Shared(Value),
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

#[derive(Clone, Debug)]
/// An environment's slots behind a one-word pointer, which keeps the environment cell within
/// the common cell size.
pub(crate) struct EnvironmentSlots(pub(super) super::thin_slice::ThinBoxSlice<EnvironmentSlot>);

impl EnvironmentSlots {
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// The value of a slot this environment owns; a shared slot lives in its owner.
    #[inline(always)]
    pub(crate) fn owned(&self, slot: usize) -> Option<Value> {
        match self.0.get(slot)? {
            EnvironmentSlot::Owned(value) => Some(*value),
            EnvironmentSlot::Shared(_) => None,
        }
    }
    pub(crate) fn roots(&self) -> impl Iterator<Item = Value> + '_ {
        self.0.iter().map(|slot| match slot {
            EnvironmentSlot::Owned(value) | EnvironmentSlot::Shared(value) => *value,
        })
    }
}

impl From<Box<[Value]>> for EnvironmentSlots {
    fn from(values: Box<[Value]>) -> Self {
        Self(
            values
                .into_iter()
                .map(EnvironmentSlot::Owned)
                .collect::<Box<[_]>>()
                .into(),
        )
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

/// A function's ordinary object and creation realm. The realm rides in the box, which
/// calls already reach through property and prototype reads, so the cell keeps four words.
#[derive(Clone, Debug)]
pub(crate) struct FunctionObject {
    pub(crate) object: Object,
    pub(crate) realm: Value,
}

impl std::ops::Deref for FunctionObject {
    type Target = Object;
    fn deref(&self) -> &Object {
        &self.object
    }
}

impl std::ops::DerefMut for FunctionObject {
    fn deref_mut(&mut self) -> &mut Object {
        &mut self.object
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

/// A RegExp's source text, flags and legacy-constructor owner: read when a pattern is
/// recompiled, reflected or matched through legacy statics, not on every match.
#[derive(Clone, Debug)]
pub(crate) struct RegExpMeta {
    pub(crate) source: JsString,
    pub(crate) flags: String,
    // One constructor identity owns creation realm and legacy eligibility.
    pub(crate) legacy_constructor: RegExpLegacyOwner,
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
    Iterator {
        object: Box<Object>,
        source: Value,
        kind: IteratorKind,
        /// Low half of the position within `source`; `IteratorExt::position` joins it with
        /// the upper half.
        index: u32,
        done: bool,
        ext: Box<IteratorExt>,
    },
    ArrayFromAsyncState(Box<ArrayFromAsyncState>),
    Proxy {
        object: Box<Object>,
        kind: ProxyKind,
        target: Value,
        handler: Value,
    },
    Function {
        object: Box<FunctionObject>,
        kind: FunctionKind,
        env: Value,
    },
    BindingReference {
        environment: Value,
        slot: u16,
        kind: crate::bytecode::LexicalBindingKind,
    },
    Environment {
        parent: Value,
        function: u32,
        slots: EnvironmentSlots,
        scope: Box<EnvironmentScope>,
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
        signature: Rc<crate::WasmSignature>,
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
    String(JsString), BigInt(String),
    Symbol(SymbolData),
    Date { milliseconds: f64, object: Box<Object> },
    RegExp {
        object: Box<Object>,
        meta: Box<RegExpMeta>,
        matcher: Rc<quench_regexp::Regex>,
    },
    Error(String),
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
            Self::Iterator { .. } => "iterator",
            Self::ArrayFromAsyncState(_) => "array_from_async_state",
            Self::Proxy { .. } => "proxy",
            Self::Function { .. } => "function",
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
            Self::String(_) => "string",
            Self::BigInt(_) => "bigint",
            Self::Symbol(_) => "symbol",
            Self::Date { .. } => "date",
            Self::RegExp { .. } => "regexp",
            Self::Error(_) => "error",
            Self::PromiseResolvingState { .. } => "promise_resolving_state",
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
            Self::Function {
                kind: FunctionKind::User(program, _) | FunctionKind::NumericUser(program, _),
                ..
            } => visit(*program),
            Self::Environment { scope, .. } => {
                if let Some(program) = scope.program {
                    visit(ProgramId::from_raw(program));
                }
            }
            Self::Iterator { ext, .. } => {
                if let Some(continuation) = ext
                    .generator
                    .as_ref()
                    .and_then(|generator| generator.continuation.as_ref())
                {
                    visit(continuation.program);
                }
            }
            _ => {}
        }
    }
}
