//! `util` module — formatting + type inspection.
//!
//! Node-compatible `util.format` with `%s`, `%d`, `%i`, `%f`, `%j`,
//! `%o`, `%O`, `%%`. Plus `util.inspect` (string-only; sufficient
//! for the test262 + Node fixture conformance surface).

use std::cell::RefCell;

use quench_runtime::execute;
use quench_runtime::value::Value;
thread_local! {
    /// The live `util.inspect.defaultOptions` object; formatters read
    /// through it so JavaScript-side mutation is observed.
    static INSPECT_DEFAULT_OPTIONS: RefCell<Option<Value>> = const { RefCell::new(None) };
    /// Per-call override set by `util.formatWithOptions`.
    static SEPARATOR_OVERRIDE: RefCell<Option<bool>> = const { RefCell::new(None) };
}

/// `util.formatWithOptions(options, ...args)`.
pub fn format_with_options(args: &[Value], numeric_separator: bool) -> String {
    SEPARATOR_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(numeric_separator));
    let result = format(args);
    SEPARATOR_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
    result
}

/// Module wiring: returns the `(name, value)` pairs the host
/// installs into the `util` namespace.
fn build_factory(args: &[Value]) -> Value {
    let Ok(program) = quench_runtime::reduce::reduce_global_script_source(
        r#"(function(format,inspect,deepEqual,styleText,formatWithOptions,stripVT,inherits,getCallSites){
      function promisify(fn){ var p=function(){var a=[].slice.call(arguments), self=this;
        return new Promise(function(resolve,reject){a.push(function(e,v){if(e)reject(e);else resolve(v)});fn.apply(self,a);});};
        if (fn && fn[promisify.custom]) return fn[promisify.custom]; return p; }
      promisify.custom=Symbol('custom'); promisify[Symbol.for('nodejs.util.promisify.custom')]=promisify.custom;
      function callbackify(fn){return function(){var a=[].slice.call(arguments), cb=a.pop(), self=this;
        Promise.resolve(fn.apply(self,a)).then(function(v){cb(null,v)},function(e){cb(e);});};}
      var types={isArrayBuffer:function(v){return v instanceof ArrayBuffer},isAnyArrayBuffer:function(v){return v instanceof ArrayBuffer},isDataView:function(v){return typeof DataView!=='undefined'&&v instanceof DataView},isTypedArray:function(v){return typeof ArrayBuffer!=='undefined'&&ArrayBuffer.isView(v)&&!(v instanceof DataView)},
        isUint8Array:function(v){return v instanceof Uint8Array},isUint8ClampedArray:function(v){return typeof Uint8ClampedArray!=='undefined'&&v instanceof Uint8ClampedArray},isUint16Array:function(v){return typeof Uint16Array!=='undefined'&&v instanceof Uint16Array},isUint32Array:function(v){return typeof Uint32Array!=='undefined'&&v instanceof Uint32Array},
        isInt8Array:function(v){return typeof Int8Array!=='undefined'&&v instanceof Int8Array},isInt16Array:function(v){return typeof Int16Array!=='undefined'&&v instanceof Int16Array},isInt32Array:function(v){return typeof Int32Array!=='undefined'&&v instanceof Int32Array},isFloat32Array:function(v){return typeof Float32Array!=='undefined'&&v instanceof Float32Array},isFloat64Array:function(v){return typeof Float64Array!=='undefined'&&v instanceof Float64Array},isBigInt64Array:function(v){return typeof BigInt64Array!=='undefined'&&v instanceof BigInt64Array},isBigUint64Array:function(v){return typeof BigUint64Array!=='undefined'&&v instanceof BigUint64Array},
        isDate:function(v){return v instanceof Date},isRegExp:function(v){return v instanceof RegExp},
        isMap:function(v){return v instanceof Map},isSet:function(v){return v instanceof Set},isString:function(v){return typeof v==='string'||v instanceof String},
        isNumber:function(v){return typeof v==='number'||v instanceof Number},isBoolean:function(v){return typeof v==='boolean'||v instanceof Boolean},
        isBigInt:function(v){return typeof v==='bigint'||v instanceof Object&&Object.prototype.toString.call(v)==='[object BigInt]'},isSymbol:function(v){return typeof v==='symbol'||v instanceof Symbol},isFunction:function(v){return typeof v==='function'},isError:function(v){return v instanceof Error},isArgumentsObject:function(v){return Object.prototype.toString.call(v)==='[object Arguments]'},isPrimitive:function(v){return v===null||(typeof v!=='object'&&typeof v!=='function')},
        isNull:function(v){return v===null},isUndefined:function(v){return v===undefined},
        isBuffer:function(v){return typeof Buffer!=='undefined'&&Buffer.isBuffer(v)},isPromise:function(v){return v instanceof Promise}};
      function deprecate(fn){return function(){return fn.apply(this,arguments)}} function debuglog(){return function(){}}
      function parseArgs(config){
        config=config||{}; var input=config.args||((typeof process!=='undefined'&&process.argv)||[]).slice(2);
        var specs=config.options||{}, values={}, positionals=[], tokens=[];
        Object.keys(specs).forEach(function(k){var s=specs[k]||{}; if(s.default!==undefined) values[k]=s.default; else if(s.type==='boolean') values[k]=false;});
        function set(k,v){var s=specs[k]||{}, value=s.multiple?(Array.isArray(values[k])?values[k]:[]).concat([v]):v; values[k]=value;}
        function optionToken(name,rawName,value,index,inlineValue){if(config.tokens) tokens.push({kind:'option',name:name,rawName:rawName,value:value,index:index,inlineValue:inlineValue});}
        function positionalToken(value,index){if(config.tokens) tokens.push({kind:'positional',value:value,index:index});}
        for(var i=0;i<input.length;i++){var arg=String(input[i]), name, value, rawName, inlineValue=false;
          if(arg==='--'){for(var j=i+1;j<input.length;j++){positionals.push(input[j]); positionalToken(input[j],j);} break;}
          if(arg.indexOf('--')===0){var raw=arg.slice(2), eq=raw.indexOf('='); value=eq<0?undefined:raw.slice(eq+1); name=eq<0?raw:raw.slice(0,eq); rawName=arg;
            if(name.indexOf('no-')===0 && specs[name.slice(3)]&&specs[name.slice(3)].type==='boolean'){set(name.slice(3),false); optionToken(name.slice(3),rawName,false,i,false); continue;}
            if(!specs[name]){if(config.strict!==false) throw new TypeError('Unknown option: --'+name); positionals.push(arg); positionalToken(arg,i); continue;}
            if(specs[name].type==='boolean') {set(name,value===undefined?true:value!=='false'); inlineValue=eq>=0; optionToken(name,rawName,value===undefined?true:value!=='false',i,inlineValue);}
            else {if(value===undefined) value=String(input[++i]); else inlineValue=true; set(name,value); optionToken(name,rawName,value,i,inlineValue);} continue;
          }
          if(arg.charAt(0)==='-'&&arg.length>1){name=arg.charAt(1); var key=Object.keys(specs).find(function(k){return specs[k].short===name;});
            if(key){if(specs[key].type==='boolean'){set(key,true); optionToken(key,arg,true,i,false);} else {value=String(input[++i]); set(key,value); optionToken(key,arg,value,i,false);} continue;}
          }
          if(config.allowPositionals!==false){positionals.push(input[i]); positionalToken(input[i],i);} else if(config.strict!==false) throw new TypeError('Unexpected argument: '+arg);
        }
        var result={values:values,positionals:positionals}; if(config.tokens) result.tokens=tokens; return result;
      }
      return {format:format,inspect:inspect,isDeepStrictEqual:deepEqual,styleText:styleText,formatWithOptions:formatWithOptions,stripVTControlCharacters:stripVT,inherits:inherits,getCallSites:getCallSites,promisify:promisify,callbackify:callbackify,parseArgs:parseArgs,types:types,deprecate:deprecate,debuglog:debuglog};
    })"#,
    ) else {
        return Value::Undefined;
    };
    let context = quench_runtime::vm::current_context();
    let mut regs = Vec::new();
    quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::execute_in_place_context(program.ops(), &mut regs, &context)
    })
    .unwrap_or(Value::Undefined)
}

