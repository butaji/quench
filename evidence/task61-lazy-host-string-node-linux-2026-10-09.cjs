(function () {
  const lone = "\uD800";
  const paired = "\uD83E\uDD80";
  const values = [
    lone.length,
    lone.charCodeAt(0),
    lone.slice(0, 1).charCodeAt(0),
    paired.length,
    paired.charCodeAt(0),
    paired.charCodeAt(1),
    (lone + "x").length,
    JSON.stringify(lone),
    /./u.test(lone),
  ];
  console.log(JSON.stringify(values));
  console.log(lone);
})();
