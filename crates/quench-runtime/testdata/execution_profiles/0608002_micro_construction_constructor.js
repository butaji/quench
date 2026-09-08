var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "construction",
  question:
    "How do construction form, object width, and escape affect allocation?",
  requires: ["objects", "calls"],
  axes: ["size", "construction form", "lifetime"],
  memory: true,
  observations: ["time per object", "RSS under retained objects"],
  explanations: ["Allocation", "Initialization", "Escaping identity"],
  setup: function (n, seed) {
    return { n: n, seed: seed, retained: [] };
  },
  equivalent: [["literal", "constructor", "class", "retained", "wide"]],
  variants: {
    literal: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) {
        var o = { x: i, y: s.seed };
        t += o.x + o.y;
      }
      return t;
    },
    constructor: function (s) {
      function C(x, y) {
        this.x = x;
        this.y = y;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) {
        var o = new C(i, s.seed);
        t += o.x + o.y;
      }
      return t;
    },
    class: function (s) {
      class C {
        constructor(x, y) {
          this.x = x;
          this.y = y;
        }
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) {
        var o = new C(i, s.seed);
        t += o.x + o.y;
      }
      return t;
    },
    retained: function (s) {
      var a = [],
        t = 0;
      for (var i = 0; i < s.n; i++) a.push({ x: i, y: s.seed });
      for (var j = 0; j < a.length; j++) t += a[j].x + a[j].y;
      s.retained = a;
      return t;
    },
    wide: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) {
        var o = { x: i, y: s.seed };
        for (var j = 0; j < 16; j++) o["p" + j] = j;
        t += o.x + o.y;
      }
      return t;
    }
  },
  release: function (s) {
    s.retained = [];
  }
});

function __profileAssert(condition,message){if(!condition)throw new Error("execution profile assertion failed: "+message);}
function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
__profileAssert(__profileSpec!==undefined,"micro registration");
__profileAssert(typeof __profileSpec.setup==="function","setup is callable");
var __profileState=__profileSpec.setup(64,17,"constructor");var __profileOperation=__profileSpec.variants["constructor"];__profileAssert(typeof __profileOperation==="function","selected variant is callable");function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"constructor");var signature=JSON.stringify(__profileEncode(value));__profileAssert(signature==="[\"number\",\"3104\"]","exact encoded result");return signature;}
return __profileRun();
