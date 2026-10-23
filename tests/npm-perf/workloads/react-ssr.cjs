const React = require("react");
const { renderToString } = require("react-dom/server");

const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 5000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 250);
const cards = Array.from({ length: 24 }, (_, index) =>
  React.createElement(
    "article",
    { className: index % 2 ? "card odd" : "card even", key: index },
    React.createElement("h2", null, `Item ${index}`),
    React.createElement(
      "p",
      { "data-index": index },
      `A fixed server rendered paragraph for record ${index}.`,
    ),
    React.createElement("ul", null, ...Array.from({ length: 4 }, (_, item) =>
      React.createElement("li", { key: item }, `Value ${index}:${item}`)
    )),
  )
);
const tree = React.createElement(
  "main",
  { lang: "en", "data-suite": "react-ssr" },
  React.createElement("h1", null, "Catalog"),
  ...cards,
);

for (let index = 0; index < warmups; index += 1) renderToString(tree);

let bytes = 0;
let checksum = 0;
const start = Date.now();
for (let index = 0; index < rounds; index += 1) {
  const html = renderToString(tree);
  bytes += html.length;
  checksum = (checksum + html.charCodeAt(index % html.length)) >>> 0;
}
const elapsedMs = Math.max(1, Date.now() - start);

console.log(JSON.stringify({
  workload: "react-ssr",
  package: "react@19.3.1 + react-dom@19.3.1",
  rounds,
  warmups,
  bytes,
  checksum,
  elapsedMs,
  operationsPerSecond: rounds * 1000 / elapsedMs,
}));
