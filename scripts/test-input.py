#!/usr/bin/env python3
"""Verify input through a running disposable STAR/FLUX Kitty frontend."""
import argparse,pathlib,subprocess,time
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--to',required=True);p.add_argument('--workspace',required=True);p.add_argument('--output',default='target/input-proof.txt');a=p.parse_args()
app=pathlib.Path(__file__).resolve().parents[1]/'scripts/run-local.sh'
def rc(*args,data=None):return subprocess.check_output(['kitten','@','--to',a.to,*args],input=data,stderr=subprocess.PIPE)
def screen():return subprocess.check_output([str(app),'--workspace',a.workspace,'--capture','--pane','1'],text=True)
def until(marker):
    for _ in range(60):
        if marker in screen():return
        time.sleep(.1)
    raise AssertionError('Terminal did not display '+repr(marker))
def type_keys(text):
    # send-key supplies real press/release phases, unlike raw legacy bytes.
    for char in text:
        key='space' if char==' ' else 'shift+minus' if char=='_' else 'shift+'+char.lower() if char.isupper() else char
        rc('send-key',key)
rc('send-key','ctrl+escape')
rc('send-text','--stdin',data=b'\x1b[<0;350;220M\x1b[<0;350;220m');time.sleep(.2)
rc('send-key','ctrl+c');type_keys('echo STARFLUX_KEYS_OK');rc('send-key','enter');until('\nSTARFLUX_KEYS_OK\n')
rc('send-text','--stdin','--bracketed-paste','enable',data="printf 'STARFLUX_λ_PASTE_OK\\n'".encode());time.sleep(.2)
assert '\nSTARFLUX_λ_PASTE_OK\n' not in screen(),'Paste submitted automatically'
rc('send-key','enter');until('\nSTARFLUX_λ_PASTE_OK\n')
type_keys('sleep 30');rc('send-key','enter');time.sleep(.2);rc('send-key','ctrl+c');time.sleep(.1)
type_keys('echo STARFLUX_INTERRUPT_OK');rc('send-key','enter');until('\nSTARFLUX_INTERRUPT_OK\n')
for size in [14,12]:rc('set-font-size',str(size));time.sleep(.4)
assert '\nSTARFLUX_INTERRUPT_OK\n' in screen()
pathlib.Path(a.output).write_text(screen())
rc('send-key','ctrl+escape')
print('PASS: pixel click focus, press/release typing, Unicode bracketed paste without submission, Enter, Ctrl+C and font resize in real embedded Kitty')
