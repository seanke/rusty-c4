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
    BackArrow,
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
    /// Column (in characters) where the token starts.
    col: usize,
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
    let mut line_start = 0;
    while i < s.len() {
        let c = s[i];
        let start_line = line;
        let col = i - line_start;
        let mut push = |tok: Tok| out.push(Token { tok, line: start_line, col });
        match c {
            '\n' => {
                line += 1;
                i += 1;
                line_start = i;
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
                        line_start = i + 1;
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
                            line_start = i + 1;
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
            '<' if at(i + 1) == '-' => {
                i += 2;
                push(Tok::BackArrow);
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

/// A problem found in a source file, with where it is and (often) a likely fix.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub file: String,
    /// 1-based line; 0-based column and length, in characters.
    pub line: usize,
    pub col: usize,
    pub len: usize,
    pub message: String,
    /// Text to put in place of the marked range.
    pub fix: Option<String>,
    /// An error (something is broken) rather than a warning (something was ignored).
    pub error: bool,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.file, self.line, self.message)
    }
}

/// Where a name was written: file, line, column.
#[derive(Clone, Debug, Default)]
pub(crate) struct At {
    pub file: String,
    pub line: usize,
    pub col: usize,
}

#[derive(Default)]
pub(crate) struct RawElement {
    pub kind: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub technology: Option<String>,
    pub style: Style,
    pub declared: bool,
    /// Where its name is written, and the name's length.
    pub decl: Option<(At, usize)>,
}

pub(crate) struct RawRelation {
    pub scope: String,
    pub source: Option<String>,
    pub target: String,
    pub title: String,
    pub technology: String,
    pub source_at: Option<At>,
    pub target_at: At,
}

#[derive(Default)]
pub(crate) struct Builder {
    pub kinds: HashMap<String, Style>,
    pub colors: HashMap<String, [f32; 3]>,
    pub elements: HashMap<String, RawElement>,
    pub order: Vec<String>,
    pub relations: Vec<RawRelation>,
    pub views: Vec<RawView>,
    pub diagnostics: Vec<Diagnostic>,
    /// Element kinds as written, to check against the specification.
    pub kind_uses: Vec<(String, At)>,
    /// Sequence-diagram participants, made into elements unless they name existing ones.
    pub participants: Vec<(String, String, bool)>,
}

pub(crate) struct RawStep {
    pub from: String,
    pub to: String,
    pub title: String,
    pub technology: String,
    pub backward: bool,
    pub stage: usize,
    pub from_at: At,
    pub to_at: At,
}

pub(crate) struct RawView {
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    pub steps: Vec<RawStep>,
}

impl Builder {
    pub fn element(&mut self, fqn: &str) -> &mut RawElement {
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
    /// Where the name of the element being declared is.
    name_at: Option<At>,
}

impl Parser<'_> {
    fn peek(&self, n: usize) -> Option<&Tok> {
        self.toks.get(self.pos + n).map(|t| &t.tok)
    }

    fn line(&self) -> usize {
        self.toks.get(self.pos).map(|t| t.line).unwrap_or(0)
    }

    /// Where the current token is.
    fn at(&self) -> At {
        let t = self.toks.get(self.pos);
        At {
            file: self.file.clone(),
            line: t.map_or(0, |t| t.line),
            col: t.map_or(0, |t| t.col),
        }
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
                Some(Tok::Ident(k)) if k == "views" => {
                    self.strings();
                    if self.eat_block_open() {
                        self.parse_views();
                    }
                }
                Some(Tok::LBrace) => self.skip_rest_of_block(),
                _ => {}
            }
        }
    }

    /// A `views` block: dynamic views are read, everything else skipped.
    fn parse_views(&mut self) {
        loop {
            match self.next() {
                None | Some(Tok::RBrace) => return,
                Some(Tok::LBrace) => self.skip_rest_of_block(),
                Some(Tok::Ident(k)) if k == "dynamic" && matches!(self.peek(0), Some(Tok::Ident(v)) if v == "view") => {
                    self.pos += 1;
                    let name = self.ident().unwrap_or_default();
                    let title = self.strings().into_iter().next();
                    // Skip anything before the body (e.g. tags).
                    while !matches!(self.peek(0), Some(Tok::LBrace) | Some(Tok::RBrace) | None) {
                        self.pos += 1;
                    }
                    if self.eat_block_open() {
                        let mut view = RawView {
                            name,
                            title,
                            description: String::new(),
                            steps: Vec::new(),
                        };
                        let mut stage = 0;
                        self.parse_dynamic(&mut view, &mut stage, false);
                        self.b.views.push(view);
                    }
                }
                _ => {}
            }
        }
    }

    /// The body of a dynamic view (or of a `parallel` block inside one, whose steps all
    /// share a stage).
    fn parse_dynamic(&mut self, view: &mut RawView, stage: &mut usize, parallel: bool) {
        loop {
            let line = self.line();
            let Some(t) = self.peek(0).cloned() else { return };
            match t {
                Tok::RBrace => {
                    self.pos += 1;
                    return;
                }
                Tok::LBrace => {
                    self.pos += 1;
                    self.skip_rest_of_block();
                }
                Tok::Ident(k) if (k == "title" || k == "description") && matches!(self.peek(1), Some(Tok::Str(_))) => {
                    self.pos += 1;
                    let v = self.string().unwrap_or_default();
                    if k == "title" {
                        view.title = Some(v);
                    } else {
                        view.description = v;
                    }
                }
                Tok::Ident(k) if (k == "parallel" || k == "par") && self.peek(1) == Some(&Tok::LBrace) => {
                    self.pos += 2;
                    self.parse_dynamic(view, stage, true);
                    *stage += 1;
                }
                Tok::Ident(src) if matches!(self.peek(1), Some(Tok::Arrow | Tok::BackArrow)) => {
                    let src_at = self.at();
                    self.pos += 1;
                    let backward = self.next() == Some(Tok::BackArrow);
                    let dst_at = self.at();
                    let Some(dst) = self.ident() else {
                        self.skip_line(line);
                        continue;
                    };
                    let strs = self.strings();
                    let mut title = strs.first().cloned().unwrap_or_default();
                    let mut technology = strs.get(1).cloned().unwrap_or_default();
                    self.skip_tags();
                    if self.eat_block_open() {
                        while let Some(t) = self.next() {
                            match t {
                                Tok::RBrace => break,
                                Tok::LBrace => self.skip_rest_of_block(),
                                Tok::Ident(k) if k == "title" => title = self.string().unwrap_or(title),
                                Tok::Ident(k) if k == "technology" => technology = self.string().unwrap_or(technology),
                                _ => {}
                            }
                        }
                    }
                    let ((from, from_at), (to, to_at)) = if backward {
                        ((dst, dst_at), (src, src_at))
                    } else {
                        ((src, src_at), (dst, dst_at))
                    };
                    view.steps.push(RawStep {
                        from,
                        to,
                        title,
                        technology,
                        backward,
                        stage: *stage,
                        from_at,
                        to_at,
                    });
                    if !parallel {
                        *stage += 1;
                    }
                }
                _ => {
                    self.pos += 1;
                    self.skip_line(line);
                }
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
                Tok::Arrow | Tok::ArrowOpen | Tok::BiArrow => self.parse_relation(scope, None, None),
                Tok::Dot if matches!(self.peek(1), Some(Tok::Ident(_))) => {
                    self.parse_relation(scope, None, None)
                }
                Tok::LBrace => {
                    self.pos += 1;
                    self.skip_rest_of_block();
                }
                Tok::Ident(name) => {
                    let next = self.peek(1).cloned();
                    match next {
                        Some(Tok::Arrow | Tok::ArrowOpen | Tok::BiArrow) => {
                            let at = self.at();
                            self.pos += 1;
                            self.parse_relation(scope, Some(name), Some(at));
                        }
                        Some(Tok::Dot) if matches!(self.peek(2), Some(Tok::Ident(_))) => {
                            let at = self.at();
                            self.pos += 1;
                            self.parse_relation(scope, Some(name), Some(at));
                        }
                        Some(Tok::Eq) => {
                            self.name_at = Some(self.at());
                            self.pos += 2;
                            let kind_at = self.at();
                            if let Some(Tok::Ident(k)) = self.peek(0) {
                                self.b.kind_uses.push((k.clone(), kind_at));
                            }
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
                            let kind_at = self.at();
                            self.b.kind_uses.push((name.clone(), kind_at));
                            self.pos += 1;
                            self.name_at = Some(self.at());
                            self.pos += 1;
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
        let name_at = self.name_at.take();
        let e = self.b.element(&fqn);
        e.kind = kind.to_string();
        e.declared = true;
        if let Some(at) = name_at {
            e.decl = Some((at, name.chars().count()));
        }
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

    fn parse_relation(&mut self, scope: &str, source: Option<String>, source_at: Option<At>) {
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
        let target_at = self.at();
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
        let rel = RawRelation {
            scope: scope.to_string(),
            source: source.clone(),
            target: target.clone(),
            title: title.clone(),
            technology: technology.clone(),
            source_at: source_at.clone(),
            target_at: target_at.clone(),
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
                source_at: Some(target_at.clone()),
                target_at: source_at.unwrap_or(target_at),
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

/// Edit distance between two strings (case-insensitive).
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = (a[i - 1] != b[j - 1]) as usize;
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// The closest of `candidates` to `text`, if it is close enough to be a likely typo.
pub fn suggest<'a>(text: &str, candidates: impl Iterator<Item = &'a str>) -> Option<String> {
    let limit = (text.chars().count() / 3).max(1);
    candidates
        .filter(|c| *c != text)
        .map(|c| (distance(text, c), c))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, c)| (*d, c.len()))
        .map(|(_, c)| c.to_string())
}

fn diag(at: &At, len: usize, message: String, fix: Option<String>, error: bool) -> Diagnostic {
    Diagnostic {
        file: at.file.clone(),
        line: at.line,
        col: at.col,
        len,
        message,
        fix,
        error,
    }
}

fn finish(mut b: Builder) -> (Model, Vec<Diagnostic>) {
    let mut warnings = std::mem::take(&mut b.diagnostics);
    // Sequence-diagram participants that name nothing in the model become elements.
    {
        let declared: Vec<String> = b.order.iter().filter(|f| b.elements[f.as_str()].declared).cloned().collect();
        for (id, title, actor) in std::mem::take(&mut b.participants) {
            let known = declared.iter().any(|f| *f == id || f.rsplit('.').next() == Some(id.as_str()));
            if !known {
                let e = b.element(&id);
                if !e.declared {
                    e.declared = true;
                    e.kind = if actor { "actor".into() } else { "participant".into() };
                    e.title = Some(title);
                    if actor {
                        e.style.shape = Some(Shape::Person);
                        e.style.color = Some("amber".into());
                    }
                }
            }
        }
    }
    let b = b;
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
            warnings.push(Diagnostic {
                file: String::new(),
                line: 0,
                col: 0,
                len: 0,
                message: format!("`extend {fqn}`: no element named `{fqn}`"),
                fix: None,
                error: true,
            });
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
            decl: raw.decl.as_ref().map(|(at, len)| Decl {
                file: at.file.clone(),
                line: at.line,
                col: at.col,
                len: *len,
            }),
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

    // An unknown name: say so, and suggest the nearest real one (full names for dotted
    // references, short names where those are unique).
    let unknown = |text: &str, at: &At| {
        let shorts = by_short.iter().filter(|(_, v)| v.len() == 1).map(|(k, _)| *k);
        let fix = if text.contains('.') {
            suggest(text, declared.iter().copied())
        } else {
            suggest(text, shorts.chain(declared.iter().copied()))
        };
        let message = match &fix {
            Some(f) => format!("can't find `{text}`; did you mean `{f}`?"),
            None => format!("can't find `{text}`"),
        };
        diag(at, text.chars().count(), message, fix, true)
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
            (None, _) => warnings.push(unknown(src_text, r.source_at.as_ref().unwrap_or(&r.target_at))),
            (_, None) => warnings.push(unknown(&r.target, &r.target_at)),
        }
    }

    // Element kinds that the specification doesn't declare.
    if !b.kinds.is_empty() {
        for (kind, at) in &b.kind_uses {
            if !b.kinds.contains_key(kind) {
                let fix = suggest(kind, b.kinds.keys().map(|k| k.as_str()));
                let message = match &fix {
                    Some(f) => format!("unknown element kind `{kind}`; did you mean `{f}`?"),
                    None => format!("unknown element kind `{kind}`"),
                };
                warnings.push(diag(at, kind.chars().count(), message, fix, false));
            }
        }
    }
    model.relations = relations;

    for v in &b.views {
        let mut steps = Vec::new();
        for st in &v.steps {
            match (resolve("", &st.from), resolve("", &st.to)) {
                (Some(from), Some(to)) => steps.push(Step {
                    from,
                    to,
                    title: tidy(&st.title),
                    technology: tidy(&st.technology),
                    backward: st.backward,
                    stage: st.stage,
                }),
                (None, _) => warnings.push(unknown(&st.from, &st.from_at)),
                (_, None) => warnings.push(unknown(&st.to, &st.to_at)),
            }
        }
        if !steps.is_empty() {
            model.views.push(DynamicView {
                name: v.name.clone(),
                title: tidy(v.title.as_deref().unwrap_or(&v.name)),
                description: tidy(&v.description),
                steps,
            });
        }
    }
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
            decl: None,
        });
        model.root = Some(id);
    }
    (model, warnings)
}

/// Whether a file is Mermaid rather than LikeC4, by its name.
pub fn is_mermaid(name: &str) -> bool {
    name.ends_with(".mmd") || name.ends_with(".mermaid")
}

/// Unmatched braces, which otherwise quietly swallow the rest of a file.
fn check_braces(file: &str, toks: &[Token], out: &mut Vec<Diagnostic>) {
    let mut open: Vec<&Token> = Vec::new();
    for t in toks {
        match t.tok {
            Tok::LBrace => open.push(t),
            Tok::RBrace if open.pop().is_none() => out.push(Diagnostic {
                file: file.to_string(),
                line: t.line,
                col: t.col,
                len: 1,
                message: "`}` without a matching `{`".into(),
                fix: None,
                error: true,
            }),
            _ => {}
        }
    }
    for t in open {
        out.push(Diagnostic {
            file: file.to_string(),
            line: t.line,
            col: t.col,
            len: 1,
            message: "`{` is never closed".into(),
            fix: None,
            error: true,
        });
    }
}

/// Parse a set of files (name, text) into one model. Names ending in `.mmd` or `.mermaid`
/// are read as Mermaid; everything else as LikeC4.
pub fn parse_sources(sources: &[(String, String)]) -> (Model, Vec<Diagnostic>) {
    let mut b = Builder::default();
    for (file, src) in sources {
        if is_mermaid(file) {
            crate::mermaid::parse(file, src, &mut b);
            continue;
        }
        let toks = lex(src);
        check_braces(file, &toks, &mut b.diagnostics);
        let mut p = Parser {
            toks,
            pos: 0,
            file: file.clone(),
            b: &mut b,
            name_at: None,
        };
        p.parse_file();
    }
    finish(b)
}

/// All `.c4`, `.likec4`, `.mmd` and `.mermaid` files under `path` (or `path` itself).
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
            } else if name.ends_with(".c4") || name.ends_with(".likec4") || is_mermaid(&name) {
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

/// Read every source file under `path`: (path relative to it, absolute path, text).
pub fn read_sources(path: &Path) -> Result<Vec<(String, PathBuf, String)>, String> {
    let files = source_files(path);
    if files.is_empty() {
        return Err(format!("no .c4, .likec4 or .mmd files found at {}", path.display()));
    }
    let root = if path.is_file() { path.parent().unwrap_or(path) } else { path };
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let rel = f
            .strip_prefix(root)
            .unwrap_or(&f)
            .to_string_lossy()
            .replace('\\', "/");
        out.push((rel, f, text));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_fixes_for_typos() {
        let src = "specification { element system }\nmodel {\n  shop = system { gateway = sytem }\n  x = system\n  x -> shop.gatway\n}";
        let (_, d) = parse_sources(&[("t.c4".into(), src.into())]);
        let fixes: Vec<_> = d.iter().map(|d| (d.line, d.col, d.len, d.fix.clone())).collect();
        assert!(fixes.contains(&(3, 28, 5, Some("system".into()))), "{d:?}");
        assert!(fixes.contains(&(5, 7, 11, Some("shop.gateway".into()))), "{d:?}");
    }

    #[test]
    fn parses_dynamic_views() {
        let src = r#"
            model {
              a = system 'A' { x = container 'X' }
              b = system 'B'
            }
            views {
              view index { include * }
              dynamic view flow {
                title 'The flow'
                description 'How it goes'
                a.x -> b 'asks'
                parallel {
                  b -> a 'one'
                  b -> x 'two' { technology 'HTTP' }
                }
                a.x <- b 'answers'
                autoLayout LeftRight
              }
            }
        "#;
        let (m, warnings) = parse_sources(&[("t.c4".into(), src.into())]);
        assert!(warnings.is_empty(), "{warnings:?}");
        let v = &m.views[0];
        assert_eq!((v.name.as_str(), v.title.as_str(), v.description.as_str()), ("flow", "The flow", "How it goes"));
        let fqn = |id: Id| m.elements[id].fqn.as_str();
        let steps: Vec<_> = v.steps.iter().map(|s| (fqn(s.from), fqn(s.to), s.stage, s.backward)).collect();
        assert_eq!(
            steps,
            vec![("a.x", "b", 0, false), ("b", "a", 1, false), ("b", "a.x", 1, false), ("b", "a.x", 2, true)]
        );
        assert_eq!(v.steps[2].technology, "HTTP");
        assert_eq!(v.stages(), vec![vec![0], vec![1, 2], vec![3]]);
    }

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
