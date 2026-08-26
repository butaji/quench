function Step() {}
Step.prototype.execute = function () {};
const list = {
  values: [new Step(), new Step(), new Step(), new Step()],
  size() {
    return this.values.length;
  },
  at(index) {
    return this.values[index];
  },
};
const plan = {
  v: list,
  size() {
    return this.v.size();
  },
  constraintAt(index) {
    return this.v.at(index);
  },
  execute() {
    for (var i = 0; i < this.size(); i++) {
      var c = this.constraintAt(i);
      c.execute();
    }
  },
};

for (let i = 0; i < 100000; i++) plan.execute();
