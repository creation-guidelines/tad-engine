#!/usr/bin/env python3
"""render.py - render data/ (a DuckDB EXPORT DATABASE folder) into documents under docs/, as Markdown, reStructuredText, or both.

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
  - A table with a `status` column and exactly two `<x>_id` columns, each naming another table (x, xs or xes)
    that has ids, is a matrix: its page is a grid - rows from the table that has a `category` (or `group`)
    column, one grid per group, columns from the other - each cell a status mark linked to its details
    (the row's other columns) below the grids. A pair with no row shows as unknown, never as a guess.
  - A table named `about` with (title, summary) columns is not a page: its first row is the heading and the introduction of the
    index page, so the front page's words are data like everything else. Without it the index is headed "Index".

Anything that does not match these shapes still renders: one page, one section per row, one line
per non-empty column. Output is deterministic (no timestamps) so CI can require docs/ to be
unchanged after a re-render.

Output formats (DOCS_FORMAT, or --format; a comma-separated list; default "md"):

  md    Markdown, one `docs/<table>.md` per page. Values in the data are Markdown.
  rst   reStructuredText, one `docs/<table>.rst` per page. The same Markdown values are converted:
        `code`, [text](url), **bold** and *emphasis* become their reStructuredText forms, and every
        other character that reStructuredText would read as markup is escaped, so data written
        once renders correctly in both.

The page content is built once and written by a format object, so a new format is one class.
"""
import os
import re
import sys
import unicodedata

import duckdb

DATA = os.environ.get("DC_DIR", "data")
DOCS = os.environ.get("DOCS_DIR", "docs")


# ---- formats ---------------------------------------------------------------------------------

def _width(text):
    """Terminal columns a string takes (wide East Asian characters take two): what an RST underline must cover."""
    return sum(2 if unicodedata.east_asian_width(c) in ("W", "F") else 1 for c in text)


class Markdown:
    ext = "md"
    notice = "<!-- Generated from data/ by .tad/tools/render.py. Do not edit by hand. -->"

    def text(self, s):  # values are Markdown already
        return s

    def link(self, label, page, rid):
        return f"[{label}]({page}.md#{rid})"

    def url(self, title, url):
        return f"[{title}]({url})"

    def code(self, s):
        return f"`{s}`"

    def title(self, text):
        return [f"# {text}", ""]

    def anchor(self, rid):
        return [f'<a id="{rid}"></a>']

    def heading(self, text):
        return [f"### {text}", ""]

    def field(self, label, value):
        return [label, f": {value}", ""]

    def many(self, label, items):
        return [label] + [f": {i}" for i in items] + [""]

    def rule(self):
        return ["---", ""]

    def index_item(self, label, page, count):
        return f"- [{label}]({page}.md): {count}"

    def table(self, header, rows):
        def cell(v):
            return str(v).replace("|", "\\|").replace("\n", " ")
        out = ["| " + " | ".join(cell(h) for h in header) + " |", "|" + "|".join("---" for _ in header) + "|"]
        return out + ["| " + " | ".join(cell(c) for c in r) + " |" for r in rows] + [""]


# reStructuredText turns an id into a lowercase, hyphen-separated one; links must name that form.
def rst_id(rid):
    return re.sub(r"[^a-z0-9]+", "-", str(rid).lower()).strip("-")


def rst_escape(s):
    """Plain text, made safe against being read as reStructuredText markup."""
    s = s.replace("\\", "\\\\").replace("*", "\\*").replace("`", "\\`").replace("|", "\\|")
    s = re.sub(r"(?<=\w)_(?=\W|$)", r"\\_", s)  # "name_" would be a reference
    return re.sub(r"(?m)^(?=[-+>] )", r"\\", s)  # a line starting "- " would be a list


_INLINE = re.compile(
    r"(?P<code>`(?P<c>[^`]+)`)"
    r"|(?P<link>\[(?P<lt>[^\]]+)\]\((?P<lu>[^)\s]+)\))"
    r"|(?P<bold>\*\*(?P<b>.+?)\*\*)"
    r"|(?P<em>(?<![\w*])\*(?P<e>[^\s*][^*]*?)(?<!\s)\*(?![\w*]))"
)


