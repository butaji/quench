const collection = {
  elements: [3, 5, 7, 11],
  at(index) {
    return this.elements[index];
  },
};

let total = 0;
for (let i = 0; i < 250000; i++) total += collection.at(i & 3);
if (total !== 1625000) throw new Error("array index result");
