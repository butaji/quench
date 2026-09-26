use super::*;

const STRING_METHODS: &[(&str, Native)] = &[
    ("charAt", Native::StringCharAt),
    ("charCodeAt", Native::StringCharCodeAt),
    ("codePointAt", Native::StringCodePointAt),
    ("concat", Native::StringConcat),
    ("endsWith", Native::StringEndsWith),
    ("includes", Native::StringIncludes),
    ("indexOf", Native::StringIndexOf),
    ("lastIndexOf", Native::StringLastIndexOf),
    ("match", Native::StringMatch),
    ("normalize", Native::StringNormalize),
    ("padEnd", Native::StringPadEnd),
    ("padStart", Native::StringPadStart),
    ("repeat", Native::StringRepeat),
    ("replace", Native::StringReplace),
    ("replaceAll", Native::StringReplaceAll),
    ("search", Native::StringSearch),
    ("slice", Native::StringSlice),
    ("split", Native::StringSplit),
    ("startsWith", Native::StringStartsWith),
    ("substr", Native::StringSubstr),
    ("substring", Native::StringSubstring),
    ("toLowerCase", Native::StringToLowerCase),
    ("toString", Native::StringToString),
    ("toUpperCase", Native::StringToUpperCase),
    ("trim", Native::StringTrim),
    ("trimEnd", Native::StringTrimEnd),
    ("trimStart", Native::StringTrimStart),
    ("valueOf", Native::StringValueOf),
    ("at", Native::StringAt),
];

fn utf16_index(text: &str, byte_index: usize) -> usize {
    text[..byte_index].encode_utf16().count()
}

pub(super) fn rfind_utf16(text: &[u16], search: &[u16], position: usize) -> Option<usize> {
    if search.is_empty() {
        return Some(position.min(text.len()));
    }
    (0..=position.min(text.len().saturating_sub(search.len())))
        .rev()
        .find(|index| text[*index..*index + search.len()] == *search)
}

impl<H: Host> Vm<H> {
    pub(super) fn string_method_receiver(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
    ) -> Result<Value, JsError> {
        let converts_receiver = STRING_METHODS.iter().any(|(_, method)| {
            *method == native && !matches!(native, Native::StringToString | Native::StringValueOf)
        });
        if !converts_receiver {
            return Ok(receiver);
        }
        self.require_object_coercible(p, receiver)?;
        if matches!(self.heap.get(receiver), Some(Cell::String(_))) {
            return Ok(receiver);
        }
        let text = self.to_string(p, receiver)?;
        Ok(self.heap.alloc(Cell::String(text.into())))
    }