def md_inline_to_rst(s):
    """The Markdown inline subset a value may hold (code, link, bold, emphasis) in reStructuredText; the rest is escaped."""
    out, pos = [], 0

    def add(markup, raw_follows):
        # inline markup needs a non-word character before and after it; `\ ` is the empty separator
        if out and out[-1] and out[-1][-1].isalnum():
            out.append("\\ ")
        out.append(markup)
        if raw_follows and raw_follows[0].isalnum():
            out.append("\\ ")

    for m in _INLINE.finditer(s):
        out.append(rst_escape(s[pos:m.start()]))
        rest = s[m.end():]
        if m.group("code"):
            add(f"``{m.group('c')}``", rest)
        elif m.group("link"):
            add(f"`{rst_escape(m.group('lt'))} <{m.group('lu')}>`__", rest)
        elif m.group("bold"):
            add(f"**{rst_escape(m.group('b'))}**", rest)
        else:
            add(f"*{rst_escape(m.group('e'))}*", rest)
        pos = m.end()
    out.append(rst_escape(s[pos:]))
    return "".join(out)


def _indented(text, by="    "):
    return [(by + line) if line else "" for line in text.split("\n")]


class Rst:
    ext = "rst"
    notice = ".. Generated from data/ by .tad/tools/render.py. Do not edit by hand."

    def text(self, s):
        return md_inline_to_rst(s)

    def link(self, label, page, rid):
        return f"`{rst_escape(label)} <{page}.rst#{rst_id(rid)}>`__"

    def url(self, title, url):
        return f"`{rst_escape(title)} <{url}>`__"

    def code(self, s):
        return f"``{s}``"

    def _underlined(self, text, char):
        t = md_inline_to_rst(text)
        return [t, char * _width(t), ""]

    def title(self, text):
        return self._underlined(text, "=")

    def anchor(self, rid):
        return [f".. _{rst_id(rid)}:", ""]

    def heading(self, text):
        return self._underlined(text, "-")

    def field(self, label, value):
        return [label] + _indented(value) + [""]

    def many(self, label, items):
        body = []
        for i in items:
            first, *rest = i.split("\n")
            body += ["    - " + first] + ["      " + r for r in rest]
        return [label] + body + [""]

    def rule(self):
        return ["----", ""]

    def index_item(self, label, page, count):
        return f"- `{rst_escape(label)} <{page}.rst>`__: {count}"

    def table(self, header, rows):
        out = [".. list-table::", "   :header-rows: 1", ""]
        for r in [header] + rows:
            out += [("   * - " if i == 0 else "     - ") + str(v).replace("\n", " ") for i, v in enumerate(r)]
        return out + [""]


FORMATS = {"md": Markdown, "rst": Rst}


def chosen_formats(argv, environ):
    names = None
    if "--format" in argv:
        names = argv[argv.index("--format") + 1]
    names = names or environ.get("DOCS_FORMAT", "md")
    out = []
    for name in (n.strip() for n in names.split(",") if n.strip()):
        if name not in FORMATS:
            sys.exit(f"render.py: unknown format {name!r}; known: {', '.join(FORMATS)}")
        if name not in out:
            out.append(name)
    return [FORMATS[n]() for n in out] or [Markdown()]


# ---- the data --------------------------------------------------------------------------------

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
about_table = next((t for t in tables if t == "about" and {"title", "summary"} <= set(table_cols[t])), None)


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
page_tables = [t for t in tables if t not in relation_tables and t not in citation_link_tables and t != about_table]

# ---- matrices: a status between two tables, drawn as a grid -----------------------------------

GROUP_COLUMNS = ("category", "group")
STATUS_MARKS = {
    "works": "\u2705",
    "partial": "\U0001F7E1",
    "broken": "\u274C",
    "missing": "\u274C",
    "unknown": "\u2754",
    "not-applicable": "\u2014",
}


