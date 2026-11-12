//! Asking the data in SQL: the third way into a tad dataset, beside its files and text tools. The
//! declared tables are loaded into a SQL engine, the statement runs, and the tables come back through
//! tad's canonical form and checks - a query changes nothing; a change is kept only if every rule
//! still holds. The engine is whatever answers `SqlEngine`: today DuckDB's standalone binary.
use crate::{canonical_line, check, Declaration, Field, Table};
use anyhow::{bail, Context, Result};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// A SQL engine tad can hand a script to: it runs the script and returns what it printed.
pub trait SqlEngine {
    fn run(&self, script: &str, format: &str) -> Result<String>;
}

/// DuckDB's standalone binary: named in TAD_DUCKDB, or `duckdb` on the PATH.
pub struct DuckDb {
    pub binary: PathBuf,
}

/// The DuckDB tad installs for itself: a pinned release, checked by fingerprint before it is trusted.
pub const DUCKDB_VERSION: &str = "1.5.5";
/// (this machine's architecture, DuckDB's name for it, the SHA-256 of its archive)
const DUCKDB_ARCHIVES: [(&str, &str, &str); 2] = [
    (
        "x86_64",
        "amd64",
        "08c0ca117111fcede14239d0093792352befdc174218c344d232c13279643d05",
    ),
    (
        "aarch64",
        "arm64",
        "02163197027a42149147364d31fa67cac82108517a4be43304a1cc226eaef07a",
    ),
];

