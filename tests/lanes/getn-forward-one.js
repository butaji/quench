const collection = {
  elements: [3, 5, 7, 11],
  at(index) {
    return this.elements[index];
  },
};
const plan = {
  collection,
  at(index) {
    return this.collection.at(index);
  },
};

let total = 0;
for (let i = 0; i < 250000; i++) total += plan.at(i & 3);
if (total !== 1625000) throw new Error("forward one result");
