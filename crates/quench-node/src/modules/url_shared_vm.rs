//! Shared-VM URL constructors and Node's legacy URL parser.

use crate::host::NodeHost;
use crate::modules::url_legacy::{self, LegacyUrlField, LegacyUrlParseError};
use quench_runtime::{NativeContext, RootId, RootedError, ValueType};
use std::path::{Component, Path, PathBuf};

const URL_CONSTRUCTOR_SOURCE: &str = "(class Url {})";

/// Pinned Node URL-like recognition; observable properties are intentional.
pub(crate) fn is_url_like(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<bool, RootedError> {
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(false);
    }
    for name in ["href", "protocol"] {
        let property = get(context, value, name)?;
        if !context.truthy_rooted(property)? {
            return Ok(false);
        }
    }
    for name in ["auth", "path"] {
        let property = get(context, value, name)?;
        if !context
            .rooted_value(property)
            .is_some_and(|value| value.is_undefined())
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn to_path_if_file_url(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let undefined = context.undefined();
    let is_url_instance = is_url_instance(context, undefined, &[value])?;
    if is_url_like(context, value)? || context.truthy_rooted(is_url_instance)? {
        let undefined = context.undefined();
        return file_url_to_path(context, undefined, &[value]);
    }
    Ok(value)
}
const WHATWG_URL_FACTORY: &str = quench_js_check::checked_js!(
    r#"(parse, URLSearchParams, state) => {
  const fields = [
    "href", "origin", "protocol", "username", "password", "host",
    "hostname", "port", "pathname", "search", "hash",
  ];
  const data = (receiver) => {
    const value = state.get(receiver);
    if (value === undefined) {
      const error = new TypeError('Receiver must be an instance of class URL');
      error.code = 'ERR_INVALID_THIS';
      throw error;
    }
    return value;
  };
  const setterData = (receiver) => {
    const value = state.get(receiver);
    if (value === undefined) throw new TypeError('Cannot read private member #context from an object whose class did not declare it');
    return value;
  };
  const toUrlString = (value) => {
    if (typeof value === 'symbol') throw new TypeError('Cannot convert a Symbol value to a string');
    return String(value);
  };
  const encodeUserInfo = (value) => encodeURIComponent(String(value).toWellFormed())
    .replace(/[!'()*]/g, (char) => `%${char.charCodeAt(0).toString(16).toUpperCase()}`);
  const serialize = (value) => {
    const auth = value.username || value.password
      ? `${value.username}${value.password ? `:${value.password}` : ""}@`
      : "";
    return `${value.protocol}${value.host ? `//${auth}${value.host}` : ""}${value.pathname}${value.search}${value.hash}`;
  };
  const syncSearchParams = (receiver, value) => {
    const current = data(receiver);
    current.search = value;
    current.href = serialize(current);
  };
  class URL {
    constructor(input, base) {
      if (arguments.length === 0) {
        const error = new TypeError('The "url" argument must be specified');
        error.code = 'ERR_MISSING_ARGS';
        throw error;
      }
      const value = parse(toUrlString(input), base === undefined ? undefined : toUrlString(base));
      if (!value) throw new TypeError("Invalid URL");
      state.set(this, value);
    }
    toString() { return data(this).href; }
    toJSON() { return data(this).href; }
    static createObjectURL(value) {
      if (typeof globalThis.Blob !== "function" || !(value instanceof globalThis.Blob)) {
        const error = new TypeError('The "obj" argument must be an instance of Blob');
        error.code = "ERR_INVALID_ARG_TYPE";
        throw error;
      }
      if (!globalThis.__nodeBlobUrls) {
        Object.defineProperty(globalThis, "__nodeBlobUrls", {
          value: new Map(), configurable: true,
        });
      }
      const id = `blob:nodedata:${Math.random().toString(16).slice(2)}`;
      globalThis.__nodeBlobUrls.set(id, value);
      return id;
    }
    static revokeObjectURL(value) {
      if (value === undefined) {
        const error = new TypeError('The "url" argument must be specified');
        error.code = "ERR_MISSING_ARGS";
        throw error;
      }
      globalThis.__nodeBlobUrls?.delete(value);
    }
    static parse(input, base) {
      if (arguments.length === 0) return null;
      try { return new URL(input, base); } catch { return null; }
    }
    static canParse(input, base) {
      if (arguments.length === 0) {
        throw Object.assign(new TypeError("The \"input\" argument must be specified"), {
          code: "ERR_MISSING_ARGS",
        });
      }
      try {
        parse(toUrlString(input), base === undefined ? undefined : toUrlString(base));
        return true;
      } catch (_) {
        return false;
      }
    }
    get searchParams() {
      const value = setterData(this);
      if (!value._searchParams) {
        const params = new URLSearchParams(value.search,
          () => syncSearchParams(this, params.toString() ? `?${params.toString()}` : ""));
        value._searchParams = params;
      }
      return value._searchParams;
    }
  }
  for (const field of fields) {
    Object.defineProperty(URL.prototype, field, {
      configurable: true,
      enumerable: true,
      get() { return data(this)[field]; },
      set(value) {
        const current = setterData(this);
        if (field === "href") {
          const updated = parse(toUrlString(value), undefined);
          if (!updated) throw new TypeError("Invalid URL");
          if (current._searchParams) {
            const params = new URLSearchParams(updated.search);
            current._searchParams._pairs = params._pairs;
          }
          Object.assign(current, updated);
          return;
        }
        current[field] = toUrlString(value);
        if (field === 'protocol' && current.protocol && !current.protocol.endsWith(':')) current.protocol += ':';
        if (field === 'search' && current.search && !current.search.startsWith('?')) current.search = `?${current.search}`;
        if (field === 'hash' && current.hash && !current.hash.startsWith('#')) current.hash = `#${current.hash}`;
        if (field === 'username' || field === 'password') {
          current[field] = encodeUserInfo(value);
        }
        if (field === 'hostname') {
          current.host = current.port ? `${current.hostname}:${current.port}` : current.hostname;
        } else if (field === 'port') {
          current.host = current.port ? `${current.hostname}:${current.port}` : current.hostname;
        } else if (field === 'host') {
          const portIndex = current.host.lastIndexOf(':');
          const ipv6End = current.host.indexOf(']');
          if (portIndex > ipv6End && portIndex >= 0) {
            current.hostname = current.host.slice(0, portIndex);
            current.port = current.host.slice(portIndex + 1);
          } else {
            current.hostname = current.host;
            current.port = '';
          }
        }
        if (field === "search") {
          current.search = current.search && current.search !== "?" ? current.search : "";
          if (current._searchParams) {
            current._searchParams._pairs = new URLSearchParams(current.search)._pairs;
          }
        }
        if (field === "pathname" && current.pathname === "") current.pathname = "/";
        current.href = serialize(current);
        const reparsed = parse(current.href);
        if (reparsed) Object.assign(current, reparsed);
        if (field === "searchParams") throw new TypeError("Cannot set property searchParams of [object URL] which has only a getter");
      },
    });
  }
  // Web IDL exposes these members as enumerable prototype properties. Class
  // syntax makes methods non-enumerable, so restore the observable ordering.
  const ordered = ['toString', 'href', 'origin', 'protocol', 'username', 'password',
    'host', 'hostname', 'port', 'pathname', 'search', 'searchParams', 'hash', 'toJSON'];
  const descriptors = new Map(ordered.map((name) => [name, Object.getOwnPropertyDescriptor(URL.prototype, name)]));
  for (const name of ordered) delete URL.prototype[name];
  for (const name of ordered) {
    const descriptor = descriptors.get(name);
    descriptor.enumerable = true;
      if (name === 'origin' || name === 'searchParams') delete descriptor.set;
    if (!['href', 'search', 'searchParams'].includes(name) && descriptor.get) {
      descriptor.get = ({ [`get ${name}`]() { return setterData(this)[name]; } })[`get ${name}`];
      Object.defineProperty(descriptor.get, 'name', { value: `get ${name}` });
    }
    if (descriptor.get && ['href', 'search', 'searchParams'].includes(name)) {
      Object.defineProperty(descriptor.get, 'name', { value: `get ${name}` });
    }
    if (descriptor.set) Object.defineProperty(descriptor.set, 'name', { value: `set ${name}` });
    Object.defineProperty(URL.prototype, name, descriptor);
  }
  Object.defineProperty(URL.prototype, Symbol.for('nodejs.util.inspect.custom'), {
    configurable: true,
    value(depth, options) {
      const value = data(this);
      const constructorName = this.constructor?.name || 'URL';
      if (depth < 0) return `${constructorName} {}`;
      const names = ['href', 'origin', 'protocol', 'username', 'password', 'host', 'hostname', 'port', 'pathname', 'search'];
      const lines = names.map((name) => `  ${name}: '${value[name]}'`);
      const params = this.searchParams;
      const entries = [...params].map(([name, item]) => `'${name}' => '${item}'`).join(', ');
      lines.push(`  searchParams: URLSearchParams { ${entries} }`);
      lines.push(`  hash: '${value.hash}'`);
      if (options?.showHidden) {
        const href = value.href;
        const protocolEnd = href.indexOf(':');
        const authorityStart = href.indexOf('//') + 2;
        const at = href.indexOf('@', authorityStart);
        const hostStart = at === -1 ? authorityStart : at + 1;
        const slash = href.indexOf('/', hostStart);
        const hostEnd = slash === -1 ? href.length : slash;
        const port = value.port ? Number(value.port) : -1;
        const actualHostEnd = hostEnd - (port === -1 ? 0 : value.port.length + 1);
        lines.push(`  Symbol(context): URLContext {\n    href: '${href}',\n    protocol_end: ${protocolEnd + 1},\n    username_end: ${at === -1 ? -1 : authorityStart + value.username.length},\n    host_start: ${hostStart - 1},\n    host_end: ${actualHostEnd},\n    pathname_start: ${hostEnd},\n    search_start: ${href.indexOf('?')},\n    hash_start: ${href.indexOf('#')},\n    port: ${port},\n    scheme_type: ${value.protocol === 'https:' ? 2 : 0},\n    [hasPort]: [Getter],\n    [hasSearch]: [Getter],\n    [hasHash]: [Getter]\n  }`);
      }
      return `${constructorName} {\n${lines.join(',\n')}\n}`;
    },
  });
  Object.defineProperty(URL.prototype[Symbol.for('nodejs.util.inspect.custom')], 'name', {
    value: '[nodejs.util.inspect.custom]',
  });
  Object.defineProperty(URL.prototype, Symbol.toStringTag, { value: "URL" });
  for (const name of ['createObjectURL', 'revokeObjectURL']) {
    const descriptor = Object.getOwnPropertyDescriptor(URL, name);
    descriptor.enumerable = true;
    Object.defineProperty(URL, name, descriptor);
  }
  return URL;
}"#
);

const URL_SEARCH_PARAMS_FACTORY: &str = quench_js_check::checked_js!(
    r#"() => {
  const usv = (value) => {
    if (typeof value === 'symbol') throw new TypeError('Cannot convert a Symbol value to a string');
    return String(value).toWellFormed();
  };
  const encodeUserInfo = (value) => encodeURIComponent(usv(value))
    .replace(/[!'()*]/g, (char) => `%${char.charCodeAt(0).toString(16).toUpperCase()}`);
  const decode = (value) => {
    const safe = value.replace(/\+/g, " ").replace(/%(?![0-9a-fA-F]{2})/g, '%25');
    try { return decodeURIComponent(safe); }
    catch { return value.replace(/\+/g, " "); }
  };
  const encode = (value) => encodeURIComponent(usv(value))
    .replace(/[!'()~]/g, (char) => `%${char.charCodeAt(0).toString(16).toUpperCase()}`)
    .replace(/%20/g, "+");
  const entriesFrom = (init) => {
    if (init === undefined) return [];
    if (typeof init === "string") {
      const query = init.startsWith("?") ? init.slice(1) : init;
      if (!query) return [];
      return query.split("&").filter(Boolean).map((part) => {
        const index = part.indexOf("=");
        return [usv(decode(index < 0 ? part : part.slice(0, index))),
          usv(decode(index < 0 ? "" : part.slice(index + 1)))];
      });
    }
    if (init === null || (typeof init !== 'object' && typeof init !== 'function')) {
      return entriesFrom(usv(init));
    }
    if (init !== null && typeof init[Symbol.iterator] === "function") {
      const entries = [];
      for (const pair of init) {
        if (pair === null || typeof pair[Symbol.iterator] !== "function") {
          const error = new TypeError('Each query pair must be an iterable [name, value] tuple');
          error.code = 'ERR_INVALID_TUPLE';
          throw error;
        }
        const values = Array.from(pair);
        if (values.length !== 2) {
          const error = new TypeError('Each query pair must be an iterable [name, value] tuple');
          error.code = 'ERR_INVALID_TUPLE';
          throw error;
        }
        entries.push([usv(values[0]), usv(values[1])]);
      }
      return entries;
    }
    if (typeof init === "object") {
      if (Object.hasOwn(init, Symbol.iterator)) {
        const error = new TypeError('Query pairs must be iterable');
        error.code = 'ERR_ARG_NOT_ITERABLE';
        throw error;
      }
      if (Object.getOwnPropertySymbols(init).length !== 0) throw new TypeError('Cannot convert a Symbol value to a string');
      const entries = [];
      for (const key of Object.keys(init)) {
        const name = usv(key), value = usv(init[key]);
        const index = entries.findIndex((entry) => entry[0] === name);
        if (index === -1) entries.push([name, value]);
        else entries[index][1] = value;
      }
      return entries;
    }
    throw new TypeError("Failed to construct 'URLSearchParams': parameter 1 is not of type 'object'.");
  };
  const paramsState = new WeakMap();
  const state = (receiver) => {
    const value = paramsState.get(receiver);
    if (value === undefined) {
      const error = new TypeError('Value of "this" must be of type URLSearchParams');
      error.code = 'ERR_INVALID_THIS';
      throw error;
    }
    return value;
  };
  const pairs = (receiver) => state(receiver).pairs;
  const update = (receiver) => { const callback = state(receiver).onchange; if (callback) callback(); };
  class URLSearchParams {
    constructor(init) { paramsState.set(this, { pairs: entriesFrom(init), onchange: arguments[1] }); }
    append(name, value) {
      const list = pairs(this);
      if (arguments.length < 2) {
        const error = new TypeError('The "name" and "value" arguments must be specified');
        error.code = 'ERR_MISSING_ARGS';
        throw error;
      }
      list.push([usv(name), usv(value)]); update(this);
    }
    delete(name, value) {
      const list = pairs(this);
      if (arguments.length < 1) {
        const error = new TypeError('The "name" argument must be specified'); error.code = 'ERR_MISSING_ARGS'; throw error;
      }
      name = usv(name);
      const matchValue = arguments.length > 1;
      const expected = matchValue ? usv(value) : undefined;
      for (let index = list.length - 1; index >= 0; index--) {
        if (list[index][0] === name && (!matchValue || list[index][1] === expected)) list.splice(index, 1);
      }
      update(this);
    }
    get(name) {
      const list = pairs(this);
      if (arguments.length < 1) { const error = new TypeError('The "name" argument must be specified'); error.code = 'ERR_MISSING_ARGS'; throw error; }
      name = usv(name); const pair = list.find((entry) => entry[0] === name); return pair ? pair[1] : null;
    }
    getAll(name) {
      const list = pairs(this);
      if (arguments.length < 1) { const error = new TypeError('The "name" argument must be specified'); error.code = 'ERR_MISSING_ARGS'; throw error; }
      name = usv(name); return list.filter((entry) => entry[0] === name).map((entry) => entry[1]);
    }
    has(name, value) {
      const list = pairs(this);
      if (arguments.length < 1) { const error = new TypeError('The "name" argument must be specified'); error.code = 'ERR_MISSING_ARGS'; throw error; }
      name = usv(name);
      return list.some((entry) => entry[0] === name && (arguments.length < 2 || entry[1] === usv(value)));
    }
    set(name, value) {
      const list = pairs(this);
      if (arguments.length < 2) { const error = new TypeError('The "name" and "value" arguments must be specified'); error.code = 'ERR_MISSING_ARGS'; throw error; }
      name = usv(name); value = usv(value);
      const index = list.findIndex((entry) => entry[0] === name);
      if (index < 0) list.push([name, value]);
      else { list[index][1] = value; for (let i = list.length - 1; i > index; i--) if (list[i][0] === name) list.splice(i, 1); }
      update(this);
    }
    sort() { pairs(this).sort((a, b) => a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0); update(this); }
    forEach(callback, thisArg) {
      const list = pairs(this);
      if (typeof callback !== "function") {
        const error = new TypeError('The "callbackFn" argument must be of type function');
        error.code = 'ERR_INVALID_ARG_TYPE';
        throw error;
      }
      for (let index = 0; index < list.length; index++) callback.call(thisArg, list[index][1], list[index][0], this);
    }
    entries() { return new URLSearchParamsIterator(this, 'entry'); }
    keys() { return new URLSearchParamsIterator(this, 'key'); }
    values() { return new URLSearchParamsIterator(this, 'value'); }
    [Symbol.iterator]() { return this.entries(); }
    toString() { return pairs(this).map(([name, value]) => `${encode(name)}=${encode(value)}`).join("&"); }
    get size() { return pairs(this).length; }
    [Symbol.for('nodejs.util.inspect.custom')](depth, options) {
      const list = pairs(this);
      if (depth < 0) return '[Object]';
      if (list.length === 0) return 'URLSearchParams {}';
      const entries = list.map(([name, value]) => `'${name}' => '${value}'`);
      return options && options.breakLength <= 1
        ? `URLSearchParams {\n  ${entries.join(',\n  ')} }`
        : `URLSearchParams { ${entries.join(', ')} }`;
    }
  }
  const iteratorState = new WeakMap();
  const invalidIterator = () => {
    const error = new TypeError('Value of "this" must be of type URLSearchParamsIterator');
    error.code = 'ERR_INVALID_THIS';
    return error;
  };
  class URLSearchParamsIterator {
    constructor(params, kind) { iteratorState.set(this, { list: pairs(params), kind, index: 0 }); }
    next() {
      const state = iteratorState.get(this);
      if (state === undefined) throw invalidIterator();
      if (state.index >= state.list.length) return { value: undefined, done: true };
      const [name, value] = state.list[state.index++];
      return { value: state.kind === 'entry' ? [name, value] : state.kind === 'key' ? name : value, done: false };
    }
    [Symbol.iterator]() {
      if (!iteratorState.has(this)) throw invalidIterator();
      return this;
    }
    [Symbol.for('nodejs.util.inspect.custom')](depth, options) {
      const state = iteratorState.get(this);
      if (state === undefined) throw invalidIterator();
      if (depth < 0) return '[Object]';
      const remaining = state.list.slice(state.index).map(([name, value]) =>
        state.kind === 'entry' ? `[ '${name}', '${value}' ]` : `'${state.kind === 'key' ? name : value}'`);
      if (remaining.length === 0) return 'URLSearchParams Iterator {  }';
      return options && options.breakLength <= 1
        ? `URLSearchParams Iterator {\n  ${remaining.join(',\n  ')} }`
        : `URLSearchParams Iterator { ${remaining.join(', ')} }`;
    }
  }
  Object.defineProperty(URLSearchParamsIterator.prototype, Symbol.toStringTag, { value: 'URLSearchParams Iterator' });
  Object.defineProperty(URLSearchParamsIterator.prototype[Symbol.for('nodejs.util.inspect.custom')], 'name', {
    value: '[nodejs.util.inspect.custom]',
  });
  Object.defineProperty(URLSearchParams.prototype, Symbol.iterator, {
    configurable: true,
    writable: true,
    value: URLSearchParams.prototype.entries,
  });
  Object.defineProperty(URLSearchParams.prototype, "_pairs", {
    configurable: true,
    get() { return pairs(this); },
    set(value) { state(this).pairs = value; },
  });
  for (const name of ['append', 'delete', 'get', 'getAll', 'has', 'set', 'sort',
    'entries', 'forEach', 'keys', 'values', 'toString']) {
    const descriptor = Object.getOwnPropertyDescriptor(URLSearchParams.prototype, name);
    descriptor.enumerable = true;
    Object.defineProperty(URLSearchParams.prototype, name, descriptor);
  }
  const sizeDescriptor = Object.getOwnPropertyDescriptor(URLSearchParams.prototype, 'size');
  sizeDescriptor.enumerable = true;
  Object.defineProperty(URLSearchParams.prototype, 'size', sizeDescriptor);
  Object.defineProperty(URLSearchParams.prototype, Symbol.toStringTag, { value: "URLSearchParams", configurable: true });
  Object.defineProperty(URLSearchParams.prototype[Symbol.for('nodejs.util.inspect.custom')], 'name', {
    value: '[nodejs.util.inspect.custom]',
  });
  return URLSearchParams;
}"#
);

const WHATWG_PARSE_MARKER: &str = "\0quench:node:url:whatwg-parse";

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let constructor = url_constructor(context)?;
    let search_params = url_search_params_constructor(context)?;
    context.evaluate_script_rooted(
        crate::polyfills::post_bootstrap::module_surface_06::JS,
        "node:url/module-surface.js",
    )?;
    let global = context.global_root()?;
    let pattern_factory = get(context, global, "__quenchURLPatternFactory")?;
    let undefined = context.undefined();
    let url_pattern = context.call_rooted(pattern_factory, undefined, &[])?;
    let cleanup = context.evaluate_script_rooted(
        "() => { delete globalThis.__nodeThrowReadonlyURLSetter; delete globalThis.__quenchURLPatternFactory; delete globalThis.__quenchURLInstallCanParse; delete globalThis.__quenchURLInstallToString; }",
        "node:url/module-surface-cleanup.js",
    )?;
    let undefined = context.undefined();
    context.call_rooted(cleanup, undefined, &[])?;
    let legacy_constructor =
        context.evaluate_script_rooted(URL_CONSTRUCTOR_SOURCE, "node:url/Url")?;
    let module = context.object_rooted()?;
    let parse = context.host_function_with_data(
        crate::host::shared_vm::operation("urlParse"),
        legacy_constructor,
    )?;
    let warning_factory = context.evaluate_script_rooted(
        r#"((parse) => {
          let warned = false;
          return function urlParse(...args) {
            const stack = new Error().stack || '';
            if (!warned && !stack.split('\n').some((line) =>
              line.includes('/node_modules/') || line.includes('\\node_modules\\'))) {
              warned = true;
              process.emitWarning(
                '`url.parse()` behavior is not standardized and prone to ' +
                  'errors that have security implications. Use the WHATWG URL API ' +
                  'instead. CVEs are not issued for `url.parse()` vulnerabilities.',
                'DeprecationWarning',
                'DEP0169',
              );
            }
            return Reflect.apply(parse, this, args);
          };
        })"#,
        "node:url/legacy-parse-warning.js",
    )?;
    let undefined = context.undefined();
    let parse = context.call_rooted(warning_factory, undefined, &[parse])?;
    set(context, module, "parse", parse)?;
    let resolve = context.host_function(crate::host::shared_vm::operation("urlResolve"))?;
    set(context, module, "resolve", resolve)?;
    let resolve_object = context.host_function_with_data(
        crate::host::shared_vm::operation("urlResolveObject"),
        legacy_constructor,
    )?;
    set(context, module, "resolveObject", resolve_object)?;
    let legacy_prototype = get(context, legacy_constructor, "prototype")?;
    let resolve_object_method = context.host_function_with_data(
        crate::host::shared_vm::operation("urlResolveObject"),
        legacy_constructor,
    )?;
    set(
        context,
        legacy_prototype,
        "resolveObject",
        resolve_object_method,
    )?;
    set(context, module, "URL", constructor)?;
    set(context, module, "URLSearchParams", search_params)?;
    set(context, module, "URLPattern", url_pattern)?;
    set(context, module, "Url", legacy_constructor)?;
    let format = context.host_function(crate::host::shared_vm::operation("urlFormat"))?;
    set(context, module, "format", format)?;
    let domain_to_ascii =
        context.host_function(crate::host::shared_vm::operation("domainToASCII"))?;
    set(context, module, "domainToASCII", domain_to_ascii)?;
    let domain_to_unicode =
        context.host_function(crate::host::shared_vm::operation("domainToUnicode"))?;
    set(context, module, "domainToUnicode", domain_to_unicode)?;
    let url_to_http_options =
        context.host_function(crate::host::shared_vm::operation("urlToHttpOptions"))?;
    set(context, module, "urlToHttpOptions", url_to_http_options)?;
    let path_to_file_url =
        context.host_function(crate::host::shared_vm::operation("pathToFileURL"))?;
    set(context, module, "pathToFileURL", path_to_file_url)?;
    let file_url_to_path =
        context.host_function(crate::host::shared_vm::operation("fileURLToPath"))?;
    set(context, module, "fileURLToPath", file_url_to_path)?;
    let file_url_to_path_buffer =
        context.host_function(crate::host::shared_vm::operation("fileURLToPathBuffer"))?;
    set(
        context,
        module,
        "fileURLToPathBuffer",
        file_url_to_path_buffer,
    )?;
    Ok(module)
}

pub(crate) fn domain_to_ascii(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = match args.first().copied() {
        Some(input) => context.to_string(input)?,
        None => "undefined".to_owned(),
    };
    let output = url::Host::parse(&input)
        .map(|host| host.to_string())
        .unwrap_or_default();
    Ok(context.string_rooted(&output))
}

pub(crate) fn domain_to_unicode(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = match args.first().copied() {
        Some(input) => context.to_string(input)?,
        None => "undefined".to_owned(),
    };
    let output = match url::Host::parse(&input) {
        Ok(url::Host::Domain(domain)) => {
            let (unicode, result) = idna::domain_to_unicode(&domain);
            if result.is_ok() {
                unicode
            } else {
                String::new()
            }
        }
        Ok(host) => host.to_string(),
        Err(_) => String::new(),
    };
    Ok(context.string_rooted(&output))
}

pub(crate) fn url_to_http_options(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(input) = args.first().copied() else {
        return Err(invalid_url_to_http_options_argument(context, None));
    };
    if !context.is_object_rooted(input)? {
        return Err(invalid_url_to_http_options_argument(context, Some(input)));
    }
    let href = url_argument_text(context, input)?;
    let is_url = href.is_some();
    let result = context.null_object_rooted()?;
    for name in ["protocol", "hostname", "hash", "search", "pathname"] {
        let mut value = get(context, input, name)?;
        if name == "hostname" {
            if let Some(hostname) = context.string_text(value)? {
                if hostname.starts_with('[') && hostname.ends_with(']') {
                    value = context.string_rooted(&hostname[1..hostname.len() - 1]);
                }
            }
        }
        set(context, result, name, value)?;
    }
    let pathname = get(context, input, "pathname")?;
    let search = get(context, input, "search")?;
    let pathname = nullish_string(context, pathname)?;
    let search = nullish_string(context, search)?;
    let path = format!("{pathname}{search}");
    let path = context.string_rooted(&path);
    set(context, result, "path", path)?;
    let href = href
        .map(|href| context.string_rooted(&href))
        .unwrap_or(context.undefined());
    set(context, result, "href", href)?;

    let port = get(context, input, "port")?;
    let port_text = context.string_text(port)?;
    if let Some(port_text) = port_text.filter(|port| !port.is_empty()) {
        let number = port_text.parse::<f64>().unwrap_or(f64::NAN);
        let number = context.number(number);
        set(context, result, "port", number)?;
    } else if !is_url {
        let number = context.number(f64::NAN);
        set(context, result, "port", number)?;
    }

    let username_value = get(context, input, "username")?;
    let password_value = get(context, input, "password")?;
    let username = nullish_string(context, username_value)?;
    let password = nullish_string(context, password_value)?;
    if !username.is_empty() || !password.is_empty() {
        let username = percent_decode(&username).unwrap_or(username);
        let password = percent_decode(&password).unwrap_or(password);
        let auth = context.string_rooted(&format!("{username}:{password}"));
        set(context, result, "auth", auth)?;
    }
    Ok(result)
}

fn nullish_string(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<String, RootedError> {
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        Ok(String::new())
    } else {
        context.to_string(value)
    }
}

fn invalid_url_to_http_options_argument(
    context: &mut NativeContext<'_, NodeHost>,
    input: Option<RootId>,
) -> RootedError {
    let received = match input.and_then(|input| {
        Some((input, context.value_type_rooted(input)?))
    }) {
        None | Some((_, ValueType::Undefined)) => "Received undefined".to_owned(),
        Some((_, ValueType::Null)) => "Received null".to_owned(),
        Some((input, ValueType::String)) => context
            .to_string(input)
            .map(|value| format!("Received type string ('{value}')"))
            .unwrap_or_else(|_| "Received type string".to_owned()),
        Some((input, kind @ (ValueType::Number | ValueType::Boolean))) => context
            .to_string(input)
            .map(|value| format!("Received type {kind:?} ({value})").to_ascii_lowercase())
            .unwrap_or_else(|_| format!("Received type {:?}", kind).to_ascii_lowercase()),
        Some((_, kind)) => format!("Received type {:?}", kind).to_ascii_lowercase(),
    };
    coded_url_type_error(
        context,
        "ERR_INVALID_ARG_TYPE",
        &format!("The \"url\" argument must be of type object. {received}"),
    )
}

pub(crate) fn path_to_file_url(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args.first().copied().unwrap_or_else(|| context.undefined());
    let windows = windows_option(context, args)?;
    let namespace = super::path_shared_vm::module(context)?;
    let flavor = get(context, namespace, if windows { "win32" } else { "posix" })?;
    let resolve = get(context, flavor, "resolve")?;
    let resolved = context.call_rooted(resolve, flavor, &[input])?;
    let resolved = context.to_string(resolved)?;
    let original = context
        .string_text(input)?
        .ok_or_else(|| invalid_path_argument(context))?;
    if windows && original.starts_with("\\\\") {
        let hostname = original[2..]
            .split(|character| character == '\\' || character == '/')
            .next()
            .unwrap_or_default();
        if hostname
            .chars()
            .any(|character| matches!(character, ' ' | '@' | ':' | '[' | ']'))
        {
            return Err(coded_url_type_error(
                context,
                "ERR_INVALID_URL",
                "Invalid file URL host",
            ));
        }
    }
    let trailing_separator = original.ends_with('/') || (windows && original.ends_with('\\'));
    let (windows_path, control_markers) = if windows {
        protect_windows_path_controls(&resolved)
    } else {
        (resolved.clone(), Vec::new())
    };
    let mut href = if windows {
        windows_unc_terminator_to_url(&original).or_else(|| windows_path_to_url(&windows_path))
    } else {
        url::Url::from_file_path(&resolved).ok()
    }
    .ok_or_else(|| invalid_path_argument(context))?
    .to_string();
    for (marker, escaped) in control_markers {
        href = href.replace(&marker, &escaped);
    }
    href = escape_file_url_path_unsafe_chars(&href);
    if trailing_separator && !href.ends_with('/') {
        href.push('/');
    }
    let href = context.string_rooted(&href);
    let constructor = url_constructor(context)?;
    context.construct_rooted(constructor, constructor, &[href])
}

fn escape_file_url_path_unsafe_chars(href: &str) -> String {
    let path_start = href
        .get("file://".len()..)
        .and_then(|authority_and_path| authority_and_path.find('/'))
        .map_or(href.len(), |offset| "file://".len() + offset);
    let (authority, path) = href.split_at(path_start);
    let encoded_path = path
        .chars()
        .map(|character| match character {
            '"' => "%22".to_owned(),
            '<' => "%3C".to_owned(),
            '>' => "%3E".to_owned(),
            '[' => "%5B".to_owned(),
            '\\' => "%5C".to_owned(),
            ']' => "%5D".to_owned(),
            '^' => "%5E".to_owned(),
            '`' => "%60".to_owned(),
            '{' => "%7B".to_owned(),
            '|' => "%7C".to_owned(),
            '}' => "%7D".to_owned(),
            '~' => "%7E".to_owned(),
            _ => character.to_string(),
        })
        .collect::<String>();
    if encoded_path != path {
        format!("{authority}{encoded_path}")
    } else {
        href.to_owned()
    }
}

fn protect_windows_path_controls(path: &str) -> (String, Vec<(String, String)>) {
    let mut protected = String::with_capacity(path.len());
    let mut markers = Vec::new();
    let unc_host_end = (path.starts_with("\\\\") || path.starts_with("//"))
        .then(|| {
            path[2..]
                .find(|character| matches!(character, '\\' | '/'))
                .map(|offset| offset + 2)
                .unwrap_or(path.len())
        })
        .unwrap_or(0);
    for (offset, character) in path.char_indices() {
        // The WHATWG host parser ignores ASCII tab, CR, and LF in a UNC
        // hostname. Drop those here before protecting control characters in
        // the path, otherwise they become literal marker text in the host.
        if offset < unc_host_end && matches!(character, '\t' | '\r' | '\n') {
            continue;
        }
        if !character.is_control() {
            protected.push(character);
            continue;
        }
        let mut index = markers.len();
        let marker = loop {
            let marker = format!("__QUENCH_CTRL_{index:X}__");
            if !path.contains(&marker) {
                break marker;
            }
            index += 1;
        };
        let escaped = character
            .to_string()
            .as_bytes()
            .iter()
            .map(|byte| format!("%{byte:02X}"))
            .collect::<String>();
        protected.push_str(&marker);
        markers.push((marker, escaped));
    }
    (protected, markers)
}

struct FileUrlPath {
    url: RootId,
    hostname: RootId,
    pathname: RootId,
    windows: bool,
}

fn validated_file_url_path(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
) -> Result<FileUrlPath, RootedError> {
    let windows = windows_option(context, args)?;
    let input = args.first().copied().unwrap_or_else(|| context.undefined());
    let path = if context.string_text(input)?.is_some() {
        context.with_startup_intrinsics(|context| {
            let global = context.global_root()?;
            let constructor = get(context, global, "URL")?;
            context.construct_rooted(constructor, constructor, &[input])
        })?
    } else if is_url_like(context, input)? {
        input
    } else {
        return Err(super::util_shared_vm::argument_type_error_with_expected(
            context,
            input,
            "path",
            "of type string or an instance of URL",
        )?);
    };
    let protocol = get(context, path, "protocol")?;
    if context.string_text(protocol)?.as_deref() != Some("file:") {
        return Err(coded_url_type_error(
            context,
            "ERR_INVALID_URL_SCHEME",
            "The URL must be of scheme file",
        ));
    }
    let hostname = get(context, path, "hostname")?;
    let local = context.string_text(hostname)?.as_deref() == Some("");
    if !windows && !local {
        let platform = match std::env::consts::OS {
            "windows" => "win32",
            "macos" => "darwin",
            platform => platform,
        };
        return Err(invalid_file_url_host(context, platform));
    }
    let pathname = get(context, path, "pathname")?;
    Ok(FileUrlPath {
        url: path,
        hostname,
        pathname,
        windows,
    })
}

/// Node observes URL-like properties rather than serializing through `toString`.
pub(crate) fn file_url_to_path(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let FileUrlPath {
        url: path,
        hostname,
        mut pathname,
        windows,
    } = validated_file_url_path(context, args)?;
    let local = context.string_text(hostname)?.as_deref() == Some("");
    // The length and indexed reads deliberately remain observable for URL-like objects.
    let mut index = 0usize;
    loop {
        let length = get(context, pathname, "length")?;
        let position = context.number(index as f64);
        if !context.greater_than_rooted(length, position)? {
            break;
        }
        let character = get(context, pathname, &index.to_string())?;
        if context.string_text(character)?.as_deref() == Some("%") {
            let third_index = context.number((index + 2) as f64);
            let third = intrinsic_string_method(context, "codePointAt", pathname, &[third_index])?;
            let third = context.to_number(third)? as u32 | 0x20;
            let second = get(context, pathname, &(index + 1).to_string())?;
            let second = context.string_text(second)?;
            let slash = second.as_deref() == Some("2") && third == u32::from(b'f');
            let backslash = if windows && !slash {
                let second = get(context, pathname, &(index + 1).to_string())?;
                context.string_text(second)?.as_deref() == Some("5") && third == u32::from(b'c')
            } else {
                false
            };
            if slash || backslash {
                let message = if windows {
                    "File URL path must not include encoded \\ or / characters"
                } else {
                    "File URL path must not include encoded / characters"
                };
                return Err(invalid_file_url_path(context, path, message));
            }
        }
        index += 1;
    }
    if windows {
        pathname = windows_path_separators(context, pathname)?;
    }
    let percent = context.string_rooted("%");
    let encoded = intrinsic_string_method(context, "includes", pathname, &[percent])?;
    if context.truthy_rooted(encoded)? {
        let decode = context.with_startup_intrinsics(|context| {
            let global = context.global_root()?;
            get(context, global, "decodeURIComponent")
        })?;
        let undefined = context.undefined();
        pathname = context.call_rooted(decode, undefined, &[pathname])?;
    }
    if !windows {
        return Ok(pathname);
    }
    if !local {
        let undefined = context.undefined();
        let domain = domain_to_unicode(context, undefined, &[hostname])?;
        let domain = context.to_string(domain)?;
        let suffix = context.to_string_code_units(pathname)?;
        let mut units = format!("\\\\{domain}").encode_utf16().collect::<Vec<_>>();
        units.extend(suffix);
        return Ok(context.string_units_rooted(&units));
    }
    let units = context.to_string_code_units(pathname)?;
    if !units.get(1).is_some_and(|unit| {
        (*unit >= u16::from(b'A') && *unit <= u16::from(b'Z'))
            || (*unit >= u16::from(b'a') && *unit <= u16::from(b'z'))
    }) || units.get(2) != Some(&u16::from(b':'))
    {
        return Err(invalid_file_url_path(
            context,
            path,
            "File URL path must be absolute",
        ));
    }
    Ok(context.string_units_rooted(&units[1..]))
}

/// Capture the primordial method, then invoke it outside the capture scope so guest
/// conversions observe the application's globals and preserve thrown values.
fn intrinsic_string_method(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let method = context.with_startup_intrinsics(|context| {
        let global = context.global_root()?;
        let constructor = get(context, global, "String")?;
        let prototype = get(context, constructor, "prototype")?;
        get(context, prototype, name)
    })?;
    context.call_rooted(method, receiver, args)
}

fn invalid_file_url_path(
    context: &mut NativeContext<'_, NodeHost>,
    input: RootId,
    message: &str,
) -> RootedError {
    let exception = match context.type_error_rooted(message) {
        Ok(exception) => exception,
        Err(error) => return error,
    };
    if let Err(error) = super::node_errors_shared_vm::decorate_type_error_with_fields(
        context,
        exception,
        "ERR_INVALID_FILE_URL_PATH",
        &[("input", input)],
    ) {
        return error;
    }
    context.throw(exception)
}

/// The byte variant uses forgiving percent decoding, including encoded separators.
pub(crate) fn file_url_to_path_buffer(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let FileUrlPath {
        url,
        hostname,
        mut pathname,
        windows,
    } = validated_file_url_path(context, args)?;
    if windows {
        pathname = windows_path_separators(context, pathname)?;
    }
    let utf8 = context.string_rooted("utf8");
    let input = file_path_buffer_from(context, &[pathname, utf8])?;
    // Execute the unmodified configured Node library as guest policy on Quench.
    let percent_decode = context.with_startup_intrinsics(|context| {
        crate::host::shared_vm::commonjs::node_library_export(
            context,
            "internal/url.js",
            "internal/data_url.js",
            "percentDecode",
        )
    })?;
    let undefined = context.undefined();
    let decoded = context.call_rooted(percent_decode, undefined, &[input])?;
    let mut parts = Vec::new();
    for name in ["buffer", "byteOffset", "byteLength"] {
        let getter = context.with_startup_intrinsics(|context| {
            let global = context.global_root()?;
            let constructor = get(context, global, "Uint8Array")?;
            let prototype = get(context, constructor, "prototype")?;
            let prototype = context.prototype_rooted(prototype)?;
            let key = context.string_rooted(name);
            let descriptor = context.own_property_descriptor_rooted(prototype, key)?;
            get(context, descriptor, "get")
        })?;
        parts.push(context.call_rooted(getter, decoded, &[])?);
    }
    let decoded = file_path_buffer_from(context, &parts)?;
    if !windows {
        return Ok(decoded);
    }
    if context.string_text(hostname)?.as_deref() != Some("") {
        let prefix = context.string_rooted("\\\\");
        let ascii = context.string_rooted("ascii");
        let prefix = file_path_buffer_from(context, &[prefix, ascii])?;
        let domain = domain_to_unicode(context, undefined, &[hostname])?;
        let domain = file_path_buffer_from(context, &[domain, utf8])?;
        let values = context.array_rooted(&[prefix, domain, decoded])?;
        let buffer = file_path_buffer_constructor(context)?;
        let concat = get(context, buffer, "concat")?;
        return context.call_rooted(concat, buffer, &[values]);
    }
    let letter = get(context, decoded, "1")?;
    let lowercase = context.number(f64::from(b' '));
    let letter = context.bitwise_or_rooted(letter, lowercase)?;
    let letter = context.to_number(letter)?;
    let separator = get(context, decoded, "2")?;
    let colon = context.number(f64::from(b':'));
    if !(f64::from(b'a')..=f64::from(b'z')).contains(&letter)
        || !context.strict_equal_rooted(separator, colon)?
    {
        return Err(invalid_file_url_path(
            context,
            url,
            "File URL path must be absolute",
        ));
    }
    let subarray = get(context, decoded, "subarray")?;
    let one = context.number(1.0);
    context.call_rooted(subarray, decoded, &[one])
}

fn file_path_buffer_constructor(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    let module = crate::host::shared_vm::commonjs::buffer_module(context)?;
    get(context, module, "Buffer")
}

fn file_path_buffer_from(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let buffer = file_path_buffer_constructor(context)?;
    let from = get(context, buffer, "from")?;
    context.call_rooted(from, buffer, args)
}

fn windows_path_separators(
    context: &mut NativeContext<'_, NodeHost>,
    pathname: RootId,
) -> Result<RootId, RootedError> {
    let mut units = context.to_string_code_units(pathname)?;
    for unit in &mut units {
        if *unit == u16::from(b'/') {
            *unit = u16::from(b'\\');
        }
    }
    Ok(context.string_units_rooted(&units))
}

fn invalid_file_url_host(context: &mut NativeContext<'_, NodeHost>, platform: &str) -> RootedError {
    let message = format!("File URL host must be \"localhost\" or empty on {platform}");
    let exception = match context.type_error_rooted(&message) {
        Ok(exception) => exception,
        Err(error) => return error,
    };
    if let Err(error) = super::node_errors_shared_vm::decorate_formatted_type_error(
        context,
        exception,
        "ERR_INVALID_FILE_URL_HOST",
    ) {
        return error;
    }
    context.throw(exception)
}

fn windows_option(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
) -> Result<bool, RootedError> {
    let Some(options) = args.get(1).copied() else {
        return Ok(cfg!(windows));
    };
    if context
        .rooted_value(options)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(cfg!(windows));
    }
    let key = context.string_rooted("windows");
    let windows = context.get_property_rooted(options, key)?;
    if context
        .rooted_value(windows)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        Ok(cfg!(windows))
    } else {
        context.truthy_rooted(windows)
    }
}

fn windows_path_to_url(path: &str) -> Option<url::Url> {
    if let Some(unc) = path.strip_prefix("\\\\?\\UNC\\") {
        let path = format!("\\\\{unc}");
        return windows_path_to_url(&path);
    }
    let path = path.replace('\\', "/");
    if let Some(unc) = path.strip_prefix("//") {
        let (host, path) = unc.split_once('/')?;
        if host.is_empty() || path.is_empty() {
            return None;
        }
        let mut url = url::Url::parse(&format!("file://{host}/")).ok()?;
        push_url_segments(&mut url, path, path.ends_with('/'))?;
        return Some(url);
    }

    if path.len() >= 2 && path.as_bytes()[0].is_ascii_alphabetic() && path.as_bytes()[1] == b':' {
        let drive = &path[..2];
        let mut rest = path[2..].to_owned();
        if !rest.starts_with('/') {
            let cwd = std::env::current_dir()
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            rest = format!("/{}/{}", cwd.trim_start_matches('/'), rest);
        }
        let mut url = url::Url::parse("file:///").ok()?;
        {
            let mut segments = url.path_segments_mut().ok()?;
            segments.push(drive);
            for segment in rest.trim_start_matches('/').split('/') {
                if !segment.is_empty() {
                    segments.push(segment);
                }
            }
            if rest.ends_with('/') {
                segments.push("");
            }
        }
        return Some(url);
    }

    let path = if path.starts_with('/') {
        PathBuf::from(path)
    } else {
        let cwd = std::env::current_dir().ok()?;
        cwd.join(path)
    };
    url::Url::from_file_path(normalize_path(&path)).ok()
}

fn windows_unc_terminator_to_url(path: &str) -> Option<url::Url> {
    if path.starts_with("\\\\?\\UNC\\") {
        return None;
    }
    let input = path.strip_prefix("\\\\")?;
    let host_end = input.find(|character| matches!(character, '\\' | '/' | '#' | '?'))?;
    let host = &input[..host_end];
    let marker = &input[host_end..];
    if !marker
        .chars()
        .any(|character| matches!(character, '#' | '?' | '/'))
    {
        return None;
    }
    let suffix = marker
        .split_once('\\')
        .map(|(_, suffix)| suffix)
        .or_else(|| marker.split_once('/').map(|(_, suffix)| suffix))
        .unwrap_or_default();
    let normalized = suffix.replace('\\', "/");
    let mut url = url::Url::parse(&format!("file://{host}/")).ok()?;
    push_url_segments(
        &mut url,
        &normalized,
        suffix
            .chars()
            .last()
            .is_some_and(|character| character == '\\' || character == '/'),
    )?;
    Some(url)
}

fn push_url_segments(url: &mut url::Url, path: &str, trailing_slash: bool) -> Option<()> {
    let mut segments = url.path_segments_mut().ok()?;
    for segment in path.split('/') {
        if !segment.is_empty() {
            segments.push(segment);
        }
    }
    if trailing_slash {
        segments.push("");
    }
    Some(())
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex_value(*bytes.get(index + 1)?)?;
            let low = hex_value(*bytes.get(index + 2)?)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

pub(crate) fn url_argument_text(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<Option<String>, RootedError> {
    if let Some(text) = context.string_text(value)? {
        return Ok(Some(text));
    }
    if !context.is_object_rooted(value)? {
        return Ok(None);
    }
    let global = context.global_root()?;
    let constructor = get(context, global, "URL")?;
    let prototype = get(context, constructor, "prototype")?;
    let to_string = get(context, prototype, "toString")?;
    match context.call_rooted(to_string, value, &[]) {
        Ok(url) => context.string_text(url),
        Err(error) => {
            if let Some(exception) = error.exception {
                context.release_root(exception);
            }
            Ok(None)
        }
    }
}

fn invalid_path_argument(context: &mut NativeContext<'_, NodeHost>) -> RootedError {
    coded_url_type_error(
        context,
        "ERR_INVALID_ARG_TYPE",
        "The \"path\" argument must be of type string",
    )
}

fn coded_url_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    code: &'static str,
    message: &str,
) -> RootedError {
    let exception = match context.type_error_rooted(message) {
        Ok(exception) => exception,
        Err(error) => return error,
    };
    if let Err(error) = super::node_errors_shared_vm::decorate_type_error(context, exception, code)
    {
        return error;
    }
    context.throw(exception)
}

/// Install the realm's one URL constructor as the global; `node:url` reads the
/// same rooted constructor from host state.
/// Installs `URL` and `URLSearchParams` as lazy data properties, built on first use.
pub(crate) fn install_global(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let initializer = context.host_function(crate::host::shared_vm::operation("urlInitialize"))?;
    let global = context.global_root()?;
    context.define_lazy_data_properties_rooted(
        global,
        &["URL", "URLSearchParams"],
        false,
        initializer,
    )?;
    context.release_root(initializer);
    Ok(())
}

pub(crate) fn initialize(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let constructor = url_constructor(context)?;
    let search_params = url_search_params_constructor(context)?;
    let install = context.evaluate_script_rooted(
        "(url, params) => { Object.defineProperty(globalThis, 'URL', { value: url, writable: true, configurable: true }); Object.defineProperty(globalThis, 'URLSearchParams', { value: params, writable: true, configurable: true }); }",
        "node:url/install-global.js",
    )?;
    let undefined = context.undefined();
    context.call_rooted(install, undefined, &[constructor, search_params])?;
    Ok(undefined)
}

fn url_search_params_constructor(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    let roots = context.host_mut().shared_state();
    if let Some(constructor) = roots.borrow().url_search_params_constructor {
        return Ok(constructor);
    }
    let factory = context
        .evaluate_script_rooted(URL_SEARCH_PARAMS_FACTORY, "node:url/url-search-params.js")?;
    let undefined = context.undefined();
    let constructor = context.call_rooted(factory, undefined, &[])?;
    let retained = context.retain(constructor)?;
    roots.borrow_mut().url_search_params_constructor = Some(retained);
    Ok(constructor)
}

fn url_constructor(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let roots = context.host_mut().shared_state();
    if let Some(constructor) = roots.borrow().url_constructor {
        return Ok(constructor);
    }

    let parser_data = context.string_rooted(WHATWG_PARSE_MARKER);
    let parser = context
        .host_function_with_data(crate::host::shared_vm::operation("urlParse"), parser_data)?;
    let search_params = url_search_params_constructor(context)?;
    let factory = context.evaluate_script_rooted(WHATWG_URL_FACTORY, "node:url/whatwg.js")?;
    let undefined = context.undefined();
    let state = context.evaluate_script_rooted("new WeakMap()", "node:url/state")?;
    let constructor = context.call_rooted(factory, undefined, &[parser, search_params, state])?;
    let retained_state = context.retain(state)?;
    roots.borrow_mut().url_state = Some(retained_state);
    let retained = context.retain(constructor)?;
    roots.borrow_mut().url_constructor = Some(retained);
    Ok(constructor)
}

/// Share the structural URL contract with native file URL conversion.
pub(crate) fn is_url(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let matches = is_url_like(context, value)?;
    Ok(context.boolean(matches))
}

pub(crate) fn parse(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    if context.string_text(data)?.as_deref() == Some(WHATWG_PARSE_MARKER) {
        return parse_whatwg(context, args);
    }

    let input = args.first().copied().unwrap_or_else(|| context.undefined());
    let Some(input) = context.string_text(input)? else {
        return Err(super::util_shared_vm::argument_type_error(
            context, input, "url", "string",
        )?);
    };
    let parsed = url_legacy::parse_legacy_parts(&input).map_err(|error| match error {
        LegacyUrlParseError::Invalid { code, input } => coded_type_error(context, &code, &input),
        LegacyUrlParseError::MalformedUri => uri_error(context),
    })?;
    let query_requested = args
        .get(1)
        .copied()
        .map(|argument| context.truthy_rooted(argument))
        .transpose()?
        .unwrap_or(false);
    let query_entries = if query_requested && parsed.query_object {
        Some(query_object(
            context,
            parsed.query.as_deref().unwrap_or_default(),
        )?)
    } else {
        None
    };
    let constructor = context.host_function_data()?;
    let instance = context.construct_rooted(constructor, constructor, &[])?;
    for (name, value) in parsed.fields {
        let value = if name == "query" {
            query_entries.unwrap_or_else(|| legacy_value(context, value))
        } else {
            legacy_value(context, value)
        };
        set(context, instance, name, value)?;
    }
    Ok(instance)
}

pub(crate) fn resolve(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let base = args
        .first()
        .copied()
        .map(|root| context.to_string(root))
        .transpose()?
        .unwrap_or_default();
    let target = args
        .get(1)
        .copied()
        .map(|root| context.to_string(root))
        .transpose()?
        .unwrap_or_default();
    let resolved = resolve_legacy_url(&base, &target);
    Ok(context.string_rooted(&resolved))
}

pub(crate) fn resolve_object(
    context: &mut NativeContext<'_, NodeHost>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let (base, target, module_call) = if args.len() >= 2 {
        (
            context.to_string(args[0])?,
            context.to_string(args[1])?,
            true,
        )
    } else {
        let base = get(context, receiver, "href")?;
        let base = context.string_text(base)?.unwrap_or_default();
        let target = args
            .first()
            .copied()
            .map(|root| context.to_string(root))
            .transpose()?
            .unwrap_or_default();
        (base, target, false)
    };
    let resolved = resolve_legacy_url(&base, &target);
    if module_call && base.is_empty() {
        return Ok(context.string_rooted(&target));
    }
    let resolved = context.string_rooted(&resolved);
    parse(context, receiver, &[resolved])
}

fn resolve_legacy_url(base: &str, target: &str) -> String {
    if base.is_empty() {
        return target.to_owned();
    }
    if target.is_empty() {
        return base.to_owned();
    }
    if let Some((scheme, suffix)) = target.split_once(':') {
        if !scheme.is_empty()
            && scheme
                .bytes()
                .enumerate()
                .all(|(index, byte)| {
                    byte.is_ascii_alphabetic()
                        || (index > 0
                            && (byte.is_ascii_digit() || matches!(byte, b'+' | b'.' | b'-')))
                })
        {
            let same_scheme = base
                .split_once(':')
                .is_some_and(|(base_scheme, _)| base_scheme.eq_ignore_ascii_case(scheme));
            if ["http", "https", "ftp", "gopher", "file", "ws", "wss"]
                .iter()
                .any(|known| scheme.eq_ignore_ascii_case(known))
                && suffix.starts_with('/')
                && !suffix.starts_with("//")
            {
                if same_scheme {
                    if let Ok(base_url) = url::Url::parse(base) {
                        return base_url
                            .join(suffix)
                            .map(|url| url.to_string())
                            .unwrap_or_else(|_| target.to_owned());
                    }
                }
                return format!("{scheme}://{}", suffix.trim_start_matches('/'));
            }
            if suffix.starts_with('#') {
                if same_scheme {
                    if let Ok(base_url) = url::Url::parse(base) {
                        return base_url
                            .join(suffix)
                            .map(|url| url.to_string())
                            .unwrap_or_else(|_| target.to_owned());
                    }
                }
                return format!("{scheme}:///{suffix}");
            }
        }
    }
    if url::Url::parse(target).is_ok() {
        return target.to_owned();
    }
    if let Ok(base_url) = url::Url::parse(base) {
        if target.starts_with("//") {
            return format!("{}:{}", base_url.scheme(), target);
        }
        return base_url
            .join(target)
            .map(|url| url.to_string())
            .unwrap_or_else(|_| target.to_owned());
    }
    if target.starts_with('/') {
        if let Some((scheme, rest)) = base.split_once("://") {
            if let Some((authority, _)) = rest.split_once('/') {
                return format!("{scheme}://{authority}{target}");
            }
        }
        return normalize_relative_url_path(target);
    }
    let prefix = base.rsplit_once('/').map_or("", |(prefix, _)| prefix);
    let combined = if prefix.is_empty() && base.starts_with('/') {
        format!("/{target}")
    } else if prefix.is_empty() {
        target.to_owned()
    } else {
        format!("{prefix}/{target}")
    };
    normalize_relative_url_path(&combined)
}

fn normalize_relative_url_path(value: &str) -> String {
    let (path, suffix) = value
        .find(['?', '#'])
        .map_or((value, ""), |index| (&value[..index], &value[index..]));
    let rooted = path.starts_with('/');
    let mut segments = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.last().is_some_and(|segment| *segment != "..") {
                    segments.pop();
                } else if !rooted {
                    segments.push("..");
                }
            }
            segment => segments.push(segment),
        }
    }
    let mut normalized = segments.join("/");
    if rooted {
        normalized.insert(0, '/');
    }
    if (path.ends_with('/') || path.ends_with("/.") || path.ends_with("/.."))
        && !normalized.ends_with('/')
    {
        normalized.push('/');
    }
    normalized.push_str(suffix);
    normalized
}

