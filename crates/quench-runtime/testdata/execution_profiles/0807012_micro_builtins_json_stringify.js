var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "builtins",
  question:
    "Which builtin operation and input size explains the observed cost?",
  requires: ["strings", "objects"],
  axes: ["size", "operation"],
  memory: true,
  observations: [
    "time per invocation versus input size",
    "callback counts",
    "peak RSS"
  ],
  explanations: ["Parsing", "Serialization", "Callback overhead", "Formatting"],
  setup: function (n, seed) {
    var a = [];
    for (var i = 0; i < n; i++) a.push({ x: i + seed });
    return {
      n: n,
      a: a,
      text: JSON.stringify(a),
      date: new Date(Date.UTC(2020, 0, 2)),
      seed: seed
    };
  },
  variants: {
    json_parse: function (s) {
      var a = JSON.parse(s.text);
      return [a.length, a[a.length - 1].x];
    },
    json_stringify: function (s) {
      return JSON.stringify(s.a);
    },
    json_reviver: function (s) {
      var calls = 0;
      var a = JSON.parse(s.text, function (k, v) {
        calls++;
        return v;
      });
      return [a.length, calls];
    },
    json_replacer: function (s) {
      var calls = 0;
      var text = JSON.stringify(s.a, function (k, v) {
        calls++;
        return v;
      });
      return [text.length, calls];
    },
    date_construct: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++)
        t += new Date(Date.UTC(2020, 0, (i % 27) + 1)).getUTCDate();
      return t;
    },
    date_access: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.date.getUTCDate();
      return t;
    },
    date_format: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.date.toISOString().length;
      return t;
    },
    string_case: function (s) {
      return s.text.toUpperCase().length;
    }
  },
  check: function (r, s, v) {
    if ((v === "json_reviver" || v === "json_replacer") && r[1] !== 2 * s.n + 1)
      throw new Error("JSON callback effects");
  }
});

function __profileAssert(condition,message){if(!condition)throw new Error("execution profile assertion failed: "+message);}
function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
__profileAssert(__profileSpec!==undefined,"micro registration");
__profileAssert(typeof __profileSpec.setup==="function","setup is callable");
var __profileState=__profileSpec.setup(64,17,"json_stringify");var __profileOperation=__profileSpec.variants["json_stringify"];__profileAssert(typeof __profileOperation==="function","selected variant is callable");function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"json_stringify");var signature=JSON.stringify(__profileEncode(value));__profileAssert(signature==="[\"string\",\"[{\\\"x\\\":17},{\\\"x\\\":18},{\\\"x\\\":19},{\\\"x\\\":20},{\\\"x\\\":21},{\\\"x\\\":22},{\\\"x\\\":23},{\\\"x\\\":24},{\\\"x\\\":25},{\\\"x\\\":26},{\\\"x\\\":27},{\\\"x\\\":28},{\\\"x\\\":29},{\\\"x\\\":30},{\\\"x\\\":31},{\\\"x\\\":32},{\\\"x\\\":33},{\\\"x\\\":34},{\\\"x\\\":35},{\\\"x\\\":36},{\\\"x\\\":37},{\\\"x\\\":38},{\\\"x\\\":39},{\\\"x\\\":40},{\\\"x\\\":41},{\\\"x\\\":42},{\\\"x\\\":43},{\\\"x\\\":44},{\\\"x\\\":45},{\\\"x\\\":46},{\\\"x\\\":47},{\\\"x\\\":48},{\\\"x\\\":49},{\\\"x\\\":50},{\\\"x\\\":51},{\\\"x\\\":52},{\\\"x\\\":53},{\\\"x\\\":54},{\\\"x\\\":55},{\\\"x\\\":56},{\\\"x\\\":57},{\\\"x\\\":58},{\\\"x\\\":59},{\\\"x\\\":60},{\\\"x\\\":61},{\\\"x\\\":62},{\\\"x\\\":63},{\\\"x\\\":64},{\\\"x\\\":65},{\\\"x\\\":66},{\\\"x\\\":67},{\\\"x\\\":68},{\\\"x\\\":69},{\\\"x\\\":70},{\\\"x\\\":71},{\\\"x\\\":72},{\\\"x\\\":73},{\\\"x\\\":74},{\\\"x\\\":75},{\\\"x\\\":76},{\\\"x\\\":77},{\\\"x\\\":78},{\\\"x\\\":79},{\\\"x\\\":80}]\"]","exact encoded result");return signature;}
return __profileRun();
