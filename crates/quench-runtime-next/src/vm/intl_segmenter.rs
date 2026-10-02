use super::*;
use unicode_segmentation::UnicodeSegmentation;

const SEGMENTER_LOCALE_SLOT: &str = "\0rqj:intl-segmenter-locale";
const SEGMENTER_GRANULARITY_SLOT: &str = "\0rqj:intl-segmenter-granularity";
const SEGMENTS_DATA_SLOT: &str = "\0rqj:intl-segments-data";
const SEGMENTS_INPUT_SLOT: &str = "\0rqj:intl-segments-input";

impl<H: Host> Vm<H> {
    pub(super) fn install_intl_segmenter_for_realm(
        &mut self,
        intl: Value,
        global: Value,
        object_prototype: Value,
    ) -> Result<(), JsError> {
        let constructor = self.native_with_realm(Native::IntlSegmenter, global, global);
        self.set_builtin_function_name(constructor, "Segmenter")?;
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.realm.intrinsics.intl_segmenter_prototypes.insert(global, prototype);
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        self.set_non_writable_property(constructor, "prototype");
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.install_builtin_to_string_tag(prototype, "Intl.Segmenter")?;
        for (name, native) in [
            ("segment", Native::IntlSegmenterSegment),
            ("resolvedOptions", Native::IntlSegmenterResolvedOptions),
        ] {
            let method = self.native_with_realm(native, global, global);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        let segments_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.realm.intrinsics.intl_segments_prototypes
            .insert(global, segments_prototype);
        self.install_builtin_to_string_tag(segments_prototype, "Intl.Segmenter Segments")?;
        let iterator =
            self.native_with_realm(Native::IntlSegmenterSegmentsIterator, global, global);
        self.set_builtin_function_name(iterator, "[Symbol.iterator]")?;
        let symbol_iterator = self.well_known_symbols["iterator"];
        self.set_symbol_property(segments_prototype, symbol_iterator, iterator)?;
        let containing =
            self.native_with_realm(Native::IntlSegmenterSegmentsContaining, global, global);
        self.set_builtin_function_name(containing, "containing")?;
        self.set_builtin_value_named(segments_prototype, "containing", containing)?;
        let supported =
            self.native_with_realm(Native::IntlSegmenterSupportedLocalesOf, global, global);
        self.set_builtin_function_name(supported, "supportedLocalesOf")?;
        self.set_builtin_value_named(constructor, "supportedLocalesOf", supported)?;
        self.set_builtin_value_named(intl, "Segmenter", constructor)
    }

    pub(super) fn intl_segmenter_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        self.with_call_roots(args.iter().copied().chain([new_target]), |vm| {
            let prototype = vm.intl_instance_prototype(p, new_target, Native::IntlSegmenter)?;
            vm.with_call_roots([prototype], |vm| {
                let locale = vm
                    .canonical_locale_list(p, args.first().copied())?
                    .into_iter()
                    .find(|locale| segmenter_locale_supported(locale))
                    .unwrap_or_else(|| "en-US".into());
                let granularity = vm.segmenter_granularity(p, args.get(1).copied())?;

                let segmenter = vm.heap.alloc(Cell::Object(Self::empty_object(prototype)));
                vm.set_hidden_string(segmenter, SEGMENTER_LOCALE_SLOT, &locale)?;
                vm.set_hidden_string(segmenter, SEGMENTER_GRANULARITY_SLOT, &granularity)?;
                Ok(segmenter)
            })
        })
    }

    fn segmenter_granularity(
        &mut self,
        p: &ResidualProgram,
        options: Option<Value>,
    ) -> Result<String, JsError> {
        let options = self.get_options_object(p, options)?;
        self.string_option(
            p,
            options,
            "localeMatcher",
            "best fit",
            &["lookup", "best fit"],
        )?;
        self.string_option(
            p,
            options,
            "granularity",
            "grapheme",
            &["grapheme", "word", "sentence"],
        )
    }

