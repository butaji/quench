//! The WHATWG `URL` and `URLSearchParams` classes (lib/internal/url.js) in Rust over the
//! `url` crate. A URL keeps its serialization in a private slot; a search-params object
//! keeps its name/value list (alternating strings) and the URL it belongs to.
use crate::host::shared_vm::operation;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, ValueType};
use url::Url;

type Context<'a> = NativeContext<'a, NodeHost>;

/// The private slots, cached as host-private symbols, and the classes.
const HREF_SLOT: &str = "\0node/url/href";
const PARAMS_SLOT: &str = "\0node/url/searchParams";
const LIST_SLOT: &str = "\0node/url/list";
const OWNER_SLOT: &str = "\0node/url/owner";
const ITERATOR_SLOT: &str = "\0node/url/iterator";
const URL_CLASS: &str = "\0node/url/URL";
const PARAMS_CLASS: &str = "\0node/url/URLSearchParams";
const ITERATOR_PROTOTYPE: &str = "\0node/url/iteratorPrototype";

/// The URL accessors, in Node's prototype order, with whether each has a setter.
#[derive(Clone, Copy, PartialEq)]
enum Component {
    Href,
    Origin,
    Protocol,
    Username,
    Password,
    Host,
    Hostname,
    Port,
    Pathname,
    Search,
    SearchParams,
    Hash,
}

const COMPONENTS: &[(&str, Component, bool)] = &[
    ("href", Component::Href, true),
    ("origin", Component::Origin, false),
    ("protocol", Component::Protocol, true),
    ("username", Component::Username, true),
    ("password", Component::Password, true),
    ("host", Component::Host, true),
    ("hostname", Component::Hostname, true),
    ("port", Component::Port, true),
    ("pathname", Component::Pathname, true),
    ("search", Component::Search, true),
    ("searchParams", Component::SearchParams, false),
    ("hash", Component::Hash, true),
];

/// What an iterator over search params yields (`'key'`, `'value'`, `'key+value'`).
#[derive(Clone, Copy)]
enum Kind {
    Keys,
    Values,
    Entries,
}

/// The `URLSearchParams.prototype` methods, in Node's order, with the arguments each
/// requires (`ERR_MISSING_ARGS`) and its length.
const PARAMS_METHODS: &[(&str, &str, usize)] = &[
    ("append", "urlParamsAppend", 2),
    ("delete", "urlParamsDelete", 1),
    ("get", "urlParamsGet", 1),
    ("getAll", "urlParamsGetAll", 1),
    ("has", "urlParamsHas", 1),
    ("set", "urlParamsSet", 2),
    ("sort", "urlParamsSort", 0),
    ("entries", "urlParamsEntries", 0),
    ("forEach", "urlParamsForEach", 1),
    ("keys", "urlParamsKeys", 0),
    ("values", "urlParamsValues", 0),
    ("toString", "urlParamsToString", 0),
];

type Pairs = Vec<(String, String)>;

fn cached(c: &mut Context<'_>, key: &str) -> Option<RootId> {
    c.host_mut().shared_state().borrow().builtin_cache.get(key).copied()
}

fn cache(c: &mut Context<'_>, key: &str, value: RootId) -> Result<RootId, RootedError> {
    let value = c.retain(value)?;
    c.host_mut().shared_state().borrow_mut().builtin_cache.insert(key.to_owned(), value);
    Ok(value)
}

fn slot(c: &mut Context<'_>, key: &str) -> Result<RootId, RootedError> {
    if let Some(symbol) = cached(c, key) {
        return Ok(symbol);
    }
    let symbol = c.private_symbol_rooted(Some(key.trim_start_matches('\0')));
    cache(c, key, symbol)
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    match args.get(index) {
        Some(value) => *value,
        None => c.undefined(),
    }
}

fn type_of(c: &Context<'_>, value: RootId) -> ValueType {
    c.value_type_rooted(value).unwrap_or(ValueType::Undefined)
}

fn read_slot(c: &mut Context<'_>, object: RootId, key: &str) -> Result<Option<RootId>, RootedError> {
    if !c.is_object_rooted(object)? {
        return Ok(None);
    }
    let slot = slot(c, key)?;
    if c.own_property_fields_rooted(object, slot)?.is_none() {
        return Ok(None);
    }
    c.get_property_rooted(object, slot).map(Some)
}

fn write_slot(c: &mut Context<'_>, object: RootId, key: &str, value: RootId) -> Result<(), RootedError> {
    let slot = slot(c, key)?;
    c.define_data_property_rooted(object, slot, value, true, false, true)?;
    Ok(())
}

/// `StringPrototypeToWellFormed(`${value}`)`.
fn usv_string(c: &mut Context<'_>, value: RootId) -> Result<String, RootedError> {
    let units = c.to_string_code_units(value)?;
    Ok(String::from_utf16_lossy(&units))
}

