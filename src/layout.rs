//! Lays out one "level" of the model: the children of the focused element as cards inside a
//! frame, plus the outside elements they talk to above and below it. Relationships between
//! deeper descendants are lifted to the visible cards, the way C4 diagrams do it.

use std::collections::{HashMap, HashSet};

use bevy::math::{Rect, Vec2};

use crate::model::{Id, Model};

pub const CARD: Vec2 = Vec2::new(320.0, 180.0);
const GAP_X: f32 = 90.0;
const GAP_Y: f32 = 125.0;
const MAX_ROW: usize = 5;
/// Spacing for left-to-right layouts: wider between ranks to leave room for labels.
const GAP_ALONG_H: f32 = 160.0;
const GAP_ACROSS_H: f32 = 70.0;
const MAX_COL: usize = 4;
const FRAME_PAD: f32 = 56.0;
const FRAME_PAD_TOP: f32 = 96.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// A child of the focus.
    Card,
    /// Something outside the focus that one of its children talks to.
    External,
    /// The focused element itself, drawn around its children.
    Frame,
    /// A sibling of the focus or of one of its ancestors, kept around (faintly) so you can
    /// see where you are. The number is how many levels out it sits.
    Context(u8),
    /// An ancestor of the focus, drawn as a faint frame around its level.
    ContextFrame(u8),
}

impl Role {
    pub fn is_frame(self) -> bool {
        matches!(self, Role::Frame | Role::ContextFrame(_))
    }
}

#[derive(Clone, Debug)]
pub struct Placement {
    pub id: Id,
    /// Where it sits on its own depth layer (before perspective).
    pub rect: Rect,
    pub role: Role,
    /// Depth layer: 0 for the open level, 1 for the level around it, and so on outward.
    pub level: u8,
}

