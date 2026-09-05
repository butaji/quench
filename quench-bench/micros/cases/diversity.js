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
