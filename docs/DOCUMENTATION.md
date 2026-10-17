# Phantom Studio documentation

Phantom Studio is a reverse engineering project by nirusaki for the Phantom TKL keyboard, USB `320f:505b`. It uses Rust, GTK4 and hidapi for direct hardware configuration.

## Supported platforms

| Platform | Configuration and previews | Desktop macro playback | Background recording |
| --- | --- | --- | --- |
| Linux, Wayland or X11 | Supported; tested on Arch Linux/niri | Linux uinput | Linux evdev, when accessible |
| macOS, Apple Silicon or Intel | Experimental builds; hardware unverified | Unavailable | Unavailable; in-window recording remains available |

ARM64 Linux builds run on native ARM64 CI runners. A successful build establishes compilation and packaging, not physical keyboard compatibility on every machine. GTK and other native libraries are not bundled.

## Installation

### Source installation on Linux

```sh
git clone https://github.com/Nirusaki-Malaal/phantom-tkl-driver.git
cd phantom-tkl-driver
./install.sh
```

Run as your desktop user. The installer uses sudo only for missing system dependencies and device rules. It installs the binary to `~/.local/bin`, a desktop entry and icon under `~/.local/share`, and Linux udev/uinput configuration. Launch from the application menu or run `~/.local/bin/phantom-studio`.

Requirements: GTK 4.12+ development files, Rust 1.89+, a C compiler, pkg-config, Python 3 and Git. Dependency installation supports apt, pacman, dnf and zypper; these distribution paths have not all been tested. On older GTK distributions, upgrade the GTK stack first.

```sh
./install.sh --no-deps --no-device-rules
./install.sh --no-desktop-rules
./install.sh --prefix "$HOME/.local"
./install.sh --help
```

`--no-deps` uses existing build dependencies. `--no-device-rules` skips system device configuration. `--no-desktop-rules` preserves compositor configuration.

### Binary installation on Linux

