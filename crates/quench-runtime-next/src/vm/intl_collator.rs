use super::*;

const DEFAULT_COLLATOR_LOCALE: &str = "en";
const COLLATOR_LOCALE_SLOT: &str = "\0rqj:intl-collator-locale";
const COLLATOR_USAGE_SLOT: &str = "\0rqj:intl-collator-usage";
const COLLATOR_SENSITIVITY_SLOT: &str = "\0rqj:intl-collator-sensitivity";
const COLLATOR_IGNORE_PUNCTUATION_SLOT: &str = "\0rqj:intl-collator-ignore-punctuation";
const COLLATOR_NUMERIC_SLOT: &str = "\0rqj:intl-collator-numeric";
const COLLATOR_CASE_FIRST_SLOT: &str = "\0rqj:intl-collator-case-first";
const COLLATOR_COLLATION_SLOT: &str = "\0rqj:intl-collator-collation";
const COLLATOR_BOUND_COMPARE_SLOT: &str = "\0rqj:intl-collator-bound-compare";
const UNICODE_KEY_LENGTH: usize = 2;

#[derive(Default)]
struct CollatorOverrides {
    numeric: bool,
    case_first: bool,
    ignore_punctuation: bool,
}

impl<H: Host> Vm<H> {
    pub(super) fn install_intl_collator_for_realm(
        &mut self,
        program: &ResidualProgram,
        intl: Value,
        global: Value,
        object_prototype: Value,
    ) -> Result<(), JsError> {
        let constructor = self.native_with_realm(Native::IntlCollator, global, global);
        self.realm.intrinsics.intl_collator_constructors.insert(global, constructor);
        self.set_builtin_function_name(constructor, "Collator")?;
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.realm.intrinsics.intl_collator_prototypes.insert(global, prototype);
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        self.set_non_writable_property(constructor, "prototype");
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.install_builtin_to_string_tag(prototype, "Intl.Collator")?;
        let resolved = self.native_with_realm(Native::IntlCollatorResolvedOptions, global, global);
        self.set_builtin_function_name(resolved, "resolvedOptions")?;
        self.set_builtin_value_named(prototype, "resolvedOptions", resolved)?;
        let compare_getter =
            self.native_with_realm(Native::IntlCollatorCompareGetter, global, global);
        self.set_builtin_function_name(compare_getter, "get compare")?;
        let compare_atom = self.intern_atom("compare");
        self.set_builtin_value_named(prototype, "compare", compare_getter)?;
        self.set_property_attributes(
            prototype,
            PropertyKey::string(compare_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(compare_getter),
                setter: None,
            },
        );
        self.set_builtin_value_named(intl, "Collator", constructor)?;
        let supported =
            self.native_with_realm(Native::IntlCollatorSupportedLocalesOf, global, global);
        self.set_builtin_function_name(supported, "supportedLocalesOf")?;
        self.set_builtin_value_named(constructor, "supportedLocalesOf", supported)?;
        let _ = program;
        Ok(())
    }

