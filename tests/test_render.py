#!/usr/bin/env python3
"""Tests for .tad/tools/render.py. Plain asserts, no framework: `python3 .tad/tests/test_render.py`.

The reStructuredText it writes is validated with docutils when docutils is installed (the engine's CI installs it; a
consumer that only renders does not need it, and the checks that do not need it still run)."""
import glob
import importlib.util
import io
import os
import re
import subprocess
import sys
import tempfile
import traceback

import duckdb

ENGINE_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RENDER = os.path.join(ENGINE_ROOT, "tools", "render.py")

try:
    from docutils.core import publish_doctree
except ImportError:  # validation of the output is skipped, loudly
    publish_doctree = None


def make_project():
    d = tempfile.mkdtemp()
    con = duckdb.connect()
    con.execute("CREATE TABLE concepts(id VARCHAR PRIMARY KEY, name VARCHAR NOT NULL, summary VARCHAR, tags VARCHAR[])")
    con.execute("INSERT INTO concepts VALUES "
                "('alpha_one','Alpha','Uses `code`, a [link](https://example.com/a), **bold** and *emphasis*.',['x','y']),"
                "('beta','Beta: a *starred* name_ with | a pipe',NULL,[]),"
                "('gamma','Gamma','- looks like a list\nsecond line',['z'])")
    con.execute("CREATE TABLE relations(from_id VARCHAR, to_id VARCHAR, relation VARCHAR, note VARCHAR)")
    con.execute("INSERT INTO relations VALUES ('alpha_one','beta','pairs-with','a note with `code`'),('gamma','alpha_one','refines',NULL)")
    con.execute("CREATE TABLE sources(id VARCHAR PRIMARY KEY, title VARCHAR, url VARCHAR)")
    con.execute("INSERT INTO sources VALUES ('s1','The Source','https://example.com/s'),('s2','Another','https://example.com/t')")
    con.execute("CREATE TABLE concept_sources(concept_id VARCHAR, source_id VARCHAR)")
    con.execute("INSERT INTO concept_sources VALUES ('alpha_one','s1'),('alpha_one','s2'),('beta','s1')")
    con.execute("CREATE TABLE log(id VARCHAR PRIMARY KEY, entry_date VARCHAR, title VARCHAR, body VARCHAR)")
    con.execute("INSERT INTO log VALUES ('l1','2026-01-01','日本語の見出し','first'),('l2','2026-01-02','Second','second')")
    cwd = os.getcwd()
    os.chdir(d)
    try:
        con.execute("EXPORT DATABASE 'data' (FORMAT json)")
    finally:
        os.chdir(cwd)
    return d


def render(d, fmt=None, *extra, env_format=None):
    env = dict(os.environ, DC_DIR="data", DOCS_DIR="docs")
    env.pop("DOCS_FORMAT", None)
    if env_format:
        env["DOCS_FORMAT"] = env_format
    args = [sys.executable, RENDER] + (["--format", fmt] if fmt else []) + list(extra)
    return subprocess.run(args, cwd=d, env=env, capture_output=True, text=True)


def read(d, name):
    with open(os.path.join(d, "docs", name), encoding="utf-8") as f:
        return f.read()


def load_render_functions():
    """The converter, imported without running the script (it reads data/ at import, so give it an empty one)."""
    d = tempfile.mkdtemp()
    duckdb.connect().execute(f"CREATE TABLE t(id VARCHAR); EXPORT DATABASE '{d}/data' (FORMAT json)")
    cwd = os.getcwd()
    os.chdir(d)
    try:
        spec = importlib.util.spec_from_file_location("render_under_test", RENDER)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
    finally:
        os.chdir(cwd)
    return module


def test_markdown_is_the_default_and_keeps_its_form():
    d = make_project()
    r = render(d)
    assert r.returncode == 0, r.stderr
    assert sorted(os.listdir(os.path.join(d, "docs"))) == ["concepts.md", "index.md", "log.md", "sources.md"]
    md = read(d, "concepts.md")
    assert md.startswith("<!-- Generated from data/ by .tad/tools/render.py. Do not edit by hand. -->\n\n# Concepts\n")
    assert '<a id="alpha_one"></a>\n### Alpha\n' in md
    assert "Summary\n: Uses `code`, a [link](https://example.com/a), **bold** and *emphasis*.\n" in md
    assert "Relations\n: \u2192 pairs-with: [Beta: a *starred* name_ with | a pipe](concepts.md#beta) \u2014 a note with `code`\n" in md
    assert "Sources\n: [The Source](https://example.com/s), [Another](https://example.com/t)\n" in md
    assert "---\n" in read(d, "log.md"), "a log-shaped table gets a rule between entries"
    assert "- [Concepts](concepts.md): 3" in read(d, "index.md")


