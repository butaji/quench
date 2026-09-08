var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "closures",
  question: "How do mutation, capture count, and escape affect closure cost?",
  requires: ["calls", "locals"],
  axes: ["size", "capture", "lifetime"],
  memory: true,
  observations: [
    "time per invocation",
    "retained RSS",
    "environment allocations, if available"
  ],
  explanations: ["Closure creation", "Capture access", "Retained environments"],
  setup: function (n, seed) {
    return { n: n, seed: seed, retained: [] };
  },
  variants: {
    readonly: function (s) {
      var value = s.seed;
      function f(x) {
        return x + value;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f(i);
      return t;
    },
    mutable: function (s) {
      var value = s.seed;
      function f() {
        return ++value;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f();
      return t;
    },
    many_captures: function (s) {
      var a = s.seed,
        b = a + 1,
        c = a + 2,
        d = a + 3;
      function f(x) {
        return x + a + b + c + d;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f(i);
      return t;
    },
    escaping: function (s) {
      var a = [];
      function make(x) {
        return function () {
          return x;
        };
      }
      for (var i = 0; i < s.n; i++) a.push(make(i + s.seed));
      s.retained = a;
      var t = 0;
      for (var j = 0; j < a.length; j++) t += a[j]();
      return t;
    }
  },
  release: function (s) {
    s.retained = [];
  }
});

function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
var __profileState=__profileSpec.setup(64,17,"mutable");var __profileOperation=__profileSpec.variants["mutable"];function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"mutable");var signature=JSON.stringify(__profileEncode(value));if(signature!=="[\"number\",\"3168\"]")throw new Error("micro exact result mismatch");return signature;}
return __profileRun();
