// C: for (i = 0; i < n; i++) h = (h << 5) - h + (unsigned char)s[i];
function kernel(s, n) {
  n = n | 0;
  let h = 0;
  for (let i = 0; i < n; i = (i + 1) | 0) h = ((h << 5) - h + s.charCodeAt(i)) | 0;
  return h;
}
const text = "the quick brown fox jumps over the lazy dog".repeat(1024);
let hash = 0;
for (let r = 0; r < 64; r++) hash = (hash + kernel(text, text.length)) | 0;
console.log(hash);
