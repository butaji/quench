(function () {
  const itemCount = 200_000;
  const values = [];
  for (let index = 0; index < itemCount; index++) {
    values.push("String for key " + index + " in leaf node");
  }
  console.log(values.length);
})();
