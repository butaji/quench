use crate::host::NodeHost;
use crate::modules::event_constructor_shared_vm::get;
use crate::modules::net_shared_vm;
use quench_runtime::{NativeArithmetic, NativeContext, RootId, RootedError, Value};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};

type Context<'a> = NativeContext<'a, NodeHost>;

const DEFAULT_LISTEN_HOST: &str = "127.0.0.1";

const MODULE_FACTORY: &str = r#"((initializeServer, EventEmitter, request, createAgent, destroyAgent, IncomingMessage, ServerResponse) => {
  const invalidArgument = (name, expected, value) => {
    let received;
    if (Array.isArray(value)) received = "an instance of Array";
    else if (value === null) received = "null";
    else if (typeof value === "string") received = `type string (${JSON.stringify(value)})`;
    else if (typeof value === "number" || typeof value === "boolean") {
      received = `type ${typeof value} (${String(value)})`;
    } else received = `type ${typeof value}`;
    const error = new TypeError(`The \"${name}\" argument must be ${expected}. Received ${received}`);
    error.code = "ERR_INVALID_ARG_TYPE";
    return error;
  };
  function Server(options, requestListener) {
      if (!(this instanceof Server)) return new Server(options, requestListener);
      EventEmitter.call(this);
      if (typeof options === "function") {
        requestListener = options;
        options = undefined;
      } else if (options !== undefined &&
          (options === null || typeof options !== "object" || Array.isArray(options))) {
        throw invalidArgument("options", "of type object", options);
      }
      if (requestListener !== undefined && typeof requestListener !== "function") {
        throw invalidArgument("requestListener", "of type function", requestListener);
      }
      this.timeout = 0;
      initializeServer([this, requestListener]);
  }
  Object.setPrototypeOf(Server, EventEmitter);
  Object.setPrototypeOf(Server.prototype, EventEmitter.prototype);
  Object.defineProperty(Server.prototype, "setTimeout", {
    configurable: true, writable: true,
    value: function setTimeout(msecs, callback) {
      this.timeout = msecs;
      if (callback !== undefined) this.on("timeout", callback);
      return this;
    }
  });
  const createServer = (...args) => new Server(...args);
  // Node's lib/_http_agent.js public pool state; the native pool owns the sockets.
  const DEFAULT_KEEP_ALIVE_MSECS = 1000;
  const DEFAULT_MAX_FREE_SOCKETS = 256;
  const DEFAULT_KEEP_ALIVE_TIMEOUT_BUFFER = 1000;
  const MIN_TOTAL_SOCKETS = 1;
  const SCHEDULING = ["fifo", "lifo"];
  function Agent(options) {
    if (!(this instanceof Agent)) return new Agent(options);
    EventEmitter.call(this);
    this.options = { __proto__: null, ...options };
    this.defaultPort = this.options.defaultPort || 80;
    this.protocol = this.options.protocol || "http:";
    if (this.options.noDelay === undefined) this.options.noDelay = true;
    this.options.path = null;
    this.requests = { __proto__: null };
    this.sockets = { __proto__: null };
    this.freeSockets = { __proto__: null };
    this.keepAliveMsecs = this.options.keepAliveMsecs || DEFAULT_KEEP_ALIVE_MSECS;
    this.keepAlive = this.options.keepAlive || false;
    this.maxSockets = this.options.maxSockets || Agent.defaultMaxSockets;
    this.maxFreeSockets = this.options.maxFreeSockets || DEFAULT_MAX_FREE_SOCKETS;
    this.scheduling = this.options.scheduling || "lifo";
    this.maxTotalSockets = this.options.maxTotalSockets;
    this.totalSocketCount = 0;
    const buffer = this.options.agentKeepAliveTimeoutBuffer;
    this.agentKeepAliveTimeoutBuffer =
      typeof buffer === "number" && buffer >= 0 && Number.isFinite(buffer) ? buffer : DEFAULT_KEEP_ALIVE_TIMEOUT_BUFFER;
    if (!SCHEDULING.includes(this.scheduling)) {
      const error = new TypeError(`The argument 'scheduling' must be one of: 'fifo', 'lifo'. Received ${JSON.stringify(this.scheduling)}`);
      error.code = "ERR_INVALID_ARG_VALUE";
      throw error;
    }
    if (this.maxTotalSockets !== undefined) {
      if (typeof this.maxTotalSockets !== "number") throw invalidArgument("maxTotalSockets", "of type number", this.maxTotalSockets);
      if (!(this.maxTotalSockets >= MIN_TOTAL_SOCKETS)) {
        const error = new RangeError(`The value of "maxTotalSockets" is out of range. It must be >= ${MIN_TOTAL_SOCKETS}. Received ${this.maxTotalSockets}`);
        error.code = "ERR_OUT_OF_RANGE";
        throw error;
      }
    } else {
      this.maxTotalSockets = Infinity;
    }
    Object.defineProperty(this, "_quenchSharedAgentId", { value: createAgent(options) });
  }
  Object.setPrototypeOf(Agent.prototype, EventEmitter.prototype);
  Object.setPrototypeOf(Agent, EventEmitter);
  Agent.defaultMaxSockets = Infinity;
  Agent.prototype.destroy = function destroy() {
    return destroyAgent(this._quenchSharedAgentId);
  };
  // Node's internal/http.js getGlobalAgent options.
  const GLOBAL_AGENT_TIMEOUT_MSECS = 5000;
  const globalAgent = new Agent({ keepAlive: true, scheduling: "lifo", timeout: GLOBAL_AGENT_TIMEOUT_MSECS });
  const get = (input, options, callback) => {
    const outgoing = request(input, options, callback);
    outgoing.end();
    return outgoing;
  };
  // Node's default --max-http-header-size, exposed for HTTP client libraries.
  const maxHeaderSize = 16 * 1024;
  const api = { Server, createServer, request, get, Agent, globalAgent, IncomingMessage, ServerResponse };
  Object.defineProperty(api, "maxHeaderSize", {
    configurable: true,
    enumerable: true,
    get() { return maxHeaderSize; },
  });
  return api;
})"#;

// Node's lib/https.js agent over the HTTP agent's pool state. Fastify and the AWS SDK
// import both transports at initialization; requests need the TLS client transport.
const HTTPS_FACTORY: &str = r#"((http) => {
  const DEFAULT_MAX_CACHED_SESSIONS = 100;
  function Agent(options) {
    if (!(this instanceof Agent)) return new Agent(options);
    options = { __proto__: null, ...options };
    options.defaultPort ??= 443;
    options.protocol ??= "https:";
    http.Agent.call(this, options);
    this.maxCachedSessions = this.options.maxCachedSessions;
    if (this.maxCachedSessions === undefined) this.maxCachedSessions = DEFAULT_MAX_CACHED_SESSIONS;
    this._sessionCache = { map: {}, list: [] };
  }
  Object.setPrototypeOf(Agent.prototype, http.Agent.prototype);
  Object.setPrototypeOf(Agent, http.Agent);
  // Node's internal/http.js getGlobalAgent options.
  const GLOBAL_AGENT_TIMEOUT_MSECS = 5000;
  const globalAgent = new Agent({ keepAlive: true, scheduling: "lifo", timeout: GLOBAL_AGENT_TIMEOUT_MSECS });
  const unavailable = () => new Error("HTTPS transport is unavailable");
  return {
    Agent,
    globalAgent,
    request: function request() { throw unavailable(); },
    get: function get() { throw unavailable(); },
  };
})"#;

const SERVER_CLOSE_FACTORY: &str = r#"((closeOperation) => function(callback) {
  if (callback !== undefined) this.once("close", callback);
  closeOperation.call(this);
  return this;
})"#;

/// Node's `ServerResponse` as an `OutgoingMessage`: a legacy `Stream` whose writes go to
/// the native exchange `id` names. Methods live on the prototype and read the exchange from
/// the instance; `end` writes and finishes synchronously, then `finish` and `close` follow on
/// ticks, as Node's socket write completion and `emitCloseNT` deliver them.
/// Node's `getDefaultHighWaterMark()` for byte streams outside Windows
/// (`lib/internal/streams/state.js`); a socket write returns false once its
/// writable length reaches it.
pub(super) const RESPONSE_HIGH_WATER_MARK: usize = 64 * 1024;

/// A ServerResponse's `statusCode` until the application sets one (Node's `ServerResponse`).
pub(super) const DEFAULT_STATUS: u16 = 200;

/// The own data properties the EventEmitter construction reads that a program may write
/// (`objectTemplateCapture`'s probes), as flat (holder, key) pairs: `init`'s reads of the
/// EventEmitter prototype's `_events`, `_maxListeners` and capture flag, `EventEmitter.init`
/// itself, and `Function.prototype.call`, which each `X.call(this)` of a constructor chain
/// reads. Prototype layout (accessors, added keys) a template guards itself.
fn emitter_template_probes(context: &mut Context<'_>, emitter: RootId) -> Result<Vec<RootId>, RootedError> {
    let event_data = context
        .host_mut()
        .shared_state()
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("event listener configuration missing"))?;
    let capture_key = crate::modules::event_constructor_shared_vm::get(context, event_data, "captureKey")?;
    let emitter_prototype = get(context, emitter, "prototype")?;
    let global = context.global_root()?;
    let function = get(context, global, "Function")?;
    let function_prototype = get(context, function, "prototype")?;
    let name = |context: &mut Context<'_>, text: &str| context.string_rooted(text);
    Ok(vec![
        emitter_prototype,
        name(context, "_events"),
        emitter_prototype,
        name(context, "_maxListeners"),
        emitter_prototype,
        capture_key,
        emitter,
        name(context, "init"),
        function_prototype,
        name(context, "call"),
    ])
}

