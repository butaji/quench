#!/bin/bash
# Compile TSX/TS to plain JS for Quench
# Usage: ./scripts/compile-tsx.sh input.tsx [output.js]

set -e

INPUT="${1:-}"
OUTPUT="${2:-}"

if [ -z "$INPUT" ]; then
    echo "Usage: $0 <input.tsx> [output.js]"
    echo ""
    echo "Examples:"
    echo "  $0 examples/counter.tsx"
    echo "  $0 examples/counter.tsx examples/counter.tb.js"
    exit 1
fi

if [ ! -f "$INPUT" ]; then
    echo "Error: File not found: $INPUT"
    exit 1
fi

# Default output: input file with .tb.js suffix
if [ -z "$OUTPUT" ]; then
    OUTPUT="${INPUT%.tsx}.tb.js"
    OUTPUT="${OUTPUT%.ts}.tb.js"
fi

echo "Transpiling $INPUT → $OUTPUT ..."

# Use deno to transpile TSX → JS, then strip imports
deno eval '
const code = await Deno.readTextFile(Deno.args[0]);

// Simple TSX → JS transform for Quench
let js = code
  // Remove JSX pragma
  .replace(/\/\*\*\s*@jsx[^*]*\*\//g, "")
  // Remove import statements (Quench provides globals)
  .replace(/import\s+\{[^}]+\}\s+from\s+["\']ink["\'];?\n?/g, "")
  .replace(/import\s+\{[^}]+\}\s+from\s+["\']react["\'];?\n?/g, "")
  .replace(/import\s+React[^;]*;?\n?/g, "")
  // Remove TypeScript interfaces
  .replace(/interface\s+\w+\s*\{[^}]*\}\s*\n?/gs, "")
  // Remove type annotations on variables
  .replace(/:\s*(string|number|boolean|any|void|JSX\.Element|\(\)\s*=>\s*void|\{[^}]+\})(\s*[,;=)])/g, "$2")
  // Remove generic type parameters
  .replace(/<\w+>/g, "")
  // Remove as assertions
  .replace(/\s+as\s+\w+/g, "")
  // Convert JSX: <Box ...>...</Box> → createElement(Box, {...}, ...)
  .replace(/<([A-Z]\w+)([^>]*)>(.*?)<\/\1>/gs, (m, tag, props, children) => {
    const propStr = props.trim()
      .replace(/(\w+)=\{([^}]+)\}/g, "$1: $2")
      .replace(/(\w+)="([^"]*)"/g, "$1: \"$2\"")
      .replace(/\s+/g, ", ");
    return `createElement(${tag}, {${propStr}}, ${children})`;
  })
  // Self-closing JSX
  .replace(/<([A-Z]\w+)([^>]*?)\s*\/>/g, (m, tag, props) => {
    const propStr = props.trim()
      .replace(/(\w+)=\{([^}]+)\}/g, "$1: $2")
      .replace(/(\w+)="([^"]*)"/g, "$1: \"$2\"")
      .replace(/\s+/g, ", ");
    return `createElement(${tag}, {${propStr}})`;
  })
  // Remove "export default" 
  .replace(/export\s+default\s+/g, "")
  // Remove "export " from functions
  .replace(/export\s+/g, "");

await Deno.writeTextFile(Deno.args[1], js);
console.log("Done! " + Deno.args[1]);
' "$INPUT" "$OUTPUT"

echo ""
echo "Run with:"
echo "  ./target/release/quench $OUTPUT"
