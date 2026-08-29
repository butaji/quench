const iterator = {
  [Symbol.asyncIterator]() { return this; },
  next() {
    return new Promise((resolve) => {
      setTimeout(() => resolve({ value: 1, done: true }), 1);
    });
  },
};

const values = [];
(async () => {
  for await (const value of iterator) values.push(value);
})().then(() => {
  if (values.length !== 1 || values[0] !== 1) throw new Error('async iteration lost its value');
});
