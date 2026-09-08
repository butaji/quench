var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "diversity",
  question:
    "Does retained memory or execution cost grow with code, shape, or pattern diversity?",
  requires: ["calls", "objects", "regexp"],
  axes: ["size", "diversity"],
  memory: true,
  observations: [
    "time per fixed operation count",
    "RSS versus distinct definitions"
  ],
  explanations: ["Code construction", "Metadata retention", "Capacity churn"],
  setup: function (n, seed, v) {
    return { n: n, seed: seed, width: v === "fixed" ? 1 : Math.min(n, 256) };
  },
  variants: {
    fixed: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += Function("x", "return x + 0")(i);
      return t;
    },
    code: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++)
        t += Function("x", "return x + " + (i % s.width))(i);
      return t;
    },
    shapes: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) {
        var o = { x: i };
        o["k" + (i % s.width)] = i;
        t += o.x;
      }
      return t;
    },
    patterns: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++)
        if (new RegExp("(?:token" + (i % s.width) + "|abc)").test("abc")) t++;
      return t;
    }
  }
});

function __profileAssert(condition,message){if(!condition)throw new Error("execution profile assertion failed: "+message);}
function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
__profileAssert(__profileSpec!==undefined,"micro registration");
__profileAssert(typeof __profileSpec.setup==="function","setup is callable");
var __profileState=__profileSpec.setup(64,17,"shapes");var __profileOperation=__profileSpec.variants["shapes"];__profileAssert(typeof __profileOperation==="function","selected variant is callable");function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"shapes");var signature=JSON.stringify(__profileEncode(value));__profileAssert(signature==="[\"number\",\"2016\"]","exact encoded result");return signature;}
return __profileRun();