/// The probes of `new IncomingMessage(undefined)` (the pinned library's `IncomingMessage`,
/// `Readable`, `ReadableState` and `Stream`): the EventEmitter's
/// (`emitter_template_probes`) and the `prototype` of each constructor `new` reads. The
/// default high water mark is module state the factory compares.
fn incoming_template_probes(
    context: &mut Context<'_>,
    emitter: RootId,
    incoming: RootId,
) -> Result<RootId, RootedError> {
    let readable_state = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "http.js",
        "internal/streams/readable.js",
        "ReadableState",
    )?;
    let mut probes = emitter_template_probes(context, emitter)?;
    for constructor in [incoming, readable_state] {
        let key = context.string_rooted("prototype");
        probes.extend([constructor, key]);
    }
    context.array_rooted(&probes)
}

/// The guest function a parsed server request enters JavaScript through: it makes the
/// IncomingMessage (`incoming::CREATE_FACTORY`'s leading arguments, from the head through the
/// exchange id) and its ServerResponse, publishes the request start to the subscribers the host
/// passes (Node's `onRequestStartChannel`, only when the channel has subscribers), then emits
/// `request` on the server, as Node's `parserOnIncoming` does within one parser callback.
const REQUEST_DISPATCH_FACTORY: &str = quench_js_check::checked_js!(
    r#"(createIncoming, respond) => {
  'use strict';
  return function dispatchRequest(method, url, statusCode, statusMessage, httpVersion, httpVersionMajor, httpVersionMinor, rawHeaders, socket, complete, body, open, exchange, responseId, responseStatus, server, startSubscribers, startChannel) {
    // Node's parser marks the message complete once its body is parsed, after the
    // `request` listeners return (`parserOnMessageComplete`).
    const request = createIncoming(method, url, statusCode, statusMessage, httpVersion, httpVersionMajor, httpVersionMinor, rawHeaders, socket, false, body, open, exchange, true);
    const response = respond(responseId, responseStatus, request, server, socket);
    if (startSubscribers !== undefined) {
      const message = { request, response, socket, server };
      for (let index = 0; index < startSubscribers.length; index++) {
        const subscriber = startSubscribers[index];
        // A subscriber's throw is uncaught on the next tick; the rest still run, as
        // Node's `Channel.publish` runs them.
        try {
          subscriber(message, startChannel);
        } catch (error) {
          process.nextTick(() => { throw error; });
        }
      }
    }
    server.emit("request", request, response);
    if (complete && !open) request.complete = true;
  };
}"#
);

const RESPONSE_FACTORY: &str = r#"((Stream, setHeaderOperation, getHeaderOperation, getHeadersOperation, removeHeaderOperation, write, finish, writeHeadOperation, destroyOperation, initializeResponse, nextTick, codes, captureTemplate, copyTemplate, emitterProbes, captureLimit) => {
  const kExchange = Symbol("kResponseExchange");
  const kFinished = Symbol("kResponseFinished");
  // `end` callbacks waiting for the response to finish, as `_http_outgoing.js`'s
  // `kEndCallbacks`: they run with `null` before "finish" is emitted.
  const kEndCallbacks = Symbol("kEndCallbacks");
  // A header value as the native exchange takes it: each element of an array as a string,
  // any other value but `undefined` as one string; an array also marks a multiple header.
  // Hoisted, so `headerValue` makes no closure and its calls need no frame.
  const headerString = (item) => String(item);
  const headerValue = (value) => {
    if (value === undefined) return value;
    if (Array.isArray(value)) return value.map(headerString);
    return String(value);
  };
  const validChunk = (chunk) => {
    if (chunk === null) throw new codes.ERR_STREAM_NULL_VALUES();
    if (typeof chunk !== "string" && !(chunk instanceof Uint8Array)) {
      throw new codes.ERR_INVALID_ARG_TYPE("chunk", ["string", "Buffer", "Uint8Array"], chunk);
    }
  };
  const writeAfterEnd = (response, callback) => {
    const error = new codes.ERR_STREAM_WRITE_AFTER_END();
    nextTick(() => {
      response.emit("error", error);
      if (typeof callback === "function") callback(error);
    });
  };
  // `_http_outgoing.js`'s `queueEndCallback` without a flush error, which a response that
  // never errors its flush does not record.
  const queueEndCallback = (response, callback) => {
    if (response[kFinished]) {
      callback(new codes.ERR_STREAM_ALREADY_FINISHED("end"));
      return;
    }
    const callbacks = response[kEndCallbacks];
    if (callbacks === undefined) response[kEndCallbacks] = [callback];
    else callbacks.push(callback);
  };
  const emitFinished = (response) => {
    response[kFinished] = true;
    const callbacks = response[kEndCallbacks];
    if (callbacks !== undefined) {
      response[kEndCallbacks] = undefined;
      for (let index = 0; index < callbacks.length; index++) callbacks[index](null);
    }
    response.emit("finish");
    nextTick(() => response.emit("close"));
  };
  function ServerResponse(id, statusCode, request, server, socket) {
    if (!(this instanceof ServerResponse)) {
      return new ServerResponse(id, statusCode, request, server, socket);
    }
    if (typeof id !== "number") {
      socket = undefined;
      server = undefined;
      request = id;
      id = 0;
      statusCode = 200;
    }
    Stream.call(this);
    this.outputData = [];
    this.outputSize = 0;
    this.writable = true;
    this.destroyed = false;
    this.chunkedEncoding = false;
    this.useChunkedEncodingByDefault = true;
    this.sendDate = true;
    this.finished = false;
    this._headerSent = false;
    this.statusCode = undefined;
    this.statusMessage = undefined;
    this.req = undefined;
    this[kExchange] = { id: undefined, request: undefined, server: undefined, socket: undefined };
    this[kFinished] = false;
    if (!capturing) bind(this, id, statusCode, request, server, socket);
  }
  // The values a response takes from its request, into the slots its construction made,
  // and the exchange's host initialization.
  const bind = (response, id, statusCode, request, server, socket) => {
    response.statusCode = statusCode;
    response.req = request;
    const exchange = response[kExchange];
    exchange.id = id;
    exchange.request = request;
    exchange.server = server;
    exchange.socket = socket;
    const initializedId = initializeResponse(id, response);
    if (typeof initializedId === "number") exchange.id = initializedId;
  };
  // A request's response: a copy of the template of the constructor's layout
  // (`objectTemplateCapture`), built with `bind` left out, then bound. The constructor reads
  // the EventEmitter probes and `ServerResponse.prototype`; any change constructs, and the
  // template is captured again up to `captureLimit` times.
  let capturing = false;
  let template;
  let captures = 0;
  const respond = (id, statusCode, request, server, socket) => {
    const copy = template === undefined ? undefined : copyTemplate(template);
    if (copy !== undefined) {
      bind(copy, id, statusCode, request, server, socket);
      return copy;
    }
    if (captures < captureLimit) {
      captures++;
      capturing = true;
      try {
        template = captureTemplate(new ServerResponse(id), new ServerResponse(id), [...emitterProbes, ServerResponse, "prototype"]);
      } finally {
        capturing = false;
      }
    }
    return new ServerResponse(id, statusCode, request, server, socket);
  };
  ServerResponse.prototype = Object.create(Stream.prototype);
  Object.setPrototypeOf(ServerResponse, Stream);
  const define = (name, value) => Object.defineProperty(ServerResponse.prototype, name, {
    value, writable: true, configurable: true,
  });
  const getter = (name, get) => Object.defineProperty(ServerResponse.prototype, name, {
    get, configurable: true, enumerable: true,
  });
  define("constructor", ServerResponse);
  define("assignSocket", function assignSocket(socket) {
    this.socket = socket;
    this.connection = socket;
    return this;
  });
  define("setHeader", function setHeader(name, value) {
    const multiple = Array.isArray(value);
    setHeaderOperation(this[kExchange].id, name, headerValue(value), multiple);
    return this;
  });
  define("getHeader", function getHeader(name) {
    return getHeaderOperation(this[kExchange].id, name);
  });
  define("getHeaders", function getHeaders() {
    return getHeadersOperation(this[kExchange].id);
  });
  define("removeHeader", function removeHeader(name) {
    return removeHeaderOperation(this[kExchange].id, name);
  });
  define("writeHead", function writeHead(status, reasonOrHeaders, headers) {
    const hasReason = typeof reasonOrHeaders === "string";
    const reason = hasReason ? reasonOrHeaders : undefined;
    const source = hasReason ? headers : reasonOrHeaders;
    const entries = [];
    let invalid = false;
    if (source !== undefined) {
      if (Array.isArray(source)) {
        if (source.length % 2 !== 0) invalid = true;
        else {
          for (let index = 0; index < source.length; index += 2) {
            const value = source[index + 1];
            const multiple = Array.isArray(value);
            entries.push(source[index], headerValue(value), multiple);
          }
        }
      } else if (source !== null && typeof source === "object") {
        const names = Object.keys(source);
        for (let index = 0; index < names.length; index++) {
          const name = names[index];
          const value = source[name];
          const multiple = Array.isArray(value);
          entries.push(name, headerValue(value), multiple);
        }
      } else {
        invalid = true;
      }
    }
    const header = writeHeadOperation(this[kExchange].id, status, reason, entries, invalid, this);
    if (typeof header === "string") this._header = header;
    this._headerSent = true;
    return this;
  });
  define("write", function write(chunk, encoding, callback) {
    if (typeof encoding === "function") {
      callback = encoding;
      encoding = null;
    }
    validChunk(chunk);
    if (this.finished) {
      writeAfterEnd(this, callback);
      return false;
    }
    const belowMark = writeChunk(this, chunk, encoding, true);
    if (typeof callback === "function") nextTick(callback);
    return belowMark;
  });
  const writeChunk = (response, chunk, encoding, more) => {
    const belowMark = write(response[kExchange].id, chunk, more, response.statusCode, response.statusMessage, response, encoding);
    response._headerSent = true;
    return belowMark;
  };
  define("end", function end(chunk, encoding, callback) {
    if (typeof chunk === "function") {
      callback = chunk;
      chunk = null;
      encoding = null;
    } else if (typeof encoding === "function") {
      callback = encoding;
      encoding = null;
    }
    if (chunk) {
      validChunk(chunk);
      if (this.finished) {
        writeAfterEnd(this, callback);
        return this;
      }
      writeChunk(this, chunk, encoding, false);
    } else if (this.finished) {
      if (typeof callback === "function") queueEndCallback(this, callback);
      return this;
    }
    if (typeof callback === "function") queueEndCallback(this, callback);
    this.finished = true;
    const exchange = this[kExchange];
    finish(exchange.id, this.statusCode, this.statusMessage, exchange.request, this, exchange.server, exchange.socket);
    this._headerSent = true;
    this.emit("prefinish");
    nextTick(emitFinished, this);
    return this;
  });
  define("destroy", function destroy(error) {
    if (this.destroyed) return this;
    this.destroyed = true;
    destroyOperation(this[kExchange].id);
    nextTick(() => {
      if (error) this.emit("error", error);
      this.emit("close");
    });
    return this;
  });
  define("cork", function cork() {});
  define("uncork", function uncork() {});
  getter("headersSent", function headersSent() { return this._headerSent; });
  getter("writableEnded", function writableEnded() { return this.finished; });
  getter("writableFinished", function writableFinished() { return this[kFinished]; });
  getter("writableObjectMode", function writableObjectMode() { return false; });
  getter("writableLength", function writableLength() { return 0; });
  getter("writableCorked", function writableCorked() { return 0; });
  getter("writableNeedDrain", function writableNeedDrain() { return false; });
  return [ServerResponse, respond];
})"#;

