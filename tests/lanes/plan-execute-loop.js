const list = {
  values: [{ execute() {} }, { execute() {} }, { execute() {} }, { execute() {} }],
  size() {
    return this.values.length;
  },
  at(index) {
    return this.values[index];
  },
};
const plan = {
  size() {
    return list.size();
  },
  constraintAt(index) {
    return list.at(index);
  },
  execute() {
    for (var i = 0; i < this.size(); i++) {
      var c = this.constraintAt(i);
      c.execute();
    }
  },
};

for (let i = 0; i < 100000; i++) plan.execute();
