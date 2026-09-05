registerMicro({
  id: "dynamic",
  question: "How costly are observable dynamic lookup and code construction?",
  requires: ["objects", "calls", "conversion"],
  axes: ["size", "dynamic behavior"],
  observations: ["time per operation", "trap counts", "dynamic binding result"],
  explanations: [
    "Trap invocation",
    "Code construction",
    "Dynamic binding resolution"
  ],
  setup: function (n, seed) {
    return { n: n, seed: seed };
  },
  variants: {
    ordinary: function (s) {
      var o = { x: s.seed },
        t = 0;
      for (var i = 0; i < s.n; i++) t += o.x;
      return t;
    },
    proxy: function (s) {
      var calls = 0,
        p = new Proxy(
          { x: s.seed },
          {
            get: function (o, k) {
              calls++;
              return o[k];
            }
          }
        ),
        t = 0;
      for (var i = 0; i < s.n; i++) t += p.x;
      return [t, calls];
    },
    reflect: function (s) {
      var o = { x: s.seed },
        t = 0;
      for (var i = 0; i < s.n; i++) t += Reflect.get(o, "x");
      return t;
    },
    function_construct: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += Function("x", "return x + 1")(i);
      return t;
    },
    direct_eval: function (s) {
      var value = s.seed,
        t = 0;
      for (var i = 0; i < s.n; i++) {
        eval("value += 1");
        t += value;
      }
      return t;
    }
  },
  equivalent: [["ordinary", "reflect"]],
  check: function (r, s, v) {
    if (v === "proxy" && r[1] !== s.n) throw new Error("proxy effects");
  }
});