pub(crate) fn format(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(input) = args.first().copied() else {
        return Err(invalid_format_argument(context));
    };
    if let Some(input) = context.string_text(input)? {
        let parts = match url_legacy::parse_legacy_parts(&input) {
            Ok(parts) => parts,
            Err(LegacyUrlParseError::Invalid { code, input }) => {
                return Err(coded_type_error(context, &code, &input));
            }
            Err(LegacyUrlParseError::MalformedUri) => return Err(uri_error(context)),
        };
        let mut fields = LegacyFormatFields::default();
        for (name, value) in parts.fields {
            match (name, value) {
                ("slashes", LegacyUrlField::Boolean(value)) => fields.slashes = value,
                (_, LegacyUrlField::Text(value)) => fields.set(name, value),
                _ => {}
            }
        }
        // A parsed authority is not enough to infer that the input contained
        // `//`; protocols such as CoAP also allow `scheme:user@host/path`.
        // Preserve the delimiter that was actually present in the string.
        fields.slashes = input.contains("://");
        if fields
            .hostname
            .as_deref()
            .is_some_and(|hostname| hostname.len() > 255)
        {
            fields.host = Some(String::new());
            fields.hostname = Some(String::new());
        }
        // Parsing a URL string preserves escapes in `auth` that are not
        // decoded by the legacy parser (for example `%2F`). Preserve those
        // escapes when formatting it again instead of encoding `%` as `%25`.
        fields.auth_encoded = fields
            .auth
            .as_deref()
            .filter(|auth| !auth.is_empty())
            .map(encode_auth_preserving_escapes);
        let formatted = format_legacy_fields(fields);
        return Ok(context.string_rooted(&formatted));
    }

    if !context.is_object_rooted(input)? {
        return Err(invalid_format_argument(context));
    }
    if let Some(href) = url_argument_text(context, input)? {
        let formatted = format_whatwg_url(context, &href, args.get(1).copied())?;
        return Ok(context.string_rooted(&formatted));
    }

    let mut fields = LegacyFormatFields::default();
    fields.auth = truthy_property_string(context, input, "auth")?;
    fields.auth_encoded = fields
        .auth
        .as_deref()
        .filter(|auth| !auth.is_empty())
        .map(encode_auth);
    fields.protocol = truthy_property_string(context, input, "protocol")?;
    fields.pathname = truthy_property_string(context, input, "pathname")?;
    fields.hash = truthy_property_string(context, input, "hash")?;
    fields.host = truthy_property_string(context, input, "host")?;
    fields.hostname = truthy_property_string(context, input, "hostname")?;
    fields.port = truthy_property_string(context, input, "port")?;
    let query = get(context, input, "query")?;
    if context.is_object_rooted(query)? {
        let query_module = crate::modules::querystring_shared_vm::module(context)?;
        let stringify = get(context, query_module, "stringify")?;
        let undefined = context.undefined();
        let stringified = context.call_rooted(stringify, undefined, &[query])?;
        fields.query = context.string_text(stringified)?;
    }
    fields.search = truthy_property_string(context, input, "search")?;
    let slashes = get(context, input, "slashes")?;
    fields.slashes = context.truthy_rooted(slashes)?;
    let formatted = format_legacy_fields(fields);
    Ok(context.string_rooted(&formatted))
}

