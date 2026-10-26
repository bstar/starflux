#!/usr/bin/env python3
"""Real GPU activity wave using a disposable read-only Herdr API fixture."""
import json,os,subprocess,tempfile,time,uuid
from pathlib import Path
repo=Path(__file__).resolve().parents[1];app=repo/'scripts/run-local.sh'
runtime=Path(os.environ['XDG_RUNTIME_DIR'])/'starmux';data=Path.home()/'.local/share/starmux/projects'
out=repo/'target/activity-wave-proof';out.mkdir(parents=True,exist_ok=True)
prefix='wave-'+uuid.uuid4().hex[:6];names=[prefix+'-'+str(i) for i in range(3)]
original=json.loads(subprocess.check_output(['hyprctl','activewindow','-j'])).get('address')
def wait(fn,label):
    end=time.monotonic()+30
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (ValueError,OSError,KeyError,subprocess.SubprocessError):pass
        time.sleep(.08)
    raise AssertionError(label)
with tempfile.TemporaryDirectory(prefix='starmux-wave-') as temp:
    temp=Path(temp);address='unix:'+str(temp/'kitty.sock');statuses=temp/'statuses.json'
    statuses.write_text(json.dumps({'w0':'working','w1':'idle','w2':'waiting_for_input'}))
    source=temp/'herdr-fixture'
    source.write_text('''#!/usr/bin/python3
import json,sys,os
from pathlib import Path
root=Path(__file__).parent
args=sys.argv[3:]
with (root/'requests').open('a') as f:f.write(json.dumps(args[:2])+'\\n')
states=json.loads((root/'statuses.json').read_text())
if args[:2]==['agent','list']:
 print(json.dumps({'result':{'agents':[dict(workspace_id=k,agent_status=v) for k,v in states.items()]}}))
elif args[:2]==['pane','get']:
 print(json.dumps({'result':{'pane':{'agent':'codex','agent_status':states[args[2].split(':')[0]]}}}))
elif args[:2]==['terminal','attach']:
 os.execvp('python3',['python3',FIXTURE,str(root/'input.jsonl')])
else:raise SystemExit(2)
'''.replace('FIXTURE',repr(str(repo/'scripts/direct-terminal-fixture.py'))));source.chmod(0o700)
    for i,name in enumerate(names):
        pane={'terminal_id':'term'+str(i),'pane_id':f'w{i}:p1','tab_id':f'w{i}:t1','label':'Codex','cwd':str(temp),'agent':'codex'}
        config={'workspace':name,'label':['Building renderer','Idle project','Needs approval'][i],'collection':prefix,'theme':'kanagawa','host':None,'directory':str(temp),'agent':'codex','executable':str(repo/'target/release/starmux'),'project_host':True,'desktop_helper':str(app),'herdr':{'session':'wave-test','workspace_id':'w'+str(i),'order':i,'executable':str(source),'panes':{'0':pane,'31':pane}}}
        path=runtime/(name+'.json');path.write_text(json.dumps(config));path.chmod(0o600)
        data.mkdir(parents=True,exist_ok=True)
        path=data/(name+'.layout.json');path.write_text(json.dumps({'space':'agent','agent':31,'terminal':0,'terminals':[0]}));path.chmod(0o600)
    def rc(*args,data=None):return subprocess.check_output(['kitten','@','--to',address,*args],input=data,stderr=subprocess.PIPE,timeout=12)
    def invoke(*args):return subprocess.check_output([str(app),'--workspace',names[0],*args],stderr=subprocess.PIPE,timeout=20)
    def state():return json.loads((runtime/(names[0]+'.state.json')).read_text())
    kitty=subprocess.Popen(['/usr/bin/kitty','--hold','--class','starmux-wave-proof','--listen-on',address,'-o','allow_remote_control=yes',str(app),'--workspace',names[0]],env=dict(os.environ,STAR_GRAPHICS_RENDERER='gpu'),stdout=subprocess.DEVNULL,stderr=(out/'kitty.log').open('w'))
    def capture(label):
        c=next(c for c in json.loads(subprocess.check_output(['hyprctl','clients','-j'])) if c['pid']==kitty.pid)
        subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.focus({{window="address:{c["address"]}"}}))'],stdout=subprocess.DEVNULL,check=True)
        time.sleep(.25)
        c=next(c for c in json.loads(subprocess.check_output(['hyprctl','clients','-j'])) if c['pid']==kitty.pid)
        x,y=c['at'];path=out/(label+'.png')
        subprocess.run(['grim','-s','1','-g',f'{x},{y} 260x240',str(path)],check=True)
        return path
    def crop(path,row):
        return subprocess.check_output(['magick',str(path),'-crop',f'195x25+41+{100+row*34}','+repage','-depth','8','rgb:-'])
    try:
        s=wait(lambda:state() if state().get('project_activity',{}).get(names[0],{}).get('working')==1 and state()['project_activity'][names[2]]['waiting']==1 and state().get('desktop',{}).get('project_windows') else None,'Activity summary missing')
        wait(lambda:8<=state()['wave_phase']<=26,'Wave phase did not advance')
        samples=[capture('wave-0'),capture('wave-1'),capture('wave-2')]
        assert len({crop(p,0) for p in samples})>1,'Working text did not animate'
        icon_x=s['project_navigation']['sidebar_width']-29
        icons=[subprocess.check_output(['magick',str(p),'-crop',f'20x22+{icon_x}+100','+repage','-depth','8','rgb:-']) for p in samples]
        assert len(set(icons))>1,'Working project icon did not animate'
        assert len({crop(p,1) for p in samples})==1,'Idle project animated'
        assert len({crop(p,2) for p in samples})==1,'Waiting project animated'
        statuses.write_text(json.dumps({'w0':'done','w1':'working','w2':'waiting_for_input'}))
        wait(lambda:state()['project_activity'][names[0]]['working']==0 and state()['project_activity'][names[1]]['working']==1,'Wave did not follow activity changes')
        statuses.write_text(json.dumps({'w0':'done','w1':'idle','w2':'waiting_for_input'}))
        wait(lambda:all(a['working']==0 for a in state()['project_activity'].values()),'Activity did not settle')
        time.sleep(.6);phase=state()['wave_phase'];time.sleep(.7)
        assert state()['wave_phase']==phase,'Animation timer ran while idle'
        # Reduced motion preserves the working indicator without a moving wave.
        old=state()['controller_pid'];invoke('--close');time.sleep(.4)
        statuses.write_text(json.dumps({'w0':'working','w1':'idle','w2':'waiting_for_input'}))
        rc('launch','--type=tab','--env','STARMUX_REDUCED_MOTION=1',str(app),'--workspace',names[0])
        wait(lambda:state()['controller_pid']!=old and state().get('reduced_motion') and state()['project_activity'][names[0]]['working']==1,'Reduced-motion workspace missing')
        phase=state()['wave_phase'];time.sleep(.7);assert state()['wave_phase']==phase
        capture('reduced-motion')
        requests=[json.loads(l) for l in (temp/'requests').read_text().splitlines()]
        assert all(r in [['agent','list'],['pane','get'],['terminal','attach']] for r in requests)
        result={'passed':True,'working_text_wave_changes_pixels':True,'working_orbit_icon_changes_pixels':True,'idle_and_waiting_rows_static':True,'activity_changes_start_and_stop_wave':True,'timer_stops_when_idle':True,'reduced_motion':True,'batched_read_only_activity':True,'original_herdr_sessions_touched':False}
        (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
    except BaseException:
        try:(out/'failure-state.json').write_text(json.dumps(state(),indent=2))
        except Exception:pass
        raise
    finally:
        try:invoke('--close')
        except subprocess.SubprocessError:pass
        try:rc('close-window')
        except subprocess.SubprocessError:pass
        if kitty.poll() is None:kitty.terminate();kitty.wait(timeout=10)
        for name in names:
            for root in [runtime,data]:
                for suffix in ['.json','.layout.json','.state.json','.workspace.json']:(root/(name+suffix)).unlink(missing_ok=True)
        if original:subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.focus({{window="address:{original}"}}))'],stdout=subprocess.DEVNULL)