pub(crate) fn module(context: &mut Context<'_>) -> Result<RootId, RootedError> {
    let status_codes = crate::modules::http_status_shared_vm::codes(context)?;
    let set_header =
        context.host_function(crate::host::shared_vm::operation("httpResponseSetHeader"))?;
    let get_header =
        context.host_function(crate::host::shared_vm::operation("httpResponseGetHeader"))?;
    let get_headers =
        context.host_function(crate::host::shared_vm::operation("httpResponseGetHeaders"))?;
    let remove_header = context.host_function(crate::host::shared_vm::operation(
        "httpResponseRemoveHeader",
    ))?;
    let write = context.host_function_with_data(
        crate::host::shared_vm::operation("httpResponseWrite"),
        status_codes,
    )?;
    let finish = context.host_function_with_data(
        crate::host::shared_vm::operation("httpResponseFinish"),
        status_codes,
    )?;
    let write_head = context.host_function_with_data(
        crate::host::shared_vm::operation("httpResponseWriteHead"),
        status_codes,
    )?;
    let destroy_response =
        context.host_function(crate::host::shared_vm::operation("httpResponseDestroy"))?;
    // Core owns the weak response-to-transport identity. Keeping the method on
    // the prototype preserves ordinary method identity and receiver dispatch.
    let global = context.global_root()?;
    let key = context.string_rooted("WeakMap");
    let weak_map = context.get_property_rooted(global, key)?;
    let responses = context.construct_rooted(weak_map, weak_map, &[])?;
    let key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(weak_map, key)?;
    let key = context.string_rooted("set");
    let map_set = context.get_property_rooted(prototype, key)?;
    let tracking = super::state::ResponseTracking {
        responses: context.retain(responses)?,
        map_set: context.retain(map_set)?,
    };
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
        .response_tracking = Some(tracking);
    let initialize_response =
        context.host_function(crate::host::shared_vm::operation("httpResponseInitialize"))?;
    let send = context.host_constructor_with_data(
        crate::host::shared_vm::operation("httpResponseSend"),
        responses,
    )?;
    let key = context.string_rooted("name");
    let name = context.string_rooted("_send");
    context.define_data_property_rooted(send, key, name, false, false, true)?;
    let stream = crate::host::shared_vm::commonjs::stream_module(context)?;
    let legacy_stream = get(context, stream, "Stream")?;
    let process = get(context, global, "process")?;
    let next_tick = get(context, process, "nextTick")?;
    let codes = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "internal/errors.js",
        "internal/errors.js",
        "codes",
    )?;
    let emitter_key = context.string_rooted("__nodeEventEmitter");
    let emitter = context.get_property_rooted(global, emitter_key)?;
    let emitter_probes = emitter_template_probes(context, emitter)?;
    let emitter_probes = context.array_rooted(&emitter_probes)?;
    let capture = context.host_function(crate::host::shared_vm::operation("objectTemplateCapture"))?;
    let copy = context.host_function(crate::host::shared_vm::operation("objectTemplateCopy"))?;
    let capture_limit = context.number(f64::from(super::incoming::TEMPLATE_CAPTURE_LIMIT));
    let response_factory =
        context.evaluate_script_rooted(RESPONSE_FACTORY, "node:http/response-factory.js")?;
    let undefined = context.undefined();
    let response_api = context.call_rooted(
        response_factory,
        undefined,
        &[
            legacy_stream,
            set_header,
            get_header,
            get_headers,
            remove_header,
            write,
            finish,
            write_head,
            destroy_response,
            initialize_response,
            next_tick,
            codes,
            capture,
            copy,
            emitter_probes,
            capture_limit,
        ],
    )?;
    let response_factory = get(context, response_api, "0")?;
    let respond = get(context, response_api, "1")?;
    let key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(response_factory, key)?;
    set(context, prototype, "_send", send)?;
    let incoming_constructor = crate::host::shared_vm::commonjs::node_library_export(
        context,
        "http.js",
        "_http_incoming.js",
        "IncomingMessage",
    )?;
    // One `_read` and `_destroy` serve every message: each finds its exchange under the
    // host-private key its side records (`incoming::create`).
    let server_exchange = context.private_symbol_rooted(Some("node:http:serverExchange"));
    let client_exchange = context.private_symbol_rooted(Some("node:http:clientExchange"));
    let exchange_keys = context.array_rooted(&[server_exchange, client_exchange])?;
    let incoming_read = context.host_function_named_with_data(
        crate::host::shared_vm::operation("httpIncomingRead"),
        "_read",
        exchange_keys,
    )?;
    let incoming_destroy = context.host_function_named_with_data(
        crate::host::shared_vm::operation("httpIncomingDestroy"),
        "_destroy",
        exchange_keys,
    )?;
    let template_probes = incoming_template_probes(context, emitter, incoming_constructor)?;
    let incoming_factory = super::incoming::create_function(
        context,
        [
            incoming_constructor,
            emitter,
            incoming_read,
            incoming_destroy,
            server_exchange,
            client_exchange,
        ],
        template_probes,
    )?;
    let request_dispatch =
        context.evaluate_script_rooted(REQUEST_DISPATCH_FACTORY, "node:http/request-dispatch.js")?;
    let request_dispatch =
        context.call_rooted(request_dispatch, undefined, &[incoming_factory, respond])?;
    let request_dispatch = context.retain(request_dispatch)?;
    let incoming_factory = context.retain(incoming_factory)?;

    let initialize_server =
        context.host_function(crate::host::shared_vm::operation("httpCreateServer"))?;
    let request = super::client::request_function(context)?;
    let create_agent =
        context.host_function(crate::host::shared_vm::operation("httpAgentCreate"))?;
    let destroy_agent =
        context.host_function(crate::host::shared_vm::operation("httpAgentDestroy"))?;
    let factory = context.evaluate_script_rooted(MODULE_FACTORY, "node:http/shared-api.js")?;
    let module = context.call_rooted(
        factory,
        undefined,
        &[
            initialize_server,
            emitter,
            request,
            create_agent,
            destroy_agent,
            incoming_constructor,
            response_factory,
        ],
    )?;
    set(context, module, "STATUS_CODES", status_codes)?;
    let net = crate::host::shared_vm::commonjs::net_module(context)?;
    let net_server = get(context, net, "Server")?;
    let server = get(context, module, "Server")?;
    let net_prototype = get(context, net_server, "prototype")?;
    let server_prototype = get(context, server, "prototype")?;
    context.set_prototype_rooted(server, net_server)?;
    context.set_prototype_rooted(server_prototype, net_prototype)?;
    let key = context.string_rooted("Agent");
    let agent = context.get_property_rooted(module, key)?;
    let key = context.string_rooted("prototype");
    let prototype = context.get_property_rooted(agent, key)?;
    let native = context.host_function(crate::host::shared_vm::operation("getName"))?;
    let wrapper = context.evaluate_script_rooted(
        "native => function getName(options = undefined) { return native(options); }",
        "node:http/agent-get-name.js",
    )?;
    let get_name = context.call_rooted(wrapper, undefined, &[native])?;
    set(context, prototype, "getName", get_name)?;
    for (name, public_operation, original_operation) in [
        (
            "validateHeaderName",
            "httpValidateHeaderNamePublic",
            "httpValidateHeaderNameOriginal",
        ),
        (
            "validateHeaderValue",
            "httpValidateHeaderValuePublic",
            "httpValidateHeaderValueOriginal",
        ),
    ] {
        let public =
            context.host_constructor(crate::host::shared_vm::operation(public_operation))?;
        let original =
            context.host_function(crate::host::shared_vm::operation(original_operation))?;
        for (function, name) in [(public, name), (original, "")] {
            let key = context.string_rooted("name");
            let name = context.string_rooted(name);
            context.define_data_property_rooted(function, key, name, false, false, true)?;
        }
        set(context, public, "withoutStackTrace", original)?;
        set(context, module, name, public)?;
    }
    let methods = crate::modules::http_protocol::HTTP_METHODS
        .iter()
        .map(|method| context.string_rooted(method))
        .collect::<Vec<_>>();
    let methods = context.array_rooted(&methods)?;
    set(context, module, "METHODS", methods)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
        .request_dispatch = Some(request_dispatch);
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
        .incoming_factory = Some(incoming_factory);
    Ok(module)
}

