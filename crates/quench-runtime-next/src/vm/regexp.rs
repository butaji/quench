use super::*;
use regex::RegexBuilder;

impl<H: Host> Vm<H> {
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
        let source_value = self.heap.alloc(Cell::String(pattern));
        let flags_value = self.heap.alloc(Cell::String(flags));
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
        let source = self.to_string(p, self.get_property(p, this, source_atom)?)?;
        let flags = self.to_string(p, self.get_property(p, this, flags_atom)?)?;
        let regex = Self::compile_regexp(&source, &flags)?;
        let input = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let stateful = flags.contains('g') || flags.contains('y');
        let sticky = flags.contains('y');
        let last_index_atom = self.intern_atom("lastIndex");
        let start = if stateful {
            let value = self.get_property(p, this, last_index_atom)?;
            let number = self.to_number(p, value)?;
            if number.is_finite() && number > 0.0 {
                let mut index = number.floor() as usize;
                index = index.min(input.len());
                while index > 0 && !input.is_char_boundary(index) {
                    index -= 1;
                }
                index
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
        let index = captures.get(0).map_or(0, |value| value.start());
        if stateful {
            let end = captures.get(0).map_or(start, |value| value.end());
            self.set_property(this, last_index_atom, Value::number(end as f64))?;
        }
        let index_atom = self.intern_atom("index");
        self.set_property(result, index_atom, Value::number(index as f64))?;
        let input_value = self.heap.alloc(Cell::String(input));
        let input_atom = self.intern_atom("input");
        self.set_property(result, input_atom, input_value)?;
        Ok(result)
    }

    fn compile_regexp(source: &str, flags: &str) -> Result<regex::Regex, JsError> {
        let mut builder = RegexBuilder::new(source);
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
