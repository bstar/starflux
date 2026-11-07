"""STAR/FLUX display-side font controls; loaded alongside the STAR/KIT watcher.

Only installed monospace families and bounded numeric sizes are accepted.
Sizes are OS-window-local. Kitty font family changes are process-wide, in memory.
No commands, paths or configuration text from the remote host are executed.
"""
import base64
import json
import re

states = {}


def families():
    from kitty.fonts.common import all_fonts_map
    return sorted({d['family'] for ds in all_fonts_map(True)['family_map'].values()
                   for d in ds if isinstance(d.get('family'), str)}, key=str.casefold)


def current_family():
    from kitty.fast_data_types import current_fonts
    face = current_fonts().get('medium')
    if face is None:
        return 'Monospace'
    psname = face.postscript_name()
    # Use Kitty's platform font catalogue on Linux and macOS alike.
    from kitty.fonts.common import all_fonts_map
    for ds in all_fonts_map(True)['family_map'].values():
        for descriptor in ds:
            if descriptor.get('postscript_name') == psname:
                return descriptor['family']
    return psname


def reply(window, value):
    # Crossterm's escape parser is bounded to 8 KiB. Page large system font
    # catalogues rather than dropping a response on a font-rich workstation.
    entries = value.pop('families', [])
    chunks = [[]]
    for family in entries[:512]:
        if len(json.dumps(dict(value, families=chunks[-1] + [family]))) > 5200:
            chunks.append([])
        chunks[-1].append(family)
    for index, chunk in enumerate(chunks):
        part = dict(value, families=chunk, append=index > 0, complete=index == len(chunks) - 1)
        encoded = base64.b64encode(json.dumps(part, ensure_ascii=True).encode()).decode()
        window.write_to_child(f'\x1b]72;starflux-font-v1;{encoded}\x1b\\'.encode())


def on_set_user_var(boss, window, data):
    if data.get('key') != 'starflux_font':
        return
    try:
        raw = data.get('value', '')
        if len(raw) > 8192:
            return
        request = json.loads(raw)
        nonce, op = request.get('nonce', ''), request.get('op')
        if not isinstance(nonce, str) or not re.fullmatch('[0-9a-f]{32}', nonce):
            return
        from kitty.fast_data_types import os_window_font_size
        size = os_window_font_size(window.os_window_id)
        if op == 'probe':
            state = states.get(window.id)
            if state is None or state['nonce'] != nonce:
                states[window.id] = dict(nonce=nonce, initial=size)
        elif states.get(window.id, {}).get('nonce') != nonce:
            return
        elif op == 'size':
            target = request.get('size')
            delta = request.get('delta')
            if target is None and type(delta) is int and delta in (-1000, 1000):
                target = max(6000, min(32000, round(size * 1000) + delta))
            if type(target) is not int or (target != 0 and not 6000 <= target <= 32000):
                raise ValueError('Invalid font size')
            points = states[window.id]['initial'] if target == 0 else target / 1000
            boss._change_font_size({window.os_window_id: points})
        elif op == 'family':
            family = request.get('family')
            if not isinstance(family, str) or family not in families():
                raise ValueError('Font is not an installed monospace family')
            from kitty.fast_data_types import get_options
            from kitty.options.utils import parse_font_spec
            opts = get_options()
            sizes = {oid: os_window_font_size(oid) for oid in boss.os_window_map}
            # Kitty's font faces belong to the process, rather than an OS window.
            boss.apply_new_options(opts._replace(font_family=parse_font_spec(family),
                                                bold_font=parse_font_spec('auto'),
                                                italic_font=parse_font_spec('auto'),
                                                bold_italic_font=parse_font_spec('auto')))
            boss._change_font_size(sizes)
        else:
            raise ValueError('Unknown font operation')
        reply(window, dict(family=current_family(), families=families(),
                           millipoints=round(os_window_font_size(window.os_window_id) * 1000),
                           scope='kitty-process', nonce=nonce))
    except Exception as error:
        reply(window, dict(error=str(error)))


def on_close(boss, window, data):
    states.pop(window.id, None)


def on_load(boss, data):
    """Kitty copies watcher callbacks at window creation. Upgrade existing windows."""
    from pathlib import Path
    managed = Path(__file__).resolve().parent

    def owned(callback):
        path = getattr(callback, '__globals__', {}).get('__file__', '')
        path = Path(path)
        return path.resolve().parent == managed and path.name.startswith('starflux_fonts')

    for window in tuple(boss.window_id_map.values()):
        for previous in window.watchers.on_set_user_var:
            if owned(previous):
                saved = getattr(previous, '__globals__', {}).get('states')
                if isinstance(saved, dict):
                    states.update(saved)
        for name, callback in [('on_set_user_var', on_set_user_var), ('on_close', on_close)]:
            hooks = getattr(window.watchers, name)
            hooks[:] = [hook for hook in hooks if not owned(hook)]
            hooks.append(callback)