pub(crate) fn https_module(context: &mut Context<'_>, http: RootId) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(HTTPS_FACTORY, "node:https.js")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[http])
}

pub(crate) fn create_server(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let packed = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("shared HTTP server arguments are missing"))?;
    let server = array_item(context, packed, 0)?;
    let listener = array_item(context, packed, 1)?;
    if context.is_callable_rooted(listener)? {
        let on_key = context.string_rooted("on");
        let on = context.get_property_rooted(server, on_key)?;
        let event = context.string_rooted("request");
        context.call_rooted(on, server, &[event, listener])?;
    }
    let id = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
        .server_id()
        .map_err(RootedError::host)?;
    let id_value = context.number(id as f64);
    for (name, operation) in [
        ("listen", "httpServerListen"),
        ("address", "httpServerAddress"),
        ("close", "httpServerClose"),
    ] {
        let function = context
            .host_function_with_data(crate::host::shared_vm::operation(operation), id_value)?;
        let function = if name == "close" {
            let factory = context
                .evaluate_script_rooted(SERVER_CLOSE_FACTORY, "node:http/server-close.js")?;
            let undefined = context.undefined();
            let wrapped = context.call_rooted(factory, undefined, &[function])?;
            context.release_root(factory);
            wrapped
        } else {
            function
        };
        set(context, server, name, function)?;
    }
    let unref = context.boolean(false);
    set(context, server, "_unref", unref)?;
    let handle = context.null();
    set(context, server, "_handle", handle)?;
    let retained = context.retain(server)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
        .servers
        .insert(
            id,
            super::state::Server {
                root: retained,
                listener: None,
                listening_pending: false,
                closing: false,
                connections: Default::default(),
            },
        );
    Ok(server)
}

/// Adopt an HTTP transport as a public Node net.Socket when the HTTP parser
/// encounters a protocol upgrade. The existing socket id stays attached to the
/// TCP transport; registering the socket transfers read delivery to net.Socket.
pub(crate) fn upgrade_socket(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = args
        .first()
        .copied()
        .ok_or_else(|| RootedError::host("HTTP upgrade socket id is missing"))?;
    let id_number = context
        .rooted_value(id)
        .and_then(Value::as_number)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| RootedError::host("HTTP upgrade socket id is invalid"))?
        as u64;
    let undefined = context.undefined();
    let handle =
        super::super::net_reference_shared_vm::create_socket_handle(context, undefined, &[id])?;
    let options = context.object_rooted()?;
    set(context, options, "handle", handle)?;
    let yes = context.boolean(true);
    set(context, options, "readable", yes)?;
    set(context, options, "writable", yes)?;
    let constructor = context
        .host_mut()
        .shared_state()
        .borrow()
        .net_socket_constructor
        .ok_or_else(|| RootedError::host("net.Socket constructor is unavailable"))?;
    let socket = context.construct_rooted(constructor, constructor, &[options])?;
    let id_value = context.number(id_number as f64);
    set(context, socket, "__quenchNetSocketId", id_value)?;
    super::super::net_shared_vm::register_socket(context, socket, id_number)?;
    Ok(socket)
}

pub(crate) fn server_listen(
    context: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = function_data_id(context)?;
    let first = args.first().copied();
    let first_is_callback = match first {
        Some(root) => context.is_callable_rooted(root)?,
        None => false,
    };
    let address = listen_address(context, first, first_is_callback, args.get(1).copied())?;
    let options =
        first.filter(|root| context.is_object_rooted(*root).unwrap_or(false) && !first_is_callback);
    let backlog = net_shared_vm::listen_backlog(context, options)?;
    let host = context.host_mut().shared_state();
    {
        let mut guard = host.borrow_mut();
        let Some(server) = guard.http.servers.get(&id) else {
            return Err(RootedError::host("shared HTTP server is closed"));
        };
        if server.listener.is_some() {
            return Err(RootedError::host("shared HTTP server is already listening"));
        }
        let listener = net_shared_vm::listen_with_backlog(&mut guard.tcp, address, backlog)
            .map_err(RootedError::host)?;
        let server = guard.http.servers.get_mut(&id).unwrap();
        server.listener = Some(listener);
        server.listening_pending = true;
    }
    let listener = host.borrow().http.servers[&id].listener.unwrap();
    crate::modules::net_reference_shared_vm::install(context, listener, receiver)?;
    let callback = if first_is_callback {
        first
    } else if let Some(second) = args.get(1).copied() {
        if context.is_callable_rooted(second)? {
            Some(second)
        } else {
            args.get(2).copied()
        }
    } else {
        args.get(2).copied()
    };
    if let Some(callback) = callback {
        if context.is_callable_rooted(callback)? {
            let on_key = context.string_rooted("on");
            let on = context.get_property_rooted(receiver, on_key)?;
            let event = context.string_rooted("listening");
            context.call_rooted(on, receiver, &[event, callback])?;
        }
    }
    Ok(receiver)
}

fn listen_address(
    context: &mut Context<'_>,
    first: Option<RootId>,
    first_is_callback: bool,
    second: Option<RootId>,
) -> Result<SocketAddr, RootedError> {
    let Some(root) = first.filter(|_| !first_is_callback) else {
        return resolve_listen_address(DEFAULT_LISTEN_HOST, 0);
    };
    let value = context
        .rooted_value(root)
        .ok_or_else(|| RootedError::host("shared HTTP listen argument is unavailable"))?;
    if let Some(port) = value.as_number() {
        return resolve_listen_address(&listen_host(context, second)?, numeric_port(port)?);
    }
    if let Some(port) = context.string_text(root)? {
        return resolve_listen_address(&listen_host(context, second)?, string_port(&port)?);
    }
    if value.is_null() || value.as_bool().is_some() {
        return Err(invalid_listen_port());
    }
    let key = context.string_rooted("port");
    let port = context.get_property_rooted(root, key)?;
    let port = listen_port(context, port)?;
    let host_key = context.string_rooted("host");
    let host = context.get_property_rooted(root, host_key)?;
    let host = match context.rooted_value(host) {
        Some(value) if value.is_undefined() || value.is_null() => DEFAULT_LISTEN_HOST.to_owned(),
        _ => context.string_text(host)?.ok_or_else(invalid_listen_port)?,
    };
    resolve_listen_address(&host, port)
}

fn listen_host(context: &mut Context<'_>, value: Option<RootId>) -> Result<String, RootedError> {
    let Some(value) = value else {
        return Ok(DEFAULT_LISTEN_HOST.to_owned());
    };
    if context.is_callable_rooted(value)? {
        return Ok(DEFAULT_LISTEN_HOST.to_owned());
    }
    match context.rooted_value(value) {
        Some(value) if value.is_undefined() || value.is_null() => {
            Ok(DEFAULT_LISTEN_HOST.to_owned())
        }
        Some(value) if value.as_bool().is_some() || value.as_number().is_some() => {
            Err(RootedError::host("invalid HTTP listen host"))
        }
        _ => context
            .string_text(value)?
            .ok_or_else(|| RootedError::host("invalid HTTP listen host")),
    }
}