#[derive(Clone, Debug)]
pub struct EdgeSpec {
    pub from: Id,
    pub to: Id,
    pub label: String,
    pub count: usize,
    /// Between context elements: drawn faintly, without a label.
    pub context: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ViewLayout {
    pub nodes: Vec<Placement>,
    pub edges: Vec<EdgeSpec>,
    /// What the camera should frame, as seen through the perspective.
    pub bounds: Rect,
    /// The point deeper layers recede towards: the centre of the open level.
    pub anchor: Vec2,
}

impl ViewLayout {
    pub fn get(&self, id: Id) -> Option<&Placement> {
        self.nodes.iter().find(|p| p.id == id)
    }
}

/// For an element outside the focus, the ancestor-or-self that sits at the same level as one
/// of the focus's ancestors: what a C4 diagram of the focus would show for it.
fn external_rep(model: &Model, x: Id, focus: Id) -> Option<Id> {
    if model.is_ancestor_or_self(x, focus) {
        return None;
    }
    let focus_chain: HashSet<Id> = model.path(Some(focus)).into_iter().collect();
    let mut cur = x;
    loop {
        match model.elements[cur].parent {
            Some(p) if focus_chain.contains(&p) => return Some(cur),
            Some(p) => cur = p,
            None => return Some(cur),
        }
    }
}

/// `aspect` is the width / height of the space the view will be shown in.
pub fn compute(model: &Model, focus: Option<Id>, aspect: f32) -> ViewLayout {
    let inner: Vec<Id> = model.children_of(focus).to_vec();
    let inner_set: HashSet<Id> = inner.iter().copied().collect();
    let rep_inner = |mut x: Id| -> Option<Id> {
        loop {
            if inner_set.contains(&x) {
                return Some(x);
            }
            x = model.elements[x].parent?;
        }
    };

    // Lift every relationship onto what is visible at this level.
    let mut edge_map: HashMap<(Id, Id), EdgeSpec> = HashMap::new();
    let mut edge_order = Vec::new();
    let mut externals: Vec<Id> = Vec::new();
    for r in &model.relations {
        let (a, b) = match (rep_inner(r.source), rep_inner(r.target), focus) {
            (Some(a), Some(b), _) if a != b => (a, b),
            (Some(a), None, Some(f)) => match external_rep(model, r.target, f) {
                Some(b) => (a, b),
                None => continue,
            },
            (None, Some(b), Some(f)) => match external_rep(model, r.source, f) {
                Some(a) => (a, b),
                None => continue,
            },
            _ => continue,
        };
        for x in [a, b] {
            if !inner_set.contains(&x) && !externals.contains(&x) {
                externals.push(x);
            }
        }
        let e = edge_map.entry((a, b)).or_insert_with(|| {
            edge_order.push((a, b));
            EdgeSpec {
                from: a,
                to: b,
                label: String::new(),
                count: 0,
                context: false,
            }
        });
        e.count += 1;
        if e.label.is_empty() {
            e.label = r.title.clone();
        }
    }
    let edges: Vec<EdgeSpec> = edge_order.iter().map(|k| edge_map[k].clone()).collect();

    // --- Layer the inner cards -------------------------------------------------------------
    let n = inner.len();
    let index: HashMap<Id, usize> = inner.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let mut succ = vec![Vec::new(); n];
    let mut pred = vec![Vec::new(); n];
    for e in &edges {
        if let (Some(&a), Some(&b)) = (index.get(&e.from), index.get(&e.to)) {
            succ[a].push(b);
            pred[b].push(a);
        }
    }

    // Drop back edges (found by DFS) so ranking runs on a DAG.
    let mut state = vec![0u8; n];
    let mut dag_succ = vec![Vec::new(); n];
    let mut topo = Vec::new();
    fn dfs(
        v: usize,
        succ: &[Vec<usize>],
        state: &mut [u8],
        dag: &mut [Vec<usize>],
        topo: &mut Vec<usize>,
    ) {
        state[v] = 1;
        for &w in &succ[v] {
            if state[w] == 1 {
                continue;
            }
            dag[v].push(w);
            if state[w] == 0 {
                dfs(w, succ, state, dag, topo);
            }
        }
        state[v] = 2;
        topo.push(v);
    }
    for v in 0..n {
        if state[v] == 0 {
            dfs(v, &succ, &mut state, &mut dag_succ, &mut topo);
        }
    }
    topo.reverse();

    let mut rank = vec![0usize; n];
    for &v in &topo {
        for &w in &dag_succ[v] {
            rank[w] = rank[w].max(rank[v] + 1);
        }
    }
    // Pull nodes down to sit right above whatever they point at.
    for &v in topo.iter().rev() {
        if let Some(min) = dag_succ[v].iter().map(|&w| rank[w]).min() {
            rank[v] = rank[v].max(min.saturating_sub(1));
        }
    }
    let connected: Vec<bool> = (0..n).map(|v| !succ[v].is_empty() || !pred[v].is_empty()).collect();
    let max_rank = rank.iter().copied().max().unwrap_or(0);
    if connected.iter().any(|&c| c) {
        for v in 0..n {
            if !connected[v] {
                rank[v] = max_rank + 1;
            }
        }
    }

    let num_ranks = rank.iter().copied().max().map(|m| m + 1).unwrap_or(0);
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); num_ranks];
    for v in 0..n {
        rows[rank[v]].push(v);
    }

    // Barycenter ordering sweeps.
    let neighbors: Vec<Vec<usize>> = (0..n)
        .map(|v| succ[v].iter().chain(pred[v].iter()).copied().collect())
        .collect();
    let mut pos_in_row = vec![0.0f32; n];
    let refresh = |rows: &Vec<Vec<usize>>, pos: &mut Vec<f32>| {
        for row in rows {
            let len = row.len().max(1) as f32;
            for (i, &v) in row.iter().enumerate() {
                pos[v] = (i as f32 + 0.5) / len;
            }
        }
    };
    refresh(&rows, &mut pos_in_row);
    for sweep in 0..8 {
        let order: Vec<usize> = if sweep % 2 == 0 {
            (0..num_ranks).collect()
        } else {
            (0..num_ranks).rev().collect()
        };
        for r in order {
            let mut keyed: Vec<(f32, usize)> = rows[r]
                .iter()
                .map(|&v| {
                    let ns: Vec<f32> = neighbors[v]
                        .iter()
                        .filter(|&&w| rank[w] != r)
                        .map(|&w| pos_in_row[w])
                        .collect();
                    let key = if ns.is_empty() {
                        pos_in_row[v]
                    } else {
                        ns.iter().sum::<f32>() / ns.len() as f32
                    };
                    (key, v)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            rows[r] = keyed.into_iter().map(|(_, v)| v).collect();
            refresh(&rows, &mut pos_in_row);
        }
    }

    let rows: Vec<Vec<usize>> = rows.into_iter().filter(|r| !r.is_empty()).collect();
    let ctx = Placer {
        inner: &inner,
        rows: &rows,
        neighbors: &neighbors,
        edges: &edges,
        externals: &externals,
        focus,
    };
    // Try both directions and keep whichever fits the window better.
    let score = |v: &ViewLayout| (v.bounds.width() / aspect).max(v.bounds.height());
    let down = ctx.place(false);
    let across = ctx.place(true);
    if score(&across) < score(&down) * 0.92 {
        across
    } else {
        down
    }
}

struct Placer<'a> {
    inner: &'a [Id],
    rows: &'a [Vec<usize>],
    neighbors: &'a [Vec<usize>],
    edges: &'a [EdgeSpec],
    externals: &'a [Id],
    focus: Option<Id>,
}

