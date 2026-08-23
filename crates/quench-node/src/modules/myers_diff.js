// Minimal internal/assert/myers_diff compatibility.
(function () {
  'use strict';
  function myersDiff(a, b) {
    var size = a.length + b.length;
    if (size >= 2147483648) {
      var e = new RangeError('The value of "myersDiff input size" is out of range. It must be < 2^31. Received ' + size);
      e.code = 'ERR_OUT_OF_RANGE';
      throw e;
    }
    var result = [], i = 0, j = 0;
    while (i < a.length && j < b.length) {
      if (a[i] === b[j]) { result.push({ type: 'equal', value: a[i] }); i++; j++; }
      else { result.push({ type: 'delete', value: a[i++] }); }
    }
    while (i < a.length) result.push({ type: 'delete', value: a[i++] });
    while (j < b.length) result.push({ type: 'insert', value: b[j++] });
    return result;
  }
  return { myersDiff: myersDiff, printMyersDiff: function () {}, printSimpleMyersDiff: function () {} };
})()
