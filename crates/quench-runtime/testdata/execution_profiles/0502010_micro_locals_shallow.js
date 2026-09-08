var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "locals",
  question:
    "Does equal useful work become more expensive with local state or call depth?",
  requires: ["calls"],
  axes: ["size", "local count", "depth"],
  observations: [
    "time per call",
    "initialized frame bytes and environment allocations, if available"
  ],
  explanations: [
    "State initialization",
    "Register traffic",
    "Recursion overhead"
  ],
  setup: function (n, seed) {
    return { n: n, seed: seed };
  },
  equivalent: [["small", "many", "body_size"]],
  variants: {
    small: function (s) {
      function f(x) {
        var a = x + 1;
        return a;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f(i + s.seed);
      return t;
    },
    many: function (s) {
      function f(x) {
        var a = x,
          b = x,
          c = x,
          d = x,
          e = x,
          f = x,
          g = x,
          h = x;
        return a + 1;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f(i + s.seed);
      return t;
    },
    body_size: function (s) {
      function f(x) {
        if (x < 0) {
          x += 1;
          x *= 3;
          x -= 7;
          x ^= 3;
          x += 9;
          x *= 5;
          x -= 1;
          x ^= 17;
        }
        return x + 1;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f(i + s.seed);
      return t;
    },
    shallow: function (s) {
      function f(x, d) {
        return d ? f(x + 1, d - 1) : x;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f(i, 2);
      return t;
    },
    deep: function (s) {
      function f(x, d) {
        return d ? f(x + 1, d - 1) : x;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f(i, 32);
      return t;
    }
  }
});

function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
var __profileState=__profileSpec.setup(64,17,"shallow");var __profileOperation=__profileSpec.variants["shallow"];function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"shallow");var signature=JSON.stringify(__profileEncode(value));if(signature!=="[\"number\",\"2144\"]")throw new Error("micro exact result mismatch");return signature;}
return __profileRun();
