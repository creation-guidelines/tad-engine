//! Documents rendered from data - a port of tad's Python renderer, output byte for byte. One page
//! per table, one section per row; shapes are detected from content, not names: relation tables
//! (from_id, to_id, relation) fold into the pages they connect; sources tables (title, url) and
//! two-column citation tables become "Sources" and "Cited by"; a table with a status and two
//! <x>_id columns is drawn as a grid; a table named about with (title, summary) heads the index.
use crate::{Declaration, Table};
use anyhow::{bail, Context, Result};
use fancy_regex::Regex as Fancy;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

type Row = Map<String, Value>;

/// Python's str() of a value.
fn py_str(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "None".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => if *b { "True" } else { "False" }.into(),
        Some(Value::Array(a)) => format!(
            "[{}]",
            a.iter()
                .map(|x| match x {
                    Value::String(s) => format!("'{s}'"),
                    o => py_str(Some(o)),
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Some(other) => other.to_string(),
    }
}

fn key(v: Option<&Value>) -> Option<String> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(o) => Some(o.to_string()),
    }
}

/// The renderer's fmt(): a value as text, or nothing when empty.
fn fmt(v: Option<&Value>) -> Option<String> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::Array(a)) => Some(
            a.iter()
                .filter_map(|i| fmt(Some(i)))
                .collect::<Vec<_>>()
                .join(", "),
        )
        .filter(|s| !s.is_empty()),
        Some(Value::Object(o)) => Some(
            o.iter()
                .filter_map(|(k, x)| fmt(Some(x)).map(|x| format!("{k}: {x}")))
                .collect::<Vec<_>>()
                .join(", "),
        )
        .filter(|s| !s.is_empty()),
        Some(Value::Bool(b)) => Some(if *b { "yes" } else { "no" }.into()),
        Some(Value::String(s)) => Some(s.clone()).filter(|s| !s.is_empty()),
        Some(n) => Some(n.to_string()),
    }
}

/// Python's str.title().
fn title_case(s: &str) -> String {
    let mut out = String::new();
    let mut prev = false;
    for c in s.chars() {
        let cased = c.is_lowercase() || c.is_uppercase();
        if cased {
            if prev {
                out.extend(c.to_lowercase())
            } else {
                out.extend(c.to_uppercase())
            }
        } else {
            out.push(c);
        }
        prev = cased;
    }
    out
}

/// Python's str.capitalize().
fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f
            .to_uppercase()
            .chain(c.flat_map(char::to_lowercase))
            .collect(),
        None => String::new(),
    }
}

fn words(t: &str) -> String {
    t.replace('_', " ")
}

fn width(text: &str) -> usize {
    use unicode_width::UnicodeWidthChar;
    text.chars()
        .map(|c| if c.width() == Some(2) { 2 } else { 1 })
        .sum()
}

fn rst_id(rid: &str) -> String {
    let lower = rid.to_lowercase();
    let mut out = String::new();
    let mut gap = false;
    for c in lower.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if gap && !out.is_empty() {
                out.push('-');
            }
            gap = false;
            out.push(c);
        } else {
            gap = true;
        }
    }
    out
}

struct Patterns {
    underscore: Fancy,
    line_start: Fancy,
    inline: Fancy,
}

fn patterns() -> Result<Patterns> {
    Ok(Patterns {
        underscore: Fancy::new(r"(?<=\w)_(?=\W|$)")?,
        line_start: Fancy::new(r"(?m)^(?=[-+>] )")?,
        inline: Fancy::new(
            r"(?P<code>`(?P<c>[^`]+)`)|(?P<link>\[(?P<lt>[^\]]+)\]\((?P<lu>[^)\s]+)\))|(?P<bold>\*\*(?P<b>.+?)\*\*)|(?P<em>(?<![\w*])\*(?P<e>[^\s*][^*]*?)(?<!\s)\*(?![\w*]))",
        )?,
    })
}

fn rst_escape(p: &Patterns, s: &str) -> String {
    let s = s
        .replace('\\', "\\\\")
        .replace('*', "\\*")
        .replace('`', "\\`")
        .replace('|', "\\|");
    let s = p.underscore.replace_all(&s, "\\_").into_owned();
    p.line_start.replace_all(&s, "\\").into_owned()
}

fn md_inline_to_rst(p: &Patterns, s: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut pos = 0;
    let alnum_end = |out: &Vec<String>| {
        out.last()
            .and_then(|x| x.chars().last())
            .is_some_and(char::is_alphanumeric)
    };
    for m in p.inline.captures_iter(s) {
        let Ok(m) = m else { break };
        let whole = m.get(0).expect("a match");
        out.push(rst_escape(p, &s[pos..whole.start()]));
        let rest = &s[whole.end()..];
        let markup = if let Some(c) = m.name("c") {
            format!("``{}``", c.as_str())
        } else if let (Some(lt), Some(lu)) = (m.name("lt"), m.name("lu")) {
            format!("`{} <{}>`__", rst_escape(p, lt.as_str()), lu.as_str())
        } else if let Some(b) = m.name("b") {
            format!("**{}**", rst_escape(p, b.as_str()))
        } else {
            format!(
                "*{}*",
                rst_escape(p, m.name("e").map(|e| e.as_str()).unwrap_or(""))
            )
        };
        if alnum_end(&out) {
            out.push("\\ ".into());
        }
        out.push(markup);
        if rest.chars().next().is_some_and(char::is_alphanumeric) {
            out.push("\\ ".into());
        }
        pos = whole.end();
    }
    out.push(rst_escape(p, &s[pos..]));
    out.concat()
}

