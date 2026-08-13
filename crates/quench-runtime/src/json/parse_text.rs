// JSON text grammar (RFC 8259 / ECMA-404) parser.
//
// Produces runtime values while recording the raw source span of every
// primitive node so the `json-parse-with-source` reviver context can report
// `context.source` for parsed primitives.

pub(crate) struct Parsed {
    pub value: Value,
    pub source: Option<String>,
}

const SOURCE_PREFIX: &str = "\0jsonsrc\0";

pub(crate) fn source_key(key: &str) -> String {
    format!("{SOURCE_PREFIX}{key}")
}

pub(crate) fn parse_text(text: &str) -> Result<Parsed, ()> {
    let mut parser = Parser { text, pos: 0 };
    parser.skip_whitespace();
    let parsed = parser.value()?;
    parser.skip_whitespace();
    if parser.pos != text.len() {
        return Err(());
    }
    Ok(parsed)
}

struct Parser<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.text[self.pos..]
    }

    fn skip_whitespace(&mut self) {
        let skipped = self
            .rest()
            .chars()
            .take_while(|character| matches!(character, '\t' | '\n' | '\r' | ' '))
            .map(char::len_utf8)
            .sum::<usize>();
        self.pos += skipped;
    }

    fn value(&mut self) -> Result<Parsed, ()> {
        let start = self.pos;
        let parsed = match self.rest().chars().next().ok_or(())? {
            '{' => self.object()?,
            '[' => self.array()?,
            '"' => self.string().map(|value| leaf(Value::String(value)))?,
            't' => self.literal("true", Value::Boolean(true))?,
            'f' => self.literal("false", Value::Boolean(false))?,
            'n' => self.literal("null", Value::Null)?,
            '-' | '0'..='9' => self.number()?,
            _ => return Err(()),
        };
        if parsed.source.is_none() {
            return Ok(parsed);
        }
        Ok(Parsed {
            value: parsed.value,
            source: Some(self.text[start..self.pos].to_string()),
        })
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Parsed, ()> {
        if !self.rest().starts_with(word) {
            return Err(());
        }
        self.pos += word.len();
        Ok(leaf(value))
    }

    fn object(&mut self) -> Result<Parsed, ()> {
        self.pos += 1;
        let mut properties = Vec::new();
        self.skip_whitespace();
        if self.consume('}') {
            return Ok(container(Value::Object(Rc::new(
                crate::value::ObjectData::new(properties),
            ))));
        }
        loop {
            self.skip_whitespace();
            let key = self.string()?;
            self.skip_whitespace();
            if !self.consume(':') {
                return Err(());
            }
            self.skip_whitespace();
            let child = self.value()?;
            self.skip_whitespace();
            record(&mut properties, key, child);
            if self.consume('}') {
                break;
            }
            if !self.consume(',') {
                return Err(());
            }
        }
        Ok(container(Value::Object(Rc::new(
            crate::value::ObjectData::new(properties),
        ))))
    }

    fn array(&mut self) -> Result<Parsed, ()> {
        self.pos += 1;
        let mut values = Vec::new();
        let mut named = Vec::new();
        self.skip_whitespace();
        if !self.consume(']') {
            loop {
                self.skip_whitespace();
                let child = self.value()?;
                if let Some(source) = &child.source {
                    let key = source_key(&values.len().to_string());
                    named.push((key, source_pair(child.value.clone(), source)));
                }
                values.push(child.value);
                self.skip_whitespace();
                if self.consume(']') {
                    break;
                }
                if !self.consume(',') {
                    return Err(());
                }
            }
        }
        let mut data = crate::value::ArrayData::new(values);
        for (key, pair) in named {
            data.set_property(&key, pair);
        }
        Ok(container(Value::Array(Rc::new(data))))
    }

    fn number(&mut self) -> Result<Parsed, ()> {
        let start = self.pos;
        self.consume('-');
        self.digits_whole()?;
        self.fraction()?;
        self.exponent()?;
        let text = &self.text[start..self.pos];
        let value = text.parse::<f64>().map_err(|_| ())?;
        Ok(leaf(Value::Number(value)))
    }

    fn digits_whole(&mut self) -> Result<(), ()> {
        if self.consume('0') {
            return Ok(());
        }
        if !matches!(self.rest().chars().next(), Some('1'..='9')) {
            return Err(());
        }
        self.pos += 1;
        self.skip_digits();
        Ok(())
    }

    fn skip_digits(&mut self) {
        let digits = self
            .rest()
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .count();
        self.pos += digits;
    }

    fn fraction(&mut self) -> Result<(), ()> {
        if !self.consume('.') {
            return Ok(());
        }
        self.digits_run()
    }

    fn exponent(&mut self) -> Result<(), ()> {
        if !self.consume('e') && !self.consume('E') {
            return Ok(());
        }
        if !self.consume('+') {
            self.consume('-');
        }
        self.digits_run()
    }

    fn digits_run(&mut self) -> Result<(), ()> {
        let start = self.pos;
        self.skip_digits();
        if self.pos == start {
            return Err(());
        }
        Ok(())
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.rest().starts_with(expected) {
            self.pos += expected.len_utf8();
            return true;
        }
        false
    }

    fn string(&mut self) -> Result<String, ()> {
        if !self.consume('"') {
            return Err(());
        }
        let mut value = String::new();
        loop {
            let character = self.rest().chars().next().ok_or(())?;
            self.pos += character.len_utf8();
            match character {
                '"' => return Ok(value),
                '\\' => self.escape(&mut value)?,
                '\u{0}'..='\u{1F}' => return Err(()),
                _ => value.push(character),
            }
        }
    }

    fn escape(&mut self, value: &mut String) -> Result<(), ()> {
        let character = self.rest().chars().next().ok_or(())?;
        self.pos += character.len_utf8();
        match character {
            '"' => value.push('"'),
            '\\' => value.push('\\'),
            '/' => value.push('/'),
            'b' => value.push('\u{8}'),
            'f' => value.push('\u{C}'),
            'n' => value.push('\n'),
            'r' => value.push('\r'),
            't' => value.push('\t'),
            'u' => self.unicode_escape(value)?,
            _ => return Err(()),
        }
        Ok(())
    }

    fn unicode_escape(&mut self, value: &mut String) -> Result<(), ()> {
        let unit = self.hex_quad()?;
        if (0xD800..0xDC00).contains(&unit) && self.rest().starts_with("\\u") {
            self.pos += 2;
            let low = self.hex_quad()?;
            if (0xDC00..0xE000).contains(&low) {
                let code = 0x1_0000 + (((unit - 0xD800) as u32) << 10) + (low - 0xDC00) as u32;
                value.push(char::from_u32(code).ok_or(())?);
                return Ok(());
            }
            push_lone_surrogate(value, unit);
            push_code_unit(value, low);
            return Ok(());
        }
        push_code_unit(value, unit);
        Ok(())
    }

    fn hex_quad(&mut self) -> Result<u16, ()> {
        let digits = self.rest().get(..4).ok_or(())?;
        let value = u16::from_str_radix(digits, 16).map_err(|_| ())?;
        if digits.len() != 4 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(());
        }
        self.pos += 4;
        Ok(value)
    }
}

fn leaf(value: Value) -> Parsed {
    Parsed {
        value,
        source: Some(String::new()),
    }
}

fn container(value: Value) -> Parsed {
    Parsed {
        value,
        source: None,
    }
}

fn push_lone_surrogate(value: &mut String, unit: u16) {
    value.push_str(&format!("\\u{unit:04x}"));
}

fn push_code_unit(value: &mut String, unit: u16) {
    if (0xD800..0xE000).contains(&unit) {
        push_lone_surrogate(value, unit);
    } else if let Some(character) = char::from_u32(unit as u32) {
        value.push(character);
    }
}

fn record(properties: &mut Vec<(String, Value)>, key: String, child: Parsed) {
    if let Some(source) = &child.source {
        let pair = source_pair(child.value.clone(), source);
        upsert(properties, source_key(&key), pair);
    }
    upsert(properties, key, child.value);
}

fn source_pair(value: Value, source: &str) -> Value {
    Value::array(vec![value, Value::String(source.to_string())])
}

fn upsert(properties: &mut Vec<(String, Value)>, key: String, value: Value) {
    if let Some((_, current)) = properties.iter_mut().find(|(name, _)| *name == key) {
        *current = value;
    } else {
        properties.push((key, value));
    }
}
