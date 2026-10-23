const fastify = require("fastify");

const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 5000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 250);
const concurrency = Number(process.env.QUENCH_BENCH_CONCURRENCY || 1);
const app = fastify({ logger: false });
const requestBody = { label: "catalog-item", count: 3, enabled: true };

app.post("/records/:id", {
  schema: {
    params: {
      type: "object",
      required: ["id"],
      properties: { id: { type: "string" } },
    },
    body: {
      type: "object",
      required: ["label", "count", "enabled"],
      properties: {
        label: { type: "string" },
        count: { type: "integer" },
        enabled: { type: "boolean" },
      },
    },
    response: {
      200: {
        type: "object",
        required: ["id", "label", "count", "enabled"],
        properties: {
          id: { type: "string" },
          label: { type: "string" },
          count: { type: "integer" },
          enabled: { type: "boolean" },
        },
      },
    },
  },
}, async (request, reply) => {
  reply.header("set-cookie", ["record=first", "record=second"]);
  return {
    id: request.params.id,
    label: request.body.label,
    count: request.body.count,
    enabled: request.body.enabled,
  };
});

async function main() {
  try {
    if (!Number.isSafeInteger(concurrency) || concurrency < 1) {
      throw new Error("QUENCH_BENCH_CONCURRENCY must be a positive integer");
    }
    await app.ready();
    for (let index = 0; index < warmups; index += concurrency) {
      const count = Math.min(concurrency, warmups - index);
      await Promise.all(Array.from({ length: count }, (_, offset) =>
        app.inject({ method: "POST", url: `/records/${(index + offset) & 63}`, payload: requestBody })));
    }

    let bytes = 0;
    let checksum = 0;
    const start = Date.now();
    for (let index = 0; index < rounds; index += concurrency) {
      const count = Math.min(concurrency, rounds - index);
      const responses = await Promise.all(Array.from({ length: count }, (_, offset) => {
        const requestIndex = index + offset;
        return app.inject({ method: "POST", url: `/records/${requestIndex & 63}`, payload: requestBody })
          .then((response) => ({ requestIndex, response }));
      }));
      for (const { requestIndex, response } of responses) {
        const id = String(requestIndex & 63);
        const expected = `{"id":"${id}","label":"catalog-item","count":3,"enabled":true}`;
        const validHeaders = response.headers["content-type"] === "application/json; charset=utf-8" &&
          response.headers["content-length"] === String(expected.length) &&
          Array.isArray(response.headers["set-cookie"]) &&
          response.headers["set-cookie"].length === 2 &&
          response.headers["set-cookie"][0] === "record=first" &&
          response.headers["set-cookie"][1] === "record=second";
        if (response.statusCode !== 200 || response.payload !== expected || !validHeaders) {
          throw new Error(`Fastify response mismatch for id ${id}: ${response.statusCode} ${response.payload}`);
        }
        bytes += response.payload.length;
        checksum = (checksum + response.payload.charCodeAt(requestIndex % response.payload.length)) >>> 0;
      }
    }
    const elapsedMs = Math.max(1, Date.now() - start);
    console.log(JSON.stringify({
      workload: "fastify-json-inject",
      package: "fastify@5.12.5",
      rounds,
      warmups,
      concurrency,
      bytes,
      checksum,
      elapsedMs,
      operationsPerSecond: rounds * 1000 / elapsedMs,
    }));
  } finally {
    await app.close();
  }
}

main().catch((error) => {
  console.error(error == null ? "Fastify inject workload failed with no error value" : error.stack || error);
  process.exitCode = 1;
});
