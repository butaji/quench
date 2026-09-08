var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "objects",
  question: "How sensitive are reads and writes to receiver and key diversity?",
  requires: ["numeric"],
  axes: ["size", "receiver diversity", "property count"],
  observations: ["time per access", "lookup observations, if available"],
  explanations: ["Lookup cost", "Receiver diversity", "Key conversion"],
  setup: function (n, seed, variant) {
    var a = [];
    for (var i = 0; i < n; i++) {
      var o = { x: i + seed };
      if (variant === "diverse") o["p" + (i % 17)] = i;
      if (variant === "wide") for (var j = 0; j < 32; j++) o["p" + j] = j;
      a.push(o);
    }
    return { a: a, n: n };
  },
  equivalent: [["read", "diverse", "wide", "computed"]],
  variants: {
    read: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.a[i].x;
      return t;
    },
    diverse: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.a[i].x;
      return t;
    },
    wide: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.a[i].x;
      return t;
    },
    computed: function (s) {
      var t = 0,
        key = String.fromCharCode(120);
      for (var i = 0; i < s.n; i++) t += s.a[i][key];
      return t;
    },
    write: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) {
        s.a[i].x = i;
        t += s.a[i].x;
      }
      return t;
    }
  }
});

function __profileAssert(condition,message){if(!condition)throw new Error("execution profile assertion failed: "+message);}
function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
__profileAssert(__profileSpec!==undefined,"micro registration");
__profileAssert(typeof __profileSpec.setup==="function","setup is callable");
var __profileState=__profileSpec.setup(64,17,"diverse");var __profileOperation=__profileSpec.variants["diverse"];__profileAssert(typeof __profileOperation==="function","selected variant is callable");function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"diverse");var signature=JSON.stringify(__profileEncode(value));__profileAssert(signature==="[\"number\",\"3104\"]","exact encoded result");return signature;}
return __profileRun();
