//! Pure JavaScript HTTP protocol helpers exposed by `node:_http_common`.
//!
//! The socket parser remains owned by the HTTP host. This module centralizes
//! the Node-visible token and header-value rules used by the public HTTP API.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const FACTORY: &str = quench_js_check::checked_js!(
    r#"((ownerSymbol) => {
      const kIncomingMessage = Symbol('IncomingMessage');
      const kSkipPendingData = Symbol('SkipPendingData');
      const kOnHeaders = 1;
      const kOnHeadersComplete = 2;
      const kOnBody = 3;
      const kOnMessageComplete = 4;
      const kOnExecute = 5;
      const methods = [
        'ACL','BIND','CHECKOUT','CONNECT','COPY','DELETE','GET','HEAD','LINK','LOCK',
        'M-SEARCH','MERGE','MKACTIVITY','MKCALENDAR','MKCOL','MOVE','NOTIFY','OPTIONS',
        'PATCH','POST','PRI','PROPFIND','PROPPATCH','PURGE','PUT','QUERY','REBIND',
        'REPORT','SEARCH','SOURCE','SUBSCRIBE','TRACE','UNBIND','UNLINK','UNLOCK','UNSUBSCRIBE'
      ];
      const token = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
      const strictInvalid = /[^\t\x20-\x7e\x80-\xff]/;
      const lenientInvalid = /[\x00\x0a\x0d]|[^\x00-\xff]/;
      const parserConstants = {
        REQUEST: 1, RESPONSE: 2,
        kOnMessageBegin: 0, kOnHeaders: 1, kOnHeadersComplete: 2, kOnBody: 3,
        kOnMessageComplete: 4, kOnExecute: 5, kOnTimeout: 6,
        kLenientHeaders: 1, kLenientChunkedLength: 2, kLenientKeepAlive: 4,
        kLenientTransferEncoding: 8, kLenientVersion: 16,
        kLenientDataAfterClose: 32, kLenientOptionalLFAfterCR: 64,
        kLenientOptionalCRBeforeLF: 128, kLenientSpacesAfterChunkSize: 256,
        kLenientNone: 0, kLenientAll: 0x1ff, kLenientHeaderValueRelaxed: 0x200,
      };
      class HTTPParser {
        constructor() {
          this._currentBuffer = Buffer.alloc(0);
          this._pending = '';
          this._message = null;
        }
        close() {}
        free() {
          this._pending = '';
          this._message = null;
          for (const callback of [0, 1, 2, 3, 4, 5, 6]) this[callback] = null;
        }
        remove() { this.free(); }
        unconsume() {}
        initialize(type, resource) {
          this.type = type;
          this._pending = '';
          this._message = null;
          this._currentBuffer = Buffer.alloc(0);
          return this;
        }
        getCurrentBuffer() { return this._currentBuffer; }
        finish() {
          if (this._message && this._message.kind === 'length' &&
              this._message.remaining === 0) this._completeMessage();
          if (this._pending.length > 0 || this._message) {
            const error = new Error('Parse Error: Invalid EOF state');
            error.code = 'HPE_INVALID_EOF_STATE';
            return error;
          }
          return undefined;
        }
        consume(handle) {
          const parser = this;
          const owner = handle[ownerSymbol];
          if (owner) {
            // Node's HTTP client/server already feeds this parser from their
            // socket data handler. Only bridge standalone internal-binding
            // consumers through the public data stream.
            if (owner.listenerCount('data') > 0) return;
            owner.on('data', (chunk) => parser.execute(chunk));
            owner.once('end', () => parser.finish());
            return;
          }
          const { streamBaseState, kReadBytesOrError, kArrayBufferOffset } =
            process.binding('stream_wrap');
          const onread = function onHttpParserRead(arrayBuffer) {
            const nread = streamBaseState[kReadBytesOrError];
            if (nread > 0) {
              const offset = streamBaseState[kArrayBufferOffset];
              parser.execute(Buffer.from(arrayBuffer, offset, nread));
            } else if (nread === -4095) {
              parser.finish();
            }
          };
          const start = (startReading = true) => {
            handle.onread = onread;
            const current = handle;
            if (startReading && typeof current.readStart === 'function') current.readStart();
          };
          start();
        }
        _headers(headers, url) {
          const maximum = this.maxHeaderPairs;
          const limit = typeof maximum === 'number' && maximum > 0 ? maximum : Infinity;
          if (headers.length > limit && typeof this[kOnHeaders] === 'function')
            this[kOnHeaders].call(this, headers.slice(limit), url);
        }
        _completeMessage() {
          if (typeof this[kOnMessageComplete] === 'function')
            this[kOnMessageComplete].call(this);
          this._message = null;
        }
        execute(input, offset = 0, length = input?.length - offset) {
          if (!(this instanceof HTTPParser)) throw new TypeError('Illegal invocation');
          if (!input || typeof input.subarray !== 'function')
            throw new TypeError('The "buffer" argument must be a Buffer or Uint8Array');
          const bytes = input.subarray(offset, offset + length);
          this._currentBuffer = Buffer.from(bytes);
          this._pending += bytes.toString('latin1');
          while (true) {
            if (!this._message) {
              const boundary = this._pending.indexOf('\r\n\r\n');
              if (boundary < 0) break;
              const head = this._pending.slice(0, boundary);
              this._pending = this._pending.slice(boundary + 4);
              const lines = head.split('\r\n');
              const start = lines.shift() || '';
              const response = this.type === HTTPParser.RESPONSE || start.startsWith('HTTP/');
              const match = response ? /^HTTP\/(\d+)\.(\d+)\s+(\d{3})(?:\s+(.*))?$/.exec(start) :
                /^([^ ]+) ([^ ]+) HTTP\/(\d+)\.(\d+)$/.exec(start);
              if (!match) throw new Error('Parse Error');
              const headers = [];
              const values = Object.create(null);
              for (const line of lines) {
                const colon = line.indexOf(':');
                if (colon <= 0) throw new Error('Parse Error');
                const name = line.slice(0, colon);
                const value = line.slice(colon + 1).trim();
                headers.push(name, value);
                const key = name.toLowerCase();
                values[key] = values[key] ? `${values[key]}, ${value}` : value;
              }
              const url = response ? undefined : match[2];
              const method = response ? undefined : methods.indexOf(match[1]);
              const major = Number(response ? match[1] : match[3]);
              const minor = Number(response ? match[2] : match[4]);
              const statusCode = response ? Number(match[3]) : undefined;
              const statusMessage = response ? (match[4] || '') : undefined;
              this._headers(headers, url);
              const callback = this[kOnHeadersComplete];
              const skip = typeof callback === 'function' &&
                callback.call(this, major, minor, headers, method, url, statusCode, statusMessage);
              const noBody = skip || (response &&
                (statusCode < 200 || statusCode === 204 || statusCode === 304)) ||
                method === methods.indexOf('HEAD');
              if (noBody) {
                this._completeMessage();
                continue;
              }
              if (/\bchunked\b/i.test(values['transfer-encoding'] || '')) {
                this._message = { kind: 'chunked' };
              } else if (values['content-length'] !== undefined) {
                this._message = { kind: 'length', remaining: Math.max(0, Number(values['content-length']) || 0) };
                if (this._message.remaining === 0) {
                  this._completeMessage();
                  continue;
                }
              } else {
                this._completeMessage();
                continue;
              }
            }
            if (this._message.kind === 'length') {
              const count = Math.min(this._message.remaining, this._pending.length);
              if (count > 0 && typeof this[kOnBody] === 'function')
                this[kOnBody].call(this, Buffer.from(this._pending.slice(0, count), 'latin1'));
              this._pending = this._pending.slice(count);
              this._message.remaining -= count;
              if (this._message.remaining !== 0) break;
              this._completeMessage();
              continue;
            }
            const lineEnd = this._pending.indexOf('\r\n');
            if (lineEnd < 0) break;
            const size = Number.parseInt(this._pending.slice(0, lineEnd).split(';', 1)[0], 16);
            if (!Number.isFinite(size) || size < 0) throw new Error('Parse Error');
            if (size === 0) {
              if (this._pending.length === lineEnd + 2) {
                this._pending = '';
                this._completeMessage();
                continue;
              }
              const afterSize = this._pending.slice(lineEnd + 2);
              if (afterSize.startsWith('\r\n')) {
                this._pending = afterSize.slice(2);
                this._completeMessage();
                continue;
              }
              const trailerEnd = this._pending.indexOf('\r\n\r\n', lineEnd + 2);
              if (trailerEnd < 0) break;
              const trailerLines = this._pending.slice(lineEnd + 2, trailerEnd).split('\r\n');
              const trailers = [];
              for (const line of trailerLines) {
                const colon = line.indexOf(':');
                if (colon > 0) trailers.push(line.slice(0, colon), line.slice(colon + 1).trim());
              }
              if (trailers.length) {
                this._headers(trailers, undefined);
                if (typeof this[kOnHeaders] === 'function') this[kOnHeaders].call(this, trailers);
              }
              this._pending = this._pending.slice(trailerEnd + 4);
              this._completeMessage();
              continue;
            }
            const bodyStart = lineEnd + 2;
            if (this._pending.length < bodyStart + size + 2) break;
            if (typeof this[kOnBody] === 'function')
              this[kOnBody].call(this, Buffer.from(this._pending.slice(bodyStart, bodyStart + size), 'latin1'));
            this._pending = this._pending.slice(bodyStart + size + 2);
          }
          if (typeof this[kOnExecute] === 'function') this[kOnExecute].call(this);
          return length;
        }
      }
      Object.assign(HTTPParser, parserConstants);
      const freeParser = (parser, req, socket) => {
        if (parser) {
          parser.remove();
          parser.free();
        }
        if (req) req.parser = null;
        if (socket) socket.parser = null;
      };
      const checkIsHttpToken = (value) => value.length > 0 && token.test(value);
      const checkInvalidHeaderChar = (value, lenient = false) =>
        (lenient ? lenientInvalid : strictInvalid).test(value);
      const prepareError = (error, parser, rawPacket) => {
        error.rawPacket = rawPacket || parser.getCurrentBuffer();
        if (typeof error.reason === 'string') error.message = `Parse Error: ${error.reason}`;
      };
      const calculateLenientFlags = (validation, insecure) => {
        if (validation === 'strict') return HTTPParser.kLenientNone;
        if (validation === 'relaxed') return HTTPParser.kLenientHeaderValueRelaxed;
        if (validation === 'insecure') return HTTPParser.kLenientAll;
        return insecure ? HTTPParser.kLenientAll : HTTPParser.kLenientNone;
      };
      const chunkExpression = /(?:^|\W)chunked(?:$|\W)/i;
      const continueExpression = /(?:^|\W)100-continue(?:$|\W)/i;
      let exported;
      const parsers = {
        name: 'parsers',
        alloc() { return new exported.HTTPParser(); },
        free() {},
        use() { return new exported.HTTPParser(); },
      };
      exported = {
        CRLF: '\r\n', HTTPParser, _checkInvalidHeaderChar: checkInvalidHeaderChar,
        _checkIsHttpToken: checkIsHttpToken, calculateLenientFlags, chunkExpression,
        continueExpression, freeParser, isLenient: () => false, kIncomingMessage,
        kSkipPendingData, methods, allMethods: methods, parsers, prepareError,
      };
      return exported;
    })"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    const CACHE_KEY: &str = "\0binding:http_parser";
    let shared = context.host_mut().shared_state();
    if let Some(value) = shared.borrow().builtin_cache.get(CACHE_KEY).copied() {
        return Ok(value);
    }
    let factory = context.evaluate_script_rooted(FACTORY, "node:_http_common/shared.js")?;
    let undefined = context.undefined();
    let owner_symbol = super::net_reference_shared_vm::owner_symbol(context)?;
    let module = context.call_rooted(factory, undefined, &[owner_symbol])?;
    context.release_root(factory);
    let retained = context.retain(module)?;
    shared
        .borrow_mut()
        .builtin_cache
        .insert(CACHE_KEY.to_owned(), retained);
    Ok(module)
}
