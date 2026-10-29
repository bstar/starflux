#!/usr/bin/env python3
"""Real GPU sidebar test using disposable projects and the imported project labels."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import uuid

repo=Path(__file__).resolve().parents[1];app=repo/'scripts/run-local.sh'
runtime=Path(os.environ['XDG_RUNTIME_DIR'])/'starflux'
data=Path.home()/'.local/share/starflux/projects'
out=repo/'target/sidebar-proof';out.mkdir(parents=True,exist_ok=True)
prefix='nav-'+uuid.uuid4().hex[:6]
names=[f'{prefix}-{i:02}' for i in range(20)]
sources=[]
for p in data.glob('herdr-default-*.json'):
    try:
        c=json.loads(p.read_text())
        if isinstance(c,dict) and c.get('collection')=='herdr-default' and c.get('herdr'):sources.append(c)
    except (ValueError,OSError):pass
sources.sort(key=lambda c:c['herdr']['order'])

def wait(fn,label):
    end=time.monotonic()+30
    while time.monotonic()<end:
        try:
            result=fn()
            if result:return result
        except (OSError,ValueError,KeyError,subprocess.SubprocessError):pass
        time.sleep(.1)
    raise AssertionError(label)

with tempfile.TemporaryDirectory(prefix='starflux-nav-') as temp:
    temp=Path(temp);address='unix:'+str(temp/'kitty.sock')
    for i,name in enumerate(names):
        config={'workspace':name,'collection':prefix,'label':sources[i]['label'] if len(sources)==20 else f'Project {i+1}','theme':'kanagawa','host':None,'directory':str(temp),'agent':f'python3 {repo}/scripts/direct-terminal-fixture.py {temp}/{i}.jsonl','executable':str(repo/'target/release/starflux'),'project_host':True,'desktop_helper':str(app)}
        p=runtime/(name+'.json');p.write_text(json.dumps(config));p.chmod(0o600)
    def invoke(name,*args):return subprocess.check_output([str(app),'--workspace',name,*args],stderr=subprocess.PIPE,timeout=20)
    def state():return json.loads((runtime/(names[0]+'.state.json')).read_text())
    def rc(*args,data=None):
        if args[0] in ['send-text','send-key']:args=(args[0],'--match','state:focused',*args[1:])
        return subprocess.check_output(['kitten','@','--to',address,*args],input=data,stderr=subprocess.PIPE,timeout=10)
    kitty=subprocess.Popen(['/usr/bin/kitty','--hold','--class','starflux-sidebar-proof','--listen-on',address,'-o','allow_remote_control=yes',str(app),'--workspace',names[0]],env=dict(os.environ,STAR_GRAPHICS_RENDERER='gpu'),stdout=subprocess.DEVNULL,stderr=(out/'kitty.log').open('w'))
    try:
        s=wait(lambda:state() if len(state().get('project_navigation',{}).get('visible',[]))==20 and state().get('desktop',{}).get('project_windows') else None,'20 project rows were not visible')
        assert s['project_navigation']['kind']=='sidebar'
        assert s['project_navigation']['sort']=='manual'
        def mouse(action,x,y):
            rc('send-text','--stdin',data=f'\x1b[<{action};{x};{y}M'.encode())
        def drag(row,gap):
            mouse(0,100,122+row*34+15)
            mouse(32,102,122+gap*34)
            wait(lambda:state()['project_navigation']['dragging'] is not None,'Drag preview missing')
            rc('send-text','--stdin',data=f'\x1b[<0;102;{122+gap*34}m'.encode())
        selected=s['active_project'];before_activity=s['project_navigation']['last_activity']
        drag(19,0)
        expected=[names[-1],*names[:-1]]
        wait(lambda:state()['project_order']==expected,'Last project did not move to first')
        assert state()['active_project']==selected,'Dragging activated a project'
        drag(0,20)
        wait(lambda:state()['project_order']==names,'First project did not move to last')
        # A cancelled drag must leave order and selection intact.
        mouse(0,100,137);mouse(32,100,200);rc('send-key','escape')
        wait(lambda:state()['project_navigation']['dragging'] is None,'Escape did not cancel drag')
        assert state()['project_order']==names and state()['active_project']==selected

        before=s['project_order'];assert before==names
        # Click the last visible project without paging or opening a menu.
        x=70;y=122+19*34+15
        rc('send-text','--stdin',data=f'\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m'.encode())
        s=wait(lambda:state() if state()['active_project']==names[19] else None,'Last sidebar row was not clickable')
        assert s['project_order']==names and len(s['project_navigation']['visible'])==20
        wait(lambda:any(w['app_id'].startswith('starflux-'+names[19]+'-') for w in state()['desktop']['project_windows']),'Selected project terminal missing')
        # Select another row while retaining all project labels and stable order.
        y=122+state()['project_order'].index(names[18])*34+15
        rc('send-text','--stdin',data=f'\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m'.encode())
        s=wait(lambda:state() if state()['active_project']==names[18] else None,'Project row return failed')
        assert s['project_order']==names and len(s['project_navigation']['visible'])==20
        c=next(c for c in json.loads(subprocess.check_output(['hyprctl','clients','-j'])) if c['pid']==kitty.pid)
        x,y=c['at'];w,h=c['size'];subprocess.run(['grim','-g',f'{x},{y} {w}x{h}',str(out/'sidebar.png')],check=True)
        rc('send-text','--stdin',data=f'\x1b[<0;50;{s["viewport"]["height"]-51}M\x1b[<0;50;{s["viewport"]["height"]-51}m'.encode())
        wait(lambda:state()['modal_open'],'Sidebar new-project button failed')
        rc('send-key','escape')
        wait(lambda:not state()['modal_open'],'Project dialog did not cancel')
        # Switch activity/manual modes and reopen: the manual arrangement is retained.
        drag(19,0);expected=[names[-1],*names[:-1]]
        wait(lambda:state()['project_order']==expected,'Second manual reorder failed')
        invoke(names[0],'--action','projects:sort:activity')
        wait(lambda:state()['project_navigation']['sort']=='recent','Activity mode missing')
        before_activity=state()['project_navigation']['last_activity']
        invoke(names[0],'--action','project:'+names[0])
        time.sleep(5)
        assert state()['project_navigation']['last_activity']==before_activity,'Selection changed activity timestamps'
        invoke(names[0],'--action','projects:sort:manual')
        wait(lambda:state()['project_order']==expected,'Manual order was lost after activity mode')
        old=state()['controller_pid'];invoke(names[0],'--close');time.sleep(.3)
        rc('launch','--type=tab',str(app),'--workspace',names[0])
        wait(lambda:state()['controller_pid']!=old and state()['project_navigation']['sort']=='manual' and state()['project_order']==expected,'Saved manual order did not survive restart')
        result={'new_project_dialog':True,'all_20_projects_visible':True,'last_row_clickable_without_paging':True,'drag_both_directions_without_activation':True,'escape_cancels_drag':True,'manual_order_survives_activity_and_restart':True,'selection_does_not_change_activity':True,'sidebar_width':s['project_navigation']['sidebar_width'],'original_herdr_sessions_touched':False}
        (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
    finally:
        for name in names:
            try:invoke(name,'--stop')
            except subprocess.SubprocessError:pass
        try:rc('close-window')
        except subprocess.SubprocessError:kitty.terminate()
        kitty.wait(timeout=10)