#[derive(Default)]
struct LegacyFormatFields {
    protocol: Option<String>,
    auth: Option<String>,
    auth_encoded: Option<String>,
    host: Option<String>,
    hostname: Option<String>,
    port: Option<String>,
    pathname: Option<String>,
    search: Option<String>,
    hash: Option<String>,
    query: Option<String>,
    slashes: bool,
}

impl LegacyFormatFields {
    fn set(&mut self, name: &str, value: String) {
        match name {
            "protocol" => self.protocol = Some(value),
            "auth" => self.auth = Some(value),
            "host" => self.host = Some(value),
            "hostname" => self.hostname = Some(value),
            "port" => self.port = Some(value),
            "pathname" => self.pathname = Some(value),
            "search" => self.search = Some(value),
            "hash" => self.hash = Some(value),
            _ => {}
        }
    }
}

fn format_legacy_fields(fields: LegacyFormatFields) -> String {
    let mut protocol = fields.protocol.unwrap_or_default();
    if !protocol.is_empty() && !protocol.ends_with(':') {
        protocol.push(':');
    }
    let mut host = fields.host.unwrap_or_default();
    if host.is_empty() {
        if let Some(hostname) = fields.hostname {
            host = if hostname.contains(':') && !hostname.starts_with('[') {
                format!("[{hostname}]")
            } else {
                hostname
            };
            if let Some(port) = fields.port {
                if !port.is_empty() {
                    host.push(':');
                    host.push_str(&port);
                }
            }
        }
    }
    if let Some(auth) = fields.auth_encoded {
        host = format!("{auth}@{host}");
    } else if let Some(auth) = fields.auth.filter(|auth| !auth.is_empty()) {
        host = format!("{}@{host}", encode_auth(&auth));
    }

    let mut pathname = fields.pathname.unwrap_or_default();
    pathname = pathname.replace('#', "%23").replace('?', "%3F");
    let mut search = fields
        .search
        .filter(|search| !search.is_empty())
        .or_else(|| {
            fields
                .query
                .filter(|query| !query.is_empty())
                .map(|query| format!("?{query}"))
        })
        .unwrap_or_default();
    if search.contains('#') {
        search = search.replace('#', "%23");
    }
    let mut hash = fields.hash.unwrap_or_default();
    if !hash.is_empty() && !hash.starts_with('#') {
        hash.insert(0, '#');
    }

    if fields.slashes || is_slashed_protocol(&protocol) {
        if fields.slashes || !host.is_empty() {
            if !pathname.is_empty() && !pathname.starts_with('/') {
                pathname.insert(0, '/');
            }
            host.insert_str(0, "//");
        } else if protocol.starts_with("file") {
            host.push_str("//");
        }
    }
    if !search.is_empty() && !search.starts_with('?') {
        search.insert(0, '?');
    }
    format!("{protocol}{host}{pathname}{search}{hash}")
}

