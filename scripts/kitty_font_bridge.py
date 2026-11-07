"""Add a fixed STAR/FLUX helper choice to a compatible STAR/KIT watcher.

Other applications retain the existing shared helper. The host can select this
fixed application ID, never a local executable path or command.
"""
def augment(source):
    anchor="CLIENT = Path.home() / '.local/bin/star-kit-terminal'"
    if source.count(anchor)!=1 or source.count('[str(CLIENT), path]')!=1:
        raise ValueError('Unsupported STAR/KIT watcher; update the bridge adapter')
    source=source.replace(anchor,anchor+'''
STARFLUX_CLIENT = Path.home() / '.local/bin/starflux-terminal-client'
starflux_probes = {}


def client_for(window, nonce):
    if (window.id, nonce) in starflux_probes and STARFLUX_CLIENT.is_file() and os.access(STARFLUX_CLIENT, os.X_OK):
        return STARFLUX_CLIENT
    return CLIENT
''')
    source=source.replace('[str(CLIENT), path]','[str(client_for(window, nonce)), path]')
    source=source.replace("        if op == 'probe':\n",'''        if op == 'probe':
            if window.user_vars.pop('starflux_client', None) == '1':
                if len(starflux_probes) >= 512:
                    starflux_probes.pop(next(iter(starflux_probes)))
                starflux_probes[(window.id, nonce)] = True
''')
    source=source.replace('if CLIENT.is_file() and os.access(CLIENT, os.X_OK):', 'if client_for(window, nonce).is_file() and os.access(client_for(window, nonce), os.X_OK):')
    source=source.replace('        self.closed = True\n','        self.closed = True\n        starflux_probes.pop((self.window.id, self.nonce), None)\n')
    source=source.replace('\n\ndef on_set_user_var(boss, window, data):', """
    # Kitty's global watcher options reload lazily; windows retain callback
    # copies. Rebind managed bridge hooks while keeping live Session objects.
    for existing_window in tuple(boss.window_id_map.values()):
        for name, callback in [('on_set_user_var', on_set_user_var), ('on_close', on_close)]:
            hooks = getattr(existing_window.watchers, name)
            def owned(hook):
                file = Path(getattr(hook, '__globals__', {}).get('__file__', ''))
                return file.resolve().parent == managed and file.name.startswith('star_kit')
            hooks[:] = [hook for hook in hooks if not owned(hook)]
            hooks.append(callback)


def on_set_user_var(boss, window, data):""")
    compile(source,'STAR/FLUX Kitty bridge','exec')
    return source
