//! The layer's entries the VM core calls (`quench_runtime::vm::layer_hooks`).
use crate::intl_prelude::*;
use quench_runtime::vm::layer_hooks::signatures;

#[quench_macros::ext(IntlTemporalDispatch)]
impl Vm {
    /// The Intl and Temporal natives the core's `call_native_table` hands over.
    fn intl_temporal_native(&mut self, p: &ResidualProgram, native: Native, this: Value, args: &[Value]) -> Result<Value, JsError> {
        match native {
            Native::TemporalInitialize => self.install_temporal(p).map(|()| Value::UNDEFINED),
            Native::IntlNumberFormat => self.intl_number_format_call(p, this, args),
            Native::IntlNumberFormatSupportedLocalesOf
            | Native::IntlDisplayNamesSupportedLocalesOf => {
                self.intl_supported_locales_of(p, args, crate::intl_number::is_supported_locale)
            }
            Native::IntlNumberFormatFormatGetter => self.intl_number_format_format_getter(p, this),
            Native::IntlNumberFormatFormat => self.intl_number_format_format(p, this, args),
            Native::IntlNumberFormatFormatToParts => {
                self.intl_number_format_format_to_parts(p, this, args)
            }
            Native::IntlNumberFormatFormatRange => {
                self.intl_number_format_format_range(p, this, args)
            }
            Native::IntlNumberFormatFormatRangeToParts => {
                self.intl_number_format_format_range_to_parts(p, this, args)
            }
            Native::IntlNumberFormatResolvedOptions => {
                self.intl_number_format_resolved_options(p, this)
            }
            Native::IntlPluralRulesSupportedLocalesOf
            | Native::IntlPluralRulesSelect
            | Native::IntlPluralRulesSelectRange
            | Native::IntlPluralRulesResolvedOptions => {
                self.intl_plural_rules_native(p, native, this, args)
            }
            Native::IntlGetCanonicalLocales
            | Native::IntlSupportedValuesOf
            | Native::IntlLocale
            | Native::IntlLocaleToString
            | Native::IntlLocaleMaximize
            | Native::IntlLocaleMinimize
            | Native::IntlLocaleGetCalendars
            | Native::IntlLocaleGetCollations
            | Native::IntlLocaleGetHourCycles
            | Native::IntlLocaleGetNumberingSystems
            | Native::IntlLocaleGetTimeZones
            | Native::IntlLocaleGetTextInfo
            | Native::IntlLocaleGetWeekInfo
            | Native::IntlLocaleBaseNameGetter
            | Native::IntlLocaleLanguageGetter
            | Native::IntlLocaleScriptGetter
            | Native::IntlLocaleRegionGetter
            | Native::IntlLocaleVariantsGetter
            | Native::IntlLocaleCalendarGetter
            | Native::IntlLocaleCollationGetter
            | Native::IntlLocaleHourCycleGetter
            | Native::IntlLocaleCaseFirstGetter
            | Native::IntlLocaleFirstDayOfWeekGetter
            | Native::IntlLocaleNumberingSystemGetter
            | Native::IntlLocaleNumericGetter => self.intl_namespace_native(p, native, this, args),
            Native::IntlRelativeTimeFormat => {
                self.intl_relative_time_format_native(p, native, this, args)
            }
            Native::IntlRelativeTimeFormatResolvedOptions => {
                self.intl_relative_time_format_resolved_options(p, this)
            }
            Native::IntlRelativeTimeFormatSupportedLocalesOf => {
                self.intl_supported_locales_of(p, args, crate::intl_number::is_supported_locale)
            }
            Native::IntlRelativeTimeFormatFormat | Native::IntlRelativeTimeFormatFormatToParts => {
                self.intl_relative_time_format_native(p, native, this, args)
            }
            Native::IntlSegmenterSupportedLocalesOf
            | Native::IntlSegmenterSegment
            | Native::IntlSegmenterResolvedOptions
            | Native::IntlSegmenterSegmentsIterator
            | Native::IntlSegmenterSegmentsContaining
            | Native::IntlSegmenterIteratorNext => {
                self.intl_segmenter_native(p, native, this, args)
            }
            Native::IntlCollator
            | Native::IntlCollatorSupportedLocalesOf
            | Native::IntlCollatorCompareGetter
            | Native::IntlCollatorCompare
            | Native::IntlCollatorResolvedOptions => {
                self.intl_collator_native(p, native, this, args)
            }
            Native::IntlDateTimeFormat => self.intl_date_time_format_call(p, this, args),
            Native::IntlDisplayNamesOf | Native::IntlDisplayNamesResolvedOptions => {
                self.intl_display_names_native(p, native, this, args)
            }
            Native::IntlDurationFormatSupportedLocalesOf => {
                self.intl_supported_locales_of(p, args, crate::intl_number::is_supported_locale)
            }
            Native::IntlDurationFormatFormatGetter
            | Native::IntlDurationFormatFormat
            | Native::IntlDurationFormatFormatToParts
            | Native::IntlDurationFormatResolvedOptions => {
                self.intl_duration_format_native(p, native, this, args)
            }
            Native::IntlListFormatFormatGetter
            | Native::IntlListFormatFormat
            | Native::IntlListFormatFormatToParts
            | Native::IntlListFormatResolvedOptions
            | Native::IntlListFormatSupportedLocalesOf => {
                self.intl_list_format_native(p, native, this, args)
            }
            Native::IntlDateTimeFormatFormatGetter
            | Native::IntlDateTimeFormatFormat
            | Native::IntlDateTimeFormatFormatToParts
            | Native::IntlDateTimeFormatFormatRange
            | Native::IntlDateTimeFormatFormatRangeToParts
            | Native::IntlDateTimeFormatSupportedLocalesOf
            | Native::IntlDateTimeFormatResolvedOptions => {
                self.intl_date_time_format_native(p, native, this, args)
            }
            Native::TemporalToLocaleString => match self.heap_reads().get(this) {
                Some(Cell::TemporalDuration { .. }) => {
                    self.temporal_duration_to_locale_string(p, this, args)
                }
                _ => self.temporal_to_locale_string(p, this, args),
            },
            Native::TemporalDuration
            | Native::TemporalDurationFrom
            | Native::TemporalDurationCompare
            | Native::TemporalDurationAdd
            | Native::TemporalDurationSubtract
            | Native::TemporalDurationWith
            | Native::TemporalDurationAbs
            | Native::TemporalDurationNegated
            | Native::TemporalDurationTotal
            | Native::TemporalDurationRound
            | Native::TemporalDurationToString
            | Native::TemporalDurationToJSON
            | Native::TemporalDurationValueOf
            | Native::TemporalDurationYearsGetter
            | Native::TemporalDurationMonthsGetter
            | Native::TemporalDurationWeeksGetter
            | Native::TemporalDurationDaysGetter
            | Native::TemporalDurationHoursGetter
            | Native::TemporalDurationMinutesGetter
            | Native::TemporalDurationSecondsGetter
            | Native::TemporalDurationMillisecondsGetter
            | Native::TemporalDurationMicrosecondsGetter
            | Native::TemporalDurationNanosecondsGetter
            | Native::TemporalDurationSignGetter
            | Native::TemporalDurationBlankGetter => {
                self.temporal_duration_native(p, native, this, args)
            }
            Native::TemporalPlainDate
            | Native::TemporalPlainDateFrom
            | Native::TemporalPlainDateCompare
            | Native::TemporalPlainDateCalendarIdGetter
            | Native::TemporalPlainDateYearGetter
            | Native::TemporalPlainDateMonthGetter
            | Native::TemporalPlainDateMonthCodeGetter
            | Native::TemporalPlainDateDayGetter
            | Native::TemporalPlainDateEraGetter
            | Native::TemporalPlainDateEraYearGetter
            | Native::TemporalPlainDateDayOfWeekGetter
            | Native::TemporalPlainDateDayOfYearGetter
            | Native::TemporalPlainDateWeekOfYearGetter
            | Native::TemporalPlainDateYearOfWeekGetter
            | Native::TemporalPlainDateDaysInWeekGetter
            | Native::TemporalPlainDateDaysInMonthGetter
            | Native::TemporalPlainDateDaysInYearGetter
            | Native::TemporalPlainDateMonthsInYearGetter
            | Native::TemporalPlainDateInLeapYearGetter
            | Native::TemporalPlainDateToString
            | Native::TemporalPlainDateToJSON
            | Native::TemporalPlainDateToLocaleString
            | Native::TemporalPlainDateToPlainDateTime
            | Native::TemporalPlainDateToPlainMonthDay
            | Native::TemporalPlainDateToPlainYearMonth
            | Native::TemporalPlainDateToZonedDateTime
            | Native::TemporalPlainDateEquals
            | Native::TemporalPlainDateWith
            | Native::TemporalPlainDateWithCalendar
            | Native::TemporalPlainDateValueOf => {
                self.temporal_plain_date_native(p, native, this, args)
            }
            Native::TemporalPlainDateAdd | Native::TemporalPlainDateSubtract => {
                self.temporal_plain_date_arithmetic(p, native, this, args)
            }
            Native::TemporalPlainDateUntil | Native::TemporalPlainDateSince => {
                self.temporal_plain_date_difference(p, native, this, args)
            }
            Native::TemporalPlainDateTime
            | Native::TemporalPlainDateTimeFrom
            | Native::TemporalPlainDateTimeCompare
            | Native::TemporalPlainDateTimeAdd
            | Native::TemporalPlainDateTimeSubtract
            | Native::TemporalPlainDateTimeRound
            | Native::TemporalPlainDateTimeUntil
            | Native::TemporalPlainDateTimeSince
            | Native::TemporalPlainDateTimeToString
            | Native::TemporalPlainDateTimeToJSON
            | Native::TemporalPlainDateTimeToPlainDate
            | Native::TemporalPlainDateTimeToPlainTime
            | Native::TemporalPlainDateTimeValueOf
            | Native::TemporalPlainDateTimeToZonedDateTime
            | Native::TemporalPlainDateTimeWith
            | Native::TemporalPlainDateTimeWithCalendar
            | Native::TemporalPlainDateTimeWithPlainTime
            | Native::TemporalPlainDateTimeCalendarIdGetter
            | Native::TemporalPlainDateTimeYearGetter
            | Native::TemporalPlainDateTimeMonthGetter
            | Native::TemporalPlainDateTimeMonthCodeGetter
            | Native::TemporalPlainDateTimeDayGetter
            | Native::TemporalPlainDateTimeEraGetter
            | Native::TemporalPlainDateTimeEraYearGetter
            | Native::TemporalPlainDateTimeDayOfWeekGetter
            | Native::TemporalPlainDateTimeDayOfYearGetter
            | Native::TemporalPlainDateTimeWeekOfYearGetter
            | Native::TemporalPlainDateTimeYearOfWeekGetter
            | Native::TemporalPlainDateTimeDaysInWeekGetter
            | Native::TemporalPlainDateTimeDaysInMonthGetter
            | Native::TemporalPlainDateTimeDaysInYearGetter
            | Native::TemporalPlainDateTimeMonthsInYearGetter
            | Native::TemporalPlainDateTimeInLeapYearGetter
            | Native::TemporalPlainDateTimeHourGetter
            | Native::TemporalPlainDateTimeMinuteGetter
            | Native::TemporalPlainDateTimeSecondGetter
            | Native::TemporalPlainDateTimeMillisecondGetter
            | Native::TemporalPlainDateTimeMicrosecondGetter
            | Native::TemporalPlainDateTimeNanosecondGetter
            | Native::TemporalPlainDateTimeEquals => {
                self.temporal_plain_date_time_native(p, native, this, args)
            }
            Native::TemporalPlainTime
            | Native::TemporalPlainTimeFrom
            | Native::TemporalPlainTimeCompare
            | Native::TemporalPlainTimeAdd
            | Native::TemporalPlainTimeSubtract
            | Native::TemporalPlainTimeEquals
            | Native::TemporalPlainTimeHourGetter
            | Native::TemporalPlainTimeMinuteGetter
            | Native::TemporalPlainTimeSecondGetter
            | Native::TemporalPlainTimeMillisecondGetter
            | Native::TemporalPlainTimeMicrosecondGetter
            | Native::TemporalPlainTimeNanosecondGetter => {
                self.temporal_plain_time_native(p, native, this, args)
            }
            Native::TemporalPlainTimeValueOf => {
                self.temporal_plain_time_native(p, native, this, args)
            }
            Native::TemporalPlainTimeRound
            | Native::TemporalPlainTimeUntil
            | Native::TemporalPlainTimeSince
            | Native::TemporalPlainTimeToString
            | Native::TemporalPlainTimeToJSON
            | Native::TemporalPlainTimeWith => {
                self.temporal_plain_time_native(p, native, this, args)
            }
            Native::TemporalPlainMonthDay
            | Native::TemporalPlainMonthDayFrom
            | Native::TemporalPlainMonthDayCompare
            | Native::TemporalPlainMonthDayCalendarIdGetter
            | Native::TemporalPlainMonthDayDayGetter
            | Native::TemporalPlainMonthDayMonthCodeGetter
            | Native::TemporalPlainMonthDayEquals
            | Native::TemporalPlainMonthDayToPlainDate
            | Native::TemporalPlainMonthDayWith
            | Native::TemporalPlainMonthDayValueOf
            | Native::TemporalPlainYearMonth
            | Native::TemporalPlainYearMonthFrom
            | Native::TemporalPlainYearMonthCompare
            | Native::TemporalPlainYearMonthCalendarIdGetter
            | Native::TemporalPlainYearMonthYearGetter
            | Native::TemporalPlainYearMonthMonthGetter
            | Native::TemporalPlainYearMonthMonthCodeGetter
            | Native::TemporalPlainYearMonthEraGetter
            | Native::TemporalPlainYearMonthEraYearGetter
            | Native::TemporalPlainYearMonthReferenceISODayGetter
            | Native::TemporalPlainYearMonthDaysInMonthGetter
            | Native::TemporalPlainYearMonthDaysInYearGetter
            | Native::TemporalPlainYearMonthMonthsInYearGetter
            | Native::TemporalPlainYearMonthInLeapYearGetter
            | Native::TemporalPlainYearMonthEquals
            | Native::TemporalPlainYearMonthAdd
            | Native::TemporalPlainYearMonthSubtract
            | Native::TemporalPlainYearMonthUntil
            | Native::TemporalPlainYearMonthSince
            | Native::TemporalPlainYearMonthWith
            | Native::TemporalPlainYearMonthValueOf
            | Native::TemporalPlainYearMonthToPlainDate => {
                self.temporal_calendar_projection_native(p, native, this, args)
            }
            Native::TemporalZonedDateTime
            | Native::TemporalZonedDateTimeFrom
            | Native::TemporalZonedDateTimeCompare
            | Native::TemporalZonedDateTimeEquals
            | Native::TemporalZonedDateTimeWith
            | Native::TemporalZonedDateTimeWithTimeZone
            | Native::TemporalZonedDateTimeWithCalendar
            | Native::TemporalZonedDateTimeWithPlainTime
            | Native::TemporalZonedDateTimeToInstant
            | Native::TemporalZonedDateTimeToPlainDate
            | Native::TemporalZonedDateTimeToPlainDateTime
            | Native::TemporalZonedDateTimeToPlainTime
            | Native::TemporalZonedDateTimeValueOf
            | Native::TemporalZonedDateTimeAdd
            | Native::TemporalZonedDateTimeSubtract
            | Native::TemporalZonedDateTimeGetTimeZoneTransition
            | Native::TemporalZonedDateTimeStartOfDay
            | Native::TemporalZonedDateTimeRound
            | Native::TemporalZonedDateTimeUntil
            | Native::TemporalZonedDateTimeSince
            | Native::TemporalZonedDateTimeToString
            | Native::TemporalZonedDateTimeToJSON
            | Native::TemporalZonedDateTimeToLocaleString
            | Native::TemporalZonedDateTimeEpochNanosecondsGetter
            | Native::TemporalZonedDateTimeTimeZoneIdGetter
            | Native::TemporalZonedDateTimeCalendarIdGetter
            | Native::TemporalZonedDateTimeYearGetter
            | Native::TemporalZonedDateTimeMonthGetter
            | Native::TemporalZonedDateTimeMonthCodeGetter
            | Native::TemporalZonedDateTimeDayGetter
            | Native::TemporalZonedDateTimeHourGetter
            | Native::TemporalZonedDateTimeMinuteGetter
            | Native::TemporalZonedDateTimeSecondGetter
            | Native::TemporalZonedDateTimeMillisecondGetter
            | Native::TemporalZonedDateTimeMicrosecondGetter
            | Native::TemporalZonedDateTimeNanosecondGetter
            | Native::TemporalZonedDateTimeEraGetter
            | Native::TemporalZonedDateTimeEraYearGetter
            | Native::TemporalZonedDateTimeDayOfWeekGetter
            | Native::TemporalZonedDateTimeDayOfYearGetter
            | Native::TemporalZonedDateTimeWeekOfYearGetter
            | Native::TemporalZonedDateTimeYearOfWeekGetter
            | Native::TemporalZonedDateTimeDaysInWeekGetter
            | Native::TemporalZonedDateTimeDaysInMonthGetter
            | Native::TemporalZonedDateTimeDaysInYearGetter
            | Native::TemporalZonedDateTimeMonthsInYearGetter
            | Native::TemporalZonedDateTimeInLeapYearGetter
            | Native::TemporalZonedDateTimeEpochMillisecondsGetter
            | Native::TemporalZonedDateTimeHoursInDayGetter
            | Native::TemporalZonedDateTimeOffsetGetter
            | Native::TemporalZonedDateTimeOffsetNanosecondsGetter => {
                self.temporal_zoned_date_time_native(p, native, this, args)
            }
            Native::TemporalInstant
            | Native::TemporalInstantFrom
            | Native::TemporalInstantCompare
            | Native::TemporalInstantFromEpochMilliseconds
            | Native::TemporalInstantFromEpochNanoseconds
            | Native::TemporalInstantEpochNanosecondsGetter
            | Native::TemporalInstantEpochMillisecondsGetter
            | Native::TemporalInstantToString
            | Native::TemporalInstantToJSON
            | Native::TemporalInstantValueOf
            | Native::TemporalInstantEquals
            | Native::TemporalInstantAdd
            | Native::TemporalInstantSubtract
            | Native::TemporalInstantRound
            | Native::TemporalInstantSince
            | Native::TemporalInstantUntil
            | Native::TemporalInstantToZonedDateTimeISO => {
                self.temporal_instant_native(p, native, this, args)
            }
            Native::TemporalNowInstant
            | Native::TemporalNowPlainDateISO
            | Native::TemporalNowPlainDateTimeISO
            | Native::TemporalNowPlainTimeISO
            | Native::TemporalNowTimeZoneId
            | Native::TemporalNowZonedDateTimeISO => self.temporal_now_native(p, native, args),
            Native::TemporalPlainMonthDayToString
            | Native::TemporalPlainMonthDayToJSON
            | Native::TemporalPlainMonthDayToLocaleString
            | Native::TemporalPlainYearMonthToString
            | Native::TemporalPlainYearMonthToJSON
            | Native::TemporalPlainYearMonthToLocaleString => {
                crate::temporal_date_projection::native(self, p, native, this, args)
            }
            _ => unreachable!("not an Intl or Temporal native: {native:?}"),
        }
    }

