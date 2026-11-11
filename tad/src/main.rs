//! tad - a transparent database of text files in git.
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
    /// Check every row of every declared table
    Check {
        /// The declaration (TOML)
        #[arg(long, conflicts_with = "from_sql")]
        tables: Option<PathBuf>,
        /// Or a schema written in SQL, translated on the fly
        #[arg(long)]
        from_sql: Option<PathBuf>,
        /// The folder holding one <table>.json file per table
        #[arg(long, default_value = "data")]
        data: PathBuf,
    },
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
                (Some(t), _) => toml::from_str(
                    &std::fs::read_to_string(&t)
                        .with_context(|| format!("reading {}", t.display()))?,
                )?,
                (None, Some(s)) => tad::from_sql(
                    &std::fs::read_to_string(&s)
                        .with_context(|| format!("reading {}", s.display()))?,
                )?,
                (None, None) => {
                    toml::from_str(&std::fs::read_to_string(data.join("tables.toml")).context(
                        "no declaration: give --tables or --from-sql, or keep data/tables.toml",
                    )?)?
                }
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
    }
    Ok(())
}
