# Phantom Studio

A native GTK 4 / Rust application for the **Phantom TKL, USB `320f:505b`**. Dark graphite surfaces, lavender accents, an interactive keyboard, and direct HID communication. No Wine, Windows runtime, or vendor DLL is required.

## Run

```sh
./run.sh
```

The built executable is `target/release/phantom-studio`. To build yourself:

```sh
cargo build --release --locked
```

Requires Rust **1.89 or newer**, GTK **4.12 or newer**, a C toolchain and pkg-config. Build validated with Rust 1.98 and GTK 4.22.4 on this machine. The executable dynamically links GTK and the usual Linux desktop libraries; it is not a universal static binary.

Arch: `sudo pacman -S --needed rust gtk4 base-devel pkgconf`

Ubuntu 24.04+: `sudo apt install libgtk-4-dev build-essential pkg-config` plus a current Rust toolchain.

## Features

- Nineteen effects listed by the original keyboard configuration. Color, speed and direction controls follow the original effect capabilities; Surmount has four preset palettes.
- Custom per-key RGB painting and ten hardware color slots.
- Keyboard and media assignments, disabled keys, and onboard macro assignments.
- Macro recording inside the app, editable event timing and a validated binary macro encoder.
- Local profiles, duplication, rename, deletion, JSON import/export, and Windows keyboard-profile import.
- Windows/Super lock, WASD/arrow swap, six-key rollover and four polling presets.
- USB discovery by VID/PID **and vendor usage page**, background device I/O, bounded packets, response matching, backups and readback verification.
- Frosted backgrounds with compositor-native blur on niri 26.04+, and an opaque appearance option.

This is a functional Linux reimplementation, **not yet a verified complete replica**. See [feature coverage and validation](research/PROTOCOL.md). Onboard macro playback and physical remapping still need end-to-end keypress verification. Vendor cloud, firmware updating, and Windows program-launch hooks are not implemented. Audio/screen lighting and repeat-rate controls are hidden in the supplied Windows keyboard UI and are not exposed here.

### Custom lighting

Choose **Edit custom lighting**, choose a brush color, select and paint individual keys (or use **Paint all keys**), then **Apply lighting**. PrtSc is dimmed because the device exposes no programmable entry for it; it no longer prevents other keys from being painted. **Device → Read lighting from keyboard** reloads current stored colors. Custom lighting was confirmed working on the physical keyboard by the user on 2026-09-11.

### Native blur

On niri 26.04 or newer, run `python tools/enable_niri_blur.py` once. It validates the configuration and makes a timestamped backup before adding a rule for Phantom Studio only. This rule is already installed on the development machine. Toggle **Device → Appearance → Frosted background** to switch between translucent and opaque surfaces. Text remains opaque. Other compositors need their own native blur configuration; see [niri's window-effects documentation](https://niri-wm.github.io/niri/Window-Effects.html).

## USB access

If the app reports permission denied:

```sh
sudo install -Dm644 packaging/70-phantom-tkl.rules /etc/udev/rules.d/70-phantom-tkl.rules
sudo udevadm control --reload-rules
```

Reconnect the keyboard afterward. The rule grants the active local user's session access. Do not run the GTK app with sudo. The driver opens only usage page `ff1c`, usage `92`; it never opens unrelated keyboards or mice.

## Profiles and backups

Stored under `${XDG_CONFIG_HOME:-$HOME/.config}/phantom-studio/`. `PHANTOM_CONFIG_DIR` overrides this path for testing. Use **Save to profile** to persist editor changes. Selecting a profile refreshes its editors; it does not write to the device.

Lighting writes first save the current settings under `backups/`. Custom color writes also save the original slot bytes. Keymap application replaces the device keymap with the profile's assignments and the original default map for other keys. The recovered read command returns the default physical map, not a verified backup of previously remapped keys. Keep your existing Windows profile if you need to preserve those assignments.

Windows imports accept the supplied `Keyboard.json` structure. Unsupported launch or macro references produce an explicit error; they are not silently converted. Windows macro files are not imported by this version.

## Diagnostics

```sh
target/release/phantom-studio --probe
target/release/phantom-studio --test-lighting
target/release/phantom-studio --test-custom
target/release/phantom-studio --restore-settings /path/to/snapshot.json
```

The two test commands temporarily modify settings/color data, read them back, then restore and verify the original bytes. `--restore-settings` restores only the 26-byte device settings block, not keymaps, macros or custom RGB slots.

## Development

```sh
make check
cargo test -- --include-ignored --test-threads=1  # includes GTK regression; needs a display
make install  # installs the executable and desktop entry under ~/.local
```

The application crate forbids unsafe Rust. GTK and HID libraries implement the native platform boundary. Python and Java under `tools/` are development-only reverse-engineering helpers, not application components.

Analysis used the supplied executable through [GhidraMCP](https://github.com/LaurieWired/GhidraMCP), with a read-only headless Ghidra adapter because the bridge initially had no connected backend. Original inputs, hashes, function addresses, captured USB transactions and limitations are recorded in [research/PROTOCOL.md](research/PROTOCOL.md). Implementation references: [GTK Rust bindings](https://gtk-rs.org/gtk4-rs/stable/latest/docs/gtk4/) and [Linux hidraw documentation](https://cdn.kernel.org/doc/html/latest/hid/hidraw.html).
