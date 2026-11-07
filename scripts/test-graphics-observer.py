#!/usr/bin/env python3
"""Private tmux control-mode experiment; never attaches a renderer or real pane."""
import argparse,base64,getpass,hashlib,json,os,queue,secrets,shlex,shutil,socket,subprocess,tempfile,threading,time
from pathlib import Path
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--ssh',action='store_true')
parser.add_argument('--decoder-executable',type=Path)
parser.add_argument('--composition',action='store_true')
parser.add_argument('--flow-control',action='store_true')
parser.add_argument('--owner-executable',type=Path)
parser.add_argument('--authority-executable',type=Path)
parser.add_argument('--mutations',action='store_true')
parser.add_argument('--stalled-upload',action='store_true')
args=parser.parse_args()
if args.mutations and not args.authority_executable:parser.error('--mutations requires --authority-executable')
if args.composition and not args.decoder_executable:parser.error('--composition requires --decoder-executable')
if args.stalled_upload and not args.owner_executable:parser.error('--stalled-upload requires --owner-executable')
with tempfile.TemporaryDirectory(prefix='starflux-graphics-observer-') as folder:
    tmp=Path(folder);(tmp/'tmux').mkdir(mode=0o700)
    env={**os.environ,'TMUX_TMPDIR':str(tmp/'tmux'),'SHELL':'/bin/sh'};env.pop('TMUX',None)
    label='sf-graphics-'+secrets.token_hex(5);command=['tmux','-f','/dev/null','-L',label]
    script=tmp/'source.py';payload=b'\x1b_Ga=T,f=24,s=1,v=1,i=11,C=1,q=2'+(b',c=2,r=2' if args.composition else b'')+b';'+base64.b64encode(b'\x11\x22\x33')+b'\x1b\\'
    wrapped=b'\x1bPtmux;'+payload.replace(b'\x1b',b'\x1b\x1b')+b'\x1b\\'
    script.write_text("import os,sys\nprint('SOURCE_READY',flush=True)\nfor line in sys.stdin:\n if line.strip()=='SHOW':os.write(1,"+repr(wrapped)+"+b'\\r\\nAFTER_FRAME\\r\\n')\n else:print('REPLY='+line.strip(),flush=True)\n")
    observer=None;daemon=None;ssh=[]
    def wait(predicate,label,timeout=10):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            value=predicate()
            if value:return value
            time.sleep(.02)
        raise AssertionError(label)
    try:
        if args.ssh:
            for key in ['host','client']:
                subprocess.run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(tmp/key)],check=True)
            (tmp/'authorized_keys').write_text((tmp/'client.pub').read_text())
            with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
            (tmp/'known_hosts').write_text(f'[127.0.0.1]:{port} '+(tmp/'host.pub').read_text())
            (tmp/'sshd_config').write_text(f'Port {port}\nListenAddress 127.0.0.1\nHostKey {tmp}/host\nPidFile {tmp}/sshd.pid\nAuthorizedKeysFile {tmp}/authorized_keys\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowUsers {getpass.getuser()}\nPerSourcePenalties no\nLogLevel ERROR\n')
            daemon=subprocess.Popen([shutil.which('sshd') or '/usr/sbin/sshd','-D','-e','-f',str(tmp/'sshd_config')],stderr=(tmp/'sshd.log').open('w'))
            def listening():
                try:
                    with socket.create_connection(('127.0.0.1',port),timeout=.1):return True
                except OSError:return False
            wait(listening,'private SSH server did not start')
            ssh=['ssh','-T','-p',str(port),'-i',str(tmp/'client'),'-o','IdentitiesOnly=yes','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','UserKnownHostsFile='+str(tmp/'known_hosts'),getpass.getuser()+'@127.0.0.1']
            remote=['env','-u','TMUX','TMUX_TMPDIR='+str(tmp/'tmux'),'SHELL=/bin/sh','tmux','-f','/dev/null','-L',label]
            command=remote
        authority_verified=False
        if args.authority_executable:
            authority_command=[str(args.authority_executable.resolve())]+(['--mutations'] if args.mutations else [])
            if args.ssh:authority_command=[*ssh,shlex.join(authority_command)]
            authority_result=subprocess.run(authority_command,env=env,capture_output=True,timeout=30)
            assert authority_result.returncode==0,authority_result.stderr.decode()
            authority_report=json.loads(authority_result.stdout)
            assert authority_report['result']=='PASS' and authority_report['source_owner_engine'] and authority_report['direct_cases']==6 and authority_report['wrapped_cases']==6 and authority_report['commit_barrier'] and authority_report['cursor_and_text_order'] and authority_report['ordered_graphics_da_dsr'] and authority_report['input_while_decode_pending'] and authority_report['retained_images']==(0 if args.mutations else 3) and authority_report['retained_bytes']==(0 if args.mutations else 12) and authority_report['released_reservations'] and authority_report['mutation_checks']==args.mutations,authority_report
            authority_verified=True
        owner_verified=False
        if args.owner_executable:
            owner_command=[str(args.owner_executable.resolve())]+(['--stalled-upload'] if args.stalled_upload else [])
            if args.ssh:owner_command=[*ssh,shlex.join(owner_command)]
            owner_result=subprocess.run(owner_command,env=env,capture_output=True,timeout=150 if args.stalled_upload else 30)
            assert owner_result.returncode==0,owner_result.stderr.decode()
            owner_report=json.loads(owner_result.stdout)
            assert owner_report['result']=='PASS' and owner_report['source_side_ordering'] and owner_report['direct_cases']==(9 if args.stalled_upload else 8) and owner_report['wrapped_cases']==(9 if args.stalled_upload else 8) and owner_report['early_chunk_barrier'] and owner_report['malformed_and_cancelled'] and owner_report['async_decode'] and owner_report['reservations_released'] and owner_report['real_stalled_upload_expiry']==args.stalled_upload,owner_report
            owner_verified=True
        def launch(*arguments,**kwargs):
            argv=[*command,*arguments]
            if args.ssh:argv=[*ssh,shlex.join(argv)]
            return subprocess.Popen(argv,env=env,**kwargs)
        def tm(*arguments):
            child=launch(*arguments,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
            stdout,stderr=child.communicate(timeout=15)
            assert child.returncode==0,(arguments,stderr.decode())
            return stdout.decode().strip()
        tm('new-session','-d','-s','proof','-x','91','-y','27','exec /usr/bin/python3 -u '+shlex.quote(str(script)))
        tm('set-option','-g','allow-passthrough','on');tm('set-option','-w','-t','proof','window-size','latest')
        owner=int(tm('display-message','-p','-t','proof','#{pane_pid}'))
        pane=tm('display-message','-p','-t','proof','#{pane_id}')
        def stamp():return Path('/proc',str(owner),'stat').read_text().rsplit(')',1)[1].split()[19]
        original_stamp=stamp();original_size=tm('display-message','-p','-t','proof','#{pane_width}x#{pane_height}')
        wait(lambda:'SOURCE_READY' in tm('capture-pane','-p','-t','proof'),'source did not start')
        observer=launch('-C','attach-session','-f','ignore-size,pause-after=1' if args.flow_control else 'ignore-size','-t','proof',stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        messages=queue.Queue()
        def receive():
            for line in observer.stdout:messages.put(line)
        threading.Thread(target=receive,daemon=True).start()
        lines=[]
        def saw(marker):
            while not messages.empty():lines.append(messages.get_nowait())
            return any(marker in line for line in lines)
        wait(lambda:saw(b'%session-changed') or observer.poll() is not None,'control client did not attach')
        assert observer.poll() is None,observer.stderr.read().decode()
        tm('send-keys','-t','proof','-l','SHOW');tm('send-keys','-t','proof','Enter')
        wait(lambda:saw(b'AFTER_FRAME'),'control client did not observe source output')
        outputs=[line for line in lines if line.startswith((b'%output ',b'%extended-output '))]
        def unescape(value):
            result=bytearray();i=0
            while i<len(value):
                if value[i]==92 and i+3<len(value) and all(48<=v<=55 for v in value[i+1:i+4]):result.append(int(value[i+1:i+4],8));i+=4
                else:result.append(value[i]);i+=1
            return bytes(result)
        raw=b''.join(unescape(line.rstrip(b'\n').split(b' : ',1)[1] if line.startswith(b'%extended-output ') else line.rstrip(b'\n').split(b' ',2)[2]) for line in outputs)
        assert wrapped in raw or payload in raw,'graphics bytes were dropped by control mode'
        decoded=False
        if args.decoder_executable:
            decoder_command=[str(args.decoder_executable.resolve()),pane.removeprefix('%')]
            if args.ssh and args.composition:decoder_command=[*ssh,shlex.join(decoder_command)]
            result=subprocess.run(decoder_command,input=b''.join(lines),capture_output=True,timeout=15)
            assert result.returncode==0,result.stderr.decode()
            report=json.loads(result.stdout)
            assert report['images']==1 and report['placements']==1 and not report['errors'] and not report['gaps'],report
            assert report['rgba_sha256']==hashlib.sha256(bytes([0x11,0x22,0x33,255])).hexdigest(),report
            assert 'AFTER_FRAME' in report['text'] and 'a=T' not in report['text'],report
            if args.composition:
                assert all(report.get(key) for key in ('composition_verified','independent_cell_fit','cells_frame_valid','source_history_unchanged','composition_reservations_released')),report
            decoded=True
        if args.flow_control:
            observer.stdin.write(("refresh-client -A '"+pane+":pause'\n").encode());observer.stdin.flush()
            
            try:wait(lambda:saw(('%pause '+pane).encode()),'control pause was not acknowledged')
            except AssertionError:
                print('CONTROL_DIAGNOSTIC',repr(lines[-12:]),flush=True);raise
            tm('send-keys','-t','proof','-l','WHILE_PAUSED');tm('send-keys','-t','proof','Enter')
            wait(lambda:'REPLY=WHILE_PAUSED' in tm('capture-pane','-p','-t','proof'),'observer pause blocked the source process')
            observer.stdin.write(("refresh-client -A '"+pane+":continue'\n").encode());observer.stdin.flush()
            wait(lambda:saw(('%continue '+pane).encode()),'control continue was not acknowledged')
            tm('send-keys','-t','proof','-l','AFTER_CONTINUE');tm('send-keys','-t','proof','Enter')
            wait(lambda:saw(b'REPLY=AFTER_CONTINUE'),'continued observer did not receive new output')
        assert tm('display-message','-p','-t','proof','#{pane_width}x#{pane_height}')==original_size,'observer resized source'
        observer.stdin.close();observer.wait(timeout=5)
        assert int(tm('display-message','-p','-t','proof','#{pane_pid}'))==owner and stamp()==original_stamp,'observer detach replaced source'
        tm('send-keys','-t','proof','-l','STILL_ALIVE');tm('send-keys','-t','proof','Enter')
        wait(lambda:'REPLY=STILL_ALIVE' in tm('capture-pane','-p','-t','proof'),'source lost input after observer detached')
        print(json.dumps(dict(result='PASS',ssh=args.ssh,source_owner_engine_verified=authority_verified,composition_verified=args.composition,cells_frame_valid=args.composition,native_pixels_verified=decoded,observer_flow_control=args.flow_control,private_owner_reply_ordering_verified=owner_verified,stalled_upload_expiry=args.stalled_upload,control_mode_receives_graphics=True,raw_tmux_wrapper_retained=wrapped in raw,unwrapped_apc_present=payload in raw,no_renderer=True,observer_sends_no_pty_input=True,source_pid_preserved=True,geometry_preserved=original_size,detach_preserves_input=True)))
    finally:
        if observer and observer.poll() is None:observer.terminate();observer.wait(timeout=5)
        argv=[*command,'kill-server']
        if args.ssh and ssh:argv=[*ssh,shlex.join(argv)]
        subprocess.run(argv,env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=15)
        if daemon:daemon.terminate();daemon.wait(timeout=5)
