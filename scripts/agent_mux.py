"""The external agent multiplexer STAR/FLUX can import sessions from.

Its program is local configuration, never part of this repository:
STARFLUX_AGENT_MUX, or the first line of $XDG_CONFIG_HOME/starflux/agent-mux.
When neither is set, PROGRAM is a placeholder that is not installed, so tests
take their "multiplexer unavailable" paths. Importing this module exports the
configured program in STARFLUX_AGENT_MUX for child processes.
"""
import os
from pathlib import Path

SOURCE = 'agent-mux'          # source identity in references, health and the API
KEY = 'agent_mux'             # project-file field holding an import binding
COLLECTION = 'starflux'       # imported collections are COLLECTION-<session>
DISPLAY = 'Multiplexer'


def _executable():
    value = os.environ.get('STARFLUX_AGENT_MUX', '').strip()
    if value:
        return value
    root = Path(os.environ.get('XDG_CONFIG_HOME') or Path.home() / '.config')
    try:
        lines = (root / 'starflux' / 'agent-mux').read_text().splitlines()
    except OSError:
        return None
    return next((line.strip() for line in lines if line.strip() and not line.strip().startswith('#')), None)


EXECUTABLE = _executable()
CONFIGURED = EXECUTABLE is not None
PROGRAM = Path(EXECUTABLE).name if CONFIGURED else 'agent-mux-unconfigured'
ENV = PROGRAM.upper().replace('-', '_')
if CONFIGURED:
    os.environ['STARFLUX_AGENT_MUX'] = EXECUTABLE
MUX = EXECUTABLE or PROGRAM   # what tests run
