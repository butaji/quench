#!/usr/bin/env python3
"""Detach an owned check from the tool process group; retain actual state."""
import datetime,json,os,subprocess,sys,time
from pathlib import Path
config=json.loads(Path(sys.argv[1]).read_text())
if os.fork():sys.exit(0)
os.setsid()
if os.fork():os._exit(0)
os.chdir(config['cwd'])
log=open(config['log'],'ab',buffering=0);null=open(os.devnull,'rb')
os.dup2(null.fileno(),0);os.dup2(log.fileno(),1);os.dup2(log.fileno(),2)
state=Path(config['state'])
def write(**values):
 values.update(supervisor_pid=os.getpid(),session_id=os.getsid(0),utc=datetime.datetime.now(datetime.timezone.utc).isoformat())
 temp=state.with_suffix('.tmp');temp.write_text(json.dumps(values,indent=2)+'\n');temp.replace(state)
for index,command in enumerate(config['commands']):
 child=subprocess.Popen(command,stdin=null,stdout=log,stderr=log)
 while child.poll() is None:
  write(status='running',index=index,command=command,child_pid=child.pid);time.sleep(5)
 code=child.returncode
 if code:
  write(status='failed',index=index,command=command,exit=code);os._exit(1)
write(status='complete',commands=config['commands'],exit=0)
os._exit(0)
