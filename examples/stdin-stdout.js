// Stdin/Stdout/Stderr Demo — TuiBridge
// Demonstrates useStdin, useStdout, useStderr hooks

var useState = ink.useState;
var useStdin = ink.useStdin;
var useStdout = ink.useStdout;
var useStderr = ink.useStderr;
var useInput = ink.useInput;
var useApp = ink.useApp;
var render = ink.render;

function StdinStdoutDemo() {
  var stdin = useStdin();
  var stdout = useStdout();
  var stderr = useStderr();

  var _useState = useState([]);
  var events = _useState[0];
  var setEvents = _useState[1];

  function addEvent(msg) {
    setEvents(function(e) {
      var next = e.slice(-9);
      next.push(msg);
      return next;
    });
  }

  useInput(function(input, key) {
    if (input === 'q') {
      useApp().exit();
      return;
    }
    if (input === 's') {
      stdout.write('Direct stdout write: hello from useStdout\\n');
      addEvent('stdout.write() called');
      return;
    }
    if (input === 'e') {
      stderr.write('Direct stderr write: error from useStderr\\n');
      addEvent('stderr.write() called');
      return;
    }
    addEvent('key: ' + input + ' (ctrl=' + key.ctrl + ' shift=' + key.shift + ')');
  });

  return {
    type: Box,
    props: {
      flexDirection: 'column',
      padding: 1,
      borderStyle: 'round',
      children: [
        { type: Text, props: { bold: true, color: 'green', children: 'Stdin / Stdout / Stderr Hooks' } },
        { type: Text, props: { dimColor: true, children: '[s] stdout | [e] stderr | type for stdin | [q] quit' } },
        { type: Text, props: { children: '' } },
        {
          type: Box,
          props: {
            flexDirection: 'row',
            children: [
              { type: Text, props: { color: 'cyan', children: 'RawMode: ' + stdin.isRawMode() + '  ' } },
              { type: Text, props: { color: 'cyan', children: 'Columns: ' + stdout.columns } }
            ]
          }
        },
        { type: Text, props: { children: '' } },
        // Event log
        {
          type: Box,
          props: {
            flexDirection: 'column',
            borderStyle: 'single',
            padding: 1,
            height: 8,
            children: [
              { type: Text, props: { bold: true, children: 'Event Log:' } },
              events.length === 0 ?
                { type: Text, props: { dimColor: true, children: '(no events yet)' } } :
                { type: Box, props: { flexDirection: 'column', children: events.map(function(e) {
                  return { type: Text, props: { children: e } };
                }) } }
            ]
          }
        }
      ]
    }
  };
}

render({ type: StdinStdoutDemo, props: {} });