fn coded_type_error(c: &mut Context<'_>, message: &str, code: &'static str) -> Result<RootedError, RootedError> {
    let error = c.type_error_rooted(message)?;
    super::node_errors_shared_vm::decorate_type_error(c, error, code)?;
    Ok(c.throw(error))
}

/// `ERR_INVALID_URL` with the input and, when given, the base.
pub(crate) fn invalid_url(c: &mut Context<'_>, input: &str, base: Option<&str>) -> Result<RootedError, RootedError> {
    let error = c.type_error_rooted("Invalid URL")?;
    super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_URL")?;
    let input = c.string_rooted(input);
    c.assign_named_property_rooted(error, "input", input)?;
    if let Some(base) = base {
        let base = c.string_rooted(base);
        c.assign_named_property_rooted(error, "base", base)?;
    }
    Ok(c.throw(error))
}

fn missing_args(c: &mut Context<'_>, names: &[&str]) -> Result<RootedError, RootedError> {
    let quoted = names.iter().map(|name| format!("\"{name}\"")).collect::<Vec<_>>();
    let (list, verb) = match quoted.as_slice() {
        [one] => (one.clone(), "argument"),
        [first, second] => (format!("{first} and {second}"), "arguments"),
        _ => (quoted.join(", "), "arguments"),
    };
    coded_type_error(c, &format!("The {list} {verb} must be specified"), "ERR_MISSING_ARGS")
}

/// The URL `input` names against `base`; `None` when either does not parse.
fn parse(input: &str, base: Option<&str>) -> Result<Url, bool> {
    let base = match base {
        Some(base) => Some(Url::parse(base).map_err(|_| true)?),
        None => None,
    };
    Url::options().base_url(base.as_ref()).parse(input).map_err(|_| false)
}

/// The classes, made once: `(URL, URLSearchParams)`.
pub(crate) fn classes(c: &mut Context<'_>) -> Result<(RootId, RootId), RootedError> {
    if let (Some(url), Some(params)) = (cached(c, URL_CLASS), cached(c, PARAMS_CLASS)) {
        return Ok((url, params));
    }
    c.with_startup_intrinsics(build_classes)
}

fn accessor(c: &mut Context<'_>, target: RootId, name: &str, getter: RootId, setter: Option<RootId>) -> Result<(), RootedError> {
    let descriptor = c.null_object_rooted()?;
    c.assign_named_property_rooted(descriptor, "get", getter)?;
    if let Some(setter) = setter {
        c.assign_named_property_rooted(descriptor, "set", setter)?;
    }
    let yes = c.boolean(true);
    c.assign_named_property_rooted(descriptor, "enumerable", yes)?;
    c.assign_named_property_rooted(descriptor, "configurable", yes)?;
    let key = c.string_rooted(name);
    c.define_property_descriptor_rooted(target, key, descriptor)?;
    Ok(())
}

fn method(c: &mut Context<'_>, target: RootId, key: RootId, name: &str, operation_name: &str, length: usize, enumerable: bool) -> Result<RootId, RootedError> {
    let function = c.host_function_named(operation(operation_name), name)?;
    let length_key = c.string_rooted("length");
    let length = c.number(length as f64);
    c.define_data_property_rooted(function, length_key, length, false, false, true)?;
    c.define_data_property_rooted(target, key, function, true, enumerable, true)?;
    Ok(function)
}

fn named_method(c: &mut Context<'_>, target: RootId, name: &str, operation_name: &str, length: usize) -> Result<RootId, RootedError> {
    let key = c.string_rooted(name);
    method(c, target, key, name, operation_name, length, true)
}

fn tag(c: &mut Context<'_>, target: RootId, value: &str) -> Result<(), RootedError> {
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let key = get(c, symbol, "toStringTag")?;
    let value = c.string_rooted(value);
    c.define_data_property_rooted(target, key, value, false, false, true)?;
    Ok(())
}

fn custom_inspect(c: &mut Context<'_>, target: RootId, operation_name: &str) -> Result<(), RootedError> {
    let primordials = super::util_shared_vm::primordials(c)?;
    let key = get(c, primordials, "custom")?;
    let function = c.host_function_named(operation(operation_name), "[nodejs.util.inspect.custom]")?;
    c.define_data_property_rooted(target, key, function, true, false, true)?;
    Ok(())
}