    pub(super) fn set_non_writable_property(&mut self, object: Value, name: &str) {
        let atom = self.intern_atom(name);
        self.set_property_attributes(
            object,
            PropertyKey::string(atom),
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

    pub(super) fn intl_collator_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        self.with_call_roots(args.iter().copied().chain([new_target]), |self_| {
            let prototype = self_.intl_instance_prototype(p, new_target, Native::IntlCollator)?;
            self_.with_call_roots([prototype], |vm| {
            let requested_locale = vm.collator_locale(p, args.first().copied())?;
            let (usage, sensitivity, ignore_punctuation, numeric, case_first, collation, overrides) =
                vm.collator_options(p, args.get(1).copied())?;
            let (locale, collation, numeric, case_first, ignore_punctuation) =
                normalize_collator_locale(
                    &requested_locale,
                    collation,
                    numeric,
                    case_first,
                    ignore_punctuation,
                    overrides,
                );

            let collator = vm
                .heap
                .alloc(Cell::Object(Self::empty_object(prototype)));
            vm.set_collator_string(collator, COLLATOR_LOCALE_SLOT, &locale)?;
            vm.set_collator_string(collator, COLLATOR_USAGE_SLOT, &usage)?;
            vm.set_collator_string(collator, COLLATOR_SENSITIVITY_SLOT, &sensitivity)?;
            vm.set_collator_string(collator, COLLATOR_CASE_FIRST_SLOT, &case_first)?;
            vm.set_collator_string(collator, COLLATOR_COLLATION_SLOT, &collation)?;
            vm.set_collator_value(
                collator,
                COLLATOR_IGNORE_PUNCTUATION_SLOT,
                if ignore_punctuation {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            )?;
            vm.set_collator_value(
                collator,
                COLLATOR_NUMERIC_SLOT,
                if numeric { Value::TRUE } else { Value::FALSE },
            )?;
            Ok(collator)
            })
        })
    }

    pub(super) fn collator_locale(
        &mut self,
        p: &ResidualProgram,
        locales: Option<Value>,
    ) -> Result<String, JsError> {
        Ok(self
            .canonical_locale_list(p, locales)?
            .into_iter()
            .next()
            .unwrap_or_else(|| DEFAULT_COLLATOR_LOCALE.into()))
    }

    fn collator_options(
        &mut self,
        p: &ResidualProgram,
        options: Option<Value>,
    ) -> Result<
        (
            String,
            String,
            bool,
            bool,
            String,
            Option<String>,
            CollatorOverrides,
        ),
        JsError,
    > {
        let (mut usage, mut sensitivity, mut ignore_punctuation, mut numeric, mut case_first) = (
            "sort".to_owned(),
            "variant".to_owned(),
            false,
            false,
            "false".to_owned(),
        );
        let mut collation = None;
        let mut overrides = CollatorOverrides::default();
        let Some(options) = options.filter(|value| !value.is_undefined()) else {
            return Ok((
                usage,
                sensitivity,
                ignore_punctuation,
                numeric,
                case_first,
                collation,
                overrides,
            ));
        };
        if options.is_null() {
            return Err(self.type_error(p, "options must not be null".into()));
        }
        let options = self.box_object(options)?;
        self.with_call_roots([options], |self_| {
            for key in [
                "usage",
                "localeMatcher",
                "collation",
                "numeric",
                "caseFirst",
                "sensitivity",
                "ignorePunctuation",
            ] {
                let atom = self_.intern_atom(key);
                let value = self_.get_property(p, options, atom)?;
                if value.is_undefined() {
                    continue;
                }
                match key {
                    "usage" => {
                        usage = self_.to_string(p, value)?;
                        validate_collator_option(p, self_, &usage, &["sort", "search"], key)?;
                    }
                    "localeMatcher" => {
                        let matcher = self_.to_string(p, value)?;
                        validate_collator_option(p, self_, &matcher, &["lookup", "best fit"], key)?;
                    }
                    "collation" => collation = Some(self_.to_string(p, value)?),
                    "numeric" => {
                        numeric = self_.truthy(value);
                        overrides.numeric = true;
                    }
                    "caseFirst" => {
                        case_first = self_.to_string(p, value)?;
                        overrides.case_first = true;
                        validate_collator_option(
                            p,
                            self_,
                            &case_first,
                            &["upper", "lower", "false"],
                            key,
                        )?;
                    }
                    "sensitivity" => {
                        sensitivity = self_.to_string(p, value)?;
                        validate_collator_option(
                            p,
                            self_,
                            &sensitivity,
                            &["base", "accent", "case", "variant"],
                            key,
                        )?;
                    }
                    "ignorePunctuation" => {
                        ignore_punctuation = self_.truthy(value);
                        overrides.ignore_punctuation = true;
                    }
                    _ => unreachable!(),
                }
            }
            Ok((
                usage,
                sensitivity,
                ignore_punctuation,
                numeric,
                case_first,
                collation,
                overrides,
            ))
        })
    }

    pub(super) fn intl_collator_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::IntlCollator => {
                let constructor = self.realm.intrinsics.intl_collator_constructors
                    .get(&self.realm.globals)
                    .copied()
                    .ok_or_else(|| JsError("Intl.Collator intrinsic is not installed".into()))?;
                self.intl_collator_construct(p, args, constructor)
            }
            Native::IntlCollatorCompareGetter => self.collator_compare_getter(p, this),
            Native::IntlCollatorCompare => self.collator_compare(p, this, args),
            Native::IntlCollatorResolvedOptions => self.collator_resolved_options(p, this),
            Native::IntlCollatorSupportedLocalesOf => self.collator_supported_locales_of(p, args),
            _ => Err(JsError("invalid Intl.Collator method".into())),
        }
    }