def test_the_format_can_be_chosen_by_flag_or_by_environment_and_both_at_once():
    d = make_project()
    assert render(d, "rst").returncode == 0
    assert sorted(os.listdir(os.path.join(d, "docs"))) == ["concepts.rst", "index.rst", "log.rst", "sources.rst"]
    d = make_project()
    assert render(d, env_format="rst").returncode == 0
    assert read(d, "index.rst").startswith(".. Generated from data/")
    d = make_project()
    assert render(d, "md,rst").returncode == 0
    names = sorted(os.listdir(os.path.join(d, "docs")))
    assert names == ["concepts.md", "concepts.rst", "index.md", "index.rst", "log.md", "log.rst", "sources.md", "sources.rst"], names
    assert render(make_project(), "md,md").returncode == 0, "a repeated format is one format"


def test_an_unknown_format_is_refused_and_writes_nothing():
    d = make_project()
    r = render(d, "html")
    assert r.returncode != 0 and "unknown format" in r.stderr and "known: md, rst" in r.stderr, r.stderr
    assert not os.path.exists(os.path.join(d, "docs")), "nothing is written for a format that is not known"


def test_the_output_is_deterministic():
    for fmt in ("md", "rst", "md,rst"):
        d = make_project()
        render(d, fmt)
        first = {f: read(d, f) for f in os.listdir(os.path.join(d, "docs"))}
        render(d, fmt)
        assert first == {f: read(d, f) for f in os.listdir(os.path.join(d, "docs"))}, fmt


def test_the_rst_has_the_structure_of_the_data():
    d = make_project()
    render(d, "rst")
    rst = read(d, "concepts.rst")
    assert rst.startswith(".. Generated from data/ by .tad/tools/render.py. Do not edit by hand.\n\nConcepts\n========\n")
    # an id keeps its meaning but is normalised the way reStructuredText normalises it, in the target and in every link to it
    assert ".. _alpha-one:\n\nAlpha\n-----\n" in rst
    assert "`Beta: a \\*starred\\* name\\_ with \\| a pipe <concepts.rst#beta>`__" in rst
    assert "Summary\n    Uses ``code``, a `link <https://example.com/a>`__, **bold** and *emphasis*.\n" in rst
    assert "Relations\n    - \u2192 pairs-with:" in rst and "\u2014 a note with ``code``" in rst
    assert "Sources\n    `The Source <https://example.com/s>`__, `Another <https://example.com/t>`__\n" in rst
    assert "\n    \\- looks like a list\n    second line\n" in rst, "a value line starting with a dash must not become a list"
    assert "- `Concepts <concepts.rst>`__: 3" in read(d, "index.rst")
    assert "\n----\n" in read(d, "log.rst")


def test_every_link_in_the_rst_points_at_a_target_that_exists():
    d = make_project()
    render(d, "rst")
    targets = {}
    for f in glob.glob(os.path.join(d, "docs", "*.rst")):
        targets[os.path.basename(f)] = set(re.findall(r"^\.\. _([^:]+):$", open(f, encoding="utf-8").read(), re.M))
    checked = 0
    for f in glob.glob(os.path.join(d, "docs", "*.rst")):
        for page, anchor in re.findall(r"<([a-z_]+\.rst)#([^>]+)>", open(f, encoding="utf-8").read()):
            assert anchor in targets[page], f"{f} links to {page}#{anchor}, which does not exist"
            checked += 1
    assert checked >= 4, checked


def test_the_rst_is_valid_without_a_single_warning():
    if publish_doctree is None:
        print("    (skipped: docutils is not installed)")
        return
    d = make_project()
    render(d, "rst")
    for f in sorted(glob.glob(os.path.join(d, "docs", "*.rst"))):
        stream = io.StringIO()
        publish_doctree(open(f, encoding="utf-8").read(), source_path=f,
                        settings_overrides={"report_level": 2, "halt_level": 2, "warning_stream": stream})
        assert stream.getvalue().strip() == "", f"{os.path.basename(f)}: {stream.getvalue()}"


def test_an_underline_covers_a_wide_heading():
    d = make_project()
    render(d, "rst")
    lines = read(d, "log.rst").split("\n")
    i = lines.index("\u65e5\u672c\u8a9e\u306e\u898b\u51fa\u3057")
    assert len(lines[i + 1]) >= 14, "seven wide characters take fourteen columns"


