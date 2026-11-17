//! tad - a transparent database of text files in git.
use anyhow::{bail, Context, Result};
use argenv_resolve::generate::{self, Program, Shell};
use argenv_resolve::*;
use std::path::{Path, PathBuf};
use tad::statements::{self, Kind};

mod contract;
use contract::{Command, Contract};

const ABOUT: &str =
    "An open, transparent, easily accessible database: text files in git, tables declared as data, every row checked";

fn declaration(path: &Path) -> Result<tad::Declaration> {
    Ok(toml::from_str(
        &std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
    )?)
}

fn usage() -> String {
    let mut s = format!(
        "tad {}: {ABOUT}\n\nusage: tad COMMAND [...]   (tad man for the whole)\n\n",
        env!("CARGO_PKG_VERSION")
    );
    for c in Command::ALL {
        s.push_str(&format!("  {:<12} {}\n", c.token(), c.summary()));
    }
    s
}

/// The value given after a flag, as `--flag V` or `--flag=V`, wherever it stands.
fn flag_value(args: &[String], flag: &str) -> Option<String> {
    let long = format!("--{flag}");
    let eq = format!("--{flag}=");
    args.iter().enumerate().find_map(|(i, a)| {
        if a == &long {
            args.get(i + 1).cloned()
        } else {
            a.strip_prefix(&eq).map(str::to_string)
        }
    })
}

/// Splits a named statement's own parameters from tad's: each `--P V` (or `--P=V`) whose P the
/// statement declares is the statement's; everything else is read by tad's contract.
/// A flag's name and the value given it.
type Given = Vec<(String, String)>;

fn split_params(args: &[String], names: &[&str]) -> Result<(Vec<String>, Given)> {
    let mut rest = Vec::new();
    let mut params = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let named = a.strip_prefix("--").and_then(|n| {
            let (n, v) = match n.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (n, None),
            };
            names.contains(&n).then(|| (n.to_string(), v))
        });
        match named {
            Some((n, Some(v))) => params.push((n, v)),
            Some((n, None)) => {
                let v = args
                    .get(i + 1)
                    .with_context(|| format!("--{n} needs a value"))?;
                params.push((n, v.clone()));
                i += 1;
            }
            None => rest.push(a.clone()),
        }
        i += 1;
    }
    Ok((rest, params))
}