    fn collator_compare_getter(&mut self, p: &ResidualProgram, this: Value) -> Result<Value, JsError> {
        self.with_call_roots([this], |self_| {
            if self_.collator_locale_slot(this).is_none() {
                return Err(self_.type_error(p, "not an Intl object".into()));
            }
            if let Some(bound) = self_.collator_value_slot(this, COLLATOR_BOUND_COMPARE_SLOT) {
                return Ok(bound);
            }
            let compare = self_.native_with_realm(
                Native::IntlCollatorCompare,
                Value::NULL,
                self_.realm.globals,
            );
            self_.set_builtin_function_name(compare, "compare")?;
            let bound = self_.bind_function(p, compare, &[this])?;
            let name = self_.heap.alloc(Cell::String("".into()));
            let name_atom = self_.intern_atom("name");
            self_.set_property_attributes(
                bound,
                PropertyKey::string(name_atom),
                PropertyAttributes {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
            self_.set_property(bound, name_atom, name)?;
            self_.set_property_attributes(
                bound,
                PropertyKey::string(name_atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
            self_.set_collator_value(this, COLLATOR_BOUND_COMPARE_SLOT, bound)?;
            Ok(bound)
        })
    }

    fn collator_compare(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.with_call_roots(args.iter().copied().chain([this]), |self_| {
            let locale = self_
                .collator_locale_slot(this)
                .ok_or_else(|| self_.type_error(p, "not an Intl object".into()))?;
            let left = self_.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
            let right = self_.to_string(p, args.get(1).copied().unwrap_or(Value::UNDEFINED))?;
            let options = quench_intl::CollatorOptions {
                ignore_punctuation: self_.collator_bool_slot(this, COLLATOR_IGNORE_PUNCTUATION_SLOT),
                sensitivity: &self_
                    .collator_string_slot(this, COLLATOR_SENSITIVITY_SLOT)
                    .unwrap_or_else(|| "variant".into()),
                usage: &self_
                    .collator_string_slot(this, COLLATOR_USAGE_SLOT)
                    .unwrap_or_else(|| "sort".into()),
                numeric: self_.collator_bool_slot(this, COLLATOR_NUMERIC_SLOT),
                case_first: &self_
                    .collator_string_slot(this, COLLATOR_CASE_FIRST_SLOT)
                    .unwrap_or_else(|| "false".into()),
            };
            Ok(Value::number(quench_intl::compare_collator(
                &left, &right, &locale, &options,
            )))
        })
    }

    fn collator_resolved_options(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        let locale = self
            .collator_locale_slot(this)
            .ok_or_else(|| self.type_error(p, "not an Intl object".into()))?;
        let result = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)));
        for (key, value) in [
            ("locale", self.heap.alloc(Cell::String(locale.into()))),
            (
                "usage",
                self.collator_string_slot(this, COLLATOR_USAGE_SLOT)
                    .map_or(Value::UNDEFINED, |v| {
                        self.heap.alloc(Cell::String(v.into()))
                    }),
            ),
            (
                "sensitivity",
                self.collator_string_slot(this, COLLATOR_SENSITIVITY_SLOT)
                    .map_or(Value::UNDEFINED, |v| {
                        self.heap.alloc(Cell::String(v.into()))
                    }),
            ),
            (
                "ignorePunctuation",
                if self.collator_bool_slot(this, COLLATOR_IGNORE_PUNCTUATION_SLOT) {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            ),
            (
                "collation",
                self.heap.alloc(Cell::String(
                    self.collator_string_slot(this, COLLATOR_COLLATION_SLOT)
                        .unwrap_or_else(|| "default".into())
                        .into(),
                )),
            ),
            (
                "numeric",
                if self.collator_bool_slot(this, COLLATOR_NUMERIC_SLOT) {
                    Value::TRUE
                } else {
                    Value::FALSE
                },
            ),
            (
                "caseFirst",
                self.collator_string_slot(this, COLLATOR_CASE_FIRST_SLOT)
                    .map_or(Value::UNDEFINED, |v| {
                        self.heap.alloc(Cell::String(v.into()))
                    }),
            ),
        ] {
            let atom = self.intern_atom(key);
            self.set_property(result, atom, value)?;
        }
        Ok(result)
    }

    fn collator_supported_locales_of(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let locales = self
            .canonical_locale_list(p, args.first().copied())?
            .into_iter()
            .filter(|locale| super::intl_number::is_supported_locale(locale))
            .collect::<Vec<_>>();
        let elements = locales
            .into_iter()
            .map(|locale| self.heap.alloc(Cell::String(locale.into())))
            .collect::<Vec<_>>();
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(elements),
        }))
    }



    fn set_collator_string(
        &mut self,
        object: Value,
        key: &str,
        value: &str,
    ) -> Result<(), JsError> {
        let value = self.heap.alloc(Cell::String(value.into()));
        self.set_collator_value(object, key, value)
    }

    fn set_collator_value(
        &mut self,
        object: Value,
        key: &str,
        value: Value,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom(key);
        self.set_property(object, atom, value)
    }

    fn collator_value_slot(&self, object: Value, key: &str) -> Option<Value> {
        self.lookup_atom(key)
            .and_then(|atom| self.own_property(object, atom))
    }

    fn collator_string_slot(&self, object: Value, key: &str) -> Option<String> {
        self.collator_value_slot(object, key)
            .and_then(|value| match self.heap.get(value) {
                Some(Cell::String(text)) => Some(text.to_string()),
                _ => None,
            })
    }

    fn collator_bool_slot(&self, object: Value, key: &str) -> bool {
        self.collator_value_slot(object, key)
            .is_some_and(|value| self.truthy(value))
    }

    fn collator_locale_slot(&self, object: Value) -> Option<String> {
        self.collator_string_slot(object, COLLATOR_LOCALE_SLOT)
    }
}

