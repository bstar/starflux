#!/usr/bin/env python3
"""Private real-Bash PTY job-control proof for the candidate supervisor."""
import argparse,fcntl,json,os,pty,re,select,signal,subprocess,tempfile,termios,time
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--executable',type=Path,required=True);args=p.parse_args()
with tempfile.TemporaryDirectory(prefix='sf-supervisor-shell-') as temporary:
 root=Path(temporary);runtime=root/'runtime';runtime.mkdir(mode=0o700)
 env={**os.environ,'XDG_RUNTIME_DIR':str(runtime),'XDG_DATA_HOME':str(root/'data'),'XDG_CONFIG_HOME':str(root/'config'),'HISTFILE':'/dev/null','PS1':'SUPERVISOR_PROMPT> '}
 env.pop('BASH_ENV',None);env.pop('ENV',None)
 wrapper=root/'run';wrapper.write_text('#!/bin/bash\nsource '+str(Path.cwd()/'target/run-env.sh')+'\nexec '+str(args.executable.resolve())+' "$@"\n');wrapper.chmod(0o700)
 rejected=subprocess.run([str(wrapper),'--native-source-supervisor','/bin/echo','MUST_NOT_RUN'],env=env,stdin=subprocess.DEVNULL,capture_output=True,timeout=5)
 assert rejected.returncode!=0 and b'MUST_NOT_RUN' not in rejected.stdout,'supervisor accepted an unowned terminal'
 master,slave=pty.openpty()
 def controlling_tty():
  os.setsid();fcntl.ioctl(0,termios.TIOCSCTTY,0)
 proc=subprocess.Popen([str(wrapper),'--native-source-supervisor','/bin/bash','--noprofile','--norc','-i'],env=env,stdin=slave,stdout=slave,stderr=slave,preexec_fn=controlling_tty)
 os.close(slave);data=b'';background=None
 def read_until(test,label):
  global data
  deadline=time.monotonic()+10
  while time.monotonic()<deadline:
   if select.select([master],[],[],.05)[0]:
    try:data+=os.read(master,8192)
    except OSError:break
   if test(data):return
   assert proc.poll() is None,(label,proc.returncode,data[-2000:])
  raise AssertionError((label,data[-2000:]))
 try:
  read_until(lambda text:b'SUPERVISOR_PROMPT> ' in text or (b'bash-' in text and b'$ ' in text),'interactive shell did not own its foreground terminal')
  os.write(master,b'printf "READY_%s\\n" "$BASHPID"\n')
  read_until(lambda text:re.search(rb'READY_(\d+)',text),'shell did not accept input')
  shell=int(re.search(rb'READY_(\d+)',data).group(1));assert shell!=proc.pid
  os.write(master,b'sleep 60\n');time.sleep(.2);os.write(master,b'\x03');time.sleep(.1)
  os.write(master,b'printf "AFTER_%s\\n" INT\n')
  read_until(lambda text:b'AFTER_INT\r\n' in text,'Ctrl+C did not return job control to Bash')
  os.write(master,b'sleep 60 & printf "BACKGROUND_%s\\n" "$!"\n')
  read_until(lambda text:re.search(rb'BACKGROUND_(\d+)',text),'background shell job did not start')
  background=int(re.search(rb'BACKGROUND_(\d+)',data).group(1));background_fd=os.pidfd_open(background)
  os.write(master,b'exit\n');assert proc.wait(timeout=5)==0
  try:signal.pidfd_send_signal(background_fd,0)
  except ProcessLookupError:pass
  else:raise AssertionError('background job survived natural shell exit')
  os.close(background_fd)
  print(json.dumps(dict(result='PASS',interactive_bash=True,foreground_job_control=True,ctrl_c_preserves_shell=True,natural_exit_reaps_background_job=True,non_pty_launch_refused=True)))
 finally:
  if proc.poll() is None:
   fd=os.pidfd_open(proc.pid);signal.pidfd_send_signal(fd,signal.SIGHUP);os.close(fd)
   try:proc.wait(timeout=5)
   except subprocess.TimeoutExpired:proc.kill();proc.wait(timeout=5)
  os.close(master)
