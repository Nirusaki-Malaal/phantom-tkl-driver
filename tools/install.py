#!/usr/bin/env python3
"""Build, install, repair or remove Phantom Studio using standard Linux tools."""
import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
APP_ID = "io.github.phantom.Studio"
START = "// BEGIN Phantom Studio"
END = "// END Phantom Studio"
OLD = "// Phantom Studio only. Requires niri 26.04 or newer."


def run(args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, **kwargs)


def atomic_write(path, text, mode=0o644):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as handle:
        temp = Path(handle.name)
        handle.write(text.encode())
    try:
        temp.chmod(mode)
        os.replace(temp, path)
    finally:
        temp.unlink(missing_ok=True)


def desktop_entry(binary):
    value = str(binary)
    if any(c in value for c in "\n\r"):
        raise ValueError("Installation paths must not contain newlines")
    for char in ('\\', '"', '`', '$'):
        value = value.replace(char, '\\' + char)
    value = value.replace('\\', '\\\\').replace('%', '%%')
    return f'''[Desktop Entry]
Type=Application
Name=Phantom Studio
Comment=Lighting, keys and macros for the Phantom TKL keyboard
Exec="{value}"
TryExec={binary}
Icon={APP_ID}
Terminal=false
Categories=Settings;HardwareSettings;
Keywords=keyboard;RGB;Phantom;TKL;macro;
StartupNotify=true
StartupWMClass={APP_ID}
'''


def replace_managed_rule(original, rule):
    block = START + "\n" + rule.rstrip() + "\n" + END
    if START in original and END in original:
        a = original.index(START)
        b = original.index(END, a) + len(END)
        return original[:a] + block + original[b:]
    if OLD in original:
        a = original.index(OLD)
        start = original.index('{', a)
        depth = 1
        b = start + 1
        while b < len(original) and depth:
            depth += (original[b] == '{') - (original[b] == '}')
            b += 1
        if depth:
            raise RuntimeError("Existing Phantom desktop rule is incomplete; config was not changed")
        return original[:a] + block + original[b:]
    return original.rstrip() + "\n\n" + block + "\n"


def configure_niri(config_home):
    if not shutil.which("niri"):
        return
    config = Path(os.environ.get("NIRI_CONFIG", config_home / "niri/config.kdl")).resolve()
    if not config.exists():
        return
    rule = (ROOT / "packaging/niri-blur.kdl").read_text()
    version = run(["niri", "--version"], capture_output=True, text=True).stdout
    match = re.search(r"(\d+)\.(\d+)", version)
    if match and tuple(map(int, match.groups())) < (26, 4):
        rule = re.sub(r"\s*background-effect\s*\{[^}]*\}", "", rule)
    original = config.read_text()
    candidate = replace_managed_rule(original, rule)
    if candidate == original:
        return
    with tempfile.NamedTemporaryFile(mode="w", dir=config.parent, suffix=".kdl", delete=False) as f:
        f.write(candidate)
        temp = Path(f.name)
    try:
        run(["niri", "validate", "-c", temp])
        backup = config.with_name(f"config.kdl.phantom-backup-{time.time_ns()}")
        shutil.copy2(config, backup)
        shutil.copymode(config, temp)
        os.replace(temp, config)
        print(f"Floating window and blur configured. Backup: {backup}")
    finally:
        temp.unlink(missing_ok=True)


def install_desktop(prefix, data_home, config_home, configure=True):
    binary = (prefix / "bin/phantom-studio").resolve()
    applications = data_home / "applications"
    entry = desktop_entry(binary)
    target = applications / f"{APP_ID}.desktop"
    atomic_write(target, entry)
    legacy = applications / "phantom-tkl.desktop"
    if legacy.exists() and any(s in legacy.read_text() for s in ("Phantom", "phantom")):
        backup = config_home / "phantom-studio/backups" / f"legacy-launcher-{time.time_ns()}.desktop"
        backup.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(legacy, backup)
        atomic_write(legacy, entry + "NoDisplay=true\n")
        print("Repaired the old Phantom shortcut; retained it as a hidden alias for pinned menu entries.")
    icon = data_home / "icons/hicolor/scalable/apps" / f"{APP_ID}.svg"
    icon.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / "packaging" / f"{APP_ID}.svg", icon)
    if shutil.which("desktop-file-validate"):
        run(["desktop-file-validate", target])
    if shutil.which("update-desktop-database"):
        run(["update-desktop-database", applications])
    if configure:
        configure_niri(config_home)


