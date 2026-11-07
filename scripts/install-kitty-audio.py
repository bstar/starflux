#!/usr/bin/env python3
"""Install a fixed, local STAR/KIT audio frontend and Kitty watcher, with backups."""
import argparse,datetime,hashlib,os,shlex,shutil,tempfile
from pathlib import Path
from kitty_font_bridge import augment
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--starflux-client',type=Path,help='Install a dedicated STAR/FLUX helper, preserving the shared STAR/KIT helper')
p.add_argument('--client',type=Path,help='Built local client; omit to preserve the installed helper')
p.add_argument('--watcher',type=Path,required=True,help='STAR/KIT integrations/kitty/star_kit.py')
p.add_argument('--font-watcher',type=Path,default=Path(__file__).resolve().parents[1]/'terminal-client/kitty/starflux_fonts.py',help='STAR/FLUX local font controls watcher')
a=p.parse_args();client=a.client.expanduser().resolve() if a.client else None;watcher=a.watcher.expanduser().resolve()
starflux_client=a.starflux_client.expanduser().resolve() if a.starflux_client else None
if starflux_client is not None and (not starflux_client.is_file() or not os.access(starflux_client,os.X_OK)):p.error('Build the local STAR/FLUX client first')
if client is not None and (not client.is_file() or not os.access(client,os.X_OK)):p.error('Build the local audio-enabled client first')
if not watcher.is_file():p.error('The STAR/KIT watcher file is missing')
compile(watcher.read_text(),str(watcher),'exec')
font_watcher=a.font_watcher.expanduser().resolve()
if not font_watcher.is_file():p.error('The STAR/FLUX font watcher is missing')
compile(font_watcher.read_text(),str(font_watcher),'exec')
font_data=font_watcher.read_bytes()
root=Path.home();config=Path(os.environ.get('KITTY_CONFIG_DIRECTORY',root/'.config/kitty'))/'kitty.conf'
watcher_data=augment(watcher.read_text()).encode()
watcher_dir=root/'.local/lib/star-kit/kitty'
# Kitty caches modules by pathname for the lifetime of the process.
local_watcher=watcher_dir/('star_kit-'+hashlib.sha256(watcher_data).hexdigest()[:16]+'.py')
local_fonts=watcher_dir/('starflux_fonts-'+hashlib.sha256(font_data).hexdigest()[:16]+'.py')
helper=root/'.local/bin/star-kit-terminal'
flux_helper=root/'.local/bin/starflux-terminal-client'
stamp=datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')
def put(path,data,mode):
 path.parent.mkdir(parents=True,exist_ok=True)
 if path.exists() and path.read_bytes()==data:return
 if path.exists():shutil.copy2(path,path.with_name(path.name+'.bak-starflux-'+stamp))
 with tempfile.NamedTemporaryFile(dir=path.parent,delete=False)as f:f.write(data);temp=Path(f.name)
 temp.chmod(mode);temp.replace(path)
put(local_watcher,watcher_data,0o600)
put(local_fonts,font_data,0o600)
if client is not None:
 put(helper,('#!/bin/sh\nexec '+shlex.quote(str(client))+' "$@"\n').encode(),0o700)
elif not helper.is_file() or not os.access(helper,os.X_OK):
 p.error('No installed client; pass --client with a built executable')
if starflux_client is not None:
 put(flux_helper,('#!/bin/sh\nexec '+shlex.quote(str(starflux_client))+' "$@"\n').encode(),0o700)
text=config.read_text()if config.exists()else''
line='watcher '+str(local_watcher)
lines=[]
for existing in text.splitlines():
 parts=existing.strip().split(maxsplit=1)
 if len(parts)==2 and parts[0]=='watcher':
  registered=Path(parts[1]).expanduser()
  if registered.parent==watcher_dir and registered.name.startswith(('star_kit','starflux_fonts-')):
   continue
 lines.append(existing)
updated='\n'.join(lines).rstrip()+'\n'+line+'\nwatcher '+str(local_fonts)+'\n'
put(config,updated.encode(),config.stat().st_mode&0o777 if config.exists() else 0o600)
print('Installed local audio helper:',helper)
print('Installed Kitty watcher:',local_watcher)
print('Installed font controls watcher:',local_fonts)
if starflux_client is not None:print('Installed dedicated STAR/FLUX helper:',flux_helper)
print('Config:',config)
print('Reload Kitty configuration, then Refresh UI or reconnect STAR/FLUX. Existing terminal sessions are retained.')
