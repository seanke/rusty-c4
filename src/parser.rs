//! A forgiving parser for the parts of the LikeC4 DSL that matter for drawing:
//! `specification` kinds and styles, `model` elements (both `name = kind` and `kind name`
//! forms, nested bodies, `extend`, `group`), and relationships (`->`, `-[kind]->`, `.kind`,
//! `<->`, implicit `this` sources). Views, deployment and anything unknown is skipped.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::model::*;

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Hash(String),
    LBrace,
    RBrace,
    Eq,
    Dot,
    Arrow,
    BiArrow,
    ArrowOpen,
    ArrowClose,
    Semi,
    Comma,
    Other,
}

#[derive(Clone, Debug)]
struct Token {
    tok: Tok,
    line: usize,
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn lex(src: &str) -> Vec<Token> {
    let s: Vec<char> = src.chars().collect();
    let at = |i: usize| s.get(i).copied().unwrap_or('\0');
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1;
    while i < s.len() {
        let c = s[i];
        let start_line = line;
        let mut push = |tok: Tok| out.push(Token { tok, line: start_line });
        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            '/' if at(i + 1) == '/' => {
                while i < s.len() && s[i] != '\n' {
                    i += 1;
                }
            }
            '/' if at(i + 1) == '*' => {
                i += 2;
                while i < s.len() && !(s[i] == '*' && at(i + 1) == '/') {
                    if s[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
                i += 2;
            }
            '\'' | '"' => {
                let triple = at(i + 1) == c && at(i + 2) == c;
                i += if triple { 3 } else { 1 };
                let mut text = String::new();
                while i < s.len() {
                    if triple && s[i] == c && at(i + 1) == c && at(i + 2) == c {
                        i += 3;
                        break;
                    }
                    if !triple && s[i] == c {
                        i += 1;
                        break;
                    }
                    if s[i] == '\\' && i + 1 < s.len() {
                        i += 1;
                        text.push(match s[i] {
                            'n' => '\n',
                            't' => '\t',
                            other => other,
                        });
                    } else {
                        if s[i] == '\n' {
                            line += 1;
                        }
                        text.push(s[i]);
                    }
                    i += 1;
                }
                push(Tok::Str(text));
            }
            '#' => {
                i += 1;
                let st = i;
                while i < s.len() && (is_ident_char(s[i]) || s[i] == '-') {
                    i += 1;
                }
                push(Tok::Hash(s[st..i].iter().collect()));
            }
            '-' if at(i + 1) == '>' => {
                i += 2;
                push(Tok::Arrow);
            }
            '-' if at(i + 1) == '[' => {
                i += 2;
                push(Tok::ArrowOpen);
            }
            ']' if at(i + 1) == '-' && at(i + 2) == '>' => {
                i += 3;
                push(Tok::ArrowClose);
            }
            '<' if at(i + 1) == '-' && at(i + 2) == '>' => {
                i += 3;
                push(Tok::BiArrow);
            }
            '{' => {
                i += 1;
                push(Tok::LBrace);
            }
            '}' => {
                i += 1;
                push(Tok::RBrace);
            }
            '=' => {
                i += 1;
                push(Tok::Eq);
            }
            ';' => {
                i += 1;
                push(Tok::Semi);
            }
            ',' => {
                i += 1;
                push(Tok::Comma);
            }
            '.' if is_ident_char(at(i + 1)) => {
                i += 1;
                push(Tok::Dot);
            }
            c if is_ident_char(c) => {
                let st = i;
                while i < s.len() {
                    let ch = s[i];
                    let next = at(i + 1);
                    if is_ident_char(ch)
                        || (ch == '-' && next != '>' && next != '[' && is_ident_char(next))
                        || (ch == '.' && is_ident_char(next))
                    {
                        i += 1;
                    } else {
                        break;
                    }
                }
                push(Tok::Ident(s[st..i].iter().collect()));
            }
            _ => {
                i += 1;
                push(Tok::Other);
            }
        }
    }
    out
}

#[derive(Default)]
struct RawElement {
    kind: String,
    title: Option<String>,
    description: Option<String>,
    technology: Option<String>,
    style: Style,
    declared: bool,
}

struct RawRelation {
    scope: String,
    source: Option<String>,
    target: String,
    title: String,
    technology: String,
    location: String,
}

#[derive(Default)]
struct Builder {
    kinds: HashMap<String, Style>,
    colors: HashMap<String, [f32; 3]>,
    elements: HashMap<String, RawElement>,
    order: Vec<String>,
    relations: Vec<RawRelation>,
    warnings: Vec<String>,
}

impl Builder {
    fn element(&mut self, fqn: &str) -> &mut RawElement {
        if !self.elements.contains_key(fqn) {
            self.order.push(fqn.to_string());
        }
        self.elements.entry(fqn.to_string()).or_default()
    }
}

const PROPERTY_KEYWORDS: &[&str] = &[
    "title",
    "description",
    "technology",
    "summary",
    "link",
    "icon",
    "metadata",
    "style",
    "notation",
    "notes",
    "navigateTo",
    "tags",
];

struct Parser<'a> {
    toks: Vec<Token>,
    pos: usize,
    file: String,
    b: &'a mut Builder,
}

impl Parser<'_> {
    fn peek(&self, n: usize) -> Option<&Tok> {
        self.toks.get(self.pos + n).map(|t| &t.tok)
    }

