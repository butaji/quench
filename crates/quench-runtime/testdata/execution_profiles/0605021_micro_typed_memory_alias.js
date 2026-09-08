var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "typed-memory",
  question:
    "How do element types, view aliasing, and copying affect memory access?",
  requires: ["arrays", "numeric"],
  axes: ["size", "view type", "aliasing"],
  memory: true,
  observations: ["time per element", "peak RSS", "aliased results"],
  explanations: ["Conversion", "Bounds handling", "Copying"],
  setup: function (n, seed) {
    return { n: n, seed: seed };
  },
  variants: {
    uint: function (s) {
      var a = new Uint32Array(s.n),
        t = 0;
      for (var i = 0; i < s.n; i++) {
        a[i] = i + s.seed;
        t += a[i];
      }
      return t;
    },
    float: function (s) {
      var a = new Float64Array(s.n),
        t = 0;
      for (var i = 0; i < s.n; i++) {
        a[i] = (i + s.seed) / 7;
        t += a[i];
      }
      return t;
    },
    dataview: function (s) {
      var d = new DataView(new ArrayBuffer(s.n * 4)),
        t = 0;
      for (var i = 0; i < s.n; i++) {
        d.setUint32(i * 4, i + s.seed, true);
        t += d.getUint32(i * 4, true);
      }
      return t;
    },
    alias: function (s) {
      var b = new ArrayBuffer(s.n * 4),
        words = new Uint32Array(b),
        bytes = new Uint8Array(b),
        t = 0;
      for (var i = 0; i < s.n; i++) {
        words[i] = 0x01010101;
        t += bytes[i * 4];
      }
      return t;
    },
    copy: function (s) {
      var a = new Uint8Array(s.n);
      for (var i = 0; i < s.n; i++) a[i] = i + s.seed;
      var b = a.slice();
      var t = 0;
      for (var j = 0; j < b.length; j++) t += b[j];
      return t;
    }
  },
  equivalent: [["uint", "dataview"]]
});

function __profileAssert(condition,message){if(!condition)throw new Error("execution profile assertion failed: "+message);}
function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
__profileAssert(__profileSpec!==undefined,"micro registration");
__profileAssert(typeof __profileSpec.setup==="function","setup is callable");
var __profileState=__profileSpec.setup(64,17,"alias");var __profileOperation=__profileSpec.variants["alias"];__profileAssert(typeof __profileOperation==="function","selected variant is callable");function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"alias");var signature=JSON.stringify(__profileEncode(value));__profileAssert(signature==="[\"number\",\"64\"]","exact encoded result");return signature;}
return __profileRun();
