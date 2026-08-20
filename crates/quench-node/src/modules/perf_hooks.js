(function (deps) {
  'use strict';
  var origin = Date.now(), entries = [], observers = [];
  function now() { return Date.now() - origin; }
  function PerformanceEntry(name, type, start, duration) { this.name = name; this.entryType = type; this.startTime = start; this.duration = duration; }
  function add(name, type, start, duration) { var e = new PerformanceEntry(name, type, start, duration), i, o; entries.push(e); for (i=0;i<observers.length;i++) { o=observers[i]; if (o.types && o.types.indexOf(type)>=0) o.callback({getEntries:function(){return [e];},getEntriesByName:function(n){return n===e.name?[e]:[];},getEntriesByType:function(t){return t===e.entryType?[e]:[];}},o); } return e; }
  var performance = { now:now, timeOrigin:origin,
    mark:function(n){return add(String(n),'mark',now(),0);},
    measure:function(n,s,en){var a=0,b=now(),i;if(s!==undefined){for(i=entries.length-1;i>=0;i--)if(entries[i].name===String(s)&&entries[i].entryType==='mark'){a=entries[i].startTime;break;}}if(en!==undefined){for(i=entries.length-1;i>=0;i--)if(entries[i].name===String(en)&&entries[i].entryType==='mark'){b=entries[i].startTime;break;}}return add(String(n),'measure',a,b-a);},
    getEntries:function(){return entries.slice();}, getEntriesByName:function(n,t){return entries.filter(function(e){return e.name===String(n)&&(t===undefined||e.entryType===t);});}, getEntriesByType:function(t){return entries.filter(function(e){return e.entryType===t;});},
    clearMarks:function(n){entries=entries.filter(function(e){return e.entryType!=='mark'||(n!==undefined&&e.name!==String(n));});}, clearMeasures:function(n){entries=entries.filter(function(e){return e.entryType!=='measure'||(n!==undefined&&e.name!==String(n));});} };
  function PerformanceObserver(cb){this.callback=cb;this.types=null;}
  PerformanceObserver.prototype.observe=function(o){this.types=o&&o.entryTypes?o.entryTypes.slice():[];if(observers.indexOf(this)<0)observers.push(this);};
  PerformanceObserver.prototype.disconnect=function(){var i=observers.indexOf(this);if(i>=0)observers.splice(i,1);this.types=null;};
  PerformanceObserver.prototype.takeRecords=function(){return [];};
  function empty(){return {};}; function timerify(fn){return function(){return fn.apply(this,arguments);};}; function elu(){return {idle:0,active:0,utilization:0};}
  return {performance:performance,PerformanceObserver:PerformanceObserver,PerformanceEntry:PerformanceEntry,PerformanceMark:PerformanceEntry,PerformanceMeasure:PerformanceEntry,monitorEventLoopDelay:empty,createHistogram:empty,constants:{},timerify:timerify,eventLoopUtilization:elu};
});
