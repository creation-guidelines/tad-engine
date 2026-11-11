//! tad: a transparent database of text files in git. Each table is a file of JSON lines; each
//! repository declares its tables as data - fields, types, required, allowed values, patterns - and
//! tad checks every row against the declaration. Declarations follow Frictionless Data's Table
//! Schema; the format is JSON Lines. A schema written in SQL can be translated into a declaration;
//! anything the translation does not recognise is refused, never dropped.
use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// One field of a table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Field {
    pub name: String,
    /// string, integer, number, boolean, date, datetime, or list (of strings)
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "enum")]
    pub allowed: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starts_with: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One table: its fields, its key, and pairs of fields that must differ in every row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Table {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub distinct: Vec<[String; 2]>,
    /// Pairs (a, b): when a is given, b is required.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<[String; 2]>,
    pub fields: Vec<Field>,
}

/// A repository's tables, declared as data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Declaration {
    pub tables: BTreeMap<String, Table>,
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').to_string()
}

fn cut(s: &str) -> String {
    s.chars().take(160).collect()
}

/// The text inside outer parentheses that enclose the whole expression, repeatedly.
fn strip_parens(mut s: &str) -> &str {
    loop {
        let t = s.trim();
        if !(t.starts_with('(') && t.ends_with(')')) {
            return t;
        }
        let mut depth = 0i32;
        let mut whole = true;
        for (i, ch) in t.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 && i != t.len() - 1 {
                        whole = false;
                        break;
                    }
                }
                _ => {}
            }
        }
        if !whole {
            return t;
        }
        s = &t[1..t.len() - 1];
    }
}

fn type_of(sql: &str) -> Result<String> {
    let t = sql.trim().to_uppercase();
    if t.ends_with("[]") {
        return Ok("list".into());
    }
    let base = t.split('(').next().unwrap_or("").trim();
    Ok(match base {
        "VARCHAR" | "TEXT" | "STRING" | "CHAR" | "UUID" => "string",
        "INTEGER" | "INT" | "BIGINT" | "SMALLINT" | "TINYINT" | "HUGEINT" | "UINTEGER"
        | "UBIGINT" => "integer",
        "DOUBLE" | "FLOAT" | "REAL" | "DECIMAL" | "NUMERIC" => "number",
        "BOOLEAN" | "BOOL" => "boolean",
        "DATE" => "date",
        "TIMESTAMP" | "DATETIME" | "TIMESTAMPTZ" => "datetime",
        other => bail!("a column type this tool does not know: {other}"),
    }
    .into())
}

fn field_mut<'a>(t: &'a mut Table, table: &str, name: &str) -> Result<&'a mut Field> {
    let name = unquote(name);
    t.fields
        .iter_mut()
        .find(|f| f.name == name)
        .with_context(|| format!("{table}: a check names a field it does not have: {name}"))
}

/// Applies one CHECK, recognised from its printed form; refuses what it does not recognise.
fn apply_check(t: &mut Table, table: &str, text: &str) -> Result<()> {
    let mut e = strip_parens(text);
    // "(A IS NULL) OR (rule)": guarded by A. A rule about A itself is A's own rule (an empty value is
    // allowed anyway unless A is required); "B IS NOT NULL" means B is required when A is given; any
    // other rule guarded by another field is refused rather than guessed.
    let nullable = Regex::new(r"^\((\S+) IS NULL\)\s+OR\s+(\(.*\))$")?;
    let owned;
    let mut guard: Option<String> = None;
    if let Some(m) = nullable.captures(e) {
        guard = Some(unquote(&m[1]));
        owned = m[2].to_string();
        e = strip_parens(&owned);
    }
    let same = |name: &str| -> Result<()> {
        match &guard {
            Some(g) if *g != unquote(name) => bail!("{table}: a rule on {} guarded by {g}, which this tool does not know, refused rather than dropped", unquote(name)),
            _ => Ok(()),
        }
    };
    let quoted = r"'((?:[^']|'')*)'";
    let pat = Regex::new(&format!(r"^regexp_matches\(\s*(\S+?)\s*,\s*{quoted}\s*\)$"))?;
    let within = Regex::new(r"^(\S+)\s+IN\s+\((.*)\)$")?;
    let starts = Regex::new(&format!(r"^starts_with\(\s*(\S+?)\s*,\s*{quoted}\s*\)$"))?;
    let differ = Regex::new(r"^(\S+)\s*(?:<>|!=)\s*(\S+)$")?;
    let between = Regex::new(r"^(\S+)\s+BETWEEN\s+(-?[0-9.]+)\s+AND\s+(-?[0-9.]+)$")?;
    let not_null = Regex::new(r"^(\S+)\s+IS NOT NULL$")?;
    if let Some(m) = not_null.captures(e) {
        let f = unquote(&m[1]);
        match &guard {
            Some(g) if *g != f => t.requires.push([g.clone(), f]),
            _ => field_mut(t, table, &f)?.required = true,
        }
    } else if let Some(m) = pat.captures(e) {
        same(&m[1])?;
        field_mut(t, table, &m[1])?.pattern = Some(m[2].replace("''", "'"));
    } else if let Some(m) = within.captures(e) {
        same(&m[1])?;
        let items: Vec<String> = Regex::new(quoted)?
            .captures_iter(&m[2])
            .map(|c| c[1].replace("''", "'"))
            .collect();
        if items.is_empty() {
            bail!("{table}: an IN list with no plain values: {}", cut(e));
        }
        field_mut(t, table, &m[1])?.allowed = items;
    } else if let Some(m) = starts.captures(e) {
        same(&m[1])?;
        field_mut(t, table, &m[1])?.starts_with = Some(m[2].replace("''", "'"));
    } else if let Some(m) = between.captures(e) {
        same(&m[1])?;
        let f = field_mut(t, table, &m[1])?;
        f.minimum = Some(m[2].parse()?);
        f.maximum = Some(m[3].parse()?);
    } else if let Some(m) = differ.captures(e) {
        if guard.is_some() {
            bail!(
                "{table}: a guarded comparison, refused rather than dropped: {}",
                cut(e)
            );
        }
        t.distinct.push([unquote(&m[1]), unquote(&m[2])]);
    } else {
        bail!(
            "{table}: a check this tool does not know yet, refused rather than dropped: {}",
            cut(e)
        );
    }
    Ok(())
}

