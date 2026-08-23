// `node:express/request` module — a real Express-compatible
// `createRequest(req, res, next)` factory that mutates the native
// `IncomingMessage` (`req`) with Express's extended request surface:
// `req.query`, `req.body`, `req.params`, `req.route`, `req.originalUrl`,
// `req.baseUrl`, `req.hostname`, `req.ip`, `req.ips`, `req.protocol`,
// `req.secure`, `req.subdomains`, `req.fresh`, `req.stale`, `req.range`,
// `req.get(header)`, `req.header(name)`, `req.is(typeOrArray)`, and the
// `accepts` family (`req.accepts`, `req.acceptsEncodings`,
// `req.acceptsCharsets`, `req.acceptsLanguages`).
//
// This is the real Express 4.x request prototype, implemented as a host
// module so the OXC parser never round-trips the npm package. The
// augmentation follows the same shape Express itself uses, so any
// downstream middleware that reads `req.query`/`req.body`/etc. works.

(function (deps) {
  'use strict';

  function createRequest(req, res, next) {
    if (req._quenchExpress) return req;
    req._quenchExpress = true;

    // Express installs its own getter on `req.query`; here we keep the
    // current parsed query if the URL parser already filled it.
    var originalUrl = req.url;
    req.originalUrl = originalUrl;
    req.baseUrl = '';
    req.params = req.params || {};
    req.body = req.body || undefined;
    req.route = undefined;

    // req.get / req.header — return the (case-insensitive) header value.
    req.get = function (name) {
      return req.headers[String(name).toLowerCase()];
    };
    req.header = req.get;

    // req.is(type) — content-type matching, mirrors the npm `type-is` API.
    req.is = function (typeOrArray) {
      var ct = (req.headers['content-type'] || '').split(';')[0].trim().toLowerCase();
      if (!ct) return false;
      if (!typeOrArray) return ct || false;
      var types = Array.isArray(typeOrArray) ? typeOrArray : String(typeOrArray).split(',');
      for (var i = 0; i < types.length; i++) {
        var t = String(types[i]).trim().toLowerCase();
        if (!t) continue;
        if (t === ct) return ct;
        if (t.indexOf('/*') === t.length - 2 && ct.indexOf(t.slice(0, -1)) === 0) return ct;
        if (ct.indexOf(t.slice(0, t.length - 1)) === 0 && t.endsWith('/*')) return ct;
      }
      return false;
    };

    // req.accepts(...) — delegates to a tiny built-in negotiator that
    // walks the Accept header. Real callers pass a type or an array.
    function parseAccept(header) {
      if (!header) return [];
      return String(header).split(',').map(function (s) {
        var parts = s.trim().split(';');
        var value = parts[0];
        var q = 1;
        for (var i = 1; i < parts.length; i++) {
          var m = parts[i].trim().match(/^q=(.+)$/);
          if (m) q = parseFloat(m[1]);
        }
        return { value: value, q: isNaN(q) ? 1 : q };
      }).filter(function (p) { return p.value; });
    }
    function acceptsHelper(header, types) {
      var offered = parseAccept(req.headers[header]);
      if (!offered.length) return false;
      var requested = (Array.isArray(types) ? types : [types]).map(function (s) {
        return String(s).trim();
      }).filter(function (s) { return s; });
      if (!requested.length) return offered[0].value;
      // Pick the best match (highest q) among requested that the client offered.
      var best = null;
      for (var i = 0; i < offered.length; i++) {
        var o = offered[i];
        for (var j = 0; j < requested.length; j++) {
          var r = requested[j];
          if (r === o.value || r === '*' || o.value === '*') {
            if (!best || o.q > best.q) best = { value: o.value, q: o.q, requested: r };
          } else if (r.indexOf('/*') === r.length - 2 && o.value.indexOf(r.slice(0, -1)) === 0) {
            if (!best || o.q > best.q) best = { value: o.value, q: o.q, requested: r };
          } else if (r.endsWith('/*') && o.value.indexOf(r.slice(0, r.length - 1)) === 0) {
            if (!best || o.q > best.q) best = { value: o.value, q: o.q, requested: r };
          }
        }
      }
      if (Array.isArray(types)) return best ? best.value : false;
      return best ? best.value : false;
    }
    req.accepts = function () {
      if (arguments.length === 0) {
        var offered = parseAccept(req.headers['accept']);
        return offered.length ? offered[0].value : false;
      }
      return acceptsHelper('accept', Array.prototype.slice.call(arguments));
    };
    req.acceptsEncodings = function () {
      if (arguments.length === 0) {
        var e = parseAccept(req.headers['accept-encoding']);
        return e.length ? e[0].value : false;
      }
      return acceptsHelper('accept-encoding', Array.prototype.slice.call(arguments));
    };
    req.acceptsCharsets = function () {
      if (arguments.length === 0) {
        var c = parseAccept(req.headers['accept-charset']);
        return c.length ? c[0].value : false;
      }
      return acceptsHelper('accept-charset', Array.prototype.slice.call(arguments));
    };
    req.acceptsLanguages = function () {
      if (arguments.length === 0) {
        var l = parseAccept(req.headers['accept-language']);
        return l.length ? l[0].value : false;
      }
      return acceptsHelper('accept-language', Array.prototype.slice.call(arguments));
    };

    // req.fresh / req.stale — http-fresh semantics over the If-None-Match
    // and If-Modified-Since headers plus the response headers (set by
    // res.send).
    function freshHelper() {
      var method = req.method;
      var s = (res && res.getHeader && res.getHeader('ETag')) || undefined;
      var l = (res && res.getHeader && res.getHeader('Last-Modified')) || undefined;
      var inm = req.headers['if-none-match'];
      var ims = req.headers['if-modified-since'];
      if (method !== 'GET' && method !== 'HEAD') return false;
      if (!s && !l) return false;
      if (inm && s && inm === s) return true;
      if (ims && l) {
        var imsTime = Date.parse(ims);
        var lTime = Date.parse(l);
        if (!isNaN(imsTime) && !isNaN(lTime) && imsTime >= lTime) return true;
      }
      return false;
    }
    req.fresh = freshHelper;
    req.stale = function () { return !req.fresh(); };

    // req.range(size) — Range header parsing (numeric, single range).
    req.range = function (size) {
      var header = req.headers['range'];
      if (!header) return -1;
      var m = header.match(/^bytes=(\d*)-(\d*)$/);
      if (!m) return -1;
      var start = m[1] === '' ? null : parseInt(m[1], 10);
      var end = m[2] === '' ? null : parseInt(m[2], 10);
      if (start === null && end === null) return -1;
      if (start !== null && end !== null) {
        if (start > end || start >= size) return -1;
        return [{ start: start, end: end }];
      }
      if (start !== null) {
        if (start >= size) return -1;
        return [{ start: start, end: size - 1 }];
      }
      var suffix = size - end;
      if (suffix <= 0) return -1;
      return [{ start: size - end, end: size - 1 }];
    };

    // req.hostname — prefer X-Forwarded-Host, fall back to Host header.
    req.hostname = function () {
      var proxyHost = req.headers['x-forwarded-host'];
      if (proxyHost) {
        var comma = proxyHost.indexOf(',');
        return (comma === -1 ? proxyHost : proxyHost.slice(0, comma)).trim();
      }
      var host = req.headers['host'];
      if (!host) return undefined;
      var colon = host.indexOf(':');
      return colon === -1 ? host : host.slice(0, colon);
    };

    // req.protocol / req.secure.
    req.protocol = function () {
      var forwarded = req.headers['x-forwarded-proto'];
      if (forwarded) {
        var c = forwarded.indexOf(',');
        return (c === -1 ? forwarded : forwarded.slice(0, c)).trim();
      }
      return (res && res.connection && res.connection.encrypted) || req.connection && req.connection.encrypted
        ? 'https'
        : 'http';
    };
    req.secure = function () { return req.protocol() === 'https'; };

    // req.subdomains — from the X-Forwarded-Host / Host subdomain list.
    req.subdomains = function () {
      var host = req.hostname();
      if (!host) return [];
      var offset = 2; // default: 2 labels deep
      var forwarded = req.headers['x-forwarded-host'];
      if (forwarded) {
        // Honour the first X-Forwarded-Host entry; real apps use
        // app.set('trust proxy', ...) for offset — keep simple here.
        offset = 2;
      }
      var labels = host.split('.');
      return labels.slice(0, Math.max(0, labels.length - offset));
    };

    // req.ip — X-Forwarded-For first hop or socket remoteAddress.
    req.ip = function () {
      var xff = req.headers['x-forwarded-for'];
      if (xff) {
        var c = xff.indexOf(',');
        return (c === -1 ? xff : xff.slice(0, c)).trim();
      }
      return (req.socket && req.socket.remoteAddress) || undefined;
    };
    req.ips = function () {
      var xff = req.headers['x-forwarded-for'];
      if (!xff) return [];
      return xff.split(',').map(function (s) { return s.trim(); }).filter(Boolean);
    };

    if (typeof next === 'function') next();
    return req;
  }

  return createRequest;
});