fn build_classes(c: &mut Context<'_>) -> Result<(RootId, RootId), RootedError> {
    let url = c.host_constructor_named(operation("urlConstruct"), "URL")?;
    let key = c.string_rooted("length");
    let one = c.number(1.0);
    c.define_data_property_rooted(url, key, one, false, false, true)?;
    let prototype = get(c, url, "prototype")?;
    named_method(c, prototype, "toString", "urlToString", 0)?;
    for (index, &(name, _, settable)) in COMPONENTS.iter().enumerate() {
        let data = c.number(index as f64);
        let getter = c.host_function_named_with_data(operation("urlGet"), &format!("get {name}"), data)?;
        let setter = if settable {
            Some(c.host_function_named_with_data(operation("urlSet"), &format!("set {name}"), data)?)
        } else {
            None
        };
        accessor(c, prototype, name, getter, setter)?;
    }
    named_method(c, prototype, "toJSON", "urlToString", 0)?;
    tag(c, prototype, "URL")?;
    custom_inspect(c, prototype, "urlInspect")?;
    for (name, operation_name) in [("parse", "urlParseStatic"), ("canParse", "urlCanParse")] {
        let function = c.host_function_named(operation(operation_name), name)?;
        let key = c.string_rooted("length");
        let one = c.number(1.0);
        c.define_data_property_rooted(function, key, one, false, false, true)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(url, key, function, true, true, true)?;
    }
    let params = c.host_constructor_named(operation("urlParamsConstruct"), "URLSearchParams")?;
    let key = c.string_rooted("length");
    let zero = c.number(0.0);
    c.define_data_property_rooted(params, key, zero, false, false, true)?;
    let params_prototype = get(c, params, "prototype")?;
    let size = c.host_function_named(operation("urlParamsSize"), "get size")?;
    accessor(c, params_prototype, "size", size, None)?;
    let mut entries = None;
    for &(name, operation_name, length) in PARAMS_METHODS {
        let function = named_method(c, params_prototype, name, operation_name, length)?;
        if name == "entries" {
            entries = Some(function);
        }
    }
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let iterator = get(c, symbol, "iterator")?;
    if let Some(entries) = entries {
        c.define_data_property_rooted(params_prototype, iterator, entries, true, false, true)?;
    }
    tag(c, params_prototype, "URLSearchParams")?;
    custom_inspect(c, params_prototype, "urlParamsInspect")?;
    let iterator_prototype = c.object_rooted()?;
    let array = c.array_rooted(&[])?;
    let values = get(c, array, "values")?;
    let array_iterator = c.call_rooted(values, array, &[])?;
    let array_iterator_prototype = c.prototype_rooted(array_iterator)?;
    let base = c.prototype_rooted(array_iterator_prototype)?;
    c.set_prototype_rooted(iterator_prototype, base)?;
    named_method(c, iterator_prototype, "next", "urlParamsIteratorNext", 0)?;
    tag(c, iterator_prototype, "URLSearchParams Iterator")?;
    custom_inspect(c, iterator_prototype, "urlParamsIteratorInspect")?;
    cache(c, ITERATOR_PROTOTYPE, iterator_prototype)?;
    let url = cache(c, URL_CLASS, url)?;
    let params = cache(c, PARAMS_CLASS, params)?;
    Ok((url, params))
}

/// A new URL object for `url`, as `new URL(href)` makes it.
pub(crate) fn url_object(c: &mut Context<'_>, url: &Url) -> Result<RootId, RootedError> {
    let (class, _) = classes(c)?;
    let prototype = get(c, class, "prototype")?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    let href = c.string_rooted(url.as_str());
    write_slot(c, object, HREF_SLOT, href)?;
    Ok(object)
}

/// `new URL(input, base)`.
pub(crate) fn construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if c.new_target_rooted().is_none() {
        let error = c.type_error_rooted("Class constructor URL cannot be invoked without 'new'")?;
        return Err(c.throw(error));
    }
    if args.is_empty() {
        return Err(missing_args(c, &["url"])?);
    }
    let input = usv_string(c, args[0])?;
    let base = match args.get(1).copied().filter(|base| type_of(c, *base) != ValueType::Undefined) {
        Some(base) => Some(usv_string(c, base)?),
        None => None,
    };
    let url = match parse(&input, base.as_deref()) {
        Ok(url) => url,
        Err(_) => return Err(invalid_url(c, &input, base.as_deref())?),
    };
    let href = c.string_rooted(url.as_str());
    write_slot(c, this, HREF_SLOT, href)?;
    Ok(this)
}

/// `URL.parse(input, base)`: the URL, or null.
pub(crate) fn parse_static(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if args.is_empty() {
        return Err(missing_args(c, &["url"])?);
    }
    let input = usv_string(c, args[0])?;
    let base = match args.get(1).copied().filter(|base| type_of(c, *base) != ValueType::Undefined) {
        Some(base) => Some(usv_string(c, base)?),
        None => None,
    };
    match parse(&input, base.as_deref()) {
        Ok(url) => url_object(c, &url),
        Err(_) => Ok(c.null()),
    }
}

