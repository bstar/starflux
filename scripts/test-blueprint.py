#!/usr/bin/env python3
"""Check Blueprint in real Kitty, live theme propagation, and retained processes."""
import json,os,pathlib,subprocess,tempfile,time
root=pathlib.Path(__file__).resolve().parents[1];app=str(root/'scripts/run-local.sh');state=pathlib.Path(os.environ['XDG_RUNTIME_DIR'])/'starmux';out=root/'target/blueprint-proof';out.mkdir(exist_ok=True)
name='blueprint-proof-'+str(os.getpid())
def wait(fn,label):
    for _ in range(150):
        try:
            value=fn()
            if value:return value
        except (OSError,KeyError,ValueError,subprocess.SubprocessError):pass
        time.sleep(.1)
    raise AssertionError(label)
def inspect():return json.loads((state/(name+'.state.json')).read_text())
original=json.loads(subprocess.check_output(['hyprctl','activewindow','-j'])).get('address')
with tempfile.TemporaryDirectory(prefix='starmux-blueprint-') as tmp:
    address='unix:'+tmp+'/kitty.sock';log=(out/'kitty.log').open('w')
    child=subprocess.Popen(['/usr/bin/kitty','--config','NONE','--class','starmux-blueprint-proof','--listen-on',address,'-o','allow_remote_control=yes','-o','window_padding_width=0','-o','background_opacity=1','-o','font_size=12',app,'--workspace',name,'--theme','kanagawa'],stdout=log,stderr=log,env={**os.environ,'STAR_GRAPHICS_RENDERER':'gpu'})
    def rc(*args,data=None):return subprocess.check_output(['kitten','@','--to',address,*args],input=data,stderr=subprocess.PIPE,timeout=10)
    def snapshot(file):
        c=next(c for c in json.loads(subprocess.check_output(['hyprctl','clients','-j'])) if c['class']=='starmux-blueprint-proof')
        subprocess.run(['hyprctl','eval','hl.dispatch(hl.dsp.focus({window="address:'+c['address']+'"}))'],stdout=subprocess.DEVNULL,check=True);time.sleep(.25)
        x,y=c['at'];w,h=c['size'];subprocess.run(['grim','-g',f'{x},{y} {w}x{h}',str(out/file)],check=True)
    try:
        c=wait(lambda:next((c for c in json.loads(subprocess.check_output(['hyprctl','clients','-j'])) if c['class']=='starmux-blueprint-proof'),None),'Kitty window missing')
        addr=c['address']
        for expr in [f'hl.dsp.window.float({{action="set",window="address:{addr}"}})',f'hl.dsp.window.resize({{x=1200,y=700,relative=false,window="address:{addr}"}})',f'hl.dsp.window.move({{x=2700,y=180,relative=false,window="address:{addr}"}})']:
            subprocess.run(['hyprctl','eval','hl.dispatch('+expr+')'],check=True,stdout=subprocess.DEVNULL)
        initial=wait(lambda:inspect() if inspect()['appearance']['treatment']=='blueprint' and inspect()['desktop']['windows']==2 else None,'Blueprint did not map two terminals')
        initial=wait(lambda:inspect() if inspect()['panes'][1].get('command')=='codex' else None,'Default second pane did not run real Codex')
        pids=[p['pid'] for p in initial['panes']];controller=initial['controller_pid'];time.sleep(1);snapshot('kanagawa.png')
        subprocess.run([app,'--workspace',name,'--theme','catppuccin-latte'],check=True,stdout=subprocess.PIPE)
        light=wait(lambda:inspect() if inspect()['appearance']['theme']=='catppuccin-latte' else None,'Theme did not change live')
        assert light['controller_pid']==controller and [p['pid'] for p in light['panes']]==pids
        for pane in [0,1]:
            socket='unix:'+str(state/f'kitty-{name}-{pane}.sock')
            colors=wait(lambda:subprocess.check_output(['kitten','@','--to',socket,'get-colors'],text=True,stderr=subprocess.PIPE,timeout=10),'Inner Kitty did not expose its palette')
            bg=next(line.split()[1] for line in colors.splitlines() if line.startswith('background '))
            assert bg==light['appearance']['tokens']['bg'],(bg,light['appearance']['tokens']['bg'])
        time.sleep(.3);snapshot('latte.png')
        rc('send-key','t')
        changed=wait(lambda:inspect() if inspect()['appearance']['theme']!='catppuccin-latte' else None,'Keyboard theme control failed')
        assert [p['pid'] for p in changed['panes']]==pids
        rc('send-text','--stdin',data=b'\x1b[<0;1000;25M\x1b[<0;1000;25m')
        clicked=wait(lambda:inspect() if inspect()['appearance']['theme']!=changed['appearance']['theme'] else None,'Theme button hit target failed')
        assert [p['pid'] for p in clicked['panes']]==pids
        # Return to the exact reference palette and verify input through the new geometry.
        subprocess.run([app,'--workspace',name,'--theme','kanagawa'],check=True,stdout=subprocess.PIPE)
        wait(lambda:inspect()['appearance']['theme']=='kanagawa','Theme did not return')
        subprocess.run(['python3',str(root/'scripts/test-input.py'),'--to',address,'--workspace',name,'--output',str(out/'input.txt')],check=True)
        result={'passed':True,'renderer':'gpu','treatment':'blueprint','default_agent':'codex','viewport':initial['viewport'],'live_theme_switch':True,'inner_kitty_palette_synchronized':True,'keyboard_theme_control':True,'theme_button_hit_target':True,'terminal_input':True,'controller_pid':controller,'agent_pids_preserved':pids,'themes':['kanagawa','catppuccin-latte']}
        (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
    finally:
        subprocess.run([app,'--workspace',name,'--stop'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=15)
        if child.poll() is None:child.terminate();child.wait(timeout=5)
        if original:subprocess.run(['hyprctl','eval','hl.dispatch(hl.dsp.focus({window="address:'+original+'"}))'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
