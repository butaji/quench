use super::*;
use unicode_segmentation::UnicodeSegmentation;

const SEGMENTER_LOCALE_SLOT: &str = "\0quench:intl-segmenter-locale";
const SEGMENTER_GRANULARITY_SLOT: &str = "\0quench:intl-segmenter-granularity";
const SEGMENTS_DATA_SLOT: &str = "\0quench:intl-segments-data";

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
        self.realm
            .intrinsics
            .intl_segmenter_prototypes
            .insert(global, prototype);
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
        self.realm
            .intrinsics
            .intl_segments_prototypes
            .insert(global, segments_prototype);
        let iterator_prototype =
            self.realm.intrinsics.builtin_prototypes[&(global, Native::Iterator)];
        let iterator_prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(iterator_prototype)));
        self.realm
            .intrinsics
            .intl_segment_iterator_prototypes
            .insert(global, iterator_prototype);
        self.install_builtin_to_string_tag(iterator_prototype, "Segmenter String Iterator")?;
        let next = self.native_with_realm(Native::IntlSegmenterIteratorNext, global, global);
        self.set_builtin_function_name(next, "next")?;
        self.set_builtin_value_named(iterator_prototype, "next", next)?;
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
            return self.intl_supported_locales_of(p, args, segmenter_locale_supported);
        }
        if native == Native::IntlSegmenterIteratorNext {
            return self.intl_segment_iterator_next(p, this);
        }
        if matches!(
            native,
            Native::IntlSegmenterSegmentsIterator | Native::IntlSegmenterSegmentsContaining
        ) {
            let data = self
                .hidden_value(this, SEGMENTS_DATA_SLOT)
                .ok_or_else(|| self.type_error(p, "incompatible Segments receiver".into()))?;
            return self.with_call_roots(
                [this, data].into_iter().chain(args.iter().copied()),
                |vm| {
                    if native == Native::IntlSegmenterSegmentsIterator {
                        let prototype =
                            vm.realm.intrinsics.intl_segment_iterator_prototypes[&vm.realm.globals];
                        return Ok(vm.heap.alloc(Cell::Iterator {
                            object: Self::empty_object(prototype),
                            source: data,
                            next_method: Value::DELETED,
                            helper: None,
                            helper_running: false,
                            helper_started: false,
                            kind: IteratorKind::IntlSegments,
                            index: 0,
                            done: false,
                            generator: None,
                        }));
                    }
                    let index =
                        vm.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                    let index = if index.is_nan() { 0.0 } else { index.trunc() };
                    if !index.is_finite() || index < 0.0 {
                        return Ok(Value::UNDEFINED);
                    }
                    Ok(vm
                        .segment_data(p, data, index as usize)?
                        .map_or(Value::UNDEFINED, |(value, _)| value))
                },
            );
        }
        let locale = self
            .hidden_string(this, SEGMENTER_LOCALE_SLOT)
            .ok_or_else(|| self.type_error(p, "incompatible Segmenter receiver".into()))?;
        let granularity = self
            .hidden_string(this, SEGMENTER_GRANULARITY_SLOT)
            .unwrap_or_else(|| "grapheme".into());
        if native == Native::IntlSegmenterResolvedOptions {
            let result = self.heap.alloc(Cell::Object(Self::empty_object(
                self.realm_object_prototype(self.realm.globals),
            )));
            self.set_intl_string_property(result, "locale", &locale)?;
            self.set_intl_string_property(result, "granularity", &granularity)?;
            return Ok(result);
        }
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        self.with_call_roots([this, value], |vm| {
            let input = vm.coerce_js_string(p, value)?;
            let boundaries = segmenter_boundaries(input.host_string(), &granularity)
                .into_iter()
                .map(|index| Value::number(index as f64))
                .collect();
            let boundaries = vm.new_array(boundaries);
            let input = vm.heap.alloc(Cell::String(input));
            let prototype = vm.realm.intrinsics.intl_segments_prototypes[&vm.realm.globals];
            let object = vm.heap.alloc(Cell::Object(Self::empty_object(prototype)));
            let data = vm.new_array(vec![this, input, boundaries]);
            vm.set_hidden_value(object, SEGMENTS_DATA_SLOT, data)?;
            Ok(object)
        })
    }

    fn segment_data(
        &mut self,
        p: &ResidualProgram,
        data: Value,
        index: usize,
    ) -> Result<Option<(Value, usize)>, JsError> {
        // The private state tuple owns [segmenter, input, boundaries]; public
        // Segments and cursors share it without retaining one another.
        let Some(Cell::Array { elements, .. }) = self.heap.get(data) else {
            return Err(self.type_error(p, "incompatible Segments receiver".into()));
        };
        let [segmenter, input, boundaries] = elements.as_slice() else {
            return Err(self.type_error(p, "incompatible Segments receiver".into()));
        };
        let (segmenter, input, boundaries) = (*segmenter, *input, *boundaries);
        let Some(Cell::String(text)) = self.heap.get(input) else {
            return Ok(None);
        };
        if index >= text.units().len() {
            return Ok(None);
        }
        let Some(Cell::Array { elements, .. }) = self.heap.get(boundaries) else {
            return Ok(None);
        };
        let end = elements
            .partition_point(|boundary| boundary.as_number().unwrap_or(0.0) <= index as f64);
        let Some((start, end)) = end
            .checked_sub(1)
            .and_then(|start| elements.get(start).zip(elements.get(end)))
        else {
            return Ok(None);
        };
        let start = start.as_number().unwrap_or(0.0) as usize;
        let end = end.as_number().unwrap_or(0.0) as usize;
        let Some(units) = text.units().get(start..end) else {
            return Ok(None);
        };
        let text = JsString::from_units(units);
        let word = self
            .hidden_string(segmenter, SEGMENTER_GRANULARITY_SLOT)
            .as_deref()
            == Some("word");
        let word_like = text.host_string().chars().any(char::is_alphanumeric);
        let object = self.heap.alloc(Cell::Object(Self::empty_object(
            self.realm_object_prototype(self.realm.globals),
        )));
        let text = self.heap.alloc(Cell::String(text));
        self.set_named(p, object, "segment", text)?;
        self.set_named(p, object, "index", Value::number(start as f64))?;
        self.set_named(p, object, "input", input)?;
        if word {
            self.set_named(
                p,
                object,
                "isWordLike",
                if word_like { Value::TRUE } else { Value::FALSE },
            )?;
        }
        Ok(Some((object, end)))
    }

    pub(super) fn intl_segment_iterator_next(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
    ) -> Result<Value, JsError> {
        self.with_call_roots([iterator], |vm| {
            let (data, index, done) = match vm.heap.get(iterator) {
                Some(Cell::Iterator {
                    source,
                    index,
                    done,
                    kind: IteratorKind::IntlSegments,
                    ..
                }) => (*source, *index, *done),
                _ => return Err(vm.type_error(p, "incompatible Segment Iterator receiver".into())),
            };
            if done {
                return vm.iterator_result(Value::UNDEFINED, true);
            }
            match vm.segment_data(p, data, index)? {
                Some((value, end)) => {
                    if let Some(Cell::Iterator { index, .. }) = vm.heap.get_mut(iterator) {
                        *index = end;
                    }
                    vm.iterator_result(value, false)
                }
                None => {
                    if let Some(Cell::Iterator { done, .. }) = vm.heap.get_mut(iterator) {
                        *done = true;
                    }
                    vm.iterator_result(Value::UNDEFINED, true)
                }
            }
        })
    }
}

