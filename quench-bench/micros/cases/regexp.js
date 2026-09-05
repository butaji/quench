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