fn program<'a>(commands: &'a [generate::Command<'a>], date: &'a str) -> Program<'a> {
    Program {
        name: "tad",
        version: env!("CARGO_PKG_VERSION"),
        about: ABOUT,
        commands,
        exit_status: &[
            ("0", "done"),
            (
                "1",
                "the data does not check out, or a statement was refused - nothing was kept",
            ),
            ("2", "the invocation is wrong"),
        ],
        see_also: &["duckdb(1)", "git(1)"],
        date,
        asked: &[
            generate::Asked {
                command: "query",
                position: 0,
                ask: &["statements", "queries"],
            },
            generate::Asked {
                command: "record",
                position: 0,
                ask: &["statements", "records"],
            },
        ],
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("tad: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let Some(first) = args.first().cloned() else {
        print!("{}", usage());
        return Ok(());
    };
    match first.as_str() {
        "-h" | "--help" | "help" => {
            print!("{}", usage());
            return Ok(());
        }
        "-V" | "--version" => {
            println!("tad {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        _ => {}
    }
    let Some(command) = Command::parse(&first) else {
        eprint!("tad: '{first}' is not a command\n\n{}", usage());
        std::process::exit(2);
    };
    args[0] = command.token().to_string();

    // what tad's contract does not read: a named statement's parameters, and a SQL statement that
    // begins with a comment (it looks like a flag)
    let tables_given = flag_value(&args, "tables");
    let tables = PathBuf::from(tables_given.as_deref().unwrap_or("tables.toml"));
    let mut named: Option<(statements::Statement, Vec<(String, String)>)> = None;
    let mut sql_text: Option<String> = None;
    // and set's repeated --key and --set: argenv keeps the last of a repeated flag
    let mut repeated: Vec<(String, String)> = Vec::new();
    match command {
        Command::Query | Command::Record => {
            let kind = if command == Command::Query {
                Kind::Query
            } else {
                Kind::Record
            };
            if let Some(name) = args.get(1).filter(|n| !n.starts_with('-')) {
                let s = statements::load(&statements::root_of(&tables), kind, name)?;
                let names: Vec<&str> = s.params.iter().map(|p| p.name.as_str()).collect();
                let (rest, params) = split_params(&args, &names)?;
                args = rest;
                named = Some((s, params));
            }
        }
        Command::Set => {
            let (rest, taken) = split_params(&args, &["key", "set"])?;
            args = rest;
            repeated = taken;
        }
        Command::Sql
            if args
                .get(1)
                .is_some_and(|s| s.starts_with("--") && s.contains(char::is_whitespace)) =>
        {
            sql_text = Some(args.remove(1));
        }
        _ => {}
    }

    let resolved = Invocation {
        args: &args,
        env: &ProcessEnv,
    }
    .resolve(&Contract::records());
    let p = Contract::problems();
    assert!(p.is_empty(), "declaration errors: {p:#?}");
    handle_problems(
        &lint(&Contract::records(), &resolved),
        OnProblems::FailOnError,
    );
    let get = |input: &Input<String, Command>| {
        input
            .get_from_or_default(&resolved)
            .filter(|v| !v.is_empty())
    };
    let data = PathBuf::from(get(&Contract::DATA).unwrap_or_else(|| "data".into()));
    let format = get(&Contract::FORMAT);
    let many = |key: &str| -> Vec<String> {
        repeated
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .collect()
    };

    match command {
        Command::Init => {
            let dir = PathBuf::from(get(&Contract::DIR).context("init needs a DIR (. for here)")?);
            let written = tad::init::init(&dir, env!("CARGO_PKG_VERSION"))?;
            println!(
                "a bare database in {}: {}\nnext: declare its tables in tables.toml; after each change tad canon && tad render; commit it all",
                dir.display(),
                written.join(", ")
            );
        }
        Command::Declare => {
            let from = get(&Contract::FROM_SQL).context("declare needs --from-sql FILE")?;
            let sql = std::fs::read_to_string(&from).with_context(|| format!("reading {from}"))?;
            print!("{}", toml::to_string(&tad::from_sql(&sql)?)?);
        }
        Command::Check => {
            let d = match (tables_given, get(&Contract::FROM_SQL)) {
                (Some(_), Some(_)) => bail!("give --tables or --from-sql, not both"),
                (None, Some(s)) => tad::from_sql(
                    &std::fs::read_to_string(&s).with_context(|| format!("reading {s}"))?,
                )?,
                _ => declaration(&tables)?,
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
        Command::Canon => {
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
        Command::Render => {
            let names = format
                .or_else(|| std::env::var("DOCS_FORMAT").ok())
                .unwrap_or_else(|| "md".into());
            let mut list: Vec<String> = Vec::new();
            for n in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                if !list.iter().any(|x| x == n) {
                    list.push(n.to_string());
                }
            }
            let docs = PathBuf::from(get(&Contract::DOCS).unwrap_or_else(|| "docs".into()));
            println!(
                "{}",
                tad::render::render(&data, &declaration(&tables)?, &docs, &list)?
            );
        }
        Command::Engine => {
            let path = tad::sql::install_engine()?;
            println!(
                "DuckDB {} installed for tad at {}",
                tad::sql::DUCKDB_VERSION,
                path.display()
            );
        }
        Command::Sql => {
            let statement = match (
                sql_text.or_else(|| get(&Contract::STATEMENT)),
                get(&Contract::FILE),
            ) {
                (Some(s), None) => s,
                (None, Some(f)) => {
                    std::fs::read_to_string(&f).with_context(|| format!("reading {f}"))?
                }
                _ => bail!("give a SQL statement, or --file FILE"),
            };
            let engine = tad::sql::DuckDb::find()?;
            let outcome = tad::sql::sql(
                &data,
                &declaration(&tables)?,
                &statement,
                &engine,
                format.as_deref().unwrap_or("box"),
            )?;
            print!("{}", outcome.printed);
            if !outcome.changed.is_empty() {
                println!("changed: {}", outcome.changed.join(", "));
            }
        }
        Command::Query | Command::Record => {
            let (s, params) = named.context(if command == Command::Query {
                "name the query: tad query NAME (tad statements lists them)"
            } else {
                "name the record: tad record NAME (tad statements lists them)"
            })?;
            let body = s.bind(&params)?;
            let engine = tad::sql::DuckDb::find()?;
            let d = declaration(&tables)?;
            let format = format.as_deref().unwrap_or("box");
            let outcome = if command == Command::Query {
                tad::sql::query(&data, &d, &body, &engine, format)
            } else {
                tad::sql::sql(&data, &d, &body, &engine, format)
            }
            .with_context(|| format!("{} {}", command.token(), s.name))?;
            print!("{}", outcome.printed);
            if !outcome.changed.is_empty() {
                println!("changed: {}", outcome.changed.join(", "));
            }
        }
        Command::Statements => {
            let root = statements::root_of(&tables);
            match get(&Contract::KIND) {
                Some(kind) => {
                    let kind = Kind::from_folder(&kind).context("queries or records")?;
                    for n in statements::names(&root, kind) {
                        println!("{n}");
                    }
                }
                None => {
                    for kind in Kind::ALL {
                        let names = statements::names(&root, kind);
                        if names.is_empty() {
                            continue;
                        }
                        println!("{}/", kind.folder());
                        for n in names {
                            match statements::load(&root, kind, &n) {
                                Ok(s) => println!("  {}", s.usage()),
                                Err(e) => println!("  {n}  - does not read: {e:#}"),
                            }
                        }
                    }
                }
            }
        }
        Command::Set => {
            let table = get(&Contract::TABLE).context("set needs a TABLE")?;
            let key = many("key");
            if key.is_empty() {
                bail!("set needs --key VALUE (repeat for a key of several fields)");
            }
            let mut changes = Vec::new();
            for s in many("set") {
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
        Command::Completions => {
            let shell = get(&Contract::SHELL)
                .and_then(|s| Shell::parse(&s))
                .context("bash, zsh, fish or nushell")?;
            let commands = commands();
            print!(
                "{}",
                generate::completions(shell, &program(&commands, ""), &Contract::records())?
            );
        }
        Command::Man => {
            let date = get(&Contract::DATE).unwrap_or_else(tad::today);
            let commands = commands();
            print!(
                "{}",
                generate::man_page(&program(&commands, &date), &Contract::records())?
            );
        }
    }
    Ok(())
}

fn commands() -> Vec<generate::Command<'static>> {
    Command::ALL
        .iter()
        .map(|c| generate::Command {
            name: c.token(),
            summary: c.summary(),
        })
        .collect()
}
