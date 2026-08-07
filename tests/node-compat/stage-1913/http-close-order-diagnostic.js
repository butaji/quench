const http = require("http");

const observations = [];
const server = http.createServer((request, response) => {
  const label = request._diagnosticLabel || "default";
  const record = { label, events: [] };
  observations.push(record);
  request.on("end", () => record.events.push(`req-end:${request.destroyed}`));
  request.on(
    "close",
    () => record.events.push(`req-close:${request.destroyed}`),
  );
  response.on(
    "finish",
    () => record.events.push(`res-finish:${response.destroyed}`),
  );
  response.on(
    "close",
    () => record.events.push(`res-close:${response.destroyed}`),
  );
  response.end();
});
server.listen(0, () => {
  const request = http.get({ port: server.address().port }, (response) => {
    response.resume();
    response.on("end", () => {
      setImmediate(() => {
        console.log(JSON.stringify(observations));
        server.close();
      });
    });
  });
  request._diagnosticLabel = "default";
});
