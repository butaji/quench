var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "iteration",
  question: "What cost comes from iterator protocols, suspension, and closing?",
  requires: ["arrays", "calls"],
  axes: ["size", "protocol"],
  observations: ["time per yielded value", "iterator close effects"],
  explanations: ["Protocol overhead", "Result allocation", "Suspension cost"],
  setup: function (n) {
    var a = [];
    for (var i = 0; i < n; i++) a.push(i);
    return { n: n, a: a };
  },
  equivalent: [["indexed", "builtin", "custom", "generator"]],
  variants: {
    indexed: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.a[i];
      return t;
    },
    builtin: function (s) {
      var t = 0;
      for (var x of s.a) t += x;
      return t;
    },
    custom: function (s) {
      var iterable = {};
      iterable[Symbol.iterator] = function () {
        var i = 0;
        return {
          next: function () {
            return { value: i, done: i++ >= s.n };
          }
        };
      };
      var t = 0;
      for (var x of iterable) t += x;
      return t;
    },
    generator: function (s) {
      function* values() {
        for (var i = 0; i < s.n; i++) yield i;
      }
      var t = 0;
      for (var x of values()) t += x;
      return t;
    },
    close: function (s) {
      var closed = 0;
      function* values() {
        try {
          for (var i = 0; i < s.n; i++) yield i;
        } finally {
          closed++;
        }
      }
      var t = 0;
      for (var x of values()) {
        t += x;
        if (x === s.n >> 1) break;
      }
      return [t, closed];
    },
    throw_close: function (s) {
      var closed = 0;
      function* values() {
        try {
          yield s.n;
        } finally {
          closed++;
        }
      }
      try {
        for (var x of values()) throw x;
      } catch (x) {
        return [x, closed];
      }
    }
  },
  check: function (r, s, v) {
    if ((v === "close" || v === "throw_close") && r[1] !== 1)
      throw new Error("iterator close");
  }
});

function __profileAssert(condition,message){if(!condition)throw new Error("execution profile assertion failed: "+message);}
function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
__profileAssert(__profileSpec!==undefined,"micro registration");
__profileAssert(typeof __profileSpec.setup==="function","setup is callable");
var __profileState=__profileSpec.setup(64,17,"close");var __profileOperation=__profileSpec.variants["close"];__profileAssert(typeof __profileOperation==="function","selected variant is callable");function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"close");var signature=JSON.stringify(__profileEncode(value));__profileAssert(signature==="[\"array\",[[\"number\",\"528\"],[\"number\",\"1\"]]]","exact encoded result");return signature;}
return __profileRun();
