var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "property-mutation",
  question:
    "What is the cost of property changes and observable accessor transitions?",
  requires: ["objects"],
  axes: ["size", "mutation"],
  observations: ["time per mutation", "accessor effects", "property ordering"],
  explanations: ["Structural mutation", "Descriptor handling", "Invalidation"],
  setup: function (n) {
    return { n: n };
  },
  variants: {
    overwrite: function (s) {
      var o = { x: 0, y: 1 },
        t = 0;
      for (var i = 0; i < s.n; i++) {
        o.x = i;
        t += o.x;
      }
      return [t, Object.keys(o)];
    },
    delete_reinsert: function (s) {
      var o = { x: 0, y: 1 },
        t = 0;
      for (var i = 0; i < s.n; i++) {
        delete o.x;
        o.x = i;
        t += o.x;
      }
      return [t, Object.keys(o)];
    },
    descriptor: function (s) {
      var o = {},
        t = 0;
      for (var i = 0; i < s.n; i++) {
        Object.defineProperty(o, "x", {
          value: i,
          writable: true,
          configurable: true
        });
        t += o.x;
      }
      return t;
    },
    accessor_transition: function (s) {
      var o = {},
        calls = 0,
        t = 0;
      for (var i = 0; i < s.n; i++) {
        Object.defineProperty(o, "x", {
          get: function () {
            calls++;
            return 7;
          },
          configurable: true
        });
        t += o.x;
        Object.defineProperty(o, "x", { value: i, configurable: true });
        t += o.x;
      }
      return [t, calls];
    }
  },
  check: function (r, s, v) {
    if (v === "accessor_transition" && r[1] !== s.n)
      throw new Error("getter count");
  }
});

function __profileAssert(condition,message){if(!condition)throw new Error("execution profile assertion failed: "+message);}
function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
__profileAssert(__profileSpec!==undefined,"micro registration");
__profileAssert(typeof __profileSpec.setup==="function","setup is callable");
var __profileState=__profileSpec.setup(64,17,"descriptor");var __profileOperation=__profileSpec.variants["descriptor"];__profileAssert(typeof __profileOperation==="function","selected variant is callable");function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"descriptor");var signature=JSON.stringify(__profileEncode(value));__profileAssert(signature==="[\"number\",\"2016\"]","exact encoded result");return signature;}
return __profileRun();
