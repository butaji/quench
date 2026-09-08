var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "numeric",
  question:
    "How do numeric representation and dependency chains affect useful arithmetic?",
  axes: ["size", "representation", "dependency"],
  requires: [],
  observations: [
    "execution time per iteration",
    "numeric conversion and decode counts, if available"
  ],
  explanations: [
    "Representation-dependent costs",
    "Dependency-limited execution",
    "Repeated conversion"
  ],
  setup: function (n, seed) {
    return { n: n, seed: seed };
  },
  variants: {
    integer: function (s) {
      var a = s.seed;
      for (var i = 0; i < s.n; i++) a = (a * 33 + i) | 0;
      return a;
    },
    floating: function (s) {
      var a = s.seed / 17;
      for (var i = 0; i < s.n; i++) a = a * 0.999 + (i % 17) / 19;
      return a;
    },
    bitwise: function (s) {
      var a = s.seed;
      for (var i = 0; i < s.n; i++) a = ((a << 5) ^ (a >>> 3) ^ i) | 0;
      return a;
    },
    independent: function (s) {
      var a = s.seed,
        b = s.seed + 1;
      for (var i = 0; i < s.n; i++) {
        a = (a * 33 + i) | 0;
        b = (b * 33 + i) | 0;
      }
      return [a, b];
    },
    mixed: function (s) {
      var a = s.seed;
      for (var i = 0; i < s.n; i++) a = (a + (i % 31 === 0 ? 0.5 : 1)) % 100003;
      return a;
    },
    bigint: function (s) {
      var a = BigInt(s.seed);
      for (var i = 0; i < s.n; i++) a = (a * 33n + BigInt(i)) & 0xffffffffn;
      return a;
    }
  }
});

function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
var __profileState=__profileSpec.setup(64,17,"integer");var __profileOperation=__profileSpec.variants["integer"];function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"integer");var signature=JSON.stringify(__profileEncode(value));if(signature!=="[\"number\",\"351545329\"]")throw new Error("micro exact result mismatch");return signature;}
return __profileRun();
