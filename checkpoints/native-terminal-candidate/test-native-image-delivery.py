#!/usr/bin/env python3
"""Keep media/input flowing while a private native presenter withholds ACK."""
import json,os,pathlib,socket,subprocess,tempfile
root=pathlib.Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='starflux-image-delivery-') as td:
 tmp=pathlib.Path(td)
 source=socket.socket(socket.AF_UNIX);source.bind(str(tmp/'source'));source.listen();source.settimeout(5)
 output=socket.socket(socket.AF_UNIX);output.bind(str(tmp/'output'));output.listen();output.settimeout(5)
 wrapper=tmp/'run';wrapper.write_text('#!/bin/bash\nsource '+str(root/'target/run-env.sh')+'\nexec '+str(root/'target/release/examples/graphics-next-presenter-proof')+' --relay "$@"\n');wrapper.chmod(0o700)
 process=subprocess.Popen([str(wrapper),str(tmp/'source')],env={**os.environ,'STARFLUX_FONT_OUTPUT':str(tmp/'output')},stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
 peer=None;first=None;second=None
 try:
  peer,_=source.accept();peer.settimeout(1);reader=peer.makefile('rb')
  def frame(n):return dict(op='planes',view='private-view',generation=1,rect=[1,1,1,1],cell=[2,2],planes=dict(id=str(n),width=2,height=2,below=None,above=None))
  def send(n):
   peer.sendall((json.dumps(dict(type='osc72',id=n,meta='starflux-image-planes-v1',payload=json.dumps(frame(n))),separators=(',',':'))+'\n').encode())
   assert json.loads(reader.readline())==dict(type='effect_ack',id=n)
  send(1);first,_=output.accept();first.settimeout(2)
  assert json.loads(first.makefile('rb').readline())['planes']['id']=='1'
  # The presenter deliberately keeps its first image in flight.
  media=b'{"type":"media","message":{"kind":"audio_chunk","session":1,"epoch":1,"samples":[0,0]}}\n'
  peer.sendall(media);assert process.stdout.readline()==media,'image wait blocked media forwarding'
  process.stdin.write(b'{"type":"presented","revision":1,"generation":1}\n');process.stdin.flush()
  assert json.loads(reader.readline())==dict(type='presented',revision=1,generation=1),'image wait blocked input forwarding'
  key=dict(type='input',id=1,revision=1,generation=1,input=dict(kind='key',code='char:x',modifiers=0))
  process.stdin.write((json.dumps(key)+'\n').encode());process.stdin.flush();assert json.loads(reader.readline())==key,'image wait blocked keyboard forwarding'
  for n in range(2,21):send(n)
  first.sendall(b'\x01');first.close();first=None
  while second is None:
   connection,_=output.accept();connection.settimeout(2);request=json.loads(connection.makefile('rb').readline())
   if request.get('op')=='probe':connection.close();continue
   assert request['planes']['id']=='20','obsolete image update was not replaced'
   second=connection
  second.sendall(b'\x01');second.close();second=None
  reader.close();peer.close();peer=None;process.wait(timeout=5)
  assert process.returncode==0,process.stderr.read().decode()
  print(json.dumps(dict(result='PASS',media_during_image_wait=True,input_during_image_wait=True,latest_per_view=True,no_obsolete_replay=True,private_processes_only=True)))
 finally:
  for connection in (first,second,peer):
   if connection:connection.close()
  if process.poll() is None:process.kill();process.wait(timeout=5)
  process.stdin.close();source.close();output.close()
