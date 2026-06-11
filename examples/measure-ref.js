// Measure Ref Demo — Quench
// Demonstrates useRef + measureElement for responsive layout

var useState = ink.useState;
var useRef = ink.useRef;
var useEffect = ink.useEffect;
var useInput = ink.useInput;
var useApp = ink.useApp;
var measureElement = ink.measureElement;
var render = ink.render;

function MeasureRefDemo() {
  var boxRef = useRef({});
  var _useState = useState({ width: 0, height: 0 });
  var dims = _useState[0];
  var setDims = _useState[1];

  useEffect(function() {
    var timer = setInterval(function() {
      var m = measureElement(boxRef);
      if (m) setDims(m);
    }, 500);
    return function() { clearInterval(timer); };
  }, []);

  useInput(function(input) {
    if (input === 'q') useApp().exit();
  });

  return {
    type: Box,
    props: {
      flexDirection: 'column',
      padding: 1,
      borderStyle: 'round',
      children: [
        { type: Text, props: { bold: true, color: 'green', children: 'measureElement + useRef Demo' } },
        { type: Text, props: { dimColor: true, children: 'Resize terminal to see dimensions update | [q] quit' } },
        { type: Text, props: { children: '' } },
        {
          type: Box,
          props: {
            ref: boxRef,
            borderStyle: 'single',
            padding: 2,
            flexGrow: 1,
            children: [
              { type: Text, props: { bold: true, children: 'Tracked Box' } },
              { type: Text, props: { children: 'Width: ' + dims.width.toFixed(1) + ' cols' } },
              { type: Text, props: { children: 'Height: ' + dims.height.toFixed(1) + ' rows' } }
            ]
          }
        },
        { type: Text, props: { children: '' } },
        { type: Text, props: { dimColor: true, children: 'This box uses flexGrow=1 to fill available space.' } },
        { type: Text, props: { dimColor: true, children: 'measureElement reads Yoga-computed layout after each commit.' } }
      ]
    }
  };
}

render({ type: MeasureRefDemo, props: {} });
