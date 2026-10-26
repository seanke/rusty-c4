//! Mermaid diagrams. A `flowchart` (or `graph`) becomes part of the model: subgraphs are
//! containers, nodes are elements (shaped by their brackets) and links are relationships.
//! A `sequenceDiagram` becomes a dynamic view; its participants are the model's elements
//! where they share a name, and new elements otherwise.

use std::collections::HashMap;

use crate::model::Shape;
use crate::parser::{At, Builder, Diagnostic, RawRelation, RawStep, RawView};

pub(crate) fn parse(file: &str, src: &str, b: &mut Builder) {
    let mut lines: Vec<(usize, &str)> = src.lines().enumerate().map(|(i, l)| (i + 1, l)).collect();
    // Front matter: `---` ... `---`, possibly with a title.
    let mut title = None;
    if lines.first().is_some_and(|(_, l)| l.trim() == "---") {
        let end = lines.iter().skip(1).position(|(_, l)| l.trim() == "---").map(|p| p + 1);
        if let Some(end) = end {
            for (_, l) in &lines[1..end] {
                if let Some(t) = l.trim().strip_prefix("title:") {
                    title = Some(unquote(t.trim()).to_string());
                }
            }
            lines.drain(..=end);
        }
    }
    let lines: Vec<(usize, &str)> = lines
        .into_iter()
        .filter(|(_, l)| !l.trim().is_empty() && !l.trim_start().starts_with("%%"))
        .collect();
    let Some(&(header_line, header)) = lines.first() else { return };
    let kind = header.split_whitespace().next().unwrap_or("");
    match kind {
        "flowchart" | "graph" => flowchart(file, &lines[1..], b),
        "sequenceDiagram" => sequence(file, &lines[1..], title, b),
        _ => b.diagnostics.push(Diagnostic {
            file: file.to_string(),
            line: header_line,
            col: indent(header),
            len: kind.chars().count(),
            message: format!("`{kind}` diagrams aren't supported (try `flowchart` or `sequenceDiagram`)"),
            fix: None,
            error: false,
        }),
    }
}

fn indent(l: &str) -> usize {
    l.chars().take_while(|c| c.is_whitespace()).count()
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(s)
}

