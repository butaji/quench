// Mouse App Example - Quench demo
// Demonstrates mouse event handling, clickable areas, and coordinates
// Note: Box, Text, etc. are already globally exported from runtime.js

var useState = ink.useState;
var useInput = ink.useInput;
var useApp = ink.useApp;
var render = ink.render;

function App() {
  var _useState = useState(null);
  var mousePos = _useState[0];
  var setMousePos = _useState[1];
  
  var _useState2 = useState(0);
  var clickCount = _useState2[0];
  var setClickCount = _useState2[1];
  
  var _useState3 = useState([]);
  var clicks = _useState3[0];
  var setClicks = _useState3[1];
  
  useInput(function(input) {
    if (input === 'q' || input === 'Q') {
      useApp().exit();
    }
  });
  
  return {
    type: Box,
    props: {
      flexDirection: 'column',
      padding: 1,
      children: [
        // Header
        { 
          type: Box, 
          props: { 
            borderStyle: 'bold',
            children: [
              { type: Text, props: { color: 'green', bold: true, children: 'Mouse Demo' }},
              { type: Text, props: { dimColor: true, children: ' | Move/click/hover to see events' }}
            ]
          }
        },
        { type: Text, props: { children: '' }},
        // Mouse position display
        {
          type: Box,
          props: {
            borderStyle: 'round',
            padding: 1,
            children: [
              { type: Text, props: { bold: true, children: 'Mouse Position' }},
              { type: Text, props: { children: '' }},
              { type: Text, props: { children: 'Click count: ' + clickCount }}
            ]
          }
        },
        { type: Text, props: { children: '' }},
        // Clickable buttons
        {
          type: Box,
          props: {
            children: [
              {
                type: Box,
                props: {
                  borderStyle: 'round',
                  padding: 1,
                  margin: 1,
                  children: [
                    { type: Text, props: { color: 'green', bold: true, children: 'Button 1' }}
                  ]
                }
              },
              {
                type: Box,
                props: {
                  borderStyle: 'round',
                  padding: 1,
                  margin: 1,
                  children: [
                    { type: Text, props: { color: 'blue', bold: true, children: 'Button 2' }}
                  ]
                }
              },
              {
                type: Box,
                props: {
                  borderStyle: 'round',
                  padding: 1,
                  margin: 1,
                  children: [
                    { type: Text, props: { color: 'magenta', bold: true, children: 'Button 3' }}
                  ]
                }
              }
            ]
          }
        },
        { type: Text, props: { children: '' }},
        // Click history
        {
          type: Box,
          props: {
            flexDirection: 'column',
            borderStyle: 'round',
            children: [
              { type: Text, props: { bold: true, children: 'Click History' }},
              { type: Text, props: { dimColor: true, children: clicks.length + ' clicks recorded' }}
            ].concat(clicks.slice(-3).map(function(msg) {
              return { type: Text, props: { color: 'green', children: '> ' + msg }};
            }))
          }
        },
        { type: Text, props: { children: '' }},
        // Instructions
        { type: Text, props: { dimColor: true, children: '[q] quit' }},
        { type: Text, props: { dimColor: true, children: 'Note: Mouse tracking requires terminal with mouse support' }}
      ]
    }
  };
}

render({ type: App, props: {} });
