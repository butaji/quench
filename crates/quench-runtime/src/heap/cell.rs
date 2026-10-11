use super::root::WeakHandle;
use crate::bytecode::Atom;
use crate::value::Value;
use crate::value_vec::{INLINE_PROPERTY_COUNT, ValueVec};
use crate::vm::program_store::ProgramId;
use crate::vm::wtf16::JsString;
use rustc_hash::FxHashMap;
use std::rc::Rc;
use std::{fmt, mem::ManuallyDrop};

#[derive(Clone, Debug, Default)]
pub(crate) struct WeakMapEntries {
    ordered: Vec<(Value, Value)>,
    indices: FxHashMap<Value, usize>,
}

#[cfg(test)]
mod cell_layout_tests {
    use super::*;

    const OBJECT_HEADER_BYTES: usize =
        std::mem::size_of::<Value>() + INLINE_PROPERTY_COUNT * std::mem::size_of::<Value>();
    /// Every heap slot stores an `Option<Cell>`. The M4 object layout includes
    /// two inline properties, with array backing stored in their spare words.
    const CELL_TAG_WORD_BYTES: usize = std::mem::size_of::<Value>();
    const ARRAY_STORAGE_BYTES: usize =
        std::mem::size_of::<Rc<Vec<Value>>>() + std::mem::size_of::<Option<Box<ObjectExtras>>>();
    const MAX_CELL_BYTES: usize = std::mem::size_of::<Object>() + CELL_TAG_WORD_BYTES;

    #[test]
    fn cells_stay_compact() {
        assert_eq!(
            std::mem::size_of::<Object>(),
            OBJECT_HEADER_BYTES + std::mem::size_of::<ValueVec>()
        );
        assert_eq!(std::mem::size_of::<ArrayStorage>(), ARRAY_STORAGE_BYTES);
        assert_eq!(std::mem::size_of::<ObjectStorage>(), ARRAY_STORAGE_BYTES);
        assert_eq!(std::mem::size_of::<Option<Cell>>(), MAX_CELL_BYTES);
    }

    #[test]
    fn object_extras_preserve_inline_properties_across_clone_and_drop() {
        let first = Value::integer(17);
        let second = Value::integer(29);
        let mut object = Object::with_property_storage(
            Value::NULL,
            ValueVec::inline_property_storage(0),
            [first, second],
        );

        object.set_arguments_object();
        assert!(object.is_arguments_object());
        assert_eq!(object.inline_properties(), Some(&[first, second]));

        let clone = object.clone();
        assert!(clone.is_arguments_object());
        assert_eq!(clone.inline_properties(), Some(&[first, second]));
    }

