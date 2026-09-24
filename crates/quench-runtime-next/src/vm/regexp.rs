use super::*;
use regex::RegexBuilder;

const REGEXP_HEX_ESCAPE_DIGITS: usize = 2;
const REGEXP_UNICODE_ESCAPE_DIGITS: usize = 4;

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
            if matches!(self.heap.get(current), Some(Cell::RegExp { .. })) {
                return true;
            }
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
        for (name, native) in REGEXP_FLAG_ACCESSORS {
            let getter = self.native_value(*native);
            let atom = self.intern_atom(name);
            self.set_named(program, self.regexp_proto, name, getter)?;
            self.set_property_attributes(
                self.regexp_proto,
                PropertyKey::string(atom),
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
        for (name, native) in [
            ("source", Native::RegExpSource),
            ("flags", Native::RegExpFlags),
        ] {
            let getter = self.native_value(native);
            let atom = self.intern_atom(name);
            self.set_named(program, self.regexp_proto, name, getter)?;
            self.set_property_attributes(
                self.regexp_proto,
                PropertyKey::string(atom),
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
        self.global(program, "RegExp", constructor)
    }

    pub(super) fn regexp_slot_native(
        &mut self,
        _p: &ResidualProgram,
        native: Native,
        this: Value,
    ) -> Result<Value, JsError> {
        match (native, self.heap.get(this)) {
            (Native::RegExpSource, Some(Cell::RegExp { source, .. })) => {
                Ok(self.heap.alloc(Cell::String(source.clone())))
            }
            (Native::RegExpFlags, Some(Cell::RegExp { flags, .. })) => {
                Ok(self.heap.alloc(Cell::String(flags.clone().into())))
            }
            (Native::RegExpSource, _) if this == self.regexp_proto => {
                Ok(self.heap.alloc(Cell::String("(?:)".into())))
            }
            (Native::RegExpFlags, _) if this == self.regexp_proto => {
                Ok(self.heap.alloc(Cell::String(String::new().into())))
            }
            _ => Err(JsError(
                "RegExp accessor called on incompatible receiver".into(),
            )),
        }
    }

    fn regexp_source_string(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<JsString, JsError> {
        let primitive = self.to_primitive(p, value, "string")?;
        if let Some(Cell::String(source)) = self.heap.get(primitive) {
            return Ok(source.clone());
        }
        self.to_string(p, primitive)
            .map(|source| JsString::from_str(&source))
    }

    pub(super) fn regexp_flag_native(
        &mut self,
        _p: &ResidualProgram,
        native: Native,
        this: Value,
    ) -> Result<Value, JsError> {
        let flags = match self.heap.get(this) {
            Some(Cell::RegExp { flags, .. }) => flags.clone(),
            _ => {
                return Err(JsError(
                    "RegExp accessor called on incompatible receiver".into(),
                ));
            }
        };
        let contains = match native {
            Native::RegExpGlobal => flags.contains('g'),
            Native::RegExpIgnoreCase => flags.contains('i'),
            Native::RegExpMultiline => flags.contains('m'),
            Native::RegExpDotAll => flags.contains('s'),
            Native::RegExpUnicode => flags.contains('u') || flags.contains('v'),
            Native::RegExpUnicodeSets => flags.contains('v'),
            Native::RegExpSticky => flags.contains('y'),
            Native::RegExpHasIndices => flags.contains('d'),
            _ => return Err(JsError("invalid RegExp flag accessor".into())),
        };
        Ok(if contains { Value::TRUE } else { Value::FALSE })
    }

    pub(super) fn construct_regexp_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let pattern = match args.first().copied() {
            None | Some(Value::UNDEFINED) => JsString::from_str(""),
            Some(value) => self.regexp_source_string(p, value)?,
        };
        let flags = args
            .get(1)
            .copied()
            .filter(|value| !value.is_undefined())
            .map(|value| self.to_string(p, value))
            .transpose()?
            .unwrap_or_default();
        let regex = Self::compile_regexp(pattern.host_string(), &flags)?;
        drop(regex);
        let object = self.heap.alloc(Cell::RegExp {
            object: Self::empty_object(self.regexp_proto),
            source: pattern,
            flags,
        });
        let last_index_atom = self.intern_atom("lastIndex");
        self.set_property(object, last_index_atom, Value::number(0.0))?;
        self.set_property_attributes(
            object,
            PropertyKey::string(last_index_atom),
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(object)
    }

    pub(super) fn regexp_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (source, flags) = match self.heap.get(this) {
            Some(Cell::RegExp { source, flags, .. }) => {
                (source.host_string().to_owned(), flags.clone())
            }
            _ => {
                return Err(JsError(
                    "RegExp method called on incompatible receiver".into(),
                ));
            }
        };
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
        let normalized = normalize_nonunicode_case_fold(
            &normalize_js_pattern(source, flags),
            flags.contains('i') && !flags.contains('u') && !flags.contains('v'),
        );
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

    pub(super) fn regexp_source_and_flags(&self, value: Value) -> Option<(String, String)> {
        match self.heap.get(value) {
            Some(Cell::RegExp { source, flags, .. }) => {
                Some((source.host_string().to_owned(), flags.clone()))
            }
            _ => None,
        }
    }
}

fn normalize_nonunicode_case_fold(pattern: &str, enabled: bool) -> String {
    if !enabled {
        return pattern.to_owned();
    }
    let mut output = String::with_capacity(pattern.len());
    let mut in_class = false;
    for character in pattern.chars() {
        if character == '[' {
            in_class = true;
        } else if character == ']' {
            in_class = false;
        }
        let uppercase = character.to_uppercase().collect::<String>();
        let lowercase = character.to_lowercase().collect::<String>();
        let changes_to_ascii = [uppercase, lowercase]
            .iter()
            .any(|case| case.len() == 1 && case.as_bytes()[0].is_ascii());
        if !in_class && !character.is_ascii() && changes_to_ascii {
            output.push_str("(?-i:");
            output.push(character);
            output.push(')');
        } else {
            output.push(character);
        }
    }
    output
}

const REGEXP_FLAG_ACCESSORS: &[(&str, Native)] = &[
    ("global", Native::RegExpGlobal),
    ("ignoreCase", Native::RegExpIgnoreCase),
    ("multiline", Native::RegExpMultiline),
    ("dotAll", Native::RegExpDotAll),
    ("unicode", Native::RegExpUnicode),
    ("unicodeSets", Native::RegExpUnicodeSets),
    ("sticky", Native::RegExpSticky),
    ("hasIndices", Native::RegExpHasIndices),
];

fn normalize_js_pattern(source: &str, flags: &str) -> String {
    let unicode = flags.contains('u') || flags.contains('v');
    let chars: Vec<char> = source.chars().collect();
    let mut output = String::with_capacity(source.len());
    let mut index = 0;
    while index < chars.len() {
        if unicode && let Some((scalar, end)) = decode_pattern_surrogate_pair(&chars, index) {
            output.push(scalar);
            index = end;
        } else if chars[index] == '\\'
            && index + 1 < chars.len()
            && chars[index + 1] == '0'
            && !chars.get(index + 2).is_some_and(char::is_ascii_digit)
        {
            output.push_str("\\x00");
            index += 2;
        } else if chars[index] == '\\'
            && index + 3 < chars.len()
            && chars[index + 1] == 'x'
            && chars[index + 2].is_ascii_hexdigit()
            && chars[index + 3].is_ascii_hexdigit()
        {
            let hex = chars[index + 2..index + 4].iter().collect::<String>();
            let scalar = u32::from_str_radix(&hex, 16).unwrap_or_default();
            output.push(char::from_u32(scalar).unwrap_or(char::REPLACEMENT_CHARACTER));
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
                output.push(char::REPLACEMENT_CHARACTER);
            } else {
                output.push(char::from_u32(scalar).unwrap_or(char::REPLACEMENT_CHARACTER));
            }
            index += 6;
        } else {
            output.push(chars[index]);
            index += 1;
        }
    }
    normalize_legacy_identity_escapes(&output, unicode)
}

fn decode_pattern_surrogate_pair(chars: &[char], start: usize) -> Option<(char, usize)> {
    let (high, after_high) = read_pattern_unicode_unit(chars, start)?;
    let (low, after_low) = read_pattern_unicode_unit(chars, after_high)?;
    let scalar = crate::unicode::decode_surrogate_pair(high, low)?;
    Some((char::from_u32(scalar)?, after_low))
}

fn read_pattern_unicode_unit(chars: &[char], start: usize) -> Option<(u16, usize)> {
    if chars.get(start..start + 2)? != ['\\', 'u'] {
        return None;
    }
    let digits = chars.get(start + 2..start + 6)?;
    if !digits.iter().all(char::is_ascii_hexdigit) {
        return None;
    }
    let value = digits.iter().collect::<String>();
    Some((u16::from_str_radix(&value, 16).ok()?, start + 6))
}

fn normalize_legacy_identity_escapes(source: &str, unicode: bool) -> String {
    if unicode {
        return source.to_owned();
    }
    let chars: Vec<char> = source.chars().collect();
    let mut output = String::with_capacity(source.len());
    let mut in_class = false;
    let mut index = 0;
    let has_named_group = source.contains("(?<");
    while index < chars.len() {
        if chars[index] == '[' {
            in_class = true;
            output.push('[');
            index += 1;
            continue;
        }
        if chars[index] == ']' {
            in_class = false;
            output.push(']');
            index += 1;
            continue;
        }
        if chars[index] != '\\' || index + 1 == chars.len() {
            output.push(chars[index]);
            index += 1;
            continue;
        }
        let escaped = chars[index + 1];
        if ('1'..='7').contains(&escaped) {
            let (value, end) = legacy_octal_value(&chars, index + 1);
            output.push_str(&format!("\\x{value:02x}"));
            index = end;
            continue;
        }
        if is_valid_regexp_escape(&chars, index, escaped, has_named_group)
            || in_class && escaped == '-'
        {
            output.push('\\');
            output.push(escaped);
        } else {
            output.push(escaped);
        }
        index += 2;
    }
    output
}

fn is_valid_regexp_escape(
    chars: &[char],
    index: usize,
    escaped: char,
    has_named_group: bool,
) -> bool {
    let next = chars.get(index + 2).copied();
    let hex_digits = |count: usize| {
        chars
            .get(index + 2..index + 2 + count)
            .is_some_and(|digits| digits.iter().all(char::is_ascii_hexdigit))
    };
    match escaped {
        'x' => hex_digits(REGEXP_HEX_ESCAPE_DIGITS),
        'u' => hex_digits(REGEXP_UNICODE_ESCAPE_DIGITS),
        'c' => next.is_some_and(|value| value.is_ascii_alphabetic()),
        'k' => has_named_group && next == Some('<'),
        'b' | 'B' | 'd' | 'D' | 'f' | 'n' | 'r' | 's' | 'S' | 't' | 'v' | 'w' | 'W' => true,
        '^' | '$' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\' => true,
        _ => false,
    }
}

fn legacy_octal_value(chars: &[char], start: usize) -> (u8, usize) {
    let max_digits = if chars[start] <= '3' { 3 } else { 2 };
    let mut end = start;
    let mut value = 0_u8;
    while end < chars.len() && end - start < max_digits && ('0'..='7').contains(&chars[end]) {
        value = value.wrapping_mul(8).wrapping_add(chars[end] as u8 - b'0');
        end += 1;
    }
    (value, end)
}
