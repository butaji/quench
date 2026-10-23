const fastify = require("fastify");
const http = require("node:http");

const rounds = Number(process.env.QUENCH_BENCH_ROUNDS || 5000);
const warmups = Number(process.env.QUENCH_BENCH_WARMUPS || 250);
const app = fastify({ logger: false });
const requestBody = { label: "catalog-item", count: 3, enabled: true };
const requestJson = JSON.stringify(requestBody);
const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });

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
}, async (request) => ({
  id: request.params.id,
  label: request.body.label,
  count: request.body.count,
  enabled: request.body.enabled,
}));

function send(port, id) {
  return new Promise((resolve, reject) => {
    const request = http.request({
      host: "127.0.0.1",
      port,
      method: "POST",
      path: `/records/${id}`,
      agent,
      headers: {
        "content-type": "application/json",
        "content-length": Buffer.byteLength(requestJson),
      },
    }, (response) => {
      let payload = "";
      response.setEncoding("utf8");
      response.on("data", (chunk) => { payload += chunk; });
      response.on("end", () => resolve({ statusCode: response.statusCode, payload }));
    });
    request.on("error", reject);
    request.end(requestJson);
  });
}

async function main() {
  try {
    await app.listen({ host: "127.0.0.1", port: 0 });
    const port = app.server.address().port;
    for (let index = 0; index < warmups; index += 1) {
      await send(port, String(index & 63));
    }

    let bytes = 0;
    let checksum = 0;
    const start = Date.now();
    for (let index = 0; index < rounds; index += 1) {
      const id = String(index & 63);
      const response = await send(port, id);
      const expected = `{"id":"${id}","label":"catalog-item","count":3,"enabled":true}`;
      if (response.statusCode !== 200 || response.payload !== expected) {
        throw new Error(`Fastify response mismatch for id ${id}: ${response.statusCode} ${response.payload}`);
      }
      bytes += response.payload.length;
      checksum = (checksum + response.payload.charCodeAt(index % response.payload.length)) >>> 0;
    }
    const elapsedMs = Math.max(1, Date.now() - start);

    console.log(JSON.stringify({
      workload: "fastify-json-http",
      package: "fastify@5.12.5",
      rounds,
      warmups,
      bytes,
      checksum,
      elapsedMs,
      operationsPerSecond: rounds * 1000 / elapsedMs,
    }));
  } finally {
    agent.destroy();
    await app.close();
  }
}

main().catch(async (error) => {
  try { await app.close(); } catch { /* preserve the workload failure */ }
  console.error(error == null ? "Fastify workload failed with no error value" : error.stack || error);
  process.exitCode = 1;
});
