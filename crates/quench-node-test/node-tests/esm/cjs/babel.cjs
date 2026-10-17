"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
var _api = require("./api.cjs");
Object.keys(_api).forEach(function (key) {
  if (key === "default" || key === "__esModule") return;
  if (key in exports && exports[key] === _api[key]) return;
  Object.defineProperty(exports, key, { enumerable: true, get: function () { return _api[key]; } });
});
Object.defineProperty(exports, "hidden", { enumerable: false, get: function () { return 3; } });
Object.defineProperty(exports, "valued", { value: 4 });
