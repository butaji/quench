registerMicro({
  id: "arrays",
  question: "How do element occupancy and growth affect indexed operations?",
  requires: ["numeric"],
  axes: ["size", "occupancy", "growth"],
  memory: true,
  observations: ["time per index", "peak RSS versus logical and live size"],
  explanations: ["Index lookup", "Growth policy", "Hole handling"],
  setup: function (n, seed, v) {
    var a = [];
    for (var i = 0; i < n; i++) {
      if (v !== "holey" || i % 4)
        a[v === "sparse" ? i * 97 : i] = (i + seed) % 31;
    }
    return { n: n, a: a, seed: seed };
  },
  equivalent: [["read", "sparse"]],
  variants: {
    read: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.a[i];
      return t;
    },
    write: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) {
        s.a[i] = i;
        t += s.a[i];
      }
      return t;
    },
    holey: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.a[i] === undefined ? 0 : s.a[i];
      return t;
    },
    sparse: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.a[i * 97];
      return t;
    },
    grow: function (s) {
      var a = [];
      for (var i = 0; i < s.n; i++) a.push(i + s.seed);
      return a[a.length - 1];
    },
    presized: function (s) {
      var a = new Array(s.n);
      for (var i = 0; i < s.n; i++) a[i] = i + s.seed;
      return a[a.length - 1];
    }
  }
});
