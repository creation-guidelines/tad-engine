//! Named statements: SQL a place keeps as files beside its tables, run by name with typed parameters.
//! A query (`queries/NAME.sql`) reads and changes nothing; a record (`records/NAME.sql`) writes, and
//! what it writes is kept only if every rule of the tables still holds - as with `tad sql`. A new
//! statement is a file in the same change as the work that needs it: tad itself is not released for it.
//!
//! The header is the statement's leading comment. Its first line says what the statement does; a line
//! that begins with `--NAME` declares a parameter, written as it is given on the command line:
//!
//! ```sql
//! -- A decision, with what was rejected.
//! -- --project  string        the place it is of
//! -- --why      string?       why (optional: null when not given)
//! -- --date     date = today  when it was decided
//! INSERT INTO decisions (id, project, date, why) VALUES ($id, $project, $date, $why);
//! ```
//!
//! Types are the declaration's own: `string`, `integer`, `number`, `boolean`, `date`, `list`
//! (comma-separated). A parameter is required unless it has `?` or a default. In the body, `$NAME`
//! stands for the value, written by tad as a SQL literal of its type - never spliced as text - and
//! never inside a quoted string or a comment.
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Which kind of statement: it says the folder and whether it may change the data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Query,
    Record,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Query, Kind::Record];

    /// The folder a place keeps them in.
    pub fn folder(self) -> &'static str {
        match self {
            Kind::Query => "queries",
            Kind::Record => "records",
        }
    }

    pub fn from_folder(folder: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.folder() == folder)
    }
}

/// A parameter's type: the declaration's types, as SQL literals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    String,
    Integer,
    Number,
    Boolean,
    Date,
    List,
}

