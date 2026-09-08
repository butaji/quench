var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "collections",
  question:
    "How do key identity, deletion, and live-set size affect collection operations?",
  requires: ["objects"],
  axes: ["size", "key kind", "churn"],
  memory: true,
  observations: ["time per lookup or mutation", "RSS at fixed live size"],
  explanations: [
    "Hashing and identity",
    "Capacity management",
    "Deletion retention"
  ],
  setup: function (n) {
    var keys = [],
      map = new Map();
    for (var i = 0; i < n; i++) {
      var key = { id: i };
      keys.push(key);
      map.set(key, i);
    }
    return { n: n, keys: keys, map: map };
  },
  variants: {
    lookup: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.map.get(s.keys[i]);
      return t;
    },
    string_keys: function (s) {
      var m = new Map(),
        t = 0;
      for (var i = 0; i < s.n; i++) m.set("k" + i, i);
      for (var j = 0; j < s.n; j++) t += m.get("k" + j);
      return t;
    },
    churn: function (s) {
      var m = new Map(),
        t = 0;
      for (var i = 0; i < s.n; i++) {
        m.set(i, i);
        t += m.get(i);
        m.delete(i);
      }
      return [t, m.size];
    },
    set: function (s) {
      var set = new Set(),
        t = 0;
      for (var i = 0; i < s.n; i++) set.add(i);
      for (var j = 0; j < s.n; j++) if (set.has(j)) t++;
      return t;
    },
    weak: function (s) {
      var m = new WeakMap(),
        t = 0;
      for (var i = 0; i < s.n; i++) m.set(s.keys[i], i);
      for (var j = 0; j < s.n; j++) t += m.get(s.keys[j]);
      return t;
    }
  }
});

function __profileAssert(condition,message){if(!condition)throw new Error("execution profile assertion failed: "+message);}
function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
__profileAssert(__profileSpec!==undefined,"micro registration");
__profileAssert(typeof __profileSpec.setup==="function","setup is callable");
var __profileState=__profileSpec.setup(64,17,"churn");var __profileOperation=__profileSpec.variants["churn"];__profileAssert(typeof __profileOperation==="function","selected variant is callable");function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"churn");var signature=JSON.stringify(__profileEncode(value));__profileAssert(signature==="[\"array\",[[\"number\",\"2016\"],[\"number\",\"0\"]]]","exact encoded result");return signature;}
return __profileRun();
