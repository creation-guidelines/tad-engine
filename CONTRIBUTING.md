# Contributing

## Commit messages: Conventional Commits

Every commit on a pull request is linted (`commitlint`, PR check) against
[Conventional Commits](https://www.conventionalcommits.org/): `type(scope): subject`. This is not
decorative - [release-please](.github/workflows/release.yml) reads these commits to decide the
next version number and to write `CHANGELOG.md`, so the type you pick has a real effect:

| Type | Use for | Version bump |
|---|---|---|
| `feat` | New content, a new table, a new column, a new capability | minor |
| `fix` | A correction: wrong fact, wrong constraint, a bug in `.tad/` | patch |
| `docs` | Hand-written docs only (README, this file, AGENTS.md) - not the generated `docs/` pages, which travel with the `feat`/`fix` commit that changed the data | patch, no changelog entry by default |
| `refactor` | Reshaping data or tooling with no behavior or content change | patch, no changelog entry |
| `chore` | Housekeeping: dependency bumps, formatting | no bump, no changelog entry |
| `ci` | Workflow changes | no bump, no changelog entry |
| `test` | Test-only changes | no bump, no changelog entry |

A breaking change (a schema change that other consumers of this data would need to react to) adds
`!` after the type/scope (`fix(schema)!: ...`) or a `BREAKING CHANGE:` footer, and bumps major.

Scope is optional and not enforced, but this repo's own convention is to name the part that
changed: `data`, `schema`, `tools`, `docs`, `checks`, `ci`. For example:

```
feat(data): add three new entries with sources
fix(data): correct the year on an existing entry
feat(schema): add an aliases column to concepts
fix(schema): require an author whenever a year is given
fix(tools): dc.py split SQL on ';' inside string literals
chore(deps): bump duckdb
```

## Workflow

See [`AGENTS.md`](AGENTS.md) for the day-to-day recipes (`make sql`, schema changes, when to reach
for Miller). In short: change data through SQL, never hand-edit `data/*.json` or `docs/*.md`, and
finish with `make verify` before opening a PR.
