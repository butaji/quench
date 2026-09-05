registerMicro({
  id: "prototypes",
  question: "How do inheritance depth and prototype replacement affect access?",
  requires: ["objects", "calls"],
  axes: ["size", "depth", "replacement"],
  observations: ["time per access", "changed values after mutation"],
  explanations: [
    "Chain traversal",
    "Dependency invalidation",
    "Method target changes"
  ],
  setup: function (n, seed, v) {
    var root = { x: seed },
      o = root;
    var depth = v === "deep" ? 16 : v === "own" ? 0 : 1;
    for (var i = 0; i < depth; i++) o = Object.create(o);
    return { n: n, seed: seed, o: o, root: root };
  },
  equivalent: [["own", "inherited", "deep"]],
  variants: {
    own: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.o.x;
      return t;
    },
    inherited: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.o.x;
      return t;
    },
    deep: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.o.x;
      return t;
    },
    replacement: function (s) {
      var o = Object.create({ x: s.seed }),
        t = 0;
      for (var i = 0; i < s.n; i++) {
        if (i === s.n >> 1) Object.setPrototypeOf(o, { x: s.seed + 1 });
        t += o.x;
      }
      return t;
    },
    method_replacement: function (s) {
      var p = {
          f: function () {
            return 1;
          }
        },
        o = Object.create(p),
        t = 0;
      for (var i = 0; i < s.n; i++) {
        if (i === s.n >> 1)
          p.f = function () {
            return 2;
          };
        t += o.f();
      }
      return t;
    }
  }
});
