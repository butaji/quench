// Tabs Example - Quench demo
// Demonstrates tab navigation, state management, and dynamic content
// Note: Box, Text, etc. are already globally exported from runtime.js

var useState = ink.useState;
var useInput = ink.useInput;
var useApp = ink.useApp;
var render = ink.render;

function TabContent(props) {
  var tab = props.tab;
  var data = props.data;
  
  if (tab === 'home') {
    return {
      type: Box,
      props: {
        padding: 1,
        children: [
          { type: Text, props: { bold: true, children: 'Welcome to Quench!' }},
          { type: Text, props: { children: '' }},
          { type: Text, props: { children: 'A React-like framework for terminals.' }},
          { type: Text, props: { children: 'Write once, run in Deno and Quench.' }}
        ]
      }
    };
  }
  
  if (tab === 'about') {
    return {
      type: Box,
      props: {
        padding: 1,
        children: [
          { type: Text, props: { bold: true, children: 'About' }},
          { type: Text, props: { children: '' }},
          { type: Text, props: { children: 'Quench v0.1.0' }},
          { type: Text, props: { children: 'Built with Rust + rquickjs + ratatui' }},
          { type: Text, props: { children: 'Yoga layout engine for flexbox' }}
        ]
      }
    };
  }
  
  if (tab === 'data') {
    return {
      type: Box,
      props: {
        padding: 1,
        children: [
          { type: Text, props: { bold: true, children: 'Data View' }},
          { type: Text, props: { children: '' }},
          { type: Text, props: { children: 'Items: ' + data.length }}
        ].concat(data.slice(0, 5).map(function(item, i) {
          return { type: Text, props: { children: '  ' + (i + 1) + '. ' + item }};
        }))
      }
    };
  }
  
  return { type: Text, props: { children: 'Unknown tab' }};
}

function App() {
  var _useState = useState('home');
  var activeTab = _useState[0];
  var setActiveTab = _useState[1];
  
  var _useState2 = useState([
    'Alpha', 'Beta', 'Gamma', 'Delta', 'Epsilon', 'Zeta', 'Eta', 'Theta'
  ]);
  var data = _useState2[0];
  
  useInput(function(input) {
    if (input === 'q' || input === 'Q') {
      useApp().exit();
    }
    if (input === '1') setActiveTab('home');
    if (input === '2') setActiveTab('about');
    if (input === '3') setActiveTab('data');
    // Left/right navigation
    if (input === 'h' || input === 'leftArrow') {
      var tabs = ['home', 'about', 'data'];
      var idx = tabs.indexOf(activeTab);
      setActiveTab(tabs[(idx - 1 + tabs.length) % tabs.length]);
    }
    if (input === 'l' || input === 'rightArrow') {
      var tabs = ['home', 'about', 'data'];
      var idx = tabs.indexOf(activeTab);
      setActiveTab(tabs[(idx + 1) % tabs.length]);
    }
  });
  
  var tabs = [
    { id: 'home', label: 'Home' },
    { id: 'about', label: 'About' },
    { id: 'data', label: 'Data' }
  ];
  
  return {
    type: Box,
    props: {
      flexDirection: 'column',
      borderStyle: 'round',
      children: [
        // Tab bar
        { 
          type: Box, 
          props: { 
            borderStyle: 'bold',
            children: tabs.map(function(t) {
              var isActive = (t.id === activeTab);
              return {
                type: Text,
                props: {
                  color: isActive ? 'black' : 'white',
                  backgroundColor: isActive ? 'white' : undefined,
                  bold: isActive,
                  children: '[' + t.label + ']'
                }
              };
            })
          }
        },
        // Content
        { type: Text, props: { children: '' }},
        {
          type: TabContent,
          props: { tab: activeTab, data: data }
        },
        // Footer
        { type: Text, props: { children: '' }},
        { type: Text, props: { dimColor: true, children: '[1-3/h/l] switch tabs | [q] quit' }}
      ]
    }
  };
}

render({ type: App, props: {} });
