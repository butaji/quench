registerMicro({
  id: "construction",
  question:
    "How do construction form, object width, and escape affect allocation?",
  requires: ["objects", "calls"],
  axes: ["size", "construction form", "lifetime"],
  memory: true,
  observations: ["time per object", "RSS under retained objects"],
  explanations: ["Allocation", "Initialization", "Escaping identity"],
  setup: function (n, seed) {
    return { n: n, seed: seed, retained: [] };
  },
  equivalent: [["literal", "constructor", "class", "retained", "wide"]],
  variants: {
    literal: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) {
        var o = { x: i, y: s.seed };
        t += o.x + o.y;
      }
      return t;
    },
    constructor: function (s) {
      function C(x, y) {
        this.x = x;
        this.y = y;
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) {
        var o = new C(i, s.seed);
        t += o.x + o.y;
      }
      return t;
    },
    class: function (s) {
      class C {
        constructor(x, y) {
          this.x = x;
          this.y = y;
        }
      }
      var t = 0;
      for (var i = 0; i < s.n; i++) {
        var o = new C(i, s.seed);
        t += o.x + o.y;
      }
      return t;
    },
    retained: function (s) {
      var a = [],
        t = 0;
      for (var i = 0; i < s.n; i++) a.push({ x: i, y: s.seed });
      for (var j = 0; j < a.length; j++) t += a[j].x + a[j].y;
      s.retained = a;
      return t;
    },
    wide: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) {
        var o = { x: i, y: s.seed };
        for (var j = 0; j < 16; j++) o["p" + j] = j;
        t += o.x + o.y;
      }
      return t;
    }
  },
  release: function (s) {
    s.retained = [];
  }
});
