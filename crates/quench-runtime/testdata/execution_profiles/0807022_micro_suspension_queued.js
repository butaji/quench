var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "suspension",
  question: "What changes with suspension and queued continuation count?",
  requires: ["calls", "iteration"],
  axes: ["size", "continuation form"],
  async: true,
  observations: [
    "time to completed useful work",
    "bounded pending continuation count"
  ],
  explanations: ["Promise creation", "Suspension", "Queue processing"],
  setup: function (n, seed) {
    return { n: n, seed: seed };
  },
  equivalent: [["synchronous", "await", "chain", "queued"]],
  variants: {
    synchronous: function (s) {
      var t = s.seed;
      for (var i = 0; i < s.n; i++) t++;
      return t;
    },
    await: async function (s) {
      var t = s.seed;
      for (var i = 0; i < s.n; i++) t = await Promise.resolve(t + 1);
      return t;
    },
    chain: function (s) {
      var p = Promise.resolve(s.seed);
      for (var i = 0; i < s.n; i++)
        p = p.then(function (x) {
          return x + 1;
        });
      return p;
    },
    queued: async function (s) {
      var a = [];
      for (var i = 0; i < s.n; i++) a.push(Promise.resolve(1));
      var values = await Promise.all(a),
        t = s.seed;
      for (var j = 0; j < values.length; j++) t += values[j];
      return t;
    }
  }
});

function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
var __profileState=__profileSpec.setup(64,17,"queued");var __profileOperation=__profileSpec.variants["queued"];async function __profileRun(){var value=await __profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"queued");var signature=JSON.stringify(__profileEncode(value));if(signature!=="[\"number\",\"81\"]")throw new Error("micro exact result mismatch");return signature;}
return __profileRun();