def referenced_table(col):
    """The table an `<x>_id` column points at - x, xs or xes - if it exists and has ids."""
    if not col.endswith("_id"):
        return None
    base = col[:-3]
    return next((t for t in (base, base + "s", base + "es") if t in table_cols and "id" in table_cols[t]), None)


def matrix_shape(t):
    """(row column, row table, column column, column table) when `t` is a matrix, else None."""
    c = table_cols[t]
    refs = [(col, referenced_table(col)) for col in c if referenced_table(col)]
    if "status" not in c or len(refs) != 2:
        return None
    (rc, rt), (cc, ct) = refs

    def grouped(x):
        return any(g in table_cols[x] for g in GROUP_COLUMNS)

    if grouped(ct) and not grouped(rt):
        (rc, rt), (cc, ct) = (cc, ct), (rc, rt)
    return rc, rt, cc, ct


matrix_tables = {t: shape for t in page_tables if (shape := matrix_shape(t))}

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


# ---- the pages, built once and written in each format -----------------------------------------

def link(F, rid):
    if rid not in index:
        return F.code(rid)
    name, page = index[rid]
    return F.link(name, page, rid)


def relations_for(F, rid):
    """One entry per relation, all under one "Relations" term."""
    lines = []
    for rt in relation_tables:
        has_note = "note" in table_cols[rt]
        note_sql = "note" if has_note else "NULL"
        for to_id, rel, note in q(f"SELECT to_id, relation, {note_sql} FROM {rt} WHERE from_id = ? ORDER BY rowid", rid):
            lines.append(f"\u2192 {rel}: {link(F, to_id)}" + (f" \u2014 {F.text(note)}" if note else ""))
        for from_id, rel, note in q(f"SELECT from_id, relation, {note_sql} FROM {rt} WHERE to_id = ? ORDER BY rowid", rid):
            lines.append(f"\u2190 {rel}: {link(F, from_id)}" + (f" \u2014 {F.text(note)}" if note else ""))
    return F.many("Relations", lines) if lines else []


def cited_by_row(F, rid):
    lines = []
    for t, src, cited_col, source_col in citation_links:
        srcs = q(f"SELECT s.title, s.url FROM {t} c JOIN {src} s ON s.id = c.{source_col} "
                 f"WHERE c.{cited_col} = ? ORDER BY s.rowid", rid)
        if srcs:
            lines += F.field("Sources", ", ".join(F.url(title, url) for title, url in srcs))
    return lines


def cites_of_source(F, source_id):
    links = []
    for t, src, cited_col, source_col in citation_links:
        for (cid,) in q(f"SELECT {cited_col} FROM {t} WHERE {source_col} = ? ORDER BY rowid", source_id):
            links.append(link(F, cid))
    return links


def render_table_page(F, t):
    """Each field is a definition (term, then its value) instead of a bold-labeled line: it reads as
    plain indented text with no bold and no font-size jump, close to an RFC's minimal style, while
    each value keeps its own inline markup (links, code) processed normally.

    A log-shaped table (rows meant to be read as a chronological sequence, not a reference list -
    detected structurally by having both an "entry_date" and a "title" column, session_log's
    shape) gets a horizontal rule between entries instead of just blank space, since a log reads
    as a sequence of separate events rather than a set of cross-referenced definitions."""
    c = table_cols[t]
    label_col = "name" if "name" in c else ("title" if "title" in c else ("id" if "id" in c else None))
    is_log = {"entry_date", "title"} <= set(c)
    rows_ = q(f"SELECT * FROM {t} ORDER BY rowid")
    lines = F.title(t.replace("_", " ").title())
    for i, row in enumerate(rows_):
        rec = dict(zip(c, row))
        rid = rec.get("id")
        heading = str(rec[label_col]) if label_col else f"Row {i + 1}"
        if rid is not None:
            lines += F.anchor(rid)
        lines += F.heading(heading)
        for col in c:
            if col in ("id", label_col):
                continue
            val = fmt(rec[col])
            if val:
                lines += F.field(col.replace("_", " ").capitalize(), F.text(val))
        if rid is not None:
            lines += relations_for(F, rid) + cited_by_row(F, rid)
            if t in sources_tables:
                cby = list(dict.fromkeys(cites_of_source(F, rid)))
                if cby:
                    lines += F.field("Cited by", ", ".join(cby))
        if is_log and i < len(rows_) - 1:
            lines += F.rule()
        lines.append("")
    return lines


