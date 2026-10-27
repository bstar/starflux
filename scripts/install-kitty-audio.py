#!/usr/bin/env python3
"""Install a fixed, local STAR/KIT audio frontend and Kitty watcher, with backups."""
import argparse,datetime,hashlib,os,shlex,shutil,tempfile
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--client',type=Path,help='Built local client; omit to preserve the installed helper')
p.add_argument('--watcher',type=Path,required=True,help='STAR/KIT integrations/kitty/star_kit.py')
a=p.parse_args();client=a.client.expanduser().resolve() if a.client else None;watcher=a.watcher.expanduser().resolve()
if client is not None and (not client.is_file() or not os.access(client,os.X_OK)):p.error('Build the local audio-enabled client first')
if not watcher.is_file():p.error('The STAR/KIT watcher file is missing')
compile(watcher.read_text(),str(watcher),'exec')
root=Path.home();config=Path(os.environ.get('KITTY_CONFIG_DIRECTORY',root/'.config/kitty'))/'kitty.conf'
watcher_data=watcher.read_bytes()
watcher_dir=root/'.local/lib/star-kit/kitty'
# Kitty caches modules by pathname for the lifetime of the process.
local_watcher=watcher_dir/('star_kit-'+hashlib.sha256(watcher_data).hexdigest()[:16]+'.py')
helper=root/'.local/bin/star-kit-terminal'
stamp=datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')
def put(path,data,mode):
 path.parent.mkdir(parents=True,exist_ok=True)
 if path.exists() and path.read_bytes()==data:return
 if path.exists():shutil.copy2(path,path.with_name(path.name+'.bak-starmux-'+stamp))
 with tempfile.NamedTemporaryFile(dir=path.parent,delete=False)as f:f.write(data);temp=Path(f.name)
 temp.chmod(mode);temp.replace(path)
put(local_watcher,watcher_data,0o600)
if client is not None:
 put(helper,('#!/bin/sh\nexec '+shlex.quote(str(client))+' "$@"\n').encode(),0o700)
elif not helper.is_file() or not os.access(helper,os.X_OK):
 p.error('No installed client; pass --client with a built executable')
text=config.read_text()if config.exists()else''
line='watcher '+str(local_watcher)
lines=[]
for existing in text.splitlines():
 parts=existing.strip().split(maxsplit=1)
 if len(parts)==2 and parts[0]=='watcher':
  registered=Path(parts[1]).expanduser()
  if registered.parent==watcher_dir and registered.name.startswith('star_kit'):
   continue
 lines.append(existing)
updated='\n'.join(lines).rstrip()+'\n'+line+'\n'
put(config,updated.encode(),config.stat().st_mode&0o777 if config.exists() else 0o600)
print('Installed local audio helper:',helper)
print('Installed Kitty watcher:',local_watcher)
print('Config:',config)
print('Reload Kitty configuration, then Refresh UI or reconnect STAR/MUX. Existing terminal sessions are retained.')
