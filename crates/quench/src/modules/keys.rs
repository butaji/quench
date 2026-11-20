//! JavaScript property and option names that several host modules read or write.

pub(crate) const ABORT_EVENT: &str = "abort";
pub(crate) const BACKPRESSURE: &str = "backpressure";
pub(crate) const BODY: &str = "body";
pub(crate) const CANCEL: &str = "cancel";
/// A read in progress: the chunks left and those read.
pub(crate) const CHUNKS: &str = "chunks";
pub(crate) const CLOSE: &str = "close";
pub(crate) const CONTROLLER: &str = "controller";
pub(crate) const COOKIE: &str = "cookie";
pub(crate) const CRLF: &str = "\r\n";
pub(crate) const DEFAULT_PROMPT: &str = "> ";
pub(crate) const DONE: &str = "done";
pub(crate) const ERROR: &str = "error";
pub(crate) const ERROR_EVENT: &str = "error";
pub(crate) const HANDLE: &str = "handle";
/// An event handler attribute's holder field: the current handler.
pub(crate) const HANDLER: &str = "handler";
pub(crate) const HEADERS: &str = "headers";
pub(crate) const HEADERS_STATE: &str = "headersState";
pub(crate) const INDEX: &str = "index";
pub(crate) const INSPECT: &str = "inspect";
/// The registry key of `util.inspect.custom`.
pub(crate) const INSPECT_CUSTOM: &str = "nodejs.util.inspect.custom";
pub(crate) const ITERATOR: &str = "iterator";
pub(crate) const KIND: &str = "kind";
pub(crate) const LISTENER: &str = "listener";
pub(crate) const MAP: &str = "Map";
pub(crate) const MAP_SET: &str = "mapSet";
pub(crate) const MESSAGE: &str = "message";
pub(crate) const METHOD: &str = "method";
pub(crate) const NAME: &str = "name";
pub(crate) const NEXT: &str = "next";
/// The directory each ancestor of a requester searches for packages.
pub(crate) const NODE_MODULES: &str = "node_modules";
pub(crate) const PASSIVE: &str = "passive";
pub(crate) const PORT: &str = "port";
pub(crate) const PREVENT_CANCEL: &str = "preventCancel";
pub(crate) const PROMISE: &str = "promise";
pub(crate) const READABLE: &str = "readable";
pub(crate) const READER: &str = "reader";
pub(crate) const REASON: &str = "reason";
pub(crate) const REDIRECT: &str = "redirect";
pub(crate) const SET_COOKIE: &str = "set-cookie";
pub(crate) const SIGNAL: &str = "signal";
pub(crate) const SIZE: &str = "size";
/// The private key of a channel's registry key.
pub(crate) const SLOT: &str = "slot";
/// The underlying source, sink or transformer user callbacks are called on.
pub(crate) const SOURCE: &str = "source";
pub(crate) const STARTED: &str = "started";
/// The module's state: a null-prototype object whose members `STATE_*` name.
pub(crate) const STATE: &str = "\0native:domain";
pub(crate) const STDERR: &str = "\0node/console/stderr";
/// The process streams the global console writes to, read when first written.
pub(crate) const STDOUT: &str = "\0node/console/stdout";
pub(crate) const STREAM: &str = "stream";
pub(crate) const SYNC: &str = "sync";
pub(crate) const TARGET: &str = "target";
pub(crate) const TRANSFORM: &str = "transform";
pub(crate) const TYPE: &str = "type";
pub(crate) const URL: &str = "url";
pub(crate) const VALUE: &str = "value";
pub(crate) const WAITERS: &str = "waiters";
pub(crate) const WRITABLE: &str = "writable";
pub(crate) const WRITER: &str = "writer";
