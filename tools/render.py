#!/usr/bin/env python3
"""render.py - render data/ (a DuckDB EXPORT DATABASE folder) into Markdown pages under docs/.

Engine code (.tad/): schema-agnostic, works on any table shape with no configuration file.
One page per table, one section per row. Two structural conventions unlock nicer output, detected
automatically rather than by table or column naming:

  - A table with columns (from_id, to_id, relation[, note]) is treated as a relation table: it is
    not given its own page, and instead shows up as "-> relation [target]" / "<- relation [source]"
    lines on the pages of whatever it connects (any table with an 'id' column).
  - A table with (title, url) columns is treated as a sources table. A two-column join table is
    treated as citations if one of its columns holds only values that exist as ids in a sources
    table (checked by content, not by name) - "Sources: ..." is added to the cited row, "Cited by:"
    to the source's own row.

Anything that does not match these shapes still renders: one page, one section per row, one line
per non-empty column. Output is deterministic (no timestamps) so CI can require docs/ to be
unchanged after a re-render.
"""
import os

import duckdb

DATA = os.environ.get("DC_DIR", "data")
DOCS = os.environ.get("DOCS_DIR", "docs")
GENERATED = "<!-- Generated from data/ by .tad/tools/render.py. Do not edit by hand. -->"

con = duckdb.connect()
con.execute(f"IMPORT DATABASE '{DATA}'")


def cols(table):
    return [r[0] for r in con.execute(
        "SELECT column_name FROM information_schema.columns WHERE table_name = ? ORDER BY ordinal_position",
        [table]).fetchall()]


def q(sql, *params):
    return con.execute(sql, list(params)).fetchall()


tables = [r[0] for r in q(
    "SELECT table_name FROM information_schema.tables WHERE table_type = 'BASE TABLE' ORDER BY table_name")]
table_cols = {t: cols(t) for t in tables}
relation_tables = [t for t in tables if {"from_id", "to_id", "relation"} <= set(table_cols[t])]
sources_tables = [t for t in tables if {"title", "url"} <= set(table_cols[t])]


def find_citation_columns(t, src_table):
    """Structural detection: which of t's two id-like columns holds only ids that exist in
    src_table? That column is the source side; the other is whatever gets cited."""
    if len(table_cols[t]) != 2:
        return None
    src_ids = {r[0] for r in q(f"SELECT id FROM {src_table}")}
    c0, c1 = table_cols[t]
    v0 = {r[0] for r in q(f"SELECT DISTINCT {c0} FROM {t}")}
    v1 = {r[0] for r in q(f"SELECT DISTINCT {c1} FROM {t}")}
    if v0 and v0 <= src_ids and not (v1 <= src_ids):
        return c1, c0  # (cited_column, source_column)
    if v1 and v1 <= src_ids and not (v0 <= src_ids):
        return c0, c1
    return None


citation_links = [
    (t, src, *cols_) for t in tables if t not in relation_tables and t not in sources_tables
    for src in sources_tables for cols_ in [find_citation_columns(t, src)] if cols_
]
citation_link_tables = {t for t, *_ in citation_links}
# Relation tables and pure citation-link (join) tables are metadata, not content: fold into the
# pages of what they connect instead of getting a page of their own.
page_tables = [t for t in tables if t not in relation_tables and t not in citation_link_tables]

# id -> (display name, page) for any row of any page table that has an 'id' column
index = {}
for t in page_tables:
    c = table_cols[t]
    if "id" not in c:
        continue
    label_col = "name" if "name" in c else ("title" if "title" in c else "id")
    for row in q(f"SELECT * FROM {t} ORDER BY rowid"):
        rec = dict(zip(c, row))
        index[rec["id"]] = (str(rec[label_col]), t)


def link(rid):
    if rid not in index:
        return f"`{rid}`"
    name, page = index[rid]
    return f"[{name}]({page}.md#{rid})"


