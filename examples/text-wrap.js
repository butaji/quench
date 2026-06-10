// Text Wrap Example — TuiBridge
// Demonstrates Ink text wrapping modes
// Covers: wrap, hard, truncate, truncate-start, truncate-middle, truncate-end

var useState = ink.useState;
var useInput = ink.useInput;
var useApp = ink.useApp;
var render = ink.render;

function TextWrapDemo() {
  var _useState = useState(0);
  var modeIdx = _useState[0];
  var setModeIdx = _useState[1];

  var modes = [
    { name: 'wrap', desc: 'Default — wraps at word boundaries' },
    { name: 'hard', desc: 'Fills each line to full width' },
    { name: 'truncate', desc: 'Cuts at end (alias: truncate-end)' },
    { name: 'truncate-start', desc: 'Cuts at start' },
    { name: 'truncate-middle', desc: 'Cuts in middle' },
  ];

  var longText = 'The quick brown fox jumps over the lazy dog';

  useInput(function(input) {
    if (input === 'q') useApp().exit();
    if (input === 'j') setModeIdx(function(i) { return Math.min(i + 1, modes.length - 1); });
    if (input === 'k') setModeIdx(function(i) { return Math.max(i - 1, 0); });
  });

  var current = modes[modeIdx];

  return {
    type: Box,
    props: {
      flexDirection: 'column',
      padding: 1,
      borderStyle: 'round',
      children: [
        { type: Text, props: { bold: true, color: 'green', children: 'Text Wrap Demo' } },
        { type: Text, props: { dimColor: true, children: '[j/k] mode | [q] quit' } },
        { type: Text, props: { children: 'Mode: ' + current.name + ' — ' + current.desc } },
        { type: Text, props: { children: '' } },
        // Demo box with constrained width
        {
          type: Box,
          props: {
            width: 12,
            borderStyle: 'single',
            padding: 1,
            children: [
              { type: Text, props: { textWrap: current.name, children: longText } }
            ]
          }
        },
        { type: Text, props: { children: '' } },
        // Mode selector
        {
          type: Box,
          props: {
            flexDirection: 'column',
            children: modes.map(function(m, i) {
              var isActive = i === modeIdx;
              return {
                type: Box,
                props: {
                  flexDirection: 'row',
                  children: [
                    { type: Text, props: { color: isActive ? 'yellow' : 'gray', children: isActive ? '> ' : '  ' } },
                    { type: Text, props: { bold: isActive, children: m.name } },
                    { type: Text, props: { dimColor: true, children: ' — ' + m.desc } }
                  ]
                }
              };
            })
          }
        }
      ]
    }
  };
}

render({ type: TextWrapDemo, props: {} });
