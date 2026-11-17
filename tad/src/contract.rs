//! tad's command line, declared once with argenv: every command and every input it accepts. Parsing,
//! the help, the shell completions and the man page all come from these declarations, so none of them
//! can disagree with what tad accepts. A named statement's own parameters are not declared here: each
//! statement declares them in its header, and tad reads them against it (see statements.rs).
use argenv_resolve::*;

/// Every command.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    Check,
    Canon,
    Set,
    Sql,
    Query,
    Record,
    Init,
    Statements,
    Render,
    Engine,
    Declare,
    Completions,
    Man,
}

impl Command {
    pub const ALL: &'static [Command] = &[
        Command::Init,
        Command::Check,
        Command::Canon,
        Command::Set,
        Command::Sql,
        Command::Query,
        Command::Record,
        Command::Statements,
        Command::Render,
        Command::Engine,
        Command::Declare,
        Command::Completions,
        Command::Man,
    ];

    /// How it is typed.
    pub const fn token(self) -> &'static str {
        match self {
            Command::Init => "init",
            Command::Check => "check",
            Command::Canon => "canon",
            Command::Set => "set",
            Command::Sql => "sql",
            Command::Query => "query",
            Command::Record => "record",
            Command::Statements => "statements",
            Command::Render => "render",
            Command::Engine => "engine",
            Command::Declare => "declare",
            Command::Completions => "completions",
            Command::Man => "man",
        }
    }

    /// One line for the help and the man page.
    pub const fn summary(self) -> &'static str {
        match self {
            Command::Init => "Start a bare database in DIR: the declaration, data/, tad pinned to this release, and the check CI runs (nothing is written where one is there)",
            Command::Check => "Check every row of every declared table, and the rules across tables",
            Command::Canon => "Rewrite every table in canonical form, only when the data checks out",
            Command::Set => "Add or update one row, checked: set TABLE --key K --set FIELD=VALUE ... (a change that would break a rule is not kept)",
            Command::Sql => "Ask the data in SQL (DuckDB): a query changes nothing; a change is kept only if every rule still holds",
            Command::Query => "Run a named query, queries/NAME.sql: query NAME --PARAM VALUE ... (short: q); it reads and changes nothing",
            Command::Record => "Run a named record, records/NAME.sql: record NAME --PARAM VALUE ... (short: r); what it writes is kept only if every rule holds",
            Command::Statements => "The named statements this place keeps, with their parameters; statements queries|records prints their names alone",
            Command::Render => "Render documents from the data: one page per table, in Markdown and/or reStructuredText",
            Command::Engine => "Install the SQL engine tad uses: DuckDB's pinned binary, checked by fingerprint",
            Command::Declare => "Translate a schema written in SQL into a declaration (TOML), refusing what it does not know",
            Command::Completions => "Print shell completions: bash, zsh, fish or nushell",
            Command::Man => "Print the man page",
        }
    }

    /// The command a word names; `q` and `r` are short for query and record.
    pub fn parse(token: &str) -> Option<Command> {
        match token {
            "q" => Some(Command::Query),
            "r" => Some(Command::Record),
            _ => Command::ALL.iter().copied().find(|c| c.token() == token),
        }
    }
}

impl serde::Serialize for Command {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.token())
    }
}

