#!/bin/sh
set -eu

REPO="christophergutierrez/devtribunal"
BINARY="devtribunal"
INSTALL_DIR="$HOME/.local/bin"

main() {
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)  os_target="unknown-linux-gnu" ;;
        Darwin) os_target="apple-darwin" ;;
        *)
            echo "Error: unsupported OS: $os" >&2
            echo "Try: cargo install --git https://github.com/$REPO" >&2
            exit 1
            ;;
    esac

    case "$arch" in
        x86_64|amd64)   arch_target="x86_64" ;;
        aarch64|arm64)   arch_target="aarch64" ;;
        *)
            echo "Error: unsupported architecture: $arch" >&2
            echo "Try: cargo install --git https://github.com/$REPO" >&2
            exit 1
            ;;
    esac

    target="${arch_target}-${os_target}"
    archive="${BINARY}-${target}.tar.gz"
    url="https://github.com/${REPO}/releases/latest/download/${archive}"
    checksums_url="https://github.com/${REPO}/releases/latest/download/checksums.txt"

    echo "Detected platform: ${target}"
    echo "Downloading ${BINARY}..."

    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    if ! curl -fsSL "$url" -o "${tmpdir}/${archive}"; then
        echo "Error: failed to download ${url}" >&2
        echo "No release found for your platform. Try building from source:" >&2
        echo "  cargo install --git https://github.com/$REPO" >&2
        exit 1
    fi

    # Verify the download against the published SHA256 checksums. A mismatch is
    # fatal; a missing checksums file or sha256 tool degrades to a warning so the
    # install still works on minimal platforms.
    if curl -fsSL "$checksums_url" -o "${tmpdir}/checksums.txt"; then
        expected="$(grep "$archive" "${tmpdir}/checksums.txt" | awk '{print $1}' | head -n1)"
        if command -v sha256sum >/dev/null 2>&1; then
            actual="$(sha256sum "${tmpdir}/${archive}" | awk '{print $1}')"
        elif command -v shasum >/dev/null 2>&1; then
            actual="$(shasum -a 256 "${tmpdir}/${archive}" | awk '{print $1}')"
        else
            actual=""
        fi
        if [ -z "$actual" ]; then
            echo "Warning: no sha256 tool found; skipping checksum verification" >&2
        elif [ -z "$expected" ]; then
            echo "Warning: ${archive} not listed in checksums.txt; skipping verification" >&2
        elif [ "$actual" != "$expected" ]; then
            echo "Error: checksum mismatch for ${archive}" >&2
            echo "  expected: ${expected}" >&2
            echo "  actual:   ${actual}" >&2
            exit 1
        else
            echo "Checksum verified."
        fi
    else
        echo "Warning: could not download checksums.txt; skipping verification" >&2
    fi

    tar xzf "${tmpdir}/${archive}" -C "$tmpdir"

    mkdir -p "$INSTALL_DIR"
    mv "${tmpdir}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
    chmod +x "${INSTALL_DIR}/${BINARY}"

    if "${INSTALL_DIR}/${BINARY}" --version > /dev/null 2>&1; then
        echo "Installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"
    else
        echo "Warning: binary installed but --version check failed" >&2
    fi

    case ":$PATH:" in
        *":${INSTALL_DIR}:"*) ;;
        *)
            echo ""
            echo "Add ${INSTALL_DIR} to your PATH:"
            echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
            ;;
    esac
}

main
