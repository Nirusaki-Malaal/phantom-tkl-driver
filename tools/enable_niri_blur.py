#!/usr/bin/env python3
"""Repair Phantom Studio's managed niri window rule."""
import os
from pathlib import Path
from install import configure_niri

if __name__ == "__main__":
    configure_niri(Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config")))