fn is_slashed_protocol(protocol: &str) -> bool {
    matches!(
        protocol,
        "http:" | "https:" | "ftp:" | "gopher:" | "file:" | "ws:" | "wss:"
    )
}

fn encode_auth(auth: &str) -> String {
    let mut encoded = String::with_capacity(auth.len());
    for byte in auth.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'():".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(b"0123456789ABCDEF"[(byte >> 4) as usize]));
            encoded.push(char::from(b"0123456789ABCDEF"[(byte & 0x0f) as usize]));
        }
    }
    encoded
}

fn encode_auth_preserving_escapes(auth: &str) -> String {
    let bytes = auth.as_bytes();
    let mut encoded = String::with_capacity(auth.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && bytes[index + 1].is_ascii_hexdigit()
            && bytes[index + 2].is_ascii_hexdigit()
        {
            encoded.push('%');
            encoded.push(char::from(bytes[index + 1]));
            encoded.push(char::from(bytes[index + 2]));
            index += 3;
            continue;
        }
        let character = auth[index..].chars().next().expect("index is in bounds");
        if character.is_ascii_alphanumeric() || "-_.!~*'():".contains(character) {
            encoded.push(character);
        } else {
            let mut bytes = [0; 4];
            for byte in character.encode_utf8(&mut bytes).as_bytes() {
                encoded.push('%');
                encoded.push(char::from(b"0123456789ABCDEF"[(byte >> 4) as usize]));
                encoded.push(char::from(b"0123456789ABCDEF"[(byte & 0x0f) as usize]));
            }
        }
        index += character.len_utf8();
    }
    encoded
}