contract! {
    /// The declaration.
    TABLES: String, Command = Input {
        key:         "tables",
        ty:          Type::Path,
        default:     Some(String::new()),
        subcommands: &[Command::Check, Command::Canon, Command::Set, Command::Sql, Command::Query, Command::Record, Command::Statements, Command::Render],
        arg:         Some(Arg { value_name: "FILE", ..Arg::long("tables") }),
        summary:     "The declaration (default: tables.toml); named statements are kept beside it",
        ..Input::EMPTY
    };

    /// The folder of the tables.
    DATA: String, Command = Input {
        key:         "data",
        ty:          Type::Path,
        default:     Some(String::new()),
        subcommands: &[Command::Check, Command::Canon, Command::Set, Command::Sql, Command::Query, Command::Record, Command::Render],
        arg:         Some(Arg { value_name: "DIR", ..Arg::long("data") }),
        summary:     "The folder holding one TABLE.json file per table (default: data)",
        ..Input::EMPTY
    };

    /// How the output is written.
    FORMAT: String, Command = Input {
        key:         "format",
        ty:          Type::String,
        subcommands: &[Command::Sql, Command::Query, Command::Record, Command::Render],
        arg:         Some(Arg { value_name: "FORMAT", ..Arg::long("format") }),
        summary:     "How it is written: rows as box, csv, json, markdown, table or line (default box); pages as md, rst or md,rst (default DOCS_FORMAT, else md)",
        ..Input::EMPTY
    };

    /// A schema in SQL.
    FROM_SQL: String, Command = Input {
        key:         "from_sql",
        ty:          Type::Path,
        subcommands: &[Command::Declare, Command::Check],
        arg:         Some(Arg { value_name: "FILE", ..Arg::long("from-sql") }),
        summary:     "A schema written in SQL, as DuckDB writes it, translated into a declaration",
        ..Input::EMPTY
    };

    /// Where pages are rendered.
    DOCS: String, Command = Input {
        key:         "docs",
        ty:          Type::Path,
        subcommands: &[Command::Render],
        arg:         Some(Arg { value_name: "DIR", ..Arg::long("docs") }),
        summary:     "Where the pages are written (default: docs)",
        ..Input::EMPTY
    };

    /// The SQL statement.
    STATEMENT: String, Command = Input {
        key:         "statement",
        ty:          Type::String,
        subcommands: &[Command::Sql],
        arg:         Some(Arg { value_name: "SQL", ..Arg::positional(0) }),
        summary:     "The SQL statement (it may begin with a comment), or give --file",
        ..Input::EMPTY
    };

    /// A file holding the SQL statement.
    FILE: String, Command = Input {
        key:         "file",
        ty:          Type::Path,
        subcommands: &[Command::Sql],
        arg:         Some(Arg { value_name: "FILE", ..Arg::long("file") }),
        summary:     "Read the statement from a file instead",
        ..Input::EMPTY
    };

    /// The table a row is set in.
    TABLE: String, Command = Input {
        key:         "table",
        ty:          Type::String,
        required:    true,
        subcommands: &[Command::Set],
        arg:         Some(Arg { value_name: "TABLE", ..Arg::positional(0) }),
        summary:     "The table",
        ..Input::EMPTY
    };

    /// The row's key.
    KEY: String, Command = Input {
        key:         "key",
        ty:          Type::String,
        subcommands: &[Command::Set],
        arg:         Some(Arg { value_name: "VALUE", ..Arg::long("key") }),
        summary:     "The row's key value(s), in the key's order (repeat for a key of several fields)",
        ..Input::EMPTY
    };

    /// A field's new value.
    SET: String, Command = Input {
        key:         "set",
        ty:          Type::String,
        subcommands: &[Command::Set],
        arg:         Some(Arg { value_name: "FIELD=VALUE", ..Arg::long("set") }),
        summary:     "FIELD=VALUE (repeatable; an empty value is null; a list is comma-separated)",
        ..Input::EMPTY
    };

    /// The named statement.
    NAME: String, Command = Input {
        key:         "name",
        ty:          Type::String,
        required:    true,
        subcommands: &[Command::Query, Command::Record],
        arg:         Some(Arg { value_name: "NAME", ..Arg::positional(0) }),
        summary:     "The statement's name: queries/NAME.sql or records/NAME.sql beside the declaration; its parameters follow it as --PARAM VALUE",
        ..Input::EMPTY
    };

    /// Which statements to name.
    KIND: String, Command = Input {
        key:         "kind",
        ty:          Type::Enum,
        allowed:     &["queries", "records"],
        subcommands: &[Command::Statements],
        arg:         Some(Arg { value_name: "KIND", ..Arg::positional(0) }),
        summary:     "queries or records: their names alone, one a line (what completion asks)",
        ..Input::EMPTY
    };

    /// Which shell.
    SHELL: String, Command = Input {
        key:         "shell",
        ty:          Type::Enum,
        required:    true,
        allowed:     &["bash", "zsh", "fish", "nushell"],
        subcommands: &[Command::Completions],
        arg:         Some(Arg { value_name: "SHELL", ..Arg::positional(0) }),
        summary:     "The shell to print completions for; statement names are asked of tad as they are completed",
        ..Input::EMPTY
    };

    /// Where a database is started.
    DIR: String, Command = Input {
        key:         "dir",
        ty:          Type::Path,
        required:    true,
        subcommands: &[Command::Init],
        arg:         Some(Arg { value_name: "DIR", ..Arg::positional(0) }),
        summary:     "The folder the database is started in (. for here); it is made if it is not there",
        ..Input::EMPTY
    };

    /// The man page's date.
    DATE: String, Command = Input {
        key:         "date",
        ty:          Type::String,
        subcommands: &[Command::Man],
        arg:         Some(Arg { value_name: "YYYY-MM-DD", ..Arg::long("date") }),
        summary:     "The man page's date, so that it is reproducible (default: today)",
        ..Input::EMPTY
    };
}
