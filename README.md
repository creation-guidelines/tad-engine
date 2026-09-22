# tad-engine

Source repository for the "text-as-data" engine. This top level holds this repo's own governance
(CI, commit linting, release automation) - it is **not** what gets vendored into consuming repos.

The actual engine payload lives in [`engine/`](engine/), and is what
[`git subtree`](https://git-scm.com/book/en/v2/Git-Tools-Advanced-Merging#_subtree_merge) pulls
into a consumer's `.tad/` folder - published to a `dist` branch that mirrors `engine/` alone (see
[`.github/workflows/dist.yml`](.github/workflows/dist.yml)), so a consumer's `.tad/` never picks up
this repo's own `commitlint.config.js`, `CHANGELOG.md`, and so on.

**If you're consuming this engine, read [`engine/README.md`](engine/README.md)** for what it does
and the exact `git subtree` commands. This top-level README is for people working on the engine
itself.
