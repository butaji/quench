// Flex Layouts Example — TuiBridge
// Demonstrates flexbox props: flexGrow, flexShrink, flexBasis, flexWrap, alignItems, justifyContent

var useState = ink.useState;
var useInput = ink.useInput;
var useApp = ink.useApp;
var render = ink.render;

function FlexLayouts() {
  var _useState = useState('row');
  var direction = _useState[0];
  var setDirection = _useState[1];

  var _useState2 = useState('flex-start');
  var align = _useState2[0];
  var setAlign = _useState2[1];

  var _useState3 = useState('flex-start');
  var justify = _useState3[0];
  var setJustify = _useState3[1];

  var _useState4 = useState(0);
  var mode = _useState4[0];
  var setMode = _useState4[1];

  var modes = ['direction', 'align', 'justify', 'grow', 'wrap'];

  useInput(function(input) {
    if (input === 'q') useApp().exit();
    if (input === 'h') setMode(function(m) { return (m + modes.length - 1) % modes.length; });
    if (input === 'l') setMode(function(m) { return (m + 1) % modes.length; });
    if (input === '1') setDirection('row');
    if (input === '2') setDirection('column');
    if (input === '3') setAlign('flex-start');
    if (input === '4') setAlign('center');
    if (input === '5') setAlign('flex-end');
    if (input === '6') setAlign('stretch');
    if (input === '7') setJustify('flex-start');
    if (input === '8') setJustify('center');
    if (input === '9') setJustify('space-between');
    if (input === '0') setJustify('space-around');
  });

  var currentMode = modes[mode];

  return {
    type: Box,
    props: {
      flexDirection: 'column',
      padding: 1,
      borderStyle: 'round',
      children: [
        { type: Text, props: { bold: true, color: 'green', children: 'Flex Layout Demo' } },
        { type: Text, props: { dimColor: true, children: '[h/l] mode | [1-0] presets | [q] quit' } },
        { type: Text, props: { children: 'Mode: ' + currentMode + ' | dir=' + direction + ' | align=' + align + ' | justify=' + justify } },
        { type: Text, props: { children: '' } },
        // Demo area
        {
          type: Box,
          props: {
            flexDirection: direction,
            alignItems: align,
            justifyContent: justify,
            borderStyle: 'single',
            padding: 1,
            height: 10,
            children: [
              { type: Box, props: { borderStyle: 'single', padding: 1, flexGrow: currentMode === 'grow' ? 1 : 0, children: [{ type: Text, props: { color: 'red', children: 'Box 1' } }] } },
              { type: Box, props: { borderStyle: 'single', padding: 1, flexGrow: currentMode === 'grow' ? 2 : 0, children: [{ type: Text, props: { color: 'green', children: 'Box 2' } }] } },
              { type: Box, props: { borderStyle: 'single', padding: 1, flexGrow: currentMode === 'grow' ? 1 : 0, children: [{ type: Text, props: { color: 'blue', children: 'Box 3' } }] } },
            ]
          }
        },
        { type: Text, props: { children: '' } },
        // flexWrap demo
        currentMode === 'wrap' ? {
          type: Box,
          props: {
            flexDirection: 'row',
            flexWrap: 'wrap',
            borderStyle: 'single',
            padding: 1,
            children: [
              { type: Box, props: { borderStyle: 'single', paddingX: 2, margin: 1, children: [{ type: Text, props: { children: 'Wrap 1' } }] } },
              { type: Box, props: { borderStyle: 'single', paddingX: 2, margin: 1, children: [{ type: Text, props: { children: 'Wrap 2' } }] } },
              { type: Box, props: { borderStyle: 'single', paddingX: 2, margin: 1, children: [{ type: Text, props: { children: 'Wrap 3' } }] } },
              { type: Box, props: { borderStyle: 'single', paddingX: 2, margin: 1, children: [{ type: Text, props: { children: 'Wrap 4' } }] } },
              { type: Box, props: { borderStyle: 'single', paddingX: 2, margin: 1, children: [{ type: Text, props: { children: 'Wrap 5' } }] } },
              { type: Box, props: { borderStyle: 'single', paddingX: 2, margin: 1, children: [{ type: Text, props: { children: 'Wrap 6' } }] } },
            ]
          }
        } : { type: Text, props: { dimColor: true, children: 'Switch to "wrap" mode to see flexWrap demo' } }
      ]
    }
  };
}

render({ type: FlexLayouts, props: {} });