fn format_whatwg_url(
    context: &mut NativeContext<'_, NodeHost>,
    href: &str,
    options: Option<RootId>,
) -> Result<String, RootedError> {
    // URL instances expose an ASCII hostname in `href`. Canonicalize here as
    // well because a structurally compatible URL object may provide a Unicode
    // hostname; `{ unicode: true }` converts the canonical hostname back.
    let mut formatted = format_ascii_hostname(href);
    let Some(options) = options else {
        return Ok(formatted);
    };
    if context
        .rooted_value(options)
        .is_some_and(|value| value.is_null() || value.is_undefined())
        || !context.truthy_rooted(options)?
    {
        return Ok(formatted);
    }
    if !context.is_object_rooted(options)? {
        return Err(invalid_format_options(context));
    }
    if !option_truthy(context, options, "fragment")? {
        if let Some(index) = formatted.find('#') {
            formatted.truncate(index);
        }
    }
    // Unlike `auth`, `fragment`, and `search`, Unicode formatting is opt-in.
    // Missing or undefined `unicode` must preserve the URL's ASCII hostname.
    let unicode_option = get(context, options, "unicode")?;
    let unicode = context.truthy_rooted(unicode_option)?;
    if unicode {
        formatted = format_unicode_hostname(&formatted);
    }
    if !option_truthy(context, options, "search")? {
        let before_hash = formatted.find('#').unwrap_or(formatted.len());
        if let Some(index) = formatted[..before_hash].find('?') {
            formatted.replace_range(index..before_hash, "");
        }
    }
    if !option_truthy(context, options, "auth")? {
        if let Some(authority_start) = formatted.find("//").map(|index| index + 2) {
            let authority_end = formatted[authority_start..]
                .find(['/', '?', '#'])
                .map(|index| authority_start + index)
                .unwrap_or(formatted.len());
            if let Some(auth_end) = formatted[authority_start..authority_end].rfind('@') {
                formatted.replace_range(authority_start..authority_start + auth_end + 1, "");
            }
        }
    }
    Ok(formatted)
}

