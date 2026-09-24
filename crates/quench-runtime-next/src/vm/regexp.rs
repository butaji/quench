use super::*;
use regex::RegexBuilder;

fn utf16_to_byte_index(text: &str, target: usize) -> usize {
    if target == 0 {
        return 0;
    }
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        if units >= target {
            return byte;
        }
        units += character.len_utf16();
        if units >= target {
            return byte + character.len_utf8();
        }
    }
    text.len()
}

fn utf16_index(text: &str, byte_index: usize) -> usize {
    text[..byte_index].encode_utf16().count()
}

impl<H: Host> Vm<H> {
    pub(super) fn is_regexp(&self, value: Value) -> bool {
        let mut current = value;
        for _ in 0..32 {
            if current == self.regexp_proto {
                return true;
            }
            let Some(Cell::Object(object)) = self.heap.get(current) else {
                return false;
            };
            if object.proto.is_null() {
                return false;
            }
            current = object.proto;
        }
        false
    }

    pub(super) fn install_regexp(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        for name in ["source", "flags", "lastIndex", "index", "input"] {
            self.intern_atom(name);
        }
        let constructor = self.native_value(Native::RegExp);
        self.regexp_proto = self.object();
        self.set_named(program, constructor, "prototype", self.regexp_proto)?;
        self.set_named(
            program,
            self.regexp_proto,
            "exec",
            self.native_value(Native::RegExpExec),
        )?;
        self.set_named(
            program,
            self.regexp_proto,
            "test",
            self.native_value(Native::RegExpTest),
        )?;
        self.global(program, "RegExp", constructor)
    }

    pub(super) fn construct_regexp_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let pattern = match args.first().copied() {
            None | Some(Value::UNDEFINED) => String::new(),
            Some(value) => self.to_string(p, value)?,
        };
        let flags = args
            .get(1)
            .copied()
            .filter(|value| !value.is_undefined())
            .map(|value| self.to_string(p, value))
            .transpose()?
            .unwrap_or_default();
        let regex = Self::compile_regexp(&pattern, &flags)?;
        drop(regex);
        let object = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.regexp_proto)));
        let source_atom = self.intern_atom("source");
        let flags_atom = self.intern_atom("flags");
        let last_index_atom = self.intern_atom("lastIndex");
        let source_value = self.heap.alloc(Cell::String(pattern.into()));
        let flags_value = self.heap.alloc(Cell::String(flags.into()));
        self.set_property(object, source_atom, source_value)?;
        self.set_property(object, flags_atom, flags_value)?;
        self.set_property(object, last_index_atom, Value::number(0.0))?;
        Ok(object)
    }

    pub(super) fn regexp_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let source_atom = self.intern_atom("source");
        let flags_atom = self.intern_atom("flags");
        let source_value = self.get_property(p, this, source_atom)?;
        let flags_value = self.get_property(p, this, flags_atom)?;
        let source = self.to_string(p, source_value)?;
        let flags = self.to_string(p, flags_value)?;
        let regex = Self::compile_regexp(&source, &flags)?;
        let input = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let stateful = flags.contains('g') || flags.contains('y');
        let sticky = flags.contains('y');
        let last_index_atom = self.intern_atom("lastIndex");
        let start = if stateful {
            let value = self.get_property(p, this, last_index_atom)?;
            let number = self.to_number(p, value)?;
            if number.is_finite() && number > 0.0 {
                utf16_to_byte_index(&input, number.floor() as usize)
            } else {
                0
            }
        } else {
            0
        };
        let captures = regex.captures_at(&input, start);
        let matched = captures
            .as_ref()
            .and_then(|captures| captures.get(0))
            .is_some_and(|matched| !sticky || matched.start() == start);
        let Some(captures) = captures.filter(|_| matched) else {
            if stateful {
                self.set_property(this, last_index_atom, Value::number(0.0))?;
            }
            return Ok(if native == Native::RegExpTest {
                Value::FALSE
            } else {
                Value::NULL
            });
        };
        if native == Native::RegExpTest {
            return Ok(Value::TRUE);
        }
        let values = captures
            .iter()
            .map(|capture| {
                capture
                    .map(|value| self.heap.alloc(Cell::String(value.as_str().into())))
                    .unwrap_or(Value::UNDEFINED)
            })
            .collect::<Vec<_>>();
        let result = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        });
        let index = captures
            .get(0)
            .map_or(0, |value| utf16_index(&input, value.start()));
        if stateful {
            let end = captures.get(0).map_or(utf16_index(&input, start), |value| {
                utf16_index(&input, value.end())
            });
            self.set_property(this, last_index_atom, Value::number(end as f64))?;
        }
        let index_atom = self.intern_atom("index");
        self.set_property(result, index_atom, Value::number(index as f64))?;
        let input_value = self.heap.alloc(Cell::String(input.into()));
        let input_atom = self.intern_atom("input");
        self.set_property(result, input_atom, input_value)?;
        Ok(result)
    }

    pub(super) fn compile_regexp(source: &str, flags: &str) -> Result<regex::Regex, JsError> {
        let normalized = normalize_js_pattern(source);
        let mut builder = RegexBuilder::new(&normalized);
        let mut seen = 0u8;
        for flag in flags.chars() {
            let bit = match flag {
                'g' => 1,
                'i' => {
                    builder.case_insensitive(true);
                    2
                }
                'm' => {
                    builder.multi_line(true);
                    4
                }
                's' => {
                    builder.dot_matches_new_line(true);
                    8
                }
                'u' | 'y' | 'd' | 'v' => 16,
                _ => return Err(JsError("invalid regular expression flag".into())),
            };
            if bit != 16 && seen & bit != 0 {
                return Err(JsError("duplicate regular expression flag".into()));
            }
            seen |= bit;
        }
        builder
            .build()
            .map_err(|error| JsError(format!("invalid regular expression: {error}").into()))
    }
}

fn normalize_js_pattern(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut output = String::with_capacity(source.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '\\'
            && index + 3 < chars.len()
            && chars[index + 1] == 'x'
            && chars[index + 2].is_ascii_hexdigit()
            && chars[index + 3].is_ascii_hexdigit()
        {
            output.push_str("\\u{");
            output.push(chars[index + 2]);
            output.push(chars[index + 3]);
            output.push('}');
            index += 4;
        } else if chars[index] == '\\'
            && index + 5 < chars.len()
            && chars[index + 1] == 'u'
            && chars[index + 2..index + 6]
                .iter()
                .all(char::is_ascii_hexdigit)
        {
            let value = chars[index + 2..index + 6].iter().collect::<String>();
            let scalar = u32::from_str_radix(&value, 16).unwrap_or(0);
            if crate::unicode::is_surrogate(scalar) {
                output.push_str("\\u{FFFD}");
            } else {
                output.push_str("\\u{");
                output.push_str(&value);
                output.push('}');
            }
            index += 6;
        } else {
            output.push(chars[index]);
            index += 1;
        }
    }
    output
}
