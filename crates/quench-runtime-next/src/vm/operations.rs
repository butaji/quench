use super::*;
use oxc_ast::ast::BinaryOperator;

pub(super) enum ArrayLikeElementKind {
    Any,
    PropertyKey,
}

const EXPONENTIATION_ZERO: f64 = 0.0;
const EXPONENTIATION_ONE: f64 = 1.0;
const EXPONENTIATION_TWO: f64 = 2.0;
const ODD_INTEGER_PARITY: f64 = 1.0;
const LAST_NUMERIC_BINARY_OPERATOR: u32 = BinaryOperator::BitwiseAnd as u32;

#[derive(Clone, Copy)]
#[repr(u32)]
pub(super) enum RelationalOperator {
    LessThan = BinaryOperator::LessThan as u32,
    LessEqual = BinaryOperator::LessEqualThan as u32,
    GreaterThan = BinaryOperator::GreaterThan as u32,
    GreaterEqual = BinaryOperator::GreaterEqualThan as u32,
}

impl RelationalOperator {
    pub(super) fn from_immediate(immediate: u32) -> Option<Self> {
        Some(match immediate {
            value if value == Self::LessThan as u32 => Self::LessThan,
            value if value == Self::LessEqual as u32 => Self::LessEqual,
            value if value == Self::GreaterThan as u32 => Self::GreaterThan,
            value if value == Self::GreaterEqual as u32 => Self::GreaterEqual,
            _ => return None,
        })
    }

    pub(super) fn matches(self, ordering: std::cmp::Ordering) -> bool {
        use std::cmp::Ordering::{Equal, Greater, Less};
        match (self, ordering) {
            (Self::LessThan, Less) | (Self::LessEqual, Less | Equal) => true,
            (Self::GreaterThan, Greater) | (Self::GreaterEqual, Greater | Equal) => true,
            _ => false,
        }
    }
}

pub(super) enum OperandCoercion {
    PrimitiveDefault,
    PrimitiveNumber,
    Numeric,
}