def make_project_with_about():
    d = make_project()
    con = duckdb.connect()
    cwd = os.getcwd()
    os.chdir(d)
    try:
        con.execute("IMPORT DATABASE 'data'")
        con.execute("CREATE TABLE about(title VARCHAR, summary VARCHAR)")
        con.execute("INSERT INTO about VALUES ('The Playbook', 'How it is done, with `code` and a [link](https://example.com/a).')")
        con.execute("EXPORT DATABASE 'data' (FORMAT json)")
    finally:
        os.chdir(cwd)
    return d


def test_an_about_table_heads_the_index_and_is_not_a_page():
    d = make_project_with_about()
    assert render(d, "md,rst").returncode == 0
    names = sorted(os.listdir(os.path.join(d, "docs")))
    assert "about.md" not in names and "about.rst" not in names, names
    md = read(d, "index.md")
    assert md.startswith("<!-- Generated from data/ by .tad/tools/render.py. Do not edit by hand. -->\n\n# The Playbook\n\nHow it is done, with `code` and a [link](https://example.com/a).\n\n- [Concepts](concepts.md): 3\n"), md
    rst = read(d, "index.rst")
    assert rst.startswith(".. Generated from data/ by .tad/tools/render.py. Do not edit by hand.\n\nThe Playbook\n============\n\nHow it is done, with ``code`` and a `link <https://example.com/a>`__.\n\n- `Concepts <concepts.rst>`__: 3\n"), rst
    assert "About" not in md and "About" not in rst, "it is not listed as a page either"


def test_without_an_about_table_the_index_is_headed_index_as_before():
    d = make_project()
    render(d, "md")
    assert read(d, "index.md").startswith("<!-- Generated from data/ by .tad/tools/render.py. Do not edit by hand. -->\n\n# Index\n")


def test_a_table_called_about_without_the_two_columns_is_an_ordinary_table():
    d = make_project()
    con = duckdb.connect()
    cwd = os.getcwd()
    os.chdir(d)
    try:
        con.execute("IMPORT DATABASE 'data'")
        con.execute("CREATE TABLE about(id VARCHAR, name VARCHAR)")
        con.execute("INSERT INTO about VALUES ('a1', 'Not a heading')")
        con.execute("EXPORT DATABASE 'data' (FORMAT json)")
    finally:
        os.chdir(cwd)
    render(d, "md")
    assert os.path.exists(os.path.join(d, "docs", "about.md")) and read(d, "index.md").startswith("<!--") and "# Index" in read(d, "index.md")


def test_the_inline_converter():
    r = load_render_functions()
    c = r.md_inline_to_rst
    assert c("plain text") == "plain text"
    assert c("`code`") == "``code``"
    assert c("[text](https://x.example/y)") == "`text <https://x.example/y>`__"
    assert c("**bold** and *em*") == "**bold** and *em*"
    assert c("a * b ** c") == "a \\* b \\*\\* c", "an asterisk that is not markup is escaped"
    assert c("snake_case and trailing_") == "snake_case and trailing\\_"
    assert c("a | b") == "a \\| b"
    assert c("back\\slash") == "back\\\\slash"
    assert c("`a` then `b`") == "``a`` then ``b``"
    assert c("word`code`") == "word\\ ``code``", "markup that touches a word is separated"
    assert c("`code`word") == "``code``\\ word"
    assert c("- item\n- item") == "\\- item\n\\- item"
    assert c("") == ""


def test_the_anchor_normalisation_matches_what_docutils_makes():
    r = load_render_functions()
    for raw, made in [("alpha_one", "alpha-one"), ("Mixed Case", "mixed-case"), ("a__b--c", "a-b-c"), ("x", "x"), ("--y--", "y")]:
        assert r.rst_id(raw) == made, (raw, r.rst_id(raw))
    if publish_doctree is not None:
        doc = publish_doctree(".. _alpha_one:\n\nHead\n----\n\ntext\n")
        assert "alpha-one" in doc.nameids or "alpha-one" in str(doc.ids), (doc.nameids, doc.ids)


TESTS = [v for k, v in sorted(globals().items()) if k.startswith("test_") and callable(v)]

if __name__ == "__main__":
    failed = 0
    for t in TESTS:
        try:
            t()
            print("ok  ", t.__name__)
        except Exception:
            failed += 1
            print("FAIL", t.__name__)
            traceback.print_exc()
    print(f"{len(TESTS) - failed}/{len(TESTS)} passed")
    sys.exit(1 if failed else 0)
