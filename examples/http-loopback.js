const http = require("node:http");

const server = http.createServer((request, response) => {
  response.end(`hello ${request.url}`);
});

server.listen(0, () => {
  const port = server.address().port;
  http.get(`http://localhost:${port}/quench`, (response) => {
    let body = "";
    response.setEncoding("utf8");
    response.on("data", (chunk) => body += chunk);
    response.on("end", () => {
      console.log(body);
      server.close();
    });
  });
});
