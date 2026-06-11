// Dashboard Example - Quench demo
// Demonstrates layout with multiple sections, live stats, and borders
// Note: Box, Text, etc. are already globally exported from runtime.js

var useState = ink.useState;
var useEffect = ink.useEffect;
var useInput = ink.useInput;
var useApp = ink.useApp;
var render = ink.render;

function ProgressBar(props) {
  var label = props.label;
  var value = props.value;
  var max = props.max || 100;
  var width = props.width || 20;
  
  var percent = Math.round((value / max) * 100);
  var filled = Math.round((value / max) * width);
  var empty = width - filled;
  
  var bar = '█'.repeat(filled) + '░'.repeat(empty);
  var color = percent > 80 ? 'green' : (percent > 50 ? 'yellow' : 'red');
  
  return {
    type: Box,
    props: {
      flexDirection: 'column',
      margin: 1,
      children: [
        { type: Text, props: { children: label + ': ' + percent + '%' }},
        { type: Text, props: { color: color, children: '[' + bar + ']' }}
      ]
    }
  };
}

function App() {
  var _useState = useState({
    cpu: 45,
    memory: 67,
    disk: 23,
    network: 89
  });
  var metrics = _useState[0];
  var setMetrics = _useState[1];
  
  var _useState2 = useState(0);
  var uptime = _useState2[0];
  var setUptime = _useState2[1];
  
  useEffect(function() {
    var timer = setInterval(function() {
      // Simulate changing metrics
      setMetrics(function(m) {
        return {
          cpu: Math.max(0, Math.min(100, m.cpu + Math.round((Math.random() - 0.5) * 20))),
          memory: Math.max(0, Math.min(100, m.memory + Math.round((Math.random() - 0.5) * 5))),
          disk: 23, // Static
          network: Math.max(0, Math.min(100, m.network + Math.round((Math.random() - 0.5) * 30)))
        };
      });
      setUptime(function(u) { return u + 1; });
    }, 1000);
    
    return function() { clearInterval(timer); };
  }, []);
  
  useInput(function(input) {
    if (input === 'q' || input === 'Q') {
      useApp().exit();
    }
  });
  
  var hours = Math.floor(uptime / 3600);
  var minutes = Math.floor((uptime % 3600) / 60);
  var seconds = uptime % 60;
  
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
              { type: Text, props: { color: 'green', bold: true, children: 'System Dashboard' }},
              { type: Text, props: { dimColor: true, children: '   Uptime: ' + hours + 'h ' + minutes + 'm ' + seconds + 's' }}
            ]
          }
        },
        { type: Text, props: { children: '' }},
        // Stats row
        { 
          type: Box, 
          props: { 
            children: [
              { type: Box, props: {
                borderStyle: 'round',
                padding: 1,
                margin: 1,
                children: [
                  { type: Text, props: { dimColor: true, children: 'CPU' }},
                  { type: Text, props: { bold: true, color: metrics.cpu > 80 ? 'red' : 'green', children: metrics.cpu + '%' }}
                ]
              }},
              { type: Box, props: {
                borderStyle: 'round',
                padding: 1,
                margin: 1,
                children: [
                  { type: Text, props: { dimColor: true, children: 'Memory' }},
                  { type: Text, props: { bold: true, color: metrics.memory > 80 ? 'red' : 'yellow', children: metrics.memory + '%' }}
                ]
              }},
              { type: Box, props: {
                borderStyle: 'round',
                padding: 1,
                margin: 1,
                children: [
                  { type: Text, props: { dimColor: true, children: 'Disk' }},
                  { type: Text, props: { bold: true, color: 'cyan', children: metrics.disk + '%' }}
                ]
              }},
              { type: Box, props: {
                borderStyle: 'round',
                padding: 1,
                margin: 1,
                children: [
                  { type: Text, props: { dimColor: true, children: 'Network' }},
                  { type: Text, props: { bold: true, color: 'magenta', children: metrics.network + ' Mbps' }}
                ]
              }}
            ]
          }
        },
        { type: Text, props: { children: '' }},
        // Progress bars
        { 
          type: Box, 
          props: { 
            flexDirection: 'column',
            borderStyle: 'round',
            children: [
              { type: Text, props: { bold: true, children: 'Resource Usage' }},
              { type: ProgressBar, props: { label: 'CPU', value: metrics.cpu }},
              { type: ProgressBar, props: { label: 'Memory', value: metrics.memory }},
              { type: ProgressBar, props: { label: 'Disk', value: metrics.disk }},
              { type: ProgressBar, props: { label: 'Network', value: metrics.network }}
            ]
          }
        },
        { type: Text, props: { dimColor: true, children: '[q] quit' }}
      ]
    }
  };
}

render({ type: App, props: {} });
