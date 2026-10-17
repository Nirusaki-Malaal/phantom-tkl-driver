#!/bin/sh
# Online source installer for supported Linux and macOS hosts.
set -eu
source_url=https://github.com/Nirusaki-Malaal/phantom-tkl-driver/archive/refs/heads/main.tar.gz
help=false
dry_run=false
for option in "$@"; do
    case "$option" in
        --help|-h) help=true ;;
        --dry-run) dry_run=true ;;
    esac
done
if [ "$help" = true ]; then
    cat <<'EOF'
Install Phantom Studio (run without sudo):
  curl -fsSL https://raw.githubusercontent.com/Nirusaki-Malaal/phantom-tkl-driver/main/bootstrap.sh | sh

Options:
  --dry-run              Show host and download plan without changing anything
  --yes                  Accept dependency/Rust installation
  --no-deps              Use existing build tools and GTK4 development libraries
  --no-device-rules      Skip Linux USB/uinput setup
  --no-desktop-rules     Leave Linux compositor configuration unchanged
  --prefix PATH          Install under PATH (default: ~/.local)
  --repair               Repair an existing Linux installation without building
  --uninstall            Remove installed app; preserve profiles

Pass options with: curl -fsSL URL | sh -s -- --yes
Linux: apt, pacman, dnf or zypper. macOS: Homebrew must already be installed.
EOF
    exit 0
fi
os=$(uname -s)
arch=$(uname -m)
case "$os" in
    Linux|Darwin) ;;
    *) echo "Unsupported operating system: $os. Use Linux or macOS." >&2; exit 1 ;;
esac
case "$arch" in
    x86_64|amd64) arch=x86_64 ;;
    aarch64|arm64) arch=aarch64 ;;
    *) echo "Unsupported CPU architecture: $arch. Use x86_64 or ARM64." >&2; exit 1 ;;
esac
if [ "$dry_run" = true ]; then
    echo "Host: $os / $arch"
    echo "Download: $source_url"
    echo 'Build native desktop app and install using tools/install.py.'
    echo 'Linux: configure menu/device integration unless disabled. macOS: install executable under ~/.local/bin.'
    exit 0
fi
[ "$(id -u)" != 0 ] || { echo 'Run as your desktop user without sudo.' >&2; exit 1; }
command -v curl >/dev/null 2>&1 || { echo 'Install curl first.' >&2; exit 1; }
command -v tar >/dev/null 2>&1 || { echo 'Install tar first.' >&2; exit 1; }
if [ "$os" = Darwin ]; then
    command -v brew >/dev/null 2>&1 || {
        echo 'Install Homebrew from https://brew.sh, then run this installer again.' >&2
        exit 1
    }
    # Homebrew Python avoids relying on the optional Apple developer-tools stub.
    if ! command -v python3 >/dev/null 2>&1; then brew install python; fi
elif ! command -v python3 >/dev/null 2>&1; then
    command -v sudo >/dev/null 2>&1 || { echo 'Install Python 3 and sudo first.' >&2; exit 1; }
    if command -v apt-get >/dev/null 2>&1; then
        sudo apt-get update
        sudo apt-get install -y python3
    elif command -v pacman >/dev/null 2>&1; then
        sudo pacman -S --needed --noconfirm python
    elif command -v dnf >/dev/null 2>&1; then
        sudo dnf install -y python3
    elif command -v zypper >/dev/null 2>&1; then
        sudo zypper --non-interactive install python3
    else
        echo 'Install Python 3 using your package manager, then retry.' >&2
        exit 1
    fi
fi
workdir=$(mktemp -d)
cleanup() { rm -rf "$workdir"; }
trap cleanup 0
trap 'exit 130' INT
trap 'exit 143' TERM
echo "Downloading Phantom Studio source for $os / $arch..."
curl -fL --retry 3 --connect-timeout 15 --max-time 120 "$source_url" -o "$workdir/source.tar.gz"
tar -xzf "$workdir/source.tar.gz" -C "$workdir"
installer="$workdir/phantom-tkl-driver-main/tools/install.py"
# curl | sh consumes stdin. Give Python's confirmation prompts the terminal
# when one exists; otherwise use --yes or let the installer fail clearly.
if ( : </dev/tty ) 2>/dev/null; then
    python3 "$installer" "$@" </dev/tty
else
    python3 "$installer" "$@" </dev/null
fi
