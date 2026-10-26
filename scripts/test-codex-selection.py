#!/usr/bin/env python3
"""Select the Codex banner in a disposable real Codex session; never send a prompt or approval."""
import json,os,subprocess,tempfile,time,uuid
from pathlib import Path
repo=Path(__file__).resolve().parents[1];app=repo/'scripts/run-local.sh'
runtime=Path(os.environ['XDG_RUNTIME_DIR'])/'starmux'
out=repo/'target/codex-selection-proof';out.mkdir(parents=True,exist_ok=True)
name='codex-select-'+uuid.uuid4().hex[:8]
original=json.loads(subprocess.check_output(['hyprctl','activewindow','-j'])).get('address')
backup_type=None;backup=None
try:
    types=subprocess.check_output(['wl-paste','--list-types'],stderr=subprocess.DEVNULL,timeout=3).decode().splitlines()
    backup_type=next((t for t in ['image/png','text/plain;charset=utf-8','text/plain'] if t in types),types[0] if types else None)
    if backup_type:backup=subprocess.check_output(['wl-paste','--type',backup_type,'--no-newline'],stderr=subprocess.DEVNULL,timeout=3)
except subprocess.SubprocessError:pass
def wait(fn,label):
    end=time.monotonic()+40
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (ValueError,OSError,KeyError,subprocess.SubprocessError):pass
        time.sleep(.1)
    raise AssertionError(label)
with tempfile.TemporaryDirectory(prefix='starmux-codex-selection-') as temp:
    temp=Path(temp);address='unix:'+str(temp/'kitty.sock')
    def rc(target,*args,data=None):return subprocess.check_output(['kitten','@','--to',target,*args],input=data,stderr=subprocess.PIPE,timeout=10)
    def invoke(*args):return subprocess.check_output([str(app),'--workspace',name,*args],stderr=subprocess.PIPE,timeout=20)
    def state():return json.loads((runtime/(name+'.state.json')).read_text())
    kitty=subprocess.Popen(['/usr/bin/kitty','--hold','--class','starmux-codex-selection-proof','--listen-on',address,'-o','allow_remote_control=yes',str(app),'--workspace',name,'--directory',str(temp),'--agent','codex'],env=dict(os.environ,STAR_GRAPHICS_RENDERER='gpu',STAR_TERMINAL_RENDERER='kitty'),stdout=subprocess.DEVNULL,stderr=(out/'kitty.log').open('w'))
    try:
        wait(lambda:state().get('desktop',{}).get('project_windows'),'Codex window missing')
        inner='unix:'+str(runtime/f'kitty-{name}-1.sock')
        text=wait(lambda:rc(inner,'get-text').decode() if b'Codex' in rc(inner,'get-text') else None,'Codex banner absent')
        # Codex repaints its startup view while checking for updates and
        # discovering project context. Select once that output has settled.
        time.sleep(3)
        text=rc(inner,'get-text').decode()
        row,line=next((i,line) for i,line in enumerate(text.splitlines()) if 'Codex' in line)
        query=temp/'geometry.py'
        query.write_text("""import json
from kittens.tui.handler import result_handler
def main(args):pass
@result_handler(no_ui=True)
def handle_result(args,answer,target_window_id,boss):
 from kitty.fast_data_types import cell_size_for_window
 w=boss.window_id_map[target_window_id]
 cw,ch=cell_size_for_window(w.os_window_id)
 return json.dumps(dict(left=w.geometry.left,top=w.geometry.top,cw=cw,ch=ch,mouse=repr(w.current_mouse_position()),has_selection=w.has_selection(),button=w.current_mouse_event_button,lines=[str(w.screen.line(i)) for i in range(w.screen.lines)]))
""")
        geometry=json.loads(rc(inner,'kitten',str(query)));cw=geometry['cw'];ch=geometry['ch']
        # get-text joins soft-wrapped rows. Use Kitty's physical grid to target
        # the banner even when an update notice wraps above it.
        row,line=next((i,line) for i,line in enumerate(geometry.pop('lines')) if 'Codex' in line)
        col=line.index('Codex');s=state()
        (out/'line.json').write_text(json.dumps({'line':line,'row':row,'columns':json.loads(rc(inner,'ls'))[0]['tabs'][0]['windows'][0]['columns']}))
        x=s['project_navigation']['sidebar_width']+15+geometry['left']+col*cw+2
        y=201+geometry['top']+row*ch+ch/2
        (out/'geometry.json').write_text(json.dumps(dict(geometry=geometry,row=row,col=col,x=x,y=y,canvas=s['terminal_presentation']['canvas'])))
        end=x+5*cw-3
        rc(address,'send-key','ctrl+escape')
        def mouse(button,x,y,end='M'):
            rc(address,'send-text','--stdin',data=f'\x1b[<{button};{round(x)};{round(y)}{end}'.encode())
        def screenshot(label):
            c=next(c for c in json.loads(subprocess.check_output(['hyprctl','clients','-j'])) if c['pid']==kitty.pid)
            subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.focus({{window="address:{c["address"]}"}}))'],stdout=subprocess.DEVNULL)
            time.sleep(.2);c=next(c for c in json.loads(subprocess.check_output(['hyprctl','clients','-j'])) if c['pid']==kitty.pid)
            px,py=c['at'];pw,ph=c['size'];subprocess.run(['grim','-s','1','-g',f'{px},{py} {pw}x{ph}',str(out/(label+'.png'))],check=True)
        screenshot('before')
        mouse(0,x,y)
        for i in range(1,8):mouse(32,x+(end-x)*i/7,y)
        time.sleep(.1)
        (out/'mid-selection.json').write_text(json.dumps({'selected':rc(inner,'get-text','--extent','selection').decode(),'geometry':json.loads(rc(inner,'kitten',str(query)))}))
        # Keep the pointer grab undisturbed during this diagnostic.
        mouse(0,end,y,'m')
        selected=wait(lambda:rc(inner,'get-text','--extent','selection').decode().rstrip('\n') or None,'Codex banner selection failed')
        assert selected=='Codex',repr(selected)
        wait(lambda:subprocess.check_output(['wl-paste','--no-newline'],stderr=subprocess.DEVNULL)==b'Codex','Real Codex selection did not reach desktop clipboard')
        screenshot('after')
        result={'passed':True,'real_codex_selection':selected,'copied_to_desktop_clipboard':True,'prompts_sent':0,'approvals_answered':0,'workspace':name}
        (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
    finally:
        try:invoke('--stop')
        except subprocess.SubprocessError:pass
        try:rc(address,'close-window')
        except subprocess.SubprocessError:pass
        if kitty.poll() is None:kitty.terminate();kitty.wait(timeout=10)
        if backup_type and backup is not None:subprocess.run(['wl-copy','--type',backup_type],input=backup,check=True)
        elif backup_type is None:subprocess.run(['wl-copy','--clear'],check=True)
        if original:subprocess.run(['hyprctl','eval',f'hl.dispatch(hl.dsp.focus({{window="address:{original}"}}))'],stdout=subprocess.DEVNULL)