fn format_ascii_hostname(href: &str) -> String {
    let Some(scheme_end) = href.find("://") else {
        return href.to_owned();
    };
    let authority_start = scheme_end + 3;
    let authority_end = href[authority_start..]
        .find(['/', '?', '#'])
        .map(|index| authority_start + index)
        .unwrap_or(href.len());
    let host_start = href[authority_start..authority_end]
        .rfind('@')
        .map(|index| authority_start + index + 1)
        .unwrap_or(authority_start);
    let host_end = if href[host_start..authority_end].starts_with('[') {
        href[host_start..authority_end]
            .find(']')
            .map(|index| host_start + index + 1)
            .unwrap_or(authority_end)
    } else {
        href[host_start..authority_end]
            .find(':')
            .map(|index| host_start + index)
            .unwrap_or(authority_end)
    };
    let hostname = &href[host_start..host_end];
    let Ok(ascii) = idna::domain_to_ascii(hostname) else {
        return href.to_owned();
    };
    if ascii == hostname {
        return href.to_owned();
    }
    let mut formatted = href.to_owned();
    formatted.replace_range(host_start..host_end, &ascii);
    formatted
}

fn format_unicode_hostname(href: &str) -> String {
    let Some(scheme_end) = href.find("://") else {
        return href.to_owned();
    };
    let authority_start = scheme_end + 3;
    let authority_end = href[authority_start..]
        .find(['/', '?', '#'])
        .map(|index| authority_start + index)
        .unwrap_or(href.len());
    let authority = &href[authority_start..authority_end];
    let host_start = authority
        .rfind('@')
        .map(|index| authority_start + index + 1)
        .unwrap_or(authority_start);
    let host_end = if href[host_start..authority_end].starts_with('[') {
        href[host_start..authority_end]
            .find(']')
            .map(|index| host_start + index + 1)
            .unwrap_or(authority_end)
    } else {
        href[host_start..authority_end]
            .find(':')
            .map(|index| host_start + index)
            .unwrap_or(authority_end)
    };
    let hostname = &href[host_start..host_end];
    let (unicode, result) = idna::domain_to_unicode(hostname);
    if result.is_err() || unicode == hostname {
        return href.to_owned();
    }
    let mut formatted = href.to_owned();
    formatted.replace_range(host_start..host_end, &unicode);
    formatted
}

