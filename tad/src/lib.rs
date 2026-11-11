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

pub mod render;

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

/// Ids unique across several tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct UniqueIds {
    pub tables: Vec<String>,
}

/// `field` ("table.field") names a row of one of `to` (by id); an empty value names nothing and passes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Reference {
    pub field: String,
    pub to: Vec<String>,
}

/// Every row of `table` appears (by id) among the values of `within` ("table.field").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AppearsIn {
    pub table: String,
    #[serde(rename = "in")]
    pub within: String,
}

/// In `table`, a relation of one of `kinds` is not also recorded the other way round.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct NoMirror {
    pub table: String,
    pub from: String,
    pub to: String,
    pub kind: String,
    pub kinds: Vec<String>,
}

/// Rules across tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Rules {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unique_ids: Vec<UniqueIds>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<Reference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub appears_in: Vec<AppearsIn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub no_mirror: Vec<NoMirror>,
}

impl Rules {
    pub fn is_empty(&self) -> bool {
        self.unique_ids.is_empty()
            && self.references.is_empty()
            && self.appears_in.is_empty()
            && self.no_mirror.is_empty()
    }
}

/// A repository's tables, declared as data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Declaration {
    pub tables: BTreeMap<String, Table>,
    #[serde(default, skip_serializing_if = "Rules::is_empty")]
    pub rules: Rules,
    /// How the index reads, and the words for relations (optional; see render).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<render::SitePresentation>,
    /// How each page reads (optional; a page not named renders as every page does).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pages: BTreeMap<String, render::Page>,
}

fn rows_of(dir: &Path, table: &str) -> Vec<serde_json::Map<String, Value>> {
    std::fs::read_to_string(dir.join(format!("{table}.json")))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| match serde_json::from_str::<Value>(l) {
            Ok(Value::Object(m)) => Some(m),
            _ => None,
        })
        .collect()
}

fn text(v: Option<&Value>) -> Option<String> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(other) => Some(other.to_string()),
    }
}

fn split_field(f: &str) -> Result<(&str, &str)> {
    f.split_once('.')
        .with_context(|| format!("'{f}' should be table.field"))
}

/// The rules across tables, checked against the rows in `dir`.
pub fn check_rules(dir: &Path, r: &Rules) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    let ids = |tables: &[String]| -> HashSet<String> {
        tables
            .iter()
            .flat_map(|t| rows_of(dir, t))
            .filter_map(|m| text(m.get("id")))
            .collect()
    };
    for u in &r.unique_ids {
        let mut seen: BTreeMap<String, Vec<&str>> = BTreeMap::new();
        for t in &u.tables {
            for m in rows_of(dir, t) {
                if let Some(id) = text(m.get("id")) {
                    seen.entry(id).or_default().push(t);
                }
            }
        }
        for (id, ts) in seen.iter().filter(|(_, ts)| ts.len() > 1) {
            problems.push(format!(
                "the id '{id}' is used more than once, in: {}",
                ts.join(", ")
            ));
        }
    }
    for x in &r.references {
        let (table, field) = split_field(&x.field)?;
        let known = ids(&x.to);
        for m in rows_of(dir, table) {
            if let Some(v) = text(m.get(field)) {
                if !known.contains(&v) {
                    problems.push(format!(
                        "{table}: {field} '{v}' names nothing in {}",
                        x.to.join(" or ")
                    ));
                }
            }
        }
    }
    for a in &r.appears_in {
        let (table, field) = split_field(&a.within)?;
        let found: HashSet<String> = rows_of(dir, table)
            .iter()
            .filter_map(|m| text(m.get(field)))
            .collect();
        for m in rows_of(dir, &a.table) {
            if let Some(id) = text(m.get("id")) {
                if !found.contains(&id) {
                    problems.push(format!(
                        "{}: '{id}' appears nowhere in {}",
                        a.table, a.within
                    ));
                }
            }
        }
    }
    for n in &r.no_mirror {
        let rows = rows_of(dir, &n.table);
        let all: HashSet<(String, String, String)> = rows
            .iter()
            .filter_map(|m| {
                Some((
                    text(m.get(&n.from))?,
                    text(m.get(&n.to))?,
                    text(m.get(&n.kind))?,
                ))
            })
            .filter(|(_, _, k)| n.kinds.contains(k))
            .collect();
        let mut found: Vec<&(String, String, String)> = all
            .iter()
            .filter(|(a, b, k)| a < b && all.contains(&(b.clone(), a.clone(), k.clone())))
            .collect();
        found.sort();
        for (a, b, k) in found {
            problems.push(format!(
                "{}: '{a}' {k} '{b}' is recorded both ways",
                n.table
            ));
        }
    }
    Ok(problems)
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
    problems.extend(check_rules(dir, &d.rules)?);
    Ok((rows, problems))
}