impl Type {
    fn parse(token: &str) -> Option<Type> {
        Some(match token {
            "string" => Type::String,
            "integer" => Type::Integer,
            "number" => Type::Number,
            "boolean" => Type::Boolean,
            "date" => Type::Date,
            "list" => Type::List,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Type::String => "string",
            Type::Integer => "integer",
            Type::Number => "number",
            Type::Boolean => "boolean",
            Type::Date => "date",
            Type::List => "list",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    /// Absent and with no default: the statement does not run.
    pub required: bool,
    /// Taken when the parameter is not given; `today` for a date is the day it runs.
    pub default: Option<String>,
    pub summary: String,
}

impl Param {
    /// How the body names it: `--run-at` is `$run_at`.
    pub fn key(&self) -> String {
        self.name.replace('-', "_")
    }
}

#[derive(Clone, Debug)]
pub struct Statement {
    pub kind: Kind,
    pub name: String,
    /// What it does, in one line.
    pub summary: String,
    pub params: Vec<Param>,
    pub body: String,
}

/// The words tad's own command line keeps for itself: a statement's parameter may not take one.
pub const RESERVED: [&str; 4] = ["tables", "data", "format", "help"];

fn is_name(s: &str) -> bool {
    !s.is_empty()
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// The folder a place's statements are kept beside: the folder of its declaration.
pub fn root_of(tables: &Path) -> PathBuf {
    match tables.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// The names of the statements of a kind, in order.
pub fn names(root: &Path, kind: Kind) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(root.join(kind.folder()))
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter_map(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    n.strip_suffix(".sql")
                        .filter(|s| is_name(s))
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Reads one statement by name.
pub fn load(root: &Path, kind: Kind, name: &str) -> Result<Statement> {
    let file = root.join(kind.folder()).join(format!("{name}.sql"));
    if !is_name(name) || !file.is_file() {
        let have = names(root, kind);
        bail!(
            "no {} named {name} in {}/ - {}",
            match kind {
                Kind::Query => "query",
                Kind::Record => "record",
            },
            root.join(kind.folder()).display(),
            if have.is_empty() {
                "there are none".to_string()
            } else {
                format!("there are: {}", have.join(", "))
            }
        );
    }
    let text =
        std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
    parse(kind, name, &text).with_context(|| format!("{}", file.display()))
}

/// Reads a statement's header and keeps its body.
pub fn parse(kind: Kind, name: &str, text: &str) -> Result<Statement> {
    let mut summary = String::new();
    let mut params: Vec<Param> = Vec::new();
    for line in text.lines() {
        let Some(comment) = line.trim_start().strip_prefix("--") else {
            break;
        };
        let comment = comment.trim();
        if let Some(rest) = comment.strip_prefix("--") {
            params.push(param(rest)?);
        } else if summary.is_empty() && !comment.is_empty() {
            summary = comment.to_string();
        }
    }
    for (i, p) in params.iter().enumerate() {
        if RESERVED.contains(&p.name.as_str()) {
            bail!(
                "--{} is tad's own word: name the parameter otherwise",
                p.name
            );
        }
        if params[..i].iter().any(|q| q.name == p.name) {
            bail!("--{} is declared twice", p.name);
        }
    }
    let s = Statement {
        kind,
        name: name.to_string(),
        summary,
        params,
        body: text.to_string(),
    };
    // every $NAME the body uses is declared: checked when it is read, not when it is run
    let all: BTreeMap<String, String> = s
        .params
        .iter()
        .map(|p| (p.key(), "NULL".to_string()))
        .collect();
    substitute(&s.body, &all)?;
    Ok(s)
}

/// `NAME TYPE[?] [= DEFAULT] [SUMMARY]`
fn param(text: &str) -> Result<Param> {
    let mut words = text.split_whitespace();
    let name = words.next().unwrap_or_default().to_string();
    if !is_name(&name) {
        bail!("--{name}: a parameter's name is lower-case letters, digits and '-'");
    }
    let ty_word = words
        .next()
        .with_context(|| format!("--{name} says no type"))?;
    let (ty_word, optional) = match ty_word.strip_suffix('?') {
        Some(t) => (t, true),
        None => (ty_word, false),
    };
    let ty = Type::parse(ty_word).with_context(|| {
        format!(
            "--{name}: '{ty_word}' is not a type (string, integer, number, boolean, date, list)"
        )
    })?;
    let rest: Vec<&str> = words.collect();
    let (default, summary) = if rest.first() == Some(&"=") {
        let d = rest
            .get(1)
            .with_context(|| format!("--{name}: '=' and no default"))?;
        (Some(d.to_string()), rest[2..].join(" "))
    } else {
        (None, rest.join(" "))
    };
    if let Some(d) = &default {
        if !(ty == Type::Date && d == "today") {
            literal(ty, d).with_context(|| format!("--{name}'s default"))?;
        }
    }
    Ok(Param {
        required: !optional && default.is_none(),
        name,
        ty,
        default,
        summary,
    })
}

fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// A value as a SQL literal of its type; a value that is not of the type is refused.
pub fn literal(ty: Type, value: &str) -> Result<String> {
    Ok(match ty {
        Type::String => quoted(value),
        Type::Integer => {
            value
                .parse::<i64>()
                .ok()
                .with_context(|| format!("'{value}' is not an integer"))?;
            value.to_string()
        }
        Type::Number => {
            let n = value
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .with_context(|| format!("'{value}' is not a number"))?;
            format!("{n}")
        }
        Type::Boolean => match value {
            "true" | "yes" | "1" => "TRUE".into(),
            "false" | "no" | "0" => "FALSE".into(),
            _ => bail!("'{value}' is not true or false"),
        },
        Type::Date => {
            let ok = value.len() == 10
                && value.bytes().enumerate().all(|(i, b)| {
                    if i == 4 || i == 7 {
                        b == b'-'
                    } else {
                        b.is_ascii_digit()
                    }
                });
            if !ok {
                bail!("'{value}' is not a date (YYYY-MM-DD)");
            }
            format!("DATE '{value}'")
        }
        Type::List => {
            let items: Vec<String> = value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(quoted)
                .collect();
            format!("[{}]::VARCHAR[]", items.join(", "))
        }
    })
}

impl Statement {
    /// The body with every parameter written in as a literal of its type. `given` is what was said on
    /// the command line, by name; a name the statement does not declare is refused.
    pub fn bind(&self, given: &[(String, String)]) -> Result<String> {
        let mut values: BTreeMap<String, String> = BTreeMap::new();
        for (name, value) in given {
            let Some(p) = self.params.iter().find(|p| &p.name == name) else {
                bail!(
                    "{} takes no --{name}{}",
                    self.name,
                    if self.params.is_empty() {
                        " (it takes no parameters)".to_string()
                    } else {
                        format!(
                            "; it takes {}",
                            self.params
                                .iter()
                                .map(|p| format!("--{}", p.name))
                                .collect::<Vec<_>>()
                                .join(" ")
                        )
                    }
                );
            };
            if values.contains_key(&p.key()) {
                bail!("--{name} is given twice");
            }
            values.insert(
                p.key(),
                literal(p.ty, value).with_context(|| format!("--{name}"))?,
            );
        }
        for p in &self.params {
            if values.contains_key(&p.key()) {
                continue;
            }
            let v = match (&p.default, p.required) {
                (Some(d), _) if p.ty == Type::Date && d == "today" => {
                    literal(Type::Date, &crate::today())?
                }
                (Some(d), _) => literal(p.ty, d)?,
                (None, false) => "NULL".to_string(),
                (None, true) => bail!(
                    "{} needs --{} ({}{})",
                    self.name,
                    p.name,
                    p.ty.name(),
                    if p.summary.is_empty() {
                        String::new()
                    } else {
                        format!(": {}", p.summary)
                    }
                ),
            };
            values.insert(p.key(), v);
        }
        substitute(&self.body, &values)
    }

    /// How it is called, for a listing: `NAME --a TYPE [--b TYPE]  summary`.
    pub fn usage(&self) -> String {
        let mut s = self.name.clone();
        for p in &self.params {
            let one = format!("--{} {}", p.name, p.ty.name());
            if p.required {
                s.push_str(&format!(" {one}"));
            } else {
                s.push_str(&format!(" [{one}]"));
            }
        }
        if !self.summary.is_empty() {
            s.push_str(&format!("  - {}", self.summary));
        }
        s
    }
}

/// Writes each `$NAME` of the body as its value, outside quoted text and comments. A `$NAME` that is
/// not a parameter is refused; `$1` and `$$...$$` are left to the engine.
fn substitute(body: &str, values: &BTreeMap<String, String>) -> Result<String> {
    let c: Vec<char> = body.chars().collect();
    let mut out = String::with_capacity(body.len());
    let mut i = 0;
    let until = |from: usize, end: &str| -> usize {
        let e: Vec<char> = end.chars().collect();
        let mut j = from;
        while j < c.len() {
            if c[j..].starts_with(&e) {
                return j + e.len();
            }
            j += 1;
        }
        c.len()
    };
    while i < c.len() {
        let ch = c[i];
        let next = c.get(i + 1).copied();
        let end = match (ch, next) {
            ('\'', _) => {
                // a quoted string: '' inside is a quote
                let mut j = i + 1;
                loop {
                    match c.get(j) {
                        None => break c.len(),
                        Some('\'') if c.get(j + 1) == Some(&'\'') => j += 2,
                        Some('\'') => break j + 1,
                        _ => j += 1,
                    }
                }
            }
            ('"', _) => until(i + 1, "\""),
            ('-', Some('-')) => until(i + 2, "\n"),
            ('/', Some('*')) => until(i + 2, "*/"),
            ('$', Some('$')) => until(i + 2, "$$"),
            ('$', Some(n)) if n.is_ascii_lowercase() => {
                let mut j = i + 1;
                while j < c.len()
                    && (c[j].is_ascii_lowercase() || c[j].is_ascii_digit() || c[j] == '_')
                {
                    j += 1;
                }
                let name: String = c[i + 1..j].iter().collect();
                match values.get(&name) {
                    Some(v) => out.push_str(v),
                    None => bail!(
                        "${name} is used but not declared in the header (-- --{} TYPE)",
                        name.replace('_', "-")
                    ),
                }
                i = j;
                continue;
            }
            _ => {
                out.push(ch);
                i += 1;
                continue;
            }
        };
        out.extend(&c[i..end]);
        i = end;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECISION: &str = "-- A decision, with what was rejected.\n\
        --\n\
        -- --project  string        the place it is of\n\
        -- --why      string?       why\n\
        -- --size     integer = 3\n\
        -- --date     date = today  when\n\
        -- --gaps     list?\n\
        INSERT INTO decisions VALUES ($project, $why, $size, $date, '$project stays', $gaps); -- $project\n";

    fn given(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn the_header_declares_the_parameters() {
        let s = parse(Kind::Record, "decision", DECISION).unwrap();
        assert_eq!(s.summary, "A decision, with what was rejected.");
        let names: Vec<_> = s.params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["project", "why", "size", "date", "gaps"]);
        assert!(s.params[0].required && !s.params[1].required && !s.params[2].required);
        assert_eq!(
            s.usage(),
            "decision --project string [--why string] [--size integer] [--date date] [--gaps list]  - A decision, with what was rejected."
        );
    }

    #[test]
    fn values_are_written_as_literals_never_inside_quotes_or_comments() {
        let s = parse(Kind::Record, "decision", DECISION).unwrap();
        let sql = s
            .bind(&given(&[
                ("project", "it's mine"),
                ("date", "2026-11-17"),
                ("gaps", "a, b"),
            ]))
            .unwrap();
        assert!(
            sql.contains("VALUES ('it''s mine', NULL, 3, DATE '2026-11-17', '$project stays', ['a', 'b']::VARCHAR[]); -- $project"),
            "{sql}"
        );
    }

    #[test]
    fn what_is_not_of_its_type_or_not_declared_is_refused() {
        let s = parse(Kind::Record, "decision", DECISION).unwrap();
        let e = |g: &[(&str, &str)]| format!("{:#}", s.bind(&given(g)).unwrap_err());
        assert!(e(&[]).contains("needs --project"));
        assert!(e(&[("project", "x"), ("size", "3; DROP")]).contains("not an integer"));
        assert!(e(&[("project", "x"), ("date", "17.11.2026")]).contains("not a date"));
        assert!(e(&[("project", "x"), ("colour", "red")]).contains("takes no --colour"));
        assert!(parse(Kind::Query, "q", "-- q\nSELECT $nope")
            .unwrap_err()
            .to_string()
            .contains("$nope is used but not declared"));
        assert!(parse(Kind::Query, "q", "-- q\n-- --data string\nSELECT 1")
            .unwrap_err()
            .to_string()
            .contains("tad's own word"));
    }

    #[test]
    fn a_date_defaults_to_the_day_it_runs() {
        let s = parse(Kind::Record, "decision", DECISION).unwrap();
        let sql = s.bind(&given(&[("project", "x")])).unwrap();
        assert!(sql.contains(&format!("DATE '{}'", crate::today())), "{sql}");
    }

    #[test]
    fn statements_are_found_by_name_in_their_folder() {
        let root = std::env::temp_dir().join(format!("tad-statements-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("queries")).unwrap();
        std::fs::write(root.join("queries/open-items.sql"), "-- open\nSELECT 1").unwrap();
        std::fs::write(root.join("queries/README.md"), "").unwrap();
        assert_eq!(names(&root, Kind::Query), ["open-items"]);
        assert!(names(&root, Kind::Record).is_empty());
        assert_eq!(
            load(&root, Kind::Query, "open-items").unwrap().summary,
            "open"
        );
        assert!(load(&root, Kind::Query, "../x")
            .unwrap_err()
            .to_string()
            .contains("there are: open-items"));
        assert_eq!(root_of(Path::new("tables.toml")), PathBuf::from("."));
        assert_eq!(root_of(Path::new("a/tables.toml")), PathBuf::from("a"));
    }
}