/// Node or subgraph text: `<br>` starts a new line, other markup is dropped. The first line
/// is the title and the rest the description.
fn split_text(text: &str) -> (String, String) {
    let mut t = unquote(text).replace("<br/>", "\n").replace("<br />", "\n").replace("<br>", "\n");
    while let Some(a) = t.find('<') {
        let Some(b) = t[a..].find('>') else { break };
        t.replace_range(a..=a + b, "");
    }
    let lines: Vec<String> = t
        .lines()
        .map(|l| l.split_whitespace().filter(|w| !w.starts_with("fa:")).collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect();
    match lines.split_first() {
        Some((first, rest)) => (first.clone(), rest.join(" ")),
        None => (String::new(), String::new()),
    }
}

fn is_id_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Bracket pairs that give a node its shape, longest first.
const SHAPES: &[(&str, &str, Option<Shape>)] = &[
    ("(((", ")))", Some(Shape::Person)),
    ("([", "])", None),
    ("[[", "]]", None),
    ("[(", ")]", Some(Shape::Cylinder)),
    ("((", "))", Some(Shape::Person)),
    ("{{", "}}", None),
    ("[/", "/]", None),
    ("[\\", "\\]", None),
    ("[/", "\\]", None),
    ("[\\", "/]", None),
    ("[", "]", None),
    ("(", ")", None),
    ("{", "}", None),
    (">", "]", Some(Shape::Queue)),
];

struct Node {
    id: String,
    text: Option<String>,
    shape: Option<Shape>,
    class: Option<String>,
    col: usize,
}

/// One node (with any bracketed text and `:::class`) at the start of `s`.
fn node(s: &str, col: usize) -> Option<(Node, &str)> {
    let id_len: usize = s.chars().take_while(|c| is_id_char(*c)).map(|c| c.len_utf8()).sum();
    if id_len == 0 {
        return None;
    }
    let id = s[..id_len].to_string();
    let mut rest = &s[id_len..];
    let mut text = None;
    let mut shape = None;
    for (open, close, sh) in SHAPES {
        if let Some(after) = rest.strip_prefix(open) {
            // Quoted text may contain the closing brackets.
            let end = if after.trim_start().starts_with('"') {
                let q = after.find('"').unwrap();
                after[q + 1..].find('"').map(|e| q + 1 + e + 1).and_then(|e| after[e..].find(close).map(|c| e + c))
            } else {
                after.find(close)
            };
            if let Some(end) = end {
                text = Some(after[..end].to_string());
                shape = Some(*sh);
                rest = &after[end + close.len()..];
                break;
            }
        }
    }
    let mut class = None;
    if let Some(after) = rest.strip_prefix(":::") {
        let n: usize = after.chars().take_while(|c| is_id_char(*c)).map(|c| c.len_utf8()).sum();
        class = Some(after[..n].to_string());
        rest = &after[n..];
    }
    Some((
        Node {
            id,
            text,
            shape: shape.flatten(),
            class,
            col,
        },
        rest,
    ))
}

/// A link at the start of `s`: its label, whether it points both ways, whether it is drawn
/// at all (`~~~` is invisible), and what follows.
fn link(s: &str) -> Option<(Option<String>, bool, bool, &str)> {
    let both = s.starts_with('<');
    let body = s.trim_start_matches('<');
    let run: usize = body.chars().take_while(|c| matches!(c, '-' | '=' | '.' | '~')).count();
    if run < 2 {
        return None;
    }
    let visible = !body.starts_with('~');
    let mut rest = &body[run..];
    let mut label = None;
    let arrowed = rest.starts_with(['>', 'x', 'o']);
    if arrowed {
        rest = &rest[1..];
    } else if run == 2 && !rest.starts_with(['>', '|']) {
        // `-- text -->`, `== text ==>`, `-. text .->`: text, then the rest of the link.
        let closers = ["-->", "---", "==>", "===", ".->", ".-", "--x", "--o"];
        let found = closers.iter().filter_map(|c| rest.find(c).map(|i| (i, c.len()))).min();
        if let Some((i, n)) = found {
            let text = rest[..i].trim();
            if !text.is_empty() {
                label = Some(unquote(text).to_string());
            }
            rest = &rest[i + n..];
            rest = rest.trim_start_matches(['-', '=', '.', '>']);
        }
    }
    let trimmed = rest.trim_start();
    if let Some(after) = trimmed.strip_prefix('|') {
        if let Some(end) = after.find('|') {
            label = Some(unquote(&after[..end]).to_string());
            rest = &after[end + 1..];
        }
    }
    Some((label, both, visible, rest))
}

struct Edge {
    from: String,
    to: String,
    label: Option<String>,
    from_at: At,
    to_at: At,
}

fn colour_of(spec: &str) -> Option<String> {
    spec.split([',', ';'])
        .filter_map(|kv| kv.trim().strip_prefix("fill:"))
        .map(|v| v.trim().to_string())
        .find(|v| v.starts_with('#'))
}

fn flowchart(file: &str, lines: &[(usize, &str)], b: &mut Builder) {
    // Subgraphs (id -> fqn, title), nodes (id -> home subgraph, title, shape, class).
    let mut stack: Vec<String> = Vec::new();
    let mut subs: HashMap<String, (String, String)> = HashMap::new();
    let mut sub_order: Vec<String> = Vec::new();
    let mut homes: HashMap<String, Option<String>> = HashMap::new();
    let mut node_order: Vec<String> = Vec::new();
    let mut texts: HashMap<String, (Option<String>, Option<Shape>)> = HashMap::new();
    let mut classes: HashMap<String, String> = HashMap::new();
    let mut class_colours: HashMap<String, String> = HashMap::new();
    let mut colours: HashMap<String, String> = HashMap::new();
    let mut edges: Vec<Edge> = Vec::new();
    let at = |line: usize, col: usize| At {
        file: file.to_string(),
        line,
        col,
    };

    for &(line_no, raw) in lines {
        for stmt in raw.split(';') {
            let col0 = raw.find(stmt.trim_start()).unwrap_or(0);
            let st = stmt.trim();
            if st.is_empty() {
                continue;
            }
            let word = st.split_whitespace().next().unwrap_or("");
            let rest = st[word.len()..].trim();
            match word {
                "subgraph" => {
                    // `subgraph id [Title]` may have a space before the bracket.
                    let id_len: usize = rest.chars().take_while(|c| is_id_char(*c)).map(|c| c.len_utf8()).sum();
                    let joined = format!("{}{}", &rest[..id_len], rest[id_len..].trim_start());
                    let (id, title) = match node(&joined, 0) {
                        Some((n, after)) if after.trim().is_empty() => {
                            let t = n.text.map(|t| split_text(&t).0).unwrap_or_else(|| n.id.clone());
                            (n.id, t)
                        }
                        _ => {
                            let t = unquote(rest).to_string();
                            let id: String = t.chars().map(|c| if is_id_char(c) { c } else { '_' }).collect();
                            (id, t)
                        }
                    };
                    let fqn = match stack.last().and_then(|p| subs.get(p)) {
                        Some((p, _)) => format!("{p}.{id}"),
                        None => id.clone(),
                    };
                    subs.insert(id.clone(), (fqn, title));
                    sub_order.push(id.clone());
                    stack.push(id);
                }
                "end" => {
                    stack.pop();
                }
                "direction" | "linkStyle" | "click" => {}
                "classDef" => {
                    let mut parts = rest.splitn(2, char::is_whitespace);
                    let names = parts.next().unwrap_or("");
                    if let Some(c) = colour_of(parts.next().unwrap_or("")) {
                        for n in names.split(',') {
                            class_colours.insert(n.trim().to_string(), c.clone());
                        }
                    }
                }
                "class" => {
                    let mut parts = rest.rsplitn(2, char::is_whitespace);
                    let class = parts.next().unwrap_or("").to_string();
                    for id in parts.next().unwrap_or("").split(',') {
                        classes.insert(id.trim().to_string(), class.clone());
                    }
                }
                "style" => {
                    let mut parts = rest.splitn(2, char::is_whitespace);
                    let id = parts.next().unwrap_or("").to_string();
                    if let Some(c) = colour_of(parts.next().unwrap_or("")) {
                        colours.insert(id, c);
                    }
                }
                _ => {
                    // A chain: nodes (joined by `&`) separated by links.
                    let mut groups: Vec<Vec<Node>> = Vec::new();
                    let mut links: Vec<(Option<String>, bool, bool)> = Vec::new();
                    let mut s = st;
                    let mut ok = true;
                    loop {
                        let mut group = Vec::new();
                        loop {
                            let col = col0 + (st.len() - s.len());
                            let Some((n, after)) = node(s, col) else {
                                ok = false;
                                break;
                            };
                            group.push(n);
                            s = after.trim_start();
                            match s.strip_prefix('&') {
                                Some(after) => s = after.trim_start(),
                                None => break,
                            }
                        }
                        if !ok {
                            break;
                        }
                        groups.push(group);
                        if s.is_empty() {
                            break;
                        }
                        match link(s) {
                            Some((label, both, visible, after)) => {
                                links.push((label, both, visible));
                                s = after.trim_start();
                            }
                            None => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if !ok || groups.len() != links.len() + 1 {
                        b.diagnostics.push(Diagnostic {
                            file: file.to_string(),
                            line: line_no,
                            col: col0,
                            len: st.chars().count(),
                            message: "couldn't read this line".into(),
                            fix: None,
                            error: false,
                        });
                        continue;
                    }
                    let home = stack.last().cloned();
                    for n in groups.iter().flatten() {
                        if !homes.contains_key(&n.id) {
                            homes.insert(n.id.clone(), home.clone());
                            node_order.push(n.id.clone());
                        }
                        let entry = texts.entry(n.id.clone()).or_insert((None, None));
                        if n.text.is_some() {
                            *entry = (n.text.clone(), n.shape);
                        }
                        if let Some(c) = &n.class {
                            classes.insert(n.id.clone(), c.clone());
                        }
                    }
                    for (i, (label, both, visible)) in links.iter().enumerate() {
                        if !visible {
                            continue;
                        }
                        for a in &groups[i] {
                            for z in &groups[i + 1] {
                                edges.push(Edge {
                                    from: a.id.clone(),
                                    to: z.id.clone(),
                                    label: label.clone(),
                                    from_at: at(line_no, a.col),
                                    to_at: at(line_no, z.col),
                                });
                                if *both {
                                    edges.push(Edge {
                                        from: z.id.clone(),
                                        to: a.id.clone(),
                                        label: label.clone(),
                                        from_at: at(line_no, z.col),
                                        to_at: at(line_no, a.col),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let colour = |id: &str| colours.get(id).cloned().or_else(|| classes.get(id).and_then(|c| class_colours.get(c)).cloned());
    for id in &sub_order {
        let (fqn, title) = &subs[id];
        let e = b.element(fqn);
        e.declared = true;
        e.kind = "group".into();
        e.title = Some(title.clone());
        e.style.color = colour(id).or(Some("secondary".into()));
    }
    let mut fqns: HashMap<String, String> = subs.iter().map(|(id, (f, _))| (id.clone(), f.clone())).collect();
    for id in &node_order {
        if subs.contains_key(id) {
            continue;
        }
        let fqn = match homes[id].as_ref().and_then(|h| subs.get(h)) {
            Some((p, _)) => format!("{p}.{id}"),
            None => id.clone(),
        };
        let (text, shape) = texts.get(id).cloned().unwrap_or((None, None));
        let (title, description) = text.map(|t| split_text(&t)).unwrap_or_else(|| (id.clone(), String::new()));
        let e = b.element(&fqn);
        e.declared = true;
        e.kind = match shape {
            Some(Shape::Cylinder) => "database".into(),
            Some(Shape::Queue) => "queue".into(),
            Some(Shape::Person) => "person".into(),
            _ => String::new(),
        };
        e.title = Some(title);
        if !description.is_empty() {
            e.description = Some(description);
        }
        e.style.shape = shape;
        e.style.color = colour(id).or(match shape {
            Some(Shape::Cylinder) => Some("slate".into()),
            Some(Shape::Person) => Some("amber".into()),
            Some(Shape::Queue) => Some("indigo".into()),
            _ => None,
        });
        fqns.insert(id.clone(), fqn);
    }
    for e in edges {
        b.relations.push(RawRelation {
            scope: String::new(),
            source: Some(fqns.get(&e.from).cloned().unwrap_or(e.from)),
            target: fqns.get(&e.to).cloned().unwrap_or(e.to),
            title: e.label.unwrap_or_default(),
            technology: String::new(),
            source_at: Some(e.from_at),
            target_at: e.to_at,
        });
    }
}

/// Message arrows, longest first. Those starting `--` are drawn dashed: replies.
const ARROWS: &[&str] = &["<<-->>", "<<->>", "-->>", "->>", "--x", "-x", "--)", "-)", "-->", "->"];

fn sequence(file: &str, lines: &[(usize, &str)], title: Option<String>, b: &mut Builder) {
    let name = std::path::Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| file.to_string());
    let mut view = RawView {
        name: name.clone(),
        title,
        description: String::new(),
        steps: Vec::new(),
    };
    let mut stage = 0;
    // Open blocks: for a `par`, the stage its branches share.
    let mut blocks: Vec<Option<usize>> = Vec::new();
    for &(line_no, raw) in lines {
        let st = raw.trim();
        let col0 = indent(raw);
        let word = st.split_whitespace().next().unwrap_or("");
        let rest = st[word.len()..].trim();
        match word {
            "title" => view.title = Some(rest.trim_start_matches(':').trim().to_string()),
            "participant" | "actor" => {
                let (id, alias) = match rest.split_once(" as ") {
                    Some((id, alias)) => (unquote(id).to_string(), unquote(alias).to_string()),
                    None => (unquote(rest).to_string(), unquote(rest).to_string()),
                };
                b.participants.push((id, alias, word == "actor"));
            }
            "create" | "destroy" | "activate" | "deactivate" | "autonumber" | "Note" | "note" | "box" | "rect" => {
                if matches!(word, "box" | "rect") {
                    blocks.push(None);
                }
            }
            "par" => {
                blocks.push(Some(stage));
            }
            "loop" | "alt" | "opt" | "critical" | "break" => blocks.push(None),
            "else" | "and" | "option" => {}
            "end" => {
                if let Some(Some(s)) = blocks.pop() {
                    stage = s + 1;
                }
            }
            _ => {
                let (head, message) = st.split_once(':').unwrap_or((st, ""));
                let arrow = ARROWS.iter().filter_map(|a| head.find(a).map(|i| (i, *a))).min_by_key(|(i, a)| (*i, std::cmp::Reverse(a.len())));
                let Some((i, arrow)) = arrow else {
                    b.diagnostics.push(Diagnostic {
                        file: file.to_string(),
                        line: line_no,
                        col: col0,
                        len: st.chars().count(),
                        message: "couldn't read this line".into(),
                        fix: None,
                        error: false,
                    });
                    continue;
                };
                let from = head[..i].trim().to_string();
                let to_raw = &head[i + arrow.len()..];
                let to = to_raw.trim().trim_start_matches(['+', '-']).trim().to_string();
                let from_col = col0 + raw.trim_start().find(&from).unwrap_or(0);
                let to_col = col0 + i + arrow.len() + (to_raw.len() - to_raw.trim_start_matches(|c: char| c.is_whitespace() || c == '+' || c == '-').len());
                for p in [&from, &to] {
                    b.participants.push((p.clone(), p.clone(), false));
                }
                let par = blocks.iter().rev().find_map(|b| *b);
                view.steps.push(RawStep {
                    from,
                    to,
                    title: message.trim().to_string(),
                    technology: String::new(),
                    backward: arrow.starts_with("--"),
                    stage: par.unwrap_or(stage),
                    from_at: At {
                        file: file.to_string(),
                        line: line_no,
                        col: from_col,
                    },
                    to_at: At {
                        file: file.to_string(),
                        line: line_no,
                        col: to_col,
                    },
                });
                if par.is_none() {
                    stage += 1;
                }
            }
        }
    }
    // Participant aliases name the elements; keep the first (declared) one for each id.
    let mut seen = std::collections::HashSet::new();
    b.participants.retain(|(id, _, _)| seen.insert(id.clone()));
    b.views.push(view);
}

#[cfg(test)]
mod tests {
    use crate::parser::parse_sources;

    #[test]
    fn reads_flowcharts_and_sequences() {
        let flow = "flowchart LR\n  user((User))\n  subgraph app [App]\n    web[Web<br/>React] --> api{{API}}\n    api --> db[(DB)]\n  end\n  user -->|uses| web & api\n  classDef hot fill:#ff0000\n  class db hot\n";
        let seq = "sequenceDiagram\n  title Login\n  actor user as User\n  user->>web: open\n  web->>+api: login\n  par\n    api-)db: write\n  and\n    api--)web: ok\n  end\n  web-->>user: done\n";
        let (m, d) = parse_sources(&[("a.mmd".into(), flow.into()), ("b.mmd".into(), seq.into())]);
        assert!(d.is_empty(), "{d:?}");
        let get = |f: &str| &m.elements[m.by_fqn[f]];
        assert_eq!(get("app.web").title, "Web");
        assert_eq!(get("app.web").description, "React");
        assert_eq!(get("app.db").shape, crate::model::Shape::Cylinder);
        assert_eq!(get("user").shape, crate::model::Shape::Person);
        assert_eq!(m.relations.len(), 4);
        let v = &m.views[0];
        assert_eq!(v.title, "Login");
        let stages: Vec<_> = v.steps.iter().map(|s| (m.elements[s.from].fqn.as_str(), m.elements[s.to].fqn.as_str(), s.stage, s.backward)).collect();
        assert_eq!(
            stages,
            vec![
                ("user", "app.web", 0, false),
                ("app.web", "app.api", 1, false),
                ("app.api", "app.db", 2, false),
                ("app.api", "app.web", 2, true),
                ("app.web", "user", 3, true),
            ]
        );
    }
}
