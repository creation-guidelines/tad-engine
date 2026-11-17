#!/bin/sh
# Installs tad, the data engine, from its pinned release (creation-guidelines/tad-engine): the binary
# for this machine, checked against the release's own checksums before it is installed.
set -eu
version="${TAD_VERSION:-@VERSION@}"
arch=$(uname -m)
case "$arch" in x86_64 | aarch64) ;; *) echo "install-tad: no tad build for $arch" >&2; exit 1 ;; esac
target="$arch-unknown-linux-musl"; name="tad-v$version-$target"
base="https://github.com/creation-guidelines/tad-engine/releases/download/tad-v$version"
dest="${TAD_DEST:-$HOME/.local/bin}"; work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
curl -fsSL -o "$work/$name.tar.gz" "$base/$name.tar.gz"
curl -fsSL -o "$work/SHA256SUMS" "$base/SHA256SUMS-$target"
want=$(grep " \./$name.tar.gz\$\| $name.tar.gz\$" "$work/SHA256SUMS" | cut -d' ' -f1)
have=$(sha256sum "$work/$name.tar.gz" | cut -d' ' -f1)
[ -n "$want" ] && [ "$want" = "$have" ] || { echo "install-tad: the download does not match the release's checksum" >&2; exit 1; }
tar -xzf "$work/$name.tar.gz" -C "$work"
mkdir -p "$dest" && install -m 755 "$work/$name/tad" "$dest/tad"
echo "tad $version installed in $dest"
