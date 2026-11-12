# tad

**tad** keeps a small database as plain text in git, with three ways into the same data: **files** you can
read and diff (one JSON object per line, one file per table, so a change to one row is a one-line diff),
**text** you can reshape with jq and Miller, and **SQL** with joins (DuckDB), every change brought back to
the same canonical files. That is tad's whole concern: what is done with the data - pages, the observatory, anything else -
belongs to the tools that use it (rendering still lives here until it moves to a tool of its own:
`2026-11-12-tad-is-text-as-data`). Why, and what was weighed: the research `why-tad-exists` in this repository's data. Each repository
declares its tables as data in `tables.toml`, and tad checks every row against it, keeps the files
in one canonical form, edits a row only when the result still checks out, and renders the data
into documents.

```
tad check                      # every row, and every rule across tables
tad canon                      # rewrite data/ in canonical form (only when it checks out)
tad set TABLE --key ID --set field=value ...   # add or update one row, checked; nothing kept that breaks a rule
tad render [--format md,rst]   # data/ -> docs/: one page per table, Markdown and/or reStructuredText
tad sql "SELECT ..." [--file F]   # ask the data in SQL (DuckDB): a query changes nothing; a change is kept only if every rule holds
tad engine                     # install the SQL engine tad uses: DuckDB's pinned binary, checked by fingerprint
tad declare --from-sql schema.sql   # translate a DuckDB schema into a declaration (refusing what it does not know)
```

## The declaration

`tables.toml` declares each table (after Frictionless Data's [Table Schema](https://specs.frictionlessdata.io/table-schema/)):
its `key`, and its `fields` - each with a `name`, a `type` (`string`, `integer`, `number`, `boolean`,
`date`, `datetime` or `list`), and as needed `required`, `enum`, `pattern`, `starts_with`, `minimum`
and `maximum`. A table may also say which pairs of fields must differ (`distinct`) and which field a
given field requires (`requires`). A table may allow fields it does not name (`extra_fields = true`, for a shared minimum that
projects extend) and name the field a new row is dated by (`dated`, filled with today by `tad set`). Rules across tables: `unique_ids` (ids unique across tables),
`references` (a field names a row of other tables), `appears_in` (every row is cited somewhere) and
`no_mirror` (a symmetric relation is not recorded both ways).

## Rendering

One page per table, one section per row; shapes are found from the data, not from names: a relation
table (`from_id`, `to_id`, `relation`) folds into the pages it connects; a sources table (`title`,
`url`) and two-column citation tables become "Sources" and "Cited by"; a table with a `status` and two
`<x>_id` columns is drawn as a grid; a table named `about` heads the index. Output is deterministic,
so CI can require the documents to be exactly what the data renders to.

## Presentation, declared as data

With nothing declared, every table renders as above. A repository may declare in `tables.toml` how
its pages read, so it never needs a renderer of its own: `[site]` gives the index a `title`, an
`intro`, an `index` order (with `count = { field, open_unless }` for "N open of M"), and
`relation_words` (each relation kind in words, from either end: `serves = ["Serves", "Served by"]`);
`[pages.<table>]` gives a page an `intro`, a `group_by` field with ordered `groups` (and
`skip_empty_groups`), an `order_by`, and what each row `show`s, in order - a `field` (or several
`fields`, joined) as a plain paragraph or under a `label`, its `relations`, its `sources` - or a
`list` layout instead of sections. The software-engineering canon renders this way.

## Using it

A repository installs tad from a pinned release, checking it against the release's checksums
(see `bin/install-tad.sh` in any of the repositories below), and its CI runs `make verify`.
Releases are built by the [package conveyor](https://github.com/linux-package-distribution-oco/conveyor)
for x86_64 and aarch64: tarballs, `.deb` and `.rpm`, with checksums and signed provenance.

A pinned version is kept current by `bin/update-pins.sh`, run weekly on the lab machine: for each repository listed in
`consumers` it moves the pin, re-renders, and lands the change only if that repository's CI passes.

In use by: [linux-phone-oco/docs](https://github.com/linux-phone-oco/docs),
[software-engineering-canon](https://github.com/creation-guidelines/software-engineering-canon),
[ideal-project-playbook](https://github.com/creation-guidelines/ideal-project-playbook) and
[git-rhizoid.github.io](https://github.com/git-rhizoid/git-rhizoid.github.io).

## History

tad began as a Python engine on DuckDB, vendored into each repository. On 2026-11-11 it was rewritten
in Rust and proven equal before any repository switched: the same verdicts on all four repositories
and on sixteen deliberately broken copies, the same canonical bytes, the same edits, and the same 74
documents in both formats, byte for byte. The vendored copies had drifted (three of four were older);
the switch ended that. The Python engine's last version is in this repository's history at
commit 02da2bc (https://github.com/creation-guidelines/tad-engine/tree/02da2bc/engine).
