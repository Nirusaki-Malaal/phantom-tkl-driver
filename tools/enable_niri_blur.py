#!/usr/bin/env python3
"""Install the app-scoped native blur rule, validating before replacing config."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

root = Path(__file__).resolve().parents[1]
config = Path(os.environ.get("NIRI_CONFIG", str(
    Path(os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config"))) / "niri/config.kdl"
))).resolve()
original = config.read_text()
marker = "// Phantom Studio only. Requires niri 26.04 or newer."
if marker in original:
    print("Phantom Studio blur rule is already installed.")
    raise SystemExit(0)
candidate = original.rstrip() + "\n\n" + (root / "packaging/niri-blur.kdl").read_text()
with tempfile.NamedTemporaryFile(mode="w", dir=config.parent, suffix=".kdl", delete=False) as f:
    f.write(candidate)
    temp = Path(f.name)
try:
    subprocess.run(["niri", "validate", "-c", str(temp)], check=True)
    backup = config.with_name(f"config.kdl.phantom-backup-{time.time_ns()}")
    shutil.copy2(config, backup)
    # Preserve file permissions; unrelated settings remain byte-for-byte intact.
    shutil.copymode(config, temp)
    os.replace(temp, config)
    print(f"Native blur enabled for Phantom Studio. Backup: {backup}")
finally:
    temp.unlink(missing_ok=True)
