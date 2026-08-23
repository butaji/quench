const express = require('express');
const app = express();
app.get('/health', (_req, res) => res.end('{"ok":true,"framework":"express"}'));
app.listen(3456, '127.0.0.1');