fn option_truthy(
    context: &mut NativeContext<'_, NodeHost>,
    options: RootId,
    name: &str,
) -> Result<bool, RootedError> {
    let value = get(context, options, name)?;
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(true);
    }
    context.truthy_rooted(value)
}

fn truthy_property_string(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<Option<String>, RootedError> {
    let value = get(context, object, name)?;
    if context.truthy_rooted(value)? {
        Ok(Some(context.to_string(value)?))
    } else {
        Ok(None)
    }
}

fn invalid_format_argument(context: &mut NativeContext<'_, NodeHost>) -> RootedError {
    coded_url_type_error(
        context,
        "ERR_INVALID_ARG_TYPE",
        "The \"urlObject\" argument must be of type object or string",
    )
}

fn invalid_format_options(context: &mut NativeContext<'_, NodeHost>) -> RootedError {
    coded_url_type_error(
        context,
        "ERR_INVALID_ARG_TYPE",
        "The \"options\" argument must be of type object",
    )
}

fn parse_whatwg(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args
        .first()
        .copied()
        .map(|root| context.to_string(root))
        .transpose()?
        .unwrap_or_else(|| "undefined".to_owned());
    let base = args
        .get(1)
        .copied()
        .filter(|root| {
            context
                .rooted_value(*root)
                .is_some_and(|value| !value.is_undefined())
        })
        .map(|root| context.to_string(root))
        .transpose()?;

    let parsed = crate::modules::url_whatwg_data::Parsed::parse_strict(&input, base.as_deref())
        .map_err(|_| invalid_url(context, &input))?;
    let fields = [
        "href", "origin", "protocol", "username", "password", "host", "hostname", "port",
        "pathname", "search", "hash",
    ];
    let object = context.object_rooted()?;
    for name in fields {
        let value = parsed.get(name);
        set_text(context, object, name, &value)?;
    }
    Ok(object)
}

fn query_object(
    context: &mut NativeContext<'_, NodeHost>,
    query: &str,
) -> Result<RootId, RootedError> {
    let object = context.null_object_rooted()?;
    for (key, values) in crate::modules::querystring_data::parse_default_entries(query) {
        let value = if values.len() == 1 {
            context.string_rooted(&values[0])
        } else {
            let values = values
                .iter()
                .map(|value| context.string_rooted(value))
                .collect::<Vec<_>>();
            context.array_rooted(&values)?
        };
        set(context, object, &key, value)?;
    }
    Ok(object)
}

fn legacy_value(context: &mut NativeContext<'_, NodeHost>, value: LegacyUrlField) -> RootId {
    match value {
        LegacyUrlField::Null => context.null(),
        LegacyUrlField::Boolean(value) => context.boolean(value),
        LegacyUrlField::Text(value) => context.string_rooted(&value),
    }
}

fn coded_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    code: &str,
    input: &str,
) -> RootedError {
    let exception = match context.type_error_rooted("Invalid URL") {
        Ok(exception) => exception,
        Err(error) => return error,
    };
    let code = context.string_rooted(code);
    let input = context.string_rooted(input);
    if let Err(error) = set(context, exception, "code", code) {
        return error;
    }
    if let Err(error) = set(context, exception, "input", input) {
        return error;
    }
    context.throw(exception)
}

