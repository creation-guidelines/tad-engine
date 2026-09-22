#!/usr/bin/env python3
"""Tests for .tad/tools/dc.py. Plain asserts, no framework: `python3 .tad/tests/test_dc.py`
(or `make test`, which is what CI runs)."""
import json
import os
import subprocess
import sys
import tempfile
import traceback

import duckdb

# Portable whether this file lives at <repo>/tests/test_dc.py (this repo, standalone) or at
# <consumer>/.tad/tests/test_dc.py (vendored into a consumer via `git subtree`): ENGINE_ROOT is
# just "the directory that contains both tools/ and tests/", wherever that happens to be.
ENGINE_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DC = os.path.join(ENGINE_ROOT, "tools", "dc.py")


def make_project():
    d = tempfile.mkdtemp()
    con = duckdb.connect()
    con.execute("CREATE TABLE t(id VARCHAR PRIMARY KEY, name VARCHAR NOT NULL, note VARCHAR, tags VARCHAR[])")
    con.execute("INSERT INTO t VALUES ('a','Alpha','first',['x','y']),('b','Béta \"q\"\nline2',NULL,[])")
    cwd = os.getcwd()
    os.chdir(d)
    try:
        con.execute("EXPORT DATABASE 'data' (FORMAT json)")
    finally:
        os.chdir(cwd)
    os.makedirs(os.path.join(d, "checks"))
    return d


def dc(d, *args):
    env = dict(os.environ, DC_DIR="data", DC_CHECKS="checks")
    return subprocess.run([sys.executable, DC, *args], cwd=d, env=env, capture_output=True, text=True)


def read(d, name):
    return open(os.path.join(d, "data", name), "rb").read()


def lines(d, name):
    return read(d, name).decode().splitlines()


def test_clean_check_passes():
    d = make_project()
    r = dc(d, "check")
    assert r.returncode == 0 and "check ok" in r.stdout, r.stdout + r.stderr


def test_unknown_key_is_caught():
    d = make_project()
    ls = lines(d, "t.json")
    obj = json.loads(ls[0]); obj["surprise"] = 1
    open(f"{d}/data/t.json", "w").write("\n".join([json.dumps(obj)] + ls[1:]) + "\n")
    r = dc(d, "check")
    assert r.returncode == 1 and "surprise" in r.stdout, r.stdout


def test_vanished_key_is_caught():
    d = make_project()
    out = []
    for l in lines(d, "t.json"):
        o = json.loads(l); o["remark"] = o.pop("note"); out.append(json.dumps(o))
    open(f"{d}/data/t.json", "w").write("\n".join(out) + "\n")
    r = dc(d, "check")
    assert r.returncode == 1 and "'note'" in r.stdout and "'remark'" in r.stdout, r.stdout


def test_canon_is_a_noop_on_canonical_data():
    d = make_project()
    before = read(d, "t.json"), read(d, "schema.sql"), read(d, "load.sql")
    assert dc(d, "canon").returncode == 0
    assert (read(d, "t.json"), read(d, "schema.sql"), read(d, "load.sql")) == before


def test_canon_normalizes_formatting():
    d = make_project()
    original = read(d, "t.json")
    reformatted = "\n".join(json.dumps(json.loads(l), separators=(", ", ": "), ensure_ascii=True) for l in lines(d, "t.json")) + "\n"
    assert reformatted.encode() != original
    open(f"{d}/data/t.json", "w").write(reformatted)
    assert dc(d, "canon").returncode == 0
    assert read(d, "t.json") == original


def test_failed_import_leaves_files_untouched():
    d = make_project()
    before = read(d, "t.json")
    s = open(f"{d}/data/schema.sql").read().replace("PRIMARY KEY,", "PRIMARY KEY CHECK (length(id) > 5),", 1)
    open(f"{d}/data/schema.sql", "w").write(s)
    r = dc(d, "canon")
    assert r.returncode != 0, "canon should fail when data violates a constraint"
    assert read(d, "t.json") == before and not os.path.exists(f"{d}/data.bak")


def test_canon_removes_stale_files():
    d = make_project()
    open(f"{d}/data/stale.json", "w").write("{}\n")
    assert dc(d, "canon").returncode == 0
    assert not os.path.exists(f"{d}/data/stale.json")


def test_sql_runs_and_exports():
    d = make_project()
    r = dc(d, "sql", "UPDATE t SET name = 'Alpha2' WHERE id = 'a'; SELECT count(*) FROM t")
    assert r.returncode == 0 and "(2,)" in r.stdout, r.stdout + r.stderr
    assert json.loads(lines(d, "t.json")[0])["name"] == "Alpha2"


def test_sql_handles_semicolons_inside_strings():
    d = make_project()
    r = dc(d, "sql", "UPDATE t SET note = 'one; two' WHERE id = 'a'; SELECT note FROM t WHERE id = 'a'")
    assert r.returncode == 0 and "one; two" in r.stdout, r.stdout + r.stderr
    assert json.loads(lines(d, "t.json")[0])["note"] == "one; two"


def test_blob_column_is_caught():
    d = make_project()
    s = open(f"{d}/data/schema.sql").read().replace("note VARCHAR,", "note BLOB,", 1)
    open(f"{d}/data/schema.sql", "w").write(s)
    r = dc(d, "check")
    assert r.returncode == 1 and "BLOB" in r.stdout and "base64" in r.stdout, r.stdout


def test_checks_fail_on_violation():
    d = make_project()
    open(f"{d}/checks/01.sql", "w").write("SELECT id FROM t WHERE id = 'a';")
    r = dc(d, "checks")
    assert r.returncode == 1 and "FAIL" in r.stdout, r.stdout
    open(f"{d}/checks/01.sql", "w").write("SELECT id FROM t WHERE id = 'zzz';")
    assert dc(d, "checks").returncode == 0


def test_large_table_roundtrip_is_byte_stable_and_updates_are_one_line():
    """Row order must not depend on threads/cores: canon is a no-op and one UPDATE changes one line."""
    d = tempfile.mkdtemp()
    con = duckdb.connect()
    con.execute("CREATE TABLE big(id INTEGER PRIMARY KEY, name VARCHAR NOT NULL, body VARCHAR, tags VARCHAR[])")
    con.execute("INSERT INTO big SELECT i, 'name-' || i, repeat('x', 100) || i, ['a', 'b' || (i % 7)] FROM range(300000) r(i)")
    cwd = os.getcwd()
    os.chdir(d)
    try:
        con.execute("EXPORT DATABASE 'data' (FORMAT json)")
    finally:
        os.chdir(cwd)
    os.makedirs(os.path.join(d, "checks"))
    before = read(d, "big.json")
    assert dc(d, "canon").returncode == 0
    assert read(d, "big.json") == before, "canon changed bytes on a 300k-row table"
    assert dc(d, "sql", "UPDATE big SET name = 'changed' WHERE id = 123456").returncode == 0
    a, b = before.decode().splitlines(), read(d, "big.json").decode().splitlines()
    assert len(a) == len(b) and sum(x != y for x, y in zip(a, b)) == 1, "one UPDATE must change exactly one line"
    print("     (cores: %s)" % os.cpu_count())


if __name__ == "__main__":
    failed = 0
    for name, fn in sorted((n, f) for n, f in globals().items() if n.startswith("test_")):
        try:
            fn()
            print("ok  ", name)
        except Exception:
            failed += 1
            print("FAIL", name)
            traceback.print_exc()
    sys.exit(1 if failed else 0)
