registerMicro({
  id: "exceptions",
  question:
    "What cost depends on exception frequency, depth, and finally effects?",
  requires: ["calls", "control"],
  axes: ["size", "frequency", "depth"],
  observations: ["time per iteration", "caught values", "finally effects"],
  explanations: ["Exception construction", "Unwinding", "Normal-path overhead"],
  setup: function (n, seed) {
    return { n: n, seed: seed };
  },
  variants: {
    normal: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) {
        try {
          t += i;
        } catch (e) {
          t--;
        }
      }
      return t;
    },
    occasional: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) {
        try {
          if (i % 31 === 0) throw i;
          t += i;
        } catch (e) {
          t += e;
        }
      }
      return t;
    },
    repeated: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) {
        try {
          throw i;
        } catch (e) {
          t += e;
        }
      }
      return t;
    },
    finally: function (s) {
      var t = 0,
        effects = 0;
      for (var i = 0; i < s.n; i++) {
        try {
          if (i % 7 === 0) throw i;
          t += i;
        } catch (e) {
          t += e;
        } finally {
          effects++;
        }
      }
      return [t, effects];
    },
    depth: function (s) {
      function f(d, x) {
        if (!d) throw x;
        return f(d - 1, x);
      }
      var t = 0;
      for (var i = 0; i < s.n; i++)
        try {
          f(16, i);
        } catch (e) {
          t += e;
        }
      return t;
    }
  },
  equivalent: [["normal", "occasional", "repeated", "depth"]],
  check: function (r, s, v) {
    if (v === "finally" && r[1] !== s.n) throw new Error("finally effects");
  }
});