/// A document format: how each piece of a page is written.
trait Format {
    fn ext(&self) -> &'static str;
    fn notice(&self) -> &'static str;
    fn text(&self, s: &str) -> String;
    fn link(&self, label: &str, page: &str, rid: &str) -> String;
    fn url(&self, title: &str, url: &str) -> String;
    fn code(&self, s: &str) -> String;
    fn title(&self, text: &str) -> Vec<String>;
    fn anchor(&self, rid: &str) -> Vec<String>;
    fn heading(&self, text: &str) -> Vec<String>;
    fn field(&self, label: &str, value: &str) -> Vec<String>;
    fn many(&self, label: &str, items: &[String]) -> Vec<String>;
    fn rule(&self) -> Vec<String>;
    fn index_item(&self, label: &str, page: &str, count: &str) -> String;
    fn table(&self, header: &[String], rows: &[Vec<String>]) -> Vec<String>;
    fn para(&self, text: &str) -> Vec<String> {
        vec![text.to_string(), String::new()]
    }
    fn heading_at(&self, text: &str, level: usize) -> Vec<String>;
    fn list_item(&self, text: &str) -> String {
        format!("- {text}")
    }
}

struct Markdown;
struct Rst(Patterns);

impl Format for Markdown {
    fn ext(&self) -> &'static str {
        "md"
    }
    fn notice(&self) -> &'static str {
        "<!-- Generated from data/ by tad. Do not edit by hand. -->"
    }
    fn text(&self, s: &str) -> String {
        s.into()
    }
    fn link(&self, label: &str, page: &str, rid: &str) -> String {
        format!("[{label}]({page}.md#{rid})")
    }
    fn url(&self, title: &str, url: &str) -> String {
        format!("[{title}]({url})")
    }
    fn code(&self, s: &str) -> String {
        format!("`{s}`")
    }
    fn title(&self, text: &str) -> Vec<String> {
        vec![format!("# {text}"), String::new()]
    }
    fn anchor(&self, rid: &str) -> Vec<String> {
        vec![format!("<a id=\"{rid}\"></a>")]
    }
    fn heading(&self, text: &str) -> Vec<String> {
        vec![format!("### {text}"), String::new()]
    }
    fn heading_at(&self, text: &str, level: usize) -> Vec<String> {
        vec![format!("{} {text}", "#".repeat(level)), String::new()]
    }
    fn field(&self, label: &str, value: &str) -> Vec<String> {
        vec![label.into(), format!(": {value}"), String::new()]
    }
    fn many(&self, label: &str, items: &[String]) -> Vec<String> {
        let mut v = vec![label.to_string()];
        v.extend(items.iter().map(|i| format!(": {i}")));
        v.push(String::new());
        v
    }
    fn rule(&self) -> Vec<String> {
        vec!["---".into(), String::new()]
    }
    fn index_item(&self, label: &str, page: &str, count: &str) -> String {
        format!("- [{label}]({page}.md): {count}")
    }
    fn table(&self, header: &[String], rows: &[Vec<String>]) -> Vec<String> {
        let cell = |v: &String| v.replace('|', "\\|").replace('\n', " ");
        let mut out = vec![
            format!(
                "| {} |",
                header.iter().map(cell).collect::<Vec<_>>().join(" | ")
            ),
            format!(
                "|{}|",
                header.iter().map(|_| "---").collect::<Vec<_>>().join("|")
            ),
        ];
        out.extend(
            rows.iter()
                .map(|r| format!("| {} |", r.iter().map(cell).collect::<Vec<_>>().join(" | "))),
        );
        out.push(String::new());
        out
    }
}

impl Rst {
    fn underlined(&self, text: &str, ch: char) -> Vec<String> {
        let t = md_inline_to_rst(&self.0, text);
        let line: String = std::iter::repeat_n(ch, width(&t)).collect();
        vec![t, line, String::new()]
    }
}

