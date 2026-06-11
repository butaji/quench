// Log Viewer Example - Quench demo
// Demonstrates scrolling content, auto-scroll, and styled log levels
// Note: Box, Text, etc. are already globally exported from runtime.js

var useState = ink.useState;
var useEffect = ink.useEffect;
var useInput = ink.useInput;
var useApp = ink.useApp;
var render = ink.render;

var LOG_LEVELS = ['DEBUG', 'INFO', 'WARN', 'ERROR'];
var SAMPLE_MESSAGES = [
  { level: 0, msg: 'Starting application server' },
  { level: 1, msg: 'Database connection established' },
  { level: 0, msg: 'Loading configuration from config.yaml' },
  { level: 1, msg: 'HTTP server listening on port 8080' },
  { level: 2, msg: 'High memory usage detected: 85%' },
  { level: 1, msg: 'New client connected from 192.168.1.100' },
  { level: 0, msg: 'Processing request: GET /api/users' },
  { level: 1, msg: 'Query executed in 23ms' },
  { level: 3, msg: 'Connection refused: database unavailable' },
  { level: 1, msg: 'Retrying database connection...' },
  { level: 1, msg: 'Database reconnected successfully' },
  { level: 2, msg: 'Slow query detected: 2.3s' },
  { level: 0, msg: 'Request completed: 200 OK (145ms)' },
  { level: 1, msg: 'Cache hit for key: user_123' },
  { level: 0, msg: 'Background job started: cleanup_sessions' },
  { level: 1, msg: 'Session cleanup: removed 47 expired sessions' },
  { level: 2, msg: 'Disk space low: 12% remaining' },
  { level: 0, msg: 'Health check passed' },
  { level: 1, msg: 'Client disconnected: 192.168.1.100' },
  { level: 0, msg: 'Scheduled task completed: daily_stats' }
];

function getLevelColor(level) {
  if (level === 0) return 'gray';    // DEBUG
  if (level === 1) return 'cyan';    // INFO
  if (level === 2) return 'yellow';  // WARN
  if (level === 3) return 'red';     // ERROR
  return 'white';
}

function getLevelName(level) {
  return LOG_LEVELS[level] || 'UNKNOWN';
}

function App() {
  var _useState = useState([]);
  var logs = _useState[0];
  var setLogs = _useState[1];
  
  var _useState2 = useState(0);
  var autoScroll = _useState2[0];
  var setAutoScroll = _useState2[1];
  
  var _useState3 = useState(0); // 0=all, 1=info+, 2=warn+, 3=error
  var filterLevel = _useState3[0];
  var setFilterLevel = _useState3[1];
  
  var _useState4 = useState(true);
  var pauseNewLogs = _useState4[0];
  var setPauseNewLogs = _useState4[1];
  
  // Simulate incoming logs
  useEffect(function() {
    if (pauseNewLogs) return;
    
    var idx = 0;
    var timer = setInterval(function() {
      if (idx < SAMPLE_MESSAGES.length) {
        var entry = {
          id: Date.now(),
          timestamp: '00:00:' + String(idx + 10).padStart(2, '0'),
          level: SAMPLE_MESSAGES[idx].level,
          msg: SAMPLE_MESSAGES[idx].msg
        };
        setLogs(function(l) {
          var next = l.concat([entry]);
          // Keep last 50 entries
          return next.slice(-50);
        });
        idx++;
      }
    }, 500);
    
    return function() { clearInterval(timer); };
  }, [pauseNewLogs]);
  
  useInput(function(input) {
    if (input === 'q' || input === 'Q') {
      useApp().exit();
    }
    if (input === 'j' || input === 'downArrow') {
      setAutoScroll(false);
    }
    if (input === 'G') {
      setAutoScroll(true);
    }
    if (input === 'f') {
      setFilterLevel(function(l) { return (l + 1) % 4; });
    }
    if (input === 'p') {
      setPauseNewLogs(function(p) { return !p; });
    }
  });
  
  var filteredLogs = logs.filter(function(log) {
    return log.level >= filterLevel;
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
              { type: Text, props: { color: 'green', bold: true, children: 'Log Viewer' }},
              { type: Text, props: { dimColor: true, children: ' | ' + logs.length + ' entries' }},
              { type: Text, props: { color: autoScroll ? 'green' : 'gray', children: ' | [j] scroll | [G] auto-scroll' }}
            ]
          }
        },
        // Filter bar
        {
          type: Box,
          props: {
            children: [
              { type: Text, props: { dimColor: true, children: 'Filter: ' }},
              { type: Text, props: { color: filterLevel === 0 ? 'green' : 'gray', children: '[ALL]' }},
              { type: Text, props: { dimColor: true, children: ' | ' }},
              { type: Text, props: { color: filterLevel === 1 ? 'green' : 'gray', children: '[INFO+]' }},
              { type: Text, props: { dimColor: true, children: ' | ' }},
              { type: Text, props: { color: filterLevel === 2 ? 'green' : 'gray', children: '[WARN+]' }},
              { type: Text, props: { dimColor: true, children: ' | ' }},
              { type: Text, props: { color: filterLevel === 3 ? 'green' : 'gray', children: '[ERROR]' }},
              { type: Text, props: { dimColor: true, children: ' | [f] cycle' }}
            ]
          }
        },
        // Status bar
        {
          type: Box,
          props: {
            children: [
              { type: Text, props: { color: pauseNewLogs ? 'yellow' : 'green', children: pauseNewLogs ? '[PAUSED]' : '[LIVE]' }},
              { type: Text, props: { dimColor: true, children: ' [p] pause/resume' }}
            ]
          }
        },
        // Log entries
        { type: Text, props: { children: '' }},
        {
          type: Box,
          props: {
            flexDirection: 'column',
            borderStyle: 'round',
            children: filteredLogs.slice(-15).map(function(log) {
              return {
                type: Text,
                props: {
                  children: log.timestamp + ' ' + getLevelName(log.level).padEnd(5) + ' ' + log.msg,
                  color: getLevelColor(log.level)
                }
              };
            })
          }
        },
        { type: Text, props: { dimColor: true, children: '[q] quit | [f] filter | [p] pause' }}
      ]
    }
  };
}

render({ type: App, props: {} });
