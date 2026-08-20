//! `QueryString.parse` — the character-by-character scan of Node's
//! `lib/querystring.js`, including multi-character separators, the `%XX`
//! encode tracker, and the `maxKeys` pair budget.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::querystring::{
    decode_str, is_hex, module_fn, units_of_value, Decode, EQ_DEFAULT, PLUS_DECODED, PLUS_ENCODED,
    SEP_DEFAULT,
};
// ---- parse ----

/// Mutable state of Node's character-by-character parse scan.
struct Scan<'a> {
    units: &'a [u16],
    sep: Vec<u16>,
    eq: Vec<u16>,
    last_pos: usize,
    sep_idx: usize,
    eq_idx: usize,
    key: Vec<u16>,
    value: Vec<u16>,
    key_encoded: bool,
    val_encoded: bool,
    encode_check: u8,
    pairs: i64,
    plus_char: &'static [u16],
    custom: bool,
}

impl Scan<'_> {
    /// A unit matched the separator at `sep_idx`. Returns `true` when the
    /// pair budget is exhausted and parsing must stop.
    fn on_separator(
        &mut self,
        i: usize,
        out: &mut Vec<(String, Vec<String>)>,
        decode: &Decode,
        fallback: &Decode,
    ) -> bool {
        self.sep_idx += 1;
        if self.sep_idx != self.sep.len() {
            return false;
        }
        let end = i + 1 - self.sep_idx;
        if self.eq_idx < self.eq.len() {
            if self.last_pos < end {
                self.key.extend_from_slice(&self.units[self.last_pos..end]);
            } else if self.key.is_empty() {
                self.pairs -= 1;
                if self.pairs == 0 {
                    return true;
                }
                self.last_pos = i + 1;
                self.sep_idx = 0;
                self.eq_idx = 0;
                return false;
            }
        } else if self.last_pos < end {
            self.value
                .extend_from_slice(&self.units[self.last_pos..end]);
        }
        self.flush(out, decode, fallback);
        self.pairs -= 1;
        if self.pairs == 0 {
            return true;
        }
        self.last_pos = i + 1;
        self.sep_idx = 0;
        self.eq_idx = 0;
        false
    }

    fn flush(&mut self, out: &mut Vec<(String, Vec<String>)>, decode: &Decode, fallback: &Decode) {
        add_key_val(
            out,
            &self.key,
            &self.value,
            self.key_encoded,
            self.val_encoded,
            decode,
            fallback,
        );
        self.key_encoded = self.custom;
        self.val_encoded = self.custom;
        self.key = Vec::new();
        self.value = Vec::new();
        self.encode_check = 0;
    }

    fn on_other(&mut self, i: usize, code: u16) {
        self.sep_idx = 0;
        if self.eq_idx < self.eq.len() && self.on_eq_char(i, code) {
            return;
        }
        if code == 43 {
            if self.last_pos < i {
                self.value.extend_from_slice(&self.units[self.last_pos..i]);
            }
            self.value.extend_from_slice(self.plus_char);
            self.last_pos = i + 1;
        } else if !self.val_encoded {
            self.track_value_encoding(code);
        }
    }

    /// Handle a non-separator unit while `eq` is still being matched.
    /// Returns `false` to fall through to value-side handling.
    fn on_eq_char(&mut self, i: usize, code: u16) -> bool {
        if self.eq.get(self.eq_idx) == Some(&code) {
            self.eq_idx += 1;
            if self.eq_idx == self.eq.len() {
                let end = i + 1 - self.eq_idx;
                if self.last_pos < end {
                    self.key.extend_from_slice(&self.units[self.last_pos..end]);
                }
                self.encode_check = 0;
                self.last_pos = i + 1;
            }
            return true;
        }
        self.eq_idx = 0;
        if !self.key_encoded && self.track_key_encoding(code) {
            return true;
        }
        if code == 43 {
            if self.last_pos < i {
                self.key.extend_from_slice(&self.units[self.last_pos..i]);
            }
            self.key.extend_from_slice(self.plus_char);
            self.last_pos = i + 1;
        }
        true
    }

    /// `%XX` tracker for the key side; `true` when the unit was consumed.
    fn track_key_encoding(&mut self, code: u16) -> bool {
        if code == 37 {
            self.encode_check = 1;
            return true;
        }
        if self.encode_check == 0 {
            return false;
        }
        if is_hex(code) {
            self.encode_check += 1;
            if self.encode_check == 3 {
                self.key_encoded = true;
            }
            return true;
        }
        self.encode_check = 0;
        false
    }

    fn track_value_encoding(&mut self, code: u16) {
        if code == 37 {
            self.encode_check = 1;
        } else if self.encode_check > 0 {
            if is_hex(code) {
                self.encode_check += 1;
                if self.encode_check == 3 {
                    self.val_encoded = true;
                }
            } else {
                self.encode_check = 0;
            }
        }
    }
}

