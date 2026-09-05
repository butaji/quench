registerMicro({
  id: "lifetime",
  question:
    "Does RSS stabilize under repeated work and a fixed retained live set?",
  requires: ["construction", "closures", "collections"],
  axes: ["size", "epochs", "lifetime"],
  memory: true,
  observations: ["peak RSS", "late-epoch RSS growth", "live-set size"],
  explanations: [
    "Unreleased references",
    "Allocator retention",
    "Delayed reclamation",
    "Metadata growth"
  ],
  setup: function (n, seed) {
    var live = [];
    for (var i = 0; i < n; i++) live.push({ x: i + seed });
    return { n: n, seed: seed, live: live, retained: [] };
  },
  variants: {
    temporary: function (s) {
      var a = [],
        t = 0;
      for (var i = 0; i < s.n; i++) a.push({ x: i, data: [i, i + 1, i + 2] });
      for (var j = 0; j < a.length; j++) t += a[j].x;
      return t;
    },
    retained: function (s) {
      var a = [],
        t = 0;
      for (var i = 0; i < s.n; i++) a.push({ x: i, live: s.live[i] });
      s.retained = a;
      for (var j = 0; j < a.length; j++) t += a[j].x;
      return t;
    },
    cycles: function (s) {
      var a = [],
        t = 0;
      for (var i = 0; i < s.n; i++) {
        var x = { value: i };
        var y = { owner: x };
        x.child = y;
        a.push(x);
      }
      for (var j = 0; j < a.length; j++) t += a[j].child.owner.value;
      s.retained = a;
      return t;
    },
    closure_retention: function (s) {
      var a = [],
        t = 0;
      function make(x) {
        var data = [x, x + 1];
        return function () {
          return data[0];
        };
      }
      for (var i = 0; i < s.n; i++) a.push(make(i));
      s.retained = a;
      for (var j = 0; j < a.length; j++) t += a[j]();
      return t;
    }
  },
  equivalent: [["temporary", "retained", "cycles", "closure_retention"]],
  release: function (s) {
    s.retained = [];
  }
});
