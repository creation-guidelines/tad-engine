#!/bin/sh
# Packs the built tad into dist/: a tarball, a .deb and an .rpm, with SHA256SUMS. Called by the package
# conveyor after build-binary.sh: packaging/build-packages.sh TARGET
set -eu
target="${1:?usage: $0 <rust target triple>}"
cd "$(dirname "$0")/.."
binary="target/$target/release/tad"
[ -x "$binary" ] || { echo "missing $binary; build it first" >&2; exit 1; }
version="$("$binary" --version | awk '{print $2}')"
[ -n "$version" ] || { echo "cannot run $binary" >&2; exit 1; }
SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || date +%s)}"; export SOURCE_DATE_EPOCH
rm -rf dist && mkdir -p dist
# what is said about the command line comes from its contract: completions for four shells and the man page
share="tad/generated"; rm -rf "$share" && mkdir -p "$share"
"$binary" completions bash > "$share/tad.bash"
"$binary" completions zsh > "$share/_tad"
"$binary" completions fish > "$share/tad.fish"
"$binary" completions nushell > "$share/tad.nu"
"$binary" man --date "$(date -u -d "@$SOURCE_DATE_EPOCH" +%F)" | gzip -9n > "$share/tad.1.gz"
cargo deb --no-build -p tad --target "$target" --output dist/
cargo generate-rpm -p tad --target "$target" --output dist/
name="tad-v$version-$target"; stage="$(mktemp -d)"; mkdir -p "$stage/$name"
cp "$binary" "$stage/$name/tad"; cp -r "$share" "$stage/$name/share"; cp README.md "$stage/$name/"; [ -f LICENSE ] && cp LICENSE "$stage/$name/"
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$SOURCE_DATE_EPOCH" -C "$stage" -czf "dist/$name.tar.gz" "$name"
rm -rf "$stage"
( cd dist && sha256sum ./* > SHA256SUMS )
ls -la dist/