/// A row's canonical line: compact JSON, the declared fields in their declared order, an empty field
/// written as null - exactly as DuckDB writes it, so the switch to tad changes no byte.
pub fn canonical_line(t: &Table, row: &serde_json::Map<String, Value>) -> String {
    let parts: Vec<String> = t
        .fields
        .iter()
        .map(|f| {
            format!(
                "{}:{}",
                Value::String(f.name.clone()),
                row.get(&f.name).cloned().unwrap_or(Value::Null)
            )
        })
        .collect();
    format!("{{{}}}", parts.join(","))
}

fn parse_rows(file: &Path) -> Result<Vec<serde_json::Map<String, Value>>> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let mut rows = Vec::new();
    for (i, l) in text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
    {
        match serde_json::from_str::<Value>(l) {
            Ok(Value::Object(m)) => rows.push(m),
            _ => bail!("{}, line {}: not a JSON object", file.display(), i + 1),
        }
    }
    Ok(rows)
}

fn write_rows(file: &Path, t: &Table, rows: &[serde_json::Map<String, Value>]) -> Result<bool> {
    let new: String = rows.iter().map(|r| canonical_line(t, r) + "\n").collect();
    let old = std::fs::read_to_string(file).unwrap_or_default();
    if new == old {
        return Ok(false);
    }
    std::fs::write(file, new).with_context(|| format!("writing {}", file.display()))?;
    Ok(true)
}

/// Rewrites every declared table in canonical form, keeping its rows in their order - only when the
/// data checks out, as Python's canon validated on import. Returns the tables whose files changed.
pub fn canon(dir: &Path, d: &Declaration) -> Result<Vec<String>> {
    let (_, problems) = check(dir, d)?;
    if !problems.is_empty() {
        bail!(
            "{} problems - canon writes nothing until they are fixed:\n{}",
            problems.len(),
            problems.join("\n")
        );
    }
    let mut changed = Vec::new();
    for (name, t) in &d.tables {
        let file = dir.join(format!("{name}.json"));
        if !file.exists() {
            continue;
        }
        if write_rows(&file, t, &parse_rows(&file)?)? {
            changed.push(name.clone());
        }
    }
    Ok(changed)
}

/// A value for a field, typed by its declaration; an empty value is null.
fn typed(f: &Field, s: &str) -> Result<Value> {
    if s.is_empty() {
        return Ok(Value::Null);
    }
    Ok(match f.ty.as_str() {
        "integer" => Value::from(
            s.trim()
                .parse::<i64>()
                .with_context(|| format!("{} is a whole number, not '{s}'", f.name))?,
        ),
        "number" => Value::from(
            s.trim()
                .parse::<f64>()
                .with_context(|| format!("{} is a number, not '{s}'", f.name))?,
        ),
        "boolean" => match s.trim() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => bail!("{} is true or false, not '{s}'", f.name),
        },
        "list" => Value::Array(
            s.split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(|x| Value::String(x.into()))
                .collect(),
        ),
        _ => Value::String(s.into()),
    })
}