/// `URL.canParse(input, base)`.
pub(crate) fn can_parse(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if args.is_empty() {
        return Err(missing_args(c, &["url"])?);
    }
    let input = usv_string(c, args[0])?;
    let base = match args.get(1).copied().filter(|base| type_of(c, *base) != ValueType::Undefined) {
        Some(base) => Some(usv_string(c, base)?),
        None => None,
    };
    let parsed = parse(&input, base.as_deref()).is_ok();
    Ok(c.boolean(parsed))
}

/// The parsed URL of a URL object; the receiver check of Node's private fields.
fn this_url(c: &mut Context<'_>, this: RootId) -> Result<Url, RootedError> {
    let Some(href) = read_slot(c, this, HREF_SLOT)? else {
        let error = c.type_error_rooted("Receiver must be an instance of class URL")?;
        return Err(c.throw(error));
    };
    let href = c.to_string(href)?;
    Url::parse(&href).map_err(|_| RootedError::host("a URL's serialization reparses"))
}

/// Stores a URL's new serialization and refreshes its search params from its query.
fn update(c: &mut Context<'_>, this: RootId, url: &Url) -> Result<(), RootedError> {
    let href = c.string_rooted(url.as_str());
    write_slot(c, this, HREF_SLOT, href)?;
    if let Some(params) = read_slot(c, this, PARAMS_SLOT)? {
        let pairs = url.query().map(parse_pairs).unwrap_or_default();
        store_pairs(c, params, &pairs)?;
    }
    Ok(())
}

pub(crate) fn to_string(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let url = this_url(c, this)?;
    Ok(c.string_rooted(url.as_str()))
}

fn component_of(c: &mut Context<'_>) -> Result<Component, RootedError> {
    let data = c.host_function_data()?;
    let index = c.to_number(data)? as usize;
    COMPONENTS.get(index).map(|entry| entry.1).ok_or_else(|| RootedError::host("URL component index out of range"))
}

fn component_text(url: &Url, component: Component) -> String {
    use url::quirks;
    match component {
        Component::Href => url.as_str().to_owned(),
        Component::Origin => url.origin().ascii_serialization(),
        Component::Protocol => quirks::protocol(url).to_owned(),
        Component::Username => quirks::username(url).to_owned(),
        Component::Password => quirks::password(url).to_owned(),
        Component::Host => quirks::host(url).to_owned(),
        Component::Hostname => quirks::hostname(url).to_owned(),
        Component::Port => quirks::port(url).to_owned(),
        Component::Pathname => quirks::pathname(url).to_owned(),
        Component::Search => quirks::search(url).to_owned(),
        Component::Hash => quirks::hash(url).to_owned(),
        Component::SearchParams => String::new(),
    }
}

/// A URL accessor's getter.
pub(crate) fn get_component(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let component = component_of(c)?;
    let url = this_url(c, this)?;
    if component == Component::SearchParams {
        return search_params(c, this, &url);
    }
    Ok(c.string_rooted(&component_text(&url, component)))
}

/// `url.searchParams`: one object per URL, made on first read.
fn search_params(c: &mut Context<'_>, this: RootId, url: &Url) -> Result<RootId, RootedError> {
    if let Some(params) = read_slot(c, this, PARAMS_SLOT)? {
        return Ok(params);
    }
    let (_, class) = classes(c)?;
    let prototype = get(c, class, "prototype")?;
    let params = c.object_rooted()?;
    c.set_prototype_rooted(params, prototype)?;
    let pairs = url.query().map(parse_pairs).unwrap_or_default();
    store_pairs(c, params, &pairs)?;
    write_slot(c, params, OWNER_SLOT, this)?;
    write_slot(c, this, PARAMS_SLOT, params)?;
    Ok(params)
}

/// A URL accessor's setter: the WHATWG setter, ignoring a rejected value; `href` throws.
pub(crate) fn set_component(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    use url::quirks;
    let component = component_of(c)?;
    let mut url = this_url(c, this)?;
    let value = argument(c, args, 0);
    let value = usv_string(c, value)?;
    let applied = match component {
        Component::Href => match Url::parse(&value) {
            Ok(parsed) => {
                url = parsed;
                true
            }
            Err(_) => return Err(invalid_url(c, &value, None)?),
        },
        Component::Protocol => quirks::set_protocol(&mut url, &value).is_ok(),
        Component::Username => quirks::set_username(&mut url, &value).is_ok(),
        Component::Password => quirks::set_password(&mut url, &value).is_ok(),
        Component::Host => quirks::set_host(&mut url, &value).is_ok(),
        Component::Hostname => quirks::set_hostname(&mut url, &value).is_ok(),
        Component::Port => quirks::set_port(&mut url, &value).is_ok(),
        Component::Pathname => {
            quirks::set_pathname(&mut url, &value);
            true
        }
        Component::Search => {
            quirks::set_search(&mut url, &value);
            true
        }
        Component::Hash => {
            quirks::set_hash(&mut url, &value);
            true
        }
        Component::Origin | Component::SearchParams => false,
    };
    if applied {
        update(c, this, &url)?;
    }
    Ok(c.undefined())
}

