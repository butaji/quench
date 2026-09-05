registerMicro({
  id: "composition",
  question:
    "Do isolated improvements survive combinations of ordinary language behavior?",
  requires: ["calls", "objects", "arrays", "strings", "regexp", "closures"],
  axes: ["size", "composition"],
  memory: true,
  observations: [
    "time per workload",
    "RSS",
    "cross-mechanism evidence, if available"
  ],
  explanations: [
    "Interaction effects",
    "Intermediate allocation",
    "Repeated boundaries"
  ],
  setup: function (n, seed) {
    return { n: n, seed: seed };
  },
  variants: {
    call_property: function (s) {
      function f(o) {
        return o.x + o.y;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += f({ x: i, y: s.seed });
      return t;
    },
    closure_allocation: function (s) {
      function make(x) {
        return function (y) {
          return x + y;
        };
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) t += make(i)(s.seed);
      return t;
    },
    numeric_array: function (s) {
      var a = [s.seed],
        t = 0;
      for (var i = 1; i < s.n; i++) a[i] = a[i - 1] * 0.5 + i;
      for (var j = 0; j < a.length; j++) t += a[j];
      return t;
    },
    string_regexp: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++)
        t += ("key=" + (i + s.seed)).replace(/\d+/g, "x").length;
      return t;
    },
    graph: function (s) {
      var node = null;
      for (var i = 0; i < s.n; i++) node = { value: i + s.seed, next: node };
      var t = 0;
      while (node) {
        t += node.value;
        node = node.next;
      }
      return t;
    }
  },
  equivalent: [["call_property", "closure_allocation", "graph"]]
});