pub fn build() -> Vec<(String, Value)> {
    let values = [
        crate::host::capability(crate::registry::SPEC_UTIL_FORMAT),
        inspect_capability(),
        crate::host::capability(crate::registry::SPEC_UTIL_IS_DEEP_STRICT_EQUAL),
        crate::host::capability(crate::registry::SPEC_UTIL_STYLE_TEXT),
        crate::host::capability(crate::registry::SPEC_UTIL_FORMAT_WITH_OPTIONS),
        crate::host::capability(crate::registry::SPEC_UTIL_STRIP_VT),
        crate::host::capability(crate::registry::SPEC_UTIL_INHERITS),
        crate::host::capability(crate::registry::SPEC_UTIL_GETCALLSITES),
    ];
    let module =
        quench_runtime::vm::call_value(&build_factory(&values), &Value::Undefined, &values)
            .unwrap_or(Value::Undefined);
    execute_pairs(module)
}

fn execute_pairs(module: Value) -> Vec<(String, Value)> {
    execute::own_enumerable_keys(&module)
        .into_iter()
        .filter_map(|k| {
            execute::get_property_result(&module, &k)
                .ok()
                .map(|v| (k, v))
        })
        .collect()
}

fn inspect_capability() -> Value {
    let inspect = crate::host::capability(crate::registry::SPEC_UTIL_INSPECT);
    let options = quench_runtime::host_api::object(vec![(
        "numericSeparator".to_string(),
        Value::Boolean(false),
    )]);
    INSPECT_DEFAULT_OPTIONS.with(|slot| *slot.borrow_mut() = Some(options.clone()));
    let _ = quench_runtime::execute::set_callable_property(&inspect, "defaultOptions", options);
    inspect
}