impl Placer<'_> {
    /// Lays ranks out top-to-bottom, or left-to-right when `horizontal`. Coordinates are
    /// computed as (along the flow, across the flow) and mapped to the world at the end.
    fn place(&self, horizontal: bool) -> ViewLayout {
        let n = self.inner.len();
        let (step_along, step_across, max_row) = if horizontal {
            (CARD.x + GAP_ALONG_H, CARD.y + GAP_ACROSS_H, MAX_COL)
        } else {
            (CARD.y + GAP_Y, CARD.x + GAP_X, MAX_ROW)
        };
        let to_world = |along: f32, across: f32| {
            if horizontal {
                Vec2::new(along, -across)
            } else {
                Vec2::new(across, -along)
            }
        };

        // Wrap wide ranks onto several rows.
        let rows: Vec<Vec<usize>> = self
            .rows
            .iter()
            .flat_map(|row| {
                let chunks = row.len().div_ceil(max_row);
                let per = row.len().div_ceil(chunks.max(1));
                row.chunks(per.max(1)).map(|c| c.to_vec()).collect::<Vec<_>>()
            })
            .collect();

        let mut along = vec![0.0f32; n];
        let mut across = vec![0.0f32; n];
        let mut row_of = vec![0usize; n];
        for (ri, row) in rows.iter().enumerate() {
            let w = (row.len() as f32 - 1.0) * step_across;
            for (i, &v) in row.iter().enumerate() {
                across[v] = i as f32 * step_across - w / 2.0;
                along[v] = ri as f32 * step_along;
                row_of[v] = ri;
            }
        }
        // Nudge each row toward its neighbors while keeping order and spacing.
        for iter in 0..6 {
            let order: Vec<usize> = if iter % 2 == 0 {
                (0..rows.len()).collect()
            } else {
                (0..rows.len()).rev().collect()
            };
            for ri in order {
                let row = &rows[ri];
                let desired: Vec<f32> = row
                    .iter()
                    .map(|&v| {
                        let ns: Vec<f32> = self.neighbors[v]
                            .iter()
                            .filter(|&&w| row_of[w] != ri)
                            .map(|&w| across[w])
                            .collect();
                        if ns.is_empty() {
                            across[v]
                        } else {
                            ns.iter().sum::<f32>() / ns.len() as f32
                        }
                    })
                    .collect();
                pack_row(row, &desired, &mut across, step_across);
            }
        }

        let mut nodes: Vec<Placement> = (0..n)
            .map(|v| Placement {
                id: self.inner[v],
                rect: Rect::from_center_size(to_world(along[v], across[v]), CARD),
                role: Role::Card,
                level: 0,
            })
            .collect();

        let inner_bounds = nodes
            .iter()
            .map(|p| p.rect)
            .reduce(|a, b| a.union(b))
            .unwrap_or_else(|| Rect::from_center_size(Vec2::ZERO, CARD));
        let mut bounds = inner_bounds;

        let Some(f) = self.focus else {
            return ViewLayout {
                nodes,
                edges: self.edges.to_vec(),
                bounds,
                anchor: bounds.center(),
            };
        };
        let frame = pad_frame(inner_bounds);
        nodes.push(Placement {
            id: f,
            rect: frame,
            role: Role::Frame,
            level: 0,
        });
        bounds = frame;

        // Outside elements go on the side of the frame nearest to what they talk to: before
        // or after it along a top-down flow, above or below it in a left-to-right one (which
        // uses the spare height instead of making a wide view wider). Pure sources lean
        // towards "before"/"above" on a tie.
        let index: HashMap<Id, usize> =
            self.inner.iter().enumerate().map(|(i, &id)| (id, i)).collect();
        let mid_along = along.iter().copied().fold(0.0f32, f32::max) / 2.0;
        let mut before = Vec::new();
        let mut after = Vec::new();
        for &ext in self.externals {
            let linked: Vec<(usize, bool)> = self
                .edges
                .iter()
                .filter_map(|e| {
                    if e.from == ext {
                        index.get(&e.to).map(|&v| (v, true))
                    } else if e.to == ext {
                        index.get(&e.from).map(|&v| (v, false))
                    } else {
                        None
                    }
                })
                .collect();
            let count = linked.len().max(1) as f32;
            let mean_along = linked.iter().map(|&(v, _)| along[v]).sum::<f32>() / count;
            let mean_across = linked.iter().map(|&(v, _)| across[v]).sum::<f32>() / count;
            let only_source = linked.iter().all(|&(_, out)| out);
            let (key, slot) = if horizontal {
                (mean_across, mean_along)
            } else {
                (mean_along - mid_along, mean_across)
            };
            let goes_before = if key.abs() < 1.0 { only_source } else { key < 0.0 };
            if goes_before {
                before.push((slot, ext));
            } else {
                after.push((slot, ext));
            }
        }
        let gap = GAP_Y * 0.8;
        let slot_step = CARD.x + GAP_X;
        for (mut list, is_before) in [(before, true), (after, false)] {
            if list.is_empty() {
                continue;
            }
            list.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut xs: Vec<f32> = list.iter().map(|l| l.0).collect();
            let idx: Vec<usize> = (0..list.len()).collect();
            let desired = xs.clone();
            pack_row(&idx, &desired, &mut xs, slot_step);
            for (i, (_, ext)) in list.iter().enumerate() {
                let y = if is_before {
                    frame.max.y + gap + CARD.y / 2.0
                } else {
                    frame.min.y - gap - CARD.y / 2.0
                };
                let center = Vec2::new(xs[i], y);
                let rect = Rect::from_center_size(center, CARD);
                bounds = bounds.union(rect);
                nodes.push(Placement {
                    id: *ext,
                    rect,
                    role: Role::External,
                    level: 0,
                });
            }
        }

        ViewLayout {
            nodes,
            edges: self.edges.to_vec(),
            bounds,
            anchor: frame.center(),
        }
    }
}

