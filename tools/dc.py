#!/usr/bin/env python3
"""dc.py - guard, canonicalize and query a DuckDB EXPORT DATABASE (FORMAT json) folder.

  dc.py check         fail if a JSONL line has keys the schema does not know, or a schema
                      column appears on no line (DuckDB's COPY FROM json silently ignores the
                      first and NULL-fills the second)
  dc.py canon         check -> IMPORT (validates types and constraints) -> re-EXPORT into the
                      same relative folder, so DuckDB writes every byte and no stale file stays
  dc.py sql "SQL"     IMPORT, run the statements (rows are printed), then canonicalize
  dc.py checks        run checks/*.sql against the imported data; each query must return no rows

This is engine code (.tad/): generic across any schema. Run it via `make <target>` from the
repository root, not directly - the Makefile sets DC_DIR/DC_CHECKS. The folder is $DC_DIR
(default: data), the invariant queries are in $DC_CHECKS (default: checks).
"""
import glob
import os
import re
import shutil
import sys

import duckdb

DIR = os.environ.get("DC_DIR", "data")
CHECKS = os.environ.get("DC_CHECKS", "checks")


def tables_from_load_sql(d):
    """table -> data file, as declared by load.sql (the authority on what is loaded)."""
    text = open(f"{d}/load.sql").read()
    return {
        m.group(1).strip('"'): os.path.join(d, os.path.basename(m.group(2)))
        for m in re.finditer(r"COPY\s+(\"?\w+\"?)\s+FROM\s+'([^']+)'", text)
    }


def check(d):
    con = duckdb.connect()
    con.execute(open(f"{d}/schema.sql").read())
    problems = []
    for table, path in sorted(tables_from_load_sql(d).items()):
        cols = {r[0] for r in con.execute(
            "SELECT column_name FROM information_schema.columns WHERE table_name = ?", [table]).fetchall()}
        blob_cols = {r[0] for r in con.execute(
            "SELECT column_name FROM information_schema.columns "
            "WHERE table_name = ? AND data_type = 'BLOB'", [table]).fetchall()}
        for c in sorted(blob_cols):
            problems.append(f"{table}: column '{c}' is BLOB - this corrupts on the first JSON export/import "
                             f"round trip; use a base64 VARCHAR instead")
        keys = {r[0] for r in con.execute(
            "SELECT DISTINCT unnest(json_keys(json)) FROM read_json_objects(?, format='newline_delimited')",
            [path]).fetchall()}
        for k in sorted(keys - cols):
            problems.append(f"{table}: key '{k}' is in the data but not in the schema (it would be silently dropped)")
        for k in sorted(cols - keys):
            problems.append(f"{table}: column '{k}' is in the schema but on no line of data (it would load as NULL or the default)")
    return problems


def load(d):
    con = duckdb.connect()
    con.execute(f"IMPORT DATABASE '{d}'")
    return con


def canon(d, statements=None):
    problems = check(d)
    if problems:
        print("\n".join("CHECK FAILED: " + p for p in problems))
        sys.exit(2)
    con = load(d)
    for stmt in con.extract_statements(statements or ""):  # DuckDB's own splitter: ';' inside strings is safe
        res = con.execute(stmt)
        try:
            for row in res.fetchall():
                print(row)
        except duckdb.Error:
            pass
    bak = d + ".bak"
    shutil.rmtree(bak, ignore_errors=True)
    os.rename(d, bak)  # keep the old folder until the export has succeeded
    try:
        con.execute(f"EXPORT DATABASE '{d}' (FORMAT json)")
    except Exception:
        shutil.rmtree(d, ignore_errors=True)
        os.rename(bak, d)
        raise
    shutil.rmtree(bak)


def run_checks(d):
    con = load(d)
    failed = 0
    for path in sorted(glob.glob(f"{CHECKS}/*.sql")):
        rows = con.execute(open(path).read()).fetchall()
        if rows:
            failed += 1
            print(f"FAIL {path}")
            for r in rows:
                print("   ", r)
        else:
            print(f"ok   {path}")
    return failed


def main(argv):
    if len(argv) < 2 or argv[1] not in ("check", "canon", "sql", "checks"):
        sys.exit(__doc__)
    cmd = argv[1]
    if cmd == "check":
        problems = check(DIR)
        print("\n".join(problems) if problems else "check ok")
        return 1 if problems else 0
    if cmd == "canon":
        canon(DIR)
        print("canonicalized", DIR)
        return 0
    if cmd == "sql":
        if len(argv) < 3:
            sys.exit('usage: dc.py sql "SQL"')
        canon(DIR, argv[2])
        return 0
    return 1 if run_checks(DIR) else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
