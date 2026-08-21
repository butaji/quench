var channels = {};
function channel(name) {
  if (channels[name]) return channels[name];
  var subs = [];
  var c = { name:name, hasSubscribers:false,
    subscribe:function(fn){ if(typeof fn==='function' && subs.indexOf(fn)<0) subs.push(fn); c.hasSubscribers=subs.length>0; return c; },
    unsubscribe:function(fn){ var i=subs.indexOf(fn); if(i>=0)subs.splice(i,1); c.hasSubscribers=subs.length>0; return c; },
    publish:function(msg){ var copy=subs.slice(); for(var i=0;i<copy.length;i++)copy[i](msg); }
  }; channels[name]=c; return c;
}
module.exports={channel:channel, subscribe:function(n,f){return channel(n).subscribe(f);}, unsubscribe:function(n,f){return channel(n).unsubscribe(f);}, hasSubscribers:function(n){return channel(n).hasSubscribers;}, channelNames:function(){return Object.keys(channels);}};
