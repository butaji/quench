var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "prototypes",
  question: "How do inheritance depth and prototype replacement affect access?",
  requires: ["objects", "calls"],
  axes: ["size", "depth", "replacement"],
  observations: ["time per access", "changed values after mutation"],
  explanations: [
    "Chain traversal",
    "Dependency invalidation",
    "Method target changes"
  ],
  setup: function (n, seed, v) {
    var root = { x: seed },
      o = root;
    var depth = v === "deep" ? 16 : v === "own" ? 0 : 1;
    for (var i = 0; i < depth; i++) o = Object.create(o);
    return { n: n, seed: seed, o: o, root: root };
  },
  equivalent: [["own", "inherited", "deep"]],
  variants: {
    own: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.o.x;
      return t;
    },
    inherited: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.o.x;
      return t;
    },
    deep: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.o.x;
      return t;
    },
    replacement: function (s) {
      var o = Object.create({ x: s.seed }),
        t = 0;
      for (var i = 0; i < s.n; i++) {
        if (i === s.n >> 1) Object.setPrototypeOf(o, { x: s.seed + 1 });
        t += o.x;
      }
      return t;
    },
    method_replacement: function (s) {
      var p = {
          f: function () {
            return 1;
          }
        },
        o = Object.create(p),
        t = 0;
      for (var i = 0; i < s.n; i++) {
        if (i === s.n >> 1)
          p.f = function () {
            return 2;
          };
        t += o.f();
      }
      return t;
    }
  }
});

function __profileAssert(condition,message){if(!condition)throw new Error("execution profile assertion failed: "+message);}
function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
__profileAssert(__profileSpec!==undefined,"micro registration");
__profileAssert(typeof __profileSpec.setup==="function","setup is callable");
var __profileState=__profileSpec.setup(64,17,"method_replacement");var __profileOperation=__profileSpec.variants["method_replacement"];__profileAssert(typeof __profileOperation==="function","selected variant is callable");function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"method_replacement");var signature=JSON.stringify(__profileEncode(value));__profileAssert(signature==="[\"number\",\"96\"]","exact encoded result");return signature;}
return __profileRun();
