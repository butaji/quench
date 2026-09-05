registerMicro({
  id: "array-interaction",
  question:
    "What happens when indexed operations interact with aliasing or changing semantics?",
  requires: ["arrays", "conversion"],
  axes: ["size", "aliasing", "transition"],
  observations: ["time per index", "dependent outputs", "accessor effects"],
  explanations: [
    "Dependency handling",
    "Element transitions",
    "Inherited indexed access"
  ],
  setup: function (n, seed) {
    return { n: n, seed: seed };
  },
  variants: {
    independent: function (s) {
      var a = [],
        b = [];
      for (var i = 0; i < s.n; i++) {
        a[i] = i;
        b[i] = a[i] + 1;
      }
      return b[s.n - 1];
    },
    dependent: function (s) {
      var a = [s.seed];
      for (var i = 1; i < s.n; i++) a[i] = (a[i - 1] + i) | 0;
      return a[s.n - 1];
    },
    alias: function (s) {
      var a = [s.seed],
        b = a;
      for (var i = 1; i < s.n; i++) {
        b[i] = a[i - 1] + 1;
      }
      return a[s.n - 1];
    },
    type_change: function (s) {
      var a = [],
        t = 0;
      for (var i = 0; i < s.n; i++) a[i] = i;
      a[s.n >> 1] = "7";
      for (var j = 0; j < s.n; j++) t += +a[j];
      return t;
    },
    inherited_index: function (s) {
      var calls = 0,
        p = Object.create(Array.prototype);
      Object.defineProperty(p, "0", {
        get: function () {
          calls++;
          return s.seed;
        }
      });
      var a = [];
      Object.setPrototypeOf(a, p);
      var t = 0;
      for (var i = 0; i < s.n; i++) t += a[0];
      return [t, calls];
    }
  },
  check: function (r, s, v) {
    if (v === "inherited_index" && r[1] !== s.n)
      throw new Error("indexed getter count");
  }
});
