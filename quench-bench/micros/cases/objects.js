registerMicro({
  id: "objects",
  question: "How sensitive are reads and writes to receiver and key diversity?",
  requires: ["numeric"],
  axes: ["size", "receiver diversity", "property count"],
  observations: ["time per access", "lookup observations, if available"],
  explanations: ["Lookup cost", "Receiver diversity", "Key conversion"],
  setup: function (n, seed, variant) {
    var a = [];
    for (var i = 0; i < n; i++) {
      var o = { x: i + seed };
      if (variant === "diverse") o["p" + (i % 17)] = i;
      if (variant === "wide") for (var j = 0; j < 32; j++) o["p" + j] = j;
      a.push(o);
    }
    return { a: a, n: n };
  },
  equivalent: [["read", "diverse", "wide", "computed"]],
  variants: {
    read: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.a[i].x;
      return t;
    },
    diverse: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.a[i].x;
      return t;
    },
    wide: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) t += s.a[i].x;
      return t;
    },
    computed: function (s) {
      var t = 0,
        key = String.fromCharCode(120);
      for (var i = 0; i < s.n; i++) t += s.a[i][key];
      return t;
    },
    write: function (s) {
      var t = 0;
      for (var i = 0; i < s.n; i++) {
        s.a[i].x = i;
        t += s.a[i].x;
      }
      return t;
    }
  }
});
