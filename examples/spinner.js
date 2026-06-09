// Spinner Example - TuiBridge demo
// Demonstrates timer-driven animation and color cycling
// Note: Box, Text, etc. are already globally exported from runtime.js

var useState = ink.useState;
var useEffect = ink.useEffect;
var useInput = ink.useInput;
var useApp = ink.useApp;
var render = ink.render;

var SPINNER_FRAMES = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
var COLORS = ['cyan', 'magenta', 'yellow', 'green', 'blue', 'red'];

function Spinner() {
  var _useState = useState(0);
  var frame = _useState[0];
  var setFrame = _useState[1];
  
  var _useState2 = useState(0);
  var colorIndex = _useState2[0];
  var setColorIndex = _useState2[1];
  
  useEffect(function() {
    var timer = setInterval(function() {
      setFrame(function(f) { return (f + 1) % SPINNER_FRAMES.length; });
      setColorIndex(function(c) { return (c + 1) % COLORS.length; });
    }, 80);
    
    return function() {
      clearInterval(timer);
    };
  }, []);
  
  useInput(function(input) {
    if (input === 'q' || input === 'Q') {
      useApp().exit();
    }
  });
  
  var currentColor = COLORS[colorIndex];
  var spinnerChar = SPINNER_FRAMES[frame];
  
  return {
    type: Box,
    props: {
      flexDirection: 'column',
      padding: 2,
      borderStyle: 'round',
      children: [
        { type: Text, props: { color: 'green', bold: true, children: 'Spinner Demo' } },
        { type: Text, props: { children: '' } },
        { 
          type: Box, 
          props: { 
            justifyContent: 'center',
            children: [
              { type: Text, props: { color: currentColor, bold: true, children: spinnerChar + ' Loading...' }}
            ]
          }
        },
        { type: Text, props: { children: '' } },
        { type: Text, props: { dimColor: true, children: 'Color: ' + currentColor + ' | Frame: ' + (frame + 1) + '/' + SPINNER_FRAMES.length }},
        { type: Text, props: { dimColor: true, children: '[q] quit' }}
      ]
    }
  };
}

render({ type: Spinner, props: {} });
