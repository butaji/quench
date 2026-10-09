(function () {
  const array = [0];
  array.value = 0;
  for (let i = 0; i < 2000000; i++) array.value = i;
  if (array.value !== 1999999) throw new Error("array write");
})();
