#!/usr/bin/env python3
"""Compare actual Kitty font queries with STAR/MUX typography negotiation."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import uuid

repo=Path(__file__).resolve().parents[1]
app=repo/'scripts/run-local.sh'
runtime=Path(os.environ['XDG_RUNTIME_DIR'])/'starmux'
out=repo/'target/font-inheritance-proof';out.mkdir(parents=True,exist_ok=True)

def wait(fn,label,timeout=30):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError,KeyError,subprocess.SubprocessError):pass
        time.sleep(.1)
    raise AssertionError(label)

with tempfile.TemporaryDirectory(prefix='starmux-font-') as temp:
    temp=Path(temp);results=[]
    launcher=temp/'launch.py'
    launcher.write_text('''import fcntl,json,os,struct,subprocess,sys,termios,time
from pathlib import Path
folder=Path(sys.argv[1]);name=sys.argv[2];app=sys.argv[3]
query=subprocess.check_output(['kitten','query_terminal','--wait-for','2','font_family','font_size','dpi_y'],text=True)
info=dict(line.split(': ',1) for line in query.splitlines() if ': ' in line)
rows,cols,width,height=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,b'\\0'*8))
info['cell']=[width//cols,height//rows]
print('\\x1b[2J\\x1b[HNative Kitty font sample',flush=True)
print('0123456789  i W M  -> => != ===  café 世界 é',flush=True)
(folder/'query.json').write_text(json.dumps(info))
deadline=time.monotonic()+20
while not (folder/'continue').exists():
 if time.monotonic()>deadline:raise SystemExit('Parent did not capture native sample')
 time.sleep(.05)
os.execv(app,[app,'--workspace',name])
''')
    for label,options in [('user-default',[]),('liberation-15pt',['-o','font_family=Liberation Mono','-o','font_size=15'])]:
        name='font-'+uuid.uuid4().hex[:8];folder=temp/label;folder.mkdir();address='unix:'+str(folder/'kitty.sock')
        config={'workspace':name,'collection':name,'theme':'kanagawa','host':None,'directory':str(temp),'agent':'python3 '+str(repo/'scripts/direct-terminal-fixture.py')+' '+str(folder/'menu.jsonl'),'executable':str(repo/'target/release/starmux'),'project_host':True,'desktop_helper':str(app)}
        (runtime/(name+'.json')).write_text(json.dumps(config));os.chmod(runtime/(name+'.json'),0o600)
        env=dict(os.environ,STAR_GRAPHICS_RENDERER='gpu',STAR_TERMINAL_RENDERER='direct',STAR_GRAPHICS_FONT_DEBUG='1');env.pop('STAR_GRAPHICS_FONT',None);env.pop('STAR_GRAPHICS_FONT_SIZE',None)
        kitty=subprocess.Popen(['/usr/bin/kitty','--hold','--class','starmux-font-proof','--listen-on',address,'-o','allow_remote_control=yes',*options,'python3',str(launcher),str(folder),name,str(app)],env=env,stdout=subprocess.DEVNULL,stderr=(out/(label+'.log')).open('w'))
        def state():return json.loads((runtime/(name+'.state.json')).read_text())
        def rc(*args,data=None):return subprocess.check_output(['kitten','@','--to',address,*args],input=data,stderr=subprocess.PIPE,timeout=10)
        def screenshot(suffix):
            c=next(c for c in json.loads(subprocess.check_output(['hyprctl','clients','-j'])) if c['pid']==kitty.pid)
            x,y=c['at'];w,h=c['size'];subprocess.run(['grim','-g',f'{x},{y} {w}x{h}',str(out/(label+'-'+suffix+'.png'))],check=True)
        try:
            query=wait(lambda:json.loads((folder/'query.json').read_text()),'Kitty font query failed')
            (out/(label+'-query.json')).write_text(json.dumps(query,indent=2))
            screenshot('kitty');(folder/'continue').touch()
            s=wait(lambda:state() if state().get('terminal_typography') and any('Direct terminal menu' in t['text'] for t in state().get('direct_terminals',[])) else None,'Typography was not negotiated')
            if label=='liberation-15pt':assert query['font_family'].startswith('LiberationMono'),query
            typography=s['terminal_typography'];pixels=float(query['font_size'])*float(query['dpi_y'])/72
            assert abs(typography['millipixels']/1000-pixels)<.002,(query,typography)
            assert typography['cell']==query['cell'],(query,typography)
            assert s['project_state']['terminal_font_size']==0,'Fresh project should inherit'
            screenshot('starmux')
            # The reset control must restore the queried fractional size/cell,
            # rather than the old hard-coded 16px default.
            subprocess.check_output([str(app),'--workspace',name,'--action','terminal:font:increase'])
            wait(lambda:state()['project_state']['terminal_font_size']>0,'Zoom override missing')
            subprocess.check_output([str(app),'--workspace',name,'--action','terminal:font:reset'])
            wait(lambda:state()['project_state']['terminal_font_size']==0,'Reset did not restore inheritance')
            results.append({'case':label,'kitty':query,'typography':typography,'inherited_pixels':pixels,'reset_restores_inheritance':True})
        finally:
            try:(out/(label+'-terminal.txt')).write_bytes(rc('get-text','--extent','all'))
            except subprocess.SubprocessError:pass
            subprocess.run([str(app),'--workspace',name,'--stop'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=20)
            try:rc('close-window')
            except subprocess.SubprocessError:kitty.terminate()
            kitty.wait(timeout=10)
    (out/'result.json').write_text(json.dumps(results,indent=2)+'\n')
    print(json.dumps([{'case':r['case'],'font':r['kitty']['font_family'],'pixels':r['inherited_pixels'],'cell':r['typography']['cell']} for r in results]))
