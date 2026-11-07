"""Executed inside test-session-manager's isolated two-frontend fixture."""
import json, pathlib, os, secrets, time
fake=tmp/'fake-codex-provider'
log=tmp/'provider-calls.jsonl'
fake.write_text('''#!/usr/bin/python3
import json,sys,pathlib,time
log=pathlib.Path(LOG)
def send(v):print(json.dumps(v),flush=True)
def record(method):
 with log.open('a') as f:f.write(json.dumps({'method':method})+'\\n')
thread='00000000-0000-4000-8000-000000000001'
for line in sys.stdin:
 v=json.loads(line);method=v.get('method');record(method or 'tool_result');i=v.get('id');p=v.get('params',{})
 if method=='initialize':send(dict(id=i,result={}))
 elif method=='account/read':send(dict(id=i,result={'account':{'type':'chatgpt','planType':'pro'}}))
 elif method=='model/list':send(dict(id=i,result={'data':[{'id':'fake-codex','model':'fake-codex','displayName':'Fake Codex','supportedReasoningEfforts':[{'reasoningEffort':'high'}]}]}))
 elif method=='config/read':send(dict(id=i,result={'config':{'mcp_servers':{}}}))
 elif method in ['thread/start','thread/resume']:send(dict(id=i,result={'thread':{'id':thread}}))
 elif method=='turn/start':
  send(dict(id=i,result={'turn':{'id':'turn-one'}}))
  if 'BLOCK_REVIEW' in p['input'][0]['text']:
   send({'method':'item/agentMessage/delta','params':{'delta':'Waiting in fake provider'}})
   for _ in sys.stdin:pass
   break
  for n,arguments in enumerate([dict(project=PROJECT,operation='read',path='review-evidence.txt',pattern=''),dict(project=PROJECT,operation='read',path='.env',pattern=''),dict(project='outside-category',operation='read',path='review-evidence.txt',pattern='')]):
   send({'id':'tool-'+str(n),'method':'item/tool/call','params':{'tool':'category_repository','arguments':arguments,'threadId':thread,'turnId':'turn-one','callId':str(n)}})
   result=json.loads(sys.stdin.readline())['result'];payload=json.loads(result['contentItems'][0]['text'])
   if n==0:assert payload['result']['text']=='café 界',payload
   else:assert 'error' in payload,payload
  result={'summary':'Compared the member interfaces using scoped source evidence.','findings':[{'project':PROJECT,'severity':'gap','title':'A shared interface needs alignment','detail':'Confirm the same behaviour across members.','evidence':[PROJECT+':review-evidence.txt:1']}],'proposals':[{'project':PROJECT,'title':'Align the shared interface','text':'Implement the scoped shared interface. Verify the acceptance behaviour with a focused test.','evidence':[PROJECT+':review-evidence.txt:1']}],'requests':[]}
  text=json.dumps(result)
  send({'method':'item/agentMessage/delta','params':{'delta':text[:20]}})
  send({'method':'item/completed','params':{'item':{'type':'agentMessage','text':text}}})
  send({'method':'turn/completed','params':{'turn':{'id':'turn-one','status':'completed'}}})
'''.replace('LOG',repr(str(log))).replace('PROJECT',repr(name)))
fake.chmod(0o700)

def state():return api('category_agent.get',base)
def profile_update(**change):
 s=state()['state'];p=dict(s['profile']);p.update(change)
 return api('category_agent.configure',dict(**base,expected_revision=s['revision'],profile=p),write=True)
def calls():return [json.loads(line)['method'] for line in log.read_text().splitlines()] if log.exists() else []
def review(mode='review',message=''):
 r=api('category_agent.start',dict(**base,mode=mode,message=message),write=True)
 return r['id']
def result(identity):
 return next((r for r in state()['runtime']['history'] if r['id']==identity),{})
def completed(identity):
 r=result(identity)
 if r.get('status') in ['failed','indeterminate']:raise AssertionError(r.get('error'))
 return r if r.get('status')=='completed' else None

