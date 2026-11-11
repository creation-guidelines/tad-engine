#!/bin/sh
# Brings every repository that uses tad to tad's latest release: the pin in its bin/install-tad.sh
# moved, the data re-canonicalized and the documents re-rendered by the new tad, then landed through
# land - so each repository's own CI decides. Run weekly by the lab machine.
#   update-pins.sh [OWNER/REPO...]    (default: the repositories listed in ./consumers)
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
latest=$(gh release list -R creation-guidelines/tad-engine --limit 50 --json tagName -q '[.[].tagName | select(startswith("tad-v"))] | first' | sed 's/^tad-v//')
[ -n "$latest" ] || { echo "update-pins: no tad release found" >&2; exit 1; }
repos="$*"; [ -n "$repos" ] || repos=$(grep -vE '^[[:space:]]*(#|$)' "$here/consumers")
failed=0
for r in $repos; do
	dir="$HOME/Projects/$r"
	[ -d "$dir" ] || { echo "$r: not cloned at $dir" >&2; failed=1; continue; }
	cd "$dir"
	git fetch -q origin && git reset -q --hard && git checkout -q -B "build/tad-$latest" origin/main
	pin=$(sed -n 's/^version="\${TAD_VERSION:-\([^}]*\)}"$/\1/p' bin/install-tad.sh)
	[ -n "$pin" ] || { echo "$r: no pin in bin/install-tad.sh" >&2; failed=1; continue; }
	if [ "$pin" = "$latest" ]; then echo "$r: tad $pin, current"; continue; fi
	sed -i "s/^version=\"\${TAD_VERSION:-$pin}\"\$/version=\"\${TAD_VERSION:-$latest}\"/" bin/install-tad.sh
	grep -q "TAD_VERSION:-$latest}" bin/install-tad.sh || { echo "$r: the pin did not move" >&2; failed=1; continue; }
	bin=$(mktemp -d); TAD_DEST="$bin" TAD_VERSION="$latest" ./bin/install-tad.sh >/dev/null
	PATH="$bin:$PATH" make canon render >/dev/null
	rm -rf "$bin"
	n=$(git status --porcelain | wc -l)
	git add -A && git -c user.name="${GIT_NAME:-tad update-pins}" -c user.email="${GIT_EMAIL:-agent@creation-guidelines}" commit -qm "build: tad $latest" -m "The pin moves from $pin to $latest; the data re-canonicalized and the documents re-rendered by it ($n files changed)."
	if land --no-manual >/dev/null 2>&1; then echo "$r: tad $pin -> $latest, landed ($n files)"; else echo "$r: tad $latest refused by its checks; left on build/tad-$latest" >&2; failed=1; fi
done
exit $failed
