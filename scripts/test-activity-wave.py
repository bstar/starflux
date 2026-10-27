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
    temp=Path(temp);address='unix:'+str(temp/'kitty.sock');statuses=temp/'statuses.json';revisions=temp/'revisions.json'
    statuses.write_text(json.dumps({'w0':'working','w1':'idle','w2':'blocked'}))
    revisions.write_text(json.dumps({'w0':0,'w1':0,'w2':0}))
    source=temp/'herdr-fixture'
    source.write_text('''#!/usr/bin/python3
import json,sys,os
from pathlib import Path
root=Path(__file__).parent
args=sys.argv[3:]
with (root/'requests').open('a') as f:f.write(json.dumps(args[:2])+'\\n')
states=json.loads((root/'statuses.json').read_text())
if args[:2]==['api','snapshot']:
 revisions=json.loads((root/'revisions.json').read_text())
 print(json.dumps({'result':{'snapshot':{'panes':[dict(workspace_id=k,agent='codex',agent_status=v,terminal_id=k,pane_id=k+':p1',revision=revisions[k]) for k,v in states.items()]}}}))
elif args[:2]==['agent','list']:
 revisions=json.loads((root/'revisions.json').read_text())
 print(json.dumps({'result':{'agents':[dict(workspace_id=k,agent='codex',agent_status=v,state_change_seq=(i+1)*100+revisions[k]) for i,(k,v) in enumerate(states.items())]}}))
elif args[:2]==['pane','read']:
 key=args[2].split(':')[0];state=states[key]
 revision=json.loads((root/'revisions.json').read_text())[key]
 print('Revision '+str(revision))
 print({'working':'Working (1s · esc to interrupt)','needs_permission':'1. Yes, proceed\\n2. No, and tell Codex\\nEsc to cancel','needs_input':'Question 1/1\\n1. Option A\\n2. Option B\\nEnter to submit'}.get(state,'› Type a request'))
elif args[:2]==['pane','get']:
 print(json.dumps({'result':{'pane':{'agent':'codex','agent_status':states[args[2].split(':')[0]]}}}))
elif args[:2]==['terminal','attach']:
 os.execvp('python3',['python3',FIXTURE,str(root/'input.jsonl')])
else:raise SystemExit(2)
'''.replace('FIXTURE',repr(str(repo/'scripts/direct-terminal-fixture.py'))));source.chmod(0o700)
    for i,name in enumerate(names):
        pane={'terminal_id':'term'+str(i),'pane_id':f'w{i}:p1','tab_id':f'w{i}:t1','label':'Codex','cwd':str(temp),'agent':'codex'}
        config={'workspace':name,'label':['Building renderer','Idle project','DEC'][i],'collection':prefix,'theme':'kanagawa','host':None,'directory':str(temp),'agent':'codex','executable':str(repo/'target/release/starmux'),'project_host':True,'desktop_helper':str(app),'herdr':{'session':'wave-test','workspace_id':'w'+str(i),'order':i,'executable':str(source),'panes':{'0':pane,'31':pane}}}
        path=runtime/(name+'.json');path.write_text(json.dumps(config));path.chmod(0o600)
        data.mkdir(parents=True,exist_ok=True)
        path=data/(name+'.layout.json');path.write_text(json.dumps({'space':'agent','agent':31,'terminal':0,'terminals':[0]}));path.chmod(0o600)
    def rc(*args,data=None):
        if args[0] in ['send-text','send-key']:
            args=(args[0],'--match','state:focused',*args[1:])
        return subprocess.check_output(['kitten','@','--to',address,*args],input=data,stderr=subprocess.PIPE,timeout=12)
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
        return subprocess.check_output(['magick',str(path),'-crop',f'170x25+41+{128+row*34}','+repage','-depth','8','rgb:-'])
    try:
        wait(lambda:state().get('controller_pid'),'Controller missing')
        invoke('--action','projects:sort:activity')
        s=wait(lambda:state() if state().get('project_navigation',{}).get('sort')=='recent' and state().get('project_activity',{}).get(names[0],{}).get('working')==1 and state()['project_activity'][names[2]].get('running')==1 and state().get('desktop',{}).get('project_windows') else None,'Activity summary missing')
        assert s['project_order']==[names[0],names[2],names[1]],'Initial backend activity did not sort before new output'
        assert s['project_activity'][names[2]]['waiting']==0,'Blocked status falsely requested attention'
        wait(lambda:8<=state()['wave_phase']<=26,'Wave phase did not advance')
        samples=[capture('wave-0'),capture('wave-1'),capture('wave-2')]
        assert len({crop(p,0) for p in samples})>1,'Working text did not animate'
        assert len({crop(p,1) for p in samples})==1,'Blocked project text flashed'
        assert len({crop(p,2) for p in samples})==1,'Idle project text flashed'
        icon_x=s['project_navigation']['sidebar_width']-29
        def icon(path,row):return subprocess.check_output(['magick',str(path),'-crop',f'20x22+{icon_x}+{128+row*34}','+repage','-depth','8','rgb:-'])
        assert len({icon(p,0) for p in samples})>1,'Working icon did not animate'
        assert len({icon(p,1) for p in samples})==1,'Idle/blocked agent icon animated'
        warning=state()['appearance']['tokens']['warning'].lstrip('#');warning=tuple(int(warning[i:i+2],16) for i in [0,2,4])
        def amber(raw):return any(max(abs(raw[i+j]-warning[j]) for j in range(3))<=3 for i in range(0,len(raw),3))
        assert not amber(icon(samples[0],1)),'Blocked agent falsely showed an amber attention icon'
        statuses.write_text(json.dumps({'w0':'working','w1':'idle','w2':'needs_permission'}))
        wait(lambda:state()['project_activity'][names[2]]['waiting']==1,'Explicit approval status not detected')
        approval=[capture('approval-0'),capture('approval-1')]
        assert amber(icon(approval[0],1)),'Explicit approval icon was not amber'
        assert icon(approval[0],1)==icon(approval[1],1),'Approval marker should be static'
        statuses.write_text(json.dumps({'w0':'working','w1':'idle','w2':'needs_input'}))
        wait(lambda:state()['project_activity'][names[2]].get('questions')==1,'Pending question missing')
        question=capture('question-pending')
        assert amber(icon(question,1)),'Question marker was not amber'
        assert icon(question,1)!=icon(approval[0],1),'Question and approval markers were identical'
        statuses.write_text(json.dumps({'w0':'stopped','w1':'stopped','w2':'stopped'}))
        wait(lambda:all(a['working']==0 and a['waiting']==0 for a in state()['project_activity'].values()),'Agents did not stop')
        time.sleep(.6);phase=state()['wave_phase'];time.sleep(.7)
        assert state()['wave_phase']==phase,'Animation ran without live agents'
        old=state()['controller_pid'];invoke('--close');time.sleep(.4)
        statuses.write_text(json.dumps({'w0':'working','w1':'idle','w2':'blocked'}))
        rc('launch','--type=tab','--env','STARMUX_REDUCED_MOTION=1',str(app),'--workspace',names[0])
        wait(lambda:state()['controller_pid']!=old and state().get('reduced_motion') and state()['project_activity'][names[0]]['working']==1,'Reduced-motion workspace missing')
        phase=state()['wave_phase'];time.sleep(.7);assert state()['wave_phase']==phase
        capture('reduced-motion')
        statuses.write_text(json.dumps({'w0':'idle','w1':'idle','w2':'idle'}))
        wait(lambda:all(a['working']==0 and a['waiting']==0 for a in state()['project_activity'].values()),'Agents did not become idle')
        values={'w0':0,'w1':0,'w2':0}
        for i,name in enumerate(names):
            previous=state()['project_navigation']['last_activity'][name]
            values['w'+str(i)]+=1;revisions.write_text(json.dumps(values))
            wait(lambda:state()['project_navigation']['last_activity'][name] and state()['project_navigation']['last_activity'][name]!=previous,'New output did not update recency')
        wait(lambda:state()['project_order']==names[::-1],'High did not sort recent output first')
        selected=state()['active_project']
        def move(x,y):rc('send-text','--stdin',data=f'\x1b[<35;{x};{y}M'.encode())
        def click(x,y):
            rc('send-key','ctrl+escape');rc('send-text','--stdin',data=f'\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m'.encode())
        move(50,65);wait(lambda:not state()['project_navigation']['hover'],'Hover did not reset')
        before=capture('sort-text')
        sx=state()['project_navigation']['sidebar_width']-50
        move(sx,98);wait(lambda:state()['project_navigation']['hover'],'Sort hover border missing')
        after=capture('sort-hover')
        raw=[subprocess.check_output(['magick',str(p),'-depth','8','rgb:-']) for p in [before,after]]
        changed=[(i%260,i//260) for i in range(260*240) if raw[0][i*3:i*3+3]!=raw[1][i*3:i*3+3]]
        assert changed,'Hover outline did not render'
        left=min(x for x,y in changed);right=max(x for x,y in changed)
        top=min(y for x,y in changed);bottom=max(y for x,y in changed)
        assert (right-left+1,bottom-top+1)==(68,24),'Unexpected sort hover size'
        assert all(x in [left,right] or y in [top,bottom] for x,y in changed),'Hover filled the sort control interior'
        click(sx,98)
        wait(lambda:not state()['project_navigation']['high_first'] and state()['project_order']==names,'Low did not reverse High')
        statuses.write_text(json.dumps({'w0':'working','w1':'needs_permission','w2':'stopped'}))
        wait(lambda:state()['project_activity'][names[0]]['working']==1 and state()['project_order']==names[::-1],'Low did not put the busiest project at the bottom')
        capture('recent-low')
        click(sx,98);wait(lambda:state()['project_navigation']['high_first'] and state()['project_order']==names,'High did not put working agents on top')
        capture('recent-high')
        click(sx,98);wait(lambda:not state()['project_navigation']['high_first'] and state()['project_order']==names[::-1],'Full reverse was not repeatable')
        assert state()['active_project']==selected
        old=state()['controller_pid'];invoke('--close');time.sleep(.4)
        rc('launch','--type=tab','--env','STARMUX_REDUCED_MOTION=1',str(app),'--workspace',names[0])
        wait(lambda:state()['controller_pid']!=old and not state()['project_navigation']['high_first'] and state()['project_order']==names[::-1],'Low preference and full order were not restored')
        requests=[json.loads(l) for l in (temp/'requests').read_text().splitlines()]
        assert all(r in [['agent','list'],['api','snapshot'],['pane','get'],['pane','read'],['terminal','attach']] for r in requests)
        result={'passed':True,'first_sample_uses_backend_recency':True,'working_agents_automatically_on_top_in_high':True,'low_is_full_reverse_with_busiest_at_bottom':True,'sort_text_has_hover_border_only':True,'only_working_agent_icons_animate':True,'idle_agent_icons_clear':True,'pending_question_marker':True,'blocked_status_does_not_falsely_request_attention':True,'explicit_approval_has_static_amber_marker':True,'reduced_motion_and_idle_timer':True,'sort_and_history_survive_restart':True,'sort_preserves_selected_project':True,'batched_read_only_activity':True,'original_herdr_sessions_touched':False}
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
                for suffix in ['.json','.layout.json','.state.json','.workspace.json','.navigation.json']:(root/(name+suffix)).unlink(missing_ok=True)
        if original:subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.focus({{window="address:{original}"}}))'],stdout=subprocess.DEVNULL)
