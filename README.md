# Phantom Studio

A native Linux app for the **Phantom TKL keyboard (USB 320f:505b)**. Set lighting, paint individual keys, remap keys, record macros and play them in your favorite apps. Built with GTK 4 and Rust. No Wine or Windows software required.

## Install

Open a terminal and run:

```sh
git clone https://github.com/Nirusaki-Malaal/phantom-tkl-driver.git
cd phantom-tkl-driver
./install.sh
```

Run the installer as your normal desktop user. Enter your sudo password when requested for hardware access and any missing system packages. The first build can take a few minutes.

When it finishes, open **Phantom Studio** from your application menu. You can also launch it with:

```sh
~/.local/bin/phantom-studio
```

The installer:

- Builds the app for your machine and installs it under `~/.local`.
- Creates a working application-menu shortcut with an absolute executable path.
- Repairs the older Phantom TKL shortcut, including existing pinned entries.
- Sets up keyboard access and the virtual keyboard used for macro playback.
- Configures a floating app window on niri, with native blur on niri 26.04 or newer.
- Preserves your saved profiles when upgrading or reinstalling.

### Requirements

Use a Linux desktop with GTK **4.12 or newer**. Python 3 and Git are needed to run the commands above. The installer can install missing build packages using **apt, pacman, dnf or zypper**, and can install Rust through the official rustup installer when needed.

The source installer is the recommended option across distributions and CPU architectures. It has been tested on Arch Linux with niri. The other package-manager paths are provided but have not yet been tested on their respective distributions. Distributions with older GTK versions need an upgrade first.

If your dependencies and device permissions are already configured:

```sh
./install.sh --no-deps --no-device-rules
```

Use `./install.sh --help` for all options. On an unrecognized distribution, install Rust 1.89+, GTK 4.12 development files, a C compiler and pkg-config, then use `--no-deps`.

## Start using your keyboard

### Lighting

1. Connect the keyboard and open **Lighting**.
2. Choose an effect and adjust its available controls.
3. Click **Apply lighting**.

Effects with their own built-in colors disable the color picker. Controls for unsupported speed or direction settings are also disabled. Surmount offers four preset palettes.

For individual key colors, click **Edit custom lighting**, choose a brush color, select a key and click **Paint selected key**. Use **Paint all keys** for a uniform layout, then click **Apply custom lighting** below the keyboard.

PrtSc is dimmed because this keyboard exposes no programmable entry for that key. Fn lighting is supported, but the firmware reserves Fn from remapping.

### Key assignments

Open **Key assignments**, select a physical key, choose its new function and click **Assign selected key**. Click **Apply keymap** to send the assignments to the keyboard.

Applying a keymap replaces the existing onboard assignments. Keys without an assignment in the selected profile return to their defaults.

### Record and play macros

1. Open **Macros** and click **Record keystrokes**.
2. Type the sequence while the Phantom Studio window is focused, then click **Stop recording**.
3. Give it a name and click **Save new macro**.
4. Under **Play a macro**, select the saved macro and choose a repeat count and start delay.
5. Click **Play selected macro**, then focus the destination app during the countdown.

Playback sends real keyboard events to the focused app on Wayland or X11. **Stop playback** cancels the sequence and releases held keys. Closing Phantom Studio also stops playback. Recording is limited to this app's window.

You can edit event timing in the macro editor. Each line is `down` or `up`, a USB key usage, and a delay in milliseconds before that event. For example, pressing and releasing A:

```text
down 4 0
up 4 100
```

To assign a stored macro to a physical keyboard key, use **Key assignments → Macro number → Assign macro → Apply keymap**. Desktop playback and onboard playback are separate features. Onboard playback still needs broader physical verification.

### Profiles and appearance

Use **Save to profile** to keep lighting and key edits. The **Profiles** page supports copying, renaming, importing and exporting profiles. Selecting a profile does not immediately change the keyboard.

**Device → Read lighting from keyboard** loads current settings and custom colors. **Device → Appearance → Frosted background** switches between translucent and opaque surfaces. Native blur depends on compositor support. Color dialogs are parented to the app for proper floating-dialog behavior.

## Fix a launcher or connection problem

If a menu shortcut does not open the app, or device access needs repairing:

```sh
cd phantom-tkl-driver
./install.sh --repair
```

Then reopen **Phantom Studio** from the menu. The repaired legacy shortcut is kept as a hidden alias so existing pinned entries still work. Unrelated Wine shortcuts are left alone.

If the keyboard is not detected, reconnect it after installation and press **Reconnect**. Check its USB ID with `lsusb`: this version targets `320f:505b`.

If macro playback reports `/dev/uinput` access denied, run the repair command and retry. Hardware macros assigned to keys do not require the desktop playback feature.

For startup problems, run `~/.local/bin/phantom-studio` in a terminal to see the error. You can try the Cairo renderer if your graphics driver has GTK rendering problems:

```sh
GSK_RENDERER=cairo ~/.local/bin/phantom-studio
```

Do not launch the GUI with sudo.

## Update or uninstall

Update from the project folder:

```sh
git pull --ff-only
./install.sh
```

Remove the installed app and menu entries:

```sh
./install.sh --uninstall
```

Uninstalling keeps your profiles, configuration backups and system device-access rules.

Profiles live in `~/.config/phantom-studio`, or the corresponding directory under `XDG_CONFIG_HOME`. Lighting changes create device backups in its `backups` folder. Export profiles from the app if you want to move them to another machine.

## Supported features and current limits

The app includes 19 lighting effects, custom key colors, keyboard/media assignments, macro recording and desktop playback, profile management, Windows-key lock, WASD/arrow swap, six-key rollover and polling-rate presets.

Custom lighting has been confirmed working on the physical keyboard. Protocol readback, restoration, macro scheduling and cancellation have automated tests. Full parity with every function in the original Windows package is not claimed: firmware updates, vendor cloud services, Windows program-launch hooks and Windows macro-file import are not implemented. Some hardware behaviors, including onboard macro playback, still need broader testing.

## Development

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
python3 -m unittest discover -s tests -p 'test_*.py'
```

GTK regression tests need a desktop session:

```sh
cargo test --locked ui::tests:: -- --ignored --test-threads=1
```

The optional virtual-device capture test needs access to the created event node and no competing input grab, such as a catch-all keyd configuration. It is excluded from the normal test suite.

See [protocol and reverse-engineering notes](research/PROTOCOL.md) for GhidraMCP findings and hardware validation. Raw decompiler exports, local captures, generated binaries and personal settings are excluded from Git. A small protocol fixture is kept under `tests/fixtures`.

Phantom Studio is independent of the keyboard vendor. Application code is licensed under [GPL-3.0-or-later](LICENSE).
