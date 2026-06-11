// Chat UI Example - Quench demo
// Demonstrates real-time messaging, scrollable messages, and input
// Note: Box, Text, etc. are already globally exported from runtime.js

var useState = ink.useState;
var useEffect = ink.useEffect;
var useInput = ink.useInput;
var useApp = ink.useApp;
var render = ink.render;

var USERS = [
  { name: 'Alice', color: 'green' },
  { name: 'Bob', color: 'blue' },
  { name: 'Charlie', color: 'magenta' },
  { name: 'Diana', color: 'yellow' }
];

var GREETING_MESSAGES = [
  "Hello everyone!",
  "Hey there!",
  "Good to see you all",
  "What's up?",
  "Anyone online?",
  "Let's chat!",
  "How's it going?",
  "Nice to be here"
];

function App() {
  var _useState = useState([
    { id: 1, user: USERS[0], msg: 'Welcome to the chat room!' },
    { id: 2, user: USERS[1], msg: 'Thanks! Excited to be here.' },
    { id: 3, user: USERS[2], msg: 'Hey all!' }
  ]);
  var messages = _useState[0];
  var setMessages = _useState[1];
  
  var _useState2 = useState('');
  var inputText = _useState2[0];
  var setInputText = _useState2[1];
  
  var _useState3 = useState(0);
  var nextId = _useState3[0];
  var setNextId = _useState3[1];
  
  var _useState4 = useState(true);
  var autoScroll = _useState4[0];
  var setAutoScroll = _useState4[1];
  
  // Simulate incoming messages
  useEffect(function() {
    var timer = setInterval(function() {
      if (Math.random() > 0.7) {
        var user = USERS[Math.floor(Math.random() * USERS.length)];
        var msg = GREETING_MESSAGES[Math.floor(Math.random() * GREETING_MESSAGES.length)];
        setMessages(function(msgs) {
          return msgs.concat([{ id: nextId, user: user, msg: msg }]);
        });
        setNextId(function(id) { return id + 1; });
      }
    }, 2000);
    
    return function() { clearInterval(timer); };
  }, [nextId]);
  
  useInput(function(input) {
    if (input === 'q' || input === 'Q') {
      useApp().exit();
    }
    if (input === 'j' || input === 'downArrow') {
      setAutoScroll(false);
    }
    if (input === 'k' || input === 'upArrow') {
      setAutoScroll(false);
    }
    if (input === 'G') {
      setAutoScroll(true);
    }
    // Backspace
    if (input === 'backspace') {
      setInputText(function(t) { return t.slice(0, -1); });
    }
    // Enter - send
    if (input === 'return') {
      if (inputText.trim()) {
        setMessages(function(msgs) {
          return msgs.concat([{ 
            id: nextId, 
            user: { name: 'You', color: 'cyan' }, 
            msg: inputText 
          }]);
        });
        setInputText('');
        setNextId(function(id) { return id + 1; });
        setAutoScroll(true);
      }
    }
    // Regular character
    if (input.length === 1) {
      setInputText(function(t) { return t + input; });
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
              { type: Text, props: { color: 'green', bold: true, children: 'Chat Room' }},
              { type: Text, props: { dimColor: true, children: ' | ' + messages.length + ' messages' }},
              { type: Text, props: { color: autoScroll ? 'green' : 'gray', children: ' | auto-scroll: ' + (autoScroll ? 'ON' : 'OFF') }}
            ]
          }
        },
        // Messages area
        {
          type: Box,
          props: {
            flexDirection: 'column',
            borderStyle: 'round',
            children: messages.slice(-12).map(function(msg) {
              return {
                type: Box,
                props: {
                  children: [
                    { type: Text, props: { color: msg.user.color, bold: true, children: '<' + msg.user.name + '> ' }},
                    { type: Text, props: { children: msg.msg }}
                  ]
                }
              };
            })
          }
        },
        // Input area
        { type: Text, props: { children: '' }},
        {
          type: Box,
          props: {
            borderStyle: 'single',
            children: [
              { type: Text, props: { color: 'cyan', children: '> ' }},
              { type: Text, props: { children: inputText + '_' }}
            ]
          }
        },
        { type: Text, props: { dimColor: true, children: 'Type message | [Enter] send | [q] quit | [G] auto-scroll' }}
      ]
    }
  };
}

render({ type: App, props: {} });
