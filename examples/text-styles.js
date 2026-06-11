// Text Styles Example — Quench
// Demonstrates all Ink text styling props
// Covers: italic, strikethrough, underline, inverse, transform, textWrap

var useState = ink.useState;
var useInput = ink.useInput;
var useApp = ink.useApp;
var render = ink.render;

function TextStyles() {
  var _useState = useState(0);
  var activeIdx = _useState[0];
  var setActiveIdx = _useState[1];

  var styles = [
    { label: 'Normal', props: {} },
    { label: 'Bold', props: { bold: true } },
    { label: 'Dim', props: { dimColor: true } },
    { label: 'Italic', props: { italic: true } },
    { label: 'Underline', props: { underline: true } },
    { label: 'Strikethrough', props: { strikethrough: true } },
    { label: 'Inverse', props: { inverse: true } },
    { label: 'Uppercase', props: { transform: 'uppercase' } },
    { label: 'Lowercase', props: { transform: 'lowercase' } },
    { label: 'Red bg', props: { backgroundColor: 'red', color: 'white' } },
  ];

  useInput(function(input) {
    if (input === 'q') useApp().exit();
    if (input === 'j') setActiveIdx(function(i) { return Math.min(i + 1, styles.length - 1); });
    if (input === 'k') setActiveIdx(function(i) { return Math.max(i - 1, 0); });
  });

  return {
    type: Box,
    props: {
      flexDirection: 'column',
      padding: 1,
      borderStyle: 'round',
      children: [
        { type: Text, props: { bold: true, color: 'green', children: 'Text Styles Demo' } },
        { type: Text, props: { dimColor: true, children: '[j/k] navigate | [q] quit' } },
        { type: Text, props: { children: '' } },
        // Active style preview
        {
          type: Box,
          props: {
            borderStyle: 'single',
            padding: 1,
            borderColor: 'yellow',
            children: [
              { type: Text, props: { dimColor: true, children: 'Preview: ' } },
              { type: Text, props: Object.assign({ children: styles[activeIdx].label + ' text' }, styles[activeIdx].props) }
            ]
          }
        },
        { type: Text, props: { children: '' } },
        // Style list
        {
          type: Box,
          props: {
            flexDirection: 'column',
            children: styles.map(function(s, i) {
              var isActive = i === activeIdx;
              return {
                type: Box,
                props: {
                  flexDirection: 'row',
                  children: [
                    { type: Text, props: { color: isActive ? 'yellow' : 'gray', children: isActive ? '> ' : '  ' } },
                    { type: Text, props: Object.assign({ children: s.label }, s.props) }
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

render({ type: TextStyles, props: {} });
