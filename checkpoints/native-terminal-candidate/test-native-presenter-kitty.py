#!/usr/bin/env python3
"""Actual offscreen Kitty pixels; a private Xvfb and fixture terminal only."""
import base64,json,os,pathlib,select,shutil,socket,struct,subprocess,tempfile,time,zlib
root=pathlib.Path(__file__).resolve().parents[1]
xvfb=next(pathlib.Path('/nix/store').glob('*xorg*server*/bin/Xvfb'))
mesa=next(pathlib.Path('/nix/store').glob('*mesa-26.2.4/lib/dri/swrast_dri.so')).parents[1]
def png(width,height,data):
 def chunk(kind,content):return struct.pack('>I',len(content))+kind+content+struct.pack('>I',zlib.crc32(kind+content)&0xffffffff)
 raw=b''.join(b'\0'+data[y*width*4:(y+1)*width*4] for y in range(height))
 return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',width,height,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(raw))+chunk(b'IEND',b'')
def wait(fn,label,timeout=20):
 deadline=time.monotonic()+timeout
 while time.monotonic()<deadline:
  try:
   result=fn()
   if result:return result
  except (OSError,ValueError,KeyError,subprocess.SubprocessError):pass
  time.sleep(.05)
 raise AssertionError(label)
with tempfile.TemporaryDirectory(prefix='starflux-image-kitty-') as td:
 tmp=pathlib.Path(td);runtime=tmp/'runtime';runtime.mkdir(mode=0o700)
 rfd,wfd=os.pipe();log=(tmp/'log').open('w');xlog=(tmp/'xvfb.log').open('w');server=None;kitty=None;peer=None;reader=None
 try:
  server=subprocess.Popen([str(xvfb),'-displayfd',str(wfd),'-screen','0','1000x700x24','-nolisten','tcp','-ac','+extension','GLX','+render','-noreset'],pass_fds=[wfd],stdout=xlog,stderr=xlog,env={**os.environ,'LD_LIBRARY_PATH':str(mesa),'LIBGL_DRIVERS_PATH':str(mesa/'dri'),'LIBGL_ALWAYS_SOFTWARE':'1','GALLIUM_DRIVER':'llvmpipe','__GLX_VENDOR_LIBRARY_NAME':'mesa'});os.close(wfd)
  assert select.select([rfd],[],[],10)[0],'private Xvfb did not publish display'
  display=':'+os.read(rfd,32).decode().strip();os.close(rfd)
  env={k:v for k,v in os.environ.items() if k not in ('WAYLAND_DISPLAY','KITTY_WINDOW_ID','KITTY_PID','TMUX','TMUX_PANE','STY','NOTIFY_SOCKET')}
  env.update(DISPLAY=display,XDG_RUNTIME_DIR=str(runtime),XDG_CONFIG_HOME=str(tmp/'config'),LIBGL_ALWAYS_SOFTWARE='1')
  watcher=tmp/'metrics.py';watcher.write_text('import json\nfrom pathlib import Path\nfrom kitty.fast_data_types import cell_size_for_window\ndef on_set_user_var(boss,window,data):\n if data.get("key")=="starflux_pixel_proof":Path('+repr(str(tmp/'metrics.json'))+').write_text(json.dumps(dict(cell=cell_size_for_window(window.os_window_id),geometry=window.geometry._asdict())))\n')
  endpoint=tmp/'source.sock';listener=socket.socket(socket.AF_UNIX);listener.bind(str(endpoint));listener.listen();listener.settimeout(20)
  wrapper=tmp/'run';wrapper.write_text('#!/bin/bash\nsource '+str(root/'target/run-env.sh')+'\nexec '+str(root/'target/release/examples/graphics-next-presenter-proof')+' --text "$@"\n');wrapper.chmod(0o700)
  address='unix:'+str(tmp/'kitty.sock')
  kitty=subprocess.Popen(['/usr/bin/kitty','--config','NONE','--listen-on',address,'-o','linux_display_server=x11','-o','allow_remote_control=yes','-o','font_size=13','-o','window_padding_width=0','-o','background=#000000','-o','foreground=#ffffff','-o','confirm_os_window_close=0','-o','watcher='+str(watcher),str(wrapper),str(endpoint)],env=env,stdout=log,stderr=log)
  listener.settimeout(.2);deadline=time.monotonic()+20
  while peer is None:
   try:peer,_=listener.accept()
   except TimeoutError:
    assert kitty.poll() is None,'private Kitty exited: '+(tmp/'log').read_text()
    assert time.monotonic()<deadline,'private presenter did not connect: '+(tmp/'log').read_text()
  peer.settimeout(5);reader=peer.makefile('rb')
  metrics=wait(lambda:json.loads((tmp/'metrics.json').read_text()),'Kitty did not report fixture geometry')
  cw,ch=metrics['cell'];geometry=metrics['geometry'];width=cw*3;height=ch*2
  os_window=json.loads(subprocess.check_output(['/usr/bin/kitten','@','--to',address,'ls'],env=env,stderr=subprocess.PIPE,timeout=5))[0]
  window_id=str(os_window['platform_window_id']);x=geometry['left']+3*cw;y=geometry['top']+4*ch
  def pixels():
   shot=tmp/'screen.png'
   subprocess.run(['magick','import','-display',display,'-window',window_id,str(shot)],env=env,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,timeout=5)
   return subprocess.check_output(['magick',str(shot),'-crop',f'{width}x{height}+{x}+{y}','+repage','-depth','8','rgb:-'],env=env,stderr=subprocess.PIPE,timeout=5)
  def count(data,color):return sum(data[i:i+3]==bytes(color) for i in range(0,len(data),3))
  baseline=wait(lambda:(data if count(data:=pixels(),[255,255,255])>0 else None),'native text baseline missing')
  artifacts=root/'target/native-planes-kitty-artifacts';artifacts.mkdir(exist_ok=True)
  shutil.copyfile(tmp/'screen.png',artifacts/'baseline.png')
  def send(sequence,below,above):
   frame=dict(op='planes',view='private-kitty',generation=1,rect=[3,4,3,2],cell=[cw,ch],planes=dict(id=str(sequence),width=width,height=height,below=below,above=above))
   peer.sendall((json.dumps(dict(type='osc72',id=sequence,meta='starflux-image-planes-v1',payload=json.dumps(frame)),separators=(',',':'))+'\n').encode())
   assert json.loads(reader.readline())==dict(type='effect_ack',id=sequence)
  red=base64.b64encode(png(width,height,bytes([255,0,0,255])*(width*height))).decode();send(1,red,None)
  below=wait(lambda:(data if count(data:=pixels(),[255,0,0])>width*height//2 and count(data,[255,255,255])>0 else None),'below-text plane obscured native glyphs or failed to render')
  shutil.copyfile(tmp/'screen.png',artifacts/'below-text.png')
  rgba=bytearray([0,255,0,255]*(width*height));rgba[:12]=bytes([0,0,0,255,255,255,255,255,0,0,0,255])
  green=base64.b64encode(png(width,height,bytes(rgba))).decode();send(2,red,green)
  above=wait(lambda:(data if count(data:=pixels(),[0,255,0])==width*height-3 else None),'above-text plane did not hide native glyphs exactly')
  # llvmpipe/Kitty's linear texture sampling can round the adjacent green
  # contribution up by one 8-bit level at an otherwise black pixel. Require
  # exact one-pixel geometry/white and at most that measured edge error.
  assert above[0]==above[2]==above[6]==above[8]==0 and above[1]<=1 and above[7]<=1 and above[3:6]==bytes([255,255,255]),('one-pixel checker geometry/contrast changed',list(above[:12]))
  shutil.copyfile(tmp/'screen.png',artifacts/'above-text.png')
  peer.sendall((json.dumps(dict(type='osc72',id=3,meta='starflux-image-planes-v1',payload=json.dumps(dict(op='clear',view='private-kitty'))),separators=(',',':'))+'\n').encode());assert json.loads(reader.readline())==dict(type='effect_ack',id=3)
  restored=wait(lambda:(data if count(data:=pixels(),[255,0,0])==0 and count(data,[0,255,0])==0 and count(data,[255,255,255])>0 else None),'scoped deletion left an image placement')
  assert len(restored)==len(baseline) and max(abs(a-b) for a,b in zip(restored,baseline))<=1,'native text geometry/colour changed beyond GPU rounding after deletion'
  shutil.copyfile(tmp/'screen.png',artifacts/'restored.png')
  (artifacts/'pixels.json').write_text(json.dumps(dict(cell=[cw,ch],rect=[x,y,width,height],max_restored_channel_error=max(abs(a-b) for a,b in zip(restored,baseline)),checker_first_three=list(above[:9])),indent=2)+'\n')
  print(json.dumps(dict(result='PASS',private_xvfb=True,real_kitty=True,below_native_text=True,above_native_text=True,one_pixel_detail=True,gpu_edge_rounding_max=1,scoped_delete_restores_native_text=True,cell=[cw,ch],no_user_windows=True)))
 except Exception:
  log.flush();xlog.flush();print((tmp/'log').read_text()[-2500:]);print('X server:', '\n'.join(line for line in (tmp/'xvfb.log').read_text().splitlines() if any(word in line.lower() for word in ['glx','glamor','dri','mesa','error','failed'])));raise
 finally:
  if reader:reader.close()
  if peer:peer.close()
  if kitty and kitty.poll() is None:
   kitty.terminate()
   try:kitty.wait(timeout=5)
   except subprocess.TimeoutExpired:kitty.kill();kitty.wait(timeout=5)
  if server and server.poll() is None:server.terminate();server.wait(timeout=5)