fn names_in(list: &str) -> Vec<String> {
    strip_parens(list)
        .split(',')
        .map(unquote)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Translates a schema written in SQL (as DuckDB writes it) into a declaration.
pub fn from_sql(sql: &str) -> Result<Declaration> {
    use sqlparser::{ast::Statement, dialect::DuckDbDialect, parser::Parser};
    let cleaned = sql.replace(";;", ";");
    let statements = Parser::parse_sql(&DuckDbDialect {}, &cleaned)
        .context("the schema is not SQL this tool can read")?;
    let mut d = Declaration::default();
    for st in statements {
        let Statement::CreateTable(ct) = st else {
            bail!(
                "only CREATE TABLE is expected in a schema, found: {}",
                cut(&st.to_string())
            );
        };
        let name = unquote(&ct.name.to_string());
        let mut t = Table::default();
        let mut checks: Vec<String> = Vec::new();
        for c in &ct.columns {
            let mut f = Field {
                name: unquote(&c.name.to_string()),
                ty: type_of(&c.data_type.to_string())
                    .with_context(|| format!("{name}.{}", c.name))?,
                ..Default::default()
            };
            for o in &c.options {
                let txt = o.option.to_string();
                if txt == "NOT NULL" {
                    f.required = true;
                } else if txt.starts_with("PRIMARY KEY") {
                    f.required = true;
                    t.key.push(f.name.clone());
                } else if txt.starts_with("DEFAULT") || txt == "NULL" {
                } else if let Some(inner) = txt.strip_prefix("CHECK") {
                    checks.push(inner.to_string());
                } else {
                    bail!(
                        "{name}.{}: a column option this tool does not know: {txt}",
                        f.name
                    );
                }
            }
            t.fields.push(f);
        }
        for k in &ct.constraints {
            let txt = k.to_string();
            if let Some(cols) = txt.strip_prefix("PRIMARY KEY") {
                for c in names_in(cols) {
                    field_mut(&mut t, &name, &c)?.required = true;
                    t.key.push(c);
                }
            } else if let Some(inner) = txt.strip_prefix("CHECK") {
                checks.push(inner.to_string());
            } else {
                bail!(
                    "{name}: a table constraint this tool does not know: {}",
                    cut(&txt)
                );
            }
        }
        for c in &checks {
            apply_check(&mut t, &name, c)?;
        }
        d.tables.insert(name, t);
    }
    Ok(d)
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 10
        || b[4] != b'-'
        || b[7] != b'-'
        || !s[..10]
            .chars()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return false;
    }
    let (m, d): (u32, u32) = (s[5..7].parse().unwrap_or(0), s[8..10].parse().unwrap_or(0));
    (1..=12).contains(&m) && (1..=31).contains(&d)
}

fn type_fits(ty: &str, v: &Value) -> bool {
    match ty {
        "string" => v.is_string(),
        "integer" => v.is_i64() || v.is_u64(),
        "number" => v.is_number(),
        "boolean" => v.is_boolean(),
        "date" => v.as_str().is_some_and(|s| s.len() == 10 && is_date(s)),
        "datetime" => v.as_str().is_some_and(is_date),
        "list" => v.as_array().is_some_and(|a| a.iter().all(Value::is_string)),
        _ => false,
    }
}

