# tad-engine

The reusable "text-as-data" engine: guard, canonicalize, and query a small dataset kept as a
DuckDB `EXPORT DATABASE` folder (a `schema.sql`, a `load.sql`, one JSON-lines file per table), plus
a schema-agnostic Markdown renderer. It knows nothing about your schema, table names, or subject
matter - it's the plumbing, not the content.

This is the engine extracted from [creation-guidelines/text-as-data-template](https://github.com/creation-guidelines/text-as-data-template).
Consuming repos vendor it as a [`git subrepo`](https://github.com/ingydotnet/git-subrepo) at
`.tad/`, so engine fixes and features can flow into them with a normal `git subrepo pull` - not a
manual re-copy - as long as `.tad/` is never hand-edited downstream (see "For consuming repos"
below). `git-subrepo` is a third-party git extension (not bundled with git itself); it is only
needed by whoever runs the sync commands below, never by CI or by someone just cloning and reading
the repo - the vendored files are plain committed files either way.

## What's here
| Path | What |
|---|---|
| `tools/dc.py` | `check` (guard against silent data loss), `canon` (validate + canonical export), `sql` (run SQL, then canonicalize), `checks` (run `checks/*.sql` against the data) |
| `tools/render.py` | Render the data into Markdown pages, detecting a relations table and a sources table structurally rather than by name |
| `tests/test_dc.py` | Tests for `dc.py`, runnable standalone here or vendored into a consumer |
| `bootstrap.sh`, `requirements.txt` | Pinned DuckDB (pip) and Miller versions |

Full behavior and the reasoning behind each rule (why no BLOB columns, why no foreign keys, why
row order matters) is documented in the consuming template's `AGENTS.md`, not duplicated here.

## For consuming repos

Install `git-subrepo` once (it adds a `git subrepo` subcommand; nothing else is affected):
```bash
git clone https://github.com/ingydotnet/git-subrepo /path/to/git-subrepo
echo 'source /path/to/git-subrepo/.rc' >> ~/.bashrc   # or add /path/to/git-subrepo/lib to PATH
```

**One-time, when first adopting this into a repo whose `.tad/` is currently a plain copy** (e.g. a
repo just generated from text-as-data-template) or an existing `git subtree`:
```bash
git rm -r .tad
git commit -m "chore(tad): remove vendored copy before adopting as a git subrepo"
git subrepo clone https://github.com/creation-guidelines/tad-engine.git .tad -b dist \
  -m "chore(tad): vendor engine via git subrepo"
```

**From then on, to pull engine updates:**
```bash
git subrepo pull .tad -m "chore(tad): pull engine update"
```
**Always pass `-m` with a Conventional Commits message.** `git subrepo` accepts `-m`/`--message` on
`clone`/`pull`/`push` and, verified directly, it fully replaces the tool's own auto-generated
message (which otherwise looks like `git subrepo pull .tad` with no type prefix) - nothing about
this is hardcoded, we just weren't using the flag at first. `.gitrepo`, not the commit message,
is what `git subrepo` reads to track state, so a custom message never breaks future pulls. Because
of this, consuming repos should **not** need a commitlint `ignores` rule for subrepo's own commits
at all - if commitlint flags a subrepo commit, that means `-m` was forgotten, and the right fix is
to amend the message, not to add an exemption.
This only stays conflict-free if nothing in `.tad/` was hand-edited downstream (subrepo tracks the
pinned commit in `.tad/.gitrepo` - a plain, readable file, not something buried in commit message
trailers). If your repo needs different engine behavior, change it here and pull the update, rather
than patching the vendored copy in place - a local patch to a vendored file is exactly what turns
the next pull into a merge conflict.

We moved here from `git subtree` after testing both against this exact setup: `git subrepo` writes
one commit per sync instead of subtree's two (a squash commit plus a merge commit), and its
`.gitrepo` file states the tracked remote, branch and pinned commit explicitly instead of relying
on `git log` to find a squash commit's trailer. Neither tool's own auto-generated commit messages
follow Conventional Commits, and that isn't fixable by switching tools - it's inherent to any
sync command that writes its own commit message - so `commitlint.config.js`'s `ignores` list
still needs an entry for `git-subrepo`'s messages (`^git subrepo (clone|pull|push)`).

`dist` is a mirror of this repo's `engine/` folder alone (kept in sync by
[`.github/workflows/dist.yml`](../.github/workflows/dist.yml)) - it exists so your `.tad/` gets only
the engine payload, not this repo's own CI/commit-lint/release files.

To pin to a specific released version instead of the moving `dist`, use `dist/vX.Y.Z` as the
`<ref>` - it points at the same commit `dist` was at when that version was released, so it is
already `engine/`-only:
```bash
git subrepo clone https://github.com/creation-guidelines/tad-engine.git .tad -b dist/v1.0.2 \
  -m "chore(tad): vendor engine via git subrepo (v1.0.2)"
```
This is **not** the same tag as the plain `v1.0.2` release-please cuts on `main` - that one's tree
contains this repo's own governance files (`commitlint.config.js`, `CHANGELOG.md`, ...), exactly
what `dist` exists to avoid, and a same-named tag on `dist` would collide with it (`git`'s tag
namespace is repo-wide, not per-branch). `dist/vX.Y.Z` is a distinct, namespaced tag created right
after each release, pointing at `dist`'s tip at that moment.

## Releases
Commits follow [Conventional Commits](https://www.conventionalcommits.org/) and are linted on
every PR; [release-please](https://github.com/googleapis/release-please) turns them into a
changelog and tagged releases on `main`.

## Status
No license has been chosen yet.
# small doc tweak for subrepo pull test
