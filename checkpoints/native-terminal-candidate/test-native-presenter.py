#!/usr/bin/env python3
"""Exercise the actual local relay/output loop using captured terminal bytes."""
import base64,json,os,pathlib,re,signal,socket,struct,subprocess,tempfile,threading,time,zlib
root=pathlib.Path(__file__).resolve().parents[1]
def png(rgba):
 def chunk(kind,data):return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data)&0xffffffff)
 return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',2,2,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(b'\0'+rgba[:8]+b'\0'+rgba[8:]))+chunk(b'IEND',b'')
with tempfile.TemporaryDirectory(prefix='starflux-native-presenter-') as td:
 path=pathlib.Path(td);endpoint=path/'private.sock'
 listener=socket.socket(socket.AF_UNIX);listener.bind(str(endpoint));listener.listen();listener.settimeout(5)
 wrapper=path/'run';wrapper.write_text('#!/bin/bash\nsource '+str(root/'target/run-env.sh')+'\nexec '+str(root/'target/release/examples/graphics-next-presenter-proof')+' "$@"\n');wrapper.chmod(0o700)
 process=subprocess.Popen([str(wrapper),str(endpoint)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
 try:
  peer,_=listener.accept();peer.settimeout(5);reader=peer.makefile('rb');captured=[]
  def drain():
   while data:=os.read(process.stdout.fileno(),8192):captured.append(data)
  thread=threading.Thread(target=drain);thread.start()
  low=base64.b64encode(png(bytes([255,0,0,255]*4))).decode();high=base64.b64encode(png(bytes([0,0,255,128]*4))).decode()
  frame=dict(op='planes',view='private-view',generation=1,rect=[3,4,1,1],cell=[2,2],planes=dict(id='fixture',width=2,height=2,below=low,above=high))
  def rendered(images,deletes):
   deadline=time.monotonic()+5
   while time.monotonic()<deadline:
    packets=re.findall(rb'\x1b_G(.*?)\x1b\\',b''.join(captured),re.S)
    if sum(p.startswith(b'a=T,') for p in packets)>=images and sum(p.startswith(b'a=d,') for p in packets)>=deletes:return
    time.sleep(.01)
   raise AssertionError('local image output did not drain')
  def send(sequence,command):
   peer.sendall((json.dumps(dict(type='osc72',id=sequence,meta='starflux-image-planes-v1',payload=json.dumps(command)),separators=(',',':'))+'\n').encode())
   assert json.loads(reader.readline())==dict(type='effect_ack',id=sequence)
  # Pause only this fixture's terminal-output process. The separate relay
  # must keep forwarding input while it awaits the image application ACK.
  fd=os.pidfd_open(process.pid)
  try:
   signal.pidfd_send_signal(fd,signal.SIGSTOP)
   deadline=time.monotonic()+2
   while pathlib.Path('/proc',str(process.pid),'stat').read_text().rsplit(')',1)[1].split()[0]!='T':
    assert time.monotonic()<deadline;time.sleep(.005)
   peer.sendall((json.dumps(dict(type='osc72',id=1,meta='starflux-image-planes-v1',payload=json.dumps(frame)),separators=(',',':'))+'\n').encode())
   process.stdin.write(b'{"type":"presented","revision":1,"generation":1}\n');process.stdin.flush()
   peer.settimeout(1)
   replies=[json.loads(reader.readline()),json.loads(reader.readline())]
   assert dict(type='presented',revision=1,generation=1) in replies and dict(type='effect_ack',id=1) in replies,'image delivery blocked the input/probe path'
  finally:
   signal.pidfd_send_signal(fd,signal.SIGCONT);os.close(fd);peer.settimeout(5)
  rendered(2,0)
  send(2,frame)
  frame['rect']=[5,6,1,1];send(3,frame);rendered(4,2)
  send(4,dict(op='clear',view='private-view'));rendered(4,4);send(5,frame);rendered(6,4)
  peer.sendall(b'{"type":"pong"}\n')
  reader.close();peer.close();process.wait(timeout=8);thread.join(timeout=2)
  assert process.returncode==0,process.stderr.read().decode()
  output=b''.join(captured)
  packets=re.findall(rb'\x1b_G(.*?)\x1b\\',output,re.S)
  images=[packet for packet in packets if packet.startswith(b'a=T,')];deletes=[packet for packet in packets if packet.startswith(b'a=d,')]
  assert len(images)==6 and len(deletes)==6,(len(images),len(deletes))
  assert sum(b'z=-1,' in packet for packet in images)==3 and sum(b'z=1,' in packet for packet in images)==3
  for packet in images:
   header,payload=packet.split(b';',1);assert base64.b64decode(payload) in (base64.b64decode(low),base64.b64decode(high));assert b'q=2' in header and b'C=1' in header
  assert all(b'd=I,' in packet and b'd=A' not in packet for packet in deletes)
  assert b'\x1b[5;4H' in output and b'\x1b[7;6H' in output and b'{"type":"pong"}\n' in output
  print(json.dumps(dict(result='PASS',actual_local_relay=True,captured_native_images=True,lossless_png_bytes=True,z_order=True,replacement=True,unchanged_cached=True,scoped_delete=True,exit_cleanup=True,protocol_passthrough=True,input_during_image_wait=True,no_windows=True)))
 finally:
  if process.poll() is None:process.kill();process.wait(timeout=5)
  if process.stdin:process.stdin.close()
  listener.close()