fn listen_port(context: &mut Context<'_>, root: RootId) -> Result<u16, RootedError> {
    let value = context
        .rooted_value(root)
        .ok_or_else(|| RootedError::host("shared HTTP listen port is unavailable"))?;
    if value.is_undefined() {
        return Ok(0);
    }
    if let Some(port) = value.as_number() {
        return numeric_port(port);
    }
    if let Some(port) = context.string_text(root)? {
        return string_port(&port);
    }
    Err(invalid_listen_port())
}

fn resolve_listen_address(host: &str, port: u16) -> Result<SocketAddr, RootedError> {
    (host, port)
        .to_socket_addrs()
        .map_err(|error| RootedError::host(format!("cannot resolve HTTP listen host: {error}")))?
        .next()
        .ok_or_else(|| RootedError::host("HTTP listen host resolved to no addresses"))
}

fn numeric_port(port: f64) -> Result<u16, RootedError> {
    if port.is_finite() && port.fract() == 0.0 && (0.0..=u16::MAX as f64).contains(&port) {
        Ok(port as u16)
    } else {
        Err(invalid_listen_port())
    }
}

fn string_port(port: &str) -> Result<u16, RootedError> {
    port.parse().map_err(|_| invalid_listen_port())
}

fn invalid_listen_port() -> RootedError {
    RootedError::host("shared HTTP listen requires a numeric port or { port, host } options")
}

pub(crate) fn server_address(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = function_data_id(context)?;
    let host = context.host_mut().shared_state();
    let address = {
        let guard = host.borrow();
        guard
            .http
            .servers
            .get(&id)
            .and_then(|server| server.listener)
            .and_then(|listener| net_shared_vm::address(&guard.tcp, listener))
    };
    let Some(address) = address else {
        return Ok(context.null());
    };
    let object = context.object_rooted()?;
    let address_value = context.string_rooted(&address.ip().to_string());
    set(context, object, "address", address_value)?;
    let family = context.string_rooted(match address.ip() {
        IpAddr::V4(_) => "IPv4",
        IpAddr::V6(_) => "IPv6",
    });
    set(context, object, "family", family)?;
    let port = context.number(address.port() as f64);
    set(context, object, "port", port)?;
    Ok(object)
}

pub(crate) fn server_close(
    context: &mut Context<'_>,
    receiver: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = function_data_id(context)?;
    let host = context.host_mut().shared_state();
    {
        let mut guard = host.borrow_mut();
        let mut idle_sockets = Vec::new();
        let listener = guard.http.servers.get_mut(&id).and_then(|server| {
            server.closing = true;
            server.listening_pending = false;
            idle_sockets.extend(server.connections.iter().copied());
            server.listener.take()
        });
        if let Some(listener) = listener {
            net_shared_vm::close_listener(&mut guard.tcp, listener);
        }
        idle_sockets.retain(|socket| {
            !guard
                .http
                .connections
                .get(socket)
                .is_some_and(|connection| connection.upgraded)
                && guard
                    .http
                    .responses
                    .values()
                    .filter(|response| response.socket == *socket)
                    .all(|response| response.lifecycle.is_terminal())
        });
        for socket in idle_sockets {
            // A response can be terminal while its bytes are still queued in
            // the shared transport. Half-close after the queued writes drain
            // so server.close() cannot discard a just-finished response.
            net_shared_vm::end(&mut guard.tcp, socket).map_err(RootedError::host)?;
        }
    }
    let handle = context.null();
    set(context, receiver, "_handle", handle)?;
    Ok(receiver)
}

pub(crate) fn agent_create(
    context: &mut Context<'_>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let id = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
        .agent_id()
        .map_err(RootedError::host)?;
    Ok(context.number(id as f64))
}

/// Node's pooling key, preserving property read order and guest coercions.
pub(crate) fn agent_get_name(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let options = match args.first().copied() {
        Some(value) if !context.rooted_value(value).is_some_and(Value::is_undefined) => value,
        _ => context.null_object_rooted()?,
    };
    let get = |context: &mut Context<'_>, name: &str| {
        let key = context.string_rooted(name);
        context.get_property_rooted(options, key)
    };
    let host = get(context, "host")?;
    let mut name = if context.truthy_rooted(host)? {
        host
    } else {
        context.string_rooted("localhost")
    };
    for property in ["port", "localAddress"] {
        let colon = context.string_rooted(":");
        name = context.arithmetic_rooted(NativeArithmetic::Add, name, colon)?;
        let value = get(context, property)?;
        if context.truthy_rooted(value)? {
            let value = get(context, property)?;
            name = context.arithmetic_rooted(NativeArithmetic::Add, name, value)?;
        }
    }
    let family = get(context, "family")?;
    let ipv4 = context.rooted_value(family).and_then(Value::as_number) == Some(4.0);
    let ipv6 = if ipv4 {
        false
    } else {
        let family = get(context, "family")?;
        context.rooted_value(family).and_then(Value::as_number) == Some(6.0)
    };
    if ipv4 || ipv6 {
        let family = get(context, "family")?;
        let text = context.to_string(family)?;
        let suffix = context.string_rooted(&format!(":{text}"));
        name = context.arithmetic_rooted(NativeArithmetic::Add, name, suffix)?;
    }
    let socket = get(context, "socketPath")?;
    if context.truthy_rooted(socket)? {
        let socket = get(context, "socketPath")?;
        let text = context.to_string(socket)?;
        let suffix = context.string_rooted(&format!(":{text}"));
        name = context.arithmetic_rooted(NativeArithmetic::Add, name, suffix)?;
    }
    Ok(name)
}