    pub(super) fn intl_segmenter_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::IntlSegmenterSupportedLocalesOf {
            if let Some(options) = args.get(1).copied().filter(|value| !value.is_undefined()) {
                if options.is_null() {
                    return Err(self.type_error(p, "options must not be null".into()));
                }
                let options = self.box_object(options)?;
                self.string_option(
                    p,
                    options,
                    "localeMatcher",
                    "best fit",
                    &["lookup", "best fit"],
                )?;
            }
            let locales = self
                .canonical_locale_list(p, args.first().copied())?
                .into_iter()
                .filter(|locale| segmenter_locale_supported(locale))
                .map(|locale| self.heap.alloc(Cell::String(locale.into())))
                .collect();
            return Ok(self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(locales),
            }));
        }
        if native == Native::IntlSegmenterSegmentsIterator {
            let array = self
                .hidden_value(this, SEGMENTS_DATA_SLOT)
                .ok_or_else(|| self.type_error(p, "incompatible Segments receiver".into()))?;
            return self.array_iterator_native(p, Native::ArrayValues, array);
        }
        if native == Native::IntlSegmenterSegmentsContaining {
            let array = self
                .hidden_value(this, SEGMENTS_DATA_SLOT)
                .ok_or_else(|| self.type_error(p, "incompatible Segments receiver".into()))?;
            let input = self
                .hidden_value(this, SEGMENTS_INPUT_SLOT)
                .ok_or_else(|| self.type_error(p, "incompatible Segments receiver".into()))?;
            let index = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
            let index = if index.is_nan() { 0.0 } else { index.trunc() };
            if !index.is_finite() || index < 0.0 {
                return Ok(Value::UNDEFINED);
            }
            let Some(Cell::String(text)) = self.heap.get(input) else {
                return Ok(Value::UNDEFINED);
            };
            let length = text.units().len();
            let index = index as usize;
            if index >= length {
                return Ok(Value::UNDEFINED);
            }
            let Some(Cell::Array { elements, .. }) = self.heap.get(array) else {
                return Ok(Value::UNDEFINED);
            };
            let entries = elements.clone();
            let atom = self.intern_atom("index");
            for entry in entries.iter().rev().copied() {
                let start = self
                    .get_property(p, entry, atom)?
                    .as_number()
                    .unwrap_or(0.0) as usize;
                if start <= index {
                    return Ok(entry);
                }
            }
            return Ok(Value::UNDEFINED);
        }
        let locale = self
            .hidden_string(this, SEGMENTER_LOCALE_SLOT)
            .ok_or_else(|| self.type_error(p, "incompatible Segmenter receiver".into()))?;
        let granularity = self
            .hidden_string(this, SEGMENTER_GRANULARITY_SLOT)
            .unwrap_or_else(|| "grapheme".into());
        if native == Native::IntlSegmenterResolvedOptions {
            let result = self.object();
            self.set_intl_string_property(result, "locale", &locale)?;
            self.set_intl_string_property(result, "granularity", &granularity)?;
            return Ok(result);
        }
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let primitive = self.to_primitive(p, value, "string")?;
        let input = self.to_string(p, primitive)?;
        let input_units = match self.heap.get(primitive) {
            Some(Cell::String(text)) => text.units().to_vec(),
            _ => input.encode_utf16().collect(),
        };
        let segments = segmenter_parts(&input, &granularity);
        let mut values = Vec::with_capacity(segments.len());
        for (segment, start_byte, index, word_like) in segments {
            let entry = self.object();
            let start = input[..start_byte].encode_utf16().count();
            let end = start + segment.encode_utf16().count();
            let raw_segment = self
                .heap
                .alloc(Cell::String(super::wtf16::JsString::from_units(
                    &input_units[start..end],
                )));
            self.set_named(p, entry, "segment", raw_segment)?;
            self.set_named(p, entry, "index", Value::number(index as f64))?;
            let raw_input = self
                .heap
                .alloc(Cell::String(super::wtf16::JsString::from_units(
                    &input_units,
                )));
            self.set_named(p, entry, "input", raw_input)?;
            if granularity == "word" {
                self.set_named(
                    p,
                    entry,
                    "isWordLike",
                    if word_like { Value::TRUE } else { Value::FALSE },
                )?;
            }
            values.push(entry);
        }
        let array = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        });
        let prototype = self.realm.intrinsics.intl_segments_prototypes
            .get(&self.realm.globals)
            .copied()
            .unwrap_or(self.object_proto);
        let object = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        self.set_hidden_value(object, SEGMENTS_DATA_SLOT, array)?;
        let input_value = self
            .heap
            .alloc(Cell::String(super::wtf16::JsString::from_units(
                &input_units,
            )));
        self.set_hidden_value(object, SEGMENTS_INPUT_SLOT, input_value)?;
        Ok(object)
    }
}

fn segmenter_locale_supported(locale: &str) -> bool {
    let language = locale.split('-').next().unwrap_or_default();
    language.len() == 2 && !language.eq_ignore_ascii_case("zz")
}

fn segmenter_parts(input: &str, granularity: &str) -> Vec<(String, usize, usize, bool)> {
    let boundaries = match granularity {
        "sentence" => sentence_boundaries(input),
        "word" => word_boundaries(input),
        _ => input
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .chain(std::iter::once(input.len()))
            .collect(),
    };
    let mut start_utf16 = 0;
    boundaries
        .windows(2)
        .map(|pair| {
            let start = pair[0];
            let end = pair[1];
            let segment = &input[start..end];
            let index = start_utf16;
            start_utf16 += segment.encode_utf16().count();
            let word_like = granularity == "word" && segment.chars().any(char::is_alphanumeric);
            (segment.to_string(), start, index, word_like)
        })
        .collect()
}

fn word_boundaries(input: &str) -> Vec<usize> {
    let mut result = vec![0];
    let mut kind = None;
    for (index, grapheme) in input.grapheme_indices(true) {
        let character = grapheme.chars().next().unwrap_or(' ');
        let next = if decimal_point(input, index, character) {
            1
        } else if character.is_whitespace() {
            0
        } else if character.is_alphanumeric() {
            1
        } else {
            2
        };
        if kind.is_some_and(|previous| previous != next || next == 2)
            && index > *result.last().unwrap_or(&0)
        {
            result.push(index);
        }
        kind = Some(next);
    }
    if *result.last().unwrap_or(&0) != input.len() {
        result.push(input.len());
    }
    result
}

fn decimal_point(input: &str, index: usize, character: char) -> bool {
    character == '.'
        && input[..index]
            .chars()
            .next_back()
            .is_some_and(|value| value.is_ascii_digit())
        && input[index + character.len_utf8()..]
            .chars()
            .next()
            .is_some_and(|value| value.is_ascii_digit())
}

fn sentence_boundaries(input: &str) -> Vec<usize> {
    let mut result = vec![0];
    for (index, character) in input.char_indices() {
        if matches!(character, '.' | '!' | '?') {
            let after = index + character.len_utf8();
            let end = input[after..]
                .char_indices()
                .find(|(_, ch)| !ch.is_whitespace())
                .map_or(input.len(), |(offset, _)| after + offset);
            if end > *result.last().unwrap_or(&0) && end < input.len() {
                result.push(end);
            }
        }
    }
    if *result.last().unwrap_or(&0) != input.len() {
        result.push(input.len());
    }
    result
}
