use super::module::{ModuleEvaluationStack, ModuleOutcome, ModulePhase, ModuleRecord};
use super::*;
use crate::HostExecutionContext;
use std::collections::VecDeque;

const PROMISE_CAPABILITY_RESOLVE: &str = "\0quench:promise-capability-resolve";
const PROMISE_CAPABILITY_REJECT: &str = "\0quench:promise-capability-reject";
/// The module type of an import without a `type` attribute.
const JAVASCRIPT_MODULE_TYPE: &str = "javascript";
use crate::{ModuleSource, module_identity::ModuleIdentity};
use rustc_hash::FxHashSet;

fn module_cache_key(name: &str, module_type: &str) -> String {
    format!(
        "{}:{module_type}",
        crate::module_identity::normalized_text(name)
    )
}

fn self_import_referrer(referrer: &str, resolved: &str) -> bool {
    crate::module_identity::normalize(std::path::Path::new(referrer))
        == crate::module_identity::normalize(std::path::Path::new(resolved))
}

#[derive(Clone)]
enum StaticModuleValue {
    Constant {
        module: String,
        export: String,
        value: Constant,
    },
    Cached(Value),
    ModuleSource(Value),
    Binding {
        program: ProgramId,
        slot: u16,
        value: Value,
    },
    Namespace {
        name: String,
        exports: Vec<(String, StaticModuleValue)>,
    },
}

type ActiveModuleExports = FxHashMap<ModuleIdentity, Vec<(String, StaticModuleValue)>>;

type StaticModuleNodes = FxHashMap<ModuleIdentity, Rc<StaticModuleNode>>;
type StaticModuleResolveSet = FxHashSet<(ModuleIdentity, String)>;

/// A module's static exports: its own bindings and its re-exports. Each resolution step
/// looks a name up once, so resolving every export of a module is linear in its exports.
struct StaticModuleNode {
    locals: Vec<(String, StaticModuleValue)>,
    reexports: Vec<crate::compile::StaticModuleReexport>,
    /// Where each export name is defined: a local binding wins over a named or namespace
    /// re-export, and an earlier re-export over a later one. Star re-exports are unnamed.
    sites: FxHashMap<String, StaticExportSite>,
    /// The module each of its requests names, by specifier (`Vm::static_node_dependency`):
    /// resolving every export of a module walks the same edges once per name.
    dependencies: std::cell::RefCell<FxHashMap<String, String>>,
    /// GetExportedNames from this module with an empty export-star set, once known
    /// (`Vm::static_module_exported_name_set`); `None` inside when unsupported.
    exported_names: std::cell::OnceCell<Option<Rc<FxHashSet<String>>>>,
    /// Its star re-exports, indexed once (`Vm::static_star_index`); `None` inside when one
    /// of them does not resolve or is unsupported, and each lookup then walks them in order.
    stars: std::cell::OnceCell<Option<Rc<StaticStarIndex>>>,
}

/// A module's star re-exports: their modules in source order and, for each name one of them
/// exports, which of them do, so a name's ResolveExport visits only those.
#[derive(Default)]
struct StaticStarIndex {
    dependencies: Vec<ModuleSource>,
    exporters: FxHashMap<String, Vec<usize>>,
}

#[derive(Clone, Copy)]
enum StaticExportSite {
    Local(usize),
    Reexport(usize),
}

impl StaticModuleNode {
    fn new(
        locals: Vec<(String, StaticModuleValue)>,
        reexports: Vec<crate::compile::StaticModuleReexport>,
    ) -> Rc<Self> {
        let mut sites = FxHashMap::default();
        for (index, (name, _)) in locals.iter().enumerate() {
            sites
                .entry(name.clone())
                .or_insert(StaticExportSite::Local(index));
        }
        for (index, reexport) in reexports.iter().enumerate() {
            if let crate::compile::StaticModuleReexport::Named { exported, .. }
            | crate::compile::StaticModuleReexport::Namespace { exported, .. } = reexport
            {
                sites
                    .entry(exported.clone())
                    .or_insert(StaticExportSite::Reexport(index));
            }
        }
        Rc::new(Self {
            locals,
            reexports,
            sites,
            dependencies: Default::default(),
            exported_names: Default::default(),
            stars: Default::default(),
        })
    }
}

enum StaticModuleResolution {
    Binding(StaticModuleValue),
    Missing,
    Ambiguous,
    Unsupported,
}

enum StaticModuleGraph {
    Linked {
        name: String,
        exports: Vec<(String, StaticModuleValue)>,
        incomplete: bool,
    },
    Unsupported,
    LinkError,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StaticModuleAdd {
    Added,
    Missing,
    Conflict,
}

#[derive(Default)]
struct StaticModuleLinks {
    explicit: FxHashMap<String, StaticModuleValue>,
    stars: FxHashMap<String, StaticModuleValue>,
    ambiguous: FxHashSet<String>,
}

impl StaticModuleLinks {
    fn add_local(&mut self, name: String, value: StaticModuleValue) -> StaticModuleAdd {
        self.insert_explicit(name, value)
    }

    fn add_locals(&mut self, locals: Vec<(String, StaticModuleValue)>) -> StaticModuleAdd {
        for (name, value) in locals {
            if self.add_local(name, value) != StaticModuleAdd::Added {
                return StaticModuleAdd::Conflict;
            }
        }
        StaticModuleAdd::Added
    }

    fn add(
        &mut self,
        reexport: crate::compile::StaticModuleReexport,
        dependency: String,
        exports: Vec<(String, StaticModuleValue)>,
    ) -> StaticModuleAdd {
        match reexport {
            crate::compile::StaticModuleReexport::Named {
                imported, exported, ..
            } => {
                let Some(value) = exports
                    .into_iter()
                    .find_map(|(name, value)| (name == imported).then_some(value))
                else {
                    return StaticModuleAdd::Missing;
                };
                self.insert_explicit(exported, value)
            }
            crate::compile::StaticModuleReexport::Namespace { exported, .. } => self
                .insert_explicit(
                    exported,
                    StaticModuleValue::Namespace {
                        name: dependency,
                        exports,
                    },
                ),
            crate::compile::StaticModuleReexport::Star { .. } => {
                for (name, value) in exports {
                    if name == "default"
                        || self.explicit.contains_key(&name)
                        || self.ambiguous.contains(&name)
                    {
                        continue;
                    }
                    match self.stars.get(&name) {
                        Some(existing) if same_static_module_binding(existing, &value) => {}
                        Some(_) => {
                            self.stars.remove(&name);
                            self.ambiguous.insert(name);
                        }
                        None => {
                            self.stars.insert(name, value);
                        }
                    }
                }
                StaticModuleAdd::Added
            }
        }
    }

    fn insert_explicit(&mut self, name: String, value: StaticModuleValue) -> StaticModuleAdd {
        if self.explicit.insert(name, value).is_some() {
            StaticModuleAdd::Conflict
        } else {
            StaticModuleAdd::Added
        }
    }

    fn partial_exports(&self) -> Vec<(String, StaticModuleValue)> {
        let mut exports = self.explicit.clone();
        exports.extend(self.stars.clone());
        let mut exports = exports.into_iter().collect::<Vec<_>>();
        exports.sort_by(|left, right| left.0.encode_utf16().cmp(right.0.encode_utf16()));
        exports
    }