fn numeric_separator() -> bool {
    if let Some(override_) = SEPARATOR_OVERRIDE.with(|slot| *slot.borrow()) {
        return override_;
    }
    INSPECT_DEFAULT_OPTIONS.with(|slot| {
        let options = slot.borrow();
        let Some(options) = options.as_ref() else {
            return false;
        };
        let options = quench_runtime::execute::resolve_alias(options);
        quench_runtime::execute::is_truthy(&quench_runtime::execute::get_property(
            &options,
            "numericSeparator",
        ))
    })
}

/// Group integer digits into `_`-separated triples (Node's
/// `numericSeparator` rendering); fraction/exponent stay untouched.
fn separate_digits(text: &str) -> String {
    let (sign, rest) = text.strip_prefix('-').map_or(("", text), |r| ("-", r));
    let end = rest.find(['.', 'e', 'E', 'n']).unwrap_or(rest.len());
    let (int, tail) = rest.split_at(end);
    let mut grouped = String::with_capacity(text.len() + int.len() / 3);
    for (index, c) in int.chars().enumerate() {
        if index > 0 && (int.len() - index) % 3 == 0 {
            grouped.push('_');
        }
        grouped.push(c);
    }
    format!("{sign}{grouped}{tail}")
}

/// `util.format` — see test fixture `parallel/test-util-format.js`.
pub fn format(args: &[Value]) -> String {
    if args.is_empty() {
        return String::new();
    }
    if let Value::String(template) = &args[0] {
        if !quench_runtime::execute::is_symbol(&args[0]) {
            return format_template(template, args);
        }
    }
    format_varargs(args)
}

/// Public for `console.log` reuse.
pub fn format_template(template: &str, args: &[Value]) -> String {
    let mut out = String::new();
    let mut iter = template.chars().peekable();
    let mut index = 1usize;
    while let Some(c) = iter.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let Some(spec) = iter.next() else {
            out.push('%');
            break;
        };
        if spec == '%' {
            out.push('%');
            continue;
        }
        let Some(arg) = args.get(index).cloned() else {
            out.push('%');
            out.push(spec);
            continue;
        };
        index += 1;
        out.push_str(&format_spec(spec, &arg));
    }
    // Node's util.format appends remaining positional args separated
    // by spaces, mirroring console.log's behavior.
    for arg in args.iter().skip(index) {
        out.push(' ');
        out.push_str(&format_spec('s', arg));
    }
    out
}

fn format_varargs(args: &[Value]) -> String {
    let mut out = String::new();
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&inspect(arg));
    }
    out
}

