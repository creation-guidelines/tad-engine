# tad-engine

The reusable "text-as-data" engine: guard, canonicalize, and query a small dataset kept as a
DuckDB `EXPORT DATABASE` folder (a `schema.sql`, a `load.sql`, one JSON-lines file per table), plus
a schema-agnostic Markdown renderer. It knows nothing about your schema, table names, or subject
matter - it's the plumbing, not the content.

This is the engine extracted from [creation-guidelines/text-as-data-template](https://github.com/creation-guidelines/text-as-data-template).
Consuming repos vendor it as a `git subtree` at `.tad/`, so engine fixes and features can flow into
them with a normal `git subtree pull` - not a manual re-copy - as long as `.tad/` is never
hand-edited downstream (see "For consuming repos" below).

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

**One-time, when first adopting this into a repo whose `.tad/` is currently a plain copy** (e.g. a
repo just generated from text-as-data-template):
```bash
git rm -r .tad
git commit -m "chore(tad): remove vendored copy before adopting as a git subtree"
git subtree add --prefix=.tad https://github.com/creation-guidelines/tad-engine.git dist --squash
```

**From then on, to pull engine updates:**
```bash
git subtree pull --prefix=.tad https://github.com/creation-guidelines/tad-engine.git dist --squash
```
This only stays conflict-free if nothing in `.tad/` was hand-edited downstream. If your repo needs
different engine behavior, change it here and pull the update, rather than patching the vendored
copy in place - a local patch to a vendored file is exactly what turns the next pull into a merge
conflict.

`dist` is a mirror of this repo's `engine/` folder alone (kept in sync by
[`.github/workflows/dist.yml`](../.github/workflows/dist.yml)) - it exists so your `.tad/` gets only
the engine payload, not this repo's own CI/commit-lint/release files.

There is currently no way to pin to a specific released version: `git subtree` always imports the
*entire* tree at the ref you give it, so pointing at a tag on `main` (which also contains this
repo's governance files) would reintroduce the clutter `dist` exists to avoid, and `dist` itself
has no tags of its own yet. For now, `dist` (a moving target, tracking whatever last merged to
`main`) is the only supported source. If pinned versions turn out to matter, the fix is to also tag
`dist` at each release, not to point at a tag on `main`.

## Releases
Commits follow [Conventional Commits](https://www.conventionalcommits.org/) and are linted on
every PR; [release-please](https://github.com/googleapis/release-please) turns them into a
changelog and tagged releases on `main`.

## Status
No license has been chosen yet.
