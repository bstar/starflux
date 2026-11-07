#!/usr/bin/env python3
"""Example STAR/FLUX version-1 finite action. No hooks or terminal input."""
import json
import subprocess
import sys

context = json.load(sys.stdin)
project = context.get("project")
if not project:
    sys.exit("Select a project before running this action.")
print("Project:", project["id"], flush=True)
result = subprocess.run(
    ["git", "--no-optional-locks", "-c", "core.hooksPath=/dev/null", "-c", "core.fsmonitor=false",
     "status", "--short", "--branch", "--untracked-files=no"],
    check=False,
)
sys.exit(result.returncode)