fn format_spec(spec: char, arg: &Value) -> String {
    match spec {
        's' => value_to_string(arg),
        'd' => to_number_string(arg),
        'i' => to_int_string(arg),
        'f' => to_float_string(arg),
        'j' => json_string(arg),
        'o' | 'O' => inspect(arg),
        other => format!("%{other}"),
    }
}

fn value_to_string(value: &Value) -> String {
    if quench_runtime::execute::is_symbol(value) {
        return symbol_string(value);
    }
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => js_number(*n),
        Value::BigInt(digits) => format!("{}n", bigint_digits(digits)),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        // Node: objects with a custom `toString` go through `String(arg)`;
        // plain objects inspect.
        Value::Object(_)
        | Value::ObjectAlias(_)
        | Value::Array(_)
        | Value::Function(_)
        | Value::BoundFunction(_) => match quench_runtime::execute::to_js_string(value) {
            Ok(text) if text != "[object Object]" && !text.is_empty() => text,
            // `%s` inspects plain objects at depth 0: nested containers
            // collapse to `[Array]` / `[Object]`.
            _ => inspect_depth(value, 0),
        },
        _ => "<unknown>".into(),
    }
}

/// BigInt digits, grouped when `numericSeparator` is on.
fn bigint_digits(digits: &str) -> String {
    if numeric_separator() {
        separate_digits(digits)
    } else {
        digits.to_string()
    }
}

/// JavaScript number rendering honoring `numericSeparator`.
fn js_number(n: f64) -> String {
    if n == 0.0 && n.is_sign_negative() {
        return "-0".into();
    }
    let text = quench_runtime::execute::number_to_js_string(n);
    if numeric_separator() {
        separate_digits(&text)
    } else {
        text
    }
}

/// `Symbol.prototype.toString` rendering: `Symbol(desc)`.
fn symbol_string(value: &Value) -> String {
    let Value::String(payload) = value else {
        return "Symbol()".into();
    };
    let (body, suffix) = payload.split_once('\0').unwrap_or((payload.as_str(), ""));
    if let Some(key) = body.strip_prefix("Symbol.for.") {
        return format!("Symbol.for({key})");
    }
    let unique = !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit());
    if !unique {
        return format!("Symbol({body})");
    }
    let description = body.strip_prefix("Symbol.").unwrap_or(body);
    if description.is_empty() || description == "\u{1}" {
        return "Symbol()".into();
    }
    format!("Symbol({description})")
}

fn to_number(value: &Value) -> f64 {
    match value {
        Value::Number(n) => *n,
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                0.0
            } else {
                trimmed.parse().unwrap_or(f64::NAN)
            }
        }
        Value::Boolean(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::Null => 0.0,
        _ => f64::NAN,
    }
}

/// `%i` — `parseInt(arg, 10)`: integers keep their digits, numbers
/// stringify first (so `1.18e+21` parses as `1`), anything else is NaN.
fn to_int_string(value: &Value) -> String {
    if let Value::BigInt(digits) = value {
        return format!("{}n", bigint_digits(digits));
    }
    let text = match value {
        Value::Number(n) if n.is_finite() => quench_runtime::execute::number_to_js_string(*n),
        Value::String(s) => s.trim().to_string(),
        _ => return "NaN".into(),
    };
    let text = text.strip_prefix('+').unwrap_or(&text);
    let digits: String = text.chars().take_while(|c| c.is_ascii_digit()).collect();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (
            true,
            rest.chars().take_while(|c| c.is_ascii_digit()).collect(),
        ),
        None => (false, digits),
    };
    if digits.is_empty() {
        return "NaN".into();
    }
    let grouped = if numeric_separator() {
        separate_digits(&digits)
    } else {
        digits
    };
    if negative {
        format!("-{grouped}")
    } else {
        grouped
    }
}

/// `%d` — `Number(arg)` rendered with JavaScript number formatting;
/// BigInts render as digits plus `n`.
fn to_number_string(value: &Value) -> String {
    if let Value::BigInt(digits) = value {
        return format!("{}n", bigint_digits(digits));
    }
    let n = to_number(value);
    if n.is_nan() {
        return "NaN".into();
    }
    if n == 0.0 && n.is_sign_negative() {
        return "-0".into();
    }
    js_number(n)
}

