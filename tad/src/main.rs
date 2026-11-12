//! tad - a transparent database of text files in git.
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "tad",
    version,
    about = "A transparent database of text files in git: tables declared as data, every row checked"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Translate a schema written in SQL into a declaration (TOML), refusing what it does not know
    Declare {
        /// The schema, as DuckDB writes it
        #[arg(long)]
        from_sql: PathBuf,
    },
    /// Check every row of every declared table, and the rules across tables
    Check {
        /// The declaration (default: tables.toml)
        #[arg(long, conflicts_with = "from_sql")]
        tables: Option<PathBuf>,
        /// Or a schema written in SQL, translated on the fly
        #[arg(long)]
        from_sql: Option<PathBuf>,
        /// The folder holding one <table>.json file per table
        #[arg(long, default_value = "data")]
        data: PathBuf,
    },
    /// Rewrite every table in canonical form, only when the data checks out
    Canon {
        #[arg(long, default_value = "tables.toml")]
        tables: PathBuf,
        #[arg(long, default_value = "data")]
        data: PathBuf,
    },
    /// Render documents from the data: one page per table, in Markdown and/or reStructuredText
    Render {
        #[arg(long, default_value = "tables.toml")]
        tables: PathBuf,
        #[arg(long, default_value = "data")]
        data: PathBuf,
        #[arg(long, default_value = "docs")]
        docs: PathBuf,
        /// md, rst, or md,rst (default: DOCS_FORMAT, else md)
        #[arg(long)]
        format: Option<String>,
    },
    /// Ask the data in SQL (DuckDB): a query changes nothing; a change is kept only if every rule still holds
    Sql {
        /// The SQL statement (it may begin with a comment), or give --file
        #[arg(allow_hyphen_values = true)]
        statement: Option<String>,
        /// Read the statement from a file instead
        #[arg(long, conflicts_with = "statement")]
        file: Option<PathBuf>,
        /// How rows are printed: box, csv, json, markdown, table or line
        #[arg(long, default_value = "box")]
        format: String,
        #[arg(long, default_value = "tables.toml")]
        tables: PathBuf,
        #[arg(long, default_value = "data")]
        data: PathBuf,
    },
    /// Add or update one row, checked: a change that would break a rule is not kept
    Set {
        /// The table
        table: String,
        /// The row's key value(s), in the key's order (repeat for a key of several fields)
        #[arg(long = "key", required = true)]
        key: Vec<String>,
        /// FIELD=VALUE (repeatable; an empty value is null; a list is comma-separated)
        #[arg(long = "set")]
        set: Vec<String>,
        #[arg(long, default_value = "tables.toml")]
        tables: PathBuf,
        #[arg(long, default_value = "data")]
        data: PathBuf,
    },
}

fn declaration(path: &Path) -> Result<tad::Declaration> {
    Ok(toml::from_str(
        &std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
    )?)
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Declare { from_sql } => {
            let sql = std::fs::read_to_string(&from_sql)
                .with_context(|| format!("reading {}", from_sql.display()))?;
            print!("{}", toml::to_string(&tad::from_sql(&sql)?)?);
        }
        Cmd::Check {
            tables,
            from_sql,
            data,
        } => {
            let d = match (tables, from_sql) {
                (Some(t), _) => declaration(&t)?,
                (None, Some(s)) => tad::from_sql(
                    &std::fs::read_to_string(&s)
                        .with_context(|| format!("reading {}", s.display()))?,
                )?,
                (None, None) => declaration(Path::new("tables.toml"))?,
            };
            let (rows, problems) = tad::check(&data, &d)?;
            for p in &problems {
                println!("{p}");
            }
            if problems.is_empty() {
                println!("all is well: {} tables, {rows} rows", d.tables.len());
            } else {
                println!("{} problems in {rows} rows", problems.len());
                std::process::exit(1);
            }
        }
        Cmd::Canon { tables, data } => {
            let changed = tad::canon(&data, &declaration(&tables)?)?;
            println!(
                "{}",
                if changed.is_empty() {
                    "canonical already".to_string()
                } else {
                    format!("canonical now: {}", changed.join(", "))
                }
            );
        }
        Cmd::Render {
            tables,
            data,
            docs,
            format,
        } => {
            let names = format
                .or_else(|| std::env::var("DOCS_FORMAT").ok())
                .unwrap_or_else(|| "md".into());
            let mut list: Vec<String> = Vec::new();
            for n in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                if !list.iter().any(|x| x == n) {
                    list.push(n.to_string());
                }
            }
            println!(
                "{}",
                tad::render::render(&data, &declaration(&tables)?, &docs, &list)?
            );
        }
        Cmd::Sql {
            statement,
            file,
            format,
            tables,
            data,
        } => {
            let statement = match (statement, file) {
                (Some(s), None) => s,
                (None, Some(f)) => std::fs::read_to_string(&f)
                    .with_context(|| format!("reading {}", f.display()))?,
                _ => bail!("give a SQL statement, or --file FILE"),
            };
            let engine = tad::sql::DuckDb::find()?;
            let outcome =
                tad::sql::sql(&data, &declaration(&tables)?, &statement, &engine, &format)?;
            print!("{}", outcome.printed);
            if !outcome.changed.is_empty() {
                println!("changed: {}", outcome.changed.join(", "));
            }
        }
        Cmd::Set {
            table,
            key,
            set,
            tables,
            data,
        } => {
            let mut changes = Vec::new();
            for s in &set {
                let Some((f, v)) = s.split_once('=') else {
                    bail!("'{s}' is not FIELD=VALUE")
                };
                changes.push((f.trim().to_string(), v.to_string()));
            }
            println!(
                "{}",
                tad::set(&data, &declaration(&tables)?, &table, &key, &changes)?
            );
        }
    }
    Ok(())
}