impl Format for Rst {
    fn ext(&self) -> &'static str {
        "rst"
    }
    fn notice(&self) -> &'static str {
        ".. Generated from data/ by tad. Do not edit by hand."
    }
    fn text(&self, s: &str) -> String {
        md_inline_to_rst(&self.0, s)
    }
    fn link(&self, label: &str, page: &str, rid: &str) -> String {
        format!(
            "`{} <{page}.rst#{}>`__",
            rst_escape(&self.0, label),
            rst_id(rid)
        )
    }
    fn url(&self, title: &str, url: &str) -> String {
        format!("`{} <{url}>`__", rst_escape(&self.0, title))
    }
    fn code(&self, s: &str) -> String {
        format!("``{s}``")
    }
    fn title(&self, text: &str) -> Vec<String> {
        self.underlined(text, '=')
    }
    fn anchor(&self, rid: &str) -> Vec<String> {
        vec![format!(".. _{}:", rst_id(rid)), String::new()]
    }
    fn heading(&self, text: &str) -> Vec<String> {
        self.underlined(text, '-')
    }
    fn heading_at(&self, text: &str, level: usize) -> Vec<String> {
        self.underlined(text, ['=', '-', '~', '^'][level.clamp(1, 4) - 1])
    }
    fn field(&self, label: &str, value: &str) -> Vec<String> {
        let mut v = vec![label.to_string()];
        v.extend(value.split('\n').map(|l| {
            if l.is_empty() {
                String::new()
            } else {
                format!("    {l}")
            }
        }));
        v.push(String::new());
        v
    }
    fn many(&self, label: &str, items: &[String]) -> Vec<String> {
        let mut v = vec![label.to_string()];
        for i in items {
            let mut parts = i.split('\n');
            v.push(format!("    - {}", parts.next().unwrap_or("")));
            v.extend(parts.map(|r| format!("      {r}")));
        }
        v.push(String::new());
        v
    }
    fn rule(&self) -> Vec<String> {
        vec!["----".into(), String::new()]
    }
    fn index_item(&self, label: &str, page: &str, count: &str) -> String {
        format!("- `{} <{page}.rst>`__: {count}", rst_escape(&self.0, label))
    }
    fn table(&self, header: &[String], rows: &[Vec<String>]) -> Vec<String> {
        let mut out = vec![
            ".. list-table::".to_string(),
            "   :header-rows: 1".into(),
            String::new(),
        ];
        for r in std::iter::once(header.to_vec()).chain(rows.iter().cloned()) {
            for (i, v) in r.iter().enumerate() {
                out.push(format!(
                    "{}{}",
                    if i == 0 { "   * - " } else { "     - " },
                    v.replace('\n', " ")
                ));
            }
        }
        out.push(String::new());
        out
    }
}

const STATUS_MARKS: [(&str, &str); 6] = [
    ("works", "\u{2705}"),
    ("partial", "\u{1F7E1}"),
    ("broken", "\u{274C}"),
    ("missing", "\u{274C}"),
    ("unknown", "\u{2754}"),
    ("not-applicable", "\u{2014}"),
];
const GROUP_COLUMNS: [&str; 2] = ["category", "group"];

fn mark(status: &str) -> String {
    STATUS_MARKS
        .iter()
        .find(|(s, _)| *s == status)
        .map(|(_, m)| m.to_string())
        .unwrap_or_else(|| status.to_string())
}

struct Site<'a> {
    d: &'a Declaration,
    rows: BTreeMap<String, Vec<Row>>,
    relation_tables: Vec<String>,
    sources_tables: Vec<String>,
    citation_links: Vec<(String, String, String, String)>,
    page_tables: Vec<String>,
    matrix: HashMap<String, (String, String, String, String)>,
    index: HashMap<String, (String, String)>,
    presentation: Option<&'a SitePresentation>,
}

impl<'a> Site<'a> {
    fn cols(&self, t: &str) -> Vec<&str> {
        self.d
            .tables
            .get(t)
            .map(|x: &Table| x.fields.iter().map(|f| f.name.as_str()).collect())
            .unwrap_or_default()
    }
    fn has(&self, t: &str, c: &str) -> bool {
        self.cols(t).contains(&c)
    }
    fn rows(&self, t: &str) -> &[Row] {
        self.rows.get(t).map(Vec::as_slice).unwrap_or(&[])
    }

