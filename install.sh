#!/bin/bash
set -e

PREFIX="${PREFIX:-$HOME/.local}"

if [ "$1" = "--uninstall" ]; then
    rm -f "$PREFIX/bin/c4"
    rm -rf "$HOME/.config/c4"
    echo "Uninstalled c4."
    exit 0
fi

# Check macOS
if [ "$(uname -s)" != "Darwin" ]; then
    echo "Error: c4 only supports macOS."
    exit 1
fi

# Install Rust if needed
if ! command -v cargo &>/dev/null; then
    echo "Rust not found. Installing..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

# Clone or update
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

echo "Downloading c4..."
git clone --depth 1 https://github.com/bergerg/c4.git "$TMPDIR/c4"

echo "Building..."
cd "$TMPDIR/c4"
cargo build --release

echo "Installing to $PREFIX/bin/c4..."
install -d "$PREFIX/bin"
install -m 755 target/release/c4 "$PREFIX/bin/c4"

echo ""
echo "Done! Run 'c4' to start."
echo "To uninstall: curl -sSf <install-url> | bash -s -- --uninstall"
