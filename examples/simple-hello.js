// Simple Hello World — uses Rust-exposed Ink API (no JS shim needed)
// Run with: tuibridge examples/simple-hello.js

var element = {
  type: Box,
  props: {
    flexDirection: 'column',
    padding: 1,
    borderStyle: 'round',
    children: [
      { type: Text, props: { color: 'green', bold: true, children: 'Hello from TuiBridge!' } },
      { type: Text, props: { children: 'This is a simple test' } },
      { type: Text, props: { dimColor: true, children: '[q] quit' } },
    ]
  }
};

var instance = render(element, {});

console.log('Tree built successfully!');
console.log('Press Ctrl+C to exit or call useApp().exit()');
