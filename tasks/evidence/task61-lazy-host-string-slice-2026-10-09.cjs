(function () {
  const itemCount = 200_000;
  const source = "abcdefghijklmnopqrstuvwxyz".repeat(10_000);
  const values = [];
  for (let index = 0; index < itemCount; index++) {
    values.push(source.slice(index, index + 8));
  }
  console.log(values.length);
})();
