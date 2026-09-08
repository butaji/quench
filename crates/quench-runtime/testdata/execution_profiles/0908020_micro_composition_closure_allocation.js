var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "composition",
  question:
    "Do isolated improvements survive combinations of ordinary language behavior?",
  requires: ["calls", "objects", "arrays", "strings", "regexp", "closures"],
  axes: ["size", "composition"],
  memory: true,
  observations: [
    "time per workload",
    "RSS",
    "cross-mechanism evidence, if available"
  ],
  explanations: [
    "Interaction effects",
    "Intermediate allocation",
    "Repeated boundaries"
  ],
  setup: function (n, seed) {
    return { n: n, seed: seed };
  },
  variants: {
    call_property: function (s) {
      function f(o) {
        return o.x + o.y;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f({ x: i, y: s.seed });
      return t;
    },
    closure_allocation: function (s) {
      function make(x) {
        return function (y) {
          return x + y;
        };
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += make(i)(s.seed);
      return t;
    },
    numeric_array: function (s) {
      var a = [s.seed],
        t = 0;
      for (var i = 1; i < s.n; i++) a[i] = a[i - 1] * 0.5 + i;
      for (var j = 0; j < a.length; j++) t += a[j];
      return t;
    },
    string_regexp: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++)
        t += ("key=" + (i + s.seed)).replace(/\d+/g, "x").length;
      return t;
    },
    graph: function (s) {
      var node = null;
      for (var i = 0; i < s.n; i++) node = { value: i + s.seed, next: node };
      var t = 0;
      while (node) {
        t += node.value;
        node = node.next;
      }
      return t;
    }
  },
  equivalent: [["call_property", "closure_allocation", "graph"]]
});

function __profileAssert(condition,message){if(!condition)throw new Error("execution profile assertion failed: "+message);}
function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
__profileAssert(__profileSpec!==undefined,"micro registration");
__profileAssert(typeof __profileSpec.setup==="function","setup is callable");
var __profileState=__profileSpec.setup(64,17,"closure_allocation");var __profileOperation=__profileSpec.variants["closure_allocation"];__profileAssert(typeof __profileOperation==="function","selected variant is callable");function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"closure_allocation");var signature=JSON.stringify(__profileEncode(value));__profileAssert(signature==="[\"number\",\"3104\"]","exact encoded result");return signature;}
return __profileRun();
