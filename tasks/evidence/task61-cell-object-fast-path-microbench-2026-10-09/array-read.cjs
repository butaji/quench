(function () {
  const array = [3];
  array.value = 3;
  let result = 0;
  for (let i = 0; i < 2000000; i++) result = array.value;
  if (result !== 3) throw new Error("array read");
})();