pub(crate) fn agent_destroy(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(agent) = args
        .first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64)
    else {
        return Ok(context.undefined());
    };
    let aborted_clients = {
        let host = context.host_mut().shared_state();
        let mut guard = host.borrow_mut();
        let Some(sockets) = guard.http.agents.remove(&agent) else {
            return Ok(context.undefined());
        };
        let mut aborted_clients = Vec::new();
        for socket in sockets {
            let response_is_complete = guard
                .http
                .clients
                .get(&socket)
                .is_some_and(|client| client.response_parser.is_complete());
            if response_is_complete {
                // The poller has parsed the full response and may currently be
                // emitting its head or body. Let that terminal transition finish;
                // it will close the now-unowned socket after guest callbacks return.
                continue;
            }
            net_shared_vm::close_socket(&mut guard.tcp, socket);
            if let Some(client) = guard.http.clients.remove(&socket) {
                aborted_clients.push(client);
            }
            guard
                .http
                .responses
                .retain(|_, response| response.socket != socket);
        }
        aborted_clients
    };
    let mut first_error = None;
    for client in aborted_clients {
        if let Err(error) = super::client::abort_exchange(context, client) {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(context.undefined())
}

pub(crate) fn response_set_header(
    context: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let name_root = argument(context, args.get(1).copied());
    let value_root = argument(context, args.get(2).copied());
    let name = header_name(context, name_root)?;
    let multiple = args
        .get(3)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let values = header_values(context, value_root, multiple, &name)?;
    let lifecycle = context
        .host_mut()
        .shared_state()
        .borrow()
        .http
        .responses
        .get(&id)
        .map(|response| response.lifecycle);
    if lifecycle.is_some_and(|state| state != super::state::ResponseLifecycle::Open) {
        return throw_response_error(
            context,
            false,
            "ERR_HTTP_HEADERS_SENT",
            "Cannot set headers after they are sent to the client",
        );
    }
    let state = context.host_mut().shared_state();
    let mut host = state.borrow_mut();
    let response = host
        .http
        .responses
        .get_mut(&id)
        .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
    response
        .headers
        .retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
    let values = limit_header_values(&name, values);
    response
        .headers
        .extend(values.into_iter().map(|value| (name.clone(), value)));
    Ok(receiver)
}

pub(crate) fn response_remove_header(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let name_root = argument(context, args.get(1).copied());
    let name = context.to_string(name_root)?;
    let state = context.host_mut().shared_state();
    let mut host = state.borrow_mut();
    if let Some(response) = host.http.responses.get_mut(&id) {
        if response.lifecycle != super::state::ResponseLifecycle::Open {
            drop(host);
            return throw_response_error(
                context,
                false,
                "ERR_HTTP_HEADERS_SENT",
                "Cannot remove headers after they are sent to the client",
            );
        }
        response
            .headers
            .retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
        if name.eq_ignore_ascii_case("date") {
            response.send_date = false;
        }
    }
    Ok(context.undefined())
}

pub(crate) fn response_get_header(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let name_root = argument(context, args.get(1).copied());
    let name = context.to_string(name_root)?;
    let values = context
        .host_mut()
        .shared_state()
        .borrow()
        .http
        .responses
        .get(&id)
        .map(|response| {
            response
                .headers
                .iter()
                .filter(|(header, _)| header.eq_ignore_ascii_case(&name))
                .map(|(_, value)| value.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    match values.len() {
        0 => Ok(context.undefined()),
        1 => Ok(context.string_rooted(&values[0])),
        _ => {
            let values = values
                .iter()
                .map(|value| context.string_rooted(value))
                .collect::<Vec<_>>();
            context.array_rooted(&values)
        }
    }
}

pub(crate) fn response_get_headers(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let headers = context
        .host_mut()
        .shared_state()
        .borrow()
        .http
        .responses
        .get(&id)
        .map(|response| response.headers.clone())
        .unwrap_or_default();
    let object = context.null_object_rooted()?;
    let mut grouped: Vec<(String, Vec<String>)> = Vec::new();
    for (header_name, header_value) in headers {
        let name = header_name.to_ascii_lowercase();
        if let Some((_, values)) = grouped
            .iter_mut()
            .find(|(existing_name, _)| existing_name == &name)
        {
            values.push(header_value);
        } else {
            grouped.push((name, vec![header_value]));
        }
    }
    for (name, values) in grouped {
        let value = if values.len() == 1 && name != "set-cookie" {
            context.string_rooted(&values[0])
        } else {
            let values = values
                .iter()
                .map(|value| context.string_rooted(value))
                .collect::<Vec<_>>();
            context.array_rooted(&values)?
        };
        set(context, object, &name, value)?;
    }
    Ok(object)
}

pub(crate) fn response_initialize(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument(context, args.first().copied());
    let response = argument(context, args.get(1).copied());
    let requested_id = argument_id(context, Some(id))?;
    let owner = context.host_weak_root(response)?;
    let (numeric_id, replaced) = {
        let state = context.host_mut().shared_state();
        let mut host = state.borrow_mut();
        let numeric_id = if requested_id == 0 && !host.http.responses.contains_key(&requested_id) {
            let id = host.http.response_id().map_err(RootedError::host)?;
            host.http.responses.insert(
                id,
                super::state::Response {
                    socket: 0,
                    virtual_response: true,
                    async_id: 0,
                    headers: Vec::new(),
                    body: Default::default(),
                    send_date: true,
                    http10: false,
                    use_chunked_encoding_by_default: true,
                    lifecycle: super::state::ResponseLifecycle::Open,
                    owner: None,
                },
            );
            id
        } else {
            requested_id
        };
        let replaced = if let Some(record) = host.http.responses.get_mut(&numeric_id) {
            record.owner.replace(owner)
        } else {
            Some(owner)
        };
        (numeric_id, replaced)
    };
    if let Some(stale) = replaced {
        context.release_host_weak_root(stale);
    }
    let tracking = context
        .host_mut()
        .shared_state()
        .borrow()
        .http
        .response_tracking
        .ok_or_else(|| RootedError::host("HTTP response tracking is unavailable"))?;
    let exchange_id = context.number(numeric_id as f64);
    context.call_rooted(
        tracking.map_set,
        tracking.responses,
        &[response, exchange_id],
    )?;
    Ok(exchange_id)
}

fn response_chunk_bytes(
    context: &mut Context<'_>,
    chunk: RootId,
    encoding: Option<RootId>,
) -> Result<Vec<u8>, RootedError> {
    let bytes = match context.view_bytes_rooted(chunk) {
        Some(bytes) => bytes,
        None => {
            let encoding = match encoding {
                Some(value) if context.truthy_rooted(value)? => context.to_string(value)?,
                _ => "utf8".to_owned(),
            };
            let Some(encoding_name) = crate::modules::buffer_codec::canonical_encoding(&encoding)
            else {
                return throw_response_error(
                    context,
                    true,
                    "ERR_UNKNOWN_ENCODING",
                    &format!("Unknown encoding: {encoding}"),
                );
            };
            let units = context.to_string_code_units(chunk)?;
            crate::modules::buffer_codec::encode_units(&units, encoding_name)
        }
    };
    Ok(bytes)
}

pub(crate) fn response_send(
    context: &mut Context<'_>,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let responses = context.host_function_data()?;
    let id = context.weak_map_get_rooted(responses, receiver)?;
    let id = argument_id(context, Some(id))?;
    let chunk = argument(context, args.first().copied());
    let bytes = response_chunk_bytes(context, chunk, args.get(1).copied())?;
    let shared = context.host_mut().shared_state();
    let socket = shared
        .borrow()
        .http
        .responses
        .get(&id)
        .map(|response| response.socket);
    let Some(socket) = socket else {
        return Ok(context.boolean(false));
    };
    if !bytes.is_empty() {
        net_shared_vm::write(&mut shared.borrow_mut().tcp, socket, &bytes)
            .map_err(RootedError::host)?;
    }
    if let Some(callback) = args.get(2).copied() {
        if context.is_callable_rooted(callback)? {
            crate::modules::process_shared_vm::next_tick(context, receiver, &[callback])?;
        }
    }
    Ok(context.boolean(true))
}

// Public sendDate is consulted when headers are serialized, not after a response
// has already committed them. Header removal can independently suppress it.
fn response_send_date(
    context: &mut Context<'_>,
    id: u64,
    response: Option<RootId>,
) -> Result<(), RootedError> {
    let shared = context.host_mut().shared_state();
    let open = shared
        .borrow()
        .http
        .responses
        .get(&id)
        .is_some_and(|response| response.lifecycle == super::state::ResponseLifecycle::Open);
    if open {
        if let Some(response) = response {
            let key = context.string_rooted("sendDate");
            let value = context.get_property_rooted(response, key)?;
            let enabled = context.truthy_rooted(value)?;
            if !enabled {
                if let Some(response) = shared.borrow_mut().http.responses.get_mut(&id) {
                    response.send_date = false;
                }
            }
        }
    }
    Ok(())
}

fn response_status(
    context: &mut Context<'_>,
    id: u64,
    response: Option<RootId>,
    status: Option<RootId>,
    reason: Option<RootId>,
) -> Result<(u16, Vec<u8>), RootedError> {
    let open = context
        .host_mut()
        .shared_state()
        .borrow()
        .http
        .responses
        .get(&id)
        .is_some_and(|response| response.lifecycle == super::state::ResponseLifecycle::Open);
    if !open {
        return Ok((DEFAULT_STATUS, Vec::new()));
    }
    let response = argument(context, response);
    let status = argument(context, status);
    let reason = argument(context, reason);
    let status = crate::modules::http_status_shared_vm::prepare(
        context,
        response,
        status,
        crate::modules::http_status_shared_vm::Reason::Existing(reason),
    )?;
    let reason = crate::modules::http_status_shared_vm::encode(context, response, status)?;
    Ok((status, reason))
}

pub(crate) fn response_write(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let chunk_root = argument(context, args.get(1).copied());
    let explicit = args
        .get(2)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let state = context.host_mut().shared_state();
    let lifecycle = state
        .borrow()
        .http
        .responses
        .get(&id)
        .map(|response| response.lifecycle)
        .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
    // A view streamed after the head is framed straight into the socket's queue; other
    // chunks are encoded first, before the lifecycle check, as Node validates them.
    let streamed_view = explicit
        && lifecycle != super::state::ResponseLifecycle::Open
        && context.with_view_bytes_rooted(chunk_root, |_| ()).is_some();
    let chunk = if streamed_view {
        Vec::new()
    } else {
        response_chunk_bytes(context, chunk_root, args.get(6).copied())?
    };
    if lifecycle.is_terminal() {
        return throw_response_error(
            context,
            false,
            "ERR_STREAM_WRITE_AFTER_END",
            "write after end",
        );
    }
    if !explicit {
        let mut host = state.borrow_mut();
        let response = host
            .http
            .responses
            .get_mut(&id)
            .expect("response checked active above");
        response.body.push(chunk, false);
        return Ok(context.boolean(true));
    }
    let (status, status_message) = response_status(
        context,
        id,
        args.get(5).copied(),
        args.get(3).copied(),
        args.get(4).copied(),
    )?;
    response_send_date(context, id, args.get(5).copied())?;
    let mut host = state.borrow_mut();
    let response = host
        .http
        .responses
        .get_mut(&id)
        .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
    if response.virtual_response {
        if response.lifecycle == super::state::ResponseLifecycle::Open {
            enable_streaming_framing(
                &mut response.headers,
                response.use_chunked_encoding_by_default,
            );
            if invalid_trailer(&response.headers) {
                drop(host);
                return throw_invalid_trailer(context);
            }
            response.lifecycle.send_headers();
        }
        drop(host);
        return Ok(context.boolean(true));
    }
    let socket = response.socket;
    let chunked = is_chunked(&response.headers);
    if response.lifecycle == super::state::ResponseLifecycle::Open {
        enable_streaming_framing(
            &mut response.headers,
            response.use_chunked_encoding_by_default,
        );
        if invalid_trailer(&response.headers) {
            drop(host);
            return throw_invalid_trailer(context);
        }
        response.lifecycle.send_headers();
        let head = crate::modules::http_protocol::compose(
            status,
            &status_message,
            &response.headers,
            &chunk,
            &[],
            !response.http10,
            response.http10,
            response.send_date,
            std::time::SystemTime::now(),
        );
        net_shared_vm::write(&mut host.tcp, socket, &head).map_err(RootedError::host)?;
    } else {
        let mut write = |body: &[u8]| {
            use crate::modules::http_protocol::{CHUNK_FRAME_END, ChunkFrameHead};
            if chunked && !body.is_empty() {
                let head = ChunkFrameHead::new(body.len());
                net_shared_vm::write_parts(&mut host.tcp, socket, [head.bytes(), body, CHUNK_FRAME_END])
            } else if chunked {
                Ok(())
            } else {
                net_shared_vm::write_parts(&mut host.tcp, socket, [body])
            }
        };
        let written = if streamed_view {
            context
                .with_view_bytes_rooted(chunk_root, &mut write)
                .unwrap_or_else(|| write(&[]))
        } else {
            write(&chunk)
        };
        written.map_err(RootedError::host)?;
    }
    net_shared_vm::try_flush(&mut host.tcp, socket);
    let below_mark = net_shared_vm::queued_bytes(&host.tcp, socket)
        .is_none_or(|queued| queued < RESPONSE_HIGH_WATER_MARK);
    if !below_mark && !host.http.drain_waiters.contains_key(&socket) {
        drop(host);
        let response = argument(context, args.get(5).copied());
        let waiter = context.retain(response)?;
        state.borrow_mut().http.drain_waiters.insert(socket, waiter);
    }
    Ok(context.boolean(below_mark))
}

pub(crate) fn response_write_head(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let lifecycle = context
        .host_mut()
        .shared_state()
        .borrow()
        .http
        .responses
        .get(&id)
        .map(|response| response.lifecycle);
    if lifecycle.is_some_and(|state| state != super::state::ResponseLifecycle::Open) {
        return throw_response_error(
            context,
            false,
            "ERR_HTTP_HEADERS_SENT",
            "Cannot write headers after they are sent to the client",
        );
    }
    let status_root = argument(context, args.get(1).copied());
    let message_root = argument(context, args.get(2).copied());
    let response_root = argument(context, args.get(5).copied());
    let status = crate::modules::http_status_shared_vm::prepare(
        context,
        response_root,
        status_root,
        crate::modules::http_status_shared_vm::Reason::WriteHead(message_root),
    )?;
    let invalid_headers = args
        .get(4)
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if invalid_headers {
        return throw_response_error(
            context,
            true,
            "ERR_INVALID_ARG_VALUE",
            "The argument 'headers' is invalid",
        );
    }
    let entry_root = argument(context, args.get(3).copied());
    let entries = write_head_entries(context, entry_root)?;
    let status_message =
        crate::modules::http_status_shared_vm::encode(context, response_root, status)?;

    response_send_date(context, id, args.get(5).copied())?;
    let state = context.host_mut().shared_state();
    let (socket, bytes, virtual_response) = {
        let mut host = state.borrow_mut();
        let response = host
            .http
            .responses
            .get_mut(&id)
            .ok_or_else(|| RootedError::host("shared HTTP response is no longer active"))?;
        for (name, values) in entries {
            response
                .headers
                .retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
            let values = limit_header_values(&name, values);
            response
                .headers
                .extend(values.into_iter().map(|value| (name.clone(), value)));
        }
        enable_streaming_framing(
            &mut response.headers,
            response.use_chunked_encoding_by_default,
        );
        if invalid_trailer(&response.headers) {
            drop(host);
            return throw_invalid_trailer(context);
        }
        response.lifecycle.send_headers();
        let virtual_response = response.virtual_response;
        let bytes = crate::modules::http_protocol::compose(
            status,
            &status_message,
            &response.headers,
            &[],
            &[],
            !response.http10,
            response.http10,
            response.send_date,
            std::time::SystemTime::now(),
        );
        (response.socket, bytes, virtual_response)
    };
    if virtual_response {
        let header = String::from_utf8_lossy(&bytes);
        return Ok(context.string_rooted(&header));
    }
    net_shared_vm::write(
        &mut context.host_mut().shared_state().borrow_mut().tcp,
        socket,
        &bytes,
    )
    .map_err(RootedError::host)?;
    Ok(context.undefined())
}

pub(crate) fn response_finish(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let (status, status_message) = response_status(
        context,
        id,
        args.get(4).copied(),
        args.get(1).copied(),
        args.get(2).copied(),
    )?;
    response_send_date(context, id, args.get(4).copied())?;
    let virtual_response = {
        let state = context.host_mut().shared_state();
        let mut host = state.borrow_mut();
        match host.http.responses.get_mut(&id) {
            Some(response) if response.virtual_response => {
                response.lifecycle.end();
                true
            }
            _ => false,
        }
    };
    if virtual_response {
        return Ok(context.undefined());
    }
    let response = {
        let state = context.host_mut().shared_state();
        let mut host = state.borrow_mut();
        let Some(response) = host.http.responses.get_mut(&id) else {
            // A peer close may retire the host-side response between an
            // end() call and a later idempotent end() from Node middleware.
            drop(host);
            return Ok(context.undefined());
        };
        let headers_sent = response.lifecycle == super::state::ResponseLifecycle::HeadersSent;
        if !headers_sent
            && response
                .headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("trailer"))
        {
            enable_streaming_framing(
                &mut response.headers,
                response.use_chunked_encoding_by_default,
            );
            if invalid_trailer(&response.headers) {
                drop(host);
                return throw_invalid_trailer(context);
            }
        }
        if !response.lifecycle.end() {
            None
        } else {
            Some((
                response.socket,
                response.async_id,
                status,
                status_message.clone(),
                response.headers.clone(),
                headers_sent,
                // An ended response never writes again: its body leaves with this frame.
                std::mem::take(&mut response.body).bytes(),
                is_chunked(&response.headers),
                response.send_date,
                response.http10,
            ))
        }
    };
    let Some((
        socket,
        async_id,
        status,
        status_message,
        headers,
        headers_sent,
        body,
        chunked,
        send_date,
        http10,
    )) = response
    else {
        return Ok(context.undefined());
    };
    let close_after_response = (http10
        && !chunked
        && !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-length")))
        || headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("connection") && value.eq_ignore_ascii_case("close")
        });
    let bytes = if headers_sent {
        if chunked {
            let mut framed = crate::modules::http_protocol::chunk_frame(&body);
            framed.extend_from_slice(&crate::modules::http_protocol::chunk_terminator(&[]));
            framed
        } else {
            body
        }
    } else {
        let chunked = is_chunked(&headers);
        let mut framed = crate::modules::http_protocol::compose(
            status,
            &status_message,
            &headers,
            &body,
            &[],
            !http10,
            http10,
            send_date,
            std::time::SystemTime::now(),
        );
        if chunked {
            framed.extend_from_slice(&crate::modules::http_protocol::chunk_terminator(&[]));
        }
        framed
    };
    if !bytes.is_empty() {
        net_shared_vm::write(
            &mut context.host_mut().shared_state().borrow_mut().tcp,
            socket,
            &bytes,
        )
        .map_err(RootedError::host)?;
    }
    if close_after_response {
        net_shared_vm::end(
            &mut context.host_mut().shared_state().borrow_mut().tcp,
            socket,
        )
        .map_err(RootedError::host)?;
    } else if let Some(connection) = context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .http
        .connections
        .get_mut(&socket)
    {
        connection.received.clear();
        connection.request_dispatched = false;
    }
    if let (Some(request), Some(response), Some(server), Some(socket)) = (
        args.get(3).copied(),
        args.get(4).copied(),
        args.get(5).copied(),
        args.get(6).copied(),
    ) {
        let shared_state = context.host_mut().shared_state();
        // The finish message is made only for a channel with subscribers, as Node's server
        // publishes it only then (`hasSubscribers`).
        let subscribed = crate::modules::diagnostics_channel_shared_vm::channel_has_subscribers(
            &shared_state.borrow(),
            RESPONSE_FINISH_CHANNEL,
        );
        let result = if subscribed {
            let message = context.object_rooted()?;
            set(context, message, "request", request)?;
            set(context, message, "response", response)?;
            set(context, message, "server", server)?;
            set(context, message, "socket", socket)?;
            let _scope =
                crate::modules::async_hooks_shared_vm::enter_context(&shared_state, async_id);
            crate::modules::diagnostics_channel_shared_vm::publish_named(
                context,
                RESPONSE_FINISH_CHANNEL,
                message,
            )
        } else {
            Ok(())
        };
        for root in
            crate::modules::async_hooks_shared_vm::take_context_stores(&shared_state, async_id)
        {
            context.release_root(root);
        }
        result?;
    }
    Ok(context.undefined())
}

/// The diagnostics channel a server's response finish publishes to.
const RESPONSE_FINISH_CHANNEL: &str = "http.server.response.finish";

/// The finalization callback of a collected response object: a finished response's host
/// entry goes with it. An unfinished one stays until its socket closes.
pub(crate) fn response_destroy(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = argument_id(context, args.first().copied())?;
    let shared_state = context.host_mut().shared_state();
    let mut host = shared_state.borrow_mut();
    let (socket, async_id, owner, virtual_response) = {
        let Some(response) = host.http.responses.remove(&id) else {
            return Ok(context.undefined());
        };
        let mut response = response;
        if !response.lifecycle.destroy() {
            host.http.responses.insert(id, response);
            return Ok(context.undefined());
        }
        (
            response.socket,
            response.async_id,
            response.owner,
            response.virtual_response,
        )
    };
    if virtual_response {
        drop(host);
        if let Some(owner) = owner {
            context.release_host_weak_root(owner);
        }
        return Ok(context.undefined());
    }
    let server_id = host
        .http
        .connections
        .remove(&socket)
        .map(|connection| connection.server);
    if let Some(server_id) = server_id {
        if let Some(server) = host.http.servers.get_mut(&server_id) {
            server.connections.remove(&socket);
        }
    }
    net_shared_vm::close_socket(&mut host.tcp, socket);
    drop(host);
    if let Some(owner) = owner {
        context.release_host_weak_root(owner);
    }
    for root in crate::modules::async_hooks_shared_vm::retire_context(&shared_state, async_id) {
        context.release_root(root);
    }
    Ok(context.undefined())
}

fn enable_streaming_framing(
    headers: &mut Vec<(String, String)>,
    use_chunked_encoding_by_default: bool,
) {
    let has_length = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-length"));
    let has_transfer_encoding = headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"));
    if use_chunked_encoding_by_default && !has_length && !has_transfer_encoding {
        headers.push(("Transfer-Encoding".to_owned(), "chunked".to_owned()));
    }
}

fn is_chunked(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("transfer-encoding") && value.eq_ignore_ascii_case("chunked")
    })
}

