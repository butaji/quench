// `statuses` module — real implementation of the npm `statuses` package:
// a callable `status(codeOrMessage)` plus its standard companion maps
// (`.message`, `.code`, `.codes`, `.redirect`, `.empty`, `.retry`).
// This is the observer surface the express/body-parser dependency tree
// relies on; implemented as a host module so the OXC parser never
// round-trips the npm package.

(function (deps) {
  'use strict';

  var MESSAGE = {
    100: 'Continue',
    101: 'Switching Protocols',
    102: 'Processing',
    103: 'Early Hints',
    200: 'OK',
    201: 'Created',
    202: 'Accepted',
    203: 'Non-Authoritative Information',
    204: 'No Content',
    205: 'Reset Content',
    206: 'Partial Content',
    207: 'Multi-Status',
    208: 'Already Reported',
    226: 'IM Used',
    300: 'Multiple Choices',
    301: 'Moved Permanently',
    302: 'Found',
    303: 'See Other',
    304: 'Not Modified',
    305: 'Use Proxy',
    307: 'Temporary Redirect',
    308: 'Permanent Redirect',
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
    509: 'Bandwidth Limit Exceeded',
    510: 'Not Extended',
    511: 'Network Authentication Required'
  };

  function status(codeOrMessage) {
    if (typeof codeOrMessage === 'number') {
      return MESSAGE[codeOrMessage] || String(codeOrMessage);
    }
    return status.code[String(codeOrMessage).toLowerCase()] || status.code[codeOrMessage] || 0;
  }

  // message (lower-cased) -> code
  var CODE = {};
  var CODES = [];
  for (var code in MESSAGE) {
    if (Object.prototype.hasOwnProperty.call(MESSAGE, code)) {
      CODES.push(Number(code));
      CODE[MESSAGE[code].toLowerCase()] = Number(code);
    }
  }

  status.message = {};
  for (var k in MESSAGE) {
    if (Object.prototype.hasOwnProperty.call(MESSAGE, k)) {
      status.message[k] = MESSAGE[k];
    }
  }
  status.code = CODE;
  status.codes = CODES;
  status.redirect = { 300: true, 301: true, 302: true, 303: true, 305: true, 307: true, 308: true };
  status.empty = { 204: true, 205: true, 304: true };
  status.retry = { 502: true, 503: true, 504: true };

  return status;
});