    #[test]
    fn array_backing_survives_extras_clone_and_drop() {
        let values = Rc::new(vec![Value::integer(17), Value::integer(29)]);
        let mut array = Cell::array(Value::NULL, values);
        assert_eq!(Rc::strong_count(array.array_elements()), 1);
        assert!(!array.array_has_indexed_descriptors());

        let Cell::Array { object } = &mut array else {
            unreachable!()
        };
        assert!(object.inline_properties().is_none());
        object.set_arguments_object();
        object.mark_indexed_descriptors();
        assert!(object.is_arguments_object());
        assert!(array.array_has_indexed_descriptors());
        assert_eq!(
            array.array_elements().as_slice(),
            &[Value::integer(17), Value::integer(29)]
        );

        let clone = array.clone();
        assert_eq!(Rc::strong_count(clone.array_elements()), 2);
        let Cell::Array { object } = &clone else {
            unreachable!()
        };
        assert!(object.is_arguments_object());
        assert_eq!(clone.array_elements()[0], Value::integer(17));

        drop(array);
        assert_eq!(Rc::strong_count(clone.array_elements()), 1);
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

    #[cfg(any(feature = "profile-memory", feature = "profile-aggregate"))]
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
    FunctionCall, FunctionApply, FunctionBind, FunctionBoundCall, FunctionToString, AsyncFunction, GeneratorFunction, AsyncGeneratorFunction, AsyncGeneratorReturnResult, AsyncGeneratorDelegateReturnStart,
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
    PromiseReactionJob,
    PromiseThenableJob,
    PromiseAggregateJob,
    PromiseAsyncResumeJob, AsyncFromSyncValue, AsyncFromSyncValueRejected, AsyncGeneratorDelegateFulfilled, AsyncGeneratorDelegateRejected,
    WithEnter, WithExit,
    WasmHost,
    HostFunction,
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
                | Self::PromiseReactionJob
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
    pub proto: Value,
    // Property names live once in the VM's immutable shape table; objects keep
    // only the data vector selected by that shape.
    pub properties: ValueVec,
    storage: ObjectStorage,
}

union ObjectStorage {
    inline_properties: [Value; INLINE_PROPERTY_COUNT],
    array_storage: ManuallyDrop<ArrayStorage>,
    out_of_line: ManuallyDrop<Box<ObjectExtraStorage>>,
}

#[derive(Clone, Debug)]
struct ArrayStorage {
    elements: Rc<Vec<Value>>,
    extras: Option<Box<ObjectExtras>>,
}

#[derive(Clone, Debug)]
pub(crate) struct CallSiteRecord {
    pub(crate) file_name: String,
    pub(crate) function_name: Option<String>,
    pub(crate) this_value: Value,
    pub(crate) line: u32,
    pub(crate) column: u32,
}

#[derive(Clone, Debug)]
pub(crate) enum StackData {
    Captured(Vec<CallSiteRecord>),
    CallSite(CallSiteRecord),
}

#[derive(Clone, Debug, Default)]
struct ObjectExtras {
    arguments_map: Option<Vec<u16>>,
    arguments_object: bool,
    raw_json: bool,
    error_data: bool,
    module_namespace: bool,
    module_bindings: Vec<(Atom, ProgramId, u16)>,
    deferred_module: Option<crate::ModuleSource>,
    private_names: Vec<PrivateBrand>,
    stack_data: Option<StackData>,
    /// An index-keyed property descriptor has been recorded for this object; never cleared.
    indexed_descriptors: bool,
}

#[derive(Clone, Debug)]
struct ObjectExtraStorage {
    inline_properties: [Value; INLINE_PROPERTY_COUNT],
    extras: ObjectExtras,
}

impl Clone for Object {
    fn clone(&self) -> Self {
        let storage = if self.properties.has_array_elements() {
            // SAFETY: Cell::Array initializes this union arm before exposing
            // its Object header, and the array-kind bit remains set for life.
            let array = unsafe { &*self.storage.array_storage };
            ObjectStorage {
                array_storage: ManuallyDrop::new(array.clone()),
            }
        } else if let Some(out_of_line) = self.out_of_line() {
            ObjectStorage {
                out_of_line: ManuallyDrop::new(Box::new(out_of_line.clone())),
            }
        } else {
            ObjectStorage {
                inline_properties: *self.inline_property_values(),
            }
        };
        Self {
            proto: self.proto,
            properties: self.properties,
            storage,
        }
    }
}

impl fmt::Debug for Object {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Object")
            .field("proto", &self.proto)
            .field("properties", &self.properties)
            .field("inline_properties", self.inline_property_values())
            .field("extras", &self.extras())
            .finish()
    }
}

impl Drop for Object {
    fn drop(&mut self) {
        if self.properties.has_array_elements() {
            // SAFETY: Cell::Array initializes this union arm and keeps its
            // array-kind bit set until the record is dropped.
            unsafe { ManuallyDrop::drop(&mut self.storage.array_storage) };
        } else if self.properties.has_object_extras() {
            // SAFETY: the object-extras bit is set only after `storage` is
            // initialized with the matching out-of-line allocation.
            unsafe { ManuallyDrop::drop(&mut self.storage.out_of_line) };
        }
    }
}
impl ObjectExtras {
    #[cfg(any(feature = "profile-memory", feature = "profile-aggregate"))]
    fn allocated_bytes(&self) -> usize {
        self.arguments_map
            .as_ref()
            .map_or(0, |mapping| mapping.capacity() * std::mem::size_of::<u16>())
            + self.module_bindings.capacity() * std::mem::size_of::<(Atom, ProgramId, u16)>()
            + self.deferred_module.as_ref().map_or(0, |module| {
                module.name.capacity() + module.source.capacity() + module.bytes.capacity()
            })
            + self.private_names.capacity() * std::mem::size_of::<PrivateBrand>()
            + self
                .stack_data
                .as_ref()
                .map_or(0, StackData::allocated_bytes)
    }
}