fn validate_collator_option<H: Host>(
    p: &ResidualProgram,
    vm: &mut Vm<H>,
    value: &str,
    allowed: &[&str],
    name: &str,
) -> Result<(), JsError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(vm.range_error(p, format!("invalid {name}").into()))
    }
}

fn normalize_collator_locale(
    locale: &str,
    requested_collation: Option<String>,
    mut numeric: bool,
    mut case_first: String,
    mut ignore_punctuation: bool,
    overrides: CollatorOverrides,
) -> (String, String, bool, String, bool) {
    let parts = locale.split('-').collect::<Vec<_>>();
    let private_use = parts
        .iter()
        .position(|part| part.eq_ignore_ascii_case("x"))
        .unwrap_or(parts.len());
    let unicode = parts[..private_use]
        .iter()
        .position(|part| part.eq_ignore_ascii_case("u"));
    let Some(unicode) = unicode else {
        if !overrides.ignore_punctuation {
            ignore_punctuation = locale.starts_with("th-") || locale == "th";
        }
        let collation = requested_collation
            .as_deref()
            .filter(|value| quench_intl::collation_supported(locale, value))
            .unwrap_or("default")
            .to_owned();
        return (
            locale.to_owned(),
            collation,
            numeric,
            case_first,
            ignore_punctuation,
        );
    };
    let extension_end = parts
        .iter()
        .enumerate()
        .skip(unicode + 1)
        .find_map(|(index, part)| (part.len() == 1).then_some(index))
        .unwrap_or(parts.len());
    let mut extension = parts[unicode + 1..extension_end].to_vec();
    let mut attributes = Vec::new();
    while extension
        .first()
        .is_some_and(|part| part.len() != UNICODE_KEY_LENGTH)
    {
        attributes.push(extension.remove(0));
    }
    let mut keys = Vec::<(String, Vec<String>)>::new();
    let mut cursor = 0;
    while cursor < extension.len() {
        let key = extension[cursor].to_ascii_lowercase();
        cursor += 1;
        let start = cursor;
        while cursor < extension.len() && extension[cursor].len() != UNICODE_KEY_LENGTH {
            cursor += 1;
        }
        keys.push((
            key,
            extension[start..cursor]
                .iter()
                .map(|part| (*part).to_owned())
                .collect(),
        ));
    }
    let ext_value = |key: &str| {
        keys.iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_slice())
    };
    if !overrides.numeric
        && let Some(value) = ext_value("kn")
    {
        numeric = value.first().is_none_or(|value| value == "true");
    }
    if !overrides.case_first
        && let Some(value) = ext_value("kf")
        && let Some(value) = value.first()
    {
        case_first = value.clone();
    }
    if !overrides.ignore_punctuation {
        ignore_punctuation = locale.starts_with("th-") || locale == "th";
    }
    let extension_collation = ext_value("co").and_then(|value| value.first()).cloned();
    let has_numeric_extension = ext_value("kn").is_some();
    let extension_numeric =
        ext_value("kn").is_some_and(|value| value.first().is_none_or(|value| value == "true"));
    let supported = |collation: &str| quench_intl::collation_supported(locale, collation);
    let collation = requested_collation
        .as_ref()
        .filter(|value| supported(value))
        .cloned()
        .or_else(|| {
            extension_collation
                .as_ref()
                .filter(|value| supported(value))
                .cloned()
        })
        .unwrap_or_else(|| "default".into());
    let mut keep = Vec::new();
    for (key, values) in keys {
        match key.as_str() {
            "co" if collation == "default" => {}
            "co" if requested_collation
                .as_ref()
                .is_some_and(|value| supported(value))
                && requested_collation.as_deref() != extension_collation.as_deref() => {}
            "co" => keep.push((key, values)),
            "kn" if overrides.numeric && numeric != extension_numeric => {}
            "kn" if values.first().is_some_and(|value| value == "true") => {
                keep.push((key, Vec::new()))
            }
            "kn" => keep.push((key, values)),
            "kf" if overrides.case_first
                && values.first().is_some_and(|value| value != &case_first) => {}
            "kf" => keep.push((key, values)),
            _ => {}
        }
    }
    if overrides.numeric
        && numeric
        && extension_numeric
        && has_numeric_extension
        && !keep.iter().any(|(key, _)| key == "kn")
    {
        keep.push(("kn".into(), Vec::new()));
    }
    // Unicode extension attributes are accepted but do not affect resolution.
    let mut suffix = Vec::new();
    for (key, values) in keep {
        suffix.push(key);
        suffix.extend(values);
    }
    let mut resolved = parts[..unicode].join("-");
    if !suffix.is_empty() {
        resolved.push_str("-u-");
        resolved.push_str(&suffix.join("-"));
    }
    if extension_end < parts.len() {
        resolved.push('-');
        resolved.push_str(&parts[extension_end..].join("-"));
    }
    (resolved, collation, numeric, case_first, ignore_punctuation)
}