def dependencies(allow_install, assume_yes):
    ready = shutil.which("cc") and shutil.which("pkg-config") and subprocess.run(
        ["pkg-config", "--atleast-version=4.12", "gtk4"], capture_output=True).returncode == 0
    if not ready:
        if not allow_install:
            raise RuntimeError("Install GTK 4.12+ development files, a C compiler and pkg-config, then retry")
        managers = [
            ("apt-get", ["install", "-y", "libgtk-4-dev", "build-essential", "pkg-config", "curl", "desktop-file-utils"]),
            ("pacman", ["-S", "--needed", "--noconfirm", "gtk4", "base-devel", "pkgconf", "curl", "desktop-file-utils"]),
            ("dnf", ["install", "-y", "gtk4-devel", "gcc", "pkgconf-pkg-config", "curl", "desktop-file-utils"]),
            ("zypper", ["--non-interactive", "install", "gtk4-devel", "gcc", "pkg-config", "curl", "desktop-file-utils"]),
        ]
        selected = next(((m, args) for m, args in managers if shutil.which(m)), None)
        if selected is None:
            raise RuntimeError("Unsupported package manager. Install GTK 4.12+ development files and use --no-deps")
        manager, args = selected
        print("Required packages:", " ".join([manager] + args))
        if not assume_yes and input("Install these packages? [Y/n] ").strip().lower() not in ("", "y", "yes"):
            raise RuntimeError("Dependency installation cancelled")
        if manager == "apt-get":
            run(["sudo", manager, "update"])
        run(["sudo", manager] + args)
    run(["pkg-config", "--atleast-version=4.12", "gtk4"])
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    env = os.environ.copy()
    env["PATH"] = str(cargo_home / "bin") + os.pathsep + env.get("PATH", "")
    rustc = shutil.which("rustc", path=env["PATH"])
    rust_version = run([rustc, "--version"], capture_output=True, text=True).stdout if rustc else ""
    match = re.search(r"rustc (\d+)\.(\d+)", rust_version)
    if not match or tuple(map(int, match.groups())) < (1, 89):
        if not allow_install:
            raise RuntimeError("Rust 1.89+ is required. Install/update Rust at https://rustup.rs and retry")
        if not assume_yes and input("Install/update Rust using the official rustup installer? [Y/n] ").strip().lower() not in ("", "y", "yes"):
            raise RuntimeError("Rust installation cancelled")
        with tempfile.TemporaryDirectory(prefix="phantom-rustup-") as tmp:
            installer = Path(tmp) / "rustup-init.sh"
            with urllib.request.urlopen("https://sh.rustup.rs", timeout=60) as response:
                installer.write_bytes(response.read())
            run(["sh", installer, "-y", "--profile", "minimal", "--default-toolchain", "stable"], env=env)
    return env


def device_rules():
    run(["sudo", "install", "-Dm644", ROOT / "packaging/70-phantom-tkl.rules", "/etc/udev/rules.d/70-phantom-tkl.rules"])
    run(["sudo", "install", "-Dm644", ROOT / "packaging/70-phantom-uinput.rules", "/etc/udev/rules.d/70-phantom-uinput.rules"])
    run(["sudo", "install", "-Dm644", ROOT / "packaging/phantom-uinput.conf", "/etc/modules-load.d/phantom-uinput.conf"])
    run(["sudo", "modprobe", "uinput"])
    run(["sudo", "udevadm", "control", "--reload-rules"])
    for device in Path("/sys/class/hidraw").glob("hidraw*"):
        if "0000320F:0000505B" in (device / "device/uevent").read_text():
            run(["sudo", "udevadm", "trigger", "--action=add", device.resolve()])
    run(["sudo", "udevadm", "trigger", "--action=add", "--subsystem-match=misc", "--sysname-match=uinput"])
    run(["sudo", "udevadm", "settle"])
    print("Keyboard and macro permissions installed. Reconnect the keyboard if access is still denied.")


def main():
    parser = argparse.ArgumentParser(description="Install Phantom Studio and its application-menu shortcut")
    parser.add_argument("--yes", "-y", action="store_true", help="accept dependency installation")
    parser.add_argument("--no-deps", action="store_true", help="use existing build dependencies")
    parser.add_argument("--no-device-rules", action="store_true", help="skip system USB/uinput configuration")
    parser.add_argument("--no-desktop-rules", action="store_true", help="leave compositor configuration unchanged")
    parser.add_argument("--repair", action="store_true", help="repair an existing installation without rebuilding")
    parser.add_argument("--uninstall", action="store_true", help="remove the user installation; keep profiles")
    parser.add_argument("--prefix", type=Path, default=Path.home() / ".local")
    args = parser.parse_args()
    if sys.platform != "linux":
        parser.error("Phantom Studio requires Linux")
    if os.geteuid() == 0:
        parser.error("Run as your normal desktop user. The installer uses sudo only for system setup")
    prefix = args.prefix.expanduser().resolve()
    data_home = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share"))
    config_home = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config"))
    binary = prefix / "bin/phantom-studio"
    if args.uninstall:
        binary.unlink(missing_ok=True)
        for file in (f"applications/{APP_ID}.desktop", "applications/phantom-tkl.desktop", f"icons/hicolor/scalable/apps/{APP_ID}.svg"):
            path = data_home / file
            if path.exists() and (file.endswith(".svg") or "Phantom Studio" in path.read_text()):
                path.unlink()
        print("Application removed. Your profiles, configuration backups and system permission rules were kept.")
        return
    if not args.repair:
        if (ROOT / "Cargo.toml").exists():
            env = dependencies(not args.no_deps, args.yes)
            print("Building Phantom Studio. The first build can take a few minutes.")
            run(["cargo", "build", "--release", "--locked"], cwd=ROOT, env=env)
            source = ROOT / "target/release/phantom-studio"
        else:
            source = ROOT / "phantom-studio"
            run([source, "--help"], stdout=subprocess.DEVNULL)
        binary.parent.mkdir(parents=True, exist_ok=True)
        temp = binary.with_suffix(".new")
        shutil.copyfile(source, temp)
        temp.chmod(0o755)
        os.replace(temp, binary)
    elif not binary.is_file():
        raise RuntimeError("No existing installation found. Run ./install.sh without --repair first")
    install_desktop(prefix, data_home, config_home, not args.no_desktop_rules)
    if not args.no_device_rules:
        device_rules()
    print(f"\nInstalled! Open Phantom Studio in your application menu.\nTerminal: {binary}\nProfiles are kept when reinstalling or upgrading.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"Installation stopped: {error}", file=sys.stderr)
        sys.exit(1)