    /// The Intl and Temporal constructors the core's `construct_native_with_new_target` hands over.
    fn intl_temporal_constructor(&mut self, p: &ResidualProgram, native: Native, args: &[Value], new_target: Value) -> Result<Value, JsError> {
        match native {
            Native::IntlNumberFormat => self.intl_number_format_construct(p, args, new_target),
            Native::IntlCollator => self.intl_collator_construct(p, args, new_target),
            Native::IntlPluralRules => self.intl_plural_rules_construct(p, args, new_target),
            Native::IntlDateTimeFormat => self.intl_date_time_format_construct(p, args, new_target),
            Native::IntlDisplayNames => self.intl_display_names_construct(p, args, new_target),
            Native::IntlDurationFormat => self.intl_duration_format_construct(p, args, new_target),
            Native::IntlListFormat => self.intl_list_format_construct(p, args, new_target),
            Native::IntlLocale => self.intl_namespace_construct(p, native, args, new_target),
            Native::IntlRelativeTimeFormat => {
                self.intl_relative_time_format_construct(p, args, new_target)
            }
            Native::IntlSegmenter => self.intl_segmenter_construct(p, args, new_target),
            Native::TemporalDuration => self.temporal_duration_construct(p, args),
            Native::TemporalPlainTime => self.temporal_plain_time_construct(p, args),
            Native::TemporalPlainDate => self.temporal_plain_date_construct(p, args, new_target),
            Native::TemporalPlainDateTime => {
                self.temporal_plain_date_time_construct(p, args, new_target)
            }
            Native::TemporalPlainMonthDay | Native::TemporalPlainYearMonth => {
                self.temporal_calendar_projection_construct(p, native, args, new_target)
            }
            Native::TemporalZonedDateTime => {
                self.temporal_zoned_date_time_construct(p, args, new_target)
            }
            Native::TemporalInstant => self.temporal_instant_construct(p, args, new_target),
            _ => unreachable!("not an Intl or Temporal constructor: {native:?}"),
        }
    }
}

