//! The resolved architecture model: a tree of elements plus relationships between them.

use std::collections::HashMap;

pub type Id = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Shape {
    #[default]
    Rectangle,
    Person,
    Cylinder,
    Queue,
    Browser,
    Mobile,
}

impl Shape {
    pub fn parse(s: &str) -> Option<Shape> {
        Some(match s {
            "rectangle" => Shape::Rectangle,
            "person" => Shape::Person,
            "cylinder" | "storage" => Shape::Cylinder,
            "queue" => Shape::Queue,
            "browser" => Shape::Browser,
            "mobile" => Shape::Mobile,
            _ => return None,
        })
    }

    pub fn index(self) -> f32 {
        match self {
            Shape::Rectangle => 0.0,
            Shape::Person => 1.0,
            Shape::Cylinder => 2.0,
            Shape::Queue => 3.0,
            Shape::Browser => 4.0,
            Shape::Mobile => 5.0,
        }
    }
}

/// Style resolved from the element kind and any element-level `style { }` override.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    pub shape: Option<Shape>,
    pub color: Option<String>,
}

impl Style {
    pub fn merged_over(&self, base: &Style) -> Style {
        Style {
            shape: self.shape.or(base.shape),
            color: self.color.clone().or_else(|| base.color.clone()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Element {
    pub fqn: String,
    pub kind: String,
    pub title: String,
    pub description: String,
    pub technology: String,
    pub parent: Option<Id>,
    pub children: Vec<Id>,
    pub shape: Shape,
    /// RGB in sRGB 0..1.
    pub color: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct Relation {
    pub source: Id,
    pub target: Id,
    pub title: String,
    pub technology: String,
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub elements: Vec<Element>,
    pub relations: Vec<Relation>,
    pub roots: Vec<Id>,
    pub by_fqn: HashMap<String, Id>,
    /// A hidden element standing for the whole landscape: it holds the top-level elements
    /// as its children so it can be drawn as their container, but it is nobody's parent and
    /// never shows up in paths, breadcrumbs or relationships.
    pub root: Option<Id>,
}

impl Model {
    pub fn children_of(&self, focus: Option<Id>) -> &[Id] {
        match focus {
            Some(id) => &self.elements[id].children,
            None => &self.roots,
        }
    }

    /// `a` is `b` or one of its ancestors.
    pub fn is_ancestor_or_self(&self, a: Id, b: Id) -> bool {
        let mut cur = Some(b);
        while let Some(c) = cur {
            if c == a {
                return true;
            }
            cur = self.elements[c].parent;
        }
        false
    }

    /// Path from the root down to `id` (inclusive).
    pub fn path(&self, id: Option<Id>) -> Vec<Id> {
        let mut out = Vec::new();
        let mut cur = id;
        while let Some(c) = cur {
            out.push(c);
            cur = self.elements[c].parent;
        }
        out.reverse();
        out
    }
}

/// LikeC4's named colors, tuned a little darker so white text reads well on top.
pub fn named_color(name: &str) -> Option<[f32; 3]> {
    let hex = match name {
        "primary" | "blue" => "#3563d9",
        "secondary" | "sky" => "#1c86b8",
        "muted" | "slate" => "#4a5568",
        "gray" | "grey" => "#555b66",
        "indigo" => "#5a52cc",
        "green" => "#24895c",
        "amber" => "#b9781a",
        "red" => "#c4423d",
        "orange" => "#c9631f",
        "violet" | "purple" => "#7c4dcc",
        "teal" => "#16857f",
        "pink" => "#c2457f",
        _ => return None,
    };
    parse_hex(hex)
}

pub fn parse_hex(s: &str) -> Option<[f32; 3]> {
    let s = s.trim_start_matches('#');
    let v = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok().map(|x| x as f32 / 255.0);
    match s.len() {
        6 | 8 => Some([v(0)?, v(2)?, v(4)?]),
        3 => {
            let c = |i: usize| {
                u8::from_str_radix(&s[i..i + 1].repeat(2), 16)
                    .ok()
                    .map(|x| x as f32 / 255.0)
            };
            Some([c(0)?, c(1)?, c(2)?])
        }
        _ => None,
    }
}
