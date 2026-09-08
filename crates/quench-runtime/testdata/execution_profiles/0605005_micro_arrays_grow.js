var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "arrays",
  question: "How do element occupancy and growth affect indexed operations?",
  requires: ["numeric"],
  axes: ["size", "occupancy", "growth"],
  memory: true,
  observations: ["time per index", "peak RSS versus logical and live size"],
  explanations: ["Index lookup", "Growth policy", "Hole handling"],
  setup: function (n, seed, v) {
    var a = [];
    for (var i = 0; i < n; i++) {
      if (v !== "holey" || i % 4)
        a[v === "sparse" ? i * 97 : i] = (i + seed) % 31;
    }
    return { n: n, a: a, seed: seed };
  },
  equivalent: [["read", "sparse"]],
  variants: {
    read: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.a[i];
      return t;
    },
    write: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) {
        s.a[i] = i;
        t += s.a[i];
      }
      return t;
    },
    holey: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.a[i] === undefined ? 0 : s.a[i];
      return t;
    },
    sparse: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.a[i * 97];
      return t;
    },
    grow: function (s) {
      var a = [];
      for (var i = 0; i < s.n; i++) a.push(i + s.seed);
      return a[a.length - 1];
    },
    presized: function (s) {
      var a = new Array(s.n);
      for (var i = 0; i < s.n; i++) a[i] = i + s.seed;
      return a[a.length - 1];
    }
  }
});

function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
var __profileState=__profileSpec.setup(64,17,"grow");var __profileOperation=__profileSpec.variants["grow"];function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"grow");var signature=JSON.stringify(__profileEncode(value));if(signature!=="[\"number\",\"80\"]")throw new Error("micro exact result mismatch");return signature;}
return __profileRun();
