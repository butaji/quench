registerMicro({
  id: "control",
  question: "How do branch diversity and equivalent loop forms scale?",
  requires: ["numeric"],
  axes: ["size", "branch diversity"],
  observations: [
    "time per iteration",
    "branch and dispatch observations, if available"
  ],
  explanations: [
    "Branch predictability",
    "Loop orchestration",
    "Dispatch overhead"
  ],
  setup: function (n, seed) {
    var a = [];
    for (var i = 0; i < n; i++) a.push(((i * 1103515245 + seed) >>> 8) & 7);
    return { n: n, a: a };
  },
  equivalent: [["for", "while"]],
  variants: {
    for: function (s) {
      var sum = 0;
      for (var i = 0; i < s.n; i++) sum += s.a[i];
      return sum;
    },
    while: function (s) {
      var sum = 0,
        i = 0;
      while (i < s.n) {
        sum += s.a[i];
        i++;
      }
      return sum;
    },
    predictable: function (s) {
      var sum = 0;
      for (var i = 0; i < s.n; i++) sum += s.a[i] < 8 ? 1 : -1;
      return sum;
    },
    changing: function (s) {
      var sum = 0;
      for (var i = 0; i < s.n; i++) sum += s.a[i] < 4 ? 1 : -1;
      return sum;
    },
    switch: function (s) {
      var sum = 0;
      for (var i = 0; i < s.n; i++) {
        switch (s.a[i]) {
          case 0:
            sum += 3;
            break;
          case 1:
            sum += 7;
            break;
          case 2:
            sum -= 1;
            break;
          default:
            sum += 2;
        }
      }
      return sum;
    },
    early_exit: function (s) {
      var at = -1;
      for (var i = 0; i < s.n; i++) {
        if (s.a[i] === 7) {
          at = i;
          break;
        }
      }
      return at;
    }
  }
});