fn invalid_trailer(headers: &[(String, String)]) -> bool {
    headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("trailer"))
        && !is_chunked(headers)
}

fn throw_invalid_trailer<T>(context: &mut Context<'_>) -> Result<T, RootedError> {
    throw_response_error(
        context,
        false,
        "ERR_HTTP_TRAILER_INVALID",
        "Trailers are invalid with this transfer encoding",
    )
}

fn function_data_id(context: &mut Context<'_>) -> Result<u64, RootedError> {
    let data = context.host_function_data()?;
    context
        .rooted_value(data)
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("invalid shared HTTP operation identifier"))
}

fn argument_id(context: &mut Context<'_>, root: Option<RootId>) -> Result<u64, RootedError> {
    root.and_then(|root| context.rooted_value(root))
        .and_then(Value::as_number)
        .filter(|id| id.is_finite() && *id >= 0.0)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("invalid shared HTTP response identifier"))
}

fn argument(context: &mut Context<'_>, root: Option<RootId>) -> RootId {
    root.unwrap_or_else(|| context.undefined())
}

fn header_name(context: &mut Context<'_>, root: RootId) -> Result<String, RootedError> {
    let name = context.string_text(root)?;
    let Some(name) = name else {
        let display = context.to_string(root)?;
        return throw_header_issue(
            context,
            crate::modules::http_protocol::HeaderIssue::InvalidName(display),
        );
    };
    if name.is_empty()
        || !name
            .chars()
            .all(crate::modules::http_protocol::is_http_token_char)
    {
        return throw_header_issue(
            context,
            crate::modules::http_protocol::HeaderIssue::InvalidName(name),
        );
    }
    Ok(name)
}

