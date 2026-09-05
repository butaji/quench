registerMicro({
  id: "property-mutation",
  question:
    "What is the cost of property changes and observable accessor transitions?",
  requires: ["objects"],
  axes: ["size", "mutation"],
  observations: ["time per mutation", "accessor effects", "property ordering"],
  explanations: ["Structural mutation", "Descriptor handling", "Invalidation"],
  setup: function (n) {
    return { n: n };
  },
  variants: {
    overwrite: function (s) {
      var o = { x: 0, y: 1 },
        t = 0;
      for (var i = 0; i < s.n; i++) {
        o.x = i;
        t += o.x;
      }
      return [t, Object.keys(o)];
    },
    delete_reinsert: function (s) {
      var o = { x: 0, y: 1 },
        t = 0;
      for (var i = 0; i < s.n; i++) {
        delete o.x;
        o.x = i;
        t += o.x;
      }
      return [t, Object.keys(o)];
    },
    descriptor: function (s) {
      var o = {},
        t = 0;
      for (var i = 0; i < s.n; i++) {
        Object.defineProperty(o, "x", {
          value: i,
          writable: true,
          configurable: true
        });
        t += o.x;
      }
      return t;
    },
    accessor_transition: function (s) {
      var o = {},
        calls = 0,
        t = 0;
      for (var i = 0; i < s.n; i++) {
        Object.defineProperty(o, "x", {
          get: function () {
            calls++;
            return 7;
          },
          configurable: true
        });
        t += o.x;
        Object.defineProperty(o, "x", { value: i, configurable: true });
        t += o.x;
      }
      return [t, calls];
    }
  },
  check: function (r, s, v) {
    if (v === "accessor_transition" && r[1] !== s.n)
      throw new Error("getter count");
  }
});