/// Where tad keeps what it installs for itself: TAD_HOME, else the user's data folder.
fn home() -> PathBuf {
    if let Some(h) = std::env::var_os("TAD_HOME").filter(|h| !h.is_empty()) {
        return h.into();
    }
    if let Some(x) = std::env::var_os("XDG_DATA_HOME").filter(|x| !x.is_empty()) {
        return PathBuf::from(x).join("tad");
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/tad")
}

/// Where tad's own DuckDB lives once installed.
pub fn engine_path() -> PathBuf {
    home()
        .join(format!("duckdb-{DUCKDB_VERSION}"))
        .join("duckdb")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Installs DuckDB's pinned standalone binary for this machine into tad's own folder: downloaded,
/// checked against the fingerprint recorded here, and only then unpacked.
pub fn install_engine() -> Result<PathBuf> {
    use sha2::{Digest, Sha256};
    let (_, arch, fingerprint) = DUCKDB_ARCHIVES
        .iter()
        .find(|(machine, ..)| *machine == std::env::consts::ARCH)
        .with_context(|| format!("no pinned DuckDB for {}", std::env::consts::ARCH))?;
    let path = engine_path();
    let dir = path.parent().context("tad's folder")?.to_path_buf();
    std::fs::create_dir_all(&dir)?;
    let archive = dir.join("duckdb.zip");
    let url = format!("https://github.com/duckdb/duckdb/releases/download/v{DUCKDB_VERSION}/duckdb_cli-linux-{arch}.zip");
    let run = |program: &str, args: &[&str]| -> Result<()> {
        let ok = std::process::Command::new(program)
            .args(args)
            .status()
            .with_context(|| format!("{program} is needed to install DuckDB"))?
            .success();
        if !ok {
            bail!("{program} failed");
        }
        Ok(())
    };
    run("curl", &["-fsSL", "-o", &archive.to_string_lossy(), &url])?;
    let got = hex(Sha256::digest(std::fs::read(&archive)?).as_slice());
    if got != *fingerprint {
        let _ = std::fs::remove_file(&archive);
        bail!("DuckDB's download does not match its pinned fingerprint - nothing installed");
    }
    run(
        "unzip",
        &[
            "-q",
            "-o",
            &archive.to_string_lossy(),
            "-d",
            &dir.to_string_lossy(),
        ],
    )?;
    let _ = std::fs::remove_file(&archive);
    if !path.is_file() {
        bail!("the archive held no duckdb");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(path)
}

impl DuckDb {
    /// The engine to use: the one named in TAD_DUCKDB, else tad's own, else `duckdb` on the PATH.
    pub fn find() -> Result<DuckDb> {
        if let Some(p) = std::env::var_os("TAD_DUCKDB").filter(|p| !p.is_empty()) {
            return Ok(DuckDb { binary: p.into() });
        }
        let own = engine_path();
        if own.is_file() {
            return Ok(DuckDb { binary: own });
        }
        std::env::var_os("PATH")
            .and_then(|paths| std::env::split_paths(&paths).map(|d| d.join("duckdb")).find(|p| p.is_file()))
            .map(|binary| DuckDb { binary })
            .with_context(|| format!("tad sql needs DuckDB: run `tad engine` to install its pinned binary (v{DUCKDB_VERSION}), or name one in TAD_DUCKDB"))
    }
}

impl SqlEngine for DuckDb {
    fn run(&self, script: &str, format: &str) -> Result<String> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new(&self.binary)
            .arg("-bail")
            .arg(format!("-{format}"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("starting {}", self.binary.display()))?;
        child
            .stdin
            .take()
            .context("the engine takes no script")?
            .write_all(script.as_bytes())?;
        let out = child.wait_with_output()?;
        if !out.status.success() {
            bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

fn sql_type(f: &Field) -> &'static str {
    match f.ty.as_str() {
        "integer" => "BIGINT",
        "number" => "DOUBLE",
        "boolean" => "BOOLEAN",
        "date" => "DATE",
        "list" => "VARCHAR[]",
        _ => "VARCHAR",
    }
}

fn name(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn text(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The script that makes every declared table in the engine and fills it from its file.
pub fn load_script(dir: &Path, d: &Declaration) -> String {
    let mut s = String::new();
    for (table, t) in &d.tables {
        let columns: Vec<String> = t
            .fields
            .iter()
            .map(|f| format!("{} {}", name(&f.name), sql_type(f)))
            .collect();
        s.push_str(&format!(
            "CREATE TABLE {} ({});\n",
            name(table),
            columns.join(", ")
        ));
        let file = dir.join(format!("{table}.json"));
        if std::fs::metadata(&file)
            .map(|m| m.len() > 0)
            .unwrap_or(false)
        {
            let spec: Vec<String> = t
                .fields
                .iter()
                .map(|f| format!("{}: {}", text(&f.name), text(sql_type(f))))
                .collect();
            s.push_str(&format!(
                "INSERT INTO {} SELECT * FROM read_json({}, format = 'newline_delimited', columns = {{{}}});\n",
                name(table),
                text(&file.to_string_lossy()),
                spec.join(", ")
            ));
        }
    }
    s
}

/// A whole number the engine gave back as a decimal (5.0) is written as it was (5).
fn settle_numbers(t: &Table, row: &mut Map<String, Value>) {
    for f in t.fields.iter().filter(|f| f.ty == "number") {
        if let Some(n) = row.get(&f.name).and_then(Value::as_f64) {
            if n.fract() == 0.0
                && n.abs() < 9.0e15
                && row.get(&f.name).is_some_and(|v| !v.is_i64() && !v.is_u64())
            {
                row.insert(f.name.clone(), Value::from(n as i64));
            }
        }
    }
}

fn canonical_text(t: &Table, text: &str, settle: bool) -> Result<String> {
    let mut out = String::new();
    for l in text.lines().filter(|l| !l.trim().is_empty()) {
        let Value::Object(mut row) =
            serde_json::from_str::<Value>(l).context("a line is not a JSON object")?
        else {
            bail!("a line is not a JSON object");
        };
        if settle {
            settle_numbers(t, &mut row);
        }
        out.push_str(&canonical_line(t, &row));
        out.push('\n');
    }
    Ok(out)
}

/// What a statement did: what the engine printed, and the tables it changed.
#[derive(Debug)]
pub struct Outcome {
    pub printed: String,
    pub changed: Vec<String>,
}

/// Runs one SQL statement over the declared tables in `dir`. A query changes nothing. A change comes
/// back in canonical form and is kept only if every row and rule still checks out; otherwise every
/// file is put back and the problems are named. A table that may carry fields the declaration does
/// not name cannot be changed this way: the engine would silently drop them.
pub fn sql(
    dir: &Path,
    d: &Declaration,
    statement: &str,
    engine: &dyn SqlEngine,
    format: &str,
) -> Result<Outcome> {
    let statement = statement.trim().trim_end_matches(';');
    if statement.is_empty() {
        bail!("give a SQL statement");
    }
    // a scratch folder of this call's own: two calls never share one
    static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let scratch = std::env::temp_dir().join(format!("tad-sql-{}-{call}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch)?;
    let mut script = load_script(dir, d);
    script.push_str(statement);
    script.push_str(";\n");
    for table in d.tables.keys() {
        script.push_str(&format!(
            "COPY {} TO {} (FORMAT json);\n",
            name(table),
            text(&scratch.join(format!("{table}.json")).to_string_lossy())
        ));
    }
    let printed = engine.run(&script, format);
    let result = printed.and_then(|printed| keep(dir, d, &scratch, printed));
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

fn keep(dir: &Path, d: &Declaration, scratch: &Path, printed: String) -> Result<Outcome> {
    let mut changes: Vec<(String, PathBuf, String, String)> = Vec::new();
    for (table, t) in &d.tables {
        let file = dir.join(format!("{table}.json"));
        let old = std::fs::read_to_string(&file).unwrap_or_default();
        let back =
            std::fs::read_to_string(scratch.join(format!("{table}.json"))).unwrap_or_default();
        let new = canonical_text(t, &back, true)
            .with_context(|| format!("{table}: what the engine gave back"))?;
        let was = canonical_text(t, &old, false).with_context(|| format!("{table}: its file"))?;
        if new == was {
            continue;
        }
        if t.extra_fields {
            bail!("nothing changed - {table} may carry fields the declaration does not name, which SQL would drop: change it with tad set");
        }
        changes.push((table.clone(), file, old, new));
    }
    for (_, file, _, new) in &changes {
        std::fs::write(file, new).with_context(|| format!("writing {}", file.display()))?;
    }
    if !changes.is_empty() {
        let (_, problems) = check(dir, d)?;
        if !problems.is_empty() {
            for (_, file, old, _) in &changes {
                std::fs::write(file, old)?;
            }
            bail!(
                "nothing changed - this would break {} rule(s):\n{}",
                problems.len(),
                problems.join("\n")
            );
        }
    }
    Ok(Outcome {
        printed,
        changed: changes.into_iter().map(|(t, ..)| t).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A stand-in engine: it writes what each table "comes back as" where the script says to copy it.
    struct StandIn(HashMap<&'static str, &'static str>);

    impl SqlEngine for StandIn {
        fn run(&self, script: &str, _format: &str) -> Result<String> {
            for l in script.lines().filter(|l| l.starts_with("COPY ")) {
                let table = l.split('"').nth(1).context("a table")?;
                let path = l.split('\'').nth(1).context("a path")?;
                std::fs::write(path, self.0.get(table).copied().unwrap_or(""))?;
            }
            Ok("printed\n".into())
        }
    }

    fn data() -> (PathBuf, Declaration) {
        let d: Declaration = toml::from_str(
            r#"
[tables.items]
key = ["id"]
fields = [{ name = "id", type = "string", required = true }, { name = "status", type = "string", enum = ["open", "done"] }, { name = "size", type = "number" }, { name = "when", type = "date" }]
[tables.notes]
key = ["id"]
extra_fields = true
fields = [{ name = "id", type = "string", required = true }, { name = "text", type = "string" }]
"#,
        )
        .unwrap();
        let dir =
            std::env::temp_dir().join(format!("tad-sql-test-{}-{}", std::process::id(), rand()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("items.json"),
            "{\"id\":\"a\",\"status\":\"open\",\"size\":5,\"when\":\"2026-11-12\"}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("notes.json"),
            "{\"text\": \"kept as written\", \"id\": \"n\", \"mine\": 1}\n",
        )
        .unwrap();
        (dir, d)
    }

    fn rand() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[test]
    fn the_script_makes_and_fills_the_declared_tables() {
        let (dir, d) = data();
        let s = load_script(&dir, &d);
        assert!(s.contains("CREATE TABLE \"items\" (\"id\" VARCHAR, \"status\" VARCHAR, \"size\" DOUBLE, \"when\" DATE);"), "{s}");
        assert!(s.contains("columns = {'id': 'VARCHAR', 'status': 'VARCHAR', 'size': 'DOUBLE', 'when': 'DATE'}"), "{s}");
    }

    #[test]
    fn a_query_changes_nothing_even_where_a_file_is_not_canonical() {
        let (dir, d) = data();
        let same = StandIn(
            [
                (
                    "items",
                    "{\"id\":\"a\",\"status\":\"open\",\"size\":5.0,\"when\":\"2026-11-12\"}\n",
                ),
                ("notes", "{\"id\":\"n\",\"text\":\"kept as written\"}\n"),
            ]
            .into(),
        );
        let before = std::fs::read_to_string(dir.join("notes.json")).unwrap();
        let o = sql(&dir, &d, "SELECT 1", &same, "csv").unwrap();
        assert!(o.changed.is_empty() && o.printed == "printed\n");
        assert_eq!(
            std::fs::read_to_string(dir.join("notes.json")).unwrap(),
            before,
            "a file with fields of its own is left exactly as it was"
        );
        assert!(
            std::fs::read_to_string(dir.join("items.json"))
                .unwrap()
                .contains("\"size\":5,"),
            "5.0 back from the engine is still 5"
        );
    }

    #[test]
    fn a_change_is_kept_only_if_every_rule_still_holds() {
        let (dir, d) = data();
        let good = StandIn(
            [
                (
                    "items",
                    "{\"id\":\"a\",\"status\":\"done\",\"size\":5.0,\"when\":\"2026-11-12\"}\n",
                ),
                ("notes", "{\"id\":\"n\",\"text\":\"kept as written\"}\n"),
            ]
            .into(),
        );
        assert_eq!(
            sql(&dir, &d, "UPDATE items SET status = 'done'", &good, "csv")
                .unwrap()
                .changed,
            vec!["items"]
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("items.json")).unwrap(),
            "{\"id\":\"a\",\"status\":\"done\",\"size\":5,\"when\":\"2026-11-12\"}\n"
        );
        let bad = StandIn(
            [
                (
                    "items",
                    "{\"id\":\"a\",\"status\":\"maybe\",\"size\":5.0,\"when\":\"2026-11-12\"}\n",
                ),
                ("notes", "{\"id\":\"n\",\"text\":\"kept as written\"}\n"),
            ]
            .into(),
        );
        let e = sql(&dir, &d, "UPDATE items SET status = 'maybe'", &bad, "csv")
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("nothing changed") && e.contains("'maybe' is not one of"),
            "{e}"
        );
        assert!(
            std::fs::read_to_string(dir.join("items.json"))
                .unwrap()
                .contains("\"done\""),
            "a refused change leaves the file as it was"
        );
    }

    #[test]
    fn a_table_with_fields_of_its_own_cannot_be_changed_through_sql() {
        let (dir, d) = data();
        let lossy = StandIn(
            [
                (
                    "items",
                    "{\"id\":\"a\",\"status\":\"open\",\"size\":5.0,\"when\":\"2026-11-12\"}\n",
                ),
                ("notes", "{\"id\":\"n\",\"text\":\"changed\"}\n"),
            ]
            .into(),
        );
        let e = sql(&dir, &d, "UPDATE notes SET text = 'changed'", &lossy, "csv")
            .unwrap_err()
            .to_string();
        assert!(e.contains("SQL would drop") && e.contains("tad set"), "{e}");
        assert!(
            std::fs::read_to_string(dir.join("notes.json"))
                .unwrap()
                .contains("\"mine\": 1"),
            "nothing was written"
        );
    }

    #[test]
    fn the_pinned_engine_is_named_by_fingerprint() {
        assert!(DUCKDB_ARCHIVES
            .iter()
            .all(|(_, _, f)| f.len() == 64 && f.chars().all(|c| c.is_ascii_hexdigit())));
        assert_eq!(hex(&[0, 255, 16]), "00ff10");
        assert!(engine_path().ends_with(format!("duckdb-{DUCKDB_VERSION}/duckdb")));
    }
}
