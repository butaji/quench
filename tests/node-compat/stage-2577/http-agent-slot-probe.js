const http = require("http");

const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
const server = http.createServer((req, res) => res.end("ok"));
server.listen(0, () => {
  let completed = 0;
  for (let index = 0; index < 2; index++) {
    const request = http.get({
      host: "localhost",
      port: server.address().port,
      agent,
      path: `/${index}`,
    }, (response) => {
      response.resume();
      response.once("end", () => {
        completed++;
        request.abort();
        if (completed === 2) {
          agent.destroy();
          server.close();
          console.log("http agent slot probe passed");
        }
      });
    });
    request.on("error", (error) => {
      throw error;
    });
  }
});