    pub(super) fn install_string(
        &mut self,
        program: &ResidualProgram,
        constructor: Value,
    ) -> Result<(), JsError> {
        let empty = self.heap.alloc(Cell::String(JsString::from_str("")));
        self.string_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)));
        let value_atom = self.intern_atom("\0rqj:string-value");
        self.set_property(self.string_proto, value_atom, empty)?;
        self.set_named_constant(program, self.string_proto, "length", Value::number(0.0))?;
        self.set_builtin_named(program, self.string_proto, "constructor", Native::String)?;
        self.set_named(program, constructor, "prototype", self.string_proto)?;
        for (name, native) in STRING_METHODS {
            self.set_builtin_named(program, self.string_proto, name, *native)?;
        }
        Ok(())
    }

    pub(super) fn string_basic_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
            return Err(JsError("string method receiver is not a string".into()));
        };
        match native {
            Native::StringAt | Native::StringCodePointAt => {
                let units = receiver.units().to_vec();
                let raw = self
                    .to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?
                    .trunc() as i64;
                let index = if native == Native::StringAt && raw < 0 {
                    units.len() as i64 + raw
                } else {
                    raw
                };
                let Some(index) = usize::try_from(index)
                    .ok()
                    .filter(|index| *index < units.len())
                else {
                    return Ok(Value::UNDEFINED);
                };
                if native == Native::StringAt {
                    return self.string_from_units(&units[index..index + 1]);
                }
                let first = units[index];
                let code_point = units
                    .get(index + 1)
                    .and_then(|low| crate::unicode::decode_surrogate_pair(first, *low))
                    .unwrap_or_else(|| u32::from(first));
                Ok(Value::number(code_point as f64))
            }
            Native::StringToUpperCase | Native::StringToLowerCase => {
                let text = if native == Native::StringToUpperCase {
                    receiver.host_string().to_uppercase()
                } else {
                    receiver.host_string().to_lowercase()
                };
                Ok(self.heap.alloc(Cell::String(text.into())))
            }
            Native::StringConcat => {
                let mut text = receiver;
                for value in args {
                    text.push_str(&self.to_string(p, *value)?);
                }
                Ok(self.heap.alloc(Cell::String(text)))
            }
            Native::StringNormalize => {
                use unicode_normalization::UnicodeNormalization;
                let form = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let text = match form.as_str() {
                    "NFC" | "undefined" => receiver.host_string().nfc().collect(),
                    "NFD" => receiver.host_string().nfd().collect(),
                    "NFKC" => receiver.host_string().nfkc().collect(),
                    "NFKD" => receiver.host_string().nfkd().collect(),
                    _ => return Err(JsError("invalid normalization form".into())),
                };
                Ok(self.heap.alloc(Cell::String(text)))
            }
            _ => unreachable!(),
        }
    }

    pub(super) fn string_iterator_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        self.require_object_coercible(p, this)?;
        let source = match self.heap.get(this) {
            Some(Cell::String(_)) => this,
            _ => {
                let text = self.to_string(p, this)?;
                self.heap.alloc(Cell::String(text.into()))
            }
        };
        Ok(self.heap.alloc(Cell::Iterator {
            object: Self::empty_object(self.iterator_proto),
            source,
            next_method: None,
            helper: None,
            helper_running: false,
            kind: IteratorKind::String,
            index: 0,
            done: false,
            generator: None,
        }))
    }

    pub(super) fn string_match_or_search_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
            return Err(JsError("string method receiver is not a string".into()));
        };
        let receiver_host = receiver.host_string();
        let pattern = args.first().copied().unwrap_or(Value::UNDEFINED);
        let (source, flags) = if self.is_regexp(pattern) {
            self.regexp_source_and_flags(pattern)
                .ok_or_else(|| JsError("RegExp method called on incompatible receiver".into()))?
        } else {
            (regex::escape(&self.to_string(p, pattern)?), String::new())
        };
        let regex = Self::compile_regexp(&source, &flags)?;
        let Some(first) = regex.find_from(receiver_host, 0) else {
            return Ok(if native == Native::StringSearch {
                Value::number(-1.0)
            } else {
                Value::NULL
            });
        };
        if native == Native::StringSearch {
            return Ok(Value::number(
                utf16_index(receiver_host, first.range.start) as f64
            ));
        }
        let first_index = utf16_index(receiver_host, first.range.start);
        if flags.contains('g') {
            let values = regex
                .find_iter(receiver_host)
                .map(|matched| {
                    self.heap
                        .alloc(Cell::String(receiver_host[matched.range].to_owned().into()))
                })
                .collect();
            return Ok(self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(values),
            }));
        }
        let values = std::iter::once(Some(first.range.clone()))
            .chain(first.captures)
            .map(|range| {
                range.map_or(Value::UNDEFINED, |range| {
                    self.heap
                        .alloc(Cell::String(receiver_host[range].to_owned().into()))
                })
            })
            .collect::<Vec<_>>();
        let result = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        });
        let index = self.intern_atom("index");
        let input = self.intern_atom("input");
        self.set_property(result, index, Value::number(first_index as f64))?;
        let input_value = self.heap.alloc(Cell::String(receiver));
        self.set_property(result, input, input_value)?;
        Ok(result)
    }

    pub(super) fn string_split_regexp_native(
        &mut self,
        _p: &ResidualProgram,
        this: Value,
        separator: Value,
        limit: usize,
    ) -> Result<Value, JsError> {
        let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
            return Err(JsError("string method receiver is not a string".into()));
        };
        let receiver_host = receiver.host_string();
        let (source, flags) = self
            .regexp_source_and_flags(separator)
            .ok_or_else(|| JsError("RegExp method called on incompatible receiver".into()))?;
        let regex = Self::compile_regexp(&source, &flags)?;
        let mut values = Vec::new();
        let mut cursor = 0;
        for matched in regex.find_iter(receiver_host) {
            let whole = matched.range;
            values.push(self.heap.alloc(Cell::String(
                receiver_host[cursor..whole.start].to_owned().into(),
            )));
            for capture in matched.captures {
                values.push(capture.map_or(Value::UNDEFINED, |range| {
                    self.heap
                        .alloc(Cell::String(receiver_host[range].to_owned().into()))
                }));
            }
            cursor = whole.end;
            if values.len() >= limit {
                break;
            }
        }
        if values.len() < limit {
            values.push(
                self.heap
                    .alloc(Cell::String(receiver_host[cursor..].to_owned().into())),
            );
        }
        values.truncate(limit);
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }

    pub(super) fn string_replace_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
        replace_all: bool,
    ) -> Result<Value, JsError> {
        let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
            return Err(JsError("string method receiver is not a string".into()));
        };
        let receiver_host = receiver.host_string();
        let replacement_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let replacement_function = matches!(
            self.heap.get(replacement_value),
            Some(Cell::Function { .. })
        );
        let replacement = if replacement_function {
            String::new()
        } else {
            self.to_string(p, replacement_value)?
        };
        let search_value = args.first().copied().unwrap_or(Value::UNDEFINED);
        if self.is_regexp(search_value) {
            let (source, flags) = self
                .regexp_source_and_flags(search_value)
                .ok_or_else(|| JsError("RegExp method called on incompatible receiver".into()))?;
            let regex = Self::compile_regexp(&source, &flags)?;
            if replace_all && !flags.contains('g') {
                return Err(JsError("replaceAll requires a global RegExp".into()));
            }
            let global = flags.contains('g') || replace_all;
            let mut result = String::with_capacity(receiver_host.len());
            let mut cursor = 0;
            let mut replaced = false;
            for captures in regex.find_iter(receiver_host) {
                if replaced && !global {
                    break;
                }
                let whole = captures.range;
                result.push_str(&receiver_host[cursor..whole.start]);
                let replacement_text = if replacement_function {
                    let mut callback_args = Vec::with_capacity(captures.captures.len() + 3);
                    callback_args.push(
                        self.heap
                            .alloc(Cell::String(receiver_host[whole.clone()].to_owned().into())),
                    );
                    callback_args.extend(captures.captures.iter().map(|capture| {
                        capture.as_ref().map_or(Value::UNDEFINED, |range| {
                            self.heap
                                .alloc(Cell::String(receiver_host[range.clone()].to_owned().into()))
                        })
                    }));
                    callback_args
                        .push(Value::number(utf16_index(receiver_host, whole.start) as f64));
                    callback_args.push(self.heap.alloc(Cell::String(receiver.clone())));
                    let value =
                        self.call_value(p, replacement_value, Value::UNDEFINED, &callback_args)?;
                    self.to_string(p, value)?
                } else {
                    let captured = captures
                        .captures
                        .iter()
                        .map(|capture| capture.as_ref().map(|range| &receiver_host[range.clone()]))
                        .collect::<Vec<_>>();
                    expand_replacement(
                        &replacement,
                        &receiver_host[whole.clone()],
                        &captured,
                        receiver_host,
                        whole.start,
                        whole.end,
                    )
                };
                result.push_str(&replacement_text);
                cursor = whole.end;
                replaced = true;
            }
            if !replaced {
                return Ok(self.heap.alloc(Cell::String(receiver)));
            }
            result.push_str(&receiver_host[cursor..]);
            return Ok(self.heap.alloc(Cell::String(result.into())));
        }
        let search = self.to_string(p, search_value)?;
        if replace_all && search.is_empty() {
            let input = self.heap.alloc(Cell::String(receiver.clone()));
            let units = receiver.units().to_vec();
            let mut output = Vec::new();
            for (offset, unit) in units.iter().copied().enumerate() {
                let text = if replacement_function {
                    let callback_args = [
                        self.heap.alloc(Cell::String(String::new().into())),
                        Value::number(offset as f64),
                        input,
                    ];
                    let value =
                        self.call_value(p, replacement_value, Value::UNDEFINED, &callback_args)?;
                    self.to_string(p, value)?
                } else {
                    replacement.clone()
                };
                output.extend(text.encode_utf16());
                output.push(unit);
            }
            let text = if replacement_function {
                let callback_args = [
                    self.heap.alloc(Cell::String(String::new().into())),
                    Value::number(units.len() as f64),
                    input,
                ];
                let value =
                    self.call_value(p, replacement_value, Value::UNDEFINED, &callback_args)?;
                self.to_string(p, value)?
            } else {
                replacement
            };
            output.extend(text.encode_utf16());
            return self.string_from_units(&output);
        }
        let Some(index) = receiver_host.find(&search) else {
            return Ok(self.heap.alloc(Cell::String(receiver)));
        };
        if replace_all && !search.is_empty() {
            let mut result = String::with_capacity(receiver_host.len());
            let mut cursor = 0;
            for (index, _) in receiver_host.match_indices(&search) {
                result.push_str(&receiver_host[cursor..index]);
                let replacement_text = if replacement_function {
                    let callback_args = [
                        self.heap.alloc(Cell::String(search.clone().into())),
                        Value::number(utf16_index(receiver_host, index) as f64),
                        self.heap.alloc(Cell::String(receiver.clone())),
                    ];
                    let value =
                        self.call_value(p, replacement_value, Value::UNDEFINED, &callback_args)?;
                    self.to_string(p, value)?
                } else {
                    expand_replacement(
                        &replacement,
                        &search,
                        &[],
                        receiver_host,
                        index,
                        index + search.len(),
                    )
                };
                result.push_str(&replacement_text);
                cursor = index + search.len();
            }
            result.push_str(&receiver_host[cursor..]);
            return Ok(self.heap.alloc(Cell::String(result.into())));
        }
        let replacement = if replacement_function {
            let callback_args = [
                self.heap.alloc(Cell::String(search.clone().into())),
                Value::number(utf16_index(receiver_host, index) as f64),
                self.heap.alloc(Cell::String(receiver.clone())),
            ];
            let value = self.call_value(p, replacement_value, Value::UNDEFINED, &callback_args)?;
            self.to_string(p, value)?
        } else {
            expand_replacement(
                &replacement,
                &search,
                &[],
                receiver_host,
                index,
                index + search.len(),
            )
        };
        let mut result = String::with_capacity(
            receiver_host.len() + replacement.len().saturating_sub(search.len()),
        );
        result.push_str(&receiver_host[..index]);
        result.push_str(&replacement);
        result.push_str(&receiver_host[index + search.len()..]);
        Ok(self.heap.alloc(Cell::String(result.into())))
    }
}

