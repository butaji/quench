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