/// Extra room between an opened element and the siblings it pushes aside.
const CONTEXT_GAP: f32 = 60.0;

/// How far behind the open level each outer level sits, in units of camera scale.
pub const DEPTH: f32 = 1.0;

/// Perspective shrink of a layer `depth` behind the open level, seen from a camera at
/// orthographic `scale` (the camera's height above the front layer). Zooming in brings the
/// camera closer, so far layers shrink faster than near ones, as in real 3D.
pub fn depth_scale(scale: f32, depth: f32) -> f32 {
    scale / (scale + depth.max(0.0))
}

/// The camera scale that frames `r` in `avail` (kept in step with `Cam::fit`).
pub fn fit_scale(r: Rect, avail: Vec2) -> f32 {
    (r.width() / avail.x).max(r.height() / avail.y).clamp(0.7, 12.0)
}

/// The focus level laid out in front, with each enclosing level on a layer further back.
///
/// Each parent's own layout is "opened up" at the child: everything beyond the child's
/// card is pushed out just far enough that, once perspective shrinks that layer, it still
/// frames the level in front instead of hiding behind it. Outside elements the focus talks
/// to stay where they really live in that larger picture, on their own layer.
pub fn compose(model: &Model, focus: Option<Id>, avail: Vec2) -> ViewLayout {
    let aspect = avail.x / avail.y;
    let Some(f) = focus else {
        // The top level sits inside a frame of its own, the landscape.
        let mut view = compute(model, None, aspect);
        if let (Some(root), false) = (model.root, view.nodes.is_empty()) {
            let frame = pad_frame(view.bounds);
            view.nodes.push(Placement {
                id: root,
                rect: frame,
                role: Role::Frame,
                level: 0,
            });
            view.bounds = frame;
            view.anchor = frame.center();
        }
        return view;
    };
    let frame = compute(model, focus, aspect)
        .get(f)
        .map(|p| p.rect)
        .unwrap_or_default();
    // The perspective depends on how far the camera ends up zoomed out, which depends on
    // what it frames: settle that with one refinement pass.
    let first = compose_at(model, f, aspect, fit_scale(frame, avail));
    let s = fit_scale(first.bounds, avail);
    if s > fit_scale(frame, avail) * 1.08 {
        compose_at(model, f, aspect, s)
    } else {
        first
    }
}