fn expand_replacement(
    template: &str,
    whole: &str,
    captures: &[Option<&str>],
    input: &str,
    start: usize,
    end: usize,
) -> String {
    let chars = template.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(template.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '$' || index + 1 >= chars.len() {
            output.push(chars[index]);
            index += 1;
            continue;
        }
        let next = chars[index + 1];
        match next {
            '$' => {
                output.push('$');
                index += 2;
            }
            '&' => {
                output.push_str(whole);
                index += 2;
            }
            '`' => {
                output.push_str(&input[..start]);
                index += 2;
            }
            '\'' => {
                output.push_str(&input[end..]);
                index += 2;
            }
            '0'..='9' if next != '0' => {
                let first = next.to_digit(10).unwrap() as usize;
                let mut consumed = 1;
                let mut capture_index = first;
                if index + 2 < chars.len()
                    && chars[index + 2].is_ascii_digit()
                    && first * 10 + chars[index + 2].to_digit(10).unwrap() as usize
                        <= captures.len()
                {
                    capture_index = first * 10 + chars[index + 2].to_digit(10).unwrap() as usize;
                    consumed = 2;
                }
                if capture_index <= captures.len() {
                    output.push_str(captures[capture_index - 1].unwrap_or(""));
                    index += consumed + 1;
                } else {
                    output.push('$');
                    index += 1;
                }
            }
            _ => {
                output.push('$');
                index += 1;
            }
        }
    }
    output
}
