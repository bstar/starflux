#!/usr/bin/env python3
"""Exercise independent unnamed Kitty tabs and reject duplicate named controllers."""
import json,os,pathlib,subprocess,tempfile,time
repo=pathlib.Path(__file__).resolve().parents[1];app=repo/'scripts/run-local.sh';runtime=pathlib.Path(os.environ['XDG_RUNTIME_DIR'])/'starflux'
def wait(fn,label):
    for _ in range(150):
        try:
            result=fn()
            if result:return result
        except (OSError,ValueError,KeyError,subprocess.SubprocessError):pass
        time.sleep(.1)
    raise AssertionError(label)
with tempfile.TemporaryDirectory(prefix='starflux-tabs-') as directory:
    tmp=pathlib.Path(directory);address='unix:'+str(tmp/'kitty.sock');log=(tmp/'kitty.log').open('w')
    child=subprocess.Popen(['/usr/bin/kitty','--config','NONE','--class','starflux-tab-proof','--listen-on',address,'-o','allow_remote_control=yes',str(app)],stdout=log,stderr=log)
    names=[]
    def rc(*args):return subprocess.check_output(['kitten','@','--to',address,*args],stderr=subprocess.PIPE,timeout=10)
    def tabs():return json.loads(rc('ls'))[0]['tabs']
    def workspace(pid):
        matches=list(runtime.glob(f'session-{pid}-*.json'));matches=[p for p in matches if not p.name.endswith('.state.json')]
        return json.loads(matches[0].read_text())['workspace'] if matches else None
    def inspect(name):return json.loads(subprocess.check_output([str(app),'--workspace',name,'--inspect'],timeout=10))
    try:
        first=wait(lambda:tabs()[0]['windows'][0],'first tab missing');names.append(wait(lambda:workspace(first['pid']),'first tab reused demo instead of generating a workspace'))
        wait(lambda:inspect(names[0])['desktop']['windows']==2,'first workspace not mapped')
        rc('launch','--type=tab',str(app))
        second=wait(lambda:tabs()[1]['windows'][0] if len(tabs())==2 else None,'second tab missing');names.append(wait(lambda:workspace(second['pid']),'second unnamed workspace missing'))
        wait(lambda:inspect(names[1])['desktop']['windows']==2,'second workspace not mapped')
        before=[inspect(n) for n in names];controllers=[s['controller_pid'] for s in before]
        gpu_libraries=[]
        if os.environ.get('STAR_GRAPHICS_RENDERER')=='gpu':
            for window in [first,second]:
                maps=wait(lambda:pathlib.Path(f'/proc/{window["pid"]}/maps').read_text(),'GPU frontend maps unavailable')
                assert 'libvulkan' in maps,'Strict GPU frontend did not load a Vulkan device'
                gpu_libraries.append(sorted({line.split()[-1] for line in maps.splitlines() if 'libvulkan' in line}))
        assert names[0]!=names[1] and controllers[0]!=controllers[1]
        for _ in range(4):
            for tab in tabs():rc('focus-tab','--match','id:'+str(tab['id']));time.sleep(.15)
        for i,name in enumerate(names):
            result=subprocess.run([str(app),'--graphical-relay',name],input=b'',stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=5)
            reply=json.loads(result.stdout)
            assert reply['type']=='error' and 'already open in another tab' in reply['message'],reply
            state=inspect(name);assert state['controller_pid']==controllers[i]
            assert [p['pid']for p in state['panes']]==[p['pid']for p in before[i]['panes']]
        # The named attachment becomes available when that specific frontend exits.
        first_tab=next(t for t in tabs() if t['windows'][0]['pid']==first['pid'])
        rc('close-tab','--match','id:'+str(first_tab['id']))
        time.sleep(.5)
        rc('launch','--type=tab',str(app),'--workspace',names[0])
        wait(lambda:len(tabs())==2,'named reattachment failed')
        time.sleep(1)
        assert inspect(names[0])['controller_pid']==controllers[0]
        result={'passed':True,'independent_unnamed_tabs':True,'duplicate_named_attachment_rejected':True,'tab_switches':8,'renderer':os.environ.get('STAR_GRAPHICS_RENDERER','cpu'),'gpu_libraries':gpu_libraries,'controller_pids_preserved':controllers,'workspaces':names}
        out=repo/'target/tab-proof.json';out.write_text(json.dumps(result,indent=2)+'\n');print(out.read_text())
    finally:
        for name in names:
            subprocess.run([str(app),'--workspace',name,'--stop'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=15)
        if child.poll() is None:child.terminate();child.wait(timeout=5)