a.action('category-agent:open:'+gid)
wait(lambda:(a.snapshot().get('context_menu')or{}).get('title')=='Category agent','category workspace did not open')
assert 'turn/start' not in calls(),'opening the workspace started inference'
profile_update(executable=str(fake),model='fake-codex',effort='high')
connected=review('connect');wait(lambda:completed(connected),'provider metadata did not connect')
assert 'turn/start' not in calls(),'connect started inference'
assert state()['runtime']['capabilities']['models'][0]['model']=='fake-codex'
if args.category_agent_selection:
 selection_bounds=[]
 def choose(action,keyboard=False):
  menu=wait(lambda:next((m for m in [a.snapshot().get('context_menu')] if m and any(i['action']==action for i in m['items'])),None),'category choice unavailable: '+action)
  index=next(i for i,item in enumerate(menu['items']) if item['action']==action)
  if keyboard:
   a.key('home')
   for _ in range(index):a.key('down')
   a.key('enter');return
  r=menu['bounds'];capacity=max(1,(r['height']-2) if args.cells else (r['height']-63)//34)
  offset=(menu.get('selected') or 0)//capacity*capacity
  assert offset<=index<offset+capacity,(index,offset,capacity)
  x=r['x']+10;y=r['y']+(1+index-offset if args.cells else 63+(index-offset)*34+16)
  pointer=dict(kind='pointer',action='down',button=0,modifiers=0,x=x if args.cells else x//10,y=y if args.cells else y//20)
  if not args.cells:pointer['pixel']=[x,y]
  if selection_bounds:assert r==selection_bounds[0],('category panel moved while selecting',r,selection_bounds[0])
  else:selection_bounds.append(r)
  a.input(pointer)

 wait(lambda:any('Model:' in i['label'] for i in (a.snapshot().get('context_menu')or{}).get('items',[])),'enabled controls did not appear')
 choose('category-agent:setup')
 choose('category-agent:tab:providers')
 choose('category-agent:provider:claude')
 wait(lambda:state()['state']['profile']['provider']=='claude' and not state()['state']['profile']['enabled'],'provider choice did not save')
 choose('category-agent:tab:authentication')
 choose('category-agent:authentication:api')
 wait(lambda:state()['state']['profile']['authentication']=='api','API authentication did not save')
 choose('category-agent:tab:authentication')
 choose('category-agent:authentication:subscription')
 wait(lambda:state()['state']['profile']['authentication']=='subscription','subscription choice did not save')
 choose('category-agent:tab:providers')
 choose('category-agent:provider:custom')
 wait(lambda:state()['state']['profile']['provider']=='custom','custom provider choice did not save')
 choose('category-agent:enable')
 def selection_error():return any(i['label'].startswith('Error:') for i in (a.snapshot().get('context_menu')or{}).get('items',[]))
 wait(selection_error,'invalid custom setup did not show an error')
 error_at=time.monotonic()
 wait(lambda:time.monotonic()-error_at>1.4 and selection_error(),'background refresh erased the selection error')
 assert not state()['state']['profile']['enabled'],'invalid custom setup enabled coordination'
 choose('category-agent:tab:providers')
 choose('category-agent:provider:codex')
 wait(lambda:state()['state']['profile']['provider']=='codex','Codex choice did not save')
 choose('category-agent:settings')
 wait(lambda:a.snapshot().get('modal_open'),'settings dialog did not open')
 a.paste(str(fake));a.accept()
 wait(lambda:state()['state']['profile']['executable']==str(fake),'executable did not save')
 choose('category-agent:enable')
 wait(lambda:state()['state']['profile']['enabled'],'explicit enable choice did not save')
 choose('category-agent:connect')
 wait(lambda:(state()['runtime'].get('capabilities') or {}).get('models'),'UI connect did not load models')
 choose('category-agent:tab:overview')
 choose('category-agent:tab:models')
 choose('category-agent:model:0')
 wait(lambda:state()['state']['profile']['model']=='fake-codex' and state()['state']['profile']['effort'] is None,'model choice did not save')
 choose('category-agent:tab:effort')
 choose('category-agent:effort:0',keyboard=True)
 wait(lambda:state()['state']['profile']['effort']=='high','effort choice did not save')
 assert 'turn/start' not in calls(),'selection started inference'
 print(json.dumps(dict(result='PASS',selection=True,ssh=args.ssh,cells=args.cells)))
 raise SystemExit(0)
first=review();first=wait(lambda:completed(first),'review did not complete')
assert first['sources'] and first['result']['findings'] and first['result']['proposals']
assert 'FICTITIOUS_NEVER_DISCLOSE' not in json.dumps(first)
conversation=state()['state']['conversation'];assert conversation
second=review('chat','Compare the current progress again');wait(lambda:completed(second),'conversation did not resume')
assert state()['state']['conversation']==conversation and 'thread/resume' in calls()
# Shared ownership, streamed progress and cancellation across both windows.
blocked=review('chat','BLOCK_REVIEW');wait(lambda:result(blocked).get('text'),'streamed text did not persist')
api('category_agent.start',dict(**base,mode='review',message=''),write=True,ok=False)
b.action('category-agent:open:'+gid)
wait(lambda:(b.snapshot().get('context_menu')or{}).get('title')=='Category agent','second window could not open coordinator')
api('category_agent.cancel',base,write=True)
wait(lambda:result(blocked).get('status')=='cancelled','cancel did not stop the owned review')
# Disabling revokes a live provider without touching the project agent.
blocked=review('chat','BLOCK_REVIEW');wait(lambda:result(blocked).get('text'),'second stream did not begin')
profile_update(enabled=False)
wait(lambda:result(blocked).get('status')=='cancelled','disable did not cancel the provider')
assert alive(original_pid),'category disable stopped the project process'
profile_update(enabled=True)
# Read-only safety and evidence remain available after re-enabling.
assert state()['state']['conversation']==conversation
assert all('terminal_history' not in json.dumps(r.get('result')) for r in state()['runtime']['history'])
print(json.dumps(dict(category_workflow='PASS',metadata_only_connect=True,shared_ownership=True,persistent_conversation=True,scoped_tools=True,streaming=True,cancellation=True,disable_preserves_projects=True)))

# Approval binds source fingerprints and exact project/conversation/process identity.
bindir=tmp/'handoff-bin';bindir.mkdir(exist_ok=True);delivery=tmp/'handoffs.jsonl'
queue=bindir/'codex';queue.write_text('#!/usr/bin/python3\nimport json,sys,pathlib\nassert sys.argv[1]=="queue"\nwith pathlib.Path('+repr(str(delivery))+').open("a") as f:f.write(json.dumps(sys.argv[2:])+"\\n")\n');queue.chmod(0o700)
env['PATH']=str(bindir)+os.pathsep+env['PATH']
pane=api('panes.get',dict(project=name,pane=1));assert pane['status'].get('agent_pid'),pane
run=secrets.token_hex(16);sequence=0

def report(lifecycle,conversation='fixture-project-conversation'):
 global sequence
 sequence+=1
 current=api('panes.get',dict(project=name,pane=1))
 values=dict(run_id=run,sequence=sequence,epoch=current['epoch'],agent_pid=current['status']['agent_pid'],agent_start_ticks=current['status']['agent_start_ticks'],boot_id=current['status']['boot_id'],harness='codex',source='proof',state=lifecycle,conversation_id=conversation,resume_argv=['codex','resume',conversation])
 return api('agents.report',dict(project=name,pane=1,report=values),write=True)
report('working')
identity=api('panes.get',dict(project=name,pane=1))['target']
# Keep an editable terminal draft. Native queued delivery must not paste or submit it.
api('panes.send',dict(target=identity,text='DRAFT_DO_NOT_TOUCH',submit=False),write=True)
third=review();third=wait(lambda:completed(third),'approval review did not complete')
proposal=third['result']['proposals'][0];approval=dict(**base,review=third['id'],proposal=proposal['id'],target=identity,conversation='fixture-project-conversation')
(tmp/'review-evidence.txt').write_text('changed after assessment')
api('category_agent.approve',approval,write=True,ok=False)
assert not delivery.exists(),'stale source evidence dispatched work'
(tmp/'review-evidence.txt').write_text('café 界')
a.action('category-agent:open:'+gid)
wait(lambda:(a.snapshot().get('context_menu')or{}).get('title')=='Category agent','approval workspace missing')
a.action('category-agent:proposal:'+third['id']+':'+proposal['id'])
a.action('category-agent:targets')
wait(lambda:any(i['action'].startswith('category-agent:target:') for i in (a.snapshot().get('context_menu')or{}).get('items',[])),'native agent target picker missing')
a.action('category-agent:target:0');a.action('category-agent:approve')
wait(lambda:a.snapshot().get('modal_open'),'exact handoff approval dialog missing')
a.paste('SEND');a.accept()
wait(lambda:result(third['id'])['result']['proposals'][0]['status']=='queued','UI approval did not queue the task')
time.sleep(.3);assert not delivery.exists(),'a busy project received an immediate task'
report('idle')
wait(lambda:delivery.exists(),'approved idle task did not reach the native queue')
wait(lambda:result(third['id'])['result']['proposals'][0]['status']=='delivered','delivery receipt did not persist')
api('category_agent.approve',approval,write=True,ok=False)
api('category_agent.resume_queue',base,write=True);time.sleep(.25)
assert len(delivery.read_text().splitlines())==1,'approved task was delivered twice'
queued=json.loads(delivery.read_text().splitlines()[0]);assert queued[queued.index('--message')+1]==proposal['text']
assert queued[queued.index('--thread')+1]=='fixture-project-conversation'
# Delivery does not claim that the project completed the task.
api('category_agent.task',dict(**base,review=third['id'],proposal=proposal['id'],action='verified'),write=True)
assert result(third['id'])['result']['proposals'][0]['status']=='verified_by_user'
capture=api('panes.capture',dict(project=name,pane=1))
assert 'DRAFT_DO_NOT_TOUCH' in capture.get('text','') and 'REPLY=DRAFT_DO_NOT_TOUCH' not in capture.get('text',''),'native queue altered or submitted the draft'
assert alive(original_pid)
print(json.dumps(dict(approval_workflow='PASS',source_freshness=True,busy_queue=True,exact_conversation=True,duplicate_safe=True,verification_separate_from_delivery=True)))

# Claude uses the same evidence boundary with every native tool disabled.
claude=tmp/'fake-claude-provider'
claude.write_text('''#!/usr/bin/python3
import json,sys
if sys.argv[1:3]==['auth','status']:
 print(json.dumps({'loggedIn':True,'authMethod':'claude.ai'}));raise SystemExit
assert '--print' in sys.argv
assert sys.argv[sys.argv.index('--tools')+1]==''
assert '--strict-mcp-config' in sys.argv and sys.argv[sys.argv.index('--permission-prompts')+1]=='none'
prompt=sys.stdin.read()
if 'SCOPED REPOSITORY TOOL RESULTS' not in prompt:
 result={'summary':'','findings':[],'proposals':[],'requests':[{'project':PROJECT,'operation':'read','path':'review-evidence.txt','pattern':''}]}
else:
 answers,_=json.JSONDecoder().raw_decode(prompt[prompt.index('['):]);assert answers[0]['response']['result']['text']=='café 界'
 result={'summary':'Claude reviewed the source evidence.','findings':[],'proposals':[],'requests':[]}
print(json.dumps({'type':'result','is_error':False,'structured_output':result}))
'''.replace('PROJECT',repr(name)));claude.chmod(0o700)
old=state()['state']['conversation']
profile_update(provider='claude',executable=str(claude),model='fake-claude',effort='high')
assert state()['state']['conversation'] is None and any(c['conversation']==old for c in state()['state']['archived_conversations'])
connected=review('connect');wait(lambda:completed(connected),'Claude metadata did not connect')
claude_review=review('chat','Compare this project');claude_review=wait(lambda:completed(claude_review),'Claude evidence review failed')
assert claude_review['sources'] and claude_review['result']['summary'].startswith('Claude')
# Custom adapters cannot read host project files directly; only broker results.
custom=tmp/'fake-custom-provider'
custom.write_text('''#!/usr/bin/python3
import json,sys,pathlib
boot=json.loads(sys.stdin.readline());assert boot['version']==1 and 'assessment_schema' in boot
assert not pathlib.Path(SECRET).exists(),'Host project files leaked into custom adapter sandbox'
if boot['mode']=='connect':
 print(json.dumps({'type':'metadata','capabilities':{'provider':'custom','models':[{'model':'fake-custom','displayName':'Fake Custom'}]}}),flush=True)
 print(json.dumps({'type':'complete','result':{}}),flush=True)
else:
 print(json.dumps({'type':'tool_call','id':'custom-read','arguments':{'project':PROJECT,'operation':'read','path':'review-evidence.txt','pattern':''}}),flush=True)
 reply=json.loads(sys.stdin.readline());assert reply['result']['text']=='café 界',reply
 print(json.dumps({'type':'conversation','id':'custom-conversation'}),flush=True)
 print(json.dumps({'type':'complete','result':{'summary':'Custom adapter reviewed only broker evidence.','findings':[],'proposals':[],'requests':[]}}),flush=True)
'''.replace('SECRET',repr(str(tmp/'review-evidence.txt'))).replace('PROJECT',repr(name)));custom.chmod(0o700)
profile_update(provider='custom',authentication='custom',executable=str(custom),model='fake-custom',effort=None)
custom_review=review();custom_review=wait(lambda:completed(custom_review),'custom adapter did not complete')
assert custom_review['sources']
print(json.dumps(dict(provider_adapters='PASS',claude_tools_disabled=True,claude_evidence_rounds=True,provider_switch_archives=True,custom_sandbox=True,custom_scoped_tools=True)))

# Deleting a category archives coordination history and revokes new access.
a.action('categories:new');wait(lambda:a.snapshot().get('modal_open'),'archive fixture category prompt missing');a.paste('Archived coordinator fixture');a.accept()
archived=wait(lambda:next((g for g in groups() if g['name']=='Archived coordinator fixture'),None),'archive fixture category not created')['id']
archive_base=dict(project=name,category=archived)
api('category_agent.configure',dict(**archive_base,expected_revision=0,profile=dict(enabled=True,provider='codex',authentication='subscription',executable=str(fake),model='fake-codex',charter='Disposable archive check')),write=True)
connection=api('category_agent.start',dict(**archive_base,mode='connect',message=''),write=True)['id']
wait(lambda:any(r['id']==connection and r['status']=='completed' for r in api('category_agent.get',archive_base)['runtime']['history']),'archive connection did not complete')
a.action('categories:delete:'+archived)
wait(lambda:all(g['id']!=archived for g in groups()),'category did not delete')
archived_data=api('category_agent.get',archive_base)
assert archived_data['state']['archived'] and not archived_data['state']['profile']['enabled']
assert any(r['id']==connection for r in archived_data['runtime']['history'])
api('category_agent.start',dict(**archive_base,mode='review',message=''),write=True,ok=False)
a.action('category-agent:archives')
wait(lambda:any(i['action']=='category-agent:archive-open:'+archived for i in (a.snapshot().get('context_menu')or{}).get('items',[])),'archived category history picker missing')
a.action('category-agent:archive-open:'+archived)
wait(lambda:(a.snapshot().get('context_menu')or{}).get('title')=='Category agent','archived category workspace missing')
assert not any(i['action']=='category-agent:review' for i in a.snapshot()['context_menu']['items'])
print(json.dumps(dict(category_archive='PASS',history_preserved=True,access_revoked=True,read_only_history_ui=True)))
