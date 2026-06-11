// Focus Form Example - Quench demo
// Demonstrates focus management, input handling, and form state
// Note: Box, Text, etc. are already globally exported from runtime.js

var useState = ink.useState;
var useInput = ink.useInput;
var useApp = ink.useApp;
var render = ink.render;

function InputField(props) {
  var label = props.label;
  var value = props.value;
  var isFocused = props.isFocused;
  
  return {
    type: Box,
    props: {
      margin: 1,
      children: [
        { type: Text, props: { children: label + ': ' }},
        { 
          type: Text, 
          props: { 
            color: isFocused ? 'green' : 'gray',
            bold: isFocused,
            children: isFocused ? value + '_' : (value || '(empty)')
          }
        }
      ]
    }
  };
}

function App() {
  var _useState = useState('');
  var name = _useState[0];
  var setName = _useState[1];
  
  var _useState2 = useState('');
  var email = _useState2[0];
  var setEmail = _useState2[1];
  
  var _useState3 = useState('');
  var password = _useState3[0];
  var setPassword = _useState3[1];
  
  var _useState4 = useState(0);
  var focusIndex = _useState4[0];
  var setFocusIndex = _useState4[1];
  
  var _useState5 = useState('');
  var status = _useState5[0];
  var setStatus = _useState5[1];
  
  var fields = [
    { label: 'Name', value: name, setter: setName },
    { label: 'Email', value: email, setter: setEmail },
    { label: 'Password', value: password, setter: setPassword }
  ];
  
  useInput(function(input) {
    if (input === 'q' || input === 'Q') {
      useApp().exit();
    }
    // Tab to next field
    if (input === 'tab') {
      setFocusIndex(function(i) { return (i + 1) % 3; });
    }
    // Arrow navigation
    if (input === 'upArrow') {
      setFocusIndex(function(i) { return (i - 1 + 3) % 3; });
    }
    if (input === 'downArrow') {
      setFocusIndex(function(i) { return (i + 1) % 3; });
    }
    // Backspace
    if (input === 'backspace') {
      var current = fields[focusIndex].value;
      fields[focusIndex].setter(current.slice(0, -1));
    }
    // Enter - submit
    if (input === 'return') {
      if (name && email && password) {
        setStatus('Submitting: ' + name + ' <' + email + '>');
      } else {
        setStatus('Error: All fields required');
      }
    }
    // Regular character input
    if (input.length === 1) {
      fields[focusIndex].setter(function(v) { return (v || '') + input; });
    }
  });
  
  return {
    type: Box,
    props: {
      flexDirection: 'column',
      padding: 1,
      borderStyle: 'round',
      children: [
        { type: Text, props: { color: 'green', bold: true, children: 'Focus Form' }},
        { type: Text, props: { dimColor: true, children: 'Type to input | [tab] next field | [enter] submit' }},
        { type: Text, props: { children: '' }},
        { type: Box, props: { 
          flexDirection: 'column',
          borderStyle: 'single',
          children: fields.map(function(field, i) {
            return {
              type: InputField,
              props: {
                label: field.label,
                value: field.value,
                isFocused: (i === focusIndex)
              }
            };
          })
        }},
        { type: Text, props: { children: '' }},
        status ? { type: Text, props: { color: status.startsWith('Error') ? 'red' : 'green', children: status }} : null,
        { type: Text, props: { dimColor: true, children: '[q] quit' }}
      ]
    }
  };
}

render({ type: App, props: {} });
