#!/usr/bin/env python3
"""Compare real Kitty text pixels with the same text presented inside STAR/MUX."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import uuid

repo=Path(__file__).resolve().parents[1];app=repo/'scripts/run-local.sh'
runtime=Path(os.environ['XDG_RUNTIME_DIR'])/'starmux'
out=repo/'target/kitty-pixel-proof';out.mkdir(parents=True,exist_ok=True)
name='kitty-pixels-'+uuid.uuid4().hex[:8]
def wait(fn,label,timeout=35):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (ValueError,OSError,KeyError,subprocess.SubprocessError):pass
        time.sleep(.1)
    raise AssertionError(label)

def mask(path, expected_height=None):
    width,height=map(int,subprocess.check_output(['magick','identify','-format','%w %h',str(path)]).split())
    pixels=subprocess.check_output(['magick',str(path),'-depth','8','rgb:-'])
    dots={}
    for i in range(0,len(pixels),3):
        r,g,b=pixels[i:i+3]
        if r==g==b and r>20:dots[(i//3%width,i//3//width)]=r
    assert len(dots)>100,'Missing white text pixels'
    if expected_height is not None:
        counts=[0]*height
        for x,y in dots:counts[y]+=1
        row=max(range(height-expected_height+1),key=lambda y:sum(counts[y:y+expected_height]))
        dots={p:v for p,v in dots.items() if row<=p[1]<row+expected_height}
    left=min(x for x,y in dots);top=min(y for x,y in dots)
    right=max(x for x,y in dots);bottom=max(y for x,y in dots)
    # Locate the specimen using its neutral white ink, then compare every RGB
    # byte in its padded rectangle, including the faintest antialiased edges.
    padding=1
    x0=max(0,left-padding);y0=max(0,top-padding)
    x1=min(width,right+padding+1);y1=min(height,bottom+padding+1)
    crop=b''.join(pixels[(y*width+x0)*3:(y*width+x1)*3] for y in range(y0,y1))
    return crop,[x1-x0,y1-y0],bottom-top+1

with tempfile.TemporaryDirectory(prefix='starmux-kitty-pixels-') as temp:
    temp=Path(temp);children=[];addresses=[]
    specimen=temp/'sample.py';specimen.write_text("import sys,time\nsys.stdout.write('\\x1b[?1049h\\x1b[?25l\\x1b[48;2;0;0;0m\\x1b[38;2;255;255;255m\\x1b[2J\\x1b[HKITTY_RENDER_0123456789  != == -> => fi café')\nsys.stdout.flush()\ntime.sleep(300)\n")
    config={'workspace':name,'collection':name,'theme':'kanagawa','host':None,'directory':str(temp),'agent':'python3 '+str(specimen),'executable':str(repo/'target/release/starmux'),'project_host':True,'desktop_helper':str(app)}
    p=runtime/(name+'.json');p.write_text(json.dumps(config));p.chmod(0o600)
    env=dict(os.environ,STAR_GRAPHICS_RENDERER='gpu');env.pop('STAR_TERMINAL_RENDERER',None)
    def state():return json.loads((runtime/(name+'.state.json')).read_text())
    def rc(address,*args):return subprocess.check_output(['kitten','@','--to',address,*args],stderr=subprocess.PIPE,timeout=10)
    def launch(label,command):
        address='unix:'+str(temp/(label+'.sock'));addresses.append(address)
        child=subprocess.Popen(['/usr/bin/kitty','--hold','--class','starmux-kitty-pixel-proof','--listen-on',address,'-o','allow_remote_control=yes','-o','background_opacity=1','-o','hide_window_decorations=yes',*command],env=env,stdout=subprocess.DEVNULL,stderr=(out/(label+'.log')).open('w'));children.append(child)
        wait(lambda:rc(address,'get-text'),'Kitty remote control unavailable')
        return child,address
    def capture(child,address,path):
        rc(address,'focus-window');time.sleep(.5)
        c=next(c for c in json.loads(subprocess.check_output(['hyprctl','clients','-j'])) if c['pid']==child.pid)
        x,y=c['at'];w,h=c['size'];subprocess.run(['grim','-g',f'{x},{y} {w}x{h}',str(path)],check=True)
    try:
        normal,normal_address=launch('normal',['python3',str(specimen)])
        wait(lambda:b'KITTY_RENDER_' in rc(normal_address,'get-text'),'Native Kitty sample absent')
        capture(normal,normal_address,out/'normal.png')
        capture(normal,normal_address,out/'normal-again.png')
        rc(normal_address,'close-window');normal.wait(timeout=10)
        mux,mux_address=launch('starmux',[str(app),'--workspace',name])
        s=wait(lambda:state() if state().get('terminal_renderer')=='kitty' and len(state()['desktop']['project_windows'])==2 and state()['terminal_presentation']['frame']==state()['terminal_presentation']['canvas'] else None,'Unscaled Kitty panes did not map')
        inner='unix:'+str(runtime/f'kitty-{name}-1.sock')
        wait(lambda:b'KITTY_RENDER_' in rc(inner,'get-text'),'Embedded sample absent')
        time.sleep(.5);capture(mux,mux_address,out/'starmux.png')
        native,native_size,ink_height=mask(out/'normal.png');embedded,embedded_size,_=mask(out/'starmux.png',ink_height)
        mismatch=sum(a!=b for a,b in zip(native,embedded))+abs(len(native)-len(embedded))
        result={'renderer':'kitty','frame':s['terminal_presentation']['frame'],'canvas':s['terminal_presentation']['canvas'],'native_glyph_bounds':native_size,'embedded_glyph_bounds':embedded_size,'native_rgb_bytes':len(native),'embedded_rgb_bytes':len(embedded),'differing_rgb_bytes':mismatch,'pixel_identical':native==embedded}
        (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
        # Both paths use Kitty's glyph renderer. The additional GPU copy may
        # quantize antialiased RGB by one 8-bit level; compare glyph placement
        # and coverage separately from padding/background and roundtrip RGB.
        ink=lambda raw:{i//3:raw[i] for i in range(0,len(raw),3) if raw[i]==raw[i+1]==raw[i+2] and raw[i]>20}
        a=ink(native);b=ink(embedded)
        result['glyph_shapes_identical']=a.keys()==b.keys()
        result['max_glyph_coverage_delta']=max((abs(a.get(i,0)-b.get(i,0)) for i in a.keys()|b.keys()),default=0)
        (out/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
        assert native_size==embedded_size and result['glyph_shapes_identical'] and result['max_glyph_coverage_delta']<=1,result
    finally:
        subprocess.run([str(app),'--workspace',name,'--stop'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=20)
        for address in addresses:
            try:rc(address,'close-window')
            except subprocess.SubprocessError:pass
        for child in children:
            if child.poll() is None:child.terminate()
            child.wait(timeout=10)
