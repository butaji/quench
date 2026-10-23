const { getRouteMatcher } = require("next/dist/shared/lib/router/utils/route-matcher");
const { getRouteRegex } = require("next/dist/shared/lib/router/utils/route-regex");

const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 10000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 500);
const routeDefs = [
  ["/products/[category]/[...slug]", "/products/books/runtime/performance"],
  ["/docs/[...slug]", "/docs/api/server/route-handlers"],
  ["/users/[id]", "/users/8421"],
  ["/api/items/[id]/reviews/[reviewId]", "/api/items/sku-42/reviews/rev-9"],
];
const routes = routeDefs.map(([route, pathname], index) => {
  const match = getRouteMatcher(getRouteRegex(route));
  const expected = match(pathname);
  if (!expected) throw new Error(`route failed to match ${pathname}`);
  const encoded = JSON.stringify({ route: index, params: expected });
  return {
    index,
    pathname,
    match,
    expected,
    expectedKeys: Object.keys(expected),
    bytes: Buffer.byteLength(encoded),
    checksum: [...encoded].reduce((sum, character) => (sum + character.charCodeAt(0)) >>> 0, 0),
  };
});

function sameValue(actual, expected) {
  if (Array.isArray(expected)) {
    return Array.isArray(actual) && actual.length === expected.length &&
      actual.every((value, index) => value === expected[index]);
  }
  return actual === expected;
}

function matchRoute(index) {
  const route = routes[index % routes.length];
  const params = route.match(route.pathname);
  if (!params || route.expectedKeys.some((key) => !sameValue(params[key], route.expected[key]))) {
    throw new Error(`route params mismatch for ${route.pathname}`);
  }
  return route;
}

function main() {
  for (let index = 0; index < warmups; index += 1) matchRoute(index);
  let checksum = 0;
  let bytes = 0;
  const start = Date.now();
  for (let index = 0; index < rounds; index += 1) {
    const route = matchRoute(index);
    bytes += route.bytes;
    checksum = (checksum + route.checksum) >>> 0;
  }
  const elapsedMs = Math.max(1, Date.now() - start);
  console.log(JSON.stringify({
    workload: "next-route-match",
    package: "next@16.4.3",
    rounds,
    warmups,
    cases: ["Next dynamic-route matcher", "regexp capture groups", "param validation"],
    bytes,
    checksum,
    elapsedMs,
    operationsPerSecond: rounds * 1000 / elapsedMs,
  }));
}

try {
  main();
} catch (error) {
  console.error(error);
  process.exitCode = 1;
}
