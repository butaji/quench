// Counter Example - TuiBridge demo
// Demonstrates useState, useEffect, useInput with the Ink API
// Note: Box, Text, etc. are already globally exported from runtime.js

var useState = ink.useState;
var useEffect = ink.useEffect;
var useInput = ink.useInput;
var useApp = ink.useApp;
var render = ink.render;

// Simple Counter component
function Counter() {
  var _useState = useState(0),
      count = _useState[0],
      setCount = _useState[1];

  // Handle input
  useInput(function(input) {
    if (input === 'q' || input === 'Q') {
      useApp().exit();
    }
    if (input === ' ') {
      setCount(function(c) { return c + 1; });
    }
  });

  // Auto-increment with timer
  useEffect(function() {
    var timer = setInterval(function() {
      setCount(function(c) { return c + 1; });
    }, 1000);

    return function() {
      clearInterval(timer);
    };
  }, []);

  // Return an element tree (not call render!)
  return {
    type: Box,
    props: {
      flexDirection: 'column',
      padding: 1,
      borderStyle: 'round',
      children: [
        { type: Text, props: { color: 'green', bold: true, children: 'Counter App' } },
        { type: Text, props: { children: 'Count: ' + count } },
        { type: Text, props: { dimColor: true, children: '[space] increment | [q] quit' } },
      ]
    }
  };
}

// Run the app — render the root component
render({ type: Counter, props: {} });