fn add_key_val(
    out: &mut Vec<(String, Vec<String>)>,
    key: &[u16],
    value: &[u16],
    key_encoded: bool,
    val_encoded: bool,
    decode: &Decode,
    fallback: &Decode,
) {
    let key = if !key.is_empty() && key_encoded {
        decode_str(key, decode, fallback)
    } else {
        String::from_utf16_lossy(key)
    };
    let value = if !value.is_empty() && val_encoded {
        decode_str(value, decode, fallback)
    } else {
        String::from_utf16_lossy(value)
    };
    match out.iter_mut().find(|(k, _)| *k == key) {
        Some((_, values)) => values.push(value),
        None => out.push((key, vec![value])),
    }
}

/// `charCodes(String(arg))` with Node's `!arg ? default` fallback.
fn codes(arg: Option<&Value>, default: &[u16]) -> Result<Vec<u16>, VmError> {
    match arg {
        Some(value) if execute::is_truthy(value) => units_of_value(value),
        _ => Ok(default.to_vec()),
    }
}

/// `options.maxKeys`: positive numbers cap the pairs, everything else
/// numeric means unlimited, non-numbers keep the default of 1000.
fn max_keys(options: Option<&Value>) -> i64 {
    let Some(options) = options else {
        return 1000;
    };
    match execute::get_property(options, "maxKeys") {
        Value::Number(n) if n > 0.0 => n as i64,
        Value::Number(_) => -1,
        _ => 1000,
    }
}

/// Pick the decoder (and the `QueryString.unescape` fallback) exactly like
/// Node: `options.decodeURIComponent` wins, else the module's `unescape`.
fn select_decode(options: Option<&Value>, receiver: Option<&Value>) -> (Decode, Decode) {
    let fallback = module_fn(receiver, "unescape", crate::registry::SPEC_QS_UNESCAPE.cap);
    let decode = match options.map(|o| execute::get_property(o, "decodeURIComponent")) {
        Some(f) if quench_runtime::is_callable(&f) => Decode::Custom(f),
        _ => fallback.clone(),
    };
    (decode, fallback)
}

pub fn parse(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let Some(units) = args.first().and_then(execute::string_units) else {
        return null_object(out);
    };
    if units.is_empty() {
        return null_object(out);
    }
    let options = args.get(3);
    let (decode, fallback) = select_decode(options, receiver);
    let custom = matches!(decode, Decode::Custom(_));
    let mut scan = Scan::new(&units, args, custom, max_keys(options))?;
    let done = scan.run(&mut out, &decode, &fallback);
    if !done {
        finish_scan(&mut scan, &units, &mut out, &decode, &fallback);
    }
    null_object(out)
}

impl<'a> Scan<'a> {
    fn new(
        units: &'a [u16],
        args: &[Value],
        custom: bool,
        pairs: i64,
    ) -> Result<Scan<'a>, VmError> {
        Ok(Scan {
            units,
            sep: codes(args.get(1), SEP_DEFAULT)?,
            eq: codes(args.get(2), EQ_DEFAULT)?,
            last_pos: 0,
            sep_idx: 0,
            eq_idx: 0,
            key: Vec::new(),
            value: Vec::new(),
            key_encoded: custom,
            val_encoded: custom,
            encode_check: 0,
            pairs,
            plus_char: if custom { PLUS_ENCODED } else { PLUS_DECODED },
            custom,
        })
    }

    /// Drive the scan; returns `true` when the pair budget stopped it early.
    fn run(
        &mut self,
        out: &mut Vec<(String, Vec<String>)>,
        decode: &Decode,
        fallback: &Decode,
    ) -> bool {
        for (i, &code) in self.units.iter().enumerate() {
            if self.sep.get(self.sep_idx) == Some(&code) {
                if self.on_separator(i, out, decode, fallback) {
                    return true;
                }
            } else {
                self.on_other(i, code);
            }
        }
        false
    }
}

fn finish_scan(
    scan: &mut Scan,
    units: &[u16],
    out: &mut Vec<(String, Vec<String>)>,
    decode: &Decode,
    fallback: &Decode,
) {
    if scan.last_pos < units.len() {
        if scan.eq_idx < scan.eq.len() {
            scan.key.extend_from_slice(&units[scan.last_pos..]);
        } else if scan.sep_idx < scan.sep.len() {
            scan.value.extend_from_slice(&units[scan.last_pos..]);
        }
    } else if scan.eq_idx == 0 && scan.key.is_empty() {
        return;
    }
    scan.flush(out, decode, fallback);
}

/// Build the result object with a null prototype and insertion-ordered keys.
fn null_object(entries: Vec<(String, Vec<String>)>) -> Result<Value, VmError> {
    let pairs = entries
        .into_iter()
        .map(|(key, values)| {
            let value = if values.len() == 1 {
                Value::String(values.into_iter().next().unwrap_or_default())
            } else {
                host_api::array(values.into_iter().map(Value::String).collect())
            };
            (key, value)
        })
        .collect();
    let object = host_api::object(pairs);
    execute::set_prototype_of(&object, &Value::Null)
}