/// `url[util.inspect.custom](depth, opts)`.
pub(crate) fn inspect(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let depth = argument(c, args, 0);
    if c.rooted_value(depth).and_then(|value| value.as_number()).is_some_and(|depth| depth < 0.0) {
        return Ok(this);
    }
    let options = argument(c, args, 1);
    let url = this_url(c, this)?;
    let object = c.object_rooted()?;
    for &(name, component, _) in COMPONENTS {
        let value = if component == Component::SearchParams {
            search_params(c, this, &url)?
        } else {
            c.string_rooted(&component_text(&url, component))
        };
        c.assign_named_property_rooted(object, name, value)?;
    }
    let inspect = super::util_shared_vm::internal_inspect(c)?;
    let undefined = c.undefined();
    let text = c.call_rooted(inspect, undefined, &[object, options])?;
    let name = constructor_name(c, this, "URL")?;
    let text = c.to_string(text)?;
    Ok(c.string_rooted(&format!("{name} {text}")))
}

fn constructor_name(c: &mut Context<'_>, this: RootId, fallback: &str) -> Result<String, RootedError> {
    let constructor = get(c, this, "constructor")?;
    if !c.is_callable_rooted(constructor)? {
        return Ok(fallback.to_owned());
    }
    let name = get(c, constructor, "name")?;
    c.to_string(name)
}

/// `application/x-www-form-urlencoded` parsing.
fn parse_pairs(query: &str) -> Pairs {
    url::form_urlencoded::parse(query.as_bytes()).map(|(name, value)| (name.into_owned(), value.into_owned())).collect()
}

fn serialize_pairs(pairs: &Pairs) -> String {
    url::form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs.iter()).finish()
}

fn store_pairs(c: &mut Context<'_>, params: RootId, pairs: &Pairs) -> Result<(), RootedError> {
    let mut values = Vec::with_capacity(pairs.len() * 2);
    for (name, value) in pairs {
        values.push(c.string_rooted(name));
        values.push(c.string_rooted(value));
    }
    let list = c.array_rooted(&values)?;
    write_slot(c, params, LIST_SLOT, list)
}

fn load_pairs(c: &mut Context<'_>, list: RootId) -> Result<Pairs, RootedError> {
    let values = c.array_like_values_rooted(list)?;
    let mut pairs = Vec::with_capacity(values.len() / 2);
    for pair in values.chunks_exact(2) {
        pairs.push((c.to_string(pair[0])?, c.to_string(pair[1])?));
    }
    Ok(pairs)
}

/// The receiver's pairs; the receiver check of Node's private fields.
fn this_pairs(c: &mut Context<'_>, this: RootId) -> Result<Pairs, RootedError> {
    match read_slot(c, this, LIST_SLOT)? {
        Some(list) => load_pairs(c, list),
        None => Err(coded_type_error(c, "Value of \"this\" must be of type URLSearchParams", "ERR_INVALID_THIS")?),
    }
}

/// Stores the pairs and, for a URL's params, its query (`#updateURL`).
fn commit(c: &mut Context<'_>, this: RootId, pairs: &Pairs) -> Result<(), RootedError> {
    store_pairs(c, this, pairs)?;
    if let Some(owner) = read_slot(c, this, OWNER_SLOT)? {
        let mut url = this_url(c, owner)?;
        let query = serialize_pairs(pairs);
        url.set_query((!query.is_empty()).then_some(query.as_str()));
        let href = c.string_rooted(url.as_str());
        write_slot(c, owner, HREF_SLOT, href)?;
    }
    Ok(())
}

fn invalid_tuple(c: &mut Context<'_>) -> Result<RootedError, RootedError> {
    coded_type_error(c, "Each query pair must be an iterable [name, value] tuple", "ERR_INVALID_TUPLE")
}

/// `new URLSearchParams(init)`.
pub(crate) fn construct_params(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    if c.new_target_rooted().is_none() {
        let error = c.type_error_rooted("Class constructor URLSearchParams cannot be invoked without 'new'")?;
        return Err(c.throw(error));
    }
    let init = argument(c, args, 0);
    let pairs = match type_of(c, init) {
        ValueType::Undefined | ValueType::Null => Pairs::new(),
        ValueType::Object | ValueType::Function => pairs_from_object(c, init)?,
        _ => {
            let text = usv_string(c, init)?;
            parse_pairs(text.strip_prefix('?').unwrap_or(&text))
        }
    };
    store_pairs(c, this, &pairs)?;
    Ok(this)
}

