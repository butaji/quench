// Wizard Example — TuiBridge
// Demonstrates useMemo, useCallback, multi-step flow

var useState = ink.useState;
var useMemo = ink.useMemo;
var useCallback = ink.useCallback;
var useInput = ink.useInput;
var useApp = ink.useApp;
var render = ink.render;

function Wizard() {
  var _useState = useState(0);
  var step = _useState[0];
  var setStep = _useState[1];

  var _useState2 = useState({ name: '', email: '', role: 'dev' });
  var data = _useState2[0];
  var setData = _useState2[1];

  var steps = ['Name', 'Email', 'Role', 'Review'];

  var canNext = useMemo(function() {
    if (step === 0) return data.name.length > 0;
    if (step === 1) return data.email.length > 0;
    if (step === 2) return true;
    return false;
  }, [step, data]);

  var canPrev = useMemo(function() {
    return step > 0;
  }, [step]);

  var next = useCallback(function() {
    if (canNext) setStep(function(s) { return Math.min(s + 1, steps.length - 1); });
  }, [canNext]);

  var prev = useCallback(function() {
    if (canPrev) setStep(function(s) { return Math.max(s - 1, 0); });
  }, [canPrev]);

  useInput(function(input) {
    if (input === 'q') useApp().exit();
    if (input === 'l' || input === 'rightArrow') next();
    if (input === 'h' || input === 'leftArrow') prev();
  });

  return {
    type: Box,
    props: {
      flexDirection: 'column',
      padding: 1,
      borderStyle: 'round',
      children: [
        { type: Text, props: { bold: true, color: 'green', children: 'Setup Wizard' } },
        {
          type: Box,
          props: {
            flexDirection: 'row',
            marginY: 1,
            children: steps.map(function(s, i) {
              var color = i === step ? 'yellow' : (i < step ? 'green' : 'gray');
              var marker = i === step ? '► ' : (i < step ? '✓ ' : '○ ');
              return { type: Text, props: { color: color, children: marker + s + '  ' } };
            })
          }
        },
        // Step content
        step === 0 ? {
          type: Box,
          props: {
            flexDirection: 'column',
            children: [
              { type: Text, props: { children: 'Enter your name:' } },
              { type: Text, props: { bold: true, children: data.name || '(empty)' } }
            ]
          }
        } : step === 1 ? {
          type: Box,
          props: {
            flexDirection: 'column',
            children: [
              { type: Text, props: { children: 'Enter your email:' } },
              { type: Text, props: { bold: true, children: data.email || '(empty)' } }
            ]
          }
        } : step === 2 ? {
          type: Box,
          props: {
            flexDirection: 'column',
            children: [
              { type: Text, props: { children: 'Select your role:' } },
              { type: Text, props: { bold: true, children: data.role } }
            ]
          }
        } : {
          type: Box,
          props: {
            flexDirection: 'column',
            borderStyle: 'single',
            padding: 1,
            children: [
              { type: Text, props: { bold: true, children: 'Review' } },
              { type: Text, props: { children: 'Name:  ' + data.name } },
              { type: Text, props: { children: 'Email: ' + data.email } },
              { type: Text, props: { children: 'Role:  ' + data.role } }
            ]
          }
        },
        { type: Text, props: { children: '' } },
        { type: Text, props: { dimColor: true, children: '[h/←] back | [l/→] next | [q] quit' } }
      ]
    }
  };
}

render({ type: Wizard, props: {} });
