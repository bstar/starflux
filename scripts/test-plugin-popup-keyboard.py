"""Popup-only configurable keyboard controls, after the core popup fixture."""
def controls():return (popup() or {}).get('controls_focused',False)
def popup_key(code,modifiers=0):
    if args.cells:raw(code,modifiers)
    else:
        for phase in ['press','release']:
            a.input(dict(kind='embedded',canvas='plugin-popup',event=dict(timestamp_ns=time.monotonic_ns(),sequence=a.serial+1,kind=dict(kind='key',code=code,modifiers=modifiers,phase=phase))))
count=len(calls());identifier=opening();call=mark_ready(identifier,count+1)
before=received();a.input(dict(kind='notice',message='terminal-repaint-key'))
wait(lambda:b'\x0c' in received()[len(before):],'host repaint key did not reach popup')
before=received();popup_key('f:6',1)
wait(controls,'popup close controls were not reachable from the keyboard')
assert received()==before,'popup control shortcut leaked into the program'
popup_key('char:b',2);popup_key('tab');a.paste('MUST_NOT_REACH_POPUP')
a.input(dict(kind='notice',message='terminal-repaint-key'))
a.resize(70,24,700,480)
wait(lambda:controls() and pending_frame(identifier)['rect']==popup()['terminal_rect'],'resize lost popup close control focus')
assert received()==before,'header navigation leaked into the program'
popup_key('escape');wait(lambda:popup() and not controls(),'Escape did not return input to the popup')
assert received()==before,'returning from controls sent Escape to the program'
popup_key('f:6');wait(lambda:b'\x1b[17~' in received()[len(before):],'ordinary F6 was stolen from popup')
before=received();popup_key('f:6',1);wait(controls,'popup controls could not be reopened')
popup_key('enter');wait(lambda:not popup() and group_stopped(call),'keyboard close left the popup or child alive')
assert received()==before,'keyboard close sent Enter to the program'
a.resize(100,35,1000,700);check_membership()
# A remapped popup chord changes only popup controls. It is not a global direct
# binding, and disabling it restores literal Shift+F6 inside the program.
settings=tmp/'config/starflux/keys.json';settings.parent.mkdir(parents=True,exist_ok=True)
settings.write_text(json.dumps(dict(popup_controls='Alt+F8')))
a.action('keyboard:reload-keys')
wait(lambda:a.snapshot().get('popup_controls_shortcut')=='Alt+F8','popup shortcut settings did not reload')
identifier=opening();call=mark_ready(identifier,count+2);before=received()
popup_key('f:6',1);wait(lambda:b'\x1b[17;2~' in received()[len(before):],'remapped shortcut still stole Shift+F6')
before=received();popup_key('f:8',4);wait(controls,'remapped popup controls did not focus')
assert received()==before,'remapped control chord leaked into popup'
popup_key('enter');wait(lambda:not popup() and group_stopped(call),'remapped keyboard close failed')
settings.write_text(json.dumps(dict(popup_controls=None)));a.action('keyboard:reload-keys')
wait(lambda:a.snapshot().get('popup_controls_shortcut') is None,'popup shortcut disable did not reload')
identifier=opening();call=mark_ready(identifier,count+3);before=received()
popup_key('f:6',1);wait(lambda:b'\x1b[17;2~' in received()[len(before):],'disabled shortcut did not pass through')
assert not controls(),'disabled shortcut opened popup controls'
close(identifier);wait(lambda:group_stopped(call),'API close after disabling shortcut failed')
settings.unlink();a.action('keyboard:reload-keys');check_membership()
wait(lambda:a.snapshot().get('popup_controls_shortcut')=='Shift+F6','default popup shortcut did not reload')
assert alive(original_pid) and alive(source_pid),'popup keyboard controls changed another process'
print(json.dumps(dict(result='PASS',ssh=args.ssh,cells=args.cells,popup_keyboard_controls=True,escape_returns_to_program=True,enter_closes_owned_group=True,controls_do_not_leak_keys=True,ordinary_f6_passthrough=True,resize_preserves_focus=True,configurable_shortcut=True,disabled_passthrough=True,other_processes_preserved=True)))
