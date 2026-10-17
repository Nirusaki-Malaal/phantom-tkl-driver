# Phantom Studio V0.2.0

Phantom Studio V0.2.0 brings a compact black-and-white desktop interface, animated lighting previews and a simpler macro workflow to the Phantom TKL keyboard. It adds checked release bundles for Linux and experimental macOS builds.

## Included in V0.2.0

- **Desktop Interface**: Collapsible sidebar, compact tiled niri window, clean header and monochrome surfaces.
- **Lighting Previews**: Animated previews for all 19 effects, driven by GTK's display frame clock and responsive to lighting settings.
- **Macro Library**: Individual playback, editing and deletion, unique names, Save as New, sequence summaries and automatic key-release cleanup.
- **Linux Background Recording**: Record through accessible evdev keyboard nodes, with an in-window fallback when access is unavailable.
- **Super-Key Controls**: Recording-only key capture, device lock-state synchronization and an explicit unlock action.
- **Desktop Builds**: Reusable Rust core and native Linux x86_64/ARM64 plus macOS Apple Silicon/Intel build jobs.
- **Automated Releases**: Formatting, Clippy, Rust and Python tests, extracted-bundle smoke checks, build metadata and SHA-256 checksums.

## Install on Linux

Download `phantom-studio-0.2.0-linux-x86_64.tar.gz` or `phantom-studio-0.2.0-linux-aarch64.tar.gz`, plus `SHA256SUMS` from this release.

```sh
sha256sum --ignore-missing --check SHA256SUMS
tar -xzf phantom-studio-0.2.0-linux-x86_64.tar.gz
cd phantom-studio-0.2.0-linux-x86_64
./install.sh
```

Use the matching ARM64 filename/directory on ARM64. Run the installer without sudo; it requests elevation for system setup. Linux binaries require Ubuntu 24.04-compatible native libraries including GTK 4.12+. Source installation is available for other compatible distributions.

## Experimental macOS builds

Choose `phantom-studio-0.2.0-macos-aarch64.tar.gz` for Apple Silicon or `phantom-studio-0.2.0-macos-x86_64.tar.gz` for Intel. Install GTK4 with `brew install gtk4`, verify the matching SHA-256 digest, extract the archive and run `./phantom-studio` inside it. The Linux installer does not run on macOS.

These are unsigned GTK executables with external Homebrew library dependencies. Hardware control is unverified on macOS. Background recording and desktop macro playback require Linux. Compilation and package smoke tests do not establish full hardware compatibility; onboard macro playback also needs broader verification.

See [documentation](https://github.com/Nirusaki-Malaal/phantom-tkl-driver/blob/v0.2.0/docs/DOCUMENTATION.md) for device access, profile migration, troubleshooting and validation boundaries. Existing profiles are preserved when reinstalling.