def fmt(v):
    if v is None:
        return None
    if isinstance(v, list):
        return ", ".join(x for x in (fmt(i) for i in v) if x) or None
    if isinstance(v, dict):
        return ", ".join(f"{k}: {x}" for k, x in ((k, fmt(x)) for k, x in v.items()) if x) or None
    if isinstance(v, bool):
        return "yes" if v else "no"
    s = str(v)
    return s if s else None


def relations_for(rid):
    """Kramdown definition-list lines: one ": " line per relation, all under one "Relations" term -
    see render_table_page's docstring note for why definition lists instead of bold labels."""
    lines = []
    for rt in relation_tables:
        has_note = "note" in table_cols[rt]
        note_sql = "note" if has_note else "NULL"
        for to_id, rel, note in q(f"SELECT to_id, relation, {note_sql} FROM {rt} WHERE from_id = ? ORDER BY rowid", rid):
            lines.append(f": \u2192 {rel}: {link(to_id)}" + (f" \u2014 {note}" if note else ""))
        for from_id, rel, note in q(f"SELECT from_id, relation, {note_sql} FROM {rt} WHERE to_id = ? ORDER BY rowid", rid):
            lines.append(f": \u2190 {rel}: {link(from_id)}" + (f" \u2014 {note}" if note else ""))
    return (["Relations"] + lines + [""]) if lines else []


def cited_by_row(rid):
    lines = []
    for t, src, cited_col, source_col in citation_links:
        srcs = q(f"SELECT s.title, s.url FROM {t} c JOIN {src} s ON s.id = c.{source_col} "
                 f"WHERE c.{cited_col} = ? ORDER BY s.rowid", rid)
        if srcs:
            lines += ["Sources", ": " + ", ".join(f"[{title}]({url})" for title, url in srcs), ""]
    return lines


def cites_of_source(source_id):
    links = []
    for t, src, cited_col, source_col in citation_links:
        for (cid,) in q(f"SELECT {cited_col} FROM {t} WHERE {source_col} = ? ORDER BY rowid", source_id):
            links.append(link(cid))
    return links


def render_table_page(t):
    """Each field is a kramdown definition list (Term / : Definition) instead of a bold-labeled
    line - GitHub Pages' default markdown engine (kramdown) renders that as <dl><dt><dd>, which
    reads as plain indented text with no bold and no font-size jump, closer to an RFC's minimal
    style, while still keeping every value's own markdown (links, etc.) processed normally."""
    c = table_cols[t]
    label_col = "name" if "name" in c else ("title" if "title" in c else ("id" if "id" in c else None))
    lines = [f"# {t.replace('_', ' ').title()}", ""]
    for i, row in enumerate(q(f"SELECT * FROM {t} ORDER BY rowid")):
        rec = dict(zip(c, row))
        rid = rec.get("id")
        heading = str(rec[label_col]) if label_col else f"Row {i + 1}"
        if rid is not None:
            lines.append(f'<a id="{rid}"></a>')
        lines += [f"### {heading}", ""]
        for col in c:
            if col in ("id", label_col):
                continue
            val = fmt(rec[col])
            if val:
                lines.append(col.replace('_', ' ').capitalize())
                lines.append(f": {val}")
                lines.append("")
        if rid is not None:
            rel = relations_for(rid)
            cite = cited_by_row(rid)
            lines += rel + cite
            if t in sources_tables:
                cby = list(dict.fromkeys(cites_of_source(rid)))
                if cby:
                    lines += ["Cited by", ": " + ", ".join(cby), ""]
        lines.append("")
    return lines


def write(name, lines):
    os.makedirs(DOCS, exist_ok=True)
    with open(f"{DOCS}/{name}", "w") as f:
        f.write("\n".join([GENERATED, ""] + lines).rstrip("\n") + "\n")


for t in page_tables:
    write(f"{t}.md", render_table_page(t))

counts = {t: q(f"SELECT count(*) FROM {t}")[0][0] for t in page_tables}
out = ["# Index", ""] + [f"- [{t.replace('_', ' ').title()}]({t}.md): {counts[t]}" for t in page_tables] + [""]
write("index.md", out)
print("rendered " + ", ".join(f"{v} {k}" for k, v in counts.items()))