impl ObjectExtraStorage {
    #[cfg(any(feature = "profile-memory", feature = "profile-aggregate"))]
    fn allocated_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.extras.allocated_bytes()
    }
}
impl StackData {
    #[cfg(any(feature = "profile-memory", feature = "profile-aggregate"))]
    fn allocated_bytes(&self) -> usize {
        fn record_bytes(record: &CallSiteRecord) -> usize {
            record.file_name.capacity() + record.function_name.as_ref().map_or(0, String::capacity)
        }
        match self {
            Self::Captured(records) => {
                records.capacity() * std::mem::size_of::<CallSiteRecord>()
                    + records.iter().map(record_bytes).sum::<usize>()
            }
            Self::CallSite(record) => record_bytes(record),
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
    pub(crate) fn new(proto: Value) -> Self {
        Self {
            proto,
            properties: ValueVec::inline_property_storage(0),
            storage: ObjectStorage {
                inline_properties: [Value::UNDEFINED; INLINE_PROPERTY_COUNT],
            },
        }
    }

    pub(crate) fn with_property_storage(
        proto: Value,
        mut properties: ValueVec,
        inline_properties: [Value; INLINE_PROPERTY_COUNT],
    ) -> Self {
        properties.set_object_extras(false);
        properties.set_array_elements(false);
        Self {
            proto,
            properties,
            storage: ObjectStorage { inline_properties },
        }
    }

    fn into_array_elements(mut self, elements: Rc<Vec<Value>>) -> Self {
        debug_assert!(!self.properties.has_object_extras());
        debug_assert!(!self.properties.has_array_elements());
        debug_assert!(self.properties.has_inline_property_storage());
        debug_assert!(
            self.inline_property_values()
                .iter()
                .all(|value| *value == Value::UNDEFINED)
        );
        let properties = ValueVec::array_property_storage(self.shape());
        self.storage = ObjectStorage {
            array_storage: ManuallyDrop::new(ArrayStorage {
                elements,
                extras: None,
            }),
        };
        self.properties = properties;
        self
    }

    pub(crate) fn inline_properties(&self) -> Option<&[Value; INLINE_PROPERTY_COUNT]> {
        self.properties
            .has_inline_property_storage()
            .then(|| self.inline_property_values())
    }

    #[inline(always)]
    fn array_elements(&self) -> &Rc<Vec<Value>> {
        debug_assert!(self.properties.has_array_elements());
        // SAFETY: Cell::Array is the authority for this union arm. Its
        // elements pointer stays at this offset whether or not extras exist.
        unsafe { &self.storage.array_storage.elements }
    }

    #[inline(always)]
    fn array_elements_mut(&mut self) -> &mut Rc<Vec<Value>> {
        debug_assert!(self.properties.has_array_elements());
        // SAFETY: Cell::Array is the authority for this union arm. Its
        // elements pointer stays at this offset whether or not extras exist.
        unsafe { &mut (*self.storage.array_storage).elements }
    }

    pub(crate) fn set_inline_property(&mut self, slot: usize, value: Value) {
        debug_assert!(self.properties.has_inline_property_storage());
        debug_assert!(slot < INLINE_PROPERTY_COUNT);
        if let Some(out_of_line) = self.out_of_line_mut() {
            out_of_line.inline_properties[slot] = value;
        } else {
            // SAFETY: without object extras, the union stores inline values.
            unsafe { self.storage.inline_properties[slot] = value };
        }
    }

    pub(crate) fn replace_property_storage(
        &mut self,
        mut properties: ValueVec,
        inline_properties: [Value; INLINE_PROPERTY_COUNT],
    ) -> ValueVec {
        let previous = self.properties;
        properties.preserve_integrity_from(previous);
        self.properties = properties;
        if !self.properties.has_array_elements() {
            if let Some(out_of_line) = self.out_of_line_mut() {
                out_of_line.inline_properties = inline_properties;
            } else {
                self.storage.inline_properties = inline_properties;
            }
        }
        previous
    }

    pub(crate) fn copy_property_storage_from(&mut self, source: &Self) {
        let mut properties = source.properties;
        properties.set_object_extras(self.properties.has_object_extras());
        properties.set_array_elements(self.properties.has_array_elements());
        self.properties = properties;
        if !self.properties.has_array_elements() {
            let inline_properties = *source.inline_property_values();
            if let Some(out_of_line) = self.out_of_line_mut() {
                out_of_line.inline_properties = inline_properties;
            } else {
                self.storage.inline_properties = inline_properties;
            }
        }
    }

    /// Presence of [[ErrorData]] is the unforgeable Error brand.
    pub(crate) fn error(proto: Value) -> Self {
        let mut object = Self::new(proto);
        object.extras_mut().error_data = true;
        object
    }

    pub(crate) fn has_error_data(&self) -> bool {
        self.extras().is_some_and(|extras| extras.error_data)
    }

    pub(crate) fn stack_data(&self) -> Option<&StackData> {
        self.extras()?.stack_data.as_ref()
    }

    pub(crate) fn set_stack_data(&mut self, data: StackData) {
        self.extras_mut().stack_data = Some(data);
    }

    pub(crate) fn clear_stack_data(&mut self) {
        if let Some(extras) = self.extras_mut_if_present() {
            extras.stack_data = None;
        }
    }

    pub(crate) fn visit_stack_data_roots(&self, mut visit: impl FnMut(Value)) {
        match self.extras().and_then(|extras| extras.stack_data.as_ref()) {
            Some(StackData::Captured(records)) => {
                for record in records {
                    visit(record.this_value);
                }
            }
            Some(StackData::CallSite(record)) => visit(record.this_value),
            None => {}
        }
    }

    pub(crate) fn shape(&self) -> u32 {
        self.properties.auxiliary()
    }
    pub(crate) fn set_shape(&mut self, shape: u32) {
        self.properties.set_auxiliary(shape);
    }
    pub(crate) fn is_extensible(&self) -> bool {
        self.properties.is_extensible()
    }
    pub(crate) fn set_extensible(&mut self, value: bool) {
        self.properties.set_extensible(value);
    }
    pub(crate) fn is_frozen(&self) -> bool {
        self.properties.is_frozen()
    }
    pub(crate) fn set_frozen(&mut self, value: bool) {
        self.properties.set_frozen(value);
    }
    fn extras_mut(&mut self) -> &mut ObjectExtras {
        if self.properties.has_array_elements() {
            // Array records keep their elements pointer fixed and place only
            // rare extras behind the second word of the same storage arm.
            // SAFETY: the array-kind bit selects the initialized ArrayStorage.
            let array = unsafe { &mut *self.storage.array_storage };
            let extras = array
                .extras
                .get_or_insert_with(|| Box::new(ObjectExtras::default()));
            return extras;
        }
        if !self.properties.has_object_extras() {
            // SAFETY: before the mode bit changes, the union stores the two
            // inline property values. They move into the out-of-line record
            // with the rare extras, keeping ordinary objects at 32 bytes.
            let inline_properties = if self.properties.has_inline_property_storage() {
                // SAFETY: without object extras, the union stores inline values.
                unsafe { self.storage.inline_properties }
            } else {
                [Value::UNDEFINED; INLINE_PROPERTY_COUNT]
            };
            self.storage = ObjectStorage {
                out_of_line: ManuallyDrop::new(Box::new(ObjectExtraStorage {
                    inline_properties,
                    extras: ObjectExtras::default(),
                })),
            };
            self.properties.set_object_extras(true);
        }
        &mut self
            .out_of_line_mut()
            .expect("object extras mode has out-of-line storage")
            .extras
    }
    pub(crate) fn arguments_map(&self) -> Option<&[u16]> {
        self.extras()?.arguments_map.as_deref()
    }
    pub(crate) fn arguments_map_mut(&mut self) -> Option<&mut Vec<u16>> {
        self.extras_mut_if_present()?.arguments_map.as_mut()
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
    fn array_has_indexed_descriptors(&self) -> bool {
        self.array_extras()
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
    pub(crate) fn private_names(&self) -> &[PrivateBrand] {
        self.extras().map_or(&[], |extras| &extras.private_names)
    }
    pub(crate) fn has_private_name(&self, brand: PrivateBrand) -> bool {
        self.private_names().contains(&brand)
    }
    pub(crate) fn add_private_name(&mut self, brand: PrivateBrand) {
        let names = &mut self.extras_mut().private_names;
        if !names.contains(&brand) {
            names.push(brand);
        }
    }
    #[cfg(any(feature = "profile-memory", feature = "profile-aggregate"))]
    pub(crate) fn allocated_extra_bytes(&self) -> usize {
        if self.properties.has_array_elements() {
            self.array_extras().map_or(0, |extras| {
                std::mem::size_of::<ObjectExtras>() + extras.allocated_bytes()
            })
        } else {
            self.out_of_line()
                .map_or(0, ObjectExtraStorage::allocated_bytes)
        }
    }

    pub(crate) fn module_binding(&self, atom: Atom) -> Option<(ProgramId, u16)> {
        self.module_bindings()
            .iter()
            .find_map(|(name, program, slot)| (*name == atom).then_some((*program, *slot)))
    }

    fn inline_property_values(&self) -> &[Value; INLINE_PROPERTY_COUNT] {
        if self.properties.has_array_elements() {
            // Arrays store named properties in ValueArena and elements in the
            // array storage arm, so they have no inline named-property slots.
            &[Value::UNDEFINED; INLINE_PROPERTY_COUNT]
        } else if let Some(out_of_line) = self.out_of_line() {
            &out_of_line.inline_properties
        } else {
            // SAFETY: without object extras, the union stores inline values.
            unsafe { &self.storage.inline_properties }
        }
    }

    fn extras(&self) -> Option<&ObjectExtras> {
        if self.properties.has_array_elements() {
            self.array_extras()
        } else {
            self.out_of_line().map(|storage| &storage.extras)
        }
    }

    fn extras_mut_if_present(&mut self) -> Option<&mut ObjectExtras> {
        if self.properties.has_array_elements() {
            // SAFETY: the array-kind bit selects the initialized ArrayStorage.
            return unsafe { &mut (*self.storage.array_storage).extras }.as_deref_mut();
        }
        self.out_of_line_mut().map(|storage| &mut storage.extras)
    }

    fn array_extras(&self) -> Option<&ObjectExtras> {
        debug_assert!(self.properties.has_array_elements());
        // SAFETY: the array-kind bit selects the initialized ArrayStorage.
        unsafe { &self.storage.array_storage.extras }.as_deref()
    }

    fn out_of_line(&self) -> Option<&ObjectExtraStorage> {
        if self.properties.has_array_elements() || !self.properties.has_object_extras() {
            return None;
        }
        // SAFETY: the mode bit selects the initialized boxed union field.
        Some(unsafe { &self.storage.out_of_line })
    }

    fn out_of_line_mut(&mut self) -> Option<&mut ObjectExtraStorage> {
        if self.properties.has_array_elements() || !self.properties.has_object_extras() {
            return None;
        }
        // SAFETY: the mode bit selects the initialized boxed union field.
        Some(unsafe { &mut self.storage.out_of_line })
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
pub(crate) struct EnvironmentSlots(pub(super) Box<[EnvironmentSlot]>);

impl EnvironmentSlots {
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub(crate) fn roots(&self) -> impl Iterator<Item = Value> + '_ {
        self.0.iter().map(|slot| match slot {
            EnvironmentSlot::Owned(value) | EnvironmentSlot::Shared(value) => *value,
        })
    }
}

impl From<Box<[Value]>> for EnvironmentSlots {
    fn from(values: Box<[Value]>) -> Self {
        Self(values.into_iter().map(EnvironmentSlot::Owned).collect())
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
    pub(crate) next_method: Option<Value>,
    pub(crate) helper: Option<Box<IteratorHelper>>,
    pub(crate) helper_running: bool,
    pub(crate) helper_started: bool,
    pub(crate) generator: Option<Box<crate::vm::activation::GeneratorRecord>>,
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

#[derive(Clone, Debug)]
#[rustfmt::skip]
pub(crate) enum Cell {
    Object(Object),
    Array {
        object: Object,
    },
    ArrayBuffer {
        object: Box<Object>,
        bytes: Rc<Vec<u8>>,
        shared: bool,
        detached: bool,
        max_byte_length: usize,
        resizable: bool,
        immutable: bool,
    },
    TypedArray {
        kind: TypedArrayKind,
        object: Box<Object>,
        buffer: Value,
        offset: usize,
        length: usize,
        length_tracking: bool,
    },
    DataView {
        object: Box<Object>,
        buffer: Value,
        offset: usize,
        length: usize,
        length_tracking: bool,
    },
    Map {
        object: Box<Object>,
        entries: Vec<(Value, Value)>,
    },
    Set {
        object: Box<Object>,
        entries: Vec<Value>,
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
        entries: Vec<Value>,
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
        index: usize,
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
        object: Box<Object>,
        kind: FunctionKind,
        env: Value,
        realm: Value,
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
    WasmException { tag: Value, payload: Vec<Value> },
    /// A tag retains its original declaration; identity is independent of type equality.
    WasmTag { declarations: crate::WasmTypes, ty: u32 },
    /// GC object identity owns its original declaration and traced field values.
    WasmGc { declarations: Box<crate::WasmTypes>, ty: u32, fields: Box<Vec<Value>>, descriptor: Option<Value> },
    /// The native callable's immutable host operation and structural signature.
    WasmHostFunction {
        id: crate::WasmHostFunctionId,
        signature: Rc<crate::WasmSignature>,
    },
    /// Immutable references available until an element segment drops.
    WasmElements(Vec<Value>),
    /// One memory identity owns its bytes and original optional maximum.
    WasmGlobal { value: Value, ty: crate::WasmType, declarations: crate::WasmTypes, mutable: bool },
    WasmMemory { bytes: std::sync::Arc<crate::wasm::memory::MemoryStorage>, ty: Box<wasmparser::MemoryType> },
    /// Typed references owned by a Wasm instance, traced like other heap edges.
    WasmTable {
        table64: bool,
        elements: Box<Vec<Value>>,
        element_type: wasmparser::RefType,
        declarations: Box<crate::WasmTypes>,
        maximum: Option<u64>,
    },
    String(JsString), BigInt(String),
    Symbol(Option<String>),
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
        date: (i32, u32, u32),
        time: Box<[u32; 6]>,
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
        epoch_nanoseconds: Box<i128>,
        time_zone: Box<String>,
        calendar: Box<String>,
    },
    TemporalInstant {
        object: Box<Object>,
        epoch_nanoseconds: Box<i128>,
    },
}

impl Cell {
    #[inline(always)]
    pub(crate) fn array_has_indexed_descriptors(&self) -> bool {
        match self {
            Self::Array { object } => object.array_has_indexed_descriptors(),
            _ => unreachable!("array descriptors requested from a non-array cell"),
        }
    }

    #[inline(always)]
    pub(crate) fn array_elements(&self) -> &Rc<Vec<Value>> {
        match self {
            Self::Array { object } => object.array_elements(),
            _ => unreachable!("array elements requested from a non-array cell"),
        }
    }

    #[inline(always)]
    pub(crate) fn array_elements_mut(&mut self) -> &mut Rc<Vec<Value>> {
        match self {
            Self::Array { object } => object.array_elements_mut(),
            _ => unreachable!("array elements requested from a non-array cell"),
        }
    }

    pub(crate) fn array(proto: Value, elements: Rc<Vec<Value>>) -> Self {
        Self::Array {
            object: Object::new(proto).into_array_elements(elements),
        }
    }
}

#[cfg(feature = "profile-memory")]
impl Cell {
    pub(crate) fn profile_variant_name(&self) -> &'static str {
        match self {
            Self::Object(_) => "object",
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