fn compose_at(model: &Model, focus: Id, aspect: f32, scale: f32) -> ViewLayout {
    let mut view = compute(model, Some(focus), aspect);
    let externals: HashSet<Id> = view
        .nodes
        .iter()
        .filter(|p| p.role == Role::External)
        .map(|p| p.id)
        .collect();
    view.nodes.retain(|p| p.role != Role::External);
    let frame = view.get(focus).map(|p| p.rect).unwrap_or(view.bounds);
    let anchor = frame.center();
    let project = |r: Rect, k: f32| Rect::from_center_size(anchor + (r.center() - anchor) * k, r.size() * k);

    let mut external_rects = Vec::new();
    // The region already taken by the levels in front, as seen on screen.
    let mut extent = frame;
    let mut seen: HashSet<Id> = view.nodes.iter().map(|p| p.id).collect();
    let mut edge_keys: HashSet<(Id, Id)> = view.edges.iter().map(|e| (e.from, e.to)).collect();

    let mut open = focus;
    let mut level = 1u8;
    loop {
        let parent = model.elements[open].parent;
        let ctx = compute(model, parent, aspect);
        let Some(card) = ctx.get(open).map(|p| p.rect) else { break };
        let k = depth_scale(scale, DEPTH * level as f32);
        let c = card.center();
        let h = card.half_size();
        // Open a hole that, seen at this depth, lines up with what is in front.
        let grow = ((extent.half_size() + Vec2::splat(CONTEXT_GAP)) / k - h).max(Vec2::ZERO);
        let to = anchor + (extent.center() - anchor) / k;
        let stretch = |d: f32, h: f32, g: f32| {
            if d > h {
                d + g
            } else if d < -h {
                d - g
            } else {
                d * (h + g) / h.max(1.0)
            }
        };
        let map = |p: Vec2| {
            let d = p - c;
            to + Vec2::new(stretch(d.x, h.x, grow.x), stretch(d.y, h.y, grow.y))
        };

        let mut level_bounds = extent;
        let mut parent_frame = None;
        let ctx_cards: HashSet<Id> = ctx
            .nodes
            .iter()
            .filter(|p| p.role == Role::Card)
            .map(|p| p.id)
            .collect();
        for p in &ctx.nodes {
            if p.id == open || seen.contains(&p.id) {
                continue;
            }
            match p.role {
                Role::Card => {
                    let rect = Rect::from_center_size(map(p.rect.center()), p.rect.size());
                    let role = if externals.contains(&p.id) {
                        external_rects.push(project(rect, k));
                        Role::External
                    } else {
                        Role::Context(level)
                    };
                    level_bounds = level_bounds.union(project(rect, k));
                    seen.insert(p.id);
                    view.nodes.push(Placement {
                        id: p.id,
                        rect,
                        role,
                        level,
                    });
                }
                Role::Frame => {
                    let rect = Rect::from_corners(map(p.rect.min), map(p.rect.max));
                    parent_frame = Some(project(rect, k));
                    seen.insert(p.id);
                    view.nodes.push(Placement {
                        id: p.id,
                        rect,
                        role: Role::ContextFrame(level),
                        level,
                    });
                }
                _ => {}
            }
        }
        for e in &ctx.edges {
            let local = ctx_cards.contains(&e.from) && ctx_cards.contains(&e.to);
            if local && e.from != open && e.to != open && edge_keys.insert((e.from, e.to)) {
                view.edges.push(EdgeSpec {
                    label: String::new(),
                    context: true,
                    ..e.clone()
                });
            }
        }

        // The top level has no element of its own around it: draw the landscape there.
        if let (None, Some(root)) = (parent, model.root) {
            let hole = Rect::from_center_size(to, (h + grow) * 2.0);
            let around = ctx
                .nodes
                .iter()
                .filter(|p| p.role == Role::Card && p.id != open)
                .map(|p| Rect::from_center_size(map(p.rect.center()), p.rect.size()))
                .fold(hole, |a, b| a.union(b));
            view.nodes.push(Placement {
                id: root,
                rect: pad_frame(around),
                role: Role::ContextFrame(level),
                level,
            });
        }

        extent = parent_frame.unwrap_or(level_bounds);
        match parent {
            Some(p) => open = p,
            None => break,
        }
        level = level.saturating_add(1);
    }

    // Frame the focus plus the nearest things it talks to, stopping before the focus would
    // shrink too far to read. Anything further out is still hinted at by its edges.
    let limit = frame.size() * 2.0;
    external_rects.sort_by(|a, b| {
        let da = a.center().distance(anchor);
        let db = b.center().distance(anchor);
        da.total_cmp(&db)
    });
    let mut fit = frame;
    for r in external_rects {
        let u = fit.union(r);
        if u.width() <= limit.x && u.height() <= limit.y {
            fit = u;
        }
    }
    view.bounds = fit;
    view.anchor = anchor;
    view
}

/// A frame around `inner`, with room at the top for its title.
fn pad_frame(inner: Rect) -> Rect {
    Rect::new(
        inner.min.x - FRAME_PAD,
        inner.min.y - FRAME_PAD,
        inner.max.x + FRAME_PAD,
        inner.max.y + FRAME_PAD_TOP,
    )
}

/// Places `row` (already ordered) as close to `desired` as possible with `step` spacing.
fn pack_row(row: &[usize], desired: &[f32], x: &mut [f32], step: f32) {
    if row.is_empty() {
        return;
    }
    let mut placed = Vec::with_capacity(row.len());
    for (i, &d) in desired.iter().enumerate() {
        let v = if i == 0 { d } else { d.max(placed[i - 1] + step) };
        placed.push(v);
    }
    // Shift the whole row so it is centred on what it wanted.
    let shift = desired.iter().zip(&placed).map(|(d, p)| d - p).sum::<f32>() / row.len() as f32;
    for (i, &v) in row.iter().enumerate() {
        x[v] = placed[i] + shift;
    }
}
