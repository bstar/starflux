"""The external agent multiplexer STAR/FLUX imports sessions from.

Its name is defined once, as `PROGRAM` in src/agent_mux.rs. STAR/FLUX passes it
to embedded helpers in STARFLUX_AGENT_MUX_PROGRAM; scripts run from a checkout
read it from the source.
"""
import os
import re
from pathlib import Path


def _program():
    if os.environ.get('STARFLUX_AGENT_MUX_PROGRAM'):
        return os.environ['STARFLUX_AGENT_MUX_PROGRAM']
    source = (Path(__file__).resolve().parents[1] / 'src' / 'agent_mux.rs').read_text()
    return re.search(r'pub const PROGRAM: &str = "([^"]+)";', source).group(1)


PROGRAM = _program()
ENV = PROGRAM.upper()
DISPLAY = PROGRAM[:1].upper() + PROGRAM[1:]