    fn new(d: &'a Declaration, dir: &Path) -> Result<Site<'a>> {
        let mut rows = BTreeMap::new();
        for t in d.tables.keys() {
            let text = std::fs::read_to_string(dir.join(format!("{t}.json"))).unwrap_or_default();
            let mut v = Vec::new();
            for l in text.lines().filter(|l| !l.trim().is_empty()) {
                match serde_json::from_str::<Value>(l)
                    .with_context(|| format!("{t}: a line is not JSON"))?
                {
                    Value::Object(m) => v.push(m),
                    _ => bail!("{t}: a line is not an object"),
                }
            }
            rows.insert(t.clone(), v);
        }
        let mut s = Site {
            d,
            rows,
            relation_tables: vec![],
            sources_tables: vec![],
            citation_links: vec![],
            page_tables: vec![],
            matrix: HashMap::new(),
            index: HashMap::new(),
            presentation: d.site.as_ref(),
        };
        let tables: Vec<String> = d.tables.keys().cloned().collect();
        s.relation_tables = tables
            .iter()
            .filter(|t| ["from_id", "to_id", "relation"].iter().all(|c| s.has(t, c)))
            .cloned()
            .collect();
        s.sources_tables = tables
            .iter()
            .filter(|t| s.has(t, "title") && s.has(t, "url"))
            .cloned()
            .collect();
        let about = tables
            .iter()
            .find(|t| t.as_str() == "about" && s.has(t, "title") && s.has(t, "summary"))
            .cloned();
        let mut links = Vec::new();
        for t in tables
            .iter()
            .filter(|t| !s.relation_tables.contains(t) && !s.sources_tables.contains(t))
        {
            for src in &s.sources_tables {
                if let Some((cited, source)) = s.citation_columns(t, src) {
                    links.push((t.clone(), src.clone(), cited, source));
                }
            }
        }
        s.citation_links = links;
        let link_tables: HashSet<&String> = s.citation_links.iter().map(|(t, ..)| t).collect();
        s.page_tables = tables
            .iter()
            .filter(|t| {
                !s.relation_tables.contains(t)
                    && !link_tables.contains(t)
                    && Some(*t) != about.as_ref()
            })
            .cloned()
            .collect();
        let shapes: Vec<(String, (String, String, String, String))> = s
            .page_tables
            .iter()
            .filter_map(|t| s.matrix_shape(t).map(|m| (t.clone(), m)))
            .collect();
        s.matrix = shapes.into_iter().collect();
        let mut index = HashMap::new();
        for t in &s.page_tables {
            if !s.has(t, "id") {
                continue;
            }
            let label = if s.has(t, "name") {
                "name"
            } else if s.has(t, "title") {
                "title"
            } else {
                "id"
            };
            for r in s.rows(t) {
                if let Some(id) = key(r.get("id")) {
                    index.insert(id, (py_str(r.get(label)), t.clone()));
                }
            }
        }
        s.index = index;
        Ok(s)
    }

    fn citation_columns(&self, t: &str, src: &str) -> Option<(String, String)> {
        let c = self.cols(t);
        if c.len() != 2 {
            return None;
        }
        let ids: HashSet<Option<String>> =
            self.rows(src).iter().map(|r| key(r.get("id"))).collect();
        let values = |col: &str| -> HashSet<Option<String>> {
            self.rows(t).iter().map(|r| key(r.get(col))).collect()
        };
        let (v0, v1) = (values(c[0]), values(c[1]));
        if !v0.is_empty() && v0.is_subset(&ids) && !v1.is_subset(&ids) {
            return Some((c[1].into(), c[0].into()));
        }
        if !v1.is_empty() && v1.is_subset(&ids) && !v0.is_subset(&ids) {
            return Some((c[0].into(), c[1].into()));
        }
        None
    }

    fn referenced_table(&self, col: &str) -> Option<String> {
        let base = col.strip_suffix("_id")?;
        [base.to_string(), format!("{base}s"), format!("{base}es")]
            .into_iter()
            .find(|t| self.d.tables.contains_key(t) && self.has(t, "id"))
    }

    fn matrix_shape(&self, t: &str) -> Option<(String, String, String, String)> {
        let c = self.cols(t);
        let refs: Vec<(String, String)> = c
            .iter()
            .filter_map(|col| self.referenced_table(col).map(|r| (col.to_string(), r)))
            .collect();
        if !c.contains(&"status") || refs.len() != 2 {
            return None;
        }
        let grouped = |x: &str| GROUP_COLUMNS.iter().any(|g| self.has(x, g));
        let ((mut rc, mut rt), (mut cc, mut ct)) = (refs[0].clone(), refs[1].clone());
        if grouped(&ct) && !grouped(&rt) {
            std::mem::swap(&mut rc, &mut cc);
            std::mem::swap(&mut rt, &mut ct);
        }
        Some((rc, rt, cc, ct))
    }

    fn link(&self, f: &dyn Format, rid: &str) -> String {
        match self.index.get(rid) {
            Some((name, page)) => f.link(name, page, rid),
            None => f.code(rid),
        }
    }

    fn relations_for(&self, f: &dyn Format, rid: &str) -> Vec<String> {
        let mut lines = Vec::new();
        for rt in &self.relation_tables {
            let has_note = self.has(rt, "note");
            let note = |r: &Row| {
                if has_note {
                    key(r.get("note")).filter(|n| !n.is_empty())
                } else {
                    None
                }
            };
            for r in self
                .rows(rt)
                .iter()
                .filter(|r| key(r.get("from_id")).as_deref() == Some(rid))
            {
                lines.push(format!(
                    "\u{2192} {}: {}{}",
                    py_str(r.get("relation")),
                    self.link(f, &py_str(r.get("to_id"))),
                    note(r)
                        .map(|n| format!(" \u{2014} {}", f.text(&n)))
                        .unwrap_or_default()
                ));
            }
            for r in self
                .rows(rt)
                .iter()
                .filter(|r| key(r.get("to_id")).as_deref() == Some(rid))
            {
                lines.push(format!(
                    "\u{2190} {}: {}{}",
                    py_str(r.get("relation")),
                    self.link(f, &py_str(r.get("from_id"))),
                    note(r)
                        .map(|n| format!(" \u{2014} {}", f.text(&n)))
                        .unwrap_or_default()
                ));
            }
        }
        if lines.is_empty() {
            vec![]
        } else {
            f.many("Relations", &lines)
        }
    }

    fn cited_by_row(&self, f: &dyn Format, rid: &str) -> Vec<String> {
        let mut out = Vec::new();
        for (t, src, cited, source) in &self.citation_links {
            let mut found: Vec<(usize, String)> = Vec::new();
            for c in self
                .rows(t)
                .iter()
                .filter(|c| key(c.get(cited)).as_deref() == Some(rid))
            {
                for (i, s) in self.rows(src).iter().enumerate() {
                    if key(s.get("id")).is_some() && key(s.get("id")) == key(c.get(source)) {
                        found.push((i, f.url(&py_str(s.get("title")), &py_str(s.get("url")))));
                    }
                }
            }
            found.sort_by_key(|(i, _)| *i);
            if !found.is_empty() {
                out.extend(
                    f.field(
                        "Sources",
                        &found
                            .into_iter()
                            .map(|(_, u)| u)
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                );
            }
        }
        out
    }

    fn cites_of_source(&self, f: &dyn Format, source_id: &str) -> Vec<String> {
        let mut links = Vec::new();
        for (t, _, cited, source) in &self.citation_links {
            for c in self
                .rows(t)
                .iter()
                .filter(|c| key(c.get(source)).as_deref() == Some(source_id))
            {
                links.push(self.link(f, &py_str(c.get(cited))));
            }
        }
        links
    }

    fn table_page(&self, f: &dyn Format, t: &str) -> Vec<String> {
        let c = self.cols(t);
        let label = if c.contains(&"name") {
            Some("name")
        } else if c.contains(&"title") {
            Some("title")
        } else if c.contains(&"id") {
            Some("id")
        } else {
            None
        };
        let is_log = c.contains(&"entry_date") && c.contains(&"title");
        let rows = self.rows(t);
        let mut lines = f.title(&title_case(&words(t)));
        for (i, rec) in rows.iter().enumerate() {
            let rid = if c.contains(&"id") {
                key(rec.get("id"))
            } else {
                None
            };
            let heading = match label {
                Some(l) => py_str(rec.get(l)),
                None => format!("Row {}", i + 1),
            };
            if let Some(r) = &rid {
                lines.extend(f.anchor(r));
            }
            lines.extend(f.heading(&heading));
            for col in &c {
                if *col == "id" || Some(*col) == label {
                    continue;
                }
                if let Some(v) = fmt(rec.get(*col)) {
                    lines.extend(f.field(&capitalize(&words(col)), &f.text(&v)));
                }
            }
            if let Some(r) = &rid {
                lines.extend(self.relations_for(f, r));
                lines.extend(self.cited_by_row(f, r));
                if self.sources_tables.iter().any(|s| s == t) {
                    let mut seen = HashSet::new();
                    let cby: Vec<String> = self
                        .cites_of_source(f, r)
                        .into_iter()
                        .filter(|x| seen.insert(x.clone()))
                        .collect();
                    if !cby.is_empty() {
                        lines.extend(f.field("Cited by", &cby.join(", ")));
                    }
                }
            }
            if is_log && i + 1 < rows.len() {
                lines.extend(f.rule());
            }
            lines.push(String::new());
        }
        lines
    }

    fn matrix_page(&self, f: &dyn Format, t: &str) -> Vec<String> {
        let (rc, rt, cc, ct) = self.matrix[t].clone();
        let c = self.cols(t);
        let group_col = GROUP_COLUMNS.iter().find(|g| self.has(&rt, g)).copied();
        let columns: Vec<String> = self.rows(&ct).iter().map(|r| py_str(r.get("id"))).collect();
        let mut cells: Vec<((String, String), Row)> = Vec::new();
        let mut at: HashMap<(String, String), usize> = HashMap::new();
        for rec in self.rows(t) {
            let k = (py_str(rec.get(&rc)), py_str(rec.get(&cc)));
            match at.get(&k) {
                Some(&i) => cells[i].1 = rec.clone(),
                None => {
                    at.insert(k.clone(), cells.len());
                    cells.push((k, rec.clone()));
                }
            }
        }
        let mut groups: Vec<(Option<String>, Vec<String>)> = Vec::new();
        for rec in self.rows(&rt) {
            let g = group_col.and_then(|g| key(rec.get(g)));
            let id = py_str(rec.get("id"));
            match groups.iter_mut().find(|(x, _)| *x == g) {
                Some((_, ids)) => ids.push(id),
                None => groups.push((g, vec![id])),
            }
        }
        let label = |rid: &str| {
            self.index
                .get(rid)
                .map(|(n, _)| n.clone())
                .unwrap_or_else(|| rid.to_string())
        };
        let mut lines = f.title(&title_case(&words(t)));
        let legend = STATUS_MARKS
            .iter()
            .map(|(s, m)| format!("{m} {s}"))
            .collect::<Vec<_>>()
            .join(" \u{00b7} ");
        lines.push(f.text(&format!(
            "Rows are {}, columns {}. {legend}. A pair nobody has checked shows {}.",
            words(&rt),
            words(&ct),
            mark("unknown")
        )));
        lines.push(String::new());
        let mut header = vec![title_case(&words(&rt))];
        header.extend(columns.iter().map(|cid| self.link(f, cid)));
        for (g, ids) in &groups {
            if let Some(g) = g {
                lines.extend(f.heading(&capitalize(&g.replace('-', " "))));
            }
            let body: Vec<Vec<String>> = ids
                .iter()
                .map(|rid| {
                    let mut row = vec![self.link(f, rid)];
                    for cid in &columns {
                        row.push(match at.get(&(rid.clone(), cid.clone())) {
                            None => mark("unknown"),
                            Some(&i) => f.link(
                                &mark(&py_str(cells[i].1.get("status"))),
                                t,
                                &format!("{rid}--{cid}"),
                            ),
                        });
                    }
                    row
                })
                .collect();
            lines.extend(f.table(&header, &body));
        }
        if !cells.is_empty() {
            lines.extend(f.heading("Details"));
            for ((rid, cid), rec) in &cells {
                let mut parts = vec![py_str(rec.get("status"))];
                for k in &c {
                    if *k == rc || *k == cc || *k == "status" {
                        continue;
                    }
                    if let Some(v) = fmt(rec.get(*k)) {
                        parts.push(format!("{}: {v}", words(k)));
                    }
                }
                lines.extend(f.anchor(&format!("{rid}--{cid}")));
                lines.extend(f.field(
                    &format!("{} \u{00d7} {}", label(rid), label(cid)),
                    &f.text(&parts.join(" \u{2014} ")),
                ));
            }
        }
        lines
    }
}

// ---- presentation, declared as data -------------------------------------------------------------
// With nothing declared, every page renders as above. A repository may declare, in tables.toml, how
// its index and any page read: a [site] (the index's title, introduction, page order and counts, and
// relations in words) and [pages.<table>] (introduction, grouping, order, what each row shows, or a
// list layout). Presentation is data like everything else, so no repository needs its own renderer.

/// The index, and the words for relations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SitePresentation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Lines under the title, written one after another.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intro: Vec<String>,
    /// The pages on the index, in this order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub index: Vec<IndexEntry>,
    /// A relation kind -> [the words from its source, the words from its target].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub relation_words: BTreeMap<String, [String; 2]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct IndexEntry {
    pub page: String,
    /// Count "N open of M": the rows whose field is not one of open_unless.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<OpenCount>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OpenCount {
    pub field: String,
    pub open_unless: Vec<String>,
}

/// How one page reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Page {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intro: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Group>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub skip_empty_groups: bool,
    /// Fields to order the rows by; rows that tie keep their order in the file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order_by: Vec<String>,
    /// What each row shows, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub show: Vec<Show>,
    /// A list instead of sections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<ListLayout>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Group {
    pub value: String,
    pub heading: String,
}

