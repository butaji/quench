// `http-errors` module — a real implementation of the surface npm
// `body-parser` (and friends) rely on: the `createError(status, message,
// props)` factory. The full `http-errors` API also exports an `HttpError`
// class, an `isHttpError` predicate, and named status constructors
// (`NotFoundError`, etc.); the real npm package exercises them through
// `inherits`/`setprototypeof`/`statuses`/`toidentifier`. We provide the
// observable contract the running callers actually need: a `createError`
// that returns a real `Error` augmented with `.status`, `.statusCode`,
// `.expose`, and any caller-supplied properties, plus a status-message
// table for the codes real callers raise.

(function (deps) {
  'use strict';

  var STATUSES = {
    400: 'Bad Request',
    401: 'Unauthorized',
    402: 'Payment Required',
    403: 'Forbidden',
    404: 'Not Found',
    405: 'Method Not Allowed',
    406: 'Not Acceptable',
    407: 'Proxy Authentication Required',
    408: 'Request Timeout',
    409: 'Conflict',
    410: 'Gone',
    411: 'Length Required',
    412: 'Precondition Failed',
    413: 'Payload Too Large',
    414: 'URI Too Long',
    415: 'Unsupported Media Type',
    416: 'Range Not Satisfiable',
    417: 'Expectation Failed',
    418: "I'm a teapot",
    421: 'Misdirected Request',
    422: 'Unprocessable Entity',
    423: 'Locked',
    424: 'Failed Dependency',
    425: 'Too Early',
    426: 'Upgrade Required',
    428: 'Precondition Required',
    429: 'Too Many Requests',
    431: 'Request Header Fields Too Large',
    451: 'Unavailable For Legal Reasons',
    500: 'Internal Server Error',
    501: 'Not Implemented',
    502: 'Bad Gateway',
    503: 'Service Unavailable',
    504: 'Gateway Timeout',
    505: 'HTTP Version Not Supported',
    506: 'Variant Also Negotiates',
    507: 'Insufficient Storage',
    508: 'Loop Detected',
    510: 'Not Extended',
    511: 'Network Authentication Required'
  };

  function createError() {
    var args = Array.prototype.slice.call(arguments);
    var status = args[0];
    var message = args[1];
    var props = args[2];

    var err = new Error(message || STATUSES[status] || String(status || 'Error'));

    if (props && typeof props === 'object') {
      for (var key in props) {
        if (Object.prototype.hasOwnProperty.call(props, key)) {
          err[key] = props[key];
        }
      }
    }

    err.status = status;
    err.statusCode = status;
    err.expose = typeof status === 'number' && status >= 400 && status < 500
      ? (props && props.expose !== undefined ? !!props.expose : true)
      : false;

    if (typeof Error.captureStackTrace === 'function') {
      Error.captureStackTrace(err, createError);
    }

    return err;
  }

  return createError;
});