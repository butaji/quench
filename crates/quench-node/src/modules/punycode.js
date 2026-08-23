// node:punycode module factory — faithful port of Node's lib/punycode.js
// (RFC 3492). Parse-safe ES5: no const/arrow/spread/for-of/fromCodePoint.
(function (deps) {
  'use strict';

  var maxInt = 2147483647; // 0x7FFFFFFF
  var base = 36;
  var tMin = 1;
  var tMax = 26;
  var skew = 38;
  var damp = 700;
  var initialBias = 72;
  var initialN = 128; // 0x80
  var delimiter = '-'; // '\x2D'

  var regexPunycode = /^xn--/;
  var regexNonASCII = /[^\0-\x7F]/; // U+007F DEL counts as non-ASCII
  var regexSeparators = /[\x2E\u3002\uFF0E\uFF61]/g; // RFC 3490 separators

  var baseMinusTMin = base - tMin;
  var floor = Math.floor;
  var stringFromCharCode = String.fromCharCode;

  function error(type) {
    throw new RangeError(type);
  }

  function map(array, callback) {
    var result = [];
    var i = 0;
    var length = array.length;
    for (; i < length; i++) {
      result.push(callback(array[i]));
    }
    return result;
  }

  // Domain name string or email address: punycode only the domain part.
  function mapDomain(domain, callback) {
    var parts = domain.split('@');
    var result = '';
    if (parts.length > 1) {
      result = parts[0] + '@';
      domain = parts[1];
    }
    domain = domain.replace(regexSeparators, '\x2E');
    var labels = domain.split('.');
    var encoded = map(labels, callback).join('.');
    return result + encoded;
  }

  function ucs2decode(string) {
    var output = [];
    var counter = 0;
    var length = string.length;
    while (counter < length) {
      var value = string.charCodeAt(counter++);
      if (value >= 0xD800 && value <= 0xDBFF && counter < length) {
        var extra = string.charCodeAt(counter++);
        if ((extra & 0xFC00) === 0xDC00) {
          output.push(((value & 0x3FF) << 10) + (extra & 0x3FF) + 0x10000);
        } else {
          output.push(value);
          counter--;
        }
      } else {
        output.push(value);
      }
    }
    return output;
  }

  function ucs2encode(array) {
    return map(array, function (value) {
      var out = '';
      if (value > 0xFFFF) {
        value -= 0x10000;
        out += stringFromCharCode(value >>> 10 & 0x3FF | 0xD800);
        value = 0xDC00 | value & 0x3FF;
      }
      out += stringFromCharCode(value);
      return out;
    }).join('');
  }

  function basicToDigit(codePoint) {
    if (codePoint >= 48 && codePoint <= 57) return codePoint - 22; // '0'-'9'
    if (codePoint >= 65 && codePoint <= 90) return codePoint - 65; // 'A'-'Z'
    if (codePoint >= 97 && codePoint <= 122) return codePoint - 97; // 'a'-'z'
    return base;
  }

  function digitToBasic(digit, flag) {
    return digit + 22 + 75 * (digit < 26 ? 1 : 0) - ((flag !== 0) << 5);
  }

  function adapt(delta, numPoints, firstTime) {
    var k = 0;
    delta = firstTime ? floor(delta / damp) : delta >> 1;
    delta += floor(delta / numPoints);
    for (; delta > baseMinusTMin * tMax >> 1; k += base) {
      delta = floor(delta / baseMinusTMin);
    }
    return floor(k + (baseMinusTMin + 1) * delta / (delta + skew));
  }

  function decode(input) {
    var output = [];
    var inputLength = input.length;
    var i = 0;
    var n = initialN;
    var bias = initialBias;

    var basic = input.lastIndexOf(delimiter);
    if (basic < 0) {
      basic = 0;
    }

    for (var j = 0; j < basic; ++j) {
      if (input.charCodeAt(j) >= 0x80) {
        error('Illegal input >= 0x80 (not a basic code point)');
      }
      output.push(input.charCodeAt(j));
    }

    for (var index = basic > 0 ? basic + 1 : 0; index < inputLength; /* none */) {
      var oldi = i;
      for (var w = 1, k = base; /* none */; k += base) {
        if (index >= inputLength) {
          error('Invalid input');
        }
        var digit = basicToDigit(input.charCodeAt(index++));
        if (digit >= base) {
          error('Invalid input');
        }
        if (digit > floor((maxInt - i) / w)) {
          error('Overflow: input needs wider integers to process');
        }
        i += digit * w;
        var t = k <= bias ? tMin : (k >= bias + tMax ? tMax : k - bias);
        if (digit < t) {
          break;
        }
        var baseMinusT = base - t;
        if (w > floor(maxInt / baseMinusT)) {
          error('Overflow: input needs wider integers to process');
        }
        w *= baseMinusT;
      }
      var out = output.length + 1;
      bias = adapt(i - oldi, out, oldi === 0);
      if (floor(i / out) > maxInt - n) {
        error('Overflow');
      }
      n += floor(i / out);
      i %= out;
      output.splice(i++, 0, n);
    }
    return ucs2encode(output);
  }

  function encode(input) {
    var output = [];
    input = ucs2decode(input);
    var inputLength = input.length;
    var n = initialN;
    var delta = 0;
    var bias = initialBias;
    var currentValue;
    var index = 0;

    for (; index < inputLength; index++) {
      currentValue = input[index];
      if (currentValue < 0x80) {
        output.push(stringFromCharCode(currentValue));
      }
    }

    var basicLength = output.length;
    var handledCPCount = basicLength;

    if (basicLength) {
      output.push(delimiter);
    }

    while (handledCPCount < inputLength) {
      var m = maxInt;
      for (index = 0; index < inputLength; index++) {
        currentValue = input[index];
        if (currentValue >= n && currentValue < m) {
          m = currentValue;
        }
      }
      var handledCPCountPlusOne = handledCPCount + 1;
      if (m - n > floor((maxInt - delta) / handledCPCountPlusOne)) {
        error('Overflow: input needs wider integers to process');
      }
      delta += (m - n) * handledCPCountPlusOne;
      n = m;
      for (index = 0; index < inputLength; index++) {
        currentValue = input[index];
        if (currentValue < n && ++delta > maxInt) {
          error('Overflow');
        }
        if (currentValue === n) {
          var q = delta;
          for (var k = base; /* none */; k += base) {
            var t = k <= bias ? tMin : (k >= bias + tMax ? tMax : k - bias);
            if (q < t) {
              break;
            }
            var qMinusT = q - t;
            var baseMinusT = base - t;
            output.push(stringFromCharCode(digitToBasic(t + (qMinusT % baseMinusT), 0)));
            q = floor(qMinusT / baseMinusT);
          }
          output.push(stringFromCharCode(digitToBasic(q, 0)));
          bias = adapt(delta, handledCPCountPlusOne, handledCPCount === basicLength);
          delta = 0;
          ++handledCPCount;
        }
      }
      ++delta;
      ++n;
    }
    return output.join('');
  }

  function toUnicode(input) {
    return mapDomain(input, function (string) {
      return regexPunycode.test(string)
        ? decode(string.slice(4).toLowerCase())
        : string;
    });
  }

  function toASCII(input) {
    return mapDomain(input, function (string) {
      return regexNonASCII.test(string)
        ? 'xn--' + encode(string)
        : string;
    });
  }

  return {
    version: '2.1.0',
    ucs2: {
      decode: ucs2decode,
      encode: ucs2encode
    },
    decode: decode,
    encode: encode,
    toASCII: toASCII,
    toUnicode: toUnicode
  };
});