/// Every row of every declared table in `dir` (one `<table>.json` file of JSON lines each) checked
/// against the declaration. Returns the problems found, each naming its table and row.
pub fn check(dir: &Path, d: &Declaration) -> Result<(usize, Vec<String>)> {
    let mut problems = Vec::new();
    let mut rows = 0;
    for (name, t) in &d.tables {
        let file = dir.join(format!("{name}.json"));
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let patterns: BTreeMap<&str, Regex> = t
            .fields
            .iter()
            .filter_map(|f| {
                f.pattern
                    .as_ref()
                    .map(|p| Regex::new(p).map(|r| (f.name.as_str(), r)))
            })
            .collect::<std::result::Result<_, _>>()
            .with_context(|| format!("{name}: a pattern is not a regular expression"))?;
        let mut keys: HashSet<String> = HashSet::new();
        for (i, line) in text
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty())
        {
            rows += 1;
            let at = format!("{name}, line {}", i + 1);
            let Ok(Value::Object(row)) = serde_json::from_str::<Value>(line) else {
                problems.push(format!("{at}: not a JSON object"));
                continue;
            };
            for k in row.keys() {
                if !t.fields.iter().any(|f| &f.name == k) {
                    problems.push(format!("{at}: the field {k} is not declared"));
                }
            }
            for f in &t.fields {
                let v = match row.get(&f.name) {
                    None | Some(Value::Null) => {
                        if f.required {
                            problems.push(format!("{at}: {} is required", f.name));
                        }
                        continue;
                    }
                    Some(v) => v,
                };
                if !type_fits(&f.ty, v) {
                    problems.push(format!("{at}: {} should be a {}, not {v}", f.name, f.ty));
                    continue;
                }
                if let Some(n) = v.as_f64() {
                    if f.minimum.is_some_and(|m| n < m) || f.maximum.is_some_and(|m| n > m) {
                        problems.push(format!(
                            "{at}: {} {n} is outside {} to {}",
                            f.name,
                            f.minimum.unwrap_or(f64::MIN),
                            f.maximum.unwrap_or(f64::MAX)
                        ));
                    }
                }
                if let Some(s) = v.as_str() {
                    if !f.allowed.is_empty() && !f.allowed.iter().any(|a| a == s) {
                        problems.push(format!(
                            "{at}: {} '{s}' is not one of: {}",
                            f.name,
                            f.allowed.join(", ")
                        ));
                    }
                    if let Some(r) = patterns.get(f.name.as_str()) {
                        if !r.is_match(s) {
                            problems.push(format!(
                                "{at}: {} '{s}' does not match {}",
                                f.name,
                                r.as_str()
                            ));
                        }
                    }
                    if let Some(p) = &f.starts_with {
                        if !s.starts_with(p.as_str()) {
                            problems.push(format!("{at}: {} '{s}' should start with {p}", f.name));
                        }
                    }
                }
            }
            for [a, b] in &t.distinct {
                if row.get(a).is_some() && row.get(a) == row.get(b) {
                    problems.push(format!("{at}: {a} and {b} must differ"));
                }
            }
            for [a, b] in &t.requires {
                let given = |k: &str| row.get(k).is_some_and(|v| !v.is_null());
                if given(a) && !given(b) {
                    problems.push(format!("{at}: {b} is required when {a} is given"));
                }
            }
            if !t.key.is_empty() {
                let k: Vec<String> = t
                    .key
                    .iter()
                    .map(|f| row.get(f).map(|v| v.to_string()).unwrap_or_default())
                    .collect();
                if !keys.insert(k.join("\u{1f}")) {
                    problems.push(format!(
                        "{at}: the key ({}) is not unique: {}",
                        t.key.join(", "),
                        k.join(", ")
                    ));
                }
            }
        }
    }
    Ok((rows, problems))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = r#"CREATE TABLE components(id VARCHAR PRIMARY KEY, "name" VARCHAR NOT NULL, category VARCHAR NOT NULL, CHECK(regexp_matches(id, '^[a-z0-9]+(-[a-z0-9]+)*$')), CHECK((category IN ('audio', 'display', 'other'))));;