def render_matrix_page(F, t):
    """A grid per group of rows; each cell a status mark linked to its details below."""
    rc, rt, cc, ct = matrix_tables[t]
    c = table_cols[t]
    group_col = next((g for g in GROUP_COLUMNS if g in table_cols[rt]), None)
    columns = [r[0] for r in q(f"SELECT id FROM {ct} ORDER BY rowid")]
    cells = {}
    for row in q(f"SELECT * FROM {t} ORDER BY rowid"):
        rec = dict(zip(c, row))
        cells[(rec[rc], rec[cc])] = rec
    groups = {}
    for rec in q(f"SELECT id{', ' + group_col if group_col else ''} FROM {rt} ORDER BY rowid"):
        groups.setdefault(rec[1] if group_col else None, []).append(rec[0])

    def label(rid):
        return index[rid][0] if rid in index else str(rid)

    lines = F.title(t.replace("_", " ").title())
    legend = " \u00b7 ".join(f"{m} {s}" for s, m in STATUS_MARKS.items())
    lines += [F.text(f"Rows are {rt.replace('_', ' ')}, columns {ct.replace('_', ' ')}. {legend}. "
                     f"A pair nobody has checked shows {STATUS_MARKS['unknown']}."), ""]
    header = [rt.replace("_", " ").title()] + [link(F, cid) for cid in columns]
    for g, ids in groups.items():
        if g is not None:
            lines += F.heading(str(g).replace("-", " ").capitalize())
        body = []
        for rid in ids:
            row = [link(F, rid)]
            for cid in columns:
                rec = cells.get((rid, cid))
                if rec is None:
                    row.append(STATUS_MARKS["unknown"])
                else:
                    mark = STATUS_MARKS.get(str(rec["status"]), str(rec["status"]))
                    row.append(F.link(mark, t, f"{rid}--{cid}"))
            body.append(row)
        lines += F.table(header, body)
    if cells:
        lines += F.heading("Details")
        for (rid, cid), rec in cells.items():
            parts = [str(rec["status"])] + [
                f"{k.replace('_', ' ')}: {fmt(v)}" for k, v in rec.items() if k not in (rc, cc, "status") and fmt(v)
            ]
            lines += F.anchor(f"{rid}--{cid}") + F.field(f"{label(rid)} \u00d7 {label(cid)}", F.text(" \u2014 ".join(parts)))
    return lines


def write(F, name, lines):
    os.makedirs(DOCS, exist_ok=True)
    with open(f"{DOCS}/{name}.{F.ext}", "w") as f:
        f.write("\n".join([F.notice, ""] + lines).rstrip("\n") + "\n")


def main(argv=None, environ=None):
    formats = chosen_formats(sys.argv[1:] if argv is None else argv, os.environ if environ is None else environ)
    counts = {t: q(f"SELECT count(*) FROM {t}")[0][0] for t in page_tables}
    about = q("SELECT title, summary FROM about ORDER BY rowid LIMIT 1") if about_table else []
    for F in formats:
        for t in page_tables:
            write(F, t, render_matrix_page(F, t) if t in matrix_tables else render_table_page(F, t))
        head = F.title(about[0][0]) + [F.text(about[0][1]), ""] if about else F.title("Index")
        out = head + [F.index_item(t.replace("_", " ").title(), t, counts[t]) for t in page_tables] + [""]
        write(F, "index", out)
    print("rendered " + ", ".join(f"{v} {k}" for k, v in counts.items()) + f" as {', '.join(F.ext for F in formats)}")


if __name__ == "__main__":
    main()
