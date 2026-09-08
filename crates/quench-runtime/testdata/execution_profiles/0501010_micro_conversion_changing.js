var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "conversion",
  question: "What changes when arithmetic must perform observable conversion?",
  requires: ["numeric"],
  axes: ["size", "conversion"],
  observations: ["time per conversion", "observable conversion calls"],
  explanations: [
    "Primitive conversion cost",
    "Repeated object conversion",
    "Reentry overhead"
  ],
  setup: function (n, seed) {
    return { n: n, seed: seed };
  },
  variants: {
    number: function (s) {
      var total = 0;
      for (var i = 0; i < s.n; i++) total += Number((i + s.seed) % 31);
      return total;
    },
    string: function (s) {
      var total = 0;
      for (var i = 0; i < s.n; i++) total += Number(String((i + s.seed) % 31));
      return total;
    },
    observable: function (s) {
      var calls = 0,
        value = 0;
      var x = {
        valueOf: function () {
          calls++;
          return value;
        }
      };
      var total = 0;
      for (var i = 0; i < s.n; i++) {
        value = (i + s.seed) % 31;
        total += +x;
      }
      return [total, calls];
    },
    changing: function (s) {
      var total = 0;
      for (var i = 0; i < s.n; i++) {
        var x = (i + s.seed) % 31;
        total += +(i % 17 ? x : String(x));
      }
      return total;
    }
  },
  check: function (result, s, variant) {
    if (variant === "observable" && result[1] !== s.n)
      throw new Error("conversion effects lost");
  }
});

function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
var __profileState=__profileSpec.setup(64,17,"changing");var __profileOperation=__profileSpec.variants["changing"];function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"changing");var signature=JSON.stringify(__profileEncode(value));if(signature!=="[\"number\",\"965\"]")throw new Error("micro exact result mismatch");return signature;}
return __profileRun();