Download the matching archive and `SHA256SUMS` from [Releases](https://github.com/Nirusaki-Malaal/phantom-tkl-driver/releases). Linux bundles are built on Ubuntu 24.04 and require compatible glibc, GTK4 and udev runtime libraries; they are not portable static binaries.

```sh
# Verify the downloaded archive; ignore entries for other platforms.
sha256sum --ignore-missing --check SHA256SUMS
tar -xzf phantom-studio-0.2.0-linux-x86_64.tar.gz
cd phantom-studio-0.2.0-linux-x86_64
./install.sh
```

Use the `linux-aarch64` archive and directory on ARM64. The extracted installer uses the bundled binary instead of rebuilding it. Source installation is preferable when your system libraries differ from the release runner.

### Experimental macOS builds

Install GTK4 with Homebrew, download the matching `macos-aarch64` or `macos-x86_64` archive, verify its checksum, and extract it:

```sh
brew install gtk4
shasum -a 256 phantom-studio-0.2.0-macos-aarch64.tar.gz
# Compare the printed digest with the matching line in SHA256SUMS.
tar -xzf phantom-studio-0.2.0-macos-aarch64.tar.gz
cd phantom-studio-0.2.0-macos-aarch64
./phantom-studio
```

The Linux installer does not run on macOS. These are unsigned command-line GTK executables, not notarized application bundles. They depend on the Homebrew runtime libraries used by their runner. Do not assume broad macOS deployment compatibility. To build locally:

```sh
brew install gtk4 pkg-config
cargo build --release --locked
./target/release/phantom-studio
```

### niri integration

The installer updates its managed niri rule only when niri and an existing configuration are present. It validates the candidate before writing it and keeps a backup. The window defaults to a 1024-pixel column and is no longer forced into a floating window. Blur is included on supported niri versions; disable **Frosted background** for opaque surfaces. Use `--no-desktop-rules` to opt out.

## Lighting and key assignments

Choose one of the 19 lighting effects, adjust the controls available to that effect, and click **Apply lighting**. The preview uses GTK's display frame clock, so it can follow a high-refresh display when the compositor allows it. This does not change the keyboard's USB polling rate and is not a promise of an exact frame rate or firmware-identical effect rendering.

For custom colors, use **Edit custom lighting**, choose a brush, select keys, and apply the custom layout. PrtSc has no programmable entry in the observed device map. Fn supports lighting but is reserved from remapping by firmware.

In **Key assignments**, select a key, choose its function, then use **Assign selected key** and **Apply keymap**. Applying a keymap replaces onboard assignments; unassigned keys revert to their defaults. For a macro, choose its saved name and use **Assign macro** before applying the keymap.

## Macros

The **Saved Macros** list provides **Play**, **Edit** and **Delete** for each macro. The editor provides a name, sequence preview and editable event text.

1. Start a new macro or edit an existing one.
2. Click **Record keystrokes**, type your sequence, then stop recording.
3. Use **Save Macro** for a new macro or **Update Macro** while editing.
4. Use **Save as New** to keep a separate copy. Duplicate names receive a numeric suffix.

With **Background recording** enabled on Linux, the recorder reads accessible keyboard evdev nodes, including while other applications are focused. If it cannot open a keyboard input node, it falls back to recording in the Phantom Studio window. Input access depends on your session/device permissions; the installer configures hidraw and uinput but does not grant broad evdev access or add you to the input group. Record only the intended sequence, then stop.

Deleting a macro adjusts later macro indices and removes assignments targeting the deleted macro. Saving sanitizes redundant or unmatched transitions and adds releases for keys still held when recording ends.

Event lines accept HID usage numbers or supported key names, with a delay in milliseconds before each event:

```text
down Ctrl 0
down A 20
up A 80
up Ctrl 0
```

Desktop playback uses a countdown and repeat count, sends events through Linux uinput, and targets the focused application. **Stop** or closing the window cancels playback and releases held keys. Playback and background recording are unavailable on macOS. Onboard macros are separate from desktop playback and still need broader hardware verification.

## Profiles and device settings

Use **Save to profile** to retain lighting edits. The **Profiles** page supports saving/renaming, copying, importing and exporting. Selecting a profile does not automatically write it to the keyboard.

**Device** includes readback, Windows/Super-key lock, **Unlock Super / Windows key**, WASD swap, rollover and polling-rate controls. Hardware settings apply only when sent to a connected keyboard. Lighting writes preserve the selected Windows-key-lock state.

Toggle the sidebar with its header button, **Ctrl+B** or **F9**. The header displays **PHANTOM STUDIO**; the sidebar reports device name, physical key count and connection state.

Profiles and appearance live under `~/.config/phantom-studio`, or the corresponding `XDG_CONFIG_HOME` directory. Device writes create backups under its `backups` directory. Export a profile before moving it to another machine.

## Troubleshooting

### Keyboard not detected

Reconnect it and use **Reconnect**. Check `lsusb -d 320f:505b` on Linux. This project targets that device identity and its vendor HID collection, not every keyboard with a similar name.

```sh
./install.sh --repair
~/.local/bin/phantom-studio --probe
```

Repair reinstalls menu/device integration without rebuilding. The probe reads device information/settings without applying a profile. Run the GUI as your normal user, not sudo.

### Super key locked

Open **Device** and click **Unlock Super / Windows key**. The recording event controller is attached only during recording; the app should not intercept keys while idle. Also check firmware lock state and other system remappers if the key remains unavailable.

### Macro access or rendering failure

For `/dev/uinput` access errors, run the repair command and retry. For background recording, check access to the physical keyboard's `/dev/input/event*` node; ordinary in-window recording remains a fallback. Avoid changing permissions for every input device merely to diagnose one keyboard.

Run the executable in a terminal for startup errors. For graphics-driver problems, try:

```sh
GSK_RENDERER=cairo ~/.local/bin/phantom-studio
```

### Menu launchers and updates

Repair preserves a hidden legacy launcher alias for pinned entries and leaves unrelated launchers alone. If the UI still shows an older version after rebuilding, close and reopen the running app.

```sh
git pull --ff-only
./install.sh
./install.sh --uninstall
```

Uninstalling keeps profiles, configuration backups and system device rules.

## Development and release cycle

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
python3 -m unittest discover -s tests -p 'test_*.py'
cargo build --release --locked
```

GTK regression tests require a desktop session:

```sh
cargo test --locked ui::tests:: -- --ignored --test-threads=1
```

The ignored virtual-keyboard capture test requires access to its newly created input node and no competing input grab. Normal CI does not run these hardware/session-dependent tests.

`crates/phantom-core` contains reusable model, protocol and lighting-effect code. The desktop app also retains its existing modules; this release does not claim a complete migration to the library.

CI runs formatting, Clippy, Rust tests and Python installer/package tests. Native runners build Linux x86_64/ARM64 and macOS Apple Silicon/Intel. Each bundle includes the executable, installer/support files, documentation, license and `BUILD_INFO.json`. The packager extracts and smoke-tests the bundled executable's `--help` path on the build host. This is not a GUI or hardware test.

To build and package on the matching host:

```sh
./tools/build-cross.sh linux-x86_64
# Other native hosts: linux-aarch64, macos-aarch64, macos-x86_64.
```

Despite its legacy filename, the helper requires a matching native host and fails for unsupported host/target combinations. CI provides the other hosts.

For a release, update both Cargo package versions and `docs/RELEASE_NOTES.md`, run the checks, commit focused changes, and push an annotated version tag such as `v0.2.0`. The release workflow calls the same checks/builds, verifies the tag against the package versions, downloads those exact build artifacts, generates `SHA256SUMS`, and publishes them together. It does not publish on a failed build. Research captures, generated binaries and personal settings are excluded from the tracked tree; the small device fixture remains under `tests/fixtures`.

## Validation boundaries

Custom lighting was verified on the physical keyboard in prior development. Automated tests cover packet validation, fixture-based readback, macro scheduling/cancellation and installer behavior. CI compilation and `--help` smoke tests do not establish GUI rendering, macOS hardware access, or full onboard macro playback.

Firmware flashing, vendor cloud services, Windows application-launch hooks and Windows macro-file import are not implemented. Effect previews are visual approximations. No full parity with the original Windows package or universal desktop/compositor support is claimed.

## License and attribution

Application code and artwork use [GPL-3.0-or-later](../LICENSE). Dependencies retain their licenses. The vendor's Windows executables and DLLs are not distributed or required. See [NOTICE](../NOTICE). Development and reverse engineering were AI-assisted; published support claims are limited to the checks described above.