fn segmenter_locale_supported(locale: &str) -> bool {
    let language = locale.split('-').next().unwrap_or_default();
    language.len() == 2 && !language.eq_ignore_ascii_case("zz")
}

fn segmenter_boundaries(input: &str, granularity: &str) -> Vec<usize> {
    let boundaries = match granularity {
        "sentence" => sentence_boundaries(input),
        "word" => word_boundaries(input),
        _ => input
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .chain(std::iter::once(input.len()))
            .collect(),
    };
    let mut previous = 0;
    let mut units = 0;
    boundaries
        .into_iter()
        .map(|index| {
            units += input[previous..index].encode_utf16().count();
            previous = index;
            units
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WordBoundaryKind {
    Word,
    Whitespace,
    Punctuation,
}

fn word_boundaries(input: &str) -> Vec<usize> {
    let mut result = vec![0];
    let mut kind = None;
    for (index, grapheme) in input.grapheme_indices(true) {
        let character = grapheme.chars().next().unwrap_or(' ');
        let next = if decimal_point(input, index, character) || character.is_alphanumeric() {
            WordBoundaryKind::Word
        } else if character.is_whitespace() {
            WordBoundaryKind::Whitespace
        } else {
            WordBoundaryKind::Punctuation
        };
        if kind.is_some_and(|previous| previous != next || next == WordBoundaryKind::Punctuation)
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
