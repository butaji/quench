(function () {
  const object = { value: 0 };
  for (let i = 0; i < 2000000; i++) object.value = i;
  if (object.value !== 1999999) throw new Error("object write");
})();
