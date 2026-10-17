# Phantom Studio

**A reverse engineering project by nirusaki.** A native Rust and GTK4 control app for the **Phantom TKL keyboard** (`320f:505b`).

## Supported Platforms

- **Linux (Wayland and X11)**: Keyboard configuration, lighting previews, profiles, macro recording and desktop playback. Release bundles for `x86_64` and `aarch64` / ARM64.
- **macOS (experimental)**: GTK4 desktop builds for Apple Silicon and Intel. Hardware access needs testing on macOS; background recording and desktop macro playback require Linux.

## Install on Linux

Open a terminal and run this **without sudo**:

```sh
git clone https://github.com/Nirusaki-Malaal/phantom-tkl-driver.git
cd phantom-tkl-driver
./install.sh
```

The installer builds the app, installs your application-menu shortcut and sets up keyboard and macro playback permissions. It asks for your password when system setup needs it. Open **Phantom Studio** from your application menu when it finishes.

Pre-built desktop bundles are available in [Releases](https://github.com/Nirusaki-Malaal/phantom-tkl-driver/releases). GTK **4.12 or newer** is required. [Binary installation, special setups and uninstalling](docs/DOCUMENTATION.md#installation).

## Features

- **RGB Lighting**: 19 effects, per-key custom colors and live animation previews synchronized to the display frame clock.
- **Key Assignments**: Keyboard, media and supported mouse actions, with named macro selection and onboard configuration.
- **Macros**: A saved macro library, individual editing/deletion, unique names, background recording on Linux and cancellable desktop playback.
- **Profiles**: Save, copy, rename, import and export lighting, assignments and macros.
- **Device Controls**: Windows/Super-key lock and unlock, WASD/arrow swap, rollover and polling-rate settings.
- **Desktop UI**: Black-and-white surfaces, a collapsible sidebar and a compact tiled window on niri.

## AI usage disclosure

Reverse engineering and development were AI-assisted. Code, protocol behavior and platform support still require tests and hardware verification; see the [validation boundaries](docs/DOCUMENTATION.md#validation-boundaries).

## License

[GPL-3.0-or-later](LICENSE). Phantom Studio is independent of the keyboard vendor. Read the [documentation](docs/DOCUMENTATION.md) and [release notes](docs/RELEASE_NOTES.md) for setup, current limits and development details.