fn header_values(
    context: &mut Context<'_>,
    root: RootId,
    multiple: bool,
    name: &str,
) -> Result<Vec<String>, RootedError> {
    let values = if multiple {
        let items = context.array_like_values_rooted(root)?;
        let mut values = Vec::with_capacity(items.len());
        for item in items {
            values.push(header_value(context, item, name)?);
        }
        values
    } else {
        vec![header_value(context, root, name)?]
    };
    Ok(values)
}

fn header_value(
    context: &mut Context<'_>,
    root: RootId,
    name: &str,
) -> Result<String, RootedError> {
    if context.rooted_value(root).is_some_and(Value::is_undefined) {
        let issue = crate::modules::http_protocol::HeaderIssue::MissingValue(name.to_owned());
        return throw_header_issue(context, issue);
    }
    let value = context.to_string(root)?;
    if !crate::modules::http_protocol::valid_header_value(&value) {
        let issue = crate::modules::http_protocol::HeaderIssue::InvalidContent(name.to_owned());
        return throw_header_issue(context, issue);
    }
    Ok(value)
}

fn write_head_entries(
    context: &mut Context<'_>,
    root: RootId,
) -> Result<Vec<(String, Vec<String>)>, RootedError> {
    let items = context.array_like_values_rooted(root)?;
    let entries = items.chunks_exact(HEAD_ENTRY_FIELDS);
    if !entries.remainder().is_empty() {
        return Err(RootedError::host(
            "shared HTTP header entries are malformed",
        ));
    }
    let mut result = Vec::with_capacity(entries.len());
    for entry in entries {
        let &[name_root, values_root, multiple_root] = entry else {
            unreachable!("chunks_exact yields entries of HEAD_ENTRY_FIELDS values");
        };
        let name = header_name(context, name_root)?;
        let multiple = context
            .rooted_value(multiple_root)
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let values = header_values(context, values_root, multiple, &name)?;
        result.push((name, values));
    }
    Ok(result)
}

/// A packed head entry's values: its name, its value or values, and whether those are a list.
const HEAD_ENTRY_FIELDS: usize = 3;

fn array_item(
    context: &mut Context<'_>,
    array: RootId,
    index: usize,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(&index.to_string());
    context.get_property_rooted(array, key)
}

fn limit_header_values(name: &str, mut values: Vec<String>) -> Vec<String> {
    if crate::modules::http_protocol::NON_REPEATABLE_HEADERS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
    {
        values.truncate(1);
    }
    values
}

fn coded_error_rooted(
    context: &mut Context<'_>,
    type_error: bool,
    code: &str,
    message: &str,
) -> Result<RootId, RootedError> {
    let error = if type_error {
        context.type_error_rooted(message)?
    } else {
        context.error_rooted(message)?
    };
    let code_value = context.string_rooted(code);
    set(context, error, "code", code_value)?;
    Ok(error)
}

fn throw_header_issue(
    context: &mut Context<'_>,
    issue: crate::modules::http_protocol::HeaderIssue,
) -> Result<String, RootedError> {
    let error = coded_error_rooted(context, true, issue.code(), &issue.message())?;
    Err(context.throw(error))
}

fn throw_response_error<T>(
    context: &mut Context<'_>,
    type_error: bool,
    code: &str,
    message: &str,
) -> Result<T, RootedError> {
    let error = coded_error_rooted(context, type_error, code, message)?;
    Err(context.throw(error))
}

fn set(
    context: &mut Context<'_>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    if context.set_named_property_rooted(object, name, value)? {
        Ok(())
    } else {
        Err(RootedError::host(format!(
            "cannot install shared http property {name}"
        )))
    }
}

pub(crate) fn validate_header_name(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let name = argument(context, args.first().copied());
    let text = context.string_text(name)?;
    if text.as_ref().is_some_and(|name| {
        !name.is_empty()
            && name
                .chars()
                .all(crate::modules::http_protocol::is_http_token_char)
    }) {
        return Ok(context.undefined());
    }
    let label = argument(context, args.get(1).copied());
    let label = if context.truthy_rooted(label)? {
        label
    } else {
        context.string_rooted("Header name")
    };
    formatted_header_error(
        context,
        "ERR_INVALID_HTTP_TOKEN",
        "%s must be a valid HTTP token [\"%s\"]",
        &[label, name],
    )
}

pub(crate) fn validate_header_value(
    context: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let name = argument(context, args.first().copied());
    let value = argument(context, args.get(1).copied());
    if context.rooted_value(value).is_some_and(Value::is_undefined) {
        return formatted_header_error(
            context,
            "ERR_HTTP_INVALID_HEADER_VALUE",
            "Invalid value \"%s\" for header \"%s\"",
            &[value, name],
        );
    }
    let lenient = argument(context, args.get(2).copied());
    let lenient = context.truthy_rooted(lenient)?;
    let units = context.to_string_code_units(value)?;
    if !units
        .iter()
        .all(|unit| crate::modules::http_protocol::valid_header_unit(*unit, lenient))
    {
        let mut message: Vec<u16> = "Invalid character in header content"
            .encode_utf16()
            .collect();
        if !context.rooted_value(name).is_some_and(Value::is_undefined) {
            // ERR_INVALID_CHAR uses template-string coercion rather than %s.
            message.extend(" [\"".encode_utf16());
            message.extend(context.to_string_code_units(name)?);
            message.extend("\"]".encode_utf16());
        }
        let message = context.string_units_rooted(&message);
        return throw_header_message(context, "ERR_INVALID_CHAR", message);
    }
    Ok(context.undefined())
}

fn formatted_header_error(
    context: &mut Context<'_>,
    code: &str,
    template: &str,
    arguments: &[RootId],
) -> Result<RootId, RootedError> {
    let formatters = crate::modules::util_shared_vm::internal_formatters(context)?;
    let key = context.string_rooted("format");
    let format = context.get_property_rooted(formatters, key)?;
    let template = context.string_rooted(template);
    let mut arguments_with_template = Vec::with_capacity(arguments.len() + 1);
    arguments_with_template.push(template);
    arguments_with_template.extend_from_slice(arguments);
    let receiver = context.undefined();
    let message = context.call_rooted(format, receiver, &arguments_with_template)?;
    throw_header_message(context, code, message)
}

// Node has already formatted this message. Keep that original VM string,
// including lone surrogates, as the ordinary Error's non-enumerable message.
fn throw_header_message(
    context: &mut Context<'_>,
    code: &str,
    message: RootId,
) -> Result<RootId, RootedError> {
    let error = coded_error_rooted(context, true, code, "")?;
    set(context, error, "message", message)?;
    Err(context.throw(error))
}
