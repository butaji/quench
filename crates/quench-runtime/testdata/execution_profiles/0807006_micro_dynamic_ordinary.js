var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "dynamic",
  question: "How costly are observable dynamic lookup and code construction?",
  requires: ["objects", "calls", "conversion"],
  axes: ["size", "dynamic behavior"],
  observations: ["time per operation", "trap counts", "dynamic binding result"],
  explanations: [
    "Trap invocation",
    "Code construction",
    "Dynamic binding resolution"
  ],
  setup: function (n, seed) {
    return { n: n, seed: seed };
  },
  variants: {
    ordinary: function (s) {
      var o = { x: s.seed },
        t = 0;
      for (var i = 0; i < s.n; i++) t += o.x;
      return t;
    },
    proxy: function (s) {
      var calls = 0,
        p = new Proxy(
          { x: s.seed },
          {
            get: function (o, k) {
              calls++;
              return o[k];
            }
          }
        ),
        t = 0;
      for (var i = 0; i < s.n; i++) t += p.x;
      return [t, calls];
    },
    reflect: function (s) {
      var o = { x: s.seed },
        t = 0;
      for (var i = 0; i < s.n; i++) t += Reflect.get(o, "x");
      return t;
    },
    function_construct: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += Function("x", "return x + 1")(i);
      return t;
    },
    direct_eval: function (s) {
      var value = s.seed,
        t = 0;
      for (var i = 0; i < s.n; i++) {
        eval("value += 1");
        t += value;
      }
      return t;
    }
  },
  equivalent: [["ordinary", "reflect"]],
  check: function (r, s, v) {
    if (v === "proxy" && r[1] !== s.n) throw new Error("proxy effects");
  }
});

function __profileAssert(condition,message){if(!condition)throw new Error("execution profile assertion failed: "+message);}
function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
__profileAssert(__profileSpec!==undefined,"micro registration");
__profileAssert(typeof __profileSpec.setup==="function","setup is callable");
var __profileState=__profileSpec.setup(64,17,"ordinary");var __profileOperation=__profileSpec.variants["ordinary"];__profileAssert(typeof __profileOperation==="function","selected variant is callable");function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"ordinary");var signature=JSON.stringify(__profileEncode(value));__profileAssert(signature==="[\"number\",\"1088\"]","exact encoded result");return signature;}
return __profileRun();