    fn line(&self) -> usize {
        self.toks.get(self.pos).map(|t| t.line).unwrap_or(0)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).map(|t| t.tok.clone());
        self.pos += 1;
        t
    }

    fn ident(&mut self) -> Option<String> {
        match self.peek(0) {
            Some(Tok::Ident(s)) => {
                let s = s.clone();
                self.pos += 1;
                Some(s)
            }
            _ => None,
        }
    }

    fn string(&mut self) -> Option<String> {
        match self.peek(0) {
            Some(Tok::Str(s)) => {
                let s = s.clone();
                self.pos += 1;
                Some(s)
            }
            _ => None,
        }
    }

    fn strings(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        while let Some(s) = self.string() {
            out.push(s);
        }
        out
    }

    fn skip_tags(&mut self) {
        while matches!(self.peek(0), Some(Tok::Hash(_)) | Some(Tok::Comma)) {
            self.pos += 1;
        }
    }

    /// Call after consuming a `{`: skips to just past the matching `}`.
    fn skip_rest_of_block(&mut self) {
        let mut depth = 1;
        while let Some(t) = self.next() {
            match t {
                Tok::LBrace => depth += 1,
                Tok::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    fn eat_block_open(&mut self) -> bool {
        if self.peek(0) == Some(&Tok::LBrace) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Skips the remaining tokens on `line`, including any block that opens on it.
    fn skip_line(&mut self, line: usize) {
        while self.pos < self.toks.len() && self.toks[self.pos].line == line {
            match self.peek(0) {
                Some(Tok::RBrace) => return,
                Some(Tok::LBrace) => {
                    self.pos += 1;
                    self.skip_rest_of_block();
                    return;
                }
                _ => self.pos += 1,
            }
        }
    }

    fn parse_file(&mut self) {
        while self.pos < self.toks.len() {
            match self.next() {
                Some(Tok::Ident(k)) if k == "specification" => {
                    if self.eat_block_open() {
                        self.parse_spec();
                    }
                }
                Some(Tok::Ident(k)) if k == "model" => {
                    if self.eat_block_open() {
                        self.parse_model_body("");
                    }
                }
                Some(Tok::LBrace) => self.skip_rest_of_block(),
                _ => {}
            }
        }
    }

    fn parse_spec(&mut self) {
        while let Some(t) = self.next() {
            match t {
                Tok::RBrace => return,
                Tok::LBrace => self.skip_rest_of_block(),
                Tok::Ident(k) if k == "element" => {
                    let Some(name) = self.ident() else { continue };
                    let mut style = Style::default();
                    if self.eat_block_open() {
                        self.parse_kind_body(&mut style);
                    }
                    self.b.kinds.insert(name, style);
                }
                Tok::Ident(k) if k == "color" => {
                    let Some(name) = self.ident() else { continue };
                    if let Some(Tok::Hash(h)) = self.peek(0).cloned() {
                        self.pos += 1;
                        if let Some(rgb) = parse_hex(&h) {
                            self.b.colors.insert(name, rgb);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn parse_kind_body(&mut self, style: &mut Style) {
        while let Some(t) = self.next() {
            match t {
                Tok::RBrace => return,
                Tok::Ident(k) if k == "style" && self.peek(0) == Some(&Tok::LBrace) => {
                    self.pos += 1;
                    self.parse_style(style);
                }
                Tok::LBrace => self.skip_rest_of_block(),
                _ => {}
            }
        }
    }

    fn parse_style(&mut self, style: &mut Style) {
        while let Some(t) = self.next() {
            match t {
                Tok::RBrace => return,
                Tok::LBrace => self.skip_rest_of_block(),
                Tok::Ident(k) if k == "shape" => {
                    if let Some(v) = self.ident() {
                        style.shape = Shape::parse(&v).or(style.shape);
                    }
                }
                Tok::Ident(k) if k == "color" => match self.peek(0).cloned() {
                    Some(Tok::Ident(v)) => {
                        self.pos += 1;
                        style.color = Some(v);
                    }
                    Some(Tok::Hash(v)) => {
                        self.pos += 1;
                        style.color = Some(format!("#{v}"));
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    fn parse_model_body(&mut self, scope: &str) {
        loop {
            let line = self.line();
            let Some(t) = self.peek(0).cloned() else { return };
            match t {
                Tok::RBrace => {
                    self.pos += 1;
                    return;
                }
                Tok::Arrow | Tok::ArrowOpen | Tok::BiArrow => self.parse_relation(scope, None),
                Tok::Dot if matches!(self.peek(1), Some(Tok::Ident(_))) => {
                    self.parse_relation(scope, None)
                }
                Tok::LBrace => {
                    self.pos += 1;
                    self.skip_rest_of_block();
                }
                Tok::Ident(name) => {
                    let next = self.peek(1).cloned();
                    match next {
                        Some(Tok::Arrow | Tok::ArrowOpen | Tok::BiArrow) => {
                            self.pos += 1;
                            self.parse_relation(scope, Some(name));
                        }
                        Some(Tok::Dot) if matches!(self.peek(2), Some(Tok::Ident(_))) => {
                            self.pos += 1;
                            self.parse_relation(scope, Some(name));
                        }
                        Some(Tok::Eq) => {
                            self.pos += 2;
                            match self.ident() {
                                Some(kind) => self.parse_element(scope, &name, &kind),
                                None => self.skip_line(line),
                            }
                        }
                        _ if name == "extend" => {
                            self.pos += 1;
                            let Some(target) = self.ident() else {
                                self.skip_line(line);
                                continue;
                            };
                            self.b.element(&target);
                            if self.eat_block_open() {
                                self.parse_model_body(&target);
                            }
                        }
                        _ if name == "group" => {
                            self.pos += 1;
                            self.strings();
                            self.skip_tags();
                            if self.eat_block_open() {
                                self.parse_model_body(scope);
                            }
                        }
                        _ if PROPERTY_KEYWORDS.contains(&name.as_str()) => {
                            self.pos += 1;
                            self.parse_property(scope, &name, line);
                        }
                        Some(Tok::Ident(el_name)) => {
                            self.pos += 2;
                            self.parse_element(scope, &el_name, &name);
                        }
                        _ => {
                            self.pos += 1;
                            self.skip_line(line);
                        }
                    }
                }
                _ => self.pos += 1,
            }
        }
    }

    fn parse_element(&mut self, scope: &str, name: &str, kind: &str) {
        let fqn = if scope.is_empty() {
            name.to_string()
        } else {
            format!("{scope}.{name}")
        };
        let strs = self.strings();
        let e = self.b.element(&fqn);
        e.kind = kind.to_string();
        e.declared = true;
        let mut it = strs.into_iter();
        if let Some(t) = it.next() {
            e.title = Some(t);
        }
        if let Some(d) = it.next() {
            e.description = Some(d);
        }
        if let Some(t) = it.next() {
            e.technology = Some(t);
        }
        self.skip_tags();
        if self.eat_block_open() {
            self.parse_model_body(&fqn);
        }
    }

    fn parse_property(&mut self, scope: &str, key: &str, line: usize) {
        if scope.is_empty() {
            self.skip_line(line);
            return;
        }
        if key == "style" && self.eat_block_open() {
            let mut style = self.b.element(scope).style.clone();
            self.parse_style(&mut style);
            self.b.element(scope).style = style;
            return;
        }
        if let Some(Tok::Str(v)) = self.peek(0).cloned() {
            let e = self.b.element(scope);
            match key {
                "title" => e.title = Some(v),
                "description" => e.description = Some(v),
                "summary" => {
                    e.description.get_or_insert(v);
                }
                "technology" => e.technology = Some(v),
                _ => {}
            }
            if matches!(key, "title" | "description" | "summary" | "technology") {
                self.pos += 1;
                return;
            }
        }
        if self.eat_block_open() {
            self.skip_rest_of_block();
        } else {
            self.skip_line(line);
        }
    }

    fn parse_relation(&mut self, scope: &str, source: Option<String>) {
        let line = self.line();
        let bidir = match self.next() {
            Some(Tok::BiArrow) => true,
            Some(Tok::ArrowOpen) => {
                self.ident();
                if self.peek(0) == Some(&Tok::ArrowClose) {
                    self.pos += 1;
                }
                false
            }
            Some(Tok::Dot) => {
                self.ident();
                false
            }
            _ => false,
        };
        let Some(target) = self.ident() else {
            self.skip_line(line);
            return;
        };
        let strs = self.strings();
        let mut title = strs.first().cloned().unwrap_or_default();
        let mut technology = strs.last().filter(|_| strs.len() > 1).cloned().unwrap_or_default();
        self.skip_tags();
        if self.eat_block_open() {
            while let Some(t) = self.next() {
                match t {
                    Tok::RBrace => break,
                    Tok::LBrace => self.skip_rest_of_block(),
                    Tok::Ident(k) if k == "title" => {
                        if let Some(s) = self.string() {
                            title = s;
                        }
                    }
                    Tok::Ident(k) if k == "technology" => {
                        if let Some(s) = self.string() {
                            technology = s;
                        }
                    }
                    _ => {}
                }
            }
        }
        let location = format!("{}:{}", self.file, line);
        let rel = RawRelation {
            scope: scope.to_string(),
            source: source.clone(),
            target: target.clone(),
            title: title.clone(),
            technology: technology.clone(),
            location: location.clone(),
        };
        self.b.relations.push(rel);
        if bidir {
            // `a <-> b` is drawn as two relations; for an implicit source, `this` is the scope.
            self.b.relations.push(RawRelation {
                scope: scope.to_string(),
                source: Some(target),
                target: source.unwrap_or_else(|| "this".into()),
                title,
                technology,
                location,
            });
        }
    }
}

fn tidy(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parent_fqn(fqn: &str) -> &str {
    fqn.rsplit_once('.').map(|(p, _)| p).unwrap_or("")
}

fn finish(b: Builder) -> (Model, Vec<String>) {
    let mut warnings = b.warnings;
    let declared: HashSet<&str> = b
        .order
        .iter()
        .filter(|f| b.elements[f.as_str()].declared)
        .map(|f| f.as_str())
        .collect();

    let mut model = Model::default();
    for fqn in &b.order {
        let raw = &b.elements[fqn];
        if !raw.declared {
            warnings.push(format!("`extend {fqn}`: no element named `{fqn}`"));
            continue;
        }
        let kind_style = b.kinds.get(&raw.kind).cloned().unwrap_or_default();
        let style = raw.style.merged_over(&kind_style);
        let color_name = style.color.unwrap_or_else(|| "primary".into());
        let color = b
            .colors
            .get(&color_name)
            .copied()
            .or_else(|| named_color(&color_name))
            .or_else(|| parse_hex(&color_name))
            .unwrap_or_else(|| named_color("primary").unwrap());
        let name = fqn.rsplit('.').next().unwrap_or(fqn);
        let id = model.elements.len();
        model.by_fqn.insert(fqn.clone(), id);
        model.elements.push(Element {
            fqn: fqn.clone(),
            kind: raw.kind.clone(),
            title: tidy(raw.title.as_deref().unwrap_or(name)),
            description: tidy(raw.description.as_deref().unwrap_or("")),
            technology: tidy(raw.technology.as_deref().unwrap_or("")),
            parent: None,
            children: Vec::new(),
            shape: style.shape.unwrap_or_default(),
            color,
        });
    }

    for id in 0..model.elements.len() {
        let mut p = parent_fqn(&model.elements[id].fqn).to_string();
        let parent = loop {
            if p.is_empty() {
                break None;
            }
            if let Some(&pid) = model.by_fqn.get(&p) {
                break Some(pid);
            }
            p = parent_fqn(&p).to_string();
        };
        model.elements[id].parent = parent;
        match parent {
            Some(pid) => model.elements[pid].children.push(id),
            None => model.roots.push(id),
        }
    }

    // Unique short names, for references that are neither scoped nor fully qualified.
    let mut by_short: HashMap<&str, Vec<&str>> = HashMap::new();
    for f in &declared {
        by_short.entry(f.rsplit('.').next().unwrap()).or_default().push(f);
    }

    let resolve = |scope: &str, text: &str| -> Option<Id> {
        if text == "this" || text == "it" {
            return model.by_fqn.get(scope).copied();
        }
        let (first, rest) = match text.split_once('.') {
            Some((a, b)) => (a, Some(b)),
            None => (text, None),
        };
        let mut s = scope;
        let mut base = loop {
            let cand = if s.is_empty() {
                first.to_string()
            } else {
                format!("{s}.{first}")
            };
            if declared.contains(cand.as_str()) {
                break Some(cand);
            }
            if s.is_empty() {
                break None;
            }
            s = parent_fqn(s);
        };
        if base.is_none() {
            if let Some([only]) = by_short.get(first).map(|v| v.as_slice()) {
                base = Some(only.to_string());
            }
        }
        let full = match rest {
            Some(r) => format!("{}.{r}", base?),
            None => base?,
        };
        model.by_fqn.get(&full).copied()
    };

    let mut relations = Vec::new();
    for r in &b.relations {
        let src_text = r.source.as_deref().unwrap_or("this");
        let source = resolve(&r.scope, src_text);
        let target = resolve(&r.scope, &r.target);
        match (source, target) {
            (Some(source), Some(target)) => relations.push(Relation {
                source,
                target,
                title: tidy(&r.title),
                technology: tidy(&r.technology),
            }),
            (None, _) => warnings.push(format!("{}: can't resolve `{src_text}`", r.location)),
            (_, None) => warnings.push(format!("{}: can't resolve `{}`", r.location, r.target)),
        }
    }
    model.relations = relations;
    if !model.roots.is_empty() {
        let id = model.elements.len();
        model.elements.push(Element {
            fqn: String::new(),
            kind: String::new(),
            title: "Landscape".into(),
            description: String::new(),
            technology: String::new(),
            parent: None,
            children: model.roots.clone(),
            shape: Shape::Rectangle,
            color: named_color("slate").unwrap(),
        });
        model.root = Some(id);
    }
    (model, warnings)
}

pub fn parse_sources(sources: &[(String, String)]) -> (Model, Vec<String>) {
    let mut b = Builder::default();
    for (file, src) in sources {
        let mut p = Parser {
            toks: lex(src),
            pos: 0,
            file: file.clone(),
            b: &mut b,
        };
        p.parse_file();
    }
    finish(b)
}

/// All `.c4` / `.likec4` files under `path` (or `path` itself if it is a file).
pub fn source_files(path: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for entry in rd.flatten() {
            let p = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if p.is_dir() {
                if !name.starts_with('.') && name != "node_modules" && name != "target" {
                    walk(&p, out);
                }
            } else if name.ends_with(".c4") || name.ends_with(".likec4") {
                out.push(p);
            }
        }
    }
    if path.is_file() {
        return vec![path.to_path_buf()];
    }
    let mut out = Vec::new();
    walk(path, &mut out);
    out.sort();
    out
}

pub fn load(path: &Path) -> Result<(Model, Vec<String>), String> {
    let files = source_files(path);
    if files.is_empty() {
        return Err(format!("no .c4 or .likec4 files found at {}", path.display()));
    }
    let mut sources = Vec::new();
    for f in files {
        let src = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let name = f.file_name().unwrap_or_default().to_string_lossy().to_string();
        sources.push((name, src));
    }
    Ok(parse_sources(&sources))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_model_and_relations() {
        let src = r#"
            specification {
              element actor { style { shape person; color amber } }
              element system
              element container
            }
            model {
              customer = actor 'Customer' 'A happy customer'
              cloud = system 'Cloud' {
                ui = container 'Web UI' {
                  technology 'React'
                  -> backend 'calls API'
                }
                container backend 'Backend'
                group 'storage' {
                  db = container 'Database' { style { shape storage } }
                }
                backend -[async]-> db 'reads'
              }
              customer -> cloud.ui 'opens'
              customer .uses cloud
              extend cloud.backend {
                worker = container 'Worker'
                this <-> db
              }
            }
            views { view index { include * } }
        "#;
        let (m, warnings) = parse_sources(&[("t.c4".into(), src.into())]);
        assert!(warnings.is_empty(), "{warnings:?}");
        let get = |f: &str| &m.elements[m.by_fqn[f]];
        assert_eq!(get("customer").shape, Shape::Person);
        assert_eq!(get("customer").description, "A happy customer");
        assert_eq!(get("cloud.ui").technology, "React");
        assert_eq!(get("cloud.db").shape, Shape::Cylinder);
        assert_eq!(get("cloud.backend.worker").title, "Worker");
        assert_eq!(get("cloud").children.len(), 3);
        assert_eq!(m.roots.len(), 2);
        assert_eq!(m.relations.len(), 6);
        let rel = &m.relations[0];
        assert_eq!(m.elements[rel.source].fqn, "cloud.ui");
        assert_eq!(m.elements[rel.target].fqn, "cloud.backend");
    }
}
