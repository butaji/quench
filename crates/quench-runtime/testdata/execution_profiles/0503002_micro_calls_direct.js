var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "calls",
  question:
    "What is the cost of call boundaries, arguments, receivers, and changing targets?",
  requires: ["numeric"],
  axes: ["size", "call shape"],
  observations: [
    "time per call",
    "allocations and argument transfers per call, if available"
  ],
  explanations: [
    "Call setup",
    "Argument handling",
    "Target diversity",
    "Receiver handling"
  ],
  setup: function (n, seed) {
    return {
      n: n,
      seed: seed,
      f: function (x) {
        return (x * 33 + 7) | 0;
      },
      g: function (x) {
        return (x * 33 + 7) | 0;
      }
    };
  },
  equivalent: [
    ["inline", "direct", "changing", "receiver", "bound", "arguments"]
  ],
  variants: {
    inline: function (s) {
      var x = s.seed;
      for (var i = 0; i < s.n; i++) x = (x * 33 + 7) | 0;
      return x;
    },
    direct: function (s) {
      var x = s.seed;
      for (var i = 0; i < s.n; i++) x = s.f(x);
      return x;
    },
    changing: function (s) {
      var x = s.seed;
      for (var i = 0; i < s.n; i++) x = (i % 7 ? s.f : s.g)(x);
      return x;
    },
    receiver: function (s) {
      var o = {
        bias: 7,
        f: function (x) {
          return (x * 33 + this.bias) | 0;
        }
      };
      var x = s.seed;
      for (var i = 0; i < s.n; i++) x = o.f(x);
      return x;
    },
    bound: function (s) {
      var f = s.f.bind(null),
        x = s.seed;
      for (var i = 0; i < s.n; i++) x = f(x);
      return x;
    },
    arguments: function (s) {
      function f(a, b, c, d, e, f) {
        return (a * b + c + d + e + f) | 0;
      }
      var x = s.seed;
      for (var i = 0; i < s.n; i++) x = f(x, 33, 1, 2, 3, 1);
      return x;
    }
  }
});

function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
var __profileState=__profileSpec.setup(64,17,"direct");var __profileOperation=__profileSpec.variants["direct"];function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"direct");var signature=JSON.stringify(__profileEncode(value));if(signature!=="[\"number\",\"-451678767\"]")throw new Error("micro exact result mismatch");return signature;}
return __profileRun();