macro_rules! define {
    ($($symbol:ident: $name:ident = |$($arg:ident: $ty:ty),*| -> $ret:ty $body:block)*) => {
        $(
            #[unsafe(no_mangle)]
            pub fn $symbol($($arg: $ty),*) -> $ret $body
            const _: signatures::$name = $symbol;
        )*
    };
}

define! {
    quench_layer_intl_temporal_call: intl_temporal_call = |vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]| -> Result<Value, JsError> { vm.intl_temporal_native(p, native, this, args) }
    quench_layer_intl_temporal_construct: intl_temporal_construct = |vm: &mut Vm, p: &ResidualProgram, native: Native, args: &[Value], new_target: Value| -> Result<Value, JsError> { vm.intl_temporal_constructor(p, native, args, new_target) }
    quench_layer_intl_format_primitive: intl_format_primitive = |vm: &mut Vm, p: &ResidualProgram, value: Value, args: &[Value]| -> Result<Value, JsError> { vm.intl_format_primitive(p, value, args) }
    quench_layer_install_intl_for_realm: install_intl_for_realm = |vm: &mut Vm, p: &ResidualProgram, global: Value, object_prototype: Value| -> Result<(), JsError> { vm.install_intl_for_realm(p, global, object_prototype) }
    quench_layer_date_to_locale_string: date_to_locale_string = |vm: &mut Vm, p: &ResidualProgram, native: Native, this: Value, args: &[Value]| -> Result<Value, JsError> { vm.date_to_locale_string(p, native, this, args) }
    quench_layer_temporal_instant_constructor: temporal_instant_constructor = |vm: &mut Vm, p: &ResidualProgram| -> Result<Value, JsError> { vm.temporal_instant_constructor(p) }
    quench_layer_make_temporal_instant: make_temporal_instant = |vm: &mut Vm, p: &ResidualProgram, epoch: i128, new_target: Value| -> Result<Value, JsError> { vm.make_temporal_instant(p, epoch, new_target) }
    quench_layer_canonical_locale_list: canonical_locale_list = |vm: &mut Vm, p: &ResidualProgram, locales: Option<Value>| -> Result<Vec<String>, JsError> { vm.canonical_locale_list(p, locales) }
    quench_layer_locale_compare_collation: locale_compare_collation = |vm: &mut Vm, p: &ResidualProgram, locales: Value, options: Value| -> Result<std::rc::Rc<intl_core::Collation>, JsError> { vm.locale_compare_collation(p, locales, options) }
    quench_layer_intl_segment_iterator_next: intl_segment_iterator_next = |vm: &mut Vm, p: &ResidualProgram, iterator: Value| -> Result<Value, JsError> { vm.intl_segment_iterator_next(p, iterator) }
}