fn pairs_from_object(c: &mut Context<'_>, init: RootId) -> Result<Pairs, RootedError> {
    if let Some(list) = read_slot(c, init, LIST_SLOT)? {
        return load_pairs(c, list);
    }
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let iterator_key = get(c, symbol, "iterator")?;
    let method = c.get_property_rooted(init, iterator_key)?;
    let mut pairs = Pairs::new();
    if !matches!(type_of(c, method), ValueType::Undefined | ValueType::Null) {
        if !c.is_callable_rooted(method)? {
            return Err(coded_type_error(c, "Query pairs must be iterable", "ERR_ARG_NOT_ITERABLE")?);
        }
        let iterator = c.iterator_rooted(init)?;
        while let Some(pair) = c.iterator_step_rooted(&iterator)? {
            let elements = match pair_elements(c, pair, iterator_key) {
                Ok(elements) => elements,
                Err(error) => {
                    let _ = c.iterator_close_rooted(&iterator);
                    return Err(error);
                }
            };
            let [name, value] = elements.as_slice() else {
                let _ = c.iterator_close_rooted(&iterator);
                return Err(invalid_tuple(c)?);
            };
            pairs.push((name.clone(), value.clone()));
        }
        return Ok(pairs);
    }
    let mut positions: Vec<(String, usize)> = Vec::new();
    for key in c.own_keys_rooted(init)? {
        let Some(fields) = c.own_property_fields_rooted(init, key)? else { continue };
        if fields.enumerable != Some(true) {
            continue;
        }
        let name = usv_string(c, key)?;
        let value = c.get_property_rooted(init, key)?;
        let value = usv_string(c, value)?;
        match positions.iter().find(|(existing, _)| *existing == name) {
            Some(&(_, position)) => pairs[position].1 = value,
            None => {
                positions.push((name.clone(), pairs.len()));
                pairs.push((name, value));
            }
        }
    }
    Ok(pairs)
}

/// One query pair's strings: an array's elements or an iterable's values.
fn pair_elements(c: &mut Context<'_>, pair: RootId, iterator_key: RootId) -> Result<Vec<String>, RootedError> {
    if matches!(type_of(c, pair), ValueType::Undefined | ValueType::Null) {
        return Err(invalid_tuple(c)?);
    }
    if c.is_array_rooted(pair)? {
        let length = get(c, pair, "length")?;
        if c.to_number(length)? != 2.0 {
            return Err(invalid_tuple(c)?);
        }
        let name = get(c, pair, "0")?;
        let value = get(c, pair, "1")?;
        return Ok(vec![usv_string(c, name)?, usv_string(c, value)?]);
    }
    let iterable = c.is_object_rooted(pair)? && {
        let method = c.get_property_rooted(pair, iterator_key)?;
        c.is_callable_rooted(method)?
    };
    if !iterable {
        return Err(invalid_tuple(c)?);
    }
    let iterator = c.iterator_rooted(pair)?;
    let mut elements = Vec::new();
    while let Some(element) = c.iterator_step_rooted(&iterator)? {
        elements.push(usv_string(c, element)?);
    }
    Ok(elements)
}

/// The arguments a params method requires, as strings; `ERR_MISSING_ARGS` when absent.
fn required(c: &mut Context<'_>, this: RootId, args: &[RootId], count: usize) -> Result<(Pairs, Vec<String>), RootedError> {
    let pairs = this_pairs(c, this)?;
    if args.len() < count {
        let names = ["name", "value"];
        return Err(missing_args(c, &names[..count])?);
    }
    let mut strings = Vec::with_capacity(count);
    for &value in &args[..count] {
        strings.push(usv_string(c, value)?);
    }
    Ok((pairs, strings))
}

/// The optional `value` of `delete` and `has`.
fn optional_value(c: &mut Context<'_>, args: &[RootId]) -> Result<Option<String>, RootedError> {
    match args.get(1).copied().filter(|value| type_of(c, *value) != ValueType::Undefined) {
        Some(value) => Ok(Some(usv_string(c, value)?)),
        None => Ok(None),
    }
}

pub(crate) fn append(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (mut pairs, strings) = required(c, this, args, 2)?;
    pairs.push((strings[0].clone(), strings[1].clone()));
    commit(c, this, &pairs)?;
    Ok(c.undefined())
}

pub(crate) fn delete(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (mut pairs, strings) = required(c, this, args, 1)?;
    let value = optional_value(c, args)?;
    pairs.retain(|(name, current)| !(*name == strings[0] && value.as_ref().is_none_or(|value| value == current)));
    commit(c, this, &pairs)?;
    Ok(c.undefined())
}

