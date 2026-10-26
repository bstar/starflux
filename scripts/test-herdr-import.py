#!/usr/bin/env python3
"""Validate the imported Herdr test collection; leave its window open for review.

Does not type into Herdr terminals. Navigation targets the STAR/MUX linked controller.
"""
import json,os,pathlib,subprocess,time
repo=pathlib.Path(__file__).resolve().parents[1];app=repo/'scripts/run-local.sh';runtime=pathlib.Path(os.environ['XDG_RUNTIME_DIR'])/'starmux';data=pathlib.Path(os.environ.get('XDG_DATA_HOME',str(pathlib.Path.home()/'.local/share')))/'starmux/projects';out=repo/'target/herdr-import-proof';out.mkdir(parents=True,exist_ok=True)
report=json.loads((data/'herdr-default.import.json').read_text());workspace=report['selected']
for snapshot in runtime.glob('herdr-default-*.state.json'):
 try:
  existing=json.loads(snapshot.read_text())
  if existing.get('collection')=='herdr-default' and (runtime/(existing['workspace']+'.sock')).exists():workspace=existing['workspace'];break
 except (OSError,ValueError,KeyError):pass
address='unix:'+str(runtime/'herdr-import-test.sock')
def wait(fn,label):
 for _ in range(180):
  try:
   result=fn()
   if result:return result
  except (OSError,ValueError,KeyError,subprocess.SubprocessError):pass
  time.sleep(.1)
 raise AssertionError(label)
def state():return json.loads((runtime/(workspace+'.state.json')).read_text())
def action(text):subprocess.check_call([str(app),'--workspace',workspace,'--action',text],stdout=subprocess.DEVNULL)
# A new import has a distinct workspace identity; never close a pre-existing window.
if not (runtime/(workspace+'.sock')).exists():
 with (out/'kitty.log').open('w') as log:subprocess.Popen(['/usr/bin/kitty','--class','starmux-herdr-test','--title','STAR/MUX - Herdr session test','--config','NONE','--listen-on',address,'-o','allow_remote_control=yes','-o','initial_window_width=1450','-o','initial_window_height=1000',str(app),'--workspace',workspace],stdout=log,stderr=log,start_new_session=True)
s=wait(lambda:state() if state().get('herdr_live_attachment') and state()['desktop']['project_windows'] else None,'Imported window did not start')
expected=[p['workspace']for p in report['projects']];assert s['project_order']==expected,'Import order differs from Herdr'
assert len(expected)==report['workspaces']
before=json.loads((repo/'target/herdr-source-before.json').read_text());baseline_order=s['project_order']
for project in [expected[0],expected[5],expected[-1],expected[1],workspace]:
 action('project:'+project);s=wait(lambda:state() if state()['active_project']==project else None,'Project switch failed')
 assert s['project_order']==baseline_order,'Selecting a project reordered tabs'
 assert s['collection']=='herdr-default'
action('project:'+expected[0]);wait(lambda:state()['active_project']==expected[0],'HOME missing')
action('terminal:1');s=wait(lambda:state() if state()['project_state']['terminal']==1 else None,'Second HOME tab missing');assert s['project_order']==baseline_order
page=s['project_page'];action('project:page:right');wait(lambda:state()['project_page']>page,'Project paging failed');assert state()['active_project']==expected[0],'Paging selected a different project'
action('project:'+workspace);wait(lambda:state()['active_project']==workspace,'Return to imported current space failed')
source=json.loads(subprocess.check_output(['herdr','api','snapshot']))['result']['snapshot'];assert [w['workspace_id']for w in source['workspaces']]==before['workspaces'],'Herdr workspace order changed';assert {p['pane_id']for p in source['panes']}==set(before['panes']),'Herdr topology changed'
identities={}
for pane_id,old in before['panes'].items():
 info=json.loads(subprocess.check_output(['herdr','pane','process-info','--pane',pane_id]))['result']['process_info'];assert info['shell_pid']==old['shell_pid'],'Herdr shell was replaced: '+pane_id
 identities[pane_id]={'shell_pid':info['shell_pid'],'foreground_pids':[p['pid']for p in info['foreground_processes']]}
result={'passed':True,'workspaces':len(expected),'tabs':report['tabs'],'source_panes':len(identities),'detected_agents':report['agents'],'stable_project_order':True,'paging_does_not_switch_project':True,'original_Herdr_topology_and_shells_preserved':True,'interactive_live_attachments':True,'selected':workspace,'window_left_open':True,'identities':identities};(out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items()if k!='identities'},indent=2))
try:
 clients=json.loads(subprocess.check_output(['hyprctl','clients','-j']));c=next(c for c in clients if c['class']=='starmux-herdr-test');addr=c['address']
 for expr in [f'hl.dsp.window.float({{action="set",window="address:{addr}"}})',f'hl.dsp.window.resize({{x=1450,y=1000,relative=false,window="address:{addr}"}})']:
  subprocess.run(['hyprctl','eval','hl.dispatch('+expr+')'],check=True,stdout=subprocess.DEVNULL)
 time.sleep(1);c=next(c for c in json.loads(subprocess.check_output(['hyprctl','clients','-j']))if c['address']==addr);x,y=c['at'];w,h=c['size'];subprocess.run(['grim','-g',f'{x},{y} {w}x{h}',str(out/'imported-session.png')],check=True)
except Exception as error:print('Screenshot:',error)
