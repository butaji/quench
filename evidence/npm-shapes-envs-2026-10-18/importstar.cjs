var __createBinding = (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : 0);
var __setModuleDefault = (function(o, v) { Object.defineProperty(o, "default", { enumerable: true, value: v }); });
var ownKeys = function(o) { var ar = []; for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k; return ar; };
function __importStar(mod) {
    var result = {};
    if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
    __setModuleDefault(result, mod);
    return result;
}
const ts = {}; for (let i = 0; i < 2700; i++) ts['export' + i] = i;
const keep = [];
const N = Number(process.argv[2] ?? 80);
for (let j = 0; j < N; j++) keep.push(__importStar(ts));
let s = 0; for (const o of keep) s += o.export5;
console.log(s, process.memoryUsage().heapUsed >> 20);