impl<H: Host> Vm<H> {
    pub(super) fn call_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let this = self.string_method_receiver(p, native, this)?;
        if let Some(result) = self.string_html_method(p, native, this, args) {
            return result;
        }
        if Self::is_finalization_native(native) {
            return self.call_finalization_registry_native(p, native, this, args);
        }
        if Self::is_disposal_native(native) {
            return self.call_disposal_native(p, native, this, args);
        }
        if Self::is_collection_native(native) {
            return self.call_collection_native(p, native, this, args);
        }
        if matches!(
            native,
            Native::ShadowRealmEvaluate
                | Native::ShadowRealmImportValue
                | Native::ShadowRealmImportValueFulfilled
                | Native::ShadowRealmWrappedFunction
        ) {
            return self.shadow_realm_native(p, native, this, args);
        }
        if native == Native::ShadowRealm {
            return Err(self.type_error(p, "ShadowRealm constructor requires 'new'".into()));
        }
        if matches!(native, Native::WeakMap | Native::WeakSet | Native::WeakRef) {
            let constructor = match native {
                Native::WeakMap => "WeakMap",
                Native::WeakSet => "WeakSet",
                Native::WeakRef => "WeakRef",
                _ => unreachable!(),
            };
            return Err(self.type_error(
                p,
                format!("Constructor {constructor} requires 'new'").into(),
            ));
        }
        if let Some(result) = self.maybe_call_typed_array_native(p, native, this, args) {
            return result;
        }
        if native.is_typed_array_constructor() {
            return Err(self.type_error(p, "typed array constructor requires new".into()));
        }
        if native.is_atomics_native() {
            return self.atomics_native(p, native, args);
        }
        if native == Native::Test262Agent {
            return self.call_test262_agent(p, args);
        }
        if native.is_bigint_native() {
            return self.bigint_native(p, native, this, args);
        }
        if native.is_promise_native() {
            return self.call_promise_native(p, native, this, args);
        }
        if native == Native::AsyncFromSyncValue {
            return self.async_from_sync_value(args);
        }
        if native == Native::AsyncFromSyncValueRejected {
            return self.async_from_sync_value_rejected(p, args);
        }
        if native == Native::AsyncGeneratorReturnResult {
            return self.iterator_result(args.first().copied().unwrap_or(Value::UNDEFINED), true);
        }
        if native == Native::AsyncGeneratorReturnFulfilled {
            return self.async_generator_return_fulfilled(p, args);
        }
        if native == Native::AsyncGeneratorReturnRejected {
            return self.async_generator_return_rejected(p, args);
        }
        if native == Native::AsyncGeneratorDelegateReturnStart {
            return self.async_generator_delegate_return_start(p, args);
        }
        if native.is_object_static() {
            return self.call_object_native(p, native, args);
        }
        if native == Native::ProxyRevoke {
            let callee = self
                .realm
                .promise
                .active_native
                .last()
                .copied()
                .ok_or_else(|| JsError("proxy revoke requires an active callee".into()))?;
            return self.proxy_revoke(callee);
        }
        if native.is_data_view_native() {
            return self.data_view_native(p, native, this, args);
        }
        match native {
            Native::IsHTMLDDA => Ok(Value::NULL),
            Native::Proxy => Err(self.type_error(p, "Proxy must be called with new".into())),
            Native::DataView => Err(self.type_error(p, "DataView constructor requires new".into())),
            Native::AbstractModuleSourceToStringTag => {
                Ok(self.abstract_module_source_to_string_tag(this))
            }
            Native::WithEnter => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                if value.is_null() || value.is_undefined() {
                    return Err(self.type_error(p, "cannot convert nullish value to object".into()));
                }
                let object = self.box_object(value)?;
                self.with_stack.push(object);
                Ok(Value::UNDEFINED)
            }
            Native::WithExit => {
                self.with_stack.pop();
                Ok(Value::UNDEFINED)
            }
            Native::Eval => self.eval_native(p, args),
            Native::EvalScript => self.eval_script_native(p, args),
            Native::CollectGarbage => {
                self.collect_now(p);
                Ok(Value::UNDEFINED)
            }
            Native::ToString => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                let value = self.to_string(p, value)?;
                Ok(self.heap.alloc(Cell::String(value.into())))
            }
            Native::ProxyRevocable => self.proxy_revocable(p, args),
            Native::ArrayBuffer | Native::SharedArrayBuffer => {
                Err(self.type_error(p, "constructor requires new".into()))
            }
            native if native.is_host_control_native() => self.call_host(p, native, args),
            Native::Print => {
                let v = args.first().copied().unwrap_or(Value::UNDEFINED);
                let text = self.to_string(p, v)?;
                HostContext::new(&mut self.host).invoke(CapabilityId::WriteLine, Some(&text));
                Ok(Value::UNDEFINED)
            }
            Native::Date => self.date_call(),
            Native::IntlNumberFormat => self.intl_number_format_call(p, this, args),
            Native::IntlNumberFormatSupportedLocalesOf => {
                self.intl_number_format_supported_locales_of(p, args)
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
            Native::IntlPluralRules => Err(self.type_error(p, "constructor requires new".into())),
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
                self.intl_relative_time_format_supported_locales_of(p, args)
            }
            Native::IntlRelativeTimeFormatFormat
            | Native::IntlRelativeTimeFormatFormatToParts => {
                self.intl_relative_time_format_native(p, native, this, args)
            }
            Native::IntlSegmenter => Err(self.type_error(p, "constructor requires new".into())),
            Native::IntlSegmenterSupportedLocalesOf
            | Native::IntlSegmenterSegment
            | Native::IntlSegmenterResolvedOptions
            | Native::IntlSegmenterSegmentsIterator
            | Native::IntlSegmenterSegmentsContaining => {
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
            Native::IntlDisplayNames => Err(self.type_error(p, "constructor requires new".into())),
            Native::IntlDisplayNamesOf | Native::IntlDisplayNamesResolvedOptions => {
                self.intl_display_names_native(p, native, this, args)
            }
            Native::IntlDurationFormat => {
                Err(self.type_error(p, "constructor requires new".into()))
            }
            Native::IntlDurationFormatSupportedLocalesOf => {
                self.duration_format_supported_locales_of(p, args)
            }
            Native::IntlDurationFormatFormatGetter
            | Native::IntlDurationFormatFormat
            | Native::IntlDurationFormatFormatToParts
            | Native::IntlDurationFormatResolvedOptions => {
                self.intl_duration_format_native(p, native, this, args)
            }
            Native::IntlListFormat => Err(self.type_error(p, "constructor requires new".into())),
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
            Native::DateNow => Ok(Value::number(
                HostContext::new(&mut self.host)
                    .invoke(CapabilityId::ClockMillis, None)
                    .trunc(),
            )),
            Native::DateGetTime
            | Native::DateValueOf
            | Native::DateGetTimezoneOffset
            | Native::DateGetFullYear
            | Native::DateGetMonth
            | Native::DateGetDate
            | Native::DateGetDay
            | Native::DateGetHours
            | Native::DateGetMinutes
            | Native::DateGetSeconds
            | Native::DateGetMilliseconds
            | Native::DateGetUTCFullYear
            | Native::DateGetUTCMonth
            | Native::DateGetUTCDate
            | Native::DateGetUTCDay
            | Native::DateGetUTCHours
            | Native::DateGetUTCMinutes
            | Native::DateGetUTCSeconds
            | Native::DateGetUTCMilliseconds
            | Native::DateGetYear
            | Native::DateSetTime
            | Native::DateSetFullYear
            | Native::DateSetMonth
            | Native::DateSetUTCMonth
            | Native::DateSetDate
            | Native::DateSetUTCDate
            | Native::DateSetUTCFullYear
            | Native::DateSetHours
            | Native::DateSetMinutes
            | Native::DateSetSeconds
            | Native::DateSetMilliseconds
            | Native::DateSetUTCHours
            | Native::DateSetUTCMinutes
            | Native::DateSetUTCSeconds
            | Native::DateSetUTCMilliseconds
            | Native::DateSetYear
            | Native::DateToString
            | Native::DateToDateString
            | Native::DateToTimeString
            | Native::DateToUTCString
            | Native::DateToLocaleString
            | Native::DateToLocaleDateString
            | Native::DateToLocaleTimeString
            | Native::DateToISOString
            | Native::DateToJSON
            | Native::DateToPrimitive
            | Native::DateToTemporalInstant => self.date_native(p, native, this, args),
            Native::DateParse | Native::DateUTC => self.date_static_native(p, native, args),
            Native::RegExpCompile => self.regexp_compile_native(p, this, args),
            Native::RegExpExec => self.regexp_builtin_exec(p, this, args),
            Native::RegExpTest => self.regexp_test(p, this, args),
            Native::RegExpEscape => self.regexp_escape_native(p, args),
            Native::RegExpSymbolMatch => self.regexp_symbol_match(p, this, args),
            Native::RegExpSymbolSearch => self.regexp_symbol_search(p, this, args),
            Native::RegExpSymbolReplace => self.regexp_symbol_replace(p, this, args),
            Native::RegExpSymbolMatchAll => self.regexp_symbol_match_all(p, this, args),
            Native::RegExpSymbolSplit => self.regexp_symbol_split(p, this, args),
            Native::RegExpToString => self.regexp_to_string_native(p, this),
            Native::RegExpSpecies => Ok(this),
            Native::RegExpLegacyGetter => self.regexp_legacy_getter_native(p, this),
            Native::RegExpLegacySetter => self.regexp_legacy_setter_native(p, this, args),
            Native::RegExpGlobal
            | Native::RegExpIgnoreCase
            | Native::RegExpMultiline
            | Native::RegExpDotAll
            | Native::RegExpUnicode
            | Native::RegExpUnicodeSets
            | Native::RegExpSticky
            | Native::RegExpHasIndices => self.regexp_flag_native(p, native, this),
            Native::RegExpSource => self.regexp_slot_native(p, native, this),
            Native::RegExpFlags => self.regexp_flags_native(p, this),
            Native::ObjectPrototypeHasOwnProperty | Native::ObjectPrototypePropertyIsEnumerable => {
                let key_value =
                    self.to_property_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let receiver = self.box_object_or_type_error(p, this)?;
                let descriptor =
                    self.object_get_own_property_descriptor(p, &[receiver, key_value])?;
                if descriptor.is_undefined() {
                    return Ok(Value::FALSE);
                }
                if native == Native::ObjectPrototypeHasOwnProperty {
                    return Ok(Value::TRUE);
                }
                let enumerable = self.intern_atom("enumerable");
                let enumerable_value = self.get_property(p, descriptor, enumerable)?;
                Ok(if self.truthy(enumerable_value) {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::ObjectPrototypeLookupGetter | Native::ObjectPrototypeLookupSetter => {
                self.require_object_coercible(p, this)?;
                let key =
                    self.to_property_key(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let mut object = self.box_object(this)?;
                loop {
                    let descriptor = self.object_get_own_property_descriptor(p, &[object, key])?;
                    if !descriptor.is_undefined() {
                        let field =
                            self.intern_atom(if native == Native::ObjectPrototypeLookupGetter {
                                "get"
                            } else {
                                "set"
                            });
                        return self.get_property(p, descriptor, field);
                    }
                    object = self.object_get_prototype_of(p, object)?;
                    if object.is_null() {
                        return Ok(Value::UNDEFINED);
                    }
                }
            }
            Native::ObjectPrototypeDefineGetter | Native::ObjectPrototypeDefineSetter => {
                self.object_prototype_define_accessor(p, native, this, args)
            }
            Native::ObjectPrototypeToString => self.object_prototype_to_string(p, this),
            Native::ObjectPrototypeProtoGetter => {
                self.require_object_coercible(p, this)?;
                let object = self.box_object(this)?;
                self.object_get_prototype_of(p, object)
            }
            Native::ObjectPrototypeProtoSetter => {
                self.require_object_coercible(p, this)?;
                let proto = args.first().copied().unwrap_or(Value::UNDEFINED);
                if self.is_object_like(this)
                    && (proto.is_null() || self.object_data(proto).is_some())
                {
                    self.object_set_prototype_of(p, this, proto)?;
                }
                Ok(Value::UNDEFINED)
            }
            Native::ObjectPrototypeToLocaleString => {
                self.object_prototype_to_locale_string(p, this)
            }
            Native::ObjectPrototypeValueOf => self.box_object(this).map_err(|_| {
                self.type_error(
                    p,
                    "Object.prototype.valueOf called on null or undefined".into(),
                )
            }),
            Native::ObjectPrototypeIsPrototypeOf => {
                let target = args.first().copied().unwrap_or(Value::UNDEFINED);
                if self.object_data(target).is_none() {
                    return Ok(Value::FALSE);
                }
                let prototype = self.box_object(this)?;
                let mut current = target;
                let mut found = false;
                while self.is_object_like(current) {
                    current = self.object_get_prototype_of(p, current)?;
                    if current == prototype {
                        found = true;
                        break;
                    }
                    if current.is_null() {
                        break;
                    }
                }
                Ok(if found { Value::TRUE } else { Value::FALSE })
            }
            Native::ReflectGet
            | Native::ReflectHas
            | Native::ReflectApply
            | Native::ReflectGetOwnPropertyDescriptor
            | Native::ReflectDefineProperty
            | Native::ReflectDeleteProperty
            | Native::ReflectPreventExtensions
            | Native::ReflectIsExtensible
            | Native::ReflectSet
            | Native::SuperSet
            | Native::ObjectLiteralPrototype
            | Native::ReflectOwnKeys
            | Native::ReflectGetPrototypeOf
            | Native::ReflectSetPrototypeOf
            | Native::ReflectConstruct => self.call_reflect_native(p, native, args),
            Native::JsonParse => self.json_parse(p, args),
            Native::JsonStringify => self.json_stringify(p, args),
            Native::JsonRawJson => self.json_raw_json(p, args),
            Native::JsonIsRawJson => self.json_is_raw_json(args),
            Native::GlobalIsNaN => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                Ok(if self.to_number(p, value)?.is_nan() {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::GlobalIsFinite => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                Ok(if self.to_number(p, value)?.is_finite() {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::MathLog => {
                let v = args.first().copied().unwrap_or(Value::UNDEFINED);
                Ok(Value::number(self.to_number(p, v)?.ln()))
            }
            Native::MathPow => {
                let a = args.first().copied().unwrap_or(Value::UNDEFINED);
                let b = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                Ok(Value::number(exponentiate(
                    self.to_number(p, a)?,
                    self.to_number(p, b)?,
                )))
            }
            Native::ArrayPush => self.array_push_native(p, this, args),
            Native::ArrayIsArray => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                Ok(if self.is_array(p, value)? {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::ArrayPop => self.array_pop_native(p, this),
            Native::ArraySlice => self.array_slice_native(p, this, args),
            Native::ArrayIncludes => self.array_includes_native(p, this, args),
            Native::ArrayJoin => self.array_join_native(p, this, args),
            Native::ArrayConcat => self.array_concat_native(p, this, args),
            Native::ArrayFlat => self.array_flatten_native(p, native, this, args),
            Native::ArrayReverse => self.array_reverse_native(p, this),
            Native::ArrayShift => self.array_shift_native(p, this),
            Native::ArrayUnshift => self.array_unshift_native(p, this, args),
            Native::ArraySplice => self.array_splice_native(p, this, args),
            Native::ArrayFill => self.array_fill_native(p, this, args),
            Native::ArrayAt
            | Native::ArrayLastIndexOf
            | Native::ArrayIndexOf
            | Native::ArrayCopyWithin
            | Native::ArrayWith
            | Native::ArrayForEach
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
            | Native::ArrayFlatMap
            | Native::ArrayReduce
            | Native::ArrayReduceRight => self.array_indexed_native(p, native, this, args),
            Native::ArrayToReversed
            | Native::ArrayToSpliced
            | Native::ArraySort
            | Native::ArrayToSorted
            | Native::ArrayToString
            | Native::ArrayToLocaleString
            | Native::ArraySpecies => self.array_modern_native(p, native, this, args),
            Native::ArrayKeys | Native::ArrayValues | Native::ArrayEntries => {
                self.array_iterator_native(p, native, this)
            }
            Native::StringValues => self.string_iterator_native(p, this),
            Native::ArrayBufferSlice => self.array_buffer_slice_native(p, this, args, false),
            Native::SharedArrayBufferSlice => self.array_buffer_slice_native(p, this, args, true),
            Native::ArrayBufferTransfer => {
                self.array_buffer_transfer_native(p, this, args, true, false)
            }
            Native::ArrayBufferResize => self.array_buffer_resize_native(p, this, args),
            Native::ArrayBufferTransferToFixedLength => {
                self.array_buffer_transfer_native(p, this, args, false, false)
            }
            Native::ArrayBufferTransferToImmutable => {
                self.array_buffer_transfer_native(p, this, args, false, true)
            }
            Native::ArrayBufferSliceToImmutable => {
                self.array_buffer_slice_immutable_native(p, this, args)
            }
            Native::ArrayBufferByteLengthGetter
            | Native::ArrayBufferDetachedGetter
            | Native::ArrayBufferImmutableGetter
            | Native::ArrayBufferMaxByteLengthGetter
            | Native::ArrayBufferResizableGetter
            | Native::SharedArrayBufferByteLengthGetter
            | Native::SharedArrayBufferGrowableGetter
            | Native::SharedArrayBufferMaxByteLengthGetter => {
                self.array_buffer_getter_native(p, native, this)
            }
            Native::DetachArrayBuffer => self.detach_array_buffer_native(p, args),
            Native::SharedArrayBufferGrow => self.shared_array_buffer_grow_native(p, this, args),
            Native::ArrayFrom
            | Native::ArrayFromAsync
            | Native::ArrayOf
            | Native::TypedArrayFrom
            | Native::TypedArrayOf => self.array_modern_native(p, native, this, args),
            Native::TypedArrayToStringTag => Ok(self.typed_array_to_string_tag_native(this)),
            Native::TypedArrayBufferGetter
            | Native::TypedArrayByteLengthGetter
            | Native::TypedArrayByteOffsetGetter
            | Native::TypedArrayLengthGetter => self.typed_array_getter_native(p, native, this),
            Native::FunctionPrototype => Ok(Value::UNDEFINED),
            Native::FunctionPrototypeHasInstance => {
                let value = args.first().copied().unwrap_or(Value::UNDEFINED);
                Ok(if self.ordinary_has_instance(p, this, value)? {
                    Value::TRUE
                } else {
                    Value::FALSE
                })
            }
            Native::FunctionCall => {
                let receiver = args.first().copied().unwrap_or(Value::UNDEFINED);
                self.call_value(p, this, receiver, args.get(1..).unwrap_or_default())
            }
            Native::FunctionApply => {
                let receiver = args.first().copied().unwrap_or(Value::UNDEFINED);
                let argument_array = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let arguments = self.call_argument_list(p, argument_array, true)?;
                self.call_value(p, this, receiver, &arguments)
            }
            Native::FunctionBind => self.bind_function(p, this, args),
            Native::FunctionBoundCall => self.call_bound_function(p, args),
            Native::FunctionToString => {
                if !self.is_function(this) {
                    return Err(self.type_error(
                        p,
                        "Function.prototype.toString called on incompatible receiver".into(),
                    ));
                }
                let source_atom = self.intern_atom("\0rqj:function-source");
                if let Some(source) = self.own_property(this, source_atom) {
                    return Ok(source);
                }
                Ok(self
                    .heap
                    .alloc(Cell::String("function () { [native code] }".into())))
            }
            Native::Number
            | Native::NumberIsNaN
            | Native::NumberIsFinite
            | Native::NumberIsInteger
            | Native::NumberIsSafeInteger => self.call_number_native(p, native, args),
            Native::NumberFixed => self.number_format(p, this, args, native),
            Native::NumberPrecision => self.number_format(p, this, args, native),
            Native::String => {
                let argument = self.string_constructor_argument(args);
                let text = if let Some(Cell::Symbol(description)) = self.heap.get(argument) {
                    format!("Symbol({})", description.as_deref().unwrap_or(""))
                } else {
                    self.to_string(p, argument)?
                };
                Ok(self.heap.alloc(Cell::String(text.into())))
            }
            Native::Symbol => self.call_symbol_constructor(p, args),
            Native::SymbolToString
            | Native::SymbolToPrimitive
            | Native::SymbolValueOf
            | Native::SymbolDescriptionGetter => self.call_symbol_value_native(p, native, this),
            Native::SymbolFor | Native::SymbolKeyFor => self.call_symbol_native(p, native, args),
            Native::TemporalToLocaleString => match self.heap.get(this) {
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
            Native::TemporalPlainMonthDayToString
            | Native::TemporalPlainMonthDayToJSON
            | Native::TemporalPlainMonthDayToLocaleString
            | Native::TemporalPlainYearMonthToString
            | Native::TemporalPlainYearMonthToJSON
            | Native::TemporalPlainYearMonthToLocaleString => {
                super::temporal_date_projection::native(self, p, native, this, args)
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
            Native::Object
            | Native::Array
            | Native::Map
            | Native::Set
            | Native::WeakMap
            | Native::WeakSet
            | Native::WeakRef
            | Native::DisposableStack
            | Native::AsyncDisposableStack => self.construct_native(p, native, args),
            Native::RegExp => self.construct_regexp_native(p, args, None),
            Native::ThrowTypeError => {
                Err(self.type_error(p, "restricted arguments property".into()))
            }
            Native::FinalizationRegistry => {
                Err(self.type_error(p, "Constructor FinalizationRegistry requires 'new'".into()))
            }
            Native::FunctionCaller => {
                if this == self.function_proto || self.function_caller_is_restricted(this) {
                    Err(self.type_error(p, "restricted function caller access".into()))
                } else {
                    Ok(Value::UNDEFINED)
                }
            }
            Native::ErrorToString => self.error_to_string(p, this),
            Native::ErrorIsError => Ok(
                if self.error_is_error(args.first().copied().unwrap_or(Value::UNDEFINED)) {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            ),
            Native::ErrorStackGetter => self.error_stack_getter(p, this),
            Native::ErrorStackSetter => self.error_stack_setter(p, this, args),
            Native::ArrayBufferSpecies => Ok(this),
            Native::NumberExponential => self.number_exponential(p, this, args),
            native if native.is_error_constructor() => self.construct_native(p, native, args),
            _ => self.call_primitive_native(p, native, this, args),
        }
    }
    #[inline]
    pub(super) fn binary(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        left: Value,
        right: Value,
    ) -> Result<Value, JsError> {
        let numeric =
            (BinaryOperator::Subtraction as u32..=BinaryOperator::BitwiseAnd as u32).contains(&op);
        if numeric {
            if left.as_number().is_some() && right.as_number().is_some() {
                return self.binary_slow(p, op, left, right);
            }
            return self.with_coerced_operands(
                p,
                OperandCoercion::Numeric,
                left,
                right,
                |vm, left, right| {
                    if matches!(vm.heap.get(left), Some(Cell::BigInt(_)))
                        || matches!(vm.heap.get(right), Some(Cell::BigInt(_)))
                    {
                        vm.binary_bigint(p, op, left, right)
                    } else {
                        vm.binary_slow(p, op, left, right)
                    }
                },
            );
        }
        if let Some(operator) = RelationalOperator::from_immediate(op) {
            let result = self.compare_relational(p, operator, left, right)?;
            return Ok(Self::integrity_bool(result));
        }
        if op == BinaryOperator::Addition as u32 {
            if let Some((a, b)) = Value::int_pair(left, right) {
                return Ok(a
                    .checked_add(b)
                    .map(Value::integer)
                    .unwrap_or_else(|| Value::number(a as f64 + b as f64)));
            }
            return self.with_coerced_operands(
                p,
                OperandCoercion::PrimitiveDefault,
                left,
                right,
                |vm, left, right| {
                    if !vm.is_string(left)
                        && !vm.is_string(right)
                        && (matches!(vm.heap.get(left), Some(Cell::BigInt(_)))
                            || matches!(vm.heap.get(right), Some(Cell::BigInt(_))))
                    {
                        vm.binary_bigint(p, op, left, right)
                    } else {
                        vm.binary_slow(p, op, left, right)
                    }
                },
            );
        }
        if op <= BinaryOperator::StrictInequality as u32
            && let Some((a, b)) = Value::int_pair(left, right)
        {
            return Ok(Self::integrity_bool(
                if op == BinaryOperator::Equality as u32 || op == BinaryOperator::StrictEquality as u32
                {
                    a == b
                } else {
                    a != b
                },
            ));
        }
        self.binary_slow(p, op, left, right)
    }

    pub(super) fn with_coerced_operands<R>(
        &mut self,
        p: &ResidualProgram,
        coercion: OperandCoercion,
        left: Value,
        right: Value,
        operation: impl FnOnce(&mut Self, Value, Value) -> Result<R, JsError>,
    ) -> Result<R, JsError> {
        self.with_call_roots(
            [left, right].into_iter().filter(|value| value.is_heap()),
            |vm| {
                let convert = |vm: &mut Self, value| match coercion {
                    OperandCoercion::PrimitiveDefault => vm.to_primitive(p, value, "default"),
                    OperandCoercion::PrimitiveNumber => vm.to_primitive(p, value, "number"),
                    OperandCoercion::Numeric => vm.to_numeric(p, value),
                };
                let left = convert(vm, left)?;
                vm.with_call_roots(
                    std::iter::once(left).filter(|value| value.is_heap()),
                    |vm| {
                        let right = convert(vm, right)?;
                        vm.with_call_roots(
                            std::iter::once(right).filter(|value| value.is_heap()),
                            |vm| operation(vm, left, right),
                        )
                    },
                )
            },
        )
    }

    fn binary_bigint(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        left: Value,
        right: Value,
    ) -> Result<Value, JsError> {
        let Some(Cell::BigInt(left)) = self.heap.get(left) else {
            return Err(self.type_error(p, "Cannot mix BigInt and other types".into()));
        };
        let Some(Cell::BigInt(right)) = self.heap.get(right) else {
            return Err(self.type_error(p, "Cannot mix BigInt and other types".into()));
        };
        let result = match op {
            8 => crate::bigint::binary(left, right, |a, b| Ok(a + b)),
            9 => crate::bigint::binary(left, right, |a, b| Ok(a - b)),
            10 => crate::bigint::binary(left, right, |a, b| Ok(a * b)),
            11 => crate::bigint::binary(left, right, |a, b| {
                if b == 0.into() {
                    Err(crate::bigint::Error::DivisionByZero)
                } else {
                    Ok(a / b)
                }
            }),
            12 => crate::bigint::binary(left, right, |a, b| {
                if b == 0.into() {
                    Err(crate::bigint::Error::DivisionByZero)
                } else {
                    Ok(a % b)
                }
            }),
            13 => crate::bigint::binary(left, right, |a, b| {
                if b.sign() == num_bigint::Sign::Minus {
                    return Err(crate::bigint::Error::NegativeExponent);
                }
                let exponent = b
                    .to_str_radix(10)
                    .parse::<u32>()
                    .map_err(|_| crate::bigint::Error::ExponentTooLarge)?;
                Ok(a.pow(exponent))
            }),
            14 => crate::bigint::shift(left, right, true),
            15 => crate::bigint::shift(left, right, false),
            17 => crate::bigint::binary(left, right, |a, b| Ok(a | b)),
            18 => crate::bigint::binary(left, right, |a, b| Ok(a ^ b)),
            19 => crate::bigint::binary(left, right, |a, b| Ok(a & b)),
            _ => return Err(self.type_error(p, "BigInt operation is not supported".into())),
        };
        match result {
            Ok(value) => Ok(self.heap.alloc(Cell::BigInt(value))),
            Err(crate::bigint::Error::DivisionByZero) => {
                Err(self.range_error(p, "Division by zero".into()))
            }
            Err(crate::bigint::Error::NegativeExponent) => {
                Err(self.range_error(p, "Exponent must be positive".into()))
            }
            Err(crate::bigint::Error::ExponentTooLarge) => {
                Err(self.range_error(p, "Maximum BigInt size exceeded".into()))
            }
            Err(crate::bigint::Error::InvalidDecimal) => {
                Err(self.type_error(p, "Invalid BigInt value".into()))
            }
        }
    }
    #[cold]
    #[inline(never)]
    pub(super) fn binary_slow(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        left: Value,
        right: Value,
    ) -> Result<Value, JsError> {
        if op <= LAST_NUMERIC_BINARY_OPERATOR
            && let (Some(a), Some(b)) = (left.as_number(), right.as_number())
        {
            return Ok(match op {
                0 | 2 => {
                    if a == b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                1 | 3 => {
                    if a != b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                4 => {
                    if a < b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                5 => {
                    if a <= b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                6 => {
                    if a > b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                7 => {
                    if a >= b {
                        Value::TRUE
                    } else {
                        Value::FALSE
                    }
                }
                8 => Value::number(a + b),
                9 => Value::number(a - b),
                10 => Value::number(a * b),
                11 => Value::number(a / b),
                12 => Value::number(a % b),
                13 => Value::number(exponentiate(a, b)),
                14 => Value::number(((number_to_u32(a) as i32) << (number_to_u32(b) & 31)) as f64),
                15 => Value::number(((number_to_u32(a) as i32) >> (number_to_u32(b) & 31)) as f64),
                16 => Value::number((number_to_u32(a) >> (number_to_u32(b) & 31)) as f64),
                17 => Value::number(((number_to_u32(a) as i32) | (number_to_u32(b) as i32)) as f64),
                18 => Value::number(((number_to_u32(a) as i32) ^ (number_to_u32(b) as i32)) as f64),
                19 => Value::number(((number_to_u32(a) as i32) & (number_to_u32(b) as i32)) as f64),
                _ => return Err(JsError(format!("unsupported binary operator {op}").into())),
            });
        }
        if op == 8 && (self.is_string(left) || self.is_string(right)) {
            if let Some(value) = self.intern_dynamic_concat(left, right) {
                #[cfg(feature = "profile-aggregate")]
                self.profile.string_concat(true);
                return Ok(value);
            }
            #[cfg(feature = "profile-aggregate")]
            self.profile.string_concat(false);
            let mut text = self.coerce_js_string(p, left)?;
            text.push_js_string(&self.coerce_js_string(p, right)?);
            return Ok(self.intern_dynamic_value(text));
        }
        if let Some(operator) = RelationalOperator::from_immediate(op) {
            let result = self.compare_relational(p, operator, left, right)?;
            return Ok(Self::integrity_bool(result));
        }
        let answer = match op {
            0 => self.equal(p, left, right)?,
            1 => !self.equal(p, left, right)?,
            2 => self.strict_equal(left, right),
            3 => !self.strict_equal(left, right),
            20 => self.has_property(p, right, left)?,
            21 => self.instanceof(p, left, right)?,
            _ => return Ok(Value::number(self.numeric(p, op, left, right)?)),
        };
        Ok(if answer { Value::TRUE } else { Value::FALSE })
    }
    #[inline(always)]
    pub(super) fn binary_truthy(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        left: Value,
        right: Value,
    ) -> Result<bool, JsError> {
        if op <= 7
            && let Some((a, b)) = Value::int_pair(left, right)
        {
            return Ok(match op {
                0 | 2 => a == b,
                1 | 3 => a != b,
                4 => a < b,
                5 => a <= b,
                6 => a > b,
                7 => a >= b,
                _ => unreachable!(),
            });
        }
        let value = self.binary(p, op, left, right)?;
        Ok(self.truthy(value))
    }
    pub(super) fn numeric(
        &mut self,
        p: &ResidualProgram,
        op: u32,
        left: Value,
        right: Value,
    ) -> Result<f64, JsError> {
        let a = self.to_number(p, left)?;
        let b = self.to_number(p, right)?;
        Ok(match op {
            8 => a + b,
            9 => a - b,
            10 => a * b,
            11 => a / b,
            12 => a % b,
            13 => exponentiate(a, b),
            14 => ((number_to_u32(a) as i32) << (number_to_u32(b) & 31)) as f64,
            15 => ((number_to_u32(a) as i32) >> (number_to_u32(b) & 31)) as f64,
            16 => (number_to_u32(a) >> (number_to_u32(b) & 31)) as f64,
            17 => ((number_to_u32(a) as i32) | (number_to_u32(b) as i32)) as f64,
            18 => ((number_to_u32(a) as i32) ^ (number_to_u32(b) as i32)) as f64,
            19 => ((number_to_u32(a) as i32) & (number_to_u32(b) as i32)) as f64,
            _ => return Err(JsError(format!("unsupported binary operator {op}").into())),
        })
    }

    pub(super) fn call_argument_list(
        &mut self,
        p: &ResidualProgram,
        list: Value,
        allow_nullish: bool,
    ) -> Result<Vec<Value>, JsError> {
        if allow_nullish && (list.is_null() || list.is_undefined()) {
            return Ok(Vec::new());
        }
        self.create_list_from_array_like(p, list, ArrayLikeElementKind::Any)
    }

    pub(super) fn create_list_from_array_like(
        &mut self,
        p: &ResidualProgram,
        list: Value,
        element_kind: ArrayLikeElementKind,
    ) -> Result<Vec<Value>, JsError> {
        if !self.is_object_like(list) {
            return Err(self.type_error(p, "array-like list must be an object".into()));
        }
        let object = self.heap.root(list);
        let mut elements = Vec::new();
        let outcome = (|| {
            let length_atom = self.intern_atom("length");
            let length_value =
                self.get_property(p, self.heap.root_value(object).unwrap(), length_atom)?;
            let length_value = self.heap.root(length_value);
            let length_number = self.to_number(p, self.heap.root_value(length_value).unwrap());
            self.heap.release_root(length_value);
            let length_number = length_number?;
            let length = if length_number.is_nan() || length_number <= 0.0 {
                0
            } else {
                length_number.floor().min(MAX_SAFE_INTEGER) as usize
            };
            elements
                .try_reserve(length)
                .map_err(|_| self.type_error(p, "array-like list is too large".into()))?;
            for index in 0..length {
                let atom = self.intern_atom(&index.to_string());
                let element = self.get_property(p, self.heap.root_value(object).unwrap(), atom)?;
                if matches!(element_kind, ArrayLikeElementKind::PropertyKey)
                    && !matches!(
                        self.heap.get(element),
                        Some(Cell::String(_) | Cell::Symbol(_))
                    )
                {
                    return Err(
                        self.type_error(p, "array-like list contains an invalid property key".into())
                    );
                }
                elements.push(self.heap.root(element));
            }
            Ok(elements
                .iter()
                .map(|root| self.heap.root_value(*root).unwrap())
                .collect())
        })();
        for root in elements {
            self.heap.release_root(root);
        }
        self.heap.release_root(object);
        outcome
    }

}

fn exponentiate(base: f64, exponent: f64) -> f64 {
    if exponent.is_nan() {
        return f64::NAN;
    }
    if exponent == EXPONENTIATION_ZERO {
        return EXPONENTIATION_ONE;
    }
    if base.is_nan() || base.abs() == EXPONENTIATION_ONE && exponent.is_infinite() {
        return f64::NAN;
    }
    if base.is_infinite() {
        return infinite_power(base, exponent);
    }
    if base == EXPONENTIATION_ZERO {
        return zero_power(base, exponent);
    }
    base.powf(exponent)
}

fn infinite_power(base: f64, exponent: f64) -> f64 {
    let magnitude = if exponent.is_sign_positive() {
        f64::INFINITY
    } else {
        EXPONENTIATION_ZERO
    };
    signed_power_magnitude(base, exponent, magnitude)
}

fn zero_power(base: f64, exponent: f64) -> f64 {
    let magnitude = if exponent.is_sign_positive() {
        EXPONENTIATION_ZERO
    } else {
        f64::INFINITY
    };
    signed_power_magnitude(base, exponent, magnitude)
}

fn signed_power_magnitude(base: f64, exponent: f64, magnitude: f64) -> f64 {
    if base.is_sign_negative() && is_odd_integer(exponent) {
        -magnitude
    } else {
        magnitude
    }
}

fn is_odd_integer(value: f64) -> bool {
    value.is_finite()
        && value.fract() == EXPONENTIATION_ZERO
        && value.abs() % EXPONENTIATION_TWO == ODD_INTEGER_PARITY
}
