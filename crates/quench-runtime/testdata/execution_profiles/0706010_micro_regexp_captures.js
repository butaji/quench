var __profileSpec; function registerMicro(spec) { __profileSpec = spec; }
registerMicro({
  id: "regexp",
  question:
    "How do reuse, captures, failure, and pattern diversity affect matching?",
  requires: ["strings"],
  axes: ["size", "pattern reuse", "match behavior"],
  memory: true,
  observations: [
    "time per match",
    "RSS with repeated pattern creation",
    "compile/match observations, if available"
  ],
  explanations: [
    "Pattern construction",
    "Matching algorithm",
    "Capture work",
    "Pattern retention"
  ],
  setup: function (n, seed) {
    return { n: n, text: "id=" + seed + ";abc123;", re: /abc\d+/ };
  },
  equivalent: [["reused", "constructed"]],
  variants: {
    reused: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) if (s.re.test(s.text)) t++;
      return t;
    },
    constructed: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) if (new RegExp("abc\\d+").test(s.text)) t++;
      return t;
    },
    no_match: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) if (/xyz\d+/.test(s.text)) t++;
      return t;
    },
    captures: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) {
        var m = /(abc)(\d+)/.exec(s.text);
        t += m[1].length + m[2].length;
      }
      return t;
    },
    backtrack: function (s) {
      var t = 0,
        text = "aaaaaaaaab";
      for (var i = 0; i < s.n; i++) if (/(a|aa)+b/.test(text)) t++;
      return t;
    },
    diverse: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++)
        if (new RegExp("(?:abc|token" + i + ")\\d+").test(s.text)) t++;
      return t;
    }
  }
});

function __profileEncode(x){if(x===undefined)return["undefined"];if(typeof x==="number")return["number",Number.isNaN(x)?"NaN":Object.is(x,-0)?"-0":String(x)];if(typeof x==="bigint")return["bigint",String(x)];if(x===null||typeof x!=="object")return[typeof x,x];if(Array.isArray(x))return["array",x.map(__profileEncode)];return["object",Object.keys(x).map(function(k){return[k,__profileEncode(x[k])];})];}
var __profileState=__profileSpec.setup(64,17,"captures");var __profileOperation=__profileSpec.variants["captures"];function __profileRun(){var value=__profileOperation(__profileState);if(__profileSpec.check)__profileSpec.check(value,__profileState,"captures");var signature=JSON.stringify(__profileEncode(value));if(signature!=="[\"number\",\"384\"]")throw new Error("micro exact result mismatch");return signature;}
return __profileRun();