fn invalid_url(context: &mut NativeContext<'_, NodeHost>, input: &str) -> RootedError {
    coded_type_error(context, "ERR_INVALID_URL", input)
}

fn uri_error(context: &mut NativeContext<'_, NodeHost>) -> RootedError {
    let exception = match (|| {
        let global = context.global_root()?;
        let key = context.string_rooted("URIError");
        let constructor = context.get_property_rooted(global, key)?;
        let message = context.string_rooted("URI malformed");
        context.construct_rooted(constructor, constructor, &[message])
    })() {
        Ok(exception) => exception,
        Err(error) => return error,
    };
    context.throw(exception)
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.set_property_rooted(object, key, value, object)?;
    Ok(())
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

fn set_text(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: &str,
) -> Result<(), RootedError> {
    let value = context.string_rooted(value);
    set(context, object, name, value)
}

/// The actual URL constructor's private WeakMap is the URL-instance authority.
/// Unlike isURL's structural contract, this never reads user properties.
pub(crate) fn is_url_instance(
    c: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| c.undefined());
    if !c.is_object_rooted(value)? {
        return Ok(c.boolean(false));
    }
    let shared = c.host_mut().shared_state();
    let state = shared.borrow().url_state;
    let valid = if let Some(state) = state {
        let data = c.weak_map_get_rooted(state, value)?;
        c.is_object_rooted(data)?
    } else {
        false
    };
    Ok(c.boolean(valid))
}
pub(crate) fn get_url_href(
    c: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| c.undefined());
    let shared = c.host_mut().shared_state();
    let state = shared.borrow().url_state;
    if let Some(state) = state {
        let data = c.weak_map_get_rooted(state, value)?;
        if c.is_object_rooted(data)? {
            return get(c, data, "href");
        }
    }
    let error = c.type_error_rooted("Value must be an instance of URL")?;
    Err(c.throw(error))
}