/// One thing a row shows: a field (or several, joined), as a plain paragraph or under a label; or the
/// row's relations; or its sources.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Show {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub relations: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub sources: bool,
}

/// A list page: each row one line - a link, then fields after it, then who cites it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ListLayout {
    /// [the field shown, the field linked to]
    pub link: [String; 2],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub cited_by: bool,
}

fn order(a: Option<&Value>, b: Option<&Value>) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    match (a.filter(|v| !v.is_null()), b.filter(|v| !v.is_null())) {
        (None, None) => Equal,
        (None, Some(_)) => Greater,
        (Some(_), None) => Less,
        (Some(Value::Number(x)), Some(Value::Number(y))) => {
            x.as_f64().partial_cmp(&y.as_f64()).unwrap_or(Equal)
        }
        (Some(x), Some(y)) => py_str(Some(x)).cmp(&py_str(Some(y))),
    }
}

impl<'a> Site<'a> {
    fn declared_page(&self, f: &dyn Format, t: &str, p: &Page) -> Vec<String> {
        let mut lines = f.title(p.title.as_deref().unwrap_or(&title_case(&words(t))));
        if let Some(i) = &p.intro {
            lines.push(f.text(i));
            lines.push(String::new());
        }
        let mut rows: Vec<&Row> = self.rows(t).iter().collect();
        if !p.order_by.is_empty() {
            rows.sort_by(|a, b| {
                p.order_by
                    .iter()
                    .map(|k| order(a.get(k), b.get(k)))
                    .find(|o| o.is_ne())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        if let Some(list) = &p.list {
            lines.extend(rows.iter().map(|r| self.list_line(f, r, list)));
            return lines;
        }
        match &p.group_by {
            Some(g) => {
                for grp in &p.groups {
                    let members: Vec<&Row> = rows
                        .iter()
                        .filter(|r| key(r.get(g)).as_deref() == Some(grp.value.as_str()))
                        .copied()
                        .collect();
                    if members.is_empty() && p.skip_empty_groups {
                        continue;
                    }
                    lines.extend(f.heading_at(&grp.heading, 2));
                    for r in members {
                        lines.extend(self.declared_row(f, t, p, r, true));
                    }
                }
            }
            None => {
                for r in rows {
                    lines.extend(self.declared_row(f, t, p, r, false));
                }
            }
        }
        lines
    }

    fn declared_row(
        &self,
        f: &dyn Format,
        t: &str,
        p: &Page,
        rec: &Row,
        grouped: bool,
    ) -> Vec<String> {
        let c = self.cols(t);
        let label = ["name", "title", "id"].into_iter().find(|l| c.contains(l));
        let rid = if c.contains(&"id") {
            key(rec.get("id"))
        } else {
            None
        };
        let mut lines = Vec::new();
        if let Some(r) = &rid {
            lines.extend(f.anchor(r));
        }
        let heading = label.map(|l| py_str(rec.get(l))).unwrap_or_default();
        lines.extend(if grouped {
            f.heading_at(&heading, 3)
        } else {
            f.heading(&heading)
        });
        for s in &p.show {
            if s.relations {
                if let Some(r) = &rid {
                    lines.extend(self.relations_in_words(f, r));
                }
                continue;
            }
            if s.sources {
                if let Some(r) = &rid {
                    lines.extend(self.cited_by_row(f, r));
                }
                continue;
            }
            let parts: Vec<String> = s
                .field
                .iter()
                .chain(s.fields.iter())
                .filter_map(|k| fmt(rec.get(k.as_str())))
                .collect();
            if parts.is_empty() {
                continue;
            }
            let v = f.text(&parts.join(", "));
            match &s.label {
                Some(l) => lines.extend(f.field(l, &v)),
                None => lines.extend(f.para(&v)),
            }
        }
        lines
    }

    /// Relations under their words ("Serves", "Served by"), one term per kind of word; without
    /// declared words, the generic "Relations" list.
    fn relations_in_words(&self, f: &dyn Format, rid: &str) -> Vec<String> {
        let Some(words) = self
            .presentation
            .map(|s| &s.relation_words)
            .filter(|w| !w.is_empty())
        else {
            return self.relations_for(f, rid);
        };
        let mut parts: Vec<(String, Vec<String>)> = Vec::new();
        let mut add =
            |label: String, item: String| match parts.iter_mut().find(|(l, _)| *l == label) {
                Some((_, items)) => {
                    if !items.contains(&item) {
                        items.push(item)
                    }
                }
                None => parts.push((label, vec![item])),
            };
        for rt in &self.relation_tables {
            for (end, other, side) in [("from_id", "to_id", 0), ("to_id", "from_id", 1)] {
                for r in self
                    .rows(rt)
                    .iter()
                    .filter(|r| key(r.get(end)).as_deref() == Some(rid))
                {
                    let kind = py_str(r.get("relation"));
                    let label = words.get(&kind).map(|w| w[side].clone()).unwrap_or(kind);
                    let note = key(r.get("note"))
                        .filter(|n| !n.is_empty())
                        .map(|n| format!(" ({n})"))
                        .unwrap_or_default();
                    add(
                        label,
                        format!("{}{note}", self.link(f, &py_str(r.get(other)))),
                    );
                }
            }
        }
        parts
            .into_iter()
            .flat_map(|(l, items)| f.field(&l, &items.join(", ")))
            .collect()
    }

    fn list_line(&self, f: &dyn Format, r: &Row, list: &ListLayout) -> String {
        let mut s =
            f.list_item(&f.url(&py_str(r.get(&list.link[0])), &py_str(r.get(&list.link[1]))));
        let after: Vec<String> = list.after.iter().filter_map(|k| fmt(r.get(k))).collect();
        if !after.is_empty() {
            s.push_str(&format!(", {}", after.join(", ")));
        }
        if list.cited_by {
            if let Some(id) = key(r.get("id")) {
                let users = self.cites_of_source(f, &id);
                if !users.is_empty() {
                    s.push_str(&format!(". Cited by: {}", users.join(", ")));
                }
            }
        }
        s
    }
}

impl<'a> Site<'a> {
    /// The index's lines: the declared pages in their order (with "N open of M" counts where
    /// declared), or every page with its row count.
    fn index_lines(&self, f: &dyn Format, counts: &[(String, usize)]) -> Vec<String> {
        let declared = self
            .presentation
            .map(|p| p.index.as_slice())
            .unwrap_or_default();
        if declared.is_empty() {
            return counts
                .iter()
                .map(|(t, n)| f.index_item(&title_case(&words(t)), t, &n.to_string()))
                .collect();
        }
        declared
            .iter()
            .filter(|e| self.page_tables.contains(&e.page))
            .map(|e| {
                let rows = self.rows(&e.page);
                let n = match &e.count {
                    Some(c) => format!(
                        "{} open of {}",
                        rows.iter()
                            .filter(|r| !c
                                .open_unless
                                .iter()
                                .any(|v| key(r.get(&c.field)).as_deref() == Some(v.as_str())))
                            .count(),
                        rows.len()
                    ),
                    None => rows.len().to_string(),
                };
                f.index_item(&title_case(&words(&e.page)), &e.page, &n)
            })
            .collect()
    }
}

fn write(f: &dyn Format, docs: &Path, name: &str, lines: Vec<String>) -> Result<()> {
    std::fs::create_dir_all(docs)?;
    let mut all = vec![f.notice().to_string(), String::new()];
    all.extend(lines);
    let text = all.join("\n");
    std::fs::write(
        docs.join(format!("{name}.{}", f.ext())),
        text.trim_end_matches('\n').to_string() + "\n",
    )?;
    Ok(())
}

/// Renders the declared tables in `data` into documents in `docs`, in each of `formats` (md, rst).
pub fn render(data: &Path, d: &Declaration, docs: &Path, formats: &[String]) -> Result<String> {
    let site = Site::new(d, data)?;
    let mut fs: Vec<Box<dyn Format>> = Vec::new();
    for name in formats {
        match name.as_str() {
            "md" => fs.push(Box::new(Markdown)),
            "rst" => fs.push(Box::new(Rst(patterns()?))),
            other => bail!("unknown format {other}; known: md, rst"),
        }
    }
    if fs.is_empty() {
        fs.push(Box::new(Markdown));
    }
    let counts: Vec<(String, usize)> = site
        .page_tables
        .iter()
        .map(|t| (t.clone(), site.rows(t).len()))
        .collect();
    let about = site
        .rows
        .get("about")
        .filter(|_| site.has("about", "title") && site.has("about", "summary"))
        .and_then(|r| r.first());
    for f in &fs {
        let f = f.as_ref();
        for t in &site.page_tables {
            let lines = if let Some(p) = d.pages.get(t) {
                site.declared_page(f, t, p)
            } else if site.matrix.contains_key(t) {
                site.matrix_page(f, t)
            } else {
                site.table_page(f, t)
            };
            write(f, docs, t, lines)?;
        }
        let declared_title = site.presentation.and_then(|p| p.title.as_ref());
        let mut out = match about {
            _ if declared_title.is_some() => {
                let mut h = f.title(declared_title.map(String::as_str).unwrap_or_default());
                let intro = site
                    .presentation
                    .map(|p| p.intro.as_slice())
                    .unwrap_or_default();
                if !intro.is_empty() {
                    h.extend(intro.iter().map(|l| f.text(l)));
                    h.push(String::new());
                }
                h
            }
            Some(a) => {
                let mut h = f.title(&py_str(a.get("title")));
                h.push(f.text(&py_str(a.get("summary"))));
                h.push(String::new());
                h
            }
            None => f.title("Index"),
        };
        out.extend(site.index_lines(f, &counts));
        out.push(String::new());
        write(f, docs, "index", out)?;
    }
    Ok(format!(
        "rendered {} as {}",
        counts
            .iter()
            .map(|(k, v)| format!("{v} {k}"))
            .collect::<Vec<_>>()
            .join(", "),
        fs.iter().map(|f| f.ext()).collect::<Vec<_>>().join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_string_habits_are_kept() {
        assert_eq!(title_case("feature support"), "Feature Support");
        assert_eq!(title_case("a1b c"), "A1B C");
        assert_eq!(capitalize("best WHEN"), "Best when");
        assert_eq!(rst_id("Hello, World_2"), "hello-world-2");
        let p = patterns().unwrap();
        assert_eq!(rst_escape(&p, "a_ b* - x"), "a\\_ b\\* - x");
        assert_eq!(rst_escape(&p, "- list"), "\\- list");
        assert_eq!(
            md_inline_to_rst(&p, "see `x`, [the site](https://a.b) and **bold**"),
            "see ``x``, `the site <https://a.b>`__ and **bold**"
        );
        assert_eq!(
            md_inline_to_rst(&p, "an *em*x"),
            "an \\*em\\*x",
            "emphasis followed by a letter is not emphasis"
        );
        assert_eq!(
            md_inline_to_rst(&p, "`x`y"),
            "``x``\\ y",
            "markup followed by a letter gets the empty separator"
        );
        assert_eq!(width("Ab日本"), 6);
    }

    #[test]
    fn presentation_declared_as_data() {
        let decl: crate::Declaration = toml::from_str(
            r#"
[tables.items]
key = ["id"]
fields = [{ name = "id", type = "string" }, { name = "name", type = "string" }, { name = "kind", type = "string" }, { name = "body", type = "string" }, { name = "who", type = "string" }, { name = "year", type = "integer" }, { name = "status", type = "string" }]
[tables.links]
fields = [{ name = "from_id", type = "string" }, { name = "to_id", type = "string" }, { name = "relation", type = "string" }, { name = "note", type = "string" }]
[site]
title = "A canon"
intro = ["Line one.", "Line two."]
index = [{ page = "items", count = { field = "status", open_unless = ["done"] } }]
[site.relation_words]
serves = ["Serves", "Served by"]
[pages.items]
intro = "Things."
group_by = "kind"
groups = [{ value = "a", heading = "Kind A" }, { value = "b", heading = "Kind B" }, { value = "c", heading = "Kind C" }]
skip_empty_groups = true
show = [{ field = "body" }, { fields = ["who", "year"], label = "Origin" }, { relations = true }]
"#,
        )
        .unwrap();
        let dir = std::env::temp_dir().join(format!("tad-pres-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::write(dir.join("data/items.json"), "{\"id\":\"x\",\"name\":\"X\",\"kind\":\"b\",\"body\":\"Body of x.\",\"who\":\"Ada\",\"year\":1843,\"status\":\"open\"}\n{\"id\":\"y\",\"name\":\"Y\",\"kind\":\"a\",\"body\":\"Body of y.\",\"who\":null,\"year\":null,\"status\":\"done\"}\n").unwrap();
        std::fs::write(
            dir.join("data/links.json"),
            "{\"from_id\":\"x\",\"to_id\":\"y\",\"relation\":\"serves\",\"note\":\"mostly\"}\n",
        )
        .unwrap();
        render(&dir.join("data"), &decl, &dir.join("docs"), &["md".into()]).unwrap();
        let page = std::fs::read_to_string(dir.join("docs/items.md")).unwrap();
        let want = "# Items\n\nThings.\n\n## Kind A\n\n<a id=\"y\"></a>\n### Y\n\nBody of y.\n\nServed by\n: [X](items.md#x) (mostly)\n\n## Kind B\n\n<a id=\"x\"></a>\n### X\n\nBody of x.\n\nOrigin\n: Ada, 1843\n\nServes\n: [Y](items.md#y) (mostly)\n";
        assert!(page.ends_with(want), "{page}");
        let index = std::fs::read_to_string(dir.join("docs/index.md")).unwrap();
        assert!(
            index.ends_with(
                "# A canon\n\nLine one.\nLine two.\n\n- [Items](items.md): 1 open of 2\n"
            ),
            "{index}"
        );
    }
}
