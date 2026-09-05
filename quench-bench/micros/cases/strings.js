registerMicro({
  id: "strings",
  question:
    "How do length, encoding, and string operations affect time and retention?",
  requires: [],
  axes: ["size", "encoding", "operation"],
  memory: true,
  observations: [
    "time versus string length",
    "RSS under repeated construction"
  ],
  explanations: ["Copying", "Encoding conversion", "Search cost", "Retention"],
  setup: function (n, seed, v) {
    var unit = v === "unicode" ? "é😀" : v === "surrogate" ? "x\ud800" : "abc";
    return { n: n, seed: seed, unit: unit, text: unit.repeat(n) };
  },
  variants: {
    concat: function (s) {
      var t = String(s.seed);
      for (var i = 0; i < s.n; i++) t += s.unit;
      return [t.length, t.charCodeAt(t.length - 1)];
    },
    unicode: function (s) {
      var t = 0;
      for (var i = 0; i < s.text.length; i++) t += s.text.charCodeAt(i);
      return t;
    },
    surrogate: function (s) {
      var t = 0;
      for (var i = 0; i < s.text.length; i++) t += s.text.charCodeAt(i);
      return t;
    },
    substring: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.text.slice(i, i + 16).charCodeAt(0);
      return t;
    },
    search: function (s) {
      return [
        s.text.indexOf("bc"),
        s.text.indexOf("missing"),
        s.text.lastIndexOf("bc")
      ];
    },
    equality: function (s) {
      var other = ("!" + s.text).slice(1);
      var t = 0;
      for (var i = 0; i < s.n; i++) if (s.text === other) t++;
      return t;
    }
  }
});
