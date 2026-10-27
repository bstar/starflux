#!/usr/bin/env python3
"""Exercise read-only native activity detection against a disposable Codex-like TUI."""
import base64,json,subprocess,tempfile,time,uuid,shlex
from pathlib import Path
repo=Path(__file__).resolve().parents[1]
helper=(repo/'scripts/host.py').read_text()
name='activity-'+uuid.uuid4().hex[:8]
def call(action,**fields):
    request={'workspace':name,'pane':'p1','action':action,**fields}
    return json.loads(subprocess.check_output(['python3','-c',helper,base64.b64encode(json.dumps(request).encode()).decode()],stderr=subprocess.PIPE,timeout=10))
def wait(state):
    until=time.monotonic()+8
    while time.monotonic()<until:
        current=call('inspect')
        if current.get('state')==state:return current
        time.sleep(.05)
    raise AssertionError((state,current))
with tempfile.TemporaryDirectory(prefix='starmux-activity-') as temp:
    temp=Path(temp)
    program=temp/'fixture.py'
    program.write_text('''import ctypes,os,tty,sys
ctypes.CDLL(None).prctl(15,ctypes.c_char_p(b'codex'),0,0,0)
tty.setraw(0)
def draw(state):
 height=os.get_terminal_size().lines
 status={'w':'Working (1s · esc to interrupt)','i':'› Type a request','q':'Question 1/1\\n1. Option A\\n2. Option B\\nEnter to submit','h':'Press Enter to continue; Esc to cancel','a':'Would you like to run the following command?\\n1. Yes, proceed (y)\\n2. No, and tell Codex (esc)'}[state]
 os.write(1,('\\x1b[2J\\x1b[%d;1H%s'%(max(1,height-2),status)).encode())
draw('w')
while True:
 key=os.read(0,1).decode()
 if key in 'wiaqh':draw(key)
''')
    try:
        call('ensure',directory=str(temp),argv=['bash','-c','exec -a codex python3 '+shlex.quote(str(program))])
        working=wait('working');pid=working['pid']
        for key,state in [('i','idle'),('h','idle'),('a','waiting_for_approval'),('q','needs_input'),('w','working'),('i','idle')]:
            subprocess.run(['tmux','-L','starmux-'+name,'send-keys','-t','p1:0.0',key],check=True)
            assert wait(state)['pid']==pid
        allowed={'alive','exited','pid','command','agent','agent_pid','state','output_fingerprint'}
        assert set(working)==allowed, 'Inspector exported unexpected fields: '+repr(set(working)-allowed)
        print(json.dumps({'passed':True,'working_idle_waiting_detected':True,'same_live_process':True,'conversation_text_not_exported':True}))
    finally:call('stop')