/// Adds or updates one row (found by its key), then checks the whole repository; if anything breaks,
/// the file is put back as it was and the problems are named. New rows go at the end, as DuckDB would.
pub fn set(
    dir: &Path,
    d: &Declaration,
    table: &str,
    key: &[String],
    changes: &[(String, String)],
) -> Result<String> {
    let t = d
        .tables
        .get(table)
        .with_context(|| format!("no table {table} is declared"))?;
    if t.key.is_empty() || key.len() != t.key.len() {
        bail!(
            "{table} is keyed by ({}); give that many --key values",
            t.key.join(", ")
        );
    }
    let file = dir.join(format!("{table}.json"));
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    let mut rows = parse_rows(&file)?;
    let pos = rows.iter().position(|r| {
        t.key
            .iter()
            .zip(key)
            .all(|(k, v)| text(r.get(k)).as_deref() == Some(v.as_str()))
    });
    let mut row = match pos {
        Some(i) => rows[i].clone(),
        None => t
            .key
            .iter()
            .zip(key)
            .map(|(k, v)| {
                Ok((
                    k.clone(),
                    typed(
                        t.fields
                            .iter()
                            .find(|f| &f.name == k)
                            .context("a key field is not declared")?,
                        v,
                    )?,
                ))
            })
            .collect::<Result<_>>()?,
    };
    for (f, v) in changes {
        if t.key.contains(f) {
            bail!("{f} is part of the key, given by --key");
        }
        let field = t
            .fields
            .iter()
            .find(|x| &x.name == f)
            .with_context(|| format!("{table} has no field {f}"))?;
        row.insert(f.clone(), typed(field, v)?);
    }
    let added = pos.is_none();
    match pos {
        Some(i) => rows[i] = row,
        None => rows.push(row),
    }
    write_rows(&file, t, &rows)?;
    let (_, problems) = check(dir, d)?;
    if !problems.is_empty() {
        std::fs::write(&file, old)?;
        bail!(
            "nothing changed - this would break {} rule(s):\n{}",
            problems.len(),
            problems.join("\n")
        );
    }
    Ok(format!(
        "{} {table}/{}",
        if added { "added" } else { "updated" },
        key.join("/")
    ))
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

    #[test]
    fn rules_across_tables_are_checked() {
        let dir = std::env::temp_dir().join(format!("tad-rules-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let w = |t: &str, s: &str| std::fs::write(dir.join(format!("{t}.json")), s).unwrap();
        w("devices", "{\"id\":\"surya\"}\n{\"id\":\"mic\"}\n");
        w("components", "{\"id\":\"mic\"}\n");
        w(
            "support",
            "{\"device_id\":\"surya\"}\n{\"device_id\":\"nowhere\"}\n{\"device_id\":null}\n",
        );
        w("sources", "{\"id\":\"cited\"}\n{\"id\":\"lonely\"}\n");
        w("cites", "{\"source_id\":\"cited\"}\n");
        w("relations", "{\"from_id\":\"a\",\"to_id\":\"b\",\"relation\":\"pairs-with\"}\n{\"from_id\":\"b\",\"to_id\":\"a\",\"relation\":\"pairs-with\"}\n{\"from_id\":\"a\",\"to_id\":\"c\",\"relation\":\"uses\"}\n{\"from_id\":\"c\",\"to_id\":\"a\",\"relation\":\"uses\"}\n");
        let r: Rules = toml::from_str(
            r#"
[[unique_ids]]
tables = ["devices", "components"]
[[references]]
field = "support.device_id"
to = ["devices"]
[[appears_in]]
table = "sources"
in = "cites.source_id"
[[no_mirror]]
table = "relations"
from = "from_id"
to = "to_id"
kind = "relation"
kinds = ["pairs-with"]
"#,
        )
        .unwrap();
        let p = check_rules(&dir, &r).unwrap().join("\n");
        assert!(
            p.contains("the id 'mic' is used more than once, in: devices, components"),
            "{p}"
        );
        assert!(
            p.contains("device_id 'nowhere' names nothing in devices") && !p.contains("null"),
            "an empty reference passes: {p}"
        );
        assert!(
            p.contains("'lonely' appears nowhere in cites.source_id") && !p.contains("'cited'"),
            "{p}"
        );
        assert!(
            p.contains("'a' pairs-with 'b' is recorded both ways") && !p.contains("uses"),
            "only the symmetric kinds: {p}"
        );
        assert_eq!(p.lines().count(), 4, "{p}");
    }

    #[test]
    fn canon_writes_declared_order_with_nulls_and_set_checks_before_it_keeps() {
        let d = from_sql(SCHEMA).unwrap();
        let dir = std::env::temp_dir().join(format!("tad-canon-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("components.json");
        std::fs::write(
            &f,
            "{\"category\": \"audio\", \"id\": \"mic\", \"name\": \"Микрофон \\\"1\\\"\"}\n",
        )
        .unwrap();
        assert_eq!(canon(&dir, &d).unwrap(), vec!["components"]);
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            "{\"id\":\"mic\",\"name\":\"Микрофон \\\"1\\\"\",\"category\":\"audio\"}\n"
        );
        assert!(
            canon(&dir, &d).unwrap().is_empty(),
            "canonical data is left byte for byte"
        );
        assert_eq!(
            set(
                &dir,
                &d,
                "components",
                &["mic".into()],
                &[("category".into(), "display".into())]
            )
            .unwrap(),
            "updated components/mic"
        );
        assert_eq!(
            set(
                &dir,
                &d,
                "components",
                &["cam".into()],
                &[
                    ("name".into(), "Camera".into()),
                    ("category".into(), "other".into())
                ]
            )
            .unwrap(),
            "added components/cam"
        );
        assert!(
            std::fs::read_to_string(&f)
                .unwrap()
                .ends_with("{\"id\":\"cam\",\"name\":\"Camera\",\"category\":\"other\"}\n"),
            "new rows at the end"
        );
        let before = std::fs::read_to_string(&f).unwrap();
        let e = set(
            &dir,
            &d,
            "components",
            &["mic".into()],
            &[("category".into(), "maybe".into())],
        )
        .unwrap_err()
        .to_string();
        assert!(
            e.contains("nothing changed") && e.contains("'maybe' is not one of"),
            "{e}"
        );
        assert_eq!(
            std::fs::read_to_string(&f).unwrap(),
            before,
            "a refused edit leaves the file as it was"
        );
        std::fs::write(dir.join("days.json"), "").unwrap();
        assert_eq!(
            set(
                &dir,
                &d,
                "days",
                &["d".into()],
                &[
                    ("entry_date".into(), "2026-11-11".into()),
                    ("readiness".into(), "3".into())
                ]
            )
            .unwrap(),
            "added days/d"
        );
        assert!(
            std::fs::read_to_string(dir.join("days.json"))
                .unwrap()
                .contains("\"readiness\":3,\"origin_year\":null"),
            "typed by the declaration, empty fields null"
        );
    }
}