CREATE TABLE sources(id VARCHAR PRIMARY KEY, title VARCHAR NOT NULL, url VARCHAR NOT NULL, "year" INTEGER, tags VARCHAR[] DEFAULT(main.list_value()) NOT NULL, CHECK(starts_with(url, 'https://')));;
CREATE TABLE relations(from_id VARCHAR, to_id VARCHAR, relation VARCHAR, PRIMARY KEY(from_id, to_id, relation), CHECK((from_id != to_id)));;
CREATE TABLE days(id VARCHAR PRIMARY KEY, entry_date DATE NOT NULL, readiness INTEGER, origin_year INTEGER, origin_author VARCHAR, CHECK(readiness BETWEEN 1 AND 9), CHECK(entry_date IS NOT NULL), CHECK(((origin_year IS NULL) OR (origin_author IS NOT NULL))), CHECK(((origin_year IS NULL) OR (origin_year BETWEEN 1950 AND 2100))));;"#;

    #[test]
    fn a_duckdb_schema_becomes_a_declaration() {
        let d = from_sql(SCHEMA).unwrap();
        let c = &d.tables["components"];
        assert_eq!(c.key, vec!["id"]);
        assert_eq!(
            c.fields[0].pattern.as_deref(),
            Some("^[a-z0-9]+(-[a-z0-9]+)*$")
        );
        assert_eq!(c.fields[2].allowed, vec!["audio", "display", "other"]);
        assert!(
            c.fields[1].required && c.fields[1].name == "name",
            "quoted names unquoted"
        );
        let s = &d.tables["sources"];
        assert_eq!(s.fields[2].starts_with.as_deref(), Some("https://"));
        assert_eq!(
            (s.fields[3].ty.as_str(), s.fields[4].ty.as_str()),
            ("integer", "list")
        );
        let r = &d.tables["relations"];
        assert_eq!(r.key, vec!["from_id", "to_id", "relation"]);
        assert_eq!(
            r.distinct,
            vec![["from_id".to_string(), "to_id".to_string()]]
        );
        assert_eq!(d.tables["days"].fields[1].ty, "date");
        assert_eq!(
            (
                d.tables["days"].fields[2].minimum,
                d.tables["days"].fields[2].maximum
            ),
            (Some(1.0), Some(9.0))
        );
        assert_eq!(
            d.tables["days"].fields[3].minimum,
            Some(1950.0),
            "an empty-or-range check is a range"
        );
        assert_eq!(
            d.tables["days"].requires,
            vec![["origin_year".to_string(), "origin_author".to_string()]],
            "a guard on another field is a dependency"
        );
        assert!(
            !d.tables["days"].fields[4].required,
            "not required on its own"
        );
        assert!(from_sql(
            "CREATE TABLE t(a INTEGER, b VARCHAR, CHECK(((a IS NULL) OR (b IN ('x')))));"
        )
        .unwrap_err()
        .to_string()
        .contains("guarded by a"));
        let again: Declaration = toml::from_str(&toml::to_string(&d).unwrap()).unwrap();
        assert_eq!(again, d, "a declaration survives being written as TOML");
    }

    #[test]
    fn what_it_does_not_know_is_refused() {
        assert!(from_sql("CREATE TABLE t(a VARCHAR, CHECK(length(a) > 3));")
            .unwrap_err()
            .to_string()
            .contains("refused rather than dropped"));
        assert!(from_sql("CREATE TABLE t(a BLOB);").is_err());
        assert!(from_sql("CREATE VIEW v AS SELECT 1;").is_err());
    }

    #[test]
    fn every_kind_of_problem_is_found() {
        let d = from_sql(SCHEMA).unwrap();
        let dir = std::env::temp_dir().join(format!("tad-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("components.json"),
            "{\"id\":\"mic\",\"name\":\"Mic\",\"category\":\"audio\"}\n",
        )
        .unwrap();
        assert_eq!(check(&dir, &d).unwrap(), (1, vec![]), "a good row passes");
        std::fs::write(
            dir.join("components.json"),
            "{\"id\":\"Bad Id\",\"name\":\"X\",\"category\":\"audio\"}\n{\"id\":\"a\",\"category\":\"maybe\"}\n{\"id\":\"b\",\"name\":\"B\",\"category\":\"other\",\"extra\":1}\n{\"id\":\"b\",\"name\":\"B\",\"category\":\"other\"}\n",
        )
        .unwrap();
        std::fs::write(dir.join("sources.json"), "{\"id\":\"s\",\"title\":\"T\",\"url\":\"http://x\",\"year\":\"1999\",\"tags\":[\"a\"]}\n").unwrap();
        std::fs::write(
            dir.join("relations.json"),
            "{\"from_id\":\"a\",\"to_id\":\"a\",\"relation\":\"r\"}\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("days.json"),
            "{\"id\":\"d\",\"entry_date\":\"2026-13-01\",\"readiness\":12,\"origin_year\":2001}\n",
        )
        .unwrap();
        let (_, p) = check(&dir, &d).unwrap();
        let all = p.join("\n");
        for want in [
            "does not match",
            "name is required",
            "'maybe' is not one of",
            "extra is not declared",
            "is not unique",
            "should start with https://",
            "year should be a integer",
            "must differ",
            "entry_date should be a date",
            "readiness 12 is outside 1 to 9",
            "origin_author is required when origin_year is given",
        ] {
            assert!(all.contains(want), "missing '{want}' in:\n{all}");
        }
    }
}