/// `%f` — `parseFloat`-style: strings parse their leading float,
/// BigInts convert via digits, `-0` renders as `-0`.
fn to_float_string(value: &Value) -> String {
    if let Value::BigInt(digits) = value {
        let n = digits.parse::<f64>().unwrap_or(f64::NAN);
        return float_text(n);
    }
    let n = match value {
        Value::Number(n) => *n,
        Value::String(s) => parse_float_prefix(s),
        _ => to_number(value),
    };
    float_text(n)
}

fn float_text(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n == 0.0 && n.is_sign_negative() {
        return "-0".into();
    }
    quench_runtime::execute::number_to_js_string(n)
}

fn parse_float_prefix(text: &str) -> f64 {
    let text = text.trim_start();
    let mut end = 0;
    for (index, c) in text.char_indices() {
        let part = c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E');
        if !part
            || (matches!(c, '+' | '-')
                && index > 0
                && !matches!(text.as_bytes()[index - 1], b'e' | b'E'))
        {
            break;
        }
        end = index + 1;
    }
    text[..end].parse().unwrap_or(f64::NAN)
}

fn json_string(value: &Value) -> String {
    match quench_runtime::execute::json_stringify(value) {
        Ok(Value::String(json)) => json,
        Ok(_) => "undefined".into(),
        Err(error) => {
            let message = format!("{error:?}");
            if message.contains("ircular") {
                "[Circular]".into()
            } else {
                "undefined".into()
            }
        }
    }
}

pub use crate::modules::buffer_enc::invalid_arg_received;

/// `util.inspect` — string-only, sufficient for fixtures.
pub fn inspect(value: &Value) -> String {
    inspect_depth(value, 2)
}

fn inspect_depth(value: &Value, depth: usize) -> String {
    if quench_runtime::execute::is_symbol(value) {
        return symbol_string(value);
    }
    match value {
        Value::String(s) => format!("'{s}'"),
        Value::Number(n) => js_number(*n),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        Value::Object(_) | Value::ObjectAlias(_) => inspect_object(value, depth),
        Value::Array(_) => inspect_array(value, depth),
        Value::Uint8Array(_) => "[Buffer]".into(),
        Value::BigInt(digits) => format!("{digits}n"),
        _ => "<unknown>".into(),
    }
}

fn inspect_array(value: &Value, depth: usize) -> String {
    if depth == 0 {
        return "[Array]".into();
    }
    let mut items = Vec::new();
    for index in 0..64u32 {
        let item = quench_runtime::execute::get_property(value, &index.to_string());
        if matches!(item, Value::Undefined) {
            break;
        }
        items.push(inspect_at(&item, depth - 1));
    }
    if items.is_empty() {
        return "[]".into();
    }
    format!("[ {} ]", items.join(", "))
}

fn inspect_at(value: &Value, depth: usize) -> String {
    if depth == 0 {
        return inspect_shallow(value);
    }
    match value {
        Value::Object(_) | Value::ObjectAlias(_) if depth > 0 => inspect_object(value, depth),
        Value::Array(_) if depth > 0 => inspect_array(value, depth),
        _ => inspect_shallow(value),
    }
}

/// Plain objects render as `{ key: value, ... }` with shallow values.
fn inspect_object(value: &Value, depth: usize) -> String {
    let null_prototype = matches!(
        quench_runtime::execute::get_prototype_of(value),
        Ok(Value::Null)
    );
    let keys = quench_runtime::execute::own_enumerable_keys(value);
    if keys.is_empty() {
        return if null_prototype {
            "[Object: null prototype] {}".into()
        } else {
            "{}".into()
        };
    }
    let body = keys
        .iter()
        .map(|key| {
            format!(
                "{key}: {}",
                inspect_at(&quench_runtime::execute::get_property(value, key), depth)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    if null_prototype {
        return format!("[Object: null prototype] {{ {body} }}");
    }
    format!("{{ {body} }}")
}

fn inspect_shallow(value: &Value) -> String {
    if quench_runtime::execute::is_symbol(value) {
        return symbol_string(value);
    }
    match value {
        Value::String(s) => format!("'{s}'"),
        Value::Number(n) => js_number(*n),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        Value::Object(_) | Value::ObjectAlias(_) => "[Object]".into(),
        Value::Array(_) => "[Array]".into(),
        _ => "<unknown>".into(),
    }
}
