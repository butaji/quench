var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "lifetime",
  question:
    "Does RSS stabilize under repeated work and a fixed retained live set?",
  requires: ["construction", "closures", "collections"],
  axes: ["size", "epochs", "lifetime"],
  memory: true,
  observations: ["peak RSS", "late-epoch RSS growth", "live-set size"],
  explanations: [
    "Unreleased references",
    "Allocator retention",
    "Delayed reclamation",
    "Metadata growth"
  ],
  setup: function (n, seed) {
    var live = [];
    for (var i = 0; i < n; i++) live.push({ x: i + seed });
    return { n: n, seed: seed, live: live, retained: [] };
  },
  variants: {
    temporary: function (s) {
      var a = [],
        t = 0;
      for (var i = 0; i < s.n; i++) a.push({ x: i, data: [i, i + 1, i + 2] });
      for (var j = 0; j < a.length; j++) t += a[j].x;
      return t;
    },
    retained: function (s) {
      var a = [],
        t = 0;
      for (var i = 0; i < s.n; i++) a.push({ x: i, live: s.live[i] });
      s.retained = a;
      for (var j = 0; j < a.length; j++) t += a[j].x;
      return t;
    },
    cycles: function (s) {
      var a = [],
        t = 0;
      for (var i = 0; i < s.n; i++) {
        var x = { value: i };
        var y = { owner: x };
        x.child = y;
        a.push(x);
      }
      for (var j = 0; j < a.length; j++) t += a[j].child.owner.value;
      s.retained = a;
      return t;
    },
    closure_retention: function (s) {
      var a = [],
        t = 0;
      function make(x) {
        var data = [x, x + 1];
        return function () {
          return data[0];
        };
      }
      for (var i = 0; i < s.n; i++) a.push(make(i));
      s.retained = a;
      for (var j = 0; j < a.length; j++) t += a[j]();
      return t;
    }
  },
  equivalent: [["temporary", "retained", "cycles", "closure_retention"]],
  release: function (s) {
    s.retained = [];
  }
});

function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
var __profileState=__profileSpec.setup(64,17,"cycles");var __profileOperation=__profileSpec.variants["cycles"];function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"cycles");var signature=JSON.stringify(__profileEncode(value));if(signature!=="[\"number\",\"2016\"]")throw new Error("micro exact result mismatch");return signature;}
return __profileRun();