pub(crate) fn get_value(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (pairs, strings) = required(c, this, args, 1)?;
    match pairs.iter().find(|(name, _)| *name == strings[0]) {
        Some((_, value)) => Ok(c.string_rooted(value)),
        None => Ok(c.null()),
    }
}

pub(crate) fn get_all(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (pairs, strings) = required(c, this, args, 1)?;
    let values = pairs.iter().filter(|(name, _)| *name == strings[0]).map(|(_, value)| c.string_rooted(value)).collect::<Vec<_>>();
    c.array_rooted(&values)
}

pub(crate) fn has(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (pairs, strings) = required(c, this, args, 1)?;
    let value = optional_value(c, args)?;
    let found = pairs.iter().any(|(name, current)| *name == strings[0] && value.as_ref().is_none_or(|value| value == current));
    Ok(c.boolean(found))
}

pub(crate) fn set(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let (mut pairs, strings) = required(c, this, args, 2)?;
    let (name, value) = (&strings[0], &strings[1]);
    match pairs.iter().position(|(current, _)| current == name) {
        Some(first) => {
            pairs[first].1 = value.clone();
            let mut index = 0;
            pairs.retain(|(current, _)| {
                index += 1;
                index - 1 == first || current != name
            });
        }
        None => pairs.push((name.clone(), value.clone())),
    }
    commit(c, this, &pairs)?;
    Ok(c.undefined())
}

/// `params.sort()`: a stable sort by name in UTF-16 code unit order.
pub(crate) fn sort(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let mut pairs = this_pairs(c, this)?;
    pairs.sort_by(|left, right| left.0.encode_utf16().cmp(right.0.encode_utf16()));
    commit(c, this, &pairs)?;
    Ok(c.undefined())
}

pub(crate) fn params_to_string(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let pairs = this_pairs(c, this)?;
    Ok(c.string_rooted(&serialize_pairs(&pairs)))
}

pub(crate) fn size(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let pairs = this_pairs(c, this)?;
    Ok(c.number(pairs.len() as f64))
}

/// `params.forEach(callback, thisArg)`, reading the live list after each call.
pub(crate) fn for_each(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    this_pairs(c, this)?;
    let callback = argument(c, args, 0);
    if !c.is_callable_rooted(callback)? {
        return Err(super::util_shared_vm::argument_type_error(c, callback, "callback", "function")?);
    }
    let receiver = argument(c, args, 1);
    let mut index = 0;
    loop {
        let pairs = this_pairs(c, this)?;
        let Some((name, value)) = pairs.get(index) else { break };
        let name = c.string_rooted(name);
        let value = c.string_rooted(value);
        c.call_rooted(callback, receiver, &[value, name, this])?;
        index += 1;
    }
    Ok(c.undefined())
}

fn iterator(c: &mut Context<'_>, this: RootId, kind: Kind) -> Result<RootId, RootedError> {
    this_pairs(c, this)?;
    classes(c)?;
    let prototype = cached(c, ITERATOR_PROTOTYPE).ok_or_else(|| RootedError::host("URLSearchParams Iterator prototype missing"))?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    let kind = c.number(kind as u8 as f64);
    let index = c.number(0.0);
    let state = c.array_rooted(&[this, kind, index])?;
    write_slot(c, object, ITERATOR_SLOT, state)?;
    Ok(object)
}

pub(crate) fn entries(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    iterator(c, this, Kind::Entries)
}

pub(crate) fn keys(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    iterator(c, this, Kind::Keys)
}

pub(crate) fn values(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    iterator(c, this, Kind::Values)
}

/// An iterator's (params, kind, position), the receiver check of its `next`.
fn iterator_state(c: &mut Context<'_>, this: RootId) -> Result<(RootId, Kind, usize, RootId), RootedError> {
    let Some(state) = read_slot(c, this, ITERATOR_SLOT)? else {
        return Err(coded_type_error(c, "Value of \"this\" must be of type URLSearchParamsIterator", "ERR_INVALID_THIS")?);
    };
    let params = get(c, state, "0")?;
    let kind = get(c, state, "1")?;
    let kind = match c.to_number(kind)? as u8 {
        0 => Kind::Keys,
        1 => Kind::Values,
        _ => Kind::Entries,
    };
    let index = get(c, state, "2")?;
    let index = c.to_number(index)? as usize;
    Ok((params, kind, index, state))
}

fn yielded(c: &mut Context<'_>, kind: Kind, name: &str, value: &str) -> Result<RootId, RootedError> {
    match kind {
        Kind::Keys => Ok(c.string_rooted(name)),
        Kind::Values => Ok(c.string_rooted(value)),
        Kind::Entries => {
            let name = c.string_rooted(name);
            let value = c.string_rooted(value);
            c.array_rooted(&[name, value])
        }
    }
}

