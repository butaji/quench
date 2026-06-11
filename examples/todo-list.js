// Todo List Example - Quench demo
// Demonstrates useState, useInput, nested flex layouts, keyboard navigation
// Note: Box, Text, etc. are already globally exported from runtime.js

var useState = ink.useState;
var useInput = ink.useInput;
var useApp = ink.useApp;
var render = ink.render;

function App() {
  var _useState = useState([
    { id: 1, text: 'Learn Quench', done: true },
    { id: 2, text: 'Build a TUI app', done: false },
    { id: 3, text: 'Ship to production', done: false }
  ]);
  var todos = _useState[0];
  var setTodos = _useState[1];
  
  var _useState2 = useState(0);
  var selected = _useState2[0];
  var setSelected = _useState2[1];
  
  useInput(function(input) {
    if (input === 'q' || input === 'Q') {
      useApp().exit();
    }
    // Navigate with j/k
    if (input === 'j') {
      setSelected(function(s) { return Math.min(s + 1, todos.length - 1); });
    }
    if (input === 'k') {
      setSelected(function(s) { return Math.max(s - 1, 0); });
    }
    // Toggle with space
    if (input === ' ') {
      setTodos(function(t) {
        return t.map(function(item, i) {
          if (i === selected) {
            return { id: item.id, text: item.text, done: !item.done };
          }
          return item;
        });
      });
    }
    // Delete with d
    if (input === 'd') {
      setTodos(function(t) {
        return t.filter(function(_, i) { return i !== selected; });
      });
      setSelected(function(s) { return Math.min(s, todos.length - 2); });
    }
  });
  
  // Filter
  var _useState3 = useState('all');
  var filter = _useState3[0];
  var setFilter = _useState3[1];
  
  var filteredTodos = todos.filter(function(t) {
    if (filter === 'active') return !t.done;
    if (filter === 'done') return t.done;
    return true;
  });
  
  return {
    type: Box,
    props: {
      flexDirection: 'column',
      padding: 1,
      borderStyle: 'round',
      children: [
        { type: Text, props: { color: 'green', bold: true, children: 'Todo List' } },
        { type: Text, props: { dimColor: true, children: '[j/k] navigate | [space] toggle | [d] delete | [q] quit' } },
        { type: Text, props: { children: '' } },
        { type: Box, props: { 
          flexDirection: 'column',
          children: filteredTodos.map(function(item, i) {
            var isSelected = (i === selected);
            return {
              type: Box,
              props: {
                children: [
                  { 
                    type: Text, 
                    props: { 
                      children: isSelected ? '> ' : '  ',
                      color: isSelected ? 'yellow' : 'gray'
                    }
                  },
                  { type: Text, props: { 
                    children: (item.done ? '[x] ' : '[ ] ') + item.text,
                    color: item.done ? 'gray' : (isSelected ? 'white' : 'white')
                  }}
                ]
              }
            };
          })
        }},
        { type: Text, props: { children: '' } },
        { type: Text, props: { dimColor: true, children: 'Items: ' + todos.length + ' | Done: ' + todos.filter(function(t) { return t.done; }).length }}
      ]
    }
  };
}

render({ type: App, props: {} });
