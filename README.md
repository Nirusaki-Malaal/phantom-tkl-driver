# Phantom Studio

**A reverse engineering project by nirusaki.** A native Rust and GTK4 control app for the **Phantom TKL keyboard** (`320f:505b`).

## Supported Platforms

- **Linux (Wayland and X11)**: Keyboard configuration, lighting previews, profiles, macro recording and desktop playback. Release bundles for `x86_64` and `aarch64` / ARM64.
- **macOS (experimental)**: GTK4 desktop builds for Apple Silicon and Intel. Hardware access needs testing on macOS; background recording and desktop macro playback require Linux.

## Install on Linux

Open a terminal and paste this **without sudo**:

```sh
curl -fsSL https://raw.githubusercontent.com/Nirusaki-Malaal/phantom-tkl-driver/main/bootstrap.sh | sh
```

The universal source installer detects Linux or macOS and x86_64 or ARM64, downloads the source and builds for your machine. On Linux it installs your application-menu shortcut and keyboard/macro playback rules. It asks for your password when system setup needs it. Open **Phantom Studio** from your application menu when it finishes.

## Install on macOS (experimental)

Install [Homebrew](https://brew.sh) first, then run the same command above. The installer builds with GTK4 and installs `~/.local/bin/phantom-studio`; launch that command from a terminal. Background recording and desktop macro playback require Linux.

GTK **4.12 or newer** is required. Linux dependency setup supports apt, pacman, dnf and zypper. To accept dependency installation automatically, use `curl -fsSL URL | sh -s -- --yes` with the URL above. To inspect the plan, pass `--dry-run` instead.

Pre-built desktop bundles are also available in [Releases](https://github.com/Nirusaki-Malaal/phantom-tkl-driver/releases). [Binary/source installation, installer options and uninstalling](docs/DOCUMENTATION.md#installation).

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