pub(crate) fn iterator_next(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let (params, kind, index, state) = iterator_state(c, this)?;
    let pairs = this_pairs(c, params)?;
    let result = c.object_rooted()?;
    match pairs.get(index) {
        Some((name, value)) => {
            let value = yielded(c, kind, name, value)?;
            c.assign_named_property_rooted(result, "value", value)?;
            let no = c.boolean(false);
            c.assign_named_property_rooted(result, "done", no)?;
            let next = c.number((index + 1) as f64);
            c.assign_named_property_rooted(state, "2", next)?;
        }
        None => {
            let undefined = c.undefined();
            c.assign_named_property_rooted(result, "value", undefined)?;
            let yes = c.boolean(true);
            c.assign_named_property_rooted(result, "done", yes)?;
        }
    }
    Ok(result)
}

/// The inner options a params inspector passes on: `{ ...ctx, depth: depth - 1 }`.
fn inner_options(c: &mut Context<'_>, depth: RootId, options: RootId) -> Result<RootId, RootedError> {
    let inner = c.object_rooted()?;
    if c.is_object_rooted(options)? {
        for key in c.enumerable_own_keys_rooted(options, true, true)? {
            let value = c.get_property_rooted(options, key)?;
            c.set_property_rooted(inner, key, value, inner)?;
        }
    }
    if type_of(c, depth) != ValueType::Null {
        let depth = c.to_number(depth)?;
        let depth = c.number(depth - 1.0);
        c.assign_named_property_rooted(inner, "depth", depth)?;
    }
    Ok(inner)
}

fn inspect_with(c: &mut Context<'_>, value: RootId, options: RootId) -> Result<String, RootedError> {
    let inspect = super::util_shared_vm::internal_inspect(c)?;
    let undefined = c.undefined();
    let text = c.call_rooted(inspect, undefined, &[value, options])?;
    c.to_string(text)
}

/// `params[util.inspect.custom](depth, ctx)`.
pub(crate) fn params_inspect(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    const SEPARATOR: &str = ", ";
    let depth = argument(c, args, 0);
    let options = argument(c, args, 1);
    if c.rooted_value(depth).and_then(|value| value.as_number()).is_some_and(|depth| depth < 0.0) {
        let stylize = get(c, options, "stylize")?;
        let text = c.string_rooted("[Object]");
        let style = c.string_rooted("special");
        return c.call_rooted(stylize, options, &[text, style]);
    }
    let pairs = this_pairs(c, this)?;
    let inner = inner_options(c, depth, options)?;
    let mut output = Vec::with_capacity(pairs.len());
    for (name, value) in &pairs {
        let name = c.string_rooted(name);
        let value = c.string_rooted(value);
        output.push(format!("{} => {}", inspect_with(c, name, inner)?, inspect_with(c, value, inner)?));
    }
    let length = output
        .iter()
        .map(|entry| super::util_inspect_shared_vm::js_length(&super::util_strip::strip_vt_control_characters(entry)) + SEPARATOR.len())
        .sum::<usize>() as f64
        - SEPARATOR.len() as f64;
    let break_length = get(c, options, "breakLength")?;
    let break_length = c.to_number(break_length)?;
    let name = constructor_name(c, this, "URLSearchParams")?;
    let text = if length > break_length {
        format!("{name} {{\n  {} }}", output.join(",\n  "))
    } else if !output.is_empty() {
        format!("{name} {{ {} }}", output.join(SEPARATOR))
    } else {
        format!("{name} {{}}")
    };
    Ok(c.string_rooted(&text))
}

/// `iterator[util.inspect.custom](depth, ctx)`: the entries still ahead.
pub(crate) fn iterator_inspect(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let depth = argument(c, args, 0);
    let options = argument(c, args, 1);
    let (params, kind, index, _) = iterator_state(c, this)?;
    let pairs = this_pairs(c, params)?;
    let inner = inner_options(c, depth, options)?;
    let mut items = Vec::new();
    for (name, value) in pairs.iter().skip(index) {
        items.push(yielded(c, kind, name, value)?);
    }
    let all = c.array_rooted(&items)?;
    let multiline = inspect_with(c, all, inner)?.contains('\n');
    let mut parts = Vec::with_capacity(items.len());
    for item in items {
        parts.push(inspect_with(c, item, inner)?);
    }
    let body = if multiline { format!("\n  {}", parts.join(",\n  ")) } else { format!(" {}", parts.join(", ")) };
    Ok(c.string_rooted(&format!("URLSearchParams Iterator {{{body} }}")))
}
