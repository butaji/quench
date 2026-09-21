use super::*;

fn utf16_index(text: &str, byte_index: usize) -> usize {
    text[..byte_index].encode_utf16().count()
}

impl<H: Host> Vm<H> {
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
                let units: Vec<u16> = receiver.encode_utf16().collect();
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
                let code_point = if (0xD800..=0xDBFF).contains(&first)
                    && units
                        .get(index + 1)
                        .is_some_and(|next| (0xDC00..=0xDFFF).contains(next))
                {
                    0x10000
                        + ((u32::from(first) - 0xD800) << 10)
                        + (u32::from(units[index + 1]) - 0xDC00)
                } else {
                    u32::from(first)
                };
                Ok(Value::number(code_point as f64))
            }
            Native::StringToUpperCase | Native::StringToLowerCase => {
                let text = if native == Native::StringToUpperCase {
                    receiver.to_uppercase()
                } else {
                    receiver.to_lowercase()
                };
                Ok(self.heap.alloc(Cell::String(text)))
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
                    "NFC" | "undefined" => receiver.nfc().collect(),
                    "NFD" => receiver.nfd().collect(),
                    "NFKC" => receiver.nfkc().collect(),
                    "NFKD" => receiver.nfkd().collect(),
                    _ => return Err(JsError("invalid normalization form".into())),
                };
                Ok(self.heap.alloc(Cell::String(text)))
            }
            _ => unreachable!(),
        }
    }

    pub(super) fn string_native_for_atom(&self, atom: Atom) -> Option<Native> {
        [
            ("replace", Native::StringReplace),
            ("split", Native::StringSplit),
            ("trim", Native::StringTrim),
            ("trimStart", Native::StringTrimStart),
            ("trimEnd", Native::StringTrimEnd),
            ("repeat", Native::StringRepeat),
            ("padStart", Native::StringPadStart),
            ("padEnd", Native::StringPadEnd),
            ("match", Native::StringMatch),
            ("search", Native::StringSearch),
            ("replaceAll", Native::StringReplaceAll),
            ("at", Native::StringAt),
            ("codePointAt", Native::StringCodePointAt),
            ("toUpperCase", Native::StringToUpperCase),
            ("toLowerCase", Native::StringToLowerCase),
            ("concat", Native::StringConcat),
            ("normalize", Native::StringNormalize),
        ]
        .into_iter()
        .find_map(|(name, native)| (self.lookup_atom(name) == Some(atom)).then_some(native))
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
        let pattern = args.first().copied().unwrap_or(Value::UNDEFINED);
        let (source, flags) = if self.is_regexp(pattern) {
            let source_atom = self.intern_atom("source");
            let flags_atom = self.intern_atom("flags");
            (
                self.to_string(p, self.get_property(p, pattern, source_atom)?)?,
                self.to_string(p, self.get_property(p, pattern, flags_atom)?)?,
            )
        } else {
            (regex::escape(&self.to_string(p, pattern)?), String::new())
        };
        let regex = Self::compile_regexp(&source, &flags)?;
        let Some(first) = regex.captures(&receiver) else {
            return Ok(if native == Native::StringSearch {
                Value::number(-1.0)
            } else {
                Value::NULL
            });
        };
        if native == Native::StringSearch {
            return Ok(Value::number(first.get(0).map_or(-1isize, |value| {
                utf16_index(&receiver, value.start()) as isize
            }) as f64));
        }
        if flags.contains('g') {
            let values = regex
                .captures_iter(&receiver)
                .filter_map(|captures| captures.get(0))
                .map(|value| self.heap.alloc(Cell::String(value.as_str().into())))
                .collect();
            return Ok(self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(values),
            }));
        }
        let values = first
            .iter()
            .map(|value| {
                value.map_or(Value::UNDEFINED, |value| {
                    self.heap.alloc(Cell::String(value.as_str().into()))
                })
            })
            .collect::<Vec<_>>();
        let result = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        });
        let index = self.intern_atom("index");
        let input = self.intern_atom("input");
        self.set_property(
            result,
            index,
            Value::number(
                first
                    .get(0)
                    .map_or(0, |value| utf16_index(&receiver, value.start()))
                    as f64,
            ),
        )?;
        let input_value = self.heap.alloc(Cell::String(receiver));
        self.set_property(result, input, input_value)?;
        Ok(result)
    }

    pub(super) fn string_split_regexp_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        separator: Value,
        limit: usize,
    ) -> Result<Value, JsError> {
        let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
            return Err(JsError("string method receiver is not a string".into()));
        };
        let source_atom = self.intern_atom("source");
        let flags_atom = self.intern_atom("flags");
        let source = self.to_string(p, self.get_property(p, separator, source_atom)?)?;
        let flags = self.to_string(p, self.get_property(p, separator, flags_atom)?)?;
        let regex = Self::compile_regexp(&source, &flags)?;
        let mut values = Vec::new();
        let mut cursor = 0;
        for captures in regex.captures_iter(&receiver) {
            let Some(whole) = captures.get(0) else {
                continue;
            };
            values.push(
                self.heap
                    .alloc(Cell::String(receiver[cursor..whole.start()].to_owned())),
            );
            for capture in captures.iter().skip(1) {
                values.push(capture.map_or(Value::UNDEFINED, |value| {
                    self.heap.alloc(Cell::String(value.as_str().into()))
                }));
            }
            cursor = whole.end();
            if values.len() >= limit {
                break;
            }
        }
        if values.len() < limit {
            values.push(self.heap.alloc(Cell::String(receiver[cursor..].to_owned())));
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
            let source_atom = self.intern_atom("source");
            let flags_atom = self.intern_atom("flags");
            let source = self.to_string(p, self.get_property(p, search_value, source_atom)?)?;
            let flags = self.to_string(p, self.get_property(p, search_value, flags_atom)?)?;
            let regex = Self::compile_regexp(&source, &flags)?;
            if replace_all && !flags.contains('g') {
                return Err(JsError("replaceAll requires a global RegExp".into()));
            }
            let global = flags.contains('g') || replace_all;
            let mut result = String::with_capacity(receiver.len());
            let mut cursor = 0;
            let mut replaced = false;
            for captures in regex.captures_iter(&receiver) {
                if replaced && !global {
                    break;
                }
                let Some(whole) = captures.get(0) else {
                    continue;
                };
                result.push_str(&receiver[cursor..whole.start()]);
                let replacement_text = if replacement_function {
                    let mut callback_args = Vec::with_capacity(captures.len() + 3);
                    callback_args.push(self.heap.alloc(Cell::String(whole.as_str().into())));
                    callback_args.extend(captures.iter().skip(1).map(|capture| {
                        capture.map_or(Value::UNDEFINED, |value| {
                            self.heap.alloc(Cell::String(value.as_str().into()))
                        })
                    }));
                    callback_args.push(Value::number(utf16_index(&receiver, whole.start()) as f64));
                    callback_args.push(self.heap.alloc(Cell::String(receiver.clone())));
                    let value =
                        self.call_value(p, replacement_value, Value::UNDEFINED, &callback_args)?;
                    self.to_string(p, value)?
                } else {
                    let captured = captures
                        .iter()
                        .skip(1)
                        .map(|capture| capture.map(|value| value.as_str()))
                        .collect::<Vec<_>>();
                    expand_replacement(
                        &replacement,
                        whole.as_str(),
                        &captured,
                        &receiver,
                        whole.start(),
                        whole.end(),
                    )
                };
                result.push_str(&replacement_text);
                cursor = whole.end();
                replaced = true;
            }
            if !replaced {
                return Ok(self.heap.alloc(Cell::String(receiver)));
            }
            result.push_str(&receiver[cursor..]);
            return Ok(self.heap.alloc(Cell::String(result)));
        }
        let search = self.to_string(p, search_value)?;
        if replace_all && search.is_empty() {
            let input = self.heap.alloc(Cell::String(receiver.clone()));
            let units: Vec<u16> = receiver.encode_utf16().collect();
            let mut output = Vec::new();
            for (offset, unit) in units.iter().copied().enumerate() {
                let text = if replacement_function {
                    let callback_args = [
                        self.heap.alloc(Cell::String(String::new())),
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
                    self.heap.alloc(Cell::String(String::new())),
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
        let Some(index) = receiver.find(&search) else {
            return Ok(self.heap.alloc(Cell::String(receiver)));
        };
        if replace_all && !search.is_empty() {
            let mut result = String::with_capacity(receiver.len());
            let mut cursor = 0;
            for (index, _) in receiver.match_indices(&search) {
                result.push_str(&receiver[cursor..index]);
                let replacement_text = if replacement_function {
                    let callback_args = [
                        self.heap.alloc(Cell::String(search.clone())),
                        Value::number(utf16_index(&receiver, index) as f64),
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
                        &receiver,
                        index,
                        index + search.len(),
                    )
                };
                result.push_str(&replacement_text);
                cursor = index + search.len();
            }
            result.push_str(&receiver[cursor..]);
            return Ok(self.heap.alloc(Cell::String(result)));
        }
        let replacement = if replacement_function {
            let callback_args = [
                self.heap.alloc(Cell::String(search.clone())),
                Value::number(utf16_index(&receiver, index) as f64),
                self.heap.alloc(Cell::String(receiver.clone())),
            ];
            let value = self.call_value(p, replacement_value, Value::UNDEFINED, &callback_args)?;
            self.to_string(p, value)?
        } else {
            expand_replacement(
                &replacement,
                &search,
                &[],
                &receiver,
                index,
                index + search.len(),
            )
        };
        let mut result =
            String::with_capacity(receiver.len() + replacement.len().saturating_sub(search.len()));
        result.push_str(&receiver[..index]);
        result.push_str(&replacement);
        result.push_str(&receiver[index + search.len()..]);
        Ok(self.heap.alloc(Cell::String(result)))
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
