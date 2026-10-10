(function () {
  const itemCount = 200_000;
  const values = [];
  for (let index = 0; index < itemCount; index++) values.push("s" + index);
  console.log(values.length);
})();