    fn finish(self, name: String, incomplete: bool) -> StaticModuleGraph {
        let exports = self.partial_exports();
        StaticModuleGraph::Linked {
            name,
            exports,
            incomplete,
        }
    }
}

fn same_static_module_binding(left: &StaticModuleValue, right: &StaticModuleValue) -> bool {
    match (left, right) {
        (
            StaticModuleValue::Binding {
                program: left_program,
                slot: left_slot,
                ..
            },
            StaticModuleValue::Binding {
                program: right_program,
                slot: right_slot,
                ..
            },
        ) => left_program == right_program && left_slot == right_slot,
        (
            StaticModuleValue::Constant {
                module: left_module,
                export: left_export,
                ..
            },
            StaticModuleValue::Constant {
                module: right_module,
                export: right_export,
                ..
            },
        ) => {
            left_export == right_export
                && crate::module_identity::normalize(std::path::Path::new(left_module))
                    == crate::module_identity::normalize(std::path::Path::new(right_module))
        }
        (
            StaticModuleValue::Namespace { name: left, .. },
            StaticModuleValue::Namespace { name: right, .. },
        ) => {
            crate::module_identity::normalize(std::path::Path::new(left))
                == crate::module_identity::normalize(std::path::Path::new(right))
        }
        (StaticModuleValue::Cached(left), StaticModuleValue::Cached(right)) => left == right,
        (StaticModuleValue::ModuleSource(left), StaticModuleValue::ModuleSource(right)) => {
            left == right
        }
        _ => false,
    }
}

fn native_length(kind: Native) -> Option<f64> {
    if let Some((_, _, array_native)) = super::typed_array_install::TYPED_ARRAY_CALLBACK_METHODS
        .iter()
        .find(|(_, typed_native, _)| *typed_native == kind)
    {
        return native_length(*array_native);
    }
    if let Some(length) = super::string::string_native_length(kind) {
        return Some(length);
    }
    if let Some(length) = super::date::date_native_length(kind) {
        return Some(length);
    }
    if let Some(length) = super::error::error_native_length(kind) {
        return Some(length);
    }
    if let Some(length) = super::finalization::finalization_native_length(kind) {
        return Some(length);
    }
    if let Some(length) = super::finalization::disposal_native_length(kind) {
        return Some(length);
    }
    if let Some(length) = super::wasm_js::wasm_js_native_length(kind) {
        return Some(length);
    }
    Some(match kind {
        Native::FunctionPrototype
        | Native::IsHTMLDDA
        | Native::CollectGarbage
        | Native::FunctionToString
        | Native::FunctionCaller
        | Native::TypedArray
        | Native::TypedArrayOf
        | Native::Uint8ArrayReverse
        | Native::Uint8ArrayToString
        | Native::Uint8ArrayKeys
        | Native::Uint8ArrayValues
        | Native::Uint8ArrayEntries
        | Native::Uint8ArrayToHex
        | Native::Uint8ArrayToBase64
        | Native::TypedArrayToStringTag
        | Native::TypedArrayBufferGetter
        | Native::TypedArrayByteLengthGetter
        | Native::TypedArrayByteOffsetGetter
        | Native::TypedArrayLengthGetter
        | Native::TypedArrayToReversed
        | Native::TypedArrayToLocaleString
        | Native::ThrowTypeError => 0.0,
        Native::TypedArrayAt | Native::TypedArrayToSorted => 1.0,
        Native::TypedArrayLastIndexOf => 1.0,
        Native::TypedArraySort => 1.0,
        Native::TypedArrayWith => 2.0,
        Native::Uint8ArrayFill
        | Native::Uint8ArrayIncludes
        | Native::Uint8ArrayIndexOf
        | Native::Uint8ArrayJoin
        | Native::Uint8ArraySet => 1.0,
        Native::Uint8ArrayCopyWithin | Native::Uint8ArraySubarray | Native::Uint8ArraySlice => 2.0,
        Native::Uint8ArrayFromBase64
        | Native::Uint8ArrayFromHex
        | Native::Uint8ArraySetFromBase64
        | Native::Uint8ArraySetFromHex => 1.0,
        Native::FunctionPrototypeHasInstance => 1.0,
        Native::FunctionCall | Native::FunctionBind => 1.0,
        Native::FunctionApply => 2.0,
        Native::Proxy => 2.0,
        Native::ProxyRevocable => 2.0,
        Native::ProxyRevoke => 0.0,
        Native::AbstractModuleSource => 0.0,
        Native::AbstractModuleSourceToStringTag => 0.0,
        Native::AggregateError => 2.0,
        Native::BigInt => 1.0,
        Native::Boolean => 1.0,
        Native::BooleanToString | Native::BooleanValueOf => 0.0,
        Native::DataView => 1.0,
        Native::EncodeUri
        | Native::EncodeUriComponent
        | Native::DecodeUri
        | Native::DecodeUriComponent
        | Native::GlobalEscape
        | Native::GlobalUnescape
        | Native::Eval => 1.0,
        Native::DataViewGetBigInt64
        | Native::DataViewGetBigUint64
        | Native::DataViewGetFloat16
        | Native::DataViewGetFloat32
        | Native::DataViewGetFloat64
        | Native::DataViewGetInt8
        | Native::DataViewGetInt16
        | Native::DataViewGetInt32
        | Native::DataViewGetUint8
        | Native::DataViewGetUint16
        | Native::DataViewGetUint32 => 1.0,
        Native::DataViewSetBigInt64
        | Native::DataViewSetBigUint64
        | Native::DataViewSetFloat16
        | Native::DataViewSetFloat32
        | Native::DataViewSetFloat64
        | Native::DataViewSetInt8
        | Native::DataViewSetInt16
        | Native::DataViewSetInt32
        | Native::DataViewSetUint8
        | Native::DataViewSetUint16
        | Native::DataViewSetUint32 => 2.0,
        Native::DataViewBufferGetter
        | Native::DataViewByteLengthGetter
        | Native::DataViewByteOffsetGetter => 0.0,
        Native::BigIntAsIntN | Native::BigIntAsUintN => 2.0,
        Native::BigIntValueOf | Native::BigIntToString | Native::BigIntToLocaleString => 0.0,
        Native::IntlNumberFormat | Native::IntlNumberFormatFormatGetter => 0.0,
        Native::IntlNumberFormatSupportedLocalesOf => 1.0,
        Native::IntlNumberFormatFormat | Native::IntlNumberFormatFormatToParts => 1.0,
        Native::IntlNumberFormatFormatRange | Native::IntlNumberFormatFormatRangeToParts => 2.0,
        Native::IntlNumberFormatResolvedOptions => 0.0,
        Native::IntlRelativeTimeFormat => 0.0,
        Native::IntlRelativeTimeFormatSupportedLocalesOf => 1.0,
        Native::IntlRelativeTimeFormatFormat | Native::IntlRelativeTimeFormatFormatToParts => 2.0,
        Native::IntlRelativeTimeFormatResolvedOptions => 0.0,
        Native::IntlSegmenter => 0.0,
        Native::IntlSegmenterSupportedLocalesOf => 1.0,
        Native::IntlSegmenterSegment => 1.0,
        Native::IntlSegmenterResolvedOptions => 0.0,
        Native::IntlSegmenterSegmentsIterator => 0.0,
        Native::IntlSegmenterSegmentsContaining => 1.0,
        Native::IntlSegmenterIteratorNext => 0.0,
        Native::IntlGetCanonicalLocales | Native::IntlSupportedValuesOf | Native::IntlLocale => 1.0,
        Native::IntlLocaleToString
        | Native::IntlLocaleMaximize
        | Native::IntlLocaleMinimize
        | Native::IntlLocaleGetCalendars
        | Native::IntlLocaleGetCollations
        | Native::IntlLocaleGetHourCycles
        | Native::IntlLocaleGetNumberingSystems
        | Native::IntlLocaleGetTimeZones
        | Native::IntlLocaleGetTextInfo
        | Native::IntlLocaleGetWeekInfo => 0.0,
        Native::IntlLocaleBaseNameGetter
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
        | Native::IntlLocaleNumericGetter => 0.0,
        Native::IntlCollator => 0.0,
        Native::IntlPluralRules => 0.0,
        Native::IntlPluralRulesSupportedLocalesOf => 1.0,
        Native::IntlPluralRulesSelect => 1.0,
        Native::IntlPluralRulesSelectRange => 2.0,
        Native::IntlPluralRulesResolvedOptions => 0.0,
        Native::IntlCollatorSupportedLocalesOf => 1.0,
        Native::IntlCollatorCompareGetter => 0.0,
        Native::IntlCollatorCompare => 2.0,
        Native::IntlCollatorResolvedOptions => 0.0,
        Native::IntlDateTimeFormat => 0.0,
        Native::IntlDateTimeFormatFormatGetter | Native::IntlDateTimeFormatResolvedOptions => 0.0,
        Native::IntlDateTimeFormatFormat => 1.0,
        Native::IntlDateTimeFormatFormatToParts => 1.0,
        Native::IntlDateTimeFormatFormatRange => 2.0,
        Native::IntlDateTimeFormatFormatRangeToParts => 2.0,
        Native::IntlDateTimeFormatSupportedLocalesOf => 1.0,
        Native::IntlDisplayNames => 2.0,
        Native::IntlDisplayNamesSupportedLocalesOf => 1.0,
        Native::IntlDisplayNamesOf => 1.0,
        Native::IntlDisplayNamesResolvedOptions => 0.0,
        Native::IntlDurationFormat => 0.0,
        Native::IntlDurationFormatFormatGetter | Native::IntlDurationFormatResolvedOptions => 0.0,
        Native::IntlDurationFormatFormat | Native::IntlDurationFormatFormatToParts => 1.0,
        Native::IntlDurationFormatSupportedLocalesOf => 1.0,
        Native::IntlListFormat => 0.0,
        Native::IntlListFormatFormatGetter => 0.0,
        Native::IntlListFormatResolvedOptions => 0.0,
        Native::IntlListFormatFormat | Native::IntlListFormatFormatToParts => 1.0,
        Native::IntlListFormatSupportedLocalesOf => 1.0,
        Native::Number => 1.0,
        Native::NumberValueOf | Native::NumberToLocaleString => 0.0,
        Native::NumberString
        | Native::NumberIsNaN
        | Native::NumberIsFinite
        | Native::NumberIsInteger
        | Native::NumberIsSafeInteger
        | Native::NumberParseFloat
        | Native::NumberFixed
        | Native::NumberExponential
        | Native::NumberPrecision => 1.0,
        Native::GlobalIsFinite | Native::GlobalIsNaN => 1.0,
        Native::ParseInt => 2.0,
        Native::SuppressedError => 3.0,
        Native::Error
        | Native::EvalError
        | Native::RangeError
        | Native::ReferenceError
        | Native::SyntaxError
        | Native::TypeError
        | Native::URIError => 1.0,
        Native::Promise => 1.0,
        Native::PromiseSpeciesGetter => 0.0,
        Native::PromiseResolve
        | Native::PromiseReject
        | Native::PromiseTry
        | Native::PromiseCatch
        | Native::PromiseFinally => 1.0,
        Native::PromiseThen => 2.0,
        Native::PromiseAll
        | Native::PromiseAllKeyed
        | Native::PromiseRace
        | Native::PromiseAllSettled
        | Native::PromiseAllSettledKeyed
        | Native::PromiseAny => 1.0,
        Native::PromiseFinallyHandler => 1.0,
        Native::PromiseFinallyContinuationHandler => 0.0,
        Native::PromiseAggregateJob => 1.0,
        Native::PromiseWithResolvers => 0.0,
        Native::PromiseCapabilityExecutor => 2.0,
        Native::Object => 1.0,
        Native::ShadowRealmImportValueFulfilled => 1.0,
        Native::String => 1.0,
        Native::TemporalDuration => 0.0,
        Native::TemporalDurationFrom => 1.0,
        Native::TemporalDurationCompare => 2.0,
        Native::TemporalDurationAdd | Native::TemporalDurationSubtract => 1.0,
        Native::TemporalDurationWith => 1.0,
        Native::TemporalDurationAbs | Native::TemporalDurationNegated => 0.0,
        Native::TemporalDurationTotal => 1.0,
        Native::TemporalDurationRound => 1.0,
        Native::TemporalToLocaleString | Native::TemporalDurationToString => 0.0,
        Native::TemporalDurationToJSON
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
        | Native::TemporalDurationBlankGetter => 0.0,
        Native::TemporalPlainDate => 3.0,
        Native::TemporalPlainDateFrom => 1.0,
        Native::TemporalPlainDateCompare => 2.0,
        Native::TemporalPlainDateEquals => 1.0,
        Native::TemporalPlainDateWith => 1.0,
        Native::TemporalPlainDateWithCalendar => 1.0,
        Native::TemporalPlainDateAdd | Native::TemporalPlainDateSubtract => 1.0,
        Native::TemporalPlainDateUntil | Native::TemporalPlainDateSince => 1.0,
        Native::TemporalPlainDateTime => 3.0,
        Native::TemporalPlainDateTimeFrom => 1.0,
        Native::TemporalPlainDateTimeCompare => 2.0,
        Native::TemporalPlainDateTimeAdd | Native::TemporalPlainDateTimeSubtract => 1.0,
        Native::TemporalPlainDateTimeRound => 1.0,
        Native::TemporalPlainDateTimeUntil | Native::TemporalPlainDateTimeSince => 1.0,
        Native::TemporalPlainDateTimeToString | Native::TemporalPlainDateTimeToPlainDate => 0.0,
        Native::TemporalPlainDateTimeToJSON
        | Native::TemporalPlainDateTimeToPlainTime
        | Native::TemporalPlainDateTimeValueOf => 0.0,
        Native::TemporalPlainDateTimeToZonedDateTime => 1.0,
        Native::TemporalPlainDateTimeWith => 1.0,
        Native::TemporalPlainDateTimeWithCalendar => 1.0,
        Native::TemporalPlainDateTimeWithPlainTime => 0.0,
        Native::TemporalPlainTime => 0.0,
        Native::TemporalPlainTimeFrom => 1.0,
        Native::TemporalPlainTimeCompare => 2.0,
        Native::TemporalPlainTimeAdd | Native::TemporalPlainTimeSubtract => 1.0,
        Native::TemporalPlainTimeEquals => 1.0,
        Native::TemporalPlainTimeHourGetter
        | Native::TemporalPlainTimeMinuteGetter
        | Native::TemporalPlainTimeSecondGetter
        | Native::TemporalPlainTimeMillisecondGetter
        | Native::TemporalPlainTimeMicrosecondGetter
        | Native::TemporalPlainTimeNanosecondGetter => 0.0,
        Native::TemporalPlainTimeValueOf => 0.0,
        Native::TemporalPlainTimeRound => 1.0,
        Native::TemporalPlainTimeUntil | Native::TemporalPlainTimeSince => 1.0,
        Native::TemporalPlainTimeToString | Native::TemporalPlainTimeToJSON => 0.0,
        Native::TemporalPlainTimeWith => 1.0,
        Native::TemporalNowInstant
        | Native::TemporalNowPlainDateISO
        | Native::TemporalNowPlainDateTimeISO
        | Native::TemporalNowPlainTimeISO
        | Native::TemporalNowTimeZoneId
        | Native::TemporalNowZonedDateTimeISO => 0.0,
        Native::TemporalZonedDateTimeToInstant
        | Native::TemporalZonedDateTimeToPlainDate
        | Native::TemporalZonedDateTimeToPlainDateTime
        | Native::TemporalZonedDateTimeToPlainTime
        | Native::TemporalZonedDateTimeValueOf => 0.0,
        Native::TemporalZonedDateTimeAdd
        | Native::TemporalZonedDateTimeSubtract
        | Native::TemporalZonedDateTimeGetTimeZoneTransition
        | Native::TemporalZonedDateTimeRound
        | Native::TemporalZonedDateTimeUntil
        | Native::TemporalZonedDateTimeSince => 1.0,
        Native::TemporalZonedDateTimeStartOfDay => 0.0,
        Native::TemporalPlainMonthDay | Native::TemporalPlainYearMonth => 2.0,
        Native::TemporalPlainMonthDayFrom | Native::TemporalPlainYearMonthFrom => 1.0,
        Native::TemporalPlainMonthDayCompare | Native::TemporalPlainYearMonthCompare => 2.0,
        Native::TemporalPlainMonthDayEquals | Native::TemporalPlainMonthDayToPlainDate => 1.0,
        Native::TemporalPlainMonthDayWith => 1.0,
        Native::TemporalPlainYearMonthEquals
        | Native::TemporalPlainYearMonthAdd
        | Native::TemporalPlainYearMonthSubtract
        | Native::TemporalPlainYearMonthUntil
        | Native::TemporalPlainYearMonthSince
        | Native::TemporalPlainYearMonthWith
        | Native::TemporalPlainYearMonthToPlainDate => 1.0,
        Native::TemporalPlainMonthDayCalendarIdGetter
        | Native::TemporalPlainMonthDayDayGetter
        | Native::TemporalPlainMonthDayMonthCodeGetter
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
        | Native::TemporalPlainMonthDayToString
        | Native::TemporalPlainMonthDayToJSON
        | Native::TemporalPlainMonthDayValueOf
        | Native::TemporalPlainYearMonthToString
        | Native::TemporalPlainYearMonthToJSON
        | Native::TemporalPlainYearMonthValueOf => 0.0,
        Native::TemporalPlainMonthDayToLocaleString
        | Native::TemporalPlainYearMonthToLocaleString => 0.0,
        Native::TemporalPlainDateTimeCalendarIdGetter
        | Native::TemporalPlainDateTimeYearGetter
        | Native::TemporalPlainDateTimeMonthGetter
        | Native::TemporalPlainDateTimeMonthCodeGetter
        | Native::TemporalPlainDateTimeDayGetter
        | Native::TemporalPlainDateTimeHourGetter
        | Native::TemporalPlainDateTimeMinuteGetter
        | Native::TemporalPlainDateTimeSecondGetter
        | Native::TemporalPlainDateTimeMillisecondGetter
        | Native::TemporalPlainDateTimeMicrosecondGetter
        | Native::TemporalPlainDateTimeNanosecondGetter
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
        | Native::TemporalPlainDateTimeInLeapYearGetter => 0.0,
        Native::TemporalPlainDateTimeEquals => 1.0,
        Native::TemporalPlainDateCalendarIdGetter
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
        | Native::TemporalPlainDateValueOf => 0.0,
        Native::TemporalPlainDateToZonedDateTime => 1.0,
        Native::TemporalZonedDateTime => 2.0,
        Native::TemporalZonedDateTimeFrom => 1.0,
        Native::TemporalZonedDateTimeCompare => 2.0,
        Native::TemporalZonedDateTimeEquals
        | Native::TemporalZonedDateTimeWithTimeZone
        | Native::TemporalZonedDateTimeWith
        | Native::TemporalZonedDateTimeWithCalendar => 1.0,
        Native::TemporalZonedDateTimeWithPlainTime => 0.0,
        Native::TemporalZonedDateTimeEpochNanosecondsGetter
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
        | Native::TemporalZonedDateTimeOffsetNanosecondsGetter
        | Native::TemporalZonedDateTimeToString
        | Native::TemporalZonedDateTimeToJSON
        | Native::TemporalZonedDateTimeToLocaleString => 0.0,
        Native::TemporalInstant => 1.0,
        Native::TemporalInstantFrom
        | Native::TemporalInstantFromEpochMilliseconds
        | Native::TemporalInstantFromEpochNanoseconds => 1.0,
        Native::TemporalInstantCompare => 2.0,
        Native::TemporalInstantEpochNanosecondsGetter
        | Native::TemporalInstantEpochMillisecondsGetter
        | Native::TemporalInstantToString
        | Native::TemporalInstantToJSON
        | Native::TemporalInstantValueOf => 0.0,
        Native::TemporalInstantEquals
        | Native::TemporalInstantAdd
        | Native::TemporalInstantSubtract
        | Native::TemporalInstantRound
        | Native::TemporalInstantSince
        | Native::TemporalInstantUntil => 1.0,
        Native::TemporalInstantToZonedDateTimeISO => 1.0,
        Native::Symbol => 0.0,
        Native::SymbolFor | Native::SymbolKeyFor => 1.0,
        Native::SymbolToString | Native::SymbolValueOf => 0.0,
        Native::SymbolDescriptionGetter => 0.0,
        Native::SymbolToPrimitive => 1.0,
        Native::StringToLocaleLowerCase | Native::StringToLocaleUpperCase => 0.0,
        Native::StringValues => 0.0,
        Native::StringLocaleCompare => 1.0,
        Native::StringFromCharCode | Native::StringFromCodePoint | Native::StringRaw => 1.0,
        Native::RegExp => 2.0,
        Native::RegExpCompile => 2.0,
        Native::RegExpEscape => 1.0,
        Native::RegExpLegacyGetter => 0.0,
        Native::RegExpLegacySetter => 1.0,
        Native::ObjectPrototypeToLocaleString | Native::ObjectPrototypeValueOf => 0.0,
        Native::ObjectPrototypeToString => 0.0,
        Native::ObjectPrototypeDefineGetter | Native::ObjectPrototypeDefineSetter => 2.0,
        Native::ObjectPrototypeProtoGetter => 0.0,
        Native::ObjectPrototypeProtoSetter => 1.0,
        Native::ObjectPrototypeLookupGetter
        | Native::ObjectPrototypeLookupSetter
        | Native::ObjectPrototypeHasOwnProperty
        | Native::ObjectPrototypePropertyIsEnumerable
        | Native::ObjectPrototypeIsPrototypeOf => 1.0,
        Native::Map
        | Native::Set
        | Native::WeakMap
        | Native::WeakSet
        | Native::WeakRefDeref
        | Native::MapSizeGetter
        | Native::SetSizeGetter => 0.0,
        Native::MapGet | Native::MapHas | Native::MapDelete | Native::MapForEach => 1.0,
        Native::MapSet => 2.0,
        Native::MapClear | Native::MapKeys | Native::MapValues | Native::MapEntries => 0.0,
        Native::MapGroupBy => 2.0,
        Native::MapGetOrInsert | Native::MapGetOrInsertComputed => 2.0,
        Native::WeakMapGet | Native::WeakMapHas | Native::WeakMapDelete => 1.0,
        Native::WeakSetAdd | Native::WeakSetHas | Native::WeakSetDelete => 1.0,
        Native::WeakMapSet | Native::WeakMapGetOrInsert | Native::WeakMapGetOrInsertComputed => 2.0,
        Native::WeakRef => 1.0,
        Native::MathMax
        | Native::MathMin
        | Native::MathPow
        | Native::MathAtan2
        | Native::MathHypot
        | Native::MathImul => 2.0,
        Native::MathRandom => 0.0,
        Native::MathAbs
        | Native::MathAcos
        | Native::MathAcosh
        | Native::MathAsin
        | Native::MathAsinh
        | Native::MathAtan
        | Native::MathAtanh
        | Native::MathCbrt
        | Native::MathCeil
        | Native::MathClz32
        | Native::MathCos
        | Native::MathCosh
        | Native::MathExp
        | Native::MathExpm1
        | Native::MathF16Round
        | Native::MathFloor
        | Native::MathFround
        | Native::MathLog
        | Native::MathLog10
        | Native::MathLog1p
        | Native::MathLog2
        | Native::MathRound
        | Native::MathSign
        | Native::MathSin
        | Native::MathSinh
        | Native::MathSqrt
        | Native::MathSumPrecise
        | Native::MathTan
        | Native::MathTanh
        | Native::MathTrunc => 1.0,
        Native::SetAdd
        | Native::SetHas
        | Native::SetDelete
        | Native::SetForEach
        | Native::SetDifference
        | Native::SetIntersection
        | Native::SetSymmetricDifference
        | Native::SetUnion
        | Native::SetIsDisjointFrom
        | Native::SetIsSubsetOf
        | Native::SetIsSupersetOf => 1.0,
        Native::SetSpeciesGetter => 0.0,
        Native::SetClear | Native::SetKeys | Native::SetValues | Native::SetEntries => 0.0,
        Native::ArrayFrom | Native::ArrayFromAsync | Native::ArrayIsArray => 1.0,
        Native::TypedArrayFrom => 1.0,
        Native::Uint8Array
        | Native::Uint8ClampedArray
        | Native::Uint16Array
        | Native::Uint32Array
        | Native::Int8Array
        | Native::Int16Array
        | Native::Int32Array
        | Native::BigInt64Array
        | Native::BigUint64Array
        | Native::Float16Array
        | Native::Float32Array
        | Native::Float64Array => 3.0,
        Native::ArrayBuffer
        | Native::SharedArrayBuffer
        | Native::ArrayBufferIsView
        | Native::DetachArrayBuffer => 1.0,
        Native::Iterator => 0.0,
        Native::IteratorFrom => 1.0,
        Native::IteratorConcat => 0.0,
        Native::IteratorZip | Native::IteratorZipKeyed => 1.0,
        Native::IteratorMap
        | Native::IteratorFilter
        | Native::IteratorTake
        | Native::IteratorDrop
        | Native::IteratorFlatMap
        | Native::IteratorReduce
        | Native::IteratorForEach
        | Native::IteratorEvery
        | Native::IteratorFind
        | Native::IteratorSome => 1.0,
        Native::IteratorToArray => 0.0,
        Native::IteratorDispose | Native::IteratorProtocolNext | Native::IteratorProtocolReturn => {
            0.0
        }
        Native::IteratorHelperNext | Native::IteratorHelperReturn => 0.0,
        Native::IteratorPrototypeConstructorGetter | Native::IteratorPrototypeToStringTagGetter => {
            0.0
        }
        Native::IteratorPrototypeConstructorSetter | Native::IteratorPrototypeToStringTagSetter => {
            1.0
        }
        Native::ArrayIteratorNext
        | Native::IteratorNext
        | Native::StringIteratorNext
        | Native::RegExpStringIteratorNext
        | Native::IteratorSelf
        | Native::AsyncIteratorSelf
        | Native::AsyncIteratorDispose => 0.0,
        Native::GeneratorNext
        | Native::GeneratorReturn
        | Native::GeneratorThrow
        | Native::AsyncGeneratorNext
        | Native::AsyncGeneratorReturn
        | Native::AsyncGeneratorThrow => 1.0,
        Native::AtomicsAdd
        | Native::AtomicsAnd
        | Native::AtomicsOr
        | Native::AtomicsSub
        | Native::AtomicsXor
        | Native::AtomicsExchange
        | Native::AtomicsNotify => 3.0,
        Native::AtomicsCompareExchange => 4.0,
        Native::AtomicsIsLockFree => 1.0,
        Native::AtomicsLoad => 2.0,
        Native::AtomicsStore => 3.0,
        Native::AtomicsWait | Native::AtomicsWaitAsync => 4.0,
        Native::AtomicsPause => 0.0,
        Native::AsyncDisposableStack
        | Native::AsyncDisposableStackMove
        | Native::AsyncDisposableStackDisposeAsync
        | Native::AsyncDisposableStackDisposed => 0.0,
        Native::AsyncDisposableStackUse | Native::AsyncDisposableStackDefer => 1.0,
        Native::AsyncDisposableStackAdopt => 2.0,
        Native::AsyncFunction | Native::GeneratorFunction | Native::AsyncGeneratorFunction => 1.0,
        Native::ArrayBufferSlice
        | Native::SharedArrayBufferSlice
        | Native::ArrayBufferSliceToImmutable => 2.0,
        Native::SharedArrayBufferGrow => 1.0,
        Native::ArrayBufferResize => 1.0,
        Native::ArrayBufferTransfer
        | Native::ArrayBufferTransferToFixedLength
        | Native::ArrayBufferTransferToImmutable
        | Native::ArrayBufferByteLengthGetter
        | Native::ArrayBufferDetachedGetter
        | Native::ArrayBufferImmutableGetter
        | Native::ArrayBufferMaxByteLengthGetter
        | Native::ArrayBufferResizableGetter
        | Native::SharedArrayBufferByteLengthGetter
        | Native::SharedArrayBufferGrowableGetter
        | Native::SharedArrayBufferMaxByteLengthGetter
        | Native::ArrayBufferSpecies => 0.0,
        Native::ArraySpecies => 0.0,
        Native::ArrayOf
        | Native::ArrayPop
        | Native::ArrayShift
        | Native::ArrayFlat
        | Native::ArrayReverse
        | Native::ArrayToReversed
        | Native::ArrayToString
        | Native::ArrayToLocaleString
        | Native::ArrayKeys
        | Native::ArrayValues
        | Native::ArrayEntries => 0.0,
        Native::ArrayJoin
        | Native::ArrayConcat
        | Native::ArrayMap
        | Native::ArrayFilter
        | Native::ArraySome
        | Native::ArrayEvery
        | Native::ArrayFind
        | Native::ArrayFindIndex
        | Native::ArrayFindLast
        | Native::ArrayFindLastIndex
        | Native::ArrayGroup
        | Native::ArrayGroupToMap
        | Native::ArrayIncludes
        | Native::ArrayIndexOf
        | Native::ArrayLastIndexOf
        | Native::ArrayFlatMap
        | Native::ArrayAt
        | Native::ArraySort
        | Native::ArrayForEach
        | Native::ArrayReduce
        | Native::ArrayReduceRight
        | Native::ArrayPush
        | Native::ArrayUnshift
        | Native::ArrayFill
        | Native::ArrayToSorted => 1.0,
        Native::ArrayCopyWithin
        | Native::ArrayToSpliced
        | Native::ArrayWith
        | Native::ArraySlice
        | Native::ArraySplice => 2.0,
        Native::ObjectKeys
        | Native::ObjectValues
        | Native::ObjectEntries
        | Native::ObjectGetOwnPropertyNames
        | Native::ObjectGetOwnPropertySymbols
        | Native::ObjectGetOwnPropertyDescriptors
        | Native::ObjectFreeze
        | Native::ObjectSeal
        | Native::ObjectPreventExtensions
        | Native::ObjectIsFrozen
        | Native::ObjectIsSealed
        | Native::ObjectIsExtensible
        | Native::ObjectGetPrototypeOf => 1.0,
        Native::ObjectHasOwn | Native::ForInKeyIsEnumerable => 2.0,
        Native::ObjectGetOwnPropertyDescriptor | Native::ObjectIs => 2.0,
        Native::ObjectCreate => 2.0,
        Native::ObjectDefineProperties | Native::ObjectAssign => 2.0,
        Native::ObjectFromEntries => 1.0,
        Native::ObjectDefineProperty => 3.0,
        Native::ObjectGroupBy => 2.0,
        Native::ObjectSetPrototypeOf => 2.0,
        Native::JsonParse => 2.0,
        Native::JsonStringify => 3.0,
        Native::JsonRawJson | Native::JsonIsRawJson => 1.0,
        Native::RegExpSymbolMatch => 1.0,
        Native::RegExpSymbolSearch => 1.0,
        Native::RegExpSymbolReplace => 2.0,
        Native::RegExpSymbolMatchAll | Native::StringMatchAll => 1.0,
        Native::RegExpSymbolSplit => 2.0,
        Native::RegExpExec | Native::RegExpTest => 1.0,
        Native::RegExpToString => 0.0,
        Native::RegExpFlags
        | Native::RegExpGlobal
        | Native::RegExpIgnoreCase
        | Native::RegExpMultiline
        | Native::RegExpDotAll
        | Native::RegExpUnicode
        | Native::RegExpUnicodeSets
        | Native::RegExpSticky
        | Native::RegExpHasIndices
        | Native::RegExpSource => 0.0,
        Native::RegExpSpecies => 0.0,
        Native::ReflectHas | Native::ReflectDeleteProperty | Native::ReflectSetPrototypeOf => 2.0,
        Native::ReflectApply => 3.0,
        Native::ReflectDefineProperty => 3.0,
        Native::ReflectConstruct => 2.0,
        Native::ReflectGet => 2.0,
        Native::ReflectSet => 3.0,
        Native::ReflectGetOwnPropertyDescriptor => 2.0,
        Native::ReflectGetPrototypeOf
        | Native::ReflectIsExtensible
        | Native::ReflectOwnKeys
        | Native::ReflectPreventExtensions => 1.0,
        _ => return None,
    })
}

pub(super) use crate::heap::{PromiseReaction, PromiseRecord, PromiseRejectionState, PromiseState};

#[derive(Clone, Copy, Debug)]
pub(super) enum RejectionNotification {
    Unhandled(Value),
    Handled(Value),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PromiseJob {
    /// The realm current when the reaction was queued, where the job runs.
    pub(super) realm: Value,
    pub(super) handler: Value,
    pub(super) next: Value,
    pub(super) rejected: bool,
    pub(super) value: Value,
    pub(super) execution_context: Option<HostExecutionContext>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ThenableJob {
    /// The realm the job runs in: the one current when the thenable resolved its promise.
    pub(super) realm: Value,
    pub(super) then: Value,
    pub(super) thenable: Value,
    pub(super) promise: Value,
    pub(super) execution_context: Option<HostExecutionContext>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FinallyHandlerCallback {
    pub(super) handler: Value,
    pub(super) constructor: Value,
    pub(super) original_rejected: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FinallyContinuationCallback {
    pub(super) original_rejected: bool,
    pub(super) original: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AggregateMode {
    All,
    AllKeyed,
    Race,
    AllSettled,
    AllSettledKeyed,
    Any,
}

impl AggregateMode {
    pub(super) const fn is_all(self) -> bool {
        matches!(self, Self::All | Self::AllKeyed)
    }

    pub(super) const fn is_all_settled(self) -> bool {
        matches!(self, Self::AllSettled | Self::AllSettledKeyed)
    }

    pub(super) const fn is_keyed(self) -> bool {
        matches!(self, Self::AllKeyed | Self::AllSettledKeyed)
    }
}

#[derive(Clone, Debug)]
pub(super) struct AggregateRecord {
    pub(super) mode: AggregateMode,
    pub(super) output: Value,
    pub(super) resolve: Value,
    pub(super) reject: Value,
    pub(super) remaining: usize,
    pub(super) values: Vec<Value>,
    pub(super) called: Vec<bool>,
    pub(super) keys: Option<Vec<Value>>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct AggregateJob {
    pub(super) aggregate: Value,
    pub(super) index: usize,
    pub(super) rejected: bool,
}

#[derive(Clone, Debug)]
pub(super) struct DynamicImportJob {
    pub(super) cache_key: String,
    pub(super) module: ModuleSource,
    pub(super) promise: Value,
}

#[derive(Default)]
pub(super) struct PromiseRuntime {
    pub(super) rejection_notifications: VecDeque<RejectionNotification>,
    pub(super) last_rejection_id: u64,
    pub(super) finally_handler_callbacks: FxHashMap<Value, FinallyHandlerCallback>,
    pub(super) finally_continuation_callbacks: FxHashMap<Value, FinallyContinuationCallback>,
    pub(super) aggregates: FxHashMap<Value, AggregateRecord>,
    pub(super) aggregate_jobs: FxHashMap<Value, AggregateJob>,
    pub(super) reaction_capabilities: FxHashMap<Value, (Value, Value)>,
    pub(super) modules: FxHashMap<String, ModuleRecord>,
    pub(super) dynamic_import_jobs: Vec<DynamicImportJob>,
    pub(super) async_module_order: VecDeque<String>,
    /// A promise an async-evaluating module waits on has settled since module advancement
    /// last completed modules.
    pub(super) async_module_settled: bool,
    pub(super) module_sources: FxHashMap<ModuleIdentity, Value>,
    pub(super) waiting_static_modules: Vec<ModuleSource>,
    pub(super) active_native: Vec<super::activation::NativeActivation>,
}

impl PromiseRuntime {
    /// Visits each `(owner, edge)` the promise job and combinator tables hold outside their
    /// owner's cell; a promise's own state is traced with its `Cell::Promise`. Plain loops per
    /// table: a chained iterator costs every edge a step through each nesting level.
    pub(super) fn for_each_owned_edge(&self, mut visit: impl FnMut(Value, Value)) {
        macro_rules! edges {
            ($table:expr, |$record:ident| $roots:expr) => {
                for (owner, $record) in &$table {
                    for root in $roots {
                        visit(*owner, root);
                    }
                }
            };
        }
        edges!(self.finally_handler_callbacks, |callback| [
            callback.handler,
            callback.constructor
        ]);
        edges!(self.finally_continuation_callbacks, |callback| [
            callback.original
        ]);
        edges!(self.aggregates, |record| {
            [record.output, record.resolve, record.reject]
                .into_iter()
                .chain(record.values.iter().copied())
                .chain(record.keys.iter().flatten().copied())
        });
        edges!(self.aggregate_jobs, |job| [job.aggregate]);
        edges!(self.reaction_capabilities, |capability| [
            capability.0,
            capability.1
        ]);
    }
}

/// Names a static module's parse goal in its compile-cache key, apart from an application
/// entry's (`Runtime::compile_entry`), whose cached form differs.
const STATIC_MODULE_GOAL: &str = "static-module";
/// The compile-cache part naming a static module's analysis (`Vm::static_module_analysis`).
const STATIC_MODULE_ANALYSIS_GOAL: &str = "static-module-analysis";

impl<H: Host> Vm<H> {
    pub(super) fn abstract_module_source_to_string_tag(&mut self, receiver: Value) -> Value {
        if !self.is_object_like(receiver)
            || !self
                .realm
                .promise
                .module_sources
                .values()
                .any(|module_source| *module_source == receiver)
        {
            return Value::UNDEFINED;
        }
        self.heap.alloc_string("ModuleSource".into())
    }

    pub(super) fn call_native_guarded(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
        callee: Value,
    ) -> Result<Value, JsError> {
        // A realm's own natives run in that realm; a native shared by every realm runs in
        // its caller's, which is the realm whose objects it builds.
        let realm = match self.heap.function(callee) {
            Some(FunctionRecord { realm, .. })
                if *realm == self.realm.globals || !self.is_shared_native(callee) =>
            {
                *realm
            }
            _ => self.realm.globals,
        };
        let previous_global = self.switch_realm_global(realm);
        let boundary = match native {
            Native::FunctionCall
            | Native::FunctionApply
            | Native::FunctionBoundCall
            | Native::ReflectApply
            | Native::ReflectConstruct => super::activation::NativeCallBoundary::Forward,
            Native::Eval if self.direct_eval => super::activation::NativeCallBoundary::Forward,
            _ => super::activation::NativeCallBoundary::Opaque,
        };
        self.realm
            .promise
            .active_native
            .push(super::activation::NativeActivation {
                callable: callee,
                frame_depth: self.frames.len(),
                boundary,
            });
        let result = (|| {
            let _stack = self.enter_stack()?;
            self.call_native(p, native, this, args)
        })();
        self.realm.promise.active_native.pop();
        self.switch_realm_global(previous_global);
        result
    }

    /// `call_value(method, receiver, args)` of the protocol method a String method found
    /// (`@@match`, `@@replace`, `@@search` or `@@split`), whose receiver and arguments the
    /// caller roots. A RegExp builtin of the running realm runs as `call_native_guarded`
    /// runs it, under its native activation and stack guard, but straight into its body:
    /// its receiver needs no String conversion and the general call's dispatch decides
    /// nothing more for it. Any other method takes the general call.
    pub(super) fn call_protocol_method(
        &mut self,
        p: &ResidualProgram,
        method: Value,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let native = match self.heap.function(method) {
            Some(FunctionRecord {
                kind: FunctionKind::Native(native),
                realm,
                ..
            }) if *realm == self.realm.globals
                && matches!(
                    native,
                    Native::RegExpSymbolMatch
                        | Native::RegExpSymbolReplace
                        | Native::RegExpSymbolSearch
                        | Native::RegExpSymbolSplit
                ) =>
            {
                *native
            }
            _ => return self.call_value(p, method, receiver, args),
        };
        self.profile.call_target(0, args.len());
        self.realm
            .promise
            .active_native
            .push(super::activation::NativeActivation {
                callable: method,
                frame_depth: self.frames.len(),
                boundary: super::activation::NativeCallBoundary::Opaque,
            });
        let result = (|| {
            let _stack = self.enter_stack()?;
            self.check_execution_interrupt()?;
            self.with_call_roots([method], |vm| match native {
                Native::RegExpSymbolMatch => vm.regexp_symbol_match(p, receiver, args),
                Native::RegExpSymbolReplace => vm.regexp_symbol_replace(p, receiver, args),
                Native::RegExpSymbolSearch => vm.regexp_symbol_search(p, receiver, args),
                _ => vm.regexp_symbol_split(p, receiver, args),
            })
        })();
        self.realm.promise.active_native.pop();
        // String natives make text on demand; charge it before the caller's next safepoint.
        self.heap.charge_materialized_text();
        result
    }

    pub(super) fn native_with_env(&mut self, kind: Native, env: Value) -> Value {
        self.native_with_realm(kind, env, self.realm.globals)
    }

    pub(super) fn native_with_realm(&mut self, kind: Native, env: Value, realm: Value) -> Value {
        let function = self.heap.alloc_function(FunctionRecord {
            object: Self::empty_object(self.realm_function_prototype(realm)),
            realm,
            kind: FunctionKind::Native(kind),
            env,
        });
        let promise_resolver = !env.is_null()
            && matches!(
                kind,
                Native::PromiseResolve | Native::PromiseReject | Native::PromiseAggregateJob
            );
        let anonymous_capability_executor =
            !env.is_null() && kind == Native::PromiseCapabilityExecutor;
        let anonymous_proxy_revoke = !env.is_null() && kind == Native::ProxyRevoke;
        let length = promise_resolver
            .then_some(1.0)
            .or_else(|| native_length(kind));
        if let Some(length) = length {
            let atom = self.length_atom;
            let _ = self.set_property_with_attributes(
                function,
                atom,
                Value::number(length),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        if promise_resolver || anonymous_capability_executor || anonymous_proxy_revoke {
            let atom = self.intern_atom("name");
            let empty_name = self.heap.alloc_string("".into());
            let _ = self.set_property_with_attributes(
                function,
                atom,
                empty_name,
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        function
    }

    pub(super) fn install_promise(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let prototype = self.object();
        let native_promise = self.native_value(Native::Promise);
        self.install_promise_for_realm(program, native_promise, prototype, None)
    }

    pub(super) fn install_promise_for_realm(
        &mut self,
        program: &ResidualProgram,
        promise: Value,
        prototype: Value,
        method_realm: Option<Value>,
    ) -> Result<(), JsError> {
        let global = method_realm.unwrap_or(self.realm.globals);
        self.realm
            .intrinsics
            .promise_constructors
            .insert(global, promise);
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((global, Native::Promise), prototype);
        self.set_builtin_function_name(promise, "Promise")?;
        self.set_builtin_value_named(promise, "prototype", prototype)?;
        let prototype_atom = self.prototype_atom();
        self.set_property_attributes(
            promise,
            property_key::PropertyKey::string(prototype_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_value_named(prototype, "constructor", promise)?;
        for (name, native) in [
            ("then", Native::PromiseThen),
            ("catch", Native::PromiseCatch),
            ("finally", Native::PromiseFinally),
        ] {
            self.set_realm_builtin_named(program, prototype, name, native, method_realm)?;
        }
        self.install_builtin_to_string_tag(prototype, "Promise")?;
        for (name, native) in [
            ("resolve", Native::PromiseResolve),
            ("reject", Native::PromiseReject),
            ("withResolvers", Native::PromiseWithResolvers),
            ("all", Native::PromiseAll),
            ("allKeyed", Native::PromiseAllKeyed),
            ("race", Native::PromiseRace),
            ("allSettled", Native::PromiseAllSettled),
            ("allSettledKeyed", Native::PromiseAllSettledKeyed),
            ("any", Native::PromiseAny),
            ("try", Native::PromiseTry),
        ] {
            self.set_realm_builtin_named(program, promise, name, native, method_realm)?;
        }
        if let Some(species) = self.well_known_symbols.get(WellKnownSymbol::Species) {
            let getter = self.realm_native_value(
                Native::PromiseSpeciesGetter,
                global,
                method_realm.is_none(),
            );
            self.set_builtin_function_name(getter, "get [Symbol.species]")?;
            self.set_symbol_property(promise, species, Value::UNDEFINED)?;
            self.set_property_attributes(
                promise,
                property_key::PropertyKey::symbol(species),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: true,
                    getter: Some(getter),
                    setter: None,
                },
            );
        }
        self.set_builtin_value_named(global, "Promise", promise)
    }

    pub(super) fn promise_object(&mut self) -> Result<Value, JsError> {
        self.promise_object_with_parent(None)
    }

    pub(super) fn promise_object_with_parent(
        &mut self,
        parent: Option<HostExecutionContext>,
    ) -> Result<Value, JsError> {
        let prototype = self
            .realm
            .intrinsics
            .builtin_prototypes
            .get(&(self.realm.globals, Native::Promise))
            .copied()
            .unwrap_or(Value::NULL);
        let host_context = if self.host.observes_promise_lifecycle() {
            self.host.create_promise_context(parent)
        } else {
            None
        };
        let promise = self.heap.alloc_promise(crate::heap::PromiseCell {
            object: Self::empty_object(prototype),
            record: PromiseRecord {
                state: PromiseState::Pending,
                result: Value::UNDEFINED,
                reactions: Default::default(),
                rejection: PromiseRejectionState::Unobserved,
                rejection_id: None,
                host_context: host_context.into(),
                module_evaluation: false,
            },
        });
        if let Some(identity) = host_context {
            let mut context = crate::NativeContext::new(self);
            let resource = context.scoped_value(promise);
            H::initialize_promise(&mut context, resource, identity)
                .map_err(|error| context.finish_error(error))?;
        }
        Ok(promise)
    }

    pub(super) fn construct_promise(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let executor = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(executor) {
            return Err(self.type_error(p, "Promise resolver is not a function".into()));
        }
        let promise = self.promise_object()?;
        let (resolve, reject) = self.promise_resolving_functions(promise);
        if let Err(error) = self.call_value(p, executor, Value::UNDEFINED, &[resolve, reject]) {
            let reason = error.thrown_value().unwrap_or(Value::UNDEFINED);
            self.call_value(p, reject, Value::UNDEFINED, &[reason])?;
        }
        Ok(promise)
    }

    pub(super) fn promise_resolving_functions(&mut self, promise: Value) -> (Value, Value) {
        let state = self.heap.alloc(Cell::PromiseResolvingState {
            promise,
            already_resolved: false,
        });
        (
            self.native_with_env(Native::PromiseResolve, state),
            self.native_with_env(Native::PromiseReject, state),
        )
    }

    fn take_promise_resolving_promise(
        &mut self,
        p: &ResidualProgram,
        state: Value,
    ) -> Result<Option<Value>, JsError> {
        let Some(Cell::PromiseResolvingState {
            promise,
            already_resolved,
        }) = self.heap.get_mut(state)
        else {
            return Err(self.type_error(p, "invalid Promise resolver state".into()));
        };
        if *already_resolved {
            return Ok(None);
        }
        *already_resolved = true;
        Ok(Some(*promise))
    }

    fn promise_with_resolvers(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
    ) -> Result<Value, JsError> {
        let (promise, resolve, reject) = self.new_promise_capability(p, constructor)?;
        let result = self.object();
        self.set_named(p, result, "promise", promise)?;
        self.set_named(p, result, "resolve", resolve)?;
        self.set_named(p, result, "reject", reject)?;
        Ok(result)
    }

    pub(super) fn new_promise_capability(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
    ) -> Result<(Value, Value, Value), JsError> {
        if !self.is_constructable(p, constructor) {
            return Err(self.type_error(
                p,
                "Promise.withResolvers receiver is not a constructor".into(),
            ));
        }
        let state = self.object();
        let resolve_atom = self.intern_atom(PROMISE_CAPABILITY_RESOLVE);
        let reject_atom = self.intern_atom(PROMISE_CAPABILITY_REJECT);
        self.set_property(state, resolve_atom, Value::UNDEFINED)?;
        self.set_property(state, reject_atom, Value::UNDEFINED)?;
        let executor = self.native_with_env(Native::PromiseCapabilityExecutor, state);
        let promise = self.construct_value(p, constructor, &[executor])?;
        let resolve = self
            .own_property(state, resolve_atom)
            .filter(|value| self.is_function(*value))
            .ok_or_else(|| {
                self.type_error(p, "Promise capability resolve is not callable".into())
            })?;
        let reject = self
            .own_property(state, reject_atom)
            .filter(|value| self.is_function(*value))
            .ok_or_else(|| {
                self.type_error(p, "Promise capability reject is not callable".into())
            })?;
        Ok((promise, resolve, reject))
    }

    fn promise_capability_executor(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let state = self
            .active_native_env()
            .ok_or_else(|| self.type_error(p, "invalid Promise capability executor".into()))?;
        let resolve_atom = self.intern_atom(PROMISE_CAPABILITY_RESOLVE);
        let reject_atom = self.intern_atom(PROMISE_CAPABILITY_REJECT);
        if self
            .own_property(state, resolve_atom)
            .is_some_and(|value| !value.is_undefined())
            || self
                .own_property(state, reject_atom)
                .is_some_and(|value| !value.is_undefined())
        {
            return Err(self.type_error(p, "Promise capability executor was already called".into()));
        }
        let resolve = args.first().copied().unwrap_or(Value::UNDEFINED);
        let reject = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        self.set_property(state, resolve_atom, resolve)?;
        self.set_property(state, reject_atom, reject)?;
        Ok(Value::UNDEFINED)
    }

    pub(super) fn call_promise_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::DynamicImport => self.dynamic_import(p, args),
            Native::AbstractModuleSource => {
                Err(self.type_error(p, "AbstractModuleSource cannot be called".into()))
            }
            Native::Promise => Err(JsError(
                "Promise constructor must be called with new".into(),
            )),
            Native::PromiseWithResolvers => self.promise_with_resolvers(p, this),
            Native::PromiseCapabilityExecutor => self.promise_capability_executor(p, args),
            Native::PromiseSpeciesGetter => Ok(this),
            Native::PromiseResolve => {
                if let Some(state) = self.active_native_env() {
                    let Some(promise) = self.take_promise_resolving_promise(p, state)? else {
                        return Ok(Value::UNDEFINED);
                    };
                    self.promise_resolve_value(
                        p,
                        promise,
                        args.first().copied().unwrap_or(Value::UNDEFINED),
                    )?;
                    Ok(Value::UNDEFINED)
                } else {
                    if !self.is_constructable(p, this) {
                        return Err(self.type_error(
                            p,
                            "Promise.resolve receiver is not a constructor".into(),
                        ));
                    }
                    self.promise_resolve_for_constructor(
                        p,
                        this,
                        args.first().copied().unwrap_or(Value::UNDEFINED),
                    )
                }
            }
            Native::PromiseReject => {
                if let Some(state) = self.active_native_env() {
                    let Some(promise) = self.take_promise_resolving_promise(p, state)? else {
                        return Ok(Value::UNDEFINED);
                    };
                    self.promise_settle(
                        p,
                        promise,
                        PromiseState::Rejected,
                        args.first().copied().unwrap_or(Value::UNDEFINED),
                    )?;
                    Ok(Value::UNDEFINED)
                } else {
                    let (promise, _, reject) = self.new_promise_capability(p, this)?;
                    self.call_value(
                        p,
                        reject,
                        Value::UNDEFINED,
                        &[args.first().copied().unwrap_or(Value::UNDEFINED)],
                    )?;
                    Ok(promise)
                }
            }
            Native::PromiseTry => self.promise_try(p, this, args),
            Native::PromiseThen => self.promise_then(
                p,
                this,
                args.first().copied().unwrap_or(Value::UNDEFINED),
                args.get(1).copied().unwrap_or(Value::UNDEFINED),
            ),
            Native::PromiseCatch => self.promise_catch(p, this, args),
            Native::PromiseFinally => self.promise_finally(p, this, args),
            Native::PromiseFinallyHandler => self.promise_finally_handler(p, args),
            Native::PromiseFinallyContinuationHandler => {
                self.promise_finally_continuation_handler(p, args)
            }
            Native::PromiseAll => self.promise_aggregate(p, this, args, AggregateMode::All),
            Native::PromiseAllKeyed => {
                self.promise_aggregate(p, this, args, AggregateMode::AllKeyed)
            }
            Native::PromiseRace => self.promise_aggregate(p, this, args, AggregateMode::Race),
            Native::PromiseAllSettled => {
                self.promise_aggregate(p, this, args, AggregateMode::AllSettled)
            }
            Native::PromiseAllSettledKeyed => {
                self.promise_aggregate(p, this, args, AggregateMode::AllSettledKeyed)
            }
            Native::PromiseAny => self.promise_aggregate(p, this, args, AggregateMode::Any),
            Native::PromiseAggregateJob => {
                self.promise_aggregate_job(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::ArrayFromAsyncFulfilled | Native::ArrayFromAsyncRejected => self
                .array_from_async_reaction(
                    p,
                    native == Native::ArrayFromAsyncFulfilled,
                    args.first().copied().unwrap_or(Value::UNDEFINED),
                ),
            Native::AsyncFromSyncValue => self.async_from_sync_value(args),
            Native::AsyncGeneratorDelegateFulfilled => self.async_generator_delegate_fulfilled(
                p,
                this,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            ),
            Native::AsyncGeneratorDelegateRejected => self.async_generator_delegate_rejected(
                p,
                this,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            ),
            Native::AsyncGeneratorReturnFulfilled => self.async_generator_return_fulfilled(p, args),
            Native::AsyncGeneratorReturnRejected => self.async_generator_return_rejected(p, args),
            _ => unreachable!(),
        }
    }

    /// The name of the running code's Script or Module, the referrer of its
    /// `import()` calls.
    pub(super) fn active_script_or_module(&self, p: &ResidualProgram) -> std::rc::Rc<str> {
        self.programs
            .script_or_module(self.active_program)
            .unwrap_or_else(|| std::rc::Rc::from(p.source_name.as_str()))
    }

    fn dynamic_import(&mut self, p: &ResidualProgram, args: &[Value]) -> Result<Value, JsError> {
        let promise = self.promise_object()?;
        let specifier = args.first().copied().unwrap_or(Value::UNDEFINED);
        let options = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let phase = args
            .get(2)
            .and_then(|value| value.as_number())
            .and_then(crate::bytecode::ModuleRequestPhase::from_runtime_value)
            .unwrap_or(crate::bytecode::ModuleRequestPhase::Evaluation);
        let operation = self.with_call_roots([promise], |vm| {
            let specifier = vm.to_string(p, specifier)?;
            let module_type = vm.validate_dynamic_import_options(p, options)?;
            let referrer = vm.active_script_or_module(p);
            let resolution = vm.host_resolved_module(&referrer, &specifier);
            match resolution {
                Err(error) => return Err(vm.module_load_error(p, error)),
                Ok(Some(module)) => {
                    if phase != crate::bytecode::ModuleRequestPhase::Source {
                        let requested = module_type.as_deref().unwrap_or(JAVASCRIPT_MODULE_TYPE);
                        if let Err(error) = vm.host.validate_module_type(&module, requested) {
                            return Err(vm.module_load_error(p, error));
                        }
                    }
                    if phase == crate::bytecode::ModuleRequestPhase::Source {
                        let source = vm.module_source_value(p, &module)?;
                        vm.promise_resolve_value(p, promise, source)?;
                        return Ok(Value::UNDEFINED);
                    }
                    let module_type = module_type.as_deref().unwrap_or("javascript");
                    let cache_key = module_cache_key(&module.name, module_type);
                    if phase == crate::bytecode::ModuleRequestPhase::Evaluation
                        && module_type == "javascript"
                        && let Some(reason) = vm.errored_async_cycle_root(&cache_key)
                    {
                        vm.promise_settle(p, promise, PromiseState::Rejected, reason)?;
                        return Ok(Value::UNDEFINED);
                    }
                    if let Some(outcome) = vm
                        .realm
                        .promise
                        .modules
                        .get(&cache_key)
                        .map(|record| record.outcome)
                    {
                        match outcome {
                            ModuleOutcome::Evaluated(namespace) => {
                                let namespace =
                                    if phase == crate::bytecode::ModuleRequestPhase::Defer {
                                        if module_type == "javascript" {
                                            vm.cached_deferred_javascript_namespace(
                                                p, &cache_key, &module,
                                            )?
                                        } else {
                                            vm.deferred_synthetic_module_namespace(
                                                p,
                                                &module,
                                                module_type,
                                            )?
                                        }
                                    } else {
                                        namespace
                                    };
                                vm.promise_resolve_value(p, promise, namespace)?;
                                return Ok(Value::UNDEFINED);
                            }
                            ModuleOutcome::Deferred(namespace)
                                if phase == crate::bytecode::ModuleRequestPhase::Defer =>
                            {
                                vm.promise_resolve_value(p, promise, namespace)?;
                                return Ok(Value::UNDEFINED);
                            }
                            ModuleOutcome::Deferred(namespace) => {
                                let namespace =
                                    vm.evaluate_deferred_module_namespace(p, namespace)?;
                                vm.promise_resolve_value(p, promise, namespace)?;
                                return Ok(Value::UNDEFINED);
                            }
                            ModuleOutcome::Errored(reason) => {
                                if phase == crate::bytecode::ModuleRequestPhase::Defer
                                    && module_type == "javascript"
                                {
                                    let namespace = vm
                                        .realm
                                        .promise
                                        .modules
                                        .get(&cache_key)
                                        .and_then(ModuleRecord::deferred_namespace);
                                    let namespace = match namespace {
                                        Some(namespace) => namespace,
                                        None => {
                                            let namespace =
                                                vm.deferred_module_namespace(p, &module)?;
                                            vm.realm
                                                .promise
                                                .modules
                                                .get_mut(&cache_key)
                                                .expect("module record found above")
                                                .cache_deferred_namespace(namespace);
                                            namespace
                                        }
                                    };
                                    vm.promise_resolve_value(p, promise, namespace)?;
                                } else {
                                    vm.promise_settle(p, promise, PromiseState::Rejected, reason)?;
                                }
                                return Ok(Value::UNDEFINED);
                            }
                            ModuleOutcome::Pending(ModulePhase::Loading)
                                if module_type == "javascript"
                                    && phase == crate::bytecode::ModuleRequestPhase::Evaluation =>
                            {
                                vm.queue_dynamic_import_job(cache_key, module, promise);
                                return Ok(Value::UNDEFINED);
                            }
                            ModuleOutcome::Pending(ModulePhase::Instantiated) => {}
                            ModuleOutcome::Pending(_) => {
                                let joined = vm
                                    .realm
                                    .promise
                                    .modules
                                    .get_mut(&cache_key)
                                    .expect("module record found above")
                                    .add_waiter(promise);
                                debug_assert!(joined);
                                return Ok(Value::UNDEFINED);
                            }
                        }
                    }
                    if let Some(namespace) = vm.root_module_namespace(p, &module)? {
                        vm.realm
                            .promise
                            .modules
                            .insert(cache_key, ModuleRecord::evaluating_root(namespace, promise));
                        return Ok(Value::UNDEFINED);
                    }
                    if module_type == "javascript"
                        && phase == crate::bytecode::ModuleRequestPhase::Evaluation
                        && vm.deferred_dependency_batch
                    {
                        vm.queue_dynamic_import_job(cache_key, module, promise);
                        return Ok(Value::UNDEFINED);
                    }
                    if module_type == "javascript"
                        && phase == crate::bytecode::ModuleRequestPhase::Defer
                    {
                        let mut seen = FxHashSet::default();
                        let mut asynchronous = Vec::new();
                        vm.gather_async_transitive_dependencies(
                            p,
                            &module,
                            &mut seen,
                            &mut asynchronous,
                        )?;
                        if asynchronous.is_empty() {
                            let namespace = vm.deferred_module_namespace(p, &module)?;
                            vm.realm
                                .promise
                                .modules
                                .insert(cache_key, ModuleRecord::deferred(namespace));
                            vm.promise_resolve_value(p, promise, namespace)?;
                            return Ok(Value::UNDEFINED);
                        }
                        let mut active = ModuleEvaluationStack::default();
                        for dependency in asynchronous {
                            vm.evaluate_static_module_source(p, dependency, &mut active)?;
                        }
                        let entry_has_tla = vm
                            .static_module_plan(&module.source, &module.name)
                            .is_some_and(|plan| plan.has_top_level_await);
                        if !entry_has_tla {
                            let namespace = vm.deferred_module_namespace(p, &module)?;
                            vm.realm
                                .promise
                                .modules
                                .insert(cache_key, ModuleRecord::deferred(namespace));
                            vm.promise_resolve_value(p, promise, namespace)?;
                            return Ok(Value::UNDEFINED);
                        }
                        if let Some(ModuleOutcome::Evaluated(namespace)) = vm
                            .realm
                            .promise
                            .modules
                            .get(&cache_key)
                            .map(|record| record.outcome)
                        {
                            vm.promise_resolve_value(p, promise, namespace)?;
                            return Ok(Value::UNDEFINED);
                        }
                    }
                    if module_type != "javascript"
                        && phase == crate::bytecode::ModuleRequestPhase::Defer
                    {
                        let namespace =
                            vm.deferred_synthetic_module_namespace(p, &module, module_type)?;
                        vm.promise_resolve_value(p, promise, namespace)?;
                        return Ok(Value::UNDEFINED);
                    }
                    if module_type == "javascript"
                        && phase == crate::bytecode::ModuleRequestPhase::Evaluation
                    {
                        let mut active = ModuleEvaluationStack::default();
                        let outer_batch =
                            std::mem::replace(&mut vm.deferred_dependency_batch, true);
                        let evaluation =
                            vm.evaluate_static_module_source(p, module.clone(), &mut active);
                        vm.deferred_dependency_batch = outer_batch;
                        evaluation?;
                        match vm
                            .realm
                            .promise
                            .modules
                            .get(&cache_key)
                            .map(|record| record.outcome)
                        {
                            Some(ModuleOutcome::Evaluated(namespace)) => {
                                vm.promise_resolve_value(p, promise, namespace)?;
                            }
                            Some(ModuleOutcome::Errored(reason)) => {
                                vm.promise_settle(p, promise, PromiseState::Rejected, reason)?;
                            }
                            Some(ModuleOutcome::Pending(_)) => {
                                let joined = vm
                                    .realm
                                    .promise
                                    .modules
                                    .get_mut(&cache_key)
                                    .expect("module record found above")
                                    .add_waiter(promise);
                                debug_assert!(joined);
                            }
                            Some(ModuleOutcome::Deferred(_)) | None => {
                                return Err(vm.type_error(
                                    p,
                                    "dynamic module evaluation did not create a module record"
                                        .into(),
                                ));
                            }
                        }
                        return Ok(Value::UNDEFINED);
                    }
                    let mut record = ModuleRecord::loading(promise);
                    let linked = record.begin_linking();
                    debug_assert!(linked);
                    let evaluating = record.begin_evaluation();
                    debug_assert!(evaluating);
                    vm.realm.promise.modules.insert(cache_key.clone(), record);
                    match vm.evaluate_dynamic_module(p, &module, module_type, phase) {
                        Ok(namespace) => {
                            let waiters = vm
                                .realm
                                .promise
                                .modules
                                .get_mut(&cache_key)
                                .expect("module record inserted above")
                                .evaluate(namespace)
                                .expect("module record is evaluating");
                            for waiter in waiters {
                                vm.promise_resolve_value(p, waiter, namespace)?;
                            }
                        }
                        Err(error) => {
                            let reason = error.thrown_value().unwrap_or_else(|| {
                                vm.heap.alloc(Cell::Error(error.into_message()))
                            });
                            let waiters = vm
                                .realm
                                .promise
                                .modules
                                .get_mut(&cache_key)
                                .expect("module record inserted above")
                                .fail(reason)
                                .expect("module record is pending");
                            for waiter in waiters {
                                vm.promise_settle(p, waiter, PromiseState::Rejected, reason)?;
                            }
                        }
                    }
                    return Ok(Value::UNDEFINED);
                }
                Ok(None) => {}
            }
            Err(vm.type_error(p, "host did not resolve dynamic import module".into()))
        });
        if let Err(error) = operation {
            let reason = error
                .thrown_value()
                .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
            self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
        }
        Ok(promise)
    }

    fn root_module_namespace(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
    ) -> Result<Option<Value>, JsError> {
        if !p.is_module()
            || self.active_program != ProgramId::MAIN
            || !crate::module_identity::same_name(&module.name, &p.source_name)
        {
            return Ok(None);
        }
        let Some(plan) = p.module_link_plan.as_ref() else {
            return Ok(None);
        };
        let Some(locals) = self.root_module_local_exports(plan.locals.clone()) else {
            return Ok(None);
        };
        let mut active = ActiveModuleExports::default();
        active.insert(
            crate::module_identity::normalize(std::path::Path::new(&module.name)),
            locals.clone(),
        );
        match self.resolve_static_module_links(
            p,
            module,
            locals,
            plan.reexports.clone(),
            &mut active,
        )? {
            StaticModuleGraph::Linked {
                exports,
                incomplete: false,
                ..
            } => self.module_namespace_from_static(exports).map(Some),
            StaticModuleGraph::Linked {
                incomplete: true, ..
            }
            | StaticModuleGraph::LinkError => self
                .syntax_error_result(p, "module export could not be resolved unambiguously")
                .map(Some),
            StaticModuleGraph::Unsupported => Ok(None),
        }
    }

    fn root_module_local_exports(
        &self,
        locals: Vec<(String, String)>,
    ) -> Option<Vec<(String, StaticModuleValue)>> {
        self.module_local_exports(ProgramId::MAIN, locals)
    }

    fn module_local_exports(
        &self,
        program: ProgramId,
        locals: Vec<(String, String)>,
    ) -> Option<Vec<(String, StaticModuleValue)>> {
        let residual = self.programs.get(program)?;
        let slots = local_slots(residual.functions.first()?);
        locals
            .into_iter()
            .map(|(local, exported)| {
                // A stored module shares the VM's atom ids, so the VM finds its local's atom
                // without scanning the program's (and so the VM's) whole table.
                let local_atom = self.lookup_atom(&local)?;
                let slot = *slots.get(&local_atom)?;
                Some((
                    exported,
                    StaticModuleValue::Binding {
                        program,
                        slot: u16::try_from(slot).ok()?,
                        value: self
                            .programs
                            .module_environment(program)
                            .and_then(|environment| self.heap.environment_slot(environment, slot))
                            .filter(|value| !value.is_deleted())
                            .unwrap_or(Value::UNDEFINED),
                    },
                ))
            })
            .collect()
    }

    pub(super) fn finish_main_module(
        &mut self,
        p: &ResidualProgram,
        result: &Result<Value, JsError>,
    ) -> Result<(), JsError> {
        if !p.is_module() {
            return Ok(());
        }
        let key = module_cache_key(&p.source_name, "javascript");
        if !self.realm.promise.modules.contains_key(&key) {
            return Ok(());
        }
        match result {
            Ok(value)
                if p.functions
                    .first()
                    .is_some_and(|function| function.is_async) =>
            {
                self.finish_async_main_module(p, &key, *value)
            }
            Ok(_) => self.complete_main_module(p, &key),
            Err(error) => {
                let reason = error
                    .thrown_value()
                    .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.to_string())));
                self.fail_main_module(p, &key, reason)
            }
        }
    }

    pub(crate) fn module_evaluation_pending(&self, p: &ResidualProgram) -> Result<bool, JsError> {
        if !p.is_module() {
            return Ok(false);
        }
        let key = module_cache_key(&p.source_name, "javascript");
        let record = self
            .realm
            .promise
            .modules
            .get(&key)
            .ok_or_else(|| JsError::validation("module evaluation has not started".into()))?;
        Ok(matches!(record.outcome, ModuleOutcome::Pending(_)))
    }

    fn finish_async_main_module(
        &mut self,
        p: &ResidualProgram,
        key: &str,
        promise: Value,
    ) -> Result<(), JsError> {
        let Some(completion) = self.promise_record(promise).cloned() else {
            return Err(self.type_error(p, "module completion promise is unavailable".into()));
        };
        match completion.state {
            PromiseState::Pending => {
                let Some(record) = self.realm.promise.modules.get_mut(key) else {
                    return Ok(());
                };
                let Some(namespace) = record.pending_namespace() else {
                    return Err(self.type_error(p, "main module namespace is unavailable".into()));
                };
                record.begin_async_evaluation(namespace);
                record.track_evaluation_promise(promise);
                self.watch_module_evaluation(promise);
                self.realm
                    .promise
                    .async_module_order
                    .push_back(key.to_owned());
                Ok(())
            }
            PromiseState::Fulfilled => self.complete_main_module(p, key),
            PromiseState::Rejected => {
                self.fail_main_module(p, key, completion.result)?;
                Err(JsError::thrown(
                    completion.result,
                    "main module evaluation rejected".into(),
                ))
            }
        }
    }

    fn complete_main_module(&mut self, p: &ResidualProgram, key: &str) -> Result<(), JsError> {
        let Some((namespace, waiters)) = self
            .realm
            .promise
            .modules
            .get_mut(key)
            .and_then(ModuleRecord::evaluate_root)
        else {
            return Ok(());
        };
        for waiter in waiters {
            self.promise_resolve_value(p, waiter, namespace)?;
        }
        Ok(())
    }

    fn fail_main_module(
        &mut self,
        p: &ResidualProgram,
        key: &str,
        reason: Value,
    ) -> Result<(), JsError> {
        let waiters = self
            .realm
            .promise
            .modules
            .get_mut(key)
            .and_then(|record| record.fail(reason));
        if let Some(waiters) = waiters {
            for waiter in waiters {
                self.promise_settle(p, waiter, PromiseState::Rejected, reason)?;
            }
        }
        Ok(())
    }

    fn evaluate_dynamic_module(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        module_type: &str,
        phase: crate::bytecode::ModuleRequestPhase,
    ) -> Result<Value, JsError> {
        if phase == crate::bytecode::ModuleRequestPhase::Source {
            return self.module_source_value(p, module);
        }
        match module_type {
            "bytes" => {
                let bytes = module.bytes().to_vec();
                let length = bytes.len();
                let kind = crate::heap::TypedArrayKind::Uint8;
                let buffer = self.heap.alloc(Cell::ArrayBuffer {
                    object: Box::new(Self::empty_object(self.array_buffer_proto)),
                    bytes: bytes.into(),
                    shared: false,
                    detached: false,
                    max_byte_length: length,
                    resizable: false,
                    immutable: true,
                });
                let value = self.heap.alloc(Cell::TypedArray {
                    kind,
                    object: Box::new(crate::heap::BufferViewObject {
                        object: Self::empty_object(self.typed_array_proto(kind)),
                        offset: 0,
                    }),
                    buffer,
                    length,
                    length_tracking: false,
                });
                self.module_namespace_default(value)
            }
            "text" => {
                let source = self.heap.alloc_string(JsString::from_str(&module.source));
                self.module_namespace_default(source)
            }
            "json" => {
                let source = self.heap.alloc_string(JsString::from_str(&module.source));
                let value = self.json_parse(p, &[source])?;
                self.module_namespace_default(value)
            }
            "javascript" => self.evaluate_javascript_module(p, module),
            _ => Err(self.type_error(p, "unsupported dynamic import type".into())),
        }
    }

    fn module_source_value(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
    ) -> Result<Value, JsError> {
        if !self.host.has_module_source(module) {
            return self.syntax_error_result(
                p,
                &format!(
                    "Source phase import object is not defined for module '{}'",
                    module.name
                ),
            );
        }
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if let Some(value) = self.realm.promise.module_sources.get(&identity) {
            return Ok(*value);
        }
        let prototype_atom = self.prototype_atom();
        let constructor = self.native_value(Native::AbstractModuleSource);
        let prototype = self
            .own_property(constructor, prototype_atom)
            .unwrap_or(self.object_proto);
        let value = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        self.realm.promise.module_sources.insert(identity, value);
        Ok(value)
    }

    fn synchronous_graph_has_await(
        &self,
        key: &str,
        seen: &mut FxHashSet<String>,
    ) -> Result<bool, ()> {
        let _stack = quench_stack::StackGuard::enter()?;
        if !seen.insert(key.to_owned()) {
            return Ok(false);
        }
        let Some(record) = self.realm.promise.modules.get(key) else {
            return Ok(false);
        };
        let Some(program) = record
            .program
            .and_then(|program| self.programs.get(program))
        else {
            return Ok(false);
        };
        if program
            .functions
            .first()
            .is_some_and(|function| function.is_async)
        {
            return Ok(true);
        }
        for (request, dependency) in program
            .module_requests
            .iter()
            .zip(&record.resolved_requests)
        {
            if request.phase == crate::bytecode::ModuleRequestPhase::Evaluation
                && request.module_type.as_deref().unwrap_or("javascript") == "javascript"
                && self.synchronous_graph_has_await(
                    &module_cache_key(dependency, "javascript"),
                    seen,
                )?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn synchronous_module_namespace(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
    ) -> Result<crate::SynchronousModule<Value>, JsError> {
        use crate::SynchronousModule;
        let key = module_cache_key(&module.name, "javascript");
        let linked = match self.instantiate_static_module_graph(p, module, None) {
            Ok(linked) => linked,
            Err(error) => {
                self.reject_module_instantiation(p, &key, error)?;
                unreachable!("failed instantiation throws");
            }
        };
        if self
            .synchronous_graph_has_await(&key, &mut FxHashSet::default())
            .map_err(|()| self.stack_exhaustion_error())?
        {
            for (linked_module, _) in linked {
                if let Some(record) = self
                    .realm
                    .promise
                    .modules
                    .get_mut(&module_cache_key(&linked_module.name, "javascript"))
                {
                    record.retain_instantiation();
                }
            }
            return Ok(SynchronousModule::AsyncGraph);
        }
        drop(linked);
        if self.realm.promise.modules.get(&key).is_some_and(|record| {
            matches!(
                record.phase(),
                ModulePhase::Evaluating
                    | ModulePhase::EvaluatingAsync
                    | ModulePhase::WaitingForDependencies
            )
        }) {
            return Ok(SynchronousModule::Evaluating);
        }
        self.evaluate_static_module_source(
            p,
            module.clone(),
            &mut ModuleEvaluationStack::default(),
        )?;
        match self
            .realm
            .promise
            .modules
            .get(&key)
            .map(|record| record.outcome)
        {
            Some(ModuleOutcome::Evaluated(namespace)) => {
                Ok(SynchronousModule::Namespace(namespace))
            }
            Some(ModuleOutcome::Errored(reason)) => Err(JsError::thrown(
                reason,
                "synchronous module evaluation failed".into(),
            )),
            _ => Ok(SynchronousModule::Evaluating),
        }
    }

    pub(super) fn module_namespace_with_export(
        &mut self,
        p: &ResidualProgram,
        namespace: Value,
        name: &str,
        value: Value,
    ) -> Result<Value, JsError> {
        let mut exports = self
            .cached_static_exports(namespace)
            .ok_or_else(|| self.type_error(p, "module namespace required".into()))?;
        exports.push((name.to_owned(), StaticModuleValue::Cached(value)));
        self.module_namespace_from_static(exports)
    }

    fn evaluate_javascript_module(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
    ) -> Result<Value, JsError> {
        self.instantiate_static_module_graph(p, module, None)?;
        if let Some(message) = self.static_module_early_error(&module.source, &module.name) {
            return self.syntax_error_result(p, &message);
        }
        if let Some(plan) = self.static_module_plan(&module.source, &module.name) {
            let mut active = ModuleEvaluationStack::default();
            active.enter(crate::module_identity::normalize(std::path::Path::new(
                &module.name,
            )));
            self.evaluate_module_requests(p, &module.name, &plan.requests, &mut active)?;
        }
        self.evaluate_javascript_module_body(p, module)
    }

    fn gather_async_transitive_dependencies(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        seen: &mut FxHashSet<ModuleIdentity>,
        output: &mut Vec<ModuleSource>,
    ) -> Result<(), JsError> {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if !seen.insert(identity) {
            return Ok(());
        }
        let key = module_cache_key(&module.name, "javascript");
        if self.realm.promise.modules.get(&key).is_some_and(|record| {
            !matches!(
                record.phase(),
                ModulePhase::Loading | ModulePhase::Linking | ModulePhase::Deferred
            )
        }) {
            return Ok(());
        }
        let Some(plan) = self.static_module_plan(&module.source, &module.name) else {
            return Err(self.type_error(p, "deferred module metadata is unavailable".into()));
        };
        if plan.has_top_level_await {
            output.push(module.clone());
            return Ok(());
        }
        for request in plan
            .requests
            .iter()
            .filter(|request| request.phase == crate::bytecode::ModuleRequestPhase::Evaluation)
        {
            let dependency = self.resolve_module_request(p, &module.name, request)?;
            let Some(dependency) = dependency else {
                return Err(self.type_error(p, "static module request was not resolved".into()));
            };
            self.gather_async_transitive_dependencies(p, &dependency, seen, output)?;
        }
        Ok(())
    }

    fn deferred_module_namespace(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
    ) -> Result<Value, JsError> {
        let Some(plan) = self.static_module_plan(&module.source, &module.name) else {
            return Err(self.type_error(p, "deferred module metadata is unavailable".into()));
        };
        let mut names = plan
            .link_plan
            .into_iter()
            .flat_map(|plan| plan.locals)
            .map(|(_, exported)| exported)
            .collect::<Vec<_>>();
        if let Some(exports) = crate::Engine::static_module_exports(&module.source, &module.name) {
            names.extend(exports.into_iter().map(|(name, _)| name));
        }
        names.sort_by_key(|name| name.encode_utf16().collect::<Vec<_>>());
        names.dedup();
        let namespace = self.module_namespace_with_tag(
            names
                .into_iter()
                .map(|name| (name, Value::UNDEFINED))
                .collect(),
            "Deferred Module",
        )?;
        let Some(object) = self.object_data_mut(namespace) else {
            return Err(self.type_error(p, "module namespace allocation failed".into()));
        };
        object.set_deferred_module(Some(module.clone()));
        Ok(namespace)
    }

    pub(super) fn evaluate_deferred_module_namespace(
        &mut self,
        p: &ResidualProgram,
        namespace: Value,
    ) -> Result<Value, JsError> {
        let Some(module) = self
            .object_data(namespace)
            .and_then(|object| object.deferred_module().cloned())
        else {
            return Ok(namespace);
        };
        let cache_key = module_cache_key(&module.name, "javascript");
        match self
            .realm
            .promise
            .modules
            .get(&cache_key)
            .map(|record| record.outcome)
        {
            Some(ModuleOutcome::Errored(reason)) => {
                return Err(JsError::thrown(
                    reason,
                    "deferred module evaluation failed".into(),
                ));
            }
            Some(ModuleOutcome::Evaluated(evaluated)) => {
                self.copy_module_namespace(evaluated, namespace, p)?;
                return Ok(namespace);
            }
            Some(ModuleOutcome::Pending(ModulePhase::Evaluating)) => {
                return Err(self.type_error(
                    p,
                    "deferred module is not ready for synchronous evaluation".into(),
                ));
            }
            _ => {}
        }
        if !self.ready_for_sync_execution(p, &module, &mut FxHashSet::default())? {
            return Err(self.type_error(
                p,
                "deferred module is not ready for synchronous evaluation".into(),
            ));
        }
        let Some(record) = self.realm.promise.modules.get_mut(&cache_key) else {
            return Err(self.type_error(p, "deferred module record is unavailable".into()));
        };
        if record.begin_deferred_evaluation().is_none() {
            return Ok(namespace);
        }
        let result = self.evaluate_javascript_module(p, &module);
        match result {
            Ok(evaluated) => {
                self.copy_module_namespace(evaluated, namespace, p)?;
                self.settle_static_module(p, &cache_key, Ok(evaluated))?;
                Ok(namespace)
            }
            Err(error) => {
                let settled = self.settle_static_module(p, &cache_key, Err(error));
                if let Some(object) = self.object_data_mut(namespace) {
                    object.set_deferred_module(Some(module));
                }
                settled?;
                Ok(namespace)
            }
        }
    }

    pub(super) fn evaluate_deferred_namespace_for_key(
        &mut self,
        p: &ResidualProgram,
        namespace: Value,
        key: Option<crate::vm::property_key::PropertyKey>,
    ) -> Result<(), JsError> {
        // Only a deferred namespace can evaluate; most objects are rejected before the key.
        if !self
            .object_data(namespace)
            .is_some_and(|object| object.deferred_module().is_some())
        {
            return Ok(());
        }
        let triggers_evaluation = match key {
            None => true,
            Some(crate::vm::property_key::PropertyKey::String(atom)) => {
                self.atom_name(atom) != "then"
            }
            Some(
                crate::vm::property_key::PropertyKey::Symbol(_)
                | crate::vm::property_key::PropertyKey::Private(_),
            ) => false,
        };
        if triggers_evaluation {
            self.evaluate_deferred_module_namespace(p, namespace)?;
        }
        Ok(())
    }

    fn ready_for_sync_execution(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        seen: &mut FxHashSet<ModuleIdentity>,
    ) -> Result<bool, JsError> {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if !seen.insert(identity) {
            return Ok(true);
        }
        match self
            .realm
            .promise
            .modules
            .get(&module_cache_key(&module.name, "javascript"))
            .map(|record| record.outcome)
        {
            Some(ModuleOutcome::Evaluated(_)) | Some(ModuleOutcome::Errored(_)) => return Ok(true),
            Some(ModuleOutcome::Pending(_)) => return Ok(false),
            _ => {}
        }
        let Some(plan) = self.static_module_plan(&module.source, &module.name) else {
            return Ok(false);
        };
        if plan.has_top_level_await {
            return Ok(false);
        }
        for request in plan.requests {
            let dependency = self
                .resolve_module_request(p, &module.name, &request)?
                .ok_or_else(|| {
                    self.type_error(p, "static module request was not resolved".into())
                })?;
            if request.module_type.as_deref().unwrap_or("javascript") != "javascript" {
                continue;
            }
            if !self.ready_for_sync_execution(p, &dependency, seen)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn copy_module_namespace(
        &mut self,
        source: Value,
        target: Value,
        p: &ResidualProgram,
    ) -> Result<(), JsError> {
        let Some(source) = self.object_data(source).cloned() else {
            return Err(self.type_error(p, "evaluated module namespace is invalid".into()));
        };
        if self.heap.copy_property_storage(&source, target)
            && let Some(target) = self.object_data_mut(target)
        {
            target.set_proto(source.proto());
            if source.is_module_namespace() {
                target.set_module_namespace();
            }
            target.set_module_bindings(source.module_bindings().to_vec());
            target.set_deferred_module(None);
        }
        self.invalidate_field_caches();
        self.invalidate_method_caches();
        Ok(())
    }

    fn evaluate_javascript_module_body(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
    ) -> Result<Value, JsError> {
        let key = module_cache_key(&module.name, "javascript");
        if let Some(namespace) = self
            .realm
            .promise
            .modules
            .get(&key)
            .and_then(|record| record.program.and_then(|_| record.pending_namespace()))
        {
            self.evaluate_module_locals(
                p,
                module,
                Vec::new(),
                &mut ActiveModuleExports::default(),
            )?;
            return Ok(namespace);
        }
        if let Some(exports) = crate::Engine::static_module_exports(&module.source, &module.name) {
            let exports = exports
                .into_iter()
                .map(|(name, value)| (name, self.module_static_value(value)))
                .collect();
            return self.module_namespace(exports);
        }
        if let Some(reason) = crate::Engine::static_module_throw(&module.source) {
            return self.evaluate_static_module_throw(p, reason);
        }
        if self
            .static_module_plan(&module.source, &module.name)
            .is_some_and(|plan| {
                plan.link_plan
                    .is_some_and(|link_plan| link_plan.reexports.is_empty())
            })
            && let Some(exports) = crate::Engine::module_export_names(&module.source, &module.name)
        {
            let mut active = ActiveModuleExports::default();
            active.insert(
                crate::module_identity::normalize(std::path::Path::new(&module.name)),
                Vec::new(),
            );
            let exports = self.evaluate_module_locals(p, module, exports, &mut active)?;
            return self.module_namespace_from_static(exports);
        }
        if module.name.ends_with(".json") {
            return Err(self.type_error(
                p,
                "JSON module import requires a json type attribute".into(),
            ));
        }
        let mut active = ActiveModuleExports::default();
        match self.resolve_static_module_graph(p, module.clone(), &mut active)? {
            StaticModuleGraph::Linked {
                exports,
                incomplete: false,
                ..
            } => self.module_namespace_from_static(exports),
            StaticModuleGraph::Linked {
                incomplete: true, ..
            }
            | StaticModuleGraph::LinkError => {
                self.syntax_error_result(p, "module export could not be resolved unambiguously")
            }
            StaticModuleGraph::Unsupported => {
                Err(self.type_error(p, "dynamic import module evaluation is unsupported".into()))
            }
        }
    }

    pub(super) fn evaluate_program_module_requests(
        &mut self,
        p: &ResidualProgram,
    ) -> Result<(), JsError> {
        if !p.is_module() {
            return Ok(());
        }
        let root = ModuleSource::text(p.source_name.clone(), "");
        let namespace = self.module_namespace(Vec::new())?;
        self.realm.promise.modules.insert(
            module_cache_key(&p.source_name, "javascript"),
            ModuleRecord::evaluating_main(namespace),
        );
        self.instantiate_static_module_graph(p, &root, Some(ProgramId::MAIN))?;
        if p.module_requests.is_empty() {
            return Ok(());
        }
        let mut active = ModuleEvaluationStack::default();
        active.enter(crate::module_identity::normalize(std::path::Path::new(
            &p.source_name,
        )));
        let outer_batch = std::mem::replace(&mut self.deferred_dependency_batch, true);
        let requests =
            self.evaluate_module_requests(p, &p.source_name, &p.module_requests, &mut active);
        self.deferred_dependency_batch = outer_batch;
        requests?;
        self.advance_dynamic_import_jobs(p, false)?;
        let pending_async_evaluation = self.realm.promise.modules.values().any(|record| {
            matches!(
                record.phase(),
                ModulePhase::EvaluatingAsync | ModulePhase::WaitingForDependencies
            )
        });
        if !outer_batch && pending_async_evaluation {
            self.drain_jobs(p)?;
            self.advance_static_module_jobs(p)?;
        }
        let mut linking = ActiveModuleExports::default();
        let imports = self.resolve_module_import_values(
            p,
            p,
            &p.source_name,
            &p.module_imports,
            &mut linking,
        )?;
        self.programs.set_module_imports(ProgramId::MAIN, imports);
        Ok(())
    }

    fn queue_dynamic_import_job(
        &mut self,
        cache_key: String,
        module: ModuleSource,
        promise: Value,
    ) {
        // Each import owns an instantiation attempt. Successful evaluation is
        // shared by the module record; link failures must produce fresh errors.
        self.realm
            .promise
            .dynamic_import_jobs
            .push(DynamicImportJob {
                cache_key,
                module,
                promise,
            });
    }

    pub(super) fn advance_dynamic_import_jobs(
        &mut self,
        p: &ResidualProgram,
        allow_evaluation: bool,
    ) -> Result<(), JsError> {
        // Checked after every job: an empty queue has nothing to root or advance.
        if self.realm.promise.dynamic_import_jobs.is_empty() {
            return Ok(());
        }
        let jobs = std::mem::take(&mut self.realm.promise.dynamic_import_jobs);
        // Taking the batch transfers its roots out of the persistent queue.
        // Keep every waiter alive, including jobs not yet evaluated, until it
        // is settled or transferred back to a queue/module record.
        let waiters: Vec<_> = jobs.iter().map(|job| job.promise).collect();
        self.with_call_roots(waiters, |vm| {
            for job in jobs {
                if let Some(outcome) = vm
                    .realm
                    .promise
                    .modules
                    .get(&job.cache_key)
                    .map(|record| record.outcome)
                    && vm.settle_dynamic_import_waiter(p, job.promise, outcome)?
                {
                    continue;
                }
                if !allow_evaluation {
                    vm.realm.promise.dynamic_import_jobs.push(job);
                    continue;
                }
                let mut active = ModuleEvaluationStack::default();
                if let Err(error) = vm.evaluate_static_module_source(p, job.module, &mut active) {
                    let reason = error
                        .thrown_value()
                        .unwrap_or_else(|| vm.heap.alloc(Cell::Error(error.into_message())));
                    vm.promise_settle(p, job.promise, PromiseState::Rejected, reason)?;
                    continue;
                }
                let outcome = vm
                    .realm
                    .promise
                    .modules
                    .get(&job.cache_key)
                    .map(|record| record.outcome)
                    .unwrap_or(ModuleOutcome::Pending(ModulePhase::Evaluating));
                if !vm.settle_dynamic_import_waiter(p, job.promise, outcome)?
                    && let Some(record) = vm.realm.promise.modules.get_mut(&job.cache_key)
                {
                    record.add_waiter(job.promise);
                }
            }
            Ok(())
        })
    }

    fn settle_dynamic_import_waiter(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        outcome: ModuleOutcome,
    ) -> Result<bool, JsError> {
        let (state, value) = match outcome {
            ModuleOutcome::Evaluated(namespace) => (PromiseState::Fulfilled, namespace),
            ModuleOutcome::Errored(reason) => (PromiseState::Rejected, reason),
            ModuleOutcome::Pending(_) | ModuleOutcome::Deferred(_) => return Ok(false),
        };
        self.promise_settle(p, promise, state, value)?;
        Ok(true)
    }

    fn errored_async_cycle_root(&self, cache_key: &str) -> Option<Value> {
        let cycle_root = self
            .realm
            .promise
            .modules
            .get(cache_key)?
            .async_cycle_root()?;
        let root_key = module_cache_key(&cycle_root.text(), "javascript");
        match self.realm.promise.modules.get(&root_key)?.outcome {
            ModuleOutcome::Errored(reason) => Some(reason),
            ModuleOutcome::Pending(_)
            | ModuleOutcome::Deferred(_)
            | ModuleOutcome::Evaluated(_) => None,
        }
    }

    pub(super) fn instantiate_main_module(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        self.instantiate_module_environment(p, ProgramId::MAIN)
    }

    fn instantiate_module_environment(
        &mut self,
        p: &ResidualProgram,
        program: ProgramId,
    ) -> Result<(), JsError> {
        if !p.is_module() || self.programs.module_environment(program).is_some() {
            return Ok(());
        }
        let Some(root) = p.functions.first() else {
            return Ok(());
        };
        let local_slots = local_slots(root);
        let mut slots = vec![Value::UNDEFINED; root.local_atoms.len()];
        for atom in &root.lexical_atoms {
            if let Some(&slot) = local_slots.get(atom) {
                slots[slot] = Value::DELETED;
            }
        }
        let environment = self.heap.alloc(Cell::Environment {
            parent: Value::NULL,
            function: super::ROOT_FUNCTION_ID,
            slots: slots.into(),
            scope: crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
                program: Some(program.raw()),
                root_eval_scope: false,
                binding_site_pc: None,
                dynamic_bindings: Vec::new().into(),
                with_objects: Box::default(),
            }),
        });
        self.programs.set_module_environment(program, environment);

        // Each name's first function declared directly in the module body.
        let mut root_functions = FxHashMap::default();
        for (id, function) in p.functions.iter().enumerate().skip(1) {
            let header = function.header();
            if header.parent == Some(super::ROOT_FUNCTION_ID)
                && let Some(name) = header.name
            {
                root_functions.entry(name).or_insert(id as u32);
            }
        }
        let hoisted_functions = p
            .module_link_plan
            .as_ref()
            .into_iter()
            .flat_map(|plan| plan.hoisted_functions.iter())
            .filter_map(|(binding, function_name)| {
                // Matched by atom id: the program's atoms are the VM's, so each name is hashed
                // once instead of every local's and function's text compared to it.
                let binding = self.lookup_atom(binding)?;
                let function_name = self.lookup_atom(function_name)?;
                let slot = *local_slots.get(&binding)?;
                Some((slot, *root_functions.get(&function_name)?))
            })
            .collect::<Vec<_>>();
        for (slot, function) in hoisted_functions {
            let previous = self.replace_active_program(program);
            let closure = self.closure(p, function, environment);
            self.set_active_program(previous);
            let closure = closure?;
            if let Some(binding) = self.heap.environment_slot_mut(environment, slot) {
                *binding = closure;
            }
        }
        Ok(())
    }

    fn missing_module_export(
        &mut self,
        p: &ResidualProgram,
        source: &str,
        name: &str,
    ) -> Result<Value, JsError> {
        self.syntax_error_result(
            p,
            &format!("The requested module '{source}' does not provide an export named '{name}'"),
        )
    }

    fn resolve_module_import_values(
        &mut self,
        p: &ResidualProgram,
        bindings: &ResidualProgram,
        referrer: &str,
        imports: &[crate::bytecode::ModuleImportBinding],
        active: &mut ActiveModuleExports,
    ) -> Result<Vec<(u16, crate::vm::program_store::ModuleImport)>, JsError> {
        let Some(root) = bindings.functions.first() else {
            return Ok(Vec::new());
        };
        let local_slots = local_slots(root);
        let mut values = Vec::with_capacity(imports.len());
        for import in imports {
            let module = &self
                .host_resolved_module(referrer, &import.source)
                .map_err(|error| self.module_load_error(p, error))?
                .ok_or_else(|| self.type_error(p, "module import was not resolved".into()))?;
            let local = self
                .lookup_atom(&import.local)
                .and_then(|atom| local_slots.get(&atom).copied())
                .and_then(|slot| u16::try_from(slot).ok())
                .ok_or_else(|| self.type_error(p, "module import slot is unavailable".into()))?;
            if import.phase == crate::bytecode::ModuleRequestPhase::Source {
                values.push((
                    local,
                    crate::vm::program_store::ModuleImport::Value(
                        self.module_source_value(p, &module)?,
                    ),
                ));
                continue;
            }
            let module_type = import.module_type.as_deref().unwrap_or("javascript");
            let key = module_cache_key(&module.name, module_type);
            let outcome = self
                .realm
                .promise
                .modules
                .get(&key)
                .map(|record| record.outcome);
            let deferred_namespace = self
                .realm
                .promise
                .modules
                .get(&key)
                .and_then(ModuleRecord::deferred_namespace);
            let pending_namespace = self
                .realm
                .promise
                .modules
                .get(&key)
                .and_then(ModuleRecord::pending_namespace);
            let namespace = if import.phase == crate::bytecode::ModuleRequestPhase::Defer {
                deferred_namespace.or_else(|| match outcome {
                    Some(
                        ModuleOutcome::Evaluated(namespace) | ModuleOutcome::Deferred(namespace),
                    ) => Some(namespace),
                    _ => None,
                })
            } else {
                match outcome {
                    Some(
                        ModuleOutcome::Evaluated(namespace) | ModuleOutcome::Deferred(namespace),
                    ) => Some(namespace),
                    Some(ModuleOutcome::Pending(_)) if pending_namespace.is_some() => {
                        pending_namespace
                    }
                    Some(ModuleOutcome::Pending(_))
                        if self_import_referrer(referrer, &module.name) =>
                    {
                        self.pending_self_import_namespace(p, &module, &key, pending_namespace)?
                    }
                    Some(ModuleOutcome::Errored(reason)) => {
                        return Err(JsError::thrown(reason, "module import failed".into()));
                    }
                    None if self_import_referrer(referrer, &module.name) => {
                        match self.root_module_namespace(p, &module)? {
                            Some(namespace) => Some(namespace),
                            None => self.resolve_self_import_namespace(p, &module, active)?,
                        }
                    }
                    Some(ModuleOutcome::Pending(_)) | None => {
                        return Err(self.type_error(p, "module import was not evaluated".into()));
                    }
                }
            };
            let Some(namespace) = namespace else {
                if let crate::bytecode::ModuleImportName::Named(name) = &import.imported
                    && Self::static_module_declares_export(&module, name) == Some(false)
                {
                    return self
                        .missing_module_export(p, &import.source, name)
                        .map(|_| Vec::new());
                }
                return Err(self.type_error(p, "deferred module namespace is unavailable".into()));
            };
            let named_binding = match (&import.imported, module_type) {
                (crate::bytecode::ModuleImportName::Named(name), "javascript") => {
                    let atom = self.intern_atom(name);
                    self.object_data(namespace)
                        .and_then(|object| object.module_binding(atom))
                }
                _ => None,
            };
            let value = match (&import.phase, &import.imported) {
                (
                    crate::bytecode::ModuleRequestPhase::Defer,
                    crate::bytecode::ModuleImportName::Namespace,
                ) => namespace,
                (_, crate::bytecode::ModuleImportName::Namespace) => namespace,
                (_, crate::bytecode::ModuleImportName::Named(name))
                    if module_type == "json" && name != "default" =>
                {
                    return self
                        .missing_module_export(p, &import.source, name)
                        .map(|_| Vec::new());
                }
                (_, crate::bytecode::ModuleImportName::Named(name)) => {
                    let atom = self.intern_atom(name);
                    let is_namespace = self
                        .object_data(namespace)
                        .is_some_and(Object::is_module_namespace);
                    // A named import of a pending builtin export takes the member's value now.
                    let fallback = match self.own_property(namespace, atom) {
                        Some(value) => Some(self.resolve_pending_binding(p, value)?),
                        None => None,
                    };
                    if is_namespace && named_binding.is_none() && fallback.is_none() {
                        return self
                            .missing_module_export(p, &import.source, name)
                            .map(|_| Vec::new());
                    }
                    match (named_binding, fallback) {
                        (Some(_), Some(value)) => value,
                        (Some(_), None) => Value::UNDEFINED,
                        (None, _) => self.get_property(p, namespace, atom)?,
                    }
                }
            };
            let binding = match named_binding {
                Some((program, slot)) => {
                    crate::vm::program_store::ModuleImport::Binding(program, slot, value)
                }
                None => crate::vm::program_store::ModuleImport::Value(value),
            };
            values.push((local, binding));
        }
        Ok(values)
    }

    fn static_module_declares_export(module: &ModuleSource, name: &str) -> Option<bool> {
        crate::Engine::module_export_names(&module.source, &module.name)
            .map(|exports| exports.iter().any(|(_, exported)| exported == name))
    }

    fn resolve_self_import_namespace(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        active: &mut ActiveModuleExports,
    ) -> Result<Option<Value>, JsError> {
        match self.resolve_static_module_graph(p, module.clone(), active)? {
            StaticModuleGraph::Linked {
                exports,
                incomplete: false,
                ..
            } => self.module_namespace_from_static(exports).map(Some),
            StaticModuleGraph::Linked {
                incomplete: true, ..
            }
            | StaticModuleGraph::LinkError => self
                .syntax_error_result(p, "module export could not be resolved unambiguously")
                .map(Some),
            StaticModuleGraph::Unsupported => Ok(None),
        }
    }

    fn pending_self_import_namespace(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        key: &str,
        pending: Option<Value>,
    ) -> Result<Option<Value>, JsError> {
        if pending.is_some() {
            return Ok(pending);
        }
        let Some(namespace) = self.root_module_namespace(p, module)? else {
            return Ok(None);
        };
        if let Some(record) = self.realm.promise.modules.get_mut(key) {
            record.cache_pending_namespace(namespace);
        }
        Ok(Some(namespace))
    }

    fn evaluate_module_requests(
        &mut self,
        p: &ResidualProgram,
        referrer: &str,
        requests: &[crate::bytecode::ModuleRequest],
        active: &mut ModuleEvaluationStack,
    ) -> Result<(), JsError> {
        // Resolve a module's complete request list before evaluating any
        // dependency. Resolution is a host phase; evaluating a dependency
        // while later requests are still unresolved can expose that
        // dependency's link error in place of the host's resolution error.
        let resolved = requests
            .iter()
            .map(|request| {
                self.resolve_module_request(p, referrer, request)?
                    .ok_or_else(|| {
                        self.type_error(p, "static module request was not resolved".into())
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        // Source bindings are linked before any dependency can execute.
        for (request, module) in requests.iter().zip(&resolved) {
            if request.phase == crate::bytecode::ModuleRequestPhase::Source {
                self.module_source_value(p, module)?;
            }
        }
        for (request, module) in requests.iter().zip(resolved) {
            match request.phase {
                crate::bytecode::ModuleRequestPhase::Evaluation => {
                    let identity =
                        crate::module_identity::normalize(std::path::Path::new(&module.name));
                    if let Some((root, cycle)) =
                        self.async_cycle_merge_for_dependency(&identity, active)
                    {
                        self.merge_async_cycle_root(&root, &cycle);
                    }
                    self.evaluate_static_module_request(p, request, module, active)?;
                }
                crate::bytecode::ModuleRequestPhase::Defer => {
                    self.prepare_deferred_module_request(
                        p,
                        module,
                        request.module_type.as_deref().unwrap_or("javascript"),
                    )?;
                }
                crate::bytecode::ModuleRequestPhase::Source => {}
            }
        }
        Ok(())
    }

    fn async_cycle_merge_for_dependency(
        &self,
        dependency: &ModuleIdentity,
        active: &ModuleEvaluationStack,
    ) -> Option<(ModuleIdentity, Vec<ModuleIdentity>)> {
        if let Some(cycle) = active.cycle_to(dependency) {
            return Some((cycle.first()?.clone(), cycle.to_vec()));
        }
        let key = module_cache_key(&dependency.text(), "javascript");
        let dependency_root = self
            .realm
            .promise
            .modules
            .get(&key)?
            .async_cycle_root()?
            .clone();
        let start = active.members().iter().position(|member| {
            let key = module_cache_key(&member.text(), "javascript");
            self.realm
                .promise
                .modules
                .get(&key)
                .and_then(ModuleRecord::async_cycle_root)
                .is_some_and(|root| root == &dependency_root)
        })?;
        let cycle = active.members()[start..].to_vec();
        let extends_component = cycle.iter().any(|member| {
            let key = module_cache_key(&member.text(), "javascript");
            self.realm
                .promise
                .modules
                .get(&key)
                .and_then(ModuleRecord::async_cycle_root)
                .is_none_or(|root| root != &dependency_root)
        });
        extends_component.then_some((dependency_root, cycle))
    }

    fn merge_async_cycle_root(&mut self, root: &ModuleIdentity, cycle: &[ModuleIdentity]) {
        let previous_roots = cycle
            .iter()
            .filter_map(|member| {
                let key = module_cache_key(&member.text(), "javascript");
                self.realm
                    .promise
                    .modules
                    .get(&key)
                    .and_then(ModuleRecord::async_cycle_root)
                    .cloned()
            })
            .collect::<FxHashSet<_>>();
        let root = root.clone();
        for record in self.realm.promise.modules.values_mut() {
            if record
                .async_cycle_root()
                .is_some_and(|previous| previous_roots.contains(previous))
            {
                record.set_async_cycle_root(root.clone());
            }
        }
        for member in cycle {
            let key = module_cache_key(&member.text(), "javascript");
            if let Some(record) = self.realm.promise.modules.get_mut(&key) {
                record.set_async_cycle_root(root.clone());
            }
        }
    }

    /// Link a deferred request: allocate its deferred namespace without
    /// evaluating the requested module or any of its dependencies.
    fn link_deferred_module_request(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        module_type: &str,
    ) -> Result<(), JsError> {
        if module_type != "javascript" {
            self.deferred_synthetic_module_namespace(p, module, module_type)?;
            return Ok(());
        }
        let key = module_cache_key(&module.name, "javascript");
        let record = self.realm.promise.modules.get(&key);
        if record.and_then(ModuleRecord::deferred_namespace).is_some() {
            return Ok(());
        }
        if record.is_none()
            && let Some(message) = self.static_module_early_error(&module.source, &module.name)
        {
            return self.syntax_error_result(p, &message).map(|_| ());
        }
        let namespace = self.deferred_module_namespace(p, module)?;
        self.realm
            .promise
            .modules
            .entry(key)
            .or_insert_with(ModuleRecord::loading_static)
            .cache_deferred_namespace(namespace);
        Ok(())
    }

    /// InnerModuleEvaluation's deferred-request step: link the request, then
    /// evaluate the asynchronous transitive dependencies it gathers, in order
    /// with the referrer's other requests.
    fn prepare_deferred_module_request(
        &mut self,
        p: &ResidualProgram,
        module: ModuleSource,
        module_type: &str,
    ) -> Result<(), JsError> {
        self.link_deferred_module_request(p, &module, module_type)?;
        if module_type != "javascript" {
            return Ok(());
        }
        let key = module_cache_key(&module.name, "javascript");
        let mut seen = FxHashSet::default();
        let mut asynchronous = Vec::new();
        self.gather_async_transitive_dependencies(p, &module, &mut seen, &mut asynchronous)?;
        if !asynchronous.is_empty() {
            self.evaluate_deferred_async_dependencies(p, asynchronous)?;
        }
        if let Some(record) = self.realm.promise.modules.get_mut(&key) {
            record.defer_evaluation();
        }
        Ok(())
    }

    fn evaluate_deferred_async_dependencies(
        &mut self,
        p: &ResidualProgram,
        asynchronous: Vec<ModuleSource>,
    ) -> Result<(), JsError> {
        let mut active = ModuleEvaluationStack::default();
        let outer_batch = std::mem::replace(&mut self.deferred_dependency_batch, true);
        let launched = (|| {
            for dependency in asynchronous {
                self.evaluate_static_module_source(p, dependency, &mut active)?;
            }
            Ok::<(), JsError>(())
        })();
        self.deferred_dependency_batch = outer_batch;
        launched?;
        if !outer_batch {
            self.drain_jobs(p)?;
            self.settle_pending_async_modules(p)?;
        }
        Ok(())
    }

    fn deferred_synthetic_module_namespace(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        module_type: &str,
    ) -> Result<Value, JsError> {
        let key = module_cache_key(&module.name, module_type);
        if let Some(namespace) = self
            .realm
            .promise
            .modules
            .get(&key)
            .and_then(ModuleRecord::deferred_namespace)
        {
            return Ok(namespace);
        }
        let namespace = self.evaluate_static_synthetic_module(p, module, module_type)?;
        let default_atom = self.intern_atom("default");
        let default_value = self
            .own_property(namespace, default_atom)
            .ok_or_else(|| self.type_error(p, "synthetic module has no default export".into()))?;
        let deferred = self.module_namespace_with_tag(
            vec![("default".into(), default_value)],
            "Deferred Module",
        )?;
        let Some(record) = self.realm.promise.modules.get_mut(&key) else {
            return Err(self.type_error(p, "synthetic module record is unavailable".into()));
        };
        record.cache_deferred_namespace(deferred);
        Ok(deferred)
    }

    fn cached_deferred_javascript_namespace(
        &mut self,
        p: &ResidualProgram,
        key: &str,
        module: &ModuleSource,
    ) -> Result<Value, JsError> {
        if let Some(namespace) = self
            .realm
            .promise
            .modules
            .get(key)
            .and_then(ModuleRecord::deferred_namespace)
        {
            return Ok(namespace);
        }
        let namespace = self.deferred_module_namespace(p, module)?;
        self.realm
            .promise
            .modules
            .get_mut(key)
            .expect("module record found above")
            .cache_deferred_namespace(namespace);
        Ok(namespace)
    }

    /// Resolve a static module request and, for an evaluation, validate its type.
    /// The module the host loads for `specifier` from `referrer`, resolved once until the next
    /// collection (`Vm::resolved_module_requests`). A failed load is not kept.
    pub(super) fn host_resolved_module(
        &mut self,
        referrer: &str,
        specifier: &str,
    ) -> Result<Option<ModuleSource>, crate::ModuleLoadError> {
        let key = (referrer.to_owned(), specifier.to_owned());
        if let Some(module) = self.resolved_module_requests.get(&key) {
            return Ok(Some(module.clone()));
        }
        let module = self.host.resolve_dynamic_import(referrer, specifier)?;
        if let Some(module) = &module {
            self.resolved_module_requests.insert(key, module.clone());
        }
        Ok(module)
    }

    fn resolve_module_request(
        &mut self,
        p: &ResidualProgram,
        referrer: &str,
        request: &crate::bytecode::ModuleRequest,
    ) -> Result<Option<ModuleSource>, JsError> {
        let module = self
            .host_resolved_module(referrer, &request.source)
            .map_err(|error| self.module_load_error(p, error))?;
        if let Some(module) = &module
            && request.phase == crate::bytecode::ModuleRequestPhase::Evaluation
        {
            let requested = request
                .module_type
                .as_deref()
                .unwrap_or(JAVASCRIPT_MODULE_TYPE);
            self.host
                .validate_module_type(module, requested)
                .map_err(|error| self.module_load_error(p, error))?;
        }
        Ok(module)
    }

    fn evaluate_static_module_request(
        &mut self,
        p: &ResidualProgram,
        request: &crate::bytecode::ModuleRequest,
        module: ModuleSource,
        active: &mut ModuleEvaluationStack,
    ) -> Result<(), JsError> {
        match request.module_type.as_deref().unwrap_or("javascript") {
            "javascript" => self.evaluate_static_module_source(p, module, active),
            module_type @ ("json" | "text" | "bytes") => {
                self.evaluate_static_synthetic_module(p, &module, module_type)?;
                Ok(())
            }
            _ => Err(self.type_error(p, "unsupported static module type attribute".into())),
        }
    }

    fn evaluate_static_synthetic_module(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        module_type: &str,
    ) -> Result<Value, JsError> {
        let key = module_cache_key(&module.name, module_type);
        if let Some(outcome) = self
            .realm
            .promise
            .modules
            .get(&key)
            .map(|record| record.outcome)
        {
            return match outcome {
                ModuleOutcome::Evaluated(namespace) | ModuleOutcome::Deferred(namespace) => {
                    Ok(namespace)
                }
                ModuleOutcome::Errored(reason) => Err(JsError::thrown(
                    reason,
                    "synthetic module evaluation failed".into(),
                )),
                ModuleOutcome::Pending(_) => Err(self.type_error(
                    p,
                    "synthetic module is not ready for synchronous evaluation".into(),
                )),
            };
        }
        let namespace = self.evaluate_dynamic_module(
            p,
            module,
            module_type,
            crate::bytecode::ModuleRequestPhase::Evaluation,
        )?;
        self.realm
            .promise
            .modules
            .insert(key, ModuleRecord::materialized(namespace));
        Ok(namespace)
    }

    /// Allocate every reachable environment and namespace before linking imports.
    /// The temporary graph owns traversal order only; each ModuleRecord owns its
    /// program and namespace throughout linking, evaluation, and subsequent imports.
    fn instantiate_static_module_graph(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        precompiled: Option<ProgramId>,
    ) -> Result<Vec<(ModuleSource, ProgramId)>, JsError> {
        let mut graph = Vec::new();
        self.collect_static_module_programs(p, module, precompiled, &mut graph)?;
        let mut nodes = StaticModuleNodes::default();
        for (module, _) in &graph {
            let Some(StaticModuleGraph::Linked {
                exports,
                incomplete: false,
                ..
            }) = self.resolve_static_module_plan_graph_with_nodes(p, module.clone(), &mut nodes)?
            else {
                return self
                    .syntax_error_result(p, "module export could not be resolved unambiguously")
                    .map(|_| Vec::new());
            };
            let linked = self.module_namespace_from_static(exports)?;
            let key = module_cache_key(&module.name, "javascript");
            let namespace = self
                .realm
                .promise
                .modules
                .get(&key)
                .and_then(ModuleRecord::pending_namespace)
                .ok_or_else(|| {
                    self.type_error(p, "module namespace is unavailable during linking".into())
                })?;
            self.copy_module_namespace(linked, namespace, p)?;
        }
        for (module, program) in &graph {
            let residual = self
                .programs
                .get(*program)
                .ok_or_else(|| self.type_error(p, "module program is unavailable".into()))?;
            for request in &residual.module_requests {
                let module_type = request.module_type.as_deref().unwrap_or("javascript");
                if request.phase == crate::bytecode::ModuleRequestPhase::Evaluation
                    && module_type == "javascript"
                {
                    continue;
                }
                let dependency = self
                    .resolve_module_request(p, &module.name, request)?
                    .ok_or_else(|| {
                        self.type_error(p, "static module request was not resolved".into())
                    })?;
                match request.phase {
                    crate::bytecode::ModuleRequestPhase::Source => {
                        self.module_source_value(p, &dependency)?;
                    }
                    crate::bytecode::ModuleRequestPhase::Defer => {
                        self.link_deferred_module_request(p, &dependency, module_type)?;
                    }
                    crate::bytecode::ModuleRequestPhase::Evaluation => {
                        self.evaluate_static_synthetic_module(p, &dependency, module_type)?;
                    }
                }
            }
            let imports = self.resolve_module_import_values(
                p,
                &residual,
                &module.name,
                &residual.module_imports,
                &mut ActiveModuleExports::default(),
            )?;
            self.programs.set_module_imports(*program, imports);
        }
        for (module, _) in &graph {
            let key = module_cache_key(&module.name, "javascript");
            if let Some(record) = self.realm.promise.modules.get_mut(&key) {
                record.begin_linking();
            }
        }
        Ok(graph)
    }

    /// `Engine::static_module_analysis` of a module, kept across runs in the host's compile
    /// cache for its exact text, as its compiled program is: a process otherwise parses every
    /// static module it links once more than it compiles it.
    pub(super) fn static_module_analysis(
        &mut self,
        source: &str,
        name: &str,
    ) -> Rc<crate::compile::StaticModuleAnalysis> {
        let host = &mut self.host;
        crate::Engine::static_module_analysis_with(source, name, |analyze| {
            let parts = [STATIC_MODULE_ANALYSIS_GOAL, source];
            if let Some(analysis) = host.kept_compiled_program(name, &parts).and_then(|bytes| {
                crate::bytecode::decode_static_module_analysis((*bytes).as_ref()).ok()
            }) {
                return analysis;
            }
            let analysis = analyze();
            host.keep_compiled_program(
                name,
                &parts,
                &crate::bytecode::encode_static_module_analysis(&analysis),
            );
            analysis
        })
    }

    fn static_module_plan(
        &mut self,
        source: &str,
        name: &str,
    ) -> Option<crate::compile::StaticModulePlan> {
        self.static_module_analysis(source, name).plan.clone()
    }

    fn static_module_early_error(&mut self, source: &str, name: &str) -> Option<String> {
        self.static_module_analysis(source, name)
            .early_error
            .clone()
    }

    /// A static module's program kept by the host's compile cache
    /// (`Host::kept_compiled_program`) for its exact text: compiled as this VM specializes,
    /// after its early errors were ruled out, so neither repeats.
    fn kept_static_module_program(&mut self, module: &ModuleSource) -> Option<ResidualProgram> {
        let bytes = self.host.kept_compiled_program(
            &module.name,
            &[
                self.specialization_part(),
                STATIC_MODULE_GOAL,
                &module.source,
            ],
        )?;
        let text = crate::bytecode::SourceText::Shared(module.source.clone());
        let specialized = self.specialized;
        self.decode_cached_program(bytes, text, 0, &module.name, specialized)
            .ok()
    }

    /// Offers the host the compiled program of a static module (`kept_static_module_program`).
    fn keep_static_module_program(&mut self, module: &ModuleSource, program: ProgramId) {
        if let Some(residual) = self.programs.get(program)
            && let Some(bytes) = self.encode_cached_program(&residual)
        {
            self.host.keep_compiled_program(
                &module.name,
                &[
                    self.specialization_part(),
                    STATIC_MODULE_GOAL,
                    &module.source,
                ],
                &bytes,
            );
        }
    }

    fn collect_static_module_programs(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        precompiled: Option<ProgramId>,
        graph: &mut Vec<(ModuleSource, ProgramId)>,
    ) -> Result<(), JsError> {
        let key = module_cache_key(&module.name, "javascript");
        let existing = self.realm.promise.modules.get(&key);
        if existing.is_some_and(|record| {
            matches!(
                record.outcome,
                ModuleOutcome::Evaluated(_)
                    | ModuleOutcome::Deferred(_)
                    | ModuleOutcome::Errored(_)
            ) || record.program.is_some_and(|program| {
                record.phase() != ModulePhase::Loading
                    || graph.iter().any(|(_, visited)| *visited == program)
            })
        }) {
            return Ok(());
        }
        let precompiled = precompiled.or_else(|| existing.and_then(|record| record.program));
        let _stack =
            quench_stack::StackGuard::enter().map_err(|()| self.stack_exhaustion_error())?;
        let program = match precompiled {
            Some(program) => program,
            None if let Some(program) = self.kept_static_module_program(module) => self
                .store_module_program(program)
                .ok_or_else(|| self.type_error(p, "module program store is full".into()))?,
            None => {
                if let Some(message) = self.static_module_early_error(&module.source, &module.name)
                {
                    return self.syntax_error_result(p, &message).map(|_| ());
                }
                let residual = crate::Engine::specialize_module_with_atom_prefix(
                    &module.source,
                    &module.name,
                    &*self,
                )
                .map_err(|diagnostics| {
                    if diagnostics
                        .iter()
                        .any(crate::compile::Diagnostic::is_stack_exhausted)
                    {
                        self.stack_exhaustion_error()
                    } else {
                        self.type_error(
                            p,
                            format!("dynamic module compilation failed: {diagnostics:?}"),
                        )
                    }
                })?;
                let program = self
                    .store_module_program(residual)
                    .ok_or_else(|| self.type_error(p, "module program store is full".into()))?;
                self.keep_static_module_program(module, program);
                program
            }
        };
        let residual = self
            .programs
            .get(program)
            .ok_or_else(|| self.type_error(p, "module program is unavailable".into()))?;
        self.instantiate_module_environment(&residual, program)?;
        let namespace = match self
            .realm
            .promise
            .modules
            .get(&key)
            .and_then(ModuleRecord::pending_namespace)
        {
            Some(namespace) => namespace,
            None => self.module_namespace(Vec::new())?,
        };
        let record = self
            .realm
            .promise
            .modules
            .entry(key)
            .or_insert_with(ModuleRecord::loading_static);
        record.program = Some(program);
        record.cache_pending_namespace(namespace);
        // Graph instantiation can allocate many environments before the first
        // guest instruction. The record and program store now own all roots.
        self.maybe_collect(p);
        graph.push((module.clone(), program));
        let dependencies = residual
            .module_requests
            .iter()
            .map(|request| {
                self.resolve_module_request(p, &module.name, request)?
                    .ok_or_else(|| {
                        self.type_error(p, "static module request was not resolved".into())
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.realm
            .promise
            .modules
            .get_mut(&module_cache_key(&module.name, "javascript"))
            .expect("module program registered before resolution")
            .resolved_requests = dependencies
            .iter()
            .map(|dependency| dependency.name.clone())
            .collect();
        for (request, dependency) in residual.module_requests.iter().zip(dependencies) {
            if request.phase == crate::bytecode::ModuleRequestPhase::Evaluation
                && request.module_type.as_deref().unwrap_or("javascript") == "javascript"
            {
                self.collect_static_module_programs(p, &dependency, None, graph)?;
            }
        }
        Ok(())
    }

    fn evaluate_static_module_source(
        &mut self,
        p: &ResidualProgram,
        module: ModuleSource,
        active: &mut ModuleEvaluationStack,
    ) -> Result<(), JsError> {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if active.contains(&identity) {
            return Ok(());
        }
        let cache_key = module_cache_key(&module.name, "javascript");
        if self
            .realm
            .promise
            .modules
            .get(&cache_key)
            .is_none_or(|record| record.phase() == ModulePhase::Loading)
        {
            self.realm
                .promise
                .modules
                .entry(cache_key.clone())
                .or_insert_with(ModuleRecord::loading_static);
            if let Err(error) = self.instantiate_static_module_graph(p, &module, None) {
                return self.reject_module_instantiation(p, &cache_key, error);
            }
        }
        let resuming_waiting = matches!(
            self.realm
                .promise
                .modules
                .get(&cache_key)
                .map(|record| record.outcome),
            Some(ModuleOutcome::Pending(ModulePhase::WaitingForDependencies))
        );
        match self
            .realm
            .promise
            .modules
            .get(&cache_key)
            .map(|record| record.outcome)
        {
            Some(ModuleOutcome::Evaluated(_)) => return Ok(()),
            Some(ModuleOutcome::Pending(ModulePhase::Linking | ModulePhase::Instantiated)) => {
                self.realm
                    .promise
                    .modules
                    .get_mut(&cache_key)
                    .unwrap()
                    .begin_evaluation();
            }

            Some(ModuleOutcome::Pending(ModulePhase::WaitingForDependencies)) => {
                if self.static_module_has_pending_dependencies(p, &module)? {
                    return Ok(());
                }
                if !self
                    .realm
                    .promise
                    .modules
                    .get_mut(&cache_key)
                    .is_some_and(ModuleRecord::begin_after_dependencies)
                {
                    return Ok(());
                }
            }
            Some(ModuleOutcome::Pending(_)) => return Ok(()),
            Some(ModuleOutcome::Deferred(namespace)) => {
                self.evaluate_deferred_module_namespace(p, namespace)?;
                return Ok(());
            }
            Some(ModuleOutcome::Errored(reason)) => {
                return Err(JsError::thrown(
                    reason,
                    "static module evaluation failed".into(),
                ));
            }
            None => {}
        }
        if let Some(message) = self.static_module_early_error(&module.source, &module.name) {
            return self.syntax_error_result(p, &message).map(|_| ());
        }
        let Some(plan) = self.static_module_plan(&module.source, &module.name) else {
            return Err(self.type_error(p, "static module metadata is unavailable".into()));
        };
        if !resuming_waiting && !self.realm.promise.modules.contains_key(&cache_key) {
            self.realm
                .promise
                .modules
                .insert(cache_key.clone(), ModuleRecord::evaluating_static());
        }
        active.enter(identity.clone());
        let result = self.evaluate_static_module_body(p, &module, &plan, active);
        active.leave(&identity);
        match result {
            Ok(Some(namespace)) => self.settle_static_module(p, &cache_key, Ok(namespace)),
            Ok(None) => {
                let waiting = self
                    .realm
                    .promise
                    .modules
                    .get_mut(&cache_key)
                    .is_some_and(ModuleRecord::wait_for_dependencies);
                if waiting {
                    self.realm.promise.waiting_static_modules.push(module);
                }
                Ok(())
            }
            Err(error) => self.settle_static_module(p, &cache_key, Err(error)),
        }
    }

    fn evaluate_static_module_body(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        plan: &crate::compile::StaticModulePlan,
        active: &mut ModuleEvaluationStack,
    ) -> Result<Option<Value>, JsError> {
        self.evaluate_module_requests(p, &module.name, &plan.requests, active)?;
        if self.static_module_has_pending_dependencies(p, module)? {
            return Ok(None);
        }
        self.evaluate_javascript_module_body(p, module).map(Some)
    }

    fn settle_static_module(
        &mut self,
        p: &ResidualProgram,
        cache_key: &str,
        result: Result<Value, JsError>,
    ) -> Result<(), JsError> {
        let (state, value) = match result {
            Ok(namespace) => (PromiseState::Fulfilled, namespace),
            Err(error) => (
                PromiseState::Rejected,
                error
                    .thrown_value()
                    .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.to_string()))),
            ),
        };
        if state == PromiseState::Fulfilled {
            self.refresh_static_module_bindings(p, value)?;
        }
        if state == PromiseState::Fulfilled
            && self
                .realm
                .promise
                .modules
                .get(cache_key)
                .and_then(ModuleRecord::evaluation_promise)
                .is_some_and(|promise| {
                    self.promise_record(promise)
                        .is_some_and(|record| record.state == PromiseState::Pending)
                })
        {
            let Some(record) = self.realm.promise.modules.get_mut(cache_key) else {
                return Err(
                    self.type_error(p, "module record disappeared during evaluation".into())
                );
            };
            record.begin_async_evaluation(value);
            self.realm
                .promise
                .async_module_order
                .push_back(cache_key.to_owned());
            return Ok(());
        }
        let deferred_namespace = self
            .realm
            .promise
            .modules
            .get(cache_key)
            .and_then(ModuleRecord::deferred_namespace);
        if state == PromiseState::Fulfilled
            && let Some(namespace) = deferred_namespace
        {
            self.copy_module_namespace(value, namespace, p)?;
        }
        let Some(record) = self.realm.promise.modules.get_mut(cache_key) else {
            return Err(self.type_error(p, "module record disappeared during evaluation".into()));
        };
        let waiters = match state {
            PromiseState::Fulfilled => record.evaluate(value),
            PromiseState::Rejected => record.fail(value),
            PromiseState::Pending => None,
        };
        let Some(waiters) = waiters else {
            return Err(self.type_error(p, "module record was not evaluating".into()));
        };
        self.settle_module_waiters(p, cache_key, waiters, state, value)
    }

    fn reject_module_instantiation(
        &mut self,
        p: &ResidualProgram,
        cache_key: &str,
        error: JsError,
    ) -> Result<(), JsError> {
        // A source rejected before its record exists (an early error) has nothing to retry.
        let Some(record) = self.realm.promise.modules.get_mut(cache_key) else {
            return Err(error);
        };
        let waiters = record
            .retry_instantiation()
            .ok_or_else(|| self.type_error(p, "module was not being instantiated".into()))?;
        // Queued imports own their promises outside the module record. Return
        // the transient error to that job instead of leaving it waiting on an
        // unlinked record that deliberately does not cache the error.
        if waiters.is_empty() {
            return Err(error);
        }
        let reason = error
            .thrown_value()
            .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
        self.settle_module_waiters(p, cache_key, waiters, PromiseState::Rejected, reason)
    }

    fn settle_module_waiters(
        &mut self,
        p: &ResidualProgram,
        cache_key: &str,
        waiters: Vec<Value>,
        state: PromiseState,
        value: Value,
    ) -> Result<(), JsError> {
        self.with_call_roots(waiters.iter().copied().chain([value]), |vm| {
            let rejection_is_observed = state != PromiseState::Rejected
                || !waiters.is_empty()
                || vm.waiting_static_parent(p, cache_key)?;
            for &waiter in &waiters {
                vm.promise_settle(p, waiter, state, value)?;
            }
            if state == PromiseState::Rejected && !rejection_is_observed {
                return Err(JsError::thrown(
                    value,
                    "static module evaluation failed".into(),
                ));
            }
            Ok(())
        })
    }

    fn waiting_static_parent(
        &mut self,
        p: &ResidualProgram,
        dependency_key: &str,
    ) -> Result<bool, JsError> {
        let waiting = self.realm.promise.waiting_static_modules.clone();
        for parent in waiting {
            let Some(plan) = self.static_module_plan(&parent.source, &parent.name) else {
                return Err(self.type_error(p, "static module metadata is unavailable".into()));
            };
            for request in plan.requests.iter().filter(|request| {
                request.phase == crate::bytecode::ModuleRequestPhase::Evaluation
                    && request.module_type.as_deref().unwrap_or("javascript") == "javascript"
            }) {
                let dependency = self
                    .resolve_module_request(p, &parent.name, request)?
                    .ok_or_else(|| {
                        self.type_error(p, "static module request was not resolved".into())
                    })?;
                if module_cache_key(&dependency.name, "javascript") == dependency_key {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn refresh_static_module_bindings(
        &mut self,
        p: &ResidualProgram,
        namespace: Value,
    ) -> Result<(), JsError> {
        let bindings = self
            .object_data(namespace)
            .map(|object| object.module_bindings().to_vec())
            .unwrap_or_default();
        for (atom, program, slot) in bindings {
            let Some(environment) = self.programs.module_environment(program) else {
                continue;
            };
            let value = self
                .heap
                .environment_slot(environment, usize::from(slot))
                .ok_or_else(|| self.type_error(p, "module export slot is unavailable".into()))?;
            let property = self
                .object_data(namespace)
                .and_then(|object| self.shape_slot(object.shape(), atom))
                .ok_or_else(|| {
                    self.type_error(p, "module export property is unavailable".into())
                })?;
            self.heap.property_set(namespace, property, value);
            self.invalidate_method_caches_for_atom(atom);
        }
        Ok(())
    }

    /// Whether module advancement after a job has work: the queues the advancement drains.
    pub(super) fn module_jobs_pending(&self) -> bool {
        let promise = &self.realm.promise;
        !promise.dynamic_import_jobs.is_empty()
            || !promise.waiting_static_modules.is_empty()
            || promise.async_module_settled
    }

    /// Marks `promise` as one a module's async evaluation waits on; module advancement
    /// becomes due when it settles, or now if it already has.
    fn watch_module_evaluation(&mut self, promise: Value) {
        let settled = self.promise_record_mut(promise).is_none_or(|record| {
            record.module_evaluation = true;
            record.state != PromiseState::Pending
        });
        self.realm.promise.async_module_settled |= settled;
    }

    pub(super) fn advance_static_module_jobs(
        &mut self,
        p: &ResidualProgram,
    ) -> Result<(), JsError> {
        self.settle_pending_async_modules(p)?;
        let waiting = std::mem::take(&mut self.realm.promise.waiting_static_modules);
        for module in waiting {
            if self.static_module_has_pending_dependencies(p, &module)? {
                self.realm.promise.waiting_static_modules.push(module);
                continue;
            }
            let cache_key = module_cache_key(&module.name, "javascript");
            let Some(record) = self.realm.promise.modules.get(&cache_key) else {
                continue;
            };
            if record.phase() != ModulePhase::WaitingForDependencies {
                continue;
            }
            let mut active = ModuleEvaluationStack::default();
            let outer_batch = std::mem::replace(&mut self.deferred_dependency_batch, true);
            let result = self.evaluate_static_module_source(p, module, &mut active);
            self.deferred_dependency_batch = outer_batch;
            result?;
        }
        Ok(())
    }

    /// Async-evaluating modules whose evaluation promise has settled: the ones module
    /// advancement completes, as (key, promise, namespace).
    fn settled_async_modules(&self) -> impl Iterator<Item = (&String, Value, Value)> + '_ {
        self.realm
            .promise
            .async_module_order
            .iter()
            .filter_map(|key| {
                let module = self.realm.promise.modules.get(key)?;
                if module.phase() != ModulePhase::EvaluatingAsync {
                    return None;
                }
                let promise = module.evaluation_promise()?;
                let settled = self
                    .promise_record(promise)
                    .is_some_and(|record| record.state != PromiseState::Pending);
                settled.then_some((key, promise, module.pending_namespace()?))
            })
    }

    fn settle_pending_async_modules(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        if !std::mem::take(&mut self.realm.promise.async_module_settled) {
            return Ok(());
        }
        let completed = self.complete_settled_async_modules(p);
        // A failed completion leaves the rest due.
        self.realm.promise.async_module_settled |= completed.is_err();
        completed
    }

    fn complete_settled_async_modules(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        let settled = self
            .settled_async_modules()
            .map(|(key, promise, namespace)| (key.clone(), promise, namespace))
            .collect::<Vec<_>>();
        for (key, promise, namespace) in settled {
            let Some(record) = self.promise_record(promise).cloned() else {
                continue;
            };
            match record.state {
                PromiseState::Pending => {}
                PromiseState::Fulfilled => self.settle_static_module(p, &key, Ok(namespace))?,
                PromiseState::Rejected => self.settle_static_module(
                    p,
                    &key,
                    Err(JsError::thrown(
                        record.result,
                        "module evaluation rejected".into(),
                    )),
                )?,
            }
        }
        self.realm.promise.async_module_order.retain(|key| {
            self.realm
                .promise
                .modules
                .get(key)
                .is_some_and(|module| module.phase() == ModulePhase::EvaluatingAsync)
        });
        Ok(())
    }

    fn static_module_has_pending_dependencies(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
    ) -> Result<bool, JsError> {
        let Some(plan) = self.static_module_plan(&module.source, &module.name) else {
            return Err(self.type_error(p, "static module metadata is unavailable".into()));
        };
        for request in plan
            .requests
            .iter()
            .filter(|request| request.phase == crate::bytecode::ModuleRequestPhase::Evaluation)
        {
            if request.module_type.as_deref().unwrap_or("javascript") != "javascript" {
                continue;
            }
            let dependency = self
                .resolve_module_request(p, &module.name, request)?
                .ok_or_else(|| {
                    self.type_error(p, "static module request was not resolved".into())
                })?;
            let dependency_key = module_cache_key(&dependency.name, "javascript");
            let module_identity =
                crate::module_identity::normalize(std::path::Path::new(&module.name));
            let dependency_state = self
                .realm
                .promise
                .modules
                .get(&dependency_key)
                .map(|record| (record.phase(), record.async_cycle_root().cloned()));
            let Some((phase, cycle_root)) = dependency_state else {
                continue;
            };
            if cycle_root.as_ref() == Some(&module_identity)
                && phase == ModulePhase::WaitingForDependencies
            {
                continue;
            }
            if matches!(
                phase,
                ModulePhase::EvaluatingAsync | ModulePhase::WaitingForDependencies
            ) {
                return Ok(true);
            }
            let cycle_root_phase = cycle_root
                .as_ref()
                .filter(|root| **root != module_identity)
                .and_then(|root| {
                    self.realm
                        .promise
                        .modules
                        .get(&module_cache_key(&root.text(), "javascript"))
                })
                .map(ModuleRecord::phase);
            if matches!(
                cycle_root_phase,
                Some(ModulePhase::EvaluatingAsync | ModulePhase::WaitingForDependencies)
            ) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn evaluate_module_locals(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        exports: Vec<(String, String)>,
        active: &mut ActiveModuleExports,
    ) -> Result<Vec<(String, StaticModuleValue)>, JsError> {
        let key = module_cache_key(&module.name, "javascript");
        let program_id = match self
            .realm
            .promise
            .modules
            .get(&key)
            .and_then(|record| record.program)
        {
            Some(program) => program,
            None => {
                let residual = crate::Engine::specialize_module_with_atom_prefix(
                    &module.source,
                    &module.name,
                    &*self,
                )
                .map_err(|diagnostics| {
                    if diagnostics
                        .iter()
                        .any(crate::compile::Diagnostic::is_stack_exhausted)
                    {
                        return self.stack_exhaustion_error();
                    }
                    self.type_error(
                        p,
                        format!("dynamic module compilation failed: {diagnostics:?}"),
                    )
                })?;
                let Some(program_id) = self.store_module_program(residual) else {
                    return Err(self.type_error(p, "module program store is full".into()));
                };
                program_id
            }
        };
        let residual = self
            .programs
            .get(program_id)
            .ok_or_else(|| self.type_error(p, "module program is unavailable".into()))?;
        let imports = self.resolve_module_import_values(
            p,
            &residual,
            &module.name,
            &residual.module_imports,
            active,
        )?;
        let mut source_import_values = FxHashMap::default();
        for import in residual
            .module_imports
            .iter()
            .filter(|import| import.phase == crate::bytecode::ModuleRequestPhase::Source)
        {
            let atom = residual
                .atoms
                .iter()
                .position(|name| name == &import.local)
                .ok_or_else(|| self.type_error(p, "source import binding is unavailable".into()))?
                as u32;
            let slot = residual
                .function(0)
                .local_atoms
                .iter()
                .position(|candidate| *candidate == atom)
                .and_then(|slot| u16::try_from(slot).ok())
                .ok_or_else(|| self.type_error(p, "source import slot is unavailable".into()))?;
            let value = imports
                .iter()
                .find_map(
                    |(import_slot, import)| match (*import_slot == slot, import) {
                        (true, crate::vm::program_store::ModuleImport::Value(value)) => {
                            Some(*value)
                        }
                        (true, crate::vm::program_store::ModuleImport::Binding(_, _, _)) => None,
                        (false, _) => None,
                    },
                )
                .ok_or_else(|| self.type_error(p, "source import value is unavailable".into()))?;
            source_import_values.insert(import.local.clone(), value);
        }
        self.programs.set_module_imports(program_id, imports);
        let active_program = self.replace_active_program(program_id);
        let evaluated = (|| {
            let closure = self.closure(&residual, 0, Value::NULL)?;
            let evaluation = self.call_value(&residual, closure, Value::UNDEFINED, &[])?;
            if residual
                .functions
                .first()
                .is_some_and(|function| function.is_async)
            {
                let pending = self
                    .promise_record(evaluation)
                    .is_some_and(|record| record.state == PromiseState::Pending);
                if pending && self.deferred_dependency_batch {
                    let key = module_cache_key(&module.name, "javascript");
                    let Some(module_record) = self.realm.promise.modules.get_mut(&key) else {
                        return Err(self.type_error(
                            p,
                            "module record disappeared during asynchronous evaluation".into(),
                        ));
                    };
                    module_record.track_evaluation_promise(evaluation);
                    self.watch_module_evaluation(evaluation);
                } else if pending {
                    self.drain_jobs_until_promise(&residual, evaluation)?;
                }
                let Some(record) = self.promise_record(evaluation).cloned() else {
                    return Err(
                        self.type_error(p, "module evaluation promise is unavailable".into())
                    );
                };
                match record.state {
                    PromiseState::Fulfilled => {}
                    PromiseState::Rejected => {
                        return Err(JsError::thrown(
                            record.result,
                            "module evaluation rejected".into(),
                        ));
                    }
                    PromiseState::Pending if self.deferred_dependency_batch => {}
                    PromiseState::Pending => {
                        return Err(self.type_error(
                            p,
                            "top-level await did not complete during module evaluation".into(),
                        ));
                    }
                }
            }
            let mut values = Vec::with_capacity(exports.len());
            for (local, exported) in exports {
                let atom = residual
                    .atoms
                    .iter()
                    .position(|name| name == local)
                    .ok_or_else(|| {
                        self.type_error(p, "module export binding is unavailable".into())
                    })? as u32;
                let hoisted_function = residual
                    .functions
                    .iter()
                    .enumerate()
                    .find(|(_, function)| {
                        let header = function.header();
                        header.parent == Some(0)
                            && header
                                .name
                                .is_some_and(|name| residual.atoms[name as usize] == local)
                    })
                    .and_then(|(id, _)| self.cached_functions(program_id, id as u32).next_back());
                let slot = residual
                    .function(0)
                    .local_atoms
                    .iter()
                    .position(|candidate| *candidate == atom)
                    .and_then(|slot| u16::try_from(slot).ok())
                    .ok_or_else(|| {
                        self.type_error(p, "module export slot is unavailable".into())
                    })?;
                let value = match hoisted_function {
                    Some(value) => value,
                    None => {
                        let environment =
                            self.programs
                                .module_environment(program_id)
                                .ok_or_else(|| {
                                    self.type_error(p, "module environment is unavailable".into())
                                })?;
                        self.heap
                            .environment_slot(environment, slot as usize)
                            .ok_or_else(|| {
                                self.type_error(p, "module export slot is unavailable".into())
                            })?
                    }
                };
                let binding = match source_import_values.get(&local) {
                    Some(value) => StaticModuleValue::ModuleSource(*value),
                    None => StaticModuleValue::Binding {
                        program: program_id,
                        slot,
                        value,
                    },
                };
                values.push((exported, binding));
            }
            Ok(values)
        })();
        self.set_active_program(active_program);
        evaluated
    }

    fn evaluate_static_module_throw(
        &mut self,
        p: &ResidualProgram,
        thrown: crate::compile::StaticModuleThrow,
    ) -> Result<Value, JsError> {
        let reason = match thrown {
            crate::compile::StaticModuleThrow::Value(value) => self.module_static_value(value),
            crate::compile::StaticModuleThrow::Error { name, message } => {
                let kind = match name.as_str() {
                    "EvalError" => Native::EvalError,
                    "RangeError" => Native::RangeError,
                    "ReferenceError" => Native::ReferenceError,
                    "SyntaxError" => Native::SyntaxError,
                    "TypeError" => Native::TypeError,
                    "URIError" => Native::URIError,
                    _ => Native::Error,
                };
                let arguments = message
                    .map(|message| self.module_static_value(message))
                    .into_iter()
                    .collect::<Vec<_>>();
                self.construct_error_native(p, kind, &arguments)?
            }
        };
        Err(JsError::thrown(reason, "module evaluation threw".into()))
    }

    fn ensure_static_module_node(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        nodes: &mut StaticModuleNodes,
    ) -> Result<bool, JsError> {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if nodes.contains_key(&identity) {
            return Ok(true);
        }
        let key = module_cache_key(&module.name, "javascript");
        if let Some(program) = self
            .realm
            .promise
            .modules
            .get(&key)
            .and_then(|record| record.program)
        {
            let residual = self
                .programs
                .get(program)
                .ok_or_else(|| self.type_error(p, "module program is unavailable".into()))?;
            let Some(plan) = &residual.module_link_plan else {
                return Ok(false);
            };
            let mut locals = self
                .module_local_exports(program, plan.locals.clone())
                .ok_or_else(|| self.type_error(p, "module export binding is unavailable".into()))?;
            // A re-exported source-phase import resolves to the requested
            // module's source object, not to the re-exporting module's slot.
            for ((local, _), (_, value)) in plan.locals.iter().zip(&mut locals) {
                let Some(import) = residual.module_imports.iter().find(|import| {
                    import.phase == crate::bytecode::ModuleRequestPhase::Source
                        && &import.local == local
                }) else {
                    continue;
                };
                let dependency = self
                    .static_module_dependency(p, &module.name, &import.source)?
                    .ok_or_else(|| self.type_error(p, "module import was not resolved".into()))?;
                *value = StaticModuleValue::ModuleSource(self.module_source_value(p, &dependency)?);
            }
            nodes.insert(
                identity,
                StaticModuleNode::new(locals, plan.reexports.clone()),
            );
            return Ok(true);
        }
        if let Some(exports) = crate::Engine::static_module_exports(&module.source, &module.name) {
            let exports = exports
                .into_iter()
                .map(|(name, value)| {
                    (
                        name.clone(),
                        StaticModuleValue::Constant {
                            module: module.name.clone(),
                            export: name,
                            value,
                        },
                    )
                })
                .collect();
            nodes.insert(identity, StaticModuleNode::new(exports, Vec::new()));
            return Ok(true);
        }
        let Some(plan) = self.static_module_plan(&module.source, &module.name) else {
            return Ok(false);
        };
        let Some(link_plan) = plan.link_plan else {
            return Ok(false);
        };
        let residual =
            crate::Engine::specialize_module_with_atom_prefix(&module.source, &module.name, &*self)
                .map_err(|diagnostics| {
                    if diagnostics
                        .iter()
                        .any(crate::compile::Diagnostic::is_stack_exhausted)
                    {
                        return self.stack_exhaustion_error();
                    }
                    self.type_error(
                        p,
                        format!("dynamic module compilation failed: {diagnostics:?}"),
                    )
                })?;
        if !residual.module_imports.is_empty() {
            return Ok(false);
        }
        let locals = if link_plan.locals.is_empty() {
            Vec::new()
        } else {
            self.evaluate_module_locals(
                p,
                module,
                link_plan.locals,
                &mut ActiveModuleExports::default(),
            )?
        };
        nodes.insert(identity, StaticModuleNode::new(locals, link_plan.reexports));
        Ok(true)
    }

    fn static_module_export_names(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        nodes: &mut StaticModuleNodes,
        visited: &mut FxHashSet<ModuleIdentity>,
    ) -> Result<Option<Vec<String>>, JsError> {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if !visited.insert(identity.clone()) {
            return Ok(Some(Vec::new()));
        }
        if !self.ensure_static_module_node(p, module, nodes)? {
            return Ok(None);
        }
        let Some(node) = nodes.get(&identity).cloned() else {
            return Ok(None);
        };
        let mut names: Vec<String> = node.locals.iter().map(|(name, _)| name.clone()).collect();
        for reexport in &node.reexports {
            match reexport {
                crate::compile::StaticModuleReexport::Named { exported, .. }
                | crate::compile::StaticModuleReexport::Namespace { exported, .. } => {
                    names.push(exported.clone());
                }
                crate::compile::StaticModuleReexport::Star { source } => {
                    let Some(dependency) = self.static_node_dependency(p, module, source, nodes)?
                    else {
                        return Ok(None);
                    };
                    let Some(dependency_names) =
                        self.static_module_export_names(p, &dependency, nodes, visited)?
                    else {
                        return Ok(None);
                    };
                    names.extend(dependency_names);
                }
            }
        }
        names.sort_by(|left, right| left.encode_utf16().cmp(right.encode_utf16()));
        names.dedup();
        Ok(Some(names))
    }

    fn static_module_dependency(
        &mut self,
        p: &ResidualProgram,
        referrer: &str,
        source: &str,
    ) -> Result<Option<ModuleSource>, JsError> {
        // Export resolution follows the graph's already-resolved module edges.
        // It needs compiled metadata, never another host load or another source
        // copy. Execution and source-phase imports keep their original byte path.
        let key = module_cache_key(referrer, "javascript");
        if let Some(record) = self.realm.promise.modules.get(&key)
            && let Some(program) = record
                .program
                .and_then(|program| self.programs.get(program))
            && let Some(index) = program
                .module_requests
                .iter()
                .position(|request| request.source == source)
            && let Some(name) = record.resolved_requests.get(index)
        {
            return Ok(Some(ModuleSource::text(name.clone(), "")));
        }
        self.host_resolved_module(referrer, source)
            .map_err(|error| self.module_load_error(p, error))
    }

    /// `static_module_dependency` of a module with a node in `nodes`, through the node's
    /// memo of its edges: a dependency whose node exists needs neither the host nor its text.
    fn static_node_dependency(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        source: &str,
        nodes: &StaticModuleNodes,
    ) -> Result<Option<ModuleSource>, JsError> {
        let node = nodes
            .get(&crate::module_identity::normalize(std::path::Path::new(
                &module.name,
            )))
            .cloned();
        if let Some(node) = &node
            && let Some(name) = node.dependencies.borrow().get(source)
            && nodes.contains_key(&crate::module_identity::normalize(std::path::Path::new(
                name,
            )))
        {
            return Ok(Some(ModuleSource::text(name.clone(), "")));
        }
        let dependency = self.static_module_dependency(p, &module.name, source)?;
        if let (Some(node), Some(dependency)) = (&node, &dependency) {
            node.dependencies
                .borrow_mut()
                .insert(source.to_owned(), dependency.name.clone());
        }
        Ok(dependency)
    }

    /// GetExportedNames of `module` with an empty export-star set, as a set, memoized on its
    /// node. A name outside it cannot resolve through `module`: ResolveExport reaches no module
    /// GetExportedNames does not, so a star re-export of `module` is skipped for it.
    fn static_module_exported_name_set(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        nodes: &mut StaticModuleNodes,
    ) -> Result<Option<Rc<FxHashSet<String>>>, JsError> {
        if !self.ensure_static_module_node(p, module, nodes)? {
            return Ok(None);
        }
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        let Some(node) = nodes.get(&identity).cloned() else {
            return Ok(None);
        };
        if let Some(names) = node.exported_names.get() {
            return Ok(names.clone());
        }
        let names = self
            .static_module_export_names(p, module, nodes, &mut FxHashSet::default())?
            .map(|names| Rc::new(names.into_iter().collect::<FxHashSet<_>>()));
        Ok(node.exported_names.get_or_init(|| names).clone())
    }

    /// `node`'s star re-exports, indexed by the names each exports (`StaticStarIndex`).
    fn static_star_index(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        node: &Rc<StaticModuleNode>,
        nodes: &mut StaticModuleNodes,
    ) -> Result<Option<Rc<StaticStarIndex>>, JsError> {
        if let Some(index) = node.stars.get() {
            return Ok(index.clone());
        }
        let mut index = StaticStarIndex::default();
        let mut complete = true;
        for reexport in &node.reexports {
            let crate::compile::StaticModuleReexport::Star { source } = reexport else {
                continue;
            };
            let Some(dependency) = self.static_node_dependency(p, module, source, nodes)? else {
                complete = false;
                break;
            };
            let Some(names) = self.static_module_exported_name_set(p, &dependency, nodes)? else {
                complete = false;
                break;
            };
            let position = index.dependencies.len();
            for name in names.iter() {
                index
                    .exporters
                    .entry(name.clone())
                    .or_default()
                    .push(position);
            }
            index
                .dependencies
                .push(ModuleSource::text(dependency.name, ""));
        }
        let index = complete.then(|| Rc::new(index));
        Ok(node.stars.get_or_init(|| index).clone())
    }

    fn resolve_static_module_export(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        name: &str,
        nodes: &mut StaticModuleNodes,
        resolve_set: &mut StaticModuleResolveSet,
    ) -> Result<StaticModuleResolution, JsError> {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if !resolve_set.insert((identity.clone(), name.to_owned())) {
            return Ok(StaticModuleResolution::Missing);
        }
        if !self.ensure_static_module_node(p, module, nodes)? {
            return Ok(StaticModuleResolution::Unsupported);
        }
        let Some(node) = nodes.get(&identity).cloned() else {
            return Ok(StaticModuleResolution::Unsupported);
        };
        match node.sites.get(name).copied() {
            Some(StaticExportSite::Local(index)) => {
                return Ok(StaticModuleResolution::Binding(
                    node.locals[index].1.clone(),
                ));
            }
            Some(StaticExportSite::Reexport(index)) => match &node.reexports[index] {
                crate::compile::StaticModuleReexport::Named {
                    source, imported, ..
                } => {
                    let Some(dependency) = self.static_node_dependency(p, module, source, nodes)?
                    else {
                        return Ok(StaticModuleResolution::Missing);
                    };
                    return self.resolve_static_module_export(
                        p,
                        &dependency,
                        imported,
                        nodes,
                        resolve_set,
                    );
                }
                crate::compile::StaticModuleReexport::Namespace { source, .. } => {
                    let Some(dependency) = self.static_node_dependency(p, module, source, nodes)?
                    else {
                        return Ok(StaticModuleResolution::Missing);
                    };
                    let key = module_cache_key(&dependency.name, "javascript");
                    if let Some(namespace) =
                        self.realm.promise.modules.get(&key).and_then(|record| {
                            match record.outcome {
                                ModuleOutcome::Evaluated(namespace)
                                | ModuleOutcome::Deferred(namespace) => Some(namespace),
                                ModuleOutcome::Pending(_) => record.pending_namespace(),
                                ModuleOutcome::Errored(_) => None,
                            }
                        })
                    {
                        return Ok(StaticModuleResolution::Binding(StaticModuleValue::Cached(
                            namespace,
                        )));
                    }
                    let Some(names) = self.static_module_export_names(
                        p,
                        &dependency,
                        nodes,
                        &mut FxHashSet::default(),
                    )?
                    else {
                        return Ok(StaticModuleResolution::Unsupported);
                    };
                    let mut exports = Vec::new();
                    for name in names {
                        match self.resolve_static_module_export(
                            p,
                            &dependency,
                            &name,
                            nodes,
                            &mut StaticModuleResolveSet::default(),
                        )? {
                            StaticModuleResolution::Binding(value) => exports.push((name, value)),
                            StaticModuleResolution::Missing | StaticModuleResolution::Ambiguous => {
                            }
                            StaticModuleResolution::Unsupported => {
                                return Ok(StaticModuleResolution::Unsupported);
                            }
                        }
                    }
                    return Ok(StaticModuleResolution::Binding(
                        StaticModuleValue::Namespace {
                            name: dependency.name,
                            exports,
                        },
                    ));
                }
                crate::compile::StaticModuleReexport::Star { .. } => {
                    unreachable!("star re-exports define no named site")
                }
            },
            None => {}
        }
        // A star re-export never provides `default`.
        if name == "default" {
            return Ok(StaticModuleResolution::Missing);
        }
        // The star modules that can provide `name`, in source order: those GetExportedNames
        // finds it in (`static_module_exported_name_set`).
        let candidates = match self.static_star_index(p, module, &node, nodes)? {
            Some(index) => index
                .exporters
                .get(name)
                .map_or_else(Vec::new, |positions| {
                    positions
                        .iter()
                        .map(|&position| index.dependencies[position].clone())
                        .collect()
                }),
            None => {
                let mut candidates = Vec::new();
                for reexport in &node.reexports {
                    let crate::compile::StaticModuleReexport::Star { source } = reexport else {
                        continue;
                    };
                    let Some(dependency) = self.static_node_dependency(p, module, source, nodes)?
                    else {
                        return Ok(StaticModuleResolution::Missing);
                    };
                    if !self
                        .static_module_exported_name_set(p, &dependency, nodes)?
                        .is_some_and(|names| !names.contains(name))
                    {
                        candidates.push(dependency);
                    }
                }
                candidates
            }
        };
        let mut star_resolution: Option<StaticModuleValue> = None;
        for dependency in candidates {
            let mut branch = resolve_set.clone();
            match self.resolve_static_module_export(p, &dependency, name, nodes, &mut branch)? {
                StaticModuleResolution::Binding(value) => match &star_resolution {
                    Some(existing) if !same_static_module_binding(existing, &value) => {
                        return Ok(StaticModuleResolution::Ambiguous);
                    }
                    None => star_resolution = Some(value),
                    _ => {}
                },
                StaticModuleResolution::Ambiguous => {
                    return Ok(StaticModuleResolution::Ambiguous);
                }
                StaticModuleResolution::Missing => {}
                StaticModuleResolution::Unsupported => {
                    return Ok(StaticModuleResolution::Unsupported);
                }
            }
        }
        Ok(star_resolution.map_or(
            StaticModuleResolution::Missing,
            StaticModuleResolution::Binding,
        ))
    }

    fn validate_static_module_links(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        nodes: &mut StaticModuleNodes,
        visited: &mut FxHashSet<ModuleIdentity>,
    ) -> Result<Option<bool>, JsError> {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if !visited.insert(identity.clone()) {
            return Ok(Some(true));
        }
        if !self.ensure_static_module_node(p, module, nodes)? {
            return Ok(None);
        }
        let Some(node) = nodes.get(&identity).cloned() else {
            return Ok(Some(true));
        };
        for reexport in node.reexports.iter().cloned() {
            let (source, named) = match reexport {
                crate::compile::StaticModuleReexport::Named {
                    source, imported, ..
                } => (source, Some(imported)),
                crate::compile::StaticModuleReexport::Star { source }
                | crate::compile::StaticModuleReexport::Namespace { source, .. } => (source, None),
            };
            let Some(dependency) = self.static_module_dependency(p, &module.name, &source)? else {
                return Ok(Some(false));
            };
            if let Some(imported) = named {
                match self.resolve_static_module_export(
                    p,
                    &dependency,
                    &imported,
                    nodes,
                    &mut StaticModuleResolveSet::default(),
                )? {
                    StaticModuleResolution::Binding(_) => {}
                    StaticModuleResolution::Missing | StaticModuleResolution::Ambiguous => {
                        return Ok(Some(false));
                    }
                    StaticModuleResolution::Unsupported => return Ok(None),
                }
            }
            let Some(valid) = self.validate_static_module_links(p, &dependency, nodes, visited)?
            else {
                return Ok(None);
            };
            if !valid {
                return Ok(Some(false));
            }
        }
        Ok(Some(true))
    }

    fn resolve_static_module_plan_graph(
        &mut self,
        p: &ResidualProgram,
        module: ModuleSource,
    ) -> Result<Option<StaticModuleGraph>, JsError> {
        self.resolve_static_module_plan_graph_with_nodes(
            p,
            module,
            &mut StaticModuleNodes::default(),
        )
    }

    fn resolve_static_module_plan_graph_with_nodes(
        &mut self,
        p: &ResidualProgram,
        module: ModuleSource,
        nodes: &mut StaticModuleNodes,
    ) -> Result<Option<StaticModuleGraph>, JsError> {
        let Some(names) =
            self.static_module_export_names(p, &module, nodes, &mut FxHashSet::default())?
        else {
            return Ok(None);
        };
        let Some(valid) =
            self.validate_static_module_links(p, &module, nodes, &mut FxHashSet::default())?
        else {
            return Ok(None);
        };
        if !valid {
            return Ok(Some(StaticModuleGraph::LinkError));
        }
        let mut exports = Vec::new();
        for name in names {
            match self.resolve_static_module_export(
                p,
                &module,
                &name,
                nodes,
                &mut StaticModuleResolveSet::default(),
            )? {
                StaticModuleResolution::Binding(value) => exports.push((name, value)),
                StaticModuleResolution::Missing | StaticModuleResolution::Ambiguous => {}
                StaticModuleResolution::Unsupported => return Ok(None),
            }
        }
        Ok(Some(StaticModuleGraph::Linked {
            name: module.name,
            exports,
            incomplete: false,
        }))
    }

    fn resolve_static_module_graph(
        &mut self,
        p: &ResidualProgram,
        module: ModuleSource,
        active: &mut ActiveModuleExports,
    ) -> Result<StaticModuleGraph, JsError> {
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        if let Some(exports) = active.get(&identity) {
            return Ok(StaticModuleGraph::Linked {
                name: module.name.clone(),
                exports: exports.clone(),
                incomplete: true,
            });
        }
        if self
            .static_module_plan(&module.source, &module.name)
            .is_some_and(|plan| {
                plan.link_plan
                    .is_some_and(|link_plan| !link_plan.reexports.is_empty())
            })
            && let Some(graph) = self.resolve_static_module_plan_graph(p, module.clone())?
        {
            return Ok(graph);
        }
        if p.is_module()
            && self.active_program == ProgramId::MAIN
            && crate::module_identity::same_name(&module.name, &p.source_name)
        {
            let key = module_cache_key(&module.name, "javascript");
            let namespace = self
                .realm
                .promise
                .modules
                .get(&key)
                .and_then(ModuleRecord::pending_namespace);
            if let Some(namespace) = namespace
                && let Some(exports) = self.cached_static_exports(namespace)
            {
                return Ok(StaticModuleGraph::Linked {
                    name: module.name.clone(),
                    exports,
                    incomplete: false,
                });
            }
        }
        if let Some(exports) = crate::Engine::static_module_exports(&module.source, &module.name) {
            return Ok(StaticModuleGraph::Linked {
                name: module.name.clone(),
                exports: exports
                    .into_iter()
                    .map(|(name, value)| {
                        (
                            name.clone(),
                            StaticModuleValue::Constant {
                                module: module.name.clone(),
                                export: name,
                                value,
                            },
                        )
                    })
                    .collect(),
                incomplete: false,
            });
        }
        if let Some(plan) = self.static_module_plan(&module.source, &module.name)
            && plan
                .link_plan
                .as_ref()
                .is_some_and(|link_plan| !link_plan.reexports.is_empty())
        {
            active.insert(identity.clone(), Vec::new());
            let result = self.resolve_static_module_plan(p, &module, plan, active);
            active.remove(&identity);
            return result;
        }
        let key = module_cache_key(&module.name, "javascript");
        let namespace =
            self.realm
                .promise
                .modules
                .get(&key)
                .and_then(|record| match record.outcome {
                    ModuleOutcome::Evaluated(namespace) | ModuleOutcome::Deferred(namespace) => {
                        Some(namespace)
                    }
                    ModuleOutcome::Pending(_) => record.pending_namespace(),
                    ModuleOutcome::Errored(_) => None,
                });
        if let Some(namespace) = namespace
            && let Some(exports) = self.cached_static_exports(namespace)
            && !exports.is_empty()
        {
            return Ok(StaticModuleGraph::Linked {
                name: module.name,
                exports,
                incomplete: false,
            });
        }
        Ok(StaticModuleGraph::Unsupported)
    }

    fn resolve_static_module_plan(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        plan: crate::compile::StaticModulePlan,
        active: &mut ActiveModuleExports,
    ) -> Result<StaticModuleGraph, JsError> {
        let Some(link_plan) = plan.link_plan else {
            return Ok(StaticModuleGraph::Unsupported);
        };
        let locals = if link_plan.locals.is_empty() {
            Vec::new()
        } else {
            self.evaluate_module_locals(p, module, link_plan.locals, active)?
        };
        self.resolve_static_module_links(p, module, locals, link_plan.reexports, active)
    }

    fn resolve_static_module_links(
        &mut self,
        p: &ResidualProgram,
        module: &ModuleSource,
        locals: Vec<(String, StaticModuleValue)>,
        reexports: Vec<crate::compile::StaticModuleReexport>,
        active: &mut ActiveModuleExports,
    ) -> Result<StaticModuleGraph, JsError> {
        let mut links = StaticModuleLinks::default();
        if links.add_locals(locals.clone()) != StaticModuleAdd::Added {
            return Ok(StaticModuleGraph::LinkError);
        }
        let identity = crate::module_identity::normalize(std::path::Path::new(&module.name));
        active.insert(identity.clone(), links.partial_exports());
        let mut pending = reexports;
        loop {
            let mut remaining = Vec::new();
            let mut changed = false;
            for reexport in pending {
                let source = match &reexport {
                    crate::compile::StaticModuleReexport::Named { source, .. }
                    | crate::compile::StaticModuleReexport::Star { source }
                    | crate::compile::StaticModuleReexport::Namespace { source, .. } => source,
                };
                let dependency = self
                    .host_resolved_module(&module.name, source)
                    .map_err(|error| self.module_load_error(p, error))?;
                let Some(dependency) = dependency else {
                    return Ok(StaticModuleGraph::LinkError);
                };
                let StaticModuleGraph::Linked {
                    name: dependency_name,
                    exports: dependency_exports,
                    incomplete,
                } = self.resolve_static_module_graph(p, dependency, active)?
                else {
                    return Ok(StaticModuleGraph::Unsupported);
                };
                match links.add(reexport.clone(), dependency_name, dependency_exports) {
                    StaticModuleAdd::Added => changed = true,
                    StaticModuleAdd::Missing if incomplete => {
                        remaining.push(reexport);
                        continue;
                    }
                    StaticModuleAdd::Missing => return Ok(StaticModuleGraph::LinkError),
                    StaticModuleAdd::Conflict => return Ok(StaticModuleGraph::LinkError),
                }
                active.insert(identity.clone(), links.partial_exports());
            }
            if remaining.is_empty() {
                active.remove(&identity);
                return Ok(links.finish(module.name.clone(), false));
            }
            if !changed {
                let result = links.finish(module.name.clone(), true);
                active.remove(&identity);
                return Ok(result);
            }
            pending = remaining;
        }
    }

    fn module_namespace_from_static(
        &mut self,
        exports: Vec<(String, StaticModuleValue)>,
    ) -> Result<Value, JsError> {
        let mut values = Vec::with_capacity(exports.len());
        let mut bindings = Vec::new();
        for (name, value) in exports {
            let value = match value {
                StaticModuleValue::Constant { value, .. } => self.module_static_value(value),
                StaticModuleValue::Cached(value) => value,
                StaticModuleValue::ModuleSource(value) => value,
                StaticModuleValue::Binding {
                    program,
                    slot,
                    value,
                } => {
                    let atom = self.intern_atom(&name);
                    bindings.push((atom, program, slot));
                    value
                }
                StaticModuleValue::Namespace { name, exports } => {
                    self.cached_static_namespace(name, exports)?
                }
            };
            values.push((name, value));
        }
        let namespace = self.module_namespace(values)?;
        if let Some(object) = self.object_data_mut(namespace) {
            object.set_module_bindings(bindings);
        }
        Ok(namespace)
    }

    fn cached_static_exports(&self, namespace: Value) -> Option<Vec<(String, StaticModuleValue)>> {
        let object = self.object_data(namespace)?;
        let bindings = object
            .module_bindings()
            .iter()
            .map(|(atom, program, slot)| (*atom, (*program, *slot)))
            .collect::<FxHashMap<_, _>>();
        let keys = self.shape_keys(object.shape());
        Some(
            keys.iter()
                .filter_map(|key| {
                    let crate::vm::property_key::PropertyKey::String(atom) = key else {
                        return None;
                    };
                    let value = self.own_property(namespace, *atom)?;
                    let export = match bindings.get(atom) {
                        Some((program, slot)) => StaticModuleValue::Binding {
                            program: *program,
                            slot: *slot,
                            value,
                        },
                        None => StaticModuleValue::Cached(value),
                    };
                    Some((self.atom_name(*atom).to_owned(), export))
                })
                .collect::<Vec<_>>(),
        )
    }

    fn cached_static_namespace(
        &mut self,
        name: String,
        exports: Vec<(String, StaticModuleValue)>,
    ) -> Result<Value, JsError> {
        let key = module_cache_key(&name, "javascript");
        if let Some(ModuleOutcome::Evaluated(namespace)) = self
            .realm
            .promise
            .modules
            .get(&key)
            .map(|record| record.outcome)
        {
            return Ok(namespace);
        }
        let namespace = self.module_namespace_from_static(exports)?;
        self.realm
            .promise
            .modules
            .insert(key, ModuleRecord::materialized(namespace));
        Ok(namespace)
    }

    fn validate_dynamic_import_options(
        &mut self,
        p: &ResidualProgram,
        options: Value,
    ) -> Result<Option<String>, JsError> {
        if options.is_undefined() {
            return Ok(None);
        }
        if !self.is_object_like(options) {
            return Err(self.type_error(p, "dynamic import options must be an object".into()));
        }
        let with_atom = self.intern_atom("with");
        let mut attributes = self.get_property(p, options, with_atom)?;
        if attributes.is_undefined() {
            let assert_atom = self.intern_atom("assert");
            attributes = self.get_property(p, options, assert_atom)?;
        }
        if attributes.is_undefined() {
            return Ok(None);
        }
        if !self.is_object_like(attributes) {
            return Err(self.type_error(p, "dynamic import attributes must be an object".into()));
        }
        let keys = self.object_own_keys(p, attributes)?;
        let Some(elements) = self.heap.array_elements(keys) else {
            return Err(JsError(
                "dynamic import own keys result is not an array".into(),
            ));
        };
        let keys = elements.to_vec();
        let mut module_type = None;
        for key in keys {
            let key_name = match self.heap.get(key) {
                _ if let Some(value) = self.heap.string(key) => {
                    Some(value.host_string().to_owned())
                }
                Some(Cell::Symbol(_)) => continue,
                _ => return Err(JsError("invalid import attribute key".into())),
            };
            let descriptor = self.object_get_own_property_descriptor(p, &[attributes, key])?;
            if descriptor.is_undefined() {
                continue;
            }
            let enumerable_atom = self.intern_atom("enumerable");
            let enumerable = self.get_property(p, descriptor, enumerable_atom)?;
            if self.truthy(enumerable) {
                let value = self.get_index(p, attributes, key)?;
                if !self.heap.is_string(value) {
                    return Err(self
                        .type_error(p, "dynamic import attribute values must be strings".into()));
                }
                if key_name.as_deref() == Some("type")
                    && let Some(value) = self.heap.string(value)
                {
                    module_type = Some(value.host_string().to_owned());
                }
            }
        }
        Ok(module_type)
    }

    fn module_namespace_default(&mut self, value: Value) -> Result<Value, JsError> {
        self.module_namespace(vec![("default".into(), value)])
    }

    fn module_namespace(&mut self, exports: Vec<(String, Value)>) -> Result<Value, JsError> {
        self.module_namespace_with_tag(exports, "Module")
    }

    fn module_namespace_with_tag(
        &mut self,
        exports: Vec<(String, Value)>,
        namespace_tag: &str,
    ) -> Result<Value, JsError> {
        let namespace = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
        if let Some(object) = self.object_data_mut(namespace) {
            object.set_module_namespace();
        }
        self.mark_object_dictionary(namespace, DictionaryTrigger::ModuleNamespace);
        // Export names are distinct, so each is appended with its final attributes and
        // without a lookup: a lookup would index every intermediate dictionary shape,
        // making a namespace of N exports cost O(N^2).
        let export_attributes = DataAttributes {
            writable: false,
            enumerable: true,
            configurable: false,
        };
        let mut defined = FxHashSet::default();
        for (name, value) in exports {
            let atom = self.intern_atom(&name);
            let key = crate::vm::property_key::PropertyKey::string(atom);
            if defined.insert(atom) {
                self.create_shape_data_property(namespace, key, value, export_attributes)?;
            } else {
                self.set_property(namespace, atom, value)?;
            }
        }
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::ToStringTag) {
            let module = self.heap.alloc_string(namespace_tag.to_string().into());
            self.set_symbol_property(namespace, symbol, module)?;
            self.set_property_attributes(
                namespace,
                crate::vm::property_key::PropertyKey::symbol(symbol),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: false,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        if let Some(object) = self.object_data_mut(namespace) {
            object.set_extensible(false);
        }
        Ok(namespace)
    }

    fn module_static_value(&mut self, constant: Constant) -> Value {
        match constant {
            Constant::Number(value) => Value::number(value),
            Constant::WasmBits64(bits) => self.encode_bits64(bits),
            Constant::WasmV128(bits) => self.heap.alloc_v128(bits),
            Constant::String(value) => self.heap.alloc_string(JsString::from_shared_text(&value)),
            Constant::StringUnits(value) => self.heap.alloc_string(JsString::from_units(&value)),
            Constant::BigInt(value) => self.heap.alloc(Cell::bigint(value)),
            Constant::Boolean(true) => Value::TRUE,
            Constant::Boolean(false) => Value::FALSE,
            Constant::Null => Value::NULL,
            Constant::Undefined => Value::UNDEFINED,
        }
    }

    pub(super) fn active_native_callable(&self) -> Option<Value> {
        self.realm
            .promise
            .active_native
            .last()
            .map(|activation| activation.callable)
    }

    pub(super) fn active_native_env(&self) -> Option<Value> {
        let callee = self.active_native_callable()?;
        match self.heap.function(callee) {
            Some(FunctionRecord { env, .. }) if !env.is_null() => Some(*env),
            _ => None,
        }
    }

    pub(super) fn promise_resolve_value(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        value: Value,
    ) -> Result<(), JsError> {
        if promise == value {
            let error = self.type_error(p, "Promise cannot resolve to itself".into());
            return self.promise_settle(
                p,
                promise,
                PromiseState::Rejected,
                error.thrown_value().unwrap_or(Value::UNDEFINED),
            );
        }
        if self.object_data(value).is_none() {
            return self.promise_settle(p, promise, PromiseState::Fulfilled, value);
        }
        let then_atom = self.runtime_atoms.then;
        let then = match self.get_property(p, value, then_atom) {
            Ok(then) => then,
            // Cancellation must unwind the active job, not become a guest
            // rejection or an allocated Error during thenable assimilation.
            Err(error) if error.is_execution_terminated() => return Err(error),
            Err(error) => {
                let reason = error
                    .thrown_value()
                    .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
                return self.promise_settle(p, promise, PromiseState::Rejected, reason);
            }
        };
        if !self.is_function(then) {
            return self.promise_settle(p, promise, PromiseState::Fulfilled, value);
        }
        let execution_context = self.host.capture_job_context(
            self.promise_record(promise)
                .and_then(|record| record.host_context.get()),
        );
        self.realm
            .jobs
            .push_back(super::PendingJob::Thenable(ThenableJob {
                realm: self.realm.globals,
                then,
                thenable: value,
                promise,
                execution_context,
            }));
        Ok(())
    }

    pub(super) fn promise_for_value(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        self.promise_resolve_for_constructor(p, self.intrinsic_promise_constructor(), value)
    }

    /// `Promise.prototype.then` of a receiver whose species steps provably answer the
    /// intrinsic `%Promise%` without running guest code (`promise_species_intrinsic`), made
    /// without a frame for the lane: the reaction promise and `perform_promise_then`, as
    /// `promise_then`'s intrinsic arm makes them. `None` leaves any other receiver, an
    /// impure species read, or a lifecycle-observing host, whose hooks run guest code, to
    /// the general call.
    pub(super) fn lane_promise_then(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        on_fulfilled: Value,
        on_rejected: Value,
    ) -> Option<Value> {
        if self.host.observes_promise_lifecycle()
            || !self.is_promise(promise)
            || !self.promise_species_intrinsic(promise)
        {
            return None;
        }
        // Nothing below runs guest code or collects, so the receiver and handlers stay
        // live in the caller's registers, as `lane_new_array`'s arguments do.
        let next = self.promise_object_with_parent(None).ok()?;
        self.perform_promise_then(p, promise, on_fulfilled, on_rejected, next);
        Some(next)
    }

    /// `Promise.resolve` of the intrinsic `%Promise%` receiver whose steps provably observe
    /// nothing: a promise whose `constructor` reads as plain data to the receiver is
    /// returned as is; any other value resolves a fresh promise as `promise_resolve_value`
    /// resolves one, where its `then` also reads as plain data — fulfilled directly when
    /// that `then` is not callable, else through the thenable job the general path would
    /// enqueue. `None` leaves an impure read or a lifecycle-observing host to the general
    /// call.
    pub(super) fn lane_promise_resolve(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        value: Value,
    ) -> Option<Value> {
        if self.host.observes_promise_lifecycle()
            || receiver != self.intrinsic_promise_constructor()
        {
            return None;
        }
        if self.is_promise(value) {
            let constructor_atom = self.runtime_atoms.constructor;
            let observed = self.plain_named_load(value, constructor_atom)?;
            return (observed == receiver).then_some(value);
        }
        let then = if self.object_data(value).is_some() {
            let then_atom = self.runtime_atoms.then;
            self.plain_named_load(value, then_atom)?
        } else {
            Value::UNDEFINED
        };
        let promise = self.promise_object_with_parent(None).ok()?;
        if self.is_function(then) {
            let execution_context = self.host.capture_job_context(None);
            self.realm
                .jobs
                .push_back(super::PendingJob::Thenable(ThenableJob {
                    realm: self.realm.globals,
                    then,
                    thenable: value,
                    promise,
                    execution_context,
                }));
        } else {
            // A fresh promise has no reactions, so the settle enqueues nothing.
            self.promise_settle(p, promise, PromiseState::Fulfilled, value)
                .ok()?;
        }
        Some(promise)
    }

    /// Whether `promise`'s species steps provably answer the intrinsic `%Promise%`: its
    /// `constructor` reads as plain data (`plain_named_load`) to that constructor — or to
    /// `undefined`, which PromiseSpeciesConstructor answers with the intrinsic — and the
    /// constructor's own `@@species` is still an intrinsic accessor, whose getter returns
    /// its receiver (`has_intrinsic_species_getter`). Both reads observe nothing, so a
    /// failed proof only leaves the observable steps to the general path.
    fn promise_species_intrinsic(&mut self, promise: Value) -> bool {
        let constructor_atom = self.runtime_atoms.constructor;
        let Some(constructor) = self.plain_named_load(promise, constructor_atom) else {
            return false;
        };
        constructor.is_undefined()
            || (constructor == self.intrinsic_promise_constructor()
                && self.has_intrinsic_species_getter(constructor))
    }

    pub(super) fn promise_then(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        on_fulfilled: Value,
        on_rejected: Value,
    ) -> Result<Value, JsError> {
        self.with_call_roots([promise, on_fulfilled, on_rejected], |vm| {
            if !vm.is_promise(promise) {
                return Err(
                    vm.type_error(p, "Promise.prototype method called on non-Promise".into())
                );
            }
            let constructor = vm.promise_species_constructor(p, promise)?;
            // A %Promise% capability has no guest executor (see `promise_resolve_for_constructor`):
            // its promise is settled directly when the reaction runs.
            let next = if constructor == vm.intrinsic_promise_constructor() {
                vm.promise_object_with_parent(
                    vm.promise_record(promise)
                        .expect("promise record")
                        .host_context
                        .get(),
                )?
            } else {
                let (next, resolve, reject) = vm.new_promise_capability(p, constructor)?;
                vm.realm
                    .promise
                    .reaction_capabilities
                    .insert(next, (resolve, reject));
                next
            };
            vm.perform_promise_then(p, promise, on_fulfilled, on_rejected, next);
            Ok(next)
        })
    }

    pub(super) fn promise_then_intrinsic(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        on_fulfilled: Value,
        on_rejected: Value,
    ) -> Result<Value, JsError> {
        if !self.is_promise(promise) {
            return Err(self.type_error(p, "Promise.prototype method called on non-Promise".into()));
        }
        let next = self.with_call_roots([promise, on_fulfilled, on_rejected], |vm| {
            vm.promise_object_with_parent(
                vm.promise_record(promise)
                    .expect("promise record")
                    .host_context
                    .get(),
            )
        })?;
        let (resolve, reject) = self.promise_resolving_functions(next);
        self.realm
            .promise
            .reaction_capabilities
            .insert(next, (resolve, reject));
        self.perform_promise_then(p, promise, on_fulfilled, on_rejected, next);
        Ok(next)
    }

    pub(super) fn perform_promise_then(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        on_fulfilled: Value,
        on_rejected: Value,
        next: Value,
    ) {
        self.observe_promise_rejection(promise);
        let reaction = PromiseReaction {
            on_fulfilled: if self.is_function(on_fulfilled) {
                on_fulfilled
            } else {
                Value::UNDEFINED
            },
            on_rejected: if self.is_function(on_rejected) {
                on_rejected
            } else {
                Value::UNDEFINED
            },
            next,
            execution_context: self
                .host
                .capture_job_context(
                    self.promise_record(next)
                        .and_then(|record| record.host_context.get()),
                )
                .into(),
        };
        // Species selection and capability construction may settle the source.
        // Read its authoritative state only when installing this reaction.
        let record = self.promise_record_mut(promise).unwrap();
        if record.state == PromiseState::Pending {
            record.reactions.push(reaction);
        } else {
            let (state, result) = (record.state, record.result);
            self.enqueue_promise_reaction(p, reaction, state, result);
        }
    }

    pub(super) fn intrinsic_promise_constructor(&self) -> Value {
        self.realm.intrinsics.promise_constructors[&self.realm.globals]
    }

    pub(super) fn promise_species_constructor(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
    ) -> Result<Value, JsError> {
        let constructor_atom = self.runtime_atoms.constructor;
        let constructor = self.get_property(p, promise, constructor_atom)?;
        if constructor.is_undefined() {
            return Ok(self.intrinsic_promise_constructor());
        }
        if !self.is_object_like(constructor) {
            return Err(self.type_error(p, "Promise constructor is not an object".into()));
        }
        let species = self.species_of(p, constructor)?;
        if species.is_undefined() || species.is_null() {
            Ok(self.intrinsic_promise_constructor())
        } else if self.is_constructable(p, species) {
            Ok(species)
        } else {
            Err(self.type_error(p, "Promise species is not a constructor".into()))
        }
    }

    pub(super) fn promise_resolve_for_constructor(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        value: Value,
    ) -> Result<Value, JsError> {
        let constructor = self.heap.root(constructor);
        let value = self.heap.root(value);
        let outcome = (|| {
            let input = self.heap.root_value(value).unwrap();
            if self.is_promise(input) {
                let constructor_atom = self.runtime_atoms.constructor;
                let observed = self.get_property(p, input, constructor_atom)?;
                if observed == self.heap.root_value(constructor).unwrap() {
                    return Ok(self.heap.root_value(value).unwrap());
                }
            }
            let constructor = self.heap.root_value(constructor).unwrap();
            if constructor == self.intrinsic_promise_constructor() {
                // NewPromiseCapability(%Promise%) observes nothing: the executor is internal
                // and %Promise%.prototype is fixed, so resolve a fresh promise directly
                // instead of through resolving functions no code can reach.
                let promise = self.promise_object()?;
                let value = self.heap.root_value(value).unwrap();
                self.with_call_roots([promise], |vm| vm.promise_resolve_value(p, promise, value))?;
                return Ok(promise);
            }
            let (promise, resolve, _) = self.new_promise_capability(p, constructor)?;
            let value = self.heap.root_value(value).unwrap();
            self.call_value(p, resolve, Value::UNDEFINED, &[value])?;
            Ok(promise)
        })();
        self.heap.release_root(constructor);
        self.heap.release_root(value);
        outcome
    }

    fn finally_handler_function(
        &mut self,
        handler: Value,
        constructor: Value,
        original_rejected: bool,
    ) -> Value {
        let function = self.native_with_env(Native::PromiseFinallyHandler, Value::UNDEFINED);
        self.realm.promise.finally_handler_callbacks.insert(
            function,
            FinallyHandlerCallback {
                handler,
                constructor,
                original_rejected,
            },
        );
        function
    }

    fn finally_continuation_function(&mut self, original_rejected: bool, original: Value) -> Value {
        let function =
            self.native_with_env(Native::PromiseFinallyContinuationHandler, Value::UNDEFINED);
        self.realm.promise.finally_continuation_callbacks.insert(
            function,
            FinallyContinuationCallback {
                original_rejected,
                original,
            },
        );
        function
    }

    pub(super) fn promise_finally_handler(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let function = self
            .active_native_callable()
            .ok_or_else(|| JsError("Promise finally handler without callback".into()))?;
        let callback = *self
            .realm
            .promise
            .finally_handler_callbacks
            .get(&function)
            .ok_or_else(|| JsError("stale Promise finally handler".into()))?;
        let original = args.first().copied().unwrap_or(Value::UNDEFINED);
        let cleanup = self.call_value(p, callback.handler, Value::UNDEFINED, &[])?;
        let promise = self.promise_resolve_for_constructor(p, callback.constructor, cleanup)?;
        let then_atom = self.runtime_atoms.then;
        let then = self.get_property(p, promise, then_atom)?;
        if !self.is_function(then) {
            return Err(self.type_error(p, "Promise cleanup then is not callable".into()));
        }
        let continuation = self.finally_continuation_function(callback.original_rejected, original);
        self.call_value(p, then, promise, &[continuation])
    }

    pub(super) fn promise_finally_continuation_handler(
        &mut self,
        _p: &ResidualProgram,
        _args: &[Value],
    ) -> Result<Value, JsError> {
        let function = self
            .active_native_callable()
            .ok_or_else(|| JsError("Promise finally continuation without callback".into()))?;
        let callback = *self
            .realm
            .promise
            .finally_continuation_callbacks
            .get(&function)
            .ok_or_else(|| JsError("stale Promise finally continuation".into()))?;
        if callback.original_rejected {
            return Err(JsError::thrown(
                callback.original,
                "Promise was rejected before finally".into(),
            ));
        }
        Ok(callback.original)
    }

    fn promise_catch(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if receiver.is_null() || receiver.is_undefined() {
            return Err(
                self.type_error(p, "Promise.prototype.catch called on nullish value".into())
            );
        }
        let then_atom = self.runtime_atoms.then;
        let then = self.get_property(p, receiver, then_atom)?;
        if !self.is_function(then) {
            return Err(self.type_error(p, "Promise.prototype.catch then is not callable".into()));
        }
        self.call_value(
            p,
            then,
            receiver,
            &[
                Value::UNDEFINED,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            ],
        )
    }

    fn promise_try(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !self.is_constructable(p, constructor) {
            return Err(self.type_error(p, "Promise.try receiver is not a constructor".into()));
        }
        let callback = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(callback) {
            return Err(self.type_error(p, "Promise.try callback is not callable".into()));
        }
        let callback_root = self.heap.root(callback);
        let argument_roots = args
            .iter()
            .skip(1)
            .map(|argument| self.heap.root(*argument))
            .collect::<Vec<_>>();
        let outcome = (|| {
            let (promise, resolve, reject) = self.new_promise_capability(p, constructor)?;
            let promise_root = self.heap.root(promise);
            let resolve_root = self.heap.root(resolve);
            let reject_root = self.heap.root(reject);
            let callback = self.heap.root_value(callback_root).unwrap_or(callback);
            let callback_args = argument_roots
                .iter()
                .map(|root| self.heap.root_value(*root).unwrap_or(Value::UNDEFINED))
                .collect::<Vec<_>>();
            let call_result = self.call_value(p, callback, Value::UNDEFINED, &callback_args);
            let settlement = match call_result {
                Ok(value) => {
                    let value_root = self.heap.root(value);
                    let resolve = self.heap.root_value(resolve_root).unwrap_or(resolve);
                    let value = self.heap.root_value(value_root).unwrap_or(value);
                    let result = self.call_value(p, resolve, Value::UNDEFINED, &[value]);
                    self.heap.release_root(value_root);
                    result
                }
                Err(error) => {
                    let reason = error.thrown_value().unwrap_or(Value::UNDEFINED);
                    let reason_root = self.heap.root(reason);
                    let reject = self.heap.root_value(reject_root).unwrap_or(reject);
                    let reason = self.heap.root_value(reason_root).unwrap_or(reason);
                    let result = self.call_value(p, reject, Value::UNDEFINED, &[reason]);
                    self.heap.release_root(reason_root);
                    result
                }
            };
            self.heap.release_root(promise_root);
            self.heap.release_root(resolve_root);
            self.heap.release_root(reject_root);
            settlement?;
            Ok(promise)
        })();
        self.heap.release_root(callback_root);
        for root in argument_roots {
            self.heap.release_root(root);
        }
        outcome
    }

    fn promise_finally(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !self.is_object_like(receiver) {
            return Err(self.type_error(p, "Promise.prototype.finally requires an object".into()));
        }
        let receiver = self.heap.root(receiver);
        let handler = self
            .heap
            .root(args.first().copied().unwrap_or(Value::UNDEFINED));
        let mut constructor_root = None;
        let mut then_root = None;
        let outcome = (|| {
            let source = self.heap.root_value(receiver).unwrap();
            let constructor = self.promise_species_constructor(p, source)?;
            constructor_root = Some(self.heap.root(constructor));
            let then_atom = self.runtime_atoms.then;
            let source = self.heap.root_value(receiver).unwrap();
            let then = self.get_property(p, source, then_atom)?;
            if !self.is_function(then) {
                return Err(
                    self.type_error(p, "Promise.prototype.finally then is not callable".into())
                );
            }
            then_root = Some(self.heap.root(then));
            let handler_value = self.heap.root_value(handler).unwrap();
            // Callbacks cannot escape before invocation. Register them only
            // after lookup succeeds, so a throwing getter retains no handler.
            let callbacks = if self.is_function(handler_value) {
                [false, true].map(|rejected| {
                    let handler = self.heap.root_value(handler).unwrap();
                    let constructor = self.heap.root_value(constructor_root.unwrap()).unwrap();
                    self.finally_handler_function(handler, constructor, rejected)
                })
            } else {
                [handler_value, handler_value]
            };
            let source = self.heap.root_value(receiver).unwrap();
            let then = self.heap.root_value(then_root.unwrap()).unwrap();
            self.call_value(p, then, source, &callbacks)
        })();
        for root in [constructor_root, then_root].into_iter().flatten() {
            self.heap.release_root(root);
        }
        self.heap.release_root(receiver);
        self.heap.release_root(handler);
        outcome
    }
}

impl PromiseRuntime {
    /// A snapshot (`crate::snapshot`) is taken between jobs, with no module loading and no
    /// combinator or reaction in flight, so only the rejection numbering carries over.
    pub(super) fn write_snapshot(
        &self,
        writer: &mut crate::snapshot::Writer,
    ) -> crate::snapshot::SnapshotResult<()> {
        let Self {
            rejection_notifications,
            last_rejection_id,
            finally_handler_callbacks,
            finally_continuation_callbacks,
            aggregates,
            aggregate_jobs,
            reaction_capabilities,
            modules,
            dynamic_import_jobs,
            async_module_order,
            module_sources,
            waiting_static_modules,
            active_native,
            async_module_settled,
        } = self;
        let pending = [
            (
                "a rejection notification",
                rejection_notifications.is_empty(),
            ),
            (
                "a finally callback",
                finally_handler_callbacks.is_empty() && finally_continuation_callbacks.is_empty(),
            ),
            (
                "a promise combinator",
                aggregates.is_empty() && aggregate_jobs.is_empty(),
            ),
            ("a reaction capability", reaction_capabilities.is_empty()),
            (
                "a module record",
                modules.is_empty() && async_module_order.is_empty() && module_sources.is_empty(),
            ),
            (
                "a module load",
                dynamic_import_jobs.is_empty() && waiting_static_modules.is_empty(),
            ),
            ("a settled async module", !async_module_settled),
            ("a native activation", active_native.is_empty()),
        ];
        if let Some((what, _)) = pending.iter().find(|(_, empty)| !empty) {
            return Err(crate::snapshot::SnapshotError::unsupported(what));
        }
        writer.u64(*last_rejection_id);
        Ok(())
    }

    pub(super) fn read_snapshot(
        reader: &mut crate::snapshot::Reader<'_>,
    ) -> crate::snapshot::SnapshotResult<Self> {
        Ok(Self {
            last_rejection_id: reader.u64()?,
            ..Self::default()
        })
    }
}

/// The slot of each atom among a function's locals: the first slot naming it, as a scan from
/// the first local finds it. Module linking looks up every import, export and hoisted binding.
fn local_slots(function: &crate::bytecode::Function) -> FxHashMap<Atom, usize> {
    let mut slots =
        FxHashMap::with_capacity_and_hasher(function.local_atoms.len(), Default::default());
    for (slot, atom) in function.local_atoms.iter().enumerate() {
        slots.entry(*atom).or_insert(slot);
    }
    slots
}
