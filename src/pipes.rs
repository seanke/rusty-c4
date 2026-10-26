//! Relationships as pipes. Two blocks standing on the same surface are joined by one trunk,
//! routed at right angles like a circuit trace, into their sides. Trunks are planned once
//! and then left alone, so opening a container never moves anything outside it: instead,
//! for each level a relationship goes down through, a small port pops up on the platform
//! beside the component it belongs to, with a short pipe plugged in and the far end's name
//! painted alongside. Ports and tags are placed where they fit, clear of blocks, other pipes
//! and each other; where two trunks must cross, the later one arches over as a bridge.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

use crate::camera::Cam;
use crate::model::{Id, Model};
use crate::labels::{decal_with, DecalQuad};
use crate::paint::{span, Painter};
use crate::render::{neon, DecalMaterial, PipeMaterial, PipeUniform, Spring};
use crate::scene::{Detail, Diagram};
use crate::theme::{Style, Theme};
use crate::world::{Geometry, Heights, View};

/// Pipe radius, in layout units of the level it runs on.
const PIPE_R: f32 = 3.6;
/// How thick the blocky style's dust trails are, as a fraction of their width.
const DUST_THIN: f32 = 0.18;
/// Routing grid cell, in layout units.
const CELL: f32 = 15.0;
/// How far pipes keep from block walls, in cells.
const CLEAR: f32 = 0.8;

/// A relationship line between two visible blocks (possibly several model relations).
#[derive(Clone, Debug)]
pub struct Link {
    pub from: Id,
    pub to: Id,
    pub label: String,
    pub count: usize,
}

#[derive(Resource, Default)]
pub struct Links(pub Vec<Link>);

/// A point a pipe passes through: on a surface (None: the floor), at (x, z).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Waypoint {
    surface: Option<Id>,
    p: Vec2,
}

/// What a pipe is: a trunk between two blocks on a surface, or a port beside a block on its
/// platform, for the relationship with `far` (outgoing when it leads there).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum PipeId {
    Trunk(Option<Id>, Id, Id),
    Port { block: Id, far: Id, outgoing: bool },
}

/// Where a pipe was put. Planned once, then kept (until the model changes), so nothing
/// already on screen moves when something new is added.
#[derive(Clone)]
struct Placed {
    surface: Option<Id>,
    piece: Vec<Waypoint>,
    bridges: Vec<Waypoint>,
    cells: Vec<(usize, usize)>,
    spots: Vec<(Id, usize, i64)>,
    /// Areas kept clear for this pipe's port and tag.
    reserved: Vec<Rect>,
    /// The port's position, for a port pipe.
    port: Option<Vec2>,
    /// The painted tag (image, size in world units) and where it lies, if it fits.
    tag: Option<(Handle<Image>, Vec2, Vec2)>,
    seq: u64,
}

#[derive(Component)]
pub struct Pipe {
    /// The blocks it joins, for hovering and details.
    pub key: (Id, Id),
    pub from: Id,
    pub to: Id,
    /// The visible relationships it carries.
    pub links: Vec<(Id, Id)>,
    id: PipeId,
    piece: Vec<Waypoint>,
    bridges: Vec<Waypoint>,
    port: Option<(Entity, Vec2, Option<Id>)>,
    tag: Option<(Entity, Handle<DecalMaterial>, Vec2, Vec2, Option<Id>)>,
    mesh: Handle<Mesh>,
    material: Handle<PipeMaterial>,
    pub hl: Spring<f32>,
    pub dim: Spring<f32>,
    grow: Spring<f32>,
    delay: f32,
    pub dying: bool,
    built: f32,
    /// The centre line as last built, for picking and the story's pulses.
    pub samples: Vec<Vec3>,
}

impl Pipe {
    /// Fully drawn and staying.
    pub fn ready(&self) -> bool {
        !self.dying && self.built > 0.98
    }

    pub fn is_trunk(&self) -> bool {
        matches!(self.id, PipeId::Trunk(..))
    }
}

#[derive(Resource, Default)]
pub struct PipeState {
    version: u64,
    resets: u64,
    story: u64,
    /// The next routing follows an in-place rebuild: show the pipes at once.
    instant: bool,
    placed: HashMap<PipeId, Placed>,
    seq: u64,
}

/// The ancestor-or-self of `x` that is visible.
pub fn visible_rep(model: &Model, visible: &HashSet<Id>, mut x: Id) -> Option<Id> {
    loop {
        if visible.contains(&x) {
            return Some(x);
        }
        x = model.elements[x].parent?;
    }
}

/// The links between visible blocks: the model's relations, plus `extra` (from, to, title)
/// such as the steps of a dynamic view being played.
pub fn links(model: &Model, visible: &HashSet<Id>, extra: &[(Id, Id, String)]) -> Vec<Link> {
    let mut map: HashMap<(Id, Id), Link> = HashMap::new();
    let mut order = Vec::new();
    let all = model
        .relations
        .iter()
        .map(|r| (r.source, r.target, &r.title, false))
        .chain(extra.iter().map(|(a, b, t)| (*a, *b, t, true)));
    for (source, target, title, is_extra) in all {
        let (Some(a), Some(b)) = (visible_rep(model, visible, source), visible_rep(model, visible, target)) else {
            continue;
        };
        if model.is_ancestor_or_self(a, b) || model.is_ancestor_or_self(b, a) {
            continue;
        }
        // Extra links only add a pipe where there is none; they don't count as relations.
        if is_extra && map.contains_key(&(a, b)) {
            continue;
        }
        let l = map.entry((a, b)).or_insert_with(|| {
            order.push((a, b));
            Link {
                from: a,
                to: b,
                label: String::new(),
                count: 0,
            }
        });
        l.count += 1;
        if l.label.is_empty() {
            l.label = title.clone();
        }
    }
    order.into_iter().map(|k| map[&k].clone()).collect()
}

// --- Routing -------------------------------------------------------------------------------

/// A routing grid over one surface.
struct Surf {
    id: Option<Id>,
    min: Vec2,
    cell: f32,
    nx: usize,
    ny: usize,
    blocked: Vec<bool>,
    /// Cells under ports and tags: pipes go round them if they can.
    avoid: Vec<bool>,
    /// Axes used by pipes already routed through each cell: 1 along x, 2 along z.
    used: Vec<u8>,
}

impl Surf {
    fn new(id: Option<Id>, bounds: Rect, cell: f32, obstacles: &[Rect]) -> Surf {
        let nx = ((bounds.width() / cell).ceil() as usize).max(2);
        let ny = ((bounds.height() / cell).ceil() as usize).max(2);
        let mut s = Surf {
            id,
            min: bounds.min,
            cell,
            nx,
            ny,
            blocked: vec![false; nx * ny],
            avoid: vec![false; nx * ny],
            used: vec![0; nx * ny],
        };
        for r in obstacles {
            let r = Rect::from_center_size(r.center(), r.size() + Vec2::splat(2.0 * CLEAR * cell));
            let (i0, j0) = s.cell_of(r.min);
            let (i1, j1) = s.cell_of(r.max);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    if r.contains(s.center(i, j)) {
                        s.blocked[j * nx + i] = true;
                    }
                }
            }
        }
        s
    }

    fn cell_of(&self, p: Vec2) -> (usize, usize) {
        let q = ((p - self.min) / self.cell).floor();
        (
            (q.x.max(0.0) as usize).min(self.nx - 1),
            (q.y.max(0.0) as usize).min(self.ny - 1),
        )
    }

    fn center(&self, i: usize, j: usize) -> Vec2 {
        self.min + (Vec2::new(i as f32, j as f32) + 0.5) * self.cell
    }

    /// Snap one coordinate (0: x, 1: z) to cell centres.
    fn snap(&self, v: f32, axis: usize) -> f32 {
        let m = if axis == 0 { self.min.x } else { self.min.y };
        ((v - m) / self.cell).floor() * self.cell + m + self.cell * 0.5
    }

    /// Cheapest path of cells from `start` (heading `sdir`) to `goal` (arriving heading
    /// `gdir`): few turns, no overlapping other pipes, crossing them only if worth it.
    fn astar(&self, start: (usize, usize), sdir: usize, goal: (usize, usize), gdir: usize) -> Option<Vec<(usize, usize)>> {
        const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
        let n = self.nx * self.ny;
        let idx = |i: usize, j: usize| j * self.nx + i;
        let h = |i: usize, j: usize| ((i as i32 - goal.0 as i32).abs() + (j as i32 - goal.1 as i32).abs()) as u32 * 10;
        let mut best = vec![u32::MAX; n * 4];
        let mut prev = vec![u32::MAX; n * 4];
        let mut heap = BinaryHeap::new();
        let s = idx(start.0, start.1) * 4 + sdir;
        best[s] = 0;
        heap.push(Reverse((h(start.0, start.1), s as u32)));
        let mut expanded = 0;
        while let Some(Reverse((_, st))) = heap.pop() {
            let st = st as usize;
            let (c, d) = (st / 4, st % 4);
            let (i, j) = (c % self.nx, c / self.nx);
            let g = best[st];
            if (i, j) == goal {
                let mut path = vec![(i, j)];
                let mut cur = st;
                while prev[cur] != u32::MAX {
                    cur = prev[cur] as usize;
                    path.push(((cur / 4) % self.nx, (cur / 4) / self.nx));
                }
                path.reverse();
                path.dedup();
                return Some(path);
            }
            expanded += 1;
            if expanded > 400_000 {
                return None;
            }
            for (nd, (dx, dy)) in DIRS.iter().enumerate() {
                // No turning straight back.
                if nd ^ 1 == d {
                    continue;
                }
                let (ni, nj) = (i as i32 + dx, j as i32 + dy);
                if ni < 0 || nj < 0 || ni >= self.nx as i32 || nj >= self.ny as i32 {
                    continue;
                }
                let (ni, nj) = (ni as usize, nj as usize);
                let nc = idx(ni, nj);
                if self.blocked[nc] && (ni, nj) != goal {
                    continue;
                }
                let axis = if nd < 2 { 1 } else { 2 };
                let mut cost = 10;
                if nd != d {
                    cost += 45;
                }
                // Keep away from the surface's border, so pipes don't trace the rim.
                let ring = ni.min(nj).min(self.nx - 1 - ni).min(self.ny - 1 - nj);
                if ring < 3 {
                    cost += 12 * (3 - ring as u32);
                }
                if self.avoid[nc] {
                    cost += 150;
                }
                let used = self.used[nc];
                if used & axis != 0 {
                    cost += 120;
                } else if used != 0 {
                    cost += 35;
                }
                if (ni, nj) == goal && nd != gdir {
                    cost += 45;
                }
                let ns = nc * 4 + nd;
                let ng = g + cost;
                if ng < best[ns] {
                    best[ns] = ng;
                    prev[ns] = st as u32;
                    heap.push(Reverse((ng + h(ni, nj), ns as u32)));
                }
            }
        }
        None
    }

    /// Cells overlapping `r`.
    fn cells_in(&self, r: Rect) -> impl Iterator<Item = (usize, usize)> + '_ {
        let (i0, j0) = self.cell_of(r.min);
        let (i1, j1) = self.cell_of(r.max);
        (j0..=j1).flat_map(move |j| (i0..=i1).map(move |i| (i, j)))
    }

    fn mark_avoid(&mut self, r: Rect) {
        let cells: Vec<_> = self.cells_in(r).collect();
        for (i, j) in cells {
            self.avoid[j * self.nx + i] = true;
        }
    }

    /// Whether `r` lies over a pipe (other than on the `own` cells).
    fn over_pipes(&self, r: Rect, own: &HashSet<(usize, usize)>) -> bool {
        self.cells_in(r).any(|(i, j)| self.used[j * self.nx + i] != 0 && !own.contains(&(i, j)))
    }

    /// Record a path, returning where it crosses pipes already there.
    fn claim(&mut self, path: &[(usize, usize)]) -> Vec<Vec2> {
        let mut crossings = Vec::new();
        for k in 0..path.len() {
            let (i, j) = path[k];
            let a = if k > 0 { path[k - 1] } else { path[k] };
            let b = if k + 1 < path.len() { path[k + 1] } else { path[k] };
            let mut axis = 0u8;
            if a.0 != i || b.0 != i {
                axis |= 1;
            }
            if a.1 != j || b.1 != j {
                axis |= 2;
            }
            let c = j * self.nx + i;
            let used = self.used[c];
            if k > 1 && k + 2 < path.len() && used != 0 && used & axis == 0 && axis != 3 {
                crossings.push(self.center(i, j));
            }
            self.used[c] |= axis;
        }
        crossings
    }
}

fn dir_index(n: Vec2) -> usize {
    if n.x > 0.5 {
        0
    } else if n.x < -0.5 {
        1
    } else if n.y > 0.5 {
        2
    } else {
        3
    }
}

/// The side of `r` facing `toward`, as an outward normal.
fn pick_side(r: Rect, toward: Vec2) -> Vec2 {
    let d = (toward - r.center()) / r.half_size().max(Vec2::splat(1.0));
    if d.x.abs() > d.y.abs() * 1.15 {
        Vec2::new(d.x.signum(), 0.0)
    } else {
        Vec2::new(0.0, if d.y >= 0.0 { 1.0 } else { -1.0 })
    }
}

/// Corners of a cell path, as waypoints on `surface`.
fn corners(s: &Surf, path: &[(usize, usize)]) -> Vec<Waypoint> {
    let mut out: Vec<Waypoint> = Vec::new();
    for k in 0..path.len() {
        let keep = k == 0 || k + 1 == path.len() || {
            let (a, b, c) = (path[k - 1], path[k], path[k + 1]);
            (b.0 as i32 - a.0 as i32, b.1 as i32 - a.1 as i32) != (c.0 as i32 - b.0 as i32, c.1 as i32 - b.1 as i32)
        };
        if keep {
            out.push(Waypoint {
                surface: s.id,
                p: s.center(path[k].0, path[k].1),
            });
        }
    }
    out
}

struct Router<'a> {
    model: &'a Model,
    geometry: &'a Geometry,
    surfs: HashMap<Option<Id>, Surf>,
    /// Places along block sides already taken: (owner, side, cell).
    taken: HashSet<(Id, usize, i64)>,
    /// Spots taken while placing the current pipe.
    new_spots: Vec<(Id, usize, i64)>,
    /// Areas on each surface kept clear for ports and tags.
    reserved: HashMap<Option<Id>, Vec<Rect>>,
}

impl Router<'_> {
    /// A free spot on `side` of `rect` (owned by `owner`) near `want`, snapped to `grid`.
    fn spot(&mut self, owner: Id, rect: Rect, side: Vec2, want: Vec2, grid: Option<Id>) -> Vec2 {
        let s = &self.surfs[&grid];
        let axis = if side.x != 0.0 { 1 } else { 0 };
        let (lo, hi) = if axis == 1 { (rect.min.y, rect.max.y) } else { (rect.min.x, rect.max.x) };
        let inset = (hi - lo) * 0.18;
        let w = if axis == 1 { want.y } else { want.x };
        let base = s.snap(w.clamp(lo + inset, hi - inset), axis);
        let side_i = dir_index(side);
        let mut along = base;
        for k in 0..40i64 {
            let off = if k % 2 == 0 { k / 2 } else { -(k / 2 + 1) };
            let a = base + off as f32 * s.cell;
            if a < lo + inset * 0.5 || a > hi - inset * 0.5 {
                continue;
            }
            let key = (owner, side_i, (a / s.cell).round() as i64);
            if self.taken.insert(key) {
                self.new_spots.push(key);
                along = a;
                break;
            }
        }
        if axis == 1 {
            Vec2::new(if side.x > 0.0 { rect.max.x } else { rect.min.x }, along)
        } else {
            Vec2::new(along, if side.y > 0.0 { rect.max.y } else { rect.min.y })
        }
    }

    /// The first free cell going out from `p` along `n` on surface `surf`.
    fn out_cell(&self, surf: Option<Id>, p: Vec2, n: Vec2) -> (usize, usize) {
        let s = &self.surfs[&surf];
        let mut q = p;
        for _ in 0..12 {
            let c = s.cell_of(q);
            if !s.blocked[c.1 * s.nx + c.0] {
                return c;
            }
            q += n * s.cell;
        }
        s.cell_of(q)
    }

    /// Where ports and tags may go on a surface: inside a platform's edge, or anywhere on the
    /// floor near the diagram.
    fn bounds(&self, surf: Option<Id>) -> Rect {
        let s = &self.surfs[&surf];
        let r = Rect::from_corners(s.min, s.min + Vec2::new(s.nx as f32, s.ny as f32) * s.cell);
        match surf {
            Some(_) => Rect::from_center_size(r.center(), r.size() + Vec2::splat(s.cell * 0.8)),
            None => r,
        }
    }

    /// Whether `r` is somewhere free on `surf`: in bounds, off every block (and `exclude`'s
    /// pipes), clear of reserved areas and other pipes.
    fn free(&self, surf: Option<Id>, r: Rect, own: &HashSet<(usize, usize)>) -> bool {
        let b = self.bounds(surf);
        if !(b.contains(r.min) && b.contains(r.max)) {
            return false;
        }
        let k = self.geometry.surface_scale(surf);
        let blocks = self.model.children_of(surf).iter().filter_map(|c| self.geometry.geos.get(c));
        for g in blocks {
            let gr = Rect::from_center_size(g.rect().center(), g.rect().size() + Vec2::splat(6.0 * k));
            if !gr.intersect(r).is_empty() {
                return false;
            }
        }
        if self.reserved.get(&surf).is_some_and(|rs| rs.iter().any(|x| !x.intersect(r).is_empty())) {
            return false;
        }
        !self.surfs[&surf].over_pipes(r, own)
    }

    fn reserve(&mut self, surf: Option<Id>, r: Rect) {
        self.reserved.entry(surf).or_default().push(r);
        if let Some(s) = self.surfs.get_mut(&surf) {
            s.mark_avoid(r);
        }
    }

    /// A trunk between `a` and `b`, standing on `surf`.
    fn trunk(&mut self, surf: Option<Id>, a: Id, b: Id) -> Placed {
        let (ga, gb) = (self.geometry.geos[&a], self.geometry.geos[&b]);
        let r = PIPE_R * self.geometry.surface_scale(surf);
        let leave = |router: &mut Self, x: Id, toward: Vec2| {
            let g = router.geometry.geos[&x];
            let side = pick_side(g.rect(), toward);
            let wall = router.spot(x, g.rect(), side, toward, surf);
            let cell = router.out_cell(surf, wall + side * 0.01, side);
            (wall - side * r * 1.5, cell, dir_index(side))
        };
        let (pa, ca, da) = leave(self, a, gb.center);
        let (pb, cb, db) = leave(self, b, ga.center);
        let s = self.surfs.get_mut(&surf).unwrap();
        let cells = s.astar(ca, da, cb, db ^ 1).unwrap_or_else(|| vec![ca, (cb.0, ca.1), cb]);
        let bridges = s.claim(&cells).into_iter().map(|p| Waypoint { surface: surf, p }).collect();
        let mut piece = vec![Waypoint { surface: surf, p: pa }];
        piece.extend(corners(s, &cells));
        piece.push(Waypoint { surface: surf, p: pb });
        piece.dedup();
        Placed {
            surface: surf,
            piece,
            bridges,
            cells,
            spots: std::mem::take(&mut self.new_spots),
            reserved: Vec::new(),
            port: None,
            tag: None,
            seq: 0,
        }
    }

    /// Lay a tag of `size` on the trunk: on a straight run, clear of everything else; or
    /// beside it; or not at all if nowhere fits.
    fn trunk_tag(&mut self, placed: &Placed, size: Vec2) -> Option<Vec2> {
        let surf = placed.surface;
        let own: HashSet<(usize, usize)> = placed.cells.iter().copied().collect();
        let mut runs: Vec<(Vec2, Vec2)> = placed.piece.windows(2).map(|w| (w[0].p, w[1].p)).collect();
        runs.sort_by(|x, y| y.0.distance(y.1).total_cmp(&x.0.distance(x.1)));
        let k = self.geometry.surface_scale(surf);
        for &(a, b) in &runs {
            let dir = (b - a).normalize_or(Vec2::X);
            let perp = Vec2::new(-dir.y, dir.x);
            let side_reach = (perp.x.abs() * size.x + perp.y.abs() * size.y) * 0.5 + PIPE_R * k * 2.5;
            for t in [0.5, 0.35, 0.65, 0.2, 0.8] {
                let c = a.lerp(b, t);
                for off in [0.0, side_reach, -side_reach] {
                    let r = Rect::from_center_size(c + perp * off, size + Vec2::splat(4.0 * k));
                    if self.free(surf, r, &own) {
                        return Some(c + perp * off);
                    }
                }
            }
        }
        None
    }

    /// A port beside `block` (on its parent's platform) toward `toward`, with room for a tag
    /// of `size`: the first spot round the block where neither overlaps anything.
    fn port(&mut self, block: Id, toward: Vec2, size: Option<Vec2>) -> Placed {
        let surf = self.model.elements[block].parent;
        let g = self.geometry.geos[&block];
        let rect = g.rect();
        let k = self.geometry.surface_scale(surf);
        let cell = self.surfs[&surf].cell;
        let r = PIPE_R * k;
        let rp = r * 2.6;
        let gap = r * 1.5;
        let first = pick_side(rect, toward);
        let sides = [first, Vec2::new(first.y, first.x), Vec2::new(-first.y, -first.x), -first];
        let none = HashSet::new();
        let mut fallback: Option<(Vec2, Vec2, Vec2, Option<Rect>)> = None;
        for side in sides {
            let axis = if side.x != 0.0 { 1 } else { 0 };
            let (lo, hi) = if axis == 1 { (rect.min.y, rect.max.y) } else { (rect.min.x, rect.max.x) };
            let want = if axis == 1 { toward.y } else { toward.x };
            let base = want.clamp(lo + cell, hi - cell);
            let mut offsets: Vec<f32> = (0..24).map(|i| if i % 2 == 0 { i / 2 } else { -(i / 2 + 1) } as f32 * cell * 0.75).collect();
            offsets.retain(|o| (base + o) >= lo + cell * 0.6 && (base + o) <= hi - cell * 0.6);
            for d in [cell * 2.0, cell * 3.0, cell * 4.2] {
                for o in &offsets {
                    let along = base + o;
                    let wall = if axis == 1 {
                        Vec2::new(if side.x > 0.0 { rect.max.x } else { rect.min.x }, along)
                    } else {
                        Vec2::new(along, if side.y > 0.0 { rect.max.y } else { rect.min.y })
                    };
                    let at = wall + side * d;
                    let port_r = Rect::from_center_size(at, Vec2::splat(rp * 2.0));
                    // The short run from the wall to the port must be clear too.
                    let run = Rect::from_corners(wall + side * r * 2.0, at).inflate(r);
                    if !self.free(surf, port_r, &none) || !self.free(surf, run, &none) {
                        continue;
                    }
                    let Some(size) = size else {
                        return self.finish_port(block, side, wall, at, None, None);
                    };
                    // The tag beyond the port, or to either side of it.
                    let beyond = side * ((side.x.abs() * size.x + side.y.abs() * size.y) * 0.5 + rp + gap);
                    let across = Vec2::new(side.y.abs(), side.x.abs()) * ((side.y.abs() * size.x + side.x.abs() * size.y) * 0.5 + rp + gap);
                    for off in [beyond, across, -across] {
                        let tag = Rect::from_center_size(at + off, size + Vec2::splat(4.0 * k));
                        if self.free(surf, tag, &none) && tag.intersect(run).is_empty() {
                            return self.finish_port(block, side, wall, at, Some(at + off), Some(tag));
                        }
                    }
                    fallback.get_or_insert((side, wall, at, None));
                }
            }
        }
        // Nowhere for the tag: keep the port alone (its far end shows on hover and click).
        let (side, wall, at, _) = fallback.unwrap_or((first, rect.center() + first * rect.half_size(), rect.center() + first * (rect.half_size() + Vec2::splat(cell * 2.0)), None));
        self.finish_port(block, side, wall, at, None, None)
    }

    fn finish_port(&mut self, block: Id, side: Vec2, wall: Vec2, at: Vec2, tag_at: Option<Vec2>, tag: Option<Rect>) -> Placed {
        let surf = self.model.elements[block].parent;
        let k = self.geometry.surface_scale(surf);
        let r = PIPE_R * k;
        let cell = self.surfs[&surf].cell;
        let spot = (block, dir_index(side), ((if side.x != 0.0 { wall.y } else { wall.x }) / cell).round() as i64);
        self.taken.insert(spot);
        let port_r = Rect::from_center_size(at, Vec2::splat(r * 5.2));
        let run = Rect::from_corners(wall, at).inflate(r);
        let mut reserved = vec![port_r, run];
        reserved.extend(tag);
        for rr in &reserved {
            self.reserve(surf, *rr);
        }
        Placed {
            surface: surf,
            piece: vec![Waypoint { surface: surf, p: wall - side * r * 1.5 }, Waypoint { surface: surf, p: at }],
            bridges: Vec::new(),
            cells: Vec::new(),
            spots: vec![spot],
            reserved,
            port: Some(at),
            tag: tag_at.map(|p| (Handle::default(), Vec2::ZERO, p)),
            seq: 0,
        }
    }
}

/// The surface two blocks share, and the blocks standing on it that hold each.
fn trunk_of(model: &Model, a: Id, b: Id) -> (Option<Id>, Id, Id) {
    let up = |x: Id| std::iter::successors(Some(model.elements[x].parent), |s| s.map(|y| model.elements[y].parent)).collect::<Vec<_>>();
    let (sa, sb) = (up(a), up(b));
    let common = sa.iter().find(|s| sb.contains(s)).copied().flatten();
    let stand = |mut x: Id| {
        while model.elements[x].parent != common {
            match model.elements[x].parent {
                Some(p) => x = p,
                None => break,
            }
        }
        x
    };
    (common, stand(a), stand(b))
}

/// The tag for a trunk: every relationship from inside `a` to inside `b`, as a C4 diagram
/// at that level would show it.
fn trunk_label(model: &Model, a: Id, b: Id, extra: &[(Id, Id, String)]) -> String {
    let inside = |x: Id, y: Id| model.is_ancestor_or_self(x, y);
    let mut titles: Vec<&str> = model
        .relations
        .iter()
        .filter(|r| inside(a, r.source) && inside(b, r.target))
        .map(|r| r.title.as_str())
        .collect();
    if titles.is_empty() {
        titles = extra.iter().filter(|(s, t, _)| inside(a, *s) && inside(b, *t)).map(|(_, _, t)| t.as_str()).collect();
    }
    let first = titles.iter().find(|t| !t.is_empty()).copied().unwrap_or("");
    match (first.is_empty(), titles.len()) {
        (_, 0) => String::new(),
        (true, 1) => String::new(),
        (true, n) => format!("{n} relationships"),
        (false, 1) => truncate(first, 40),
        (false, n) => format!("{}  +{}", truncate(first, 34), n - 1),
    }
}

/// Every pipe the visible links need, in a stable order: the trunks, then the ports going
/// down through each opened container to the components the links belong to.
fn needed(model: &Model, ls: &[Link]) -> Vec<(PipeId, (Id, Id), Vec<(Id, Id)>)> {
    let mut out: Vec<(PipeId, (Id, Id), Vec<(Id, Id)>)> = Vec::new();
    let mut add = |id: PipeId, key: (Id, Id), link: (Id, Id)| match out.iter_mut().find(|(i, _, _)| *i == id) {
        Some((_, _, links)) => {
            if !links.contains(&link) {
                links.push(link)
            }
        }
        None => out.push((id, key, vec![link])),
    };
    for l in ls {
        let (common, a, b) = trunk_of(model, l.from, l.to);
        add(PipeId::Trunk(common, a, b), (a, b), (l.from, l.to));
        // Down from each end's standing block to the element itself, a port per level.
        for (end, stand, far, outgoing) in [(l.from, a, b, true), (l.to, b, a, false)] {
            let path = model.path(Some(end));
            let start = path.iter().position(|x| *x == stand).unwrap_or(path.len());
            for &block in path.iter().skip(start + 1) {
                let key = if outgoing { (block, far) } else { (far, block) };
                add(PipeId::Port { block, far, outgoing }, key, (l.from, l.to));
            }
        }
    }
    out.sort_by_key(|(id, _, _)| match id {
        PipeId::Trunk(..) => (0, *id),
        PipeId::Port { .. } => (1, *id),
    });
    out
}

// --- Meshes --------------------------------------------------------------------------------

/// The pipe's centre line in 3D, with radii: corners rounded, sampled evenly, bridges
/// lifted over the pipes they cross, and cut to `grow` of its length.
fn centre_line(route: &[Waypoint], bridges: &[Waypoint], geometry: &Geometry, heights: &Heights, grow: f32) -> Vec<(Vec3, f32)> {
    let pt = |w: &Waypoint| {
        let r = PIPE_R * geometry.surface_scale(w.surface);
        (Vec3::new(w.p.x, heights.top(w.surface) + r * 1.3, w.p.y), r)
    };
    let mut pts: Vec<(Vec3, f32)> = route.iter().map(pt).collect();
    pts.dedup_by(|a, b| a.0.distance(b.0) < 1e-4);
    if pts.len() < 2 {
        return pts;
    }
    // Round the corners.
    let mut round = vec![pts[0]];
    for i in 1..pts.len() - 1 {
        let (p, r) = pts[i];
        let a = pts[i - 1].0;
        let b = pts[i + 1].0;
        let (la, lb) = (p.distance(a), p.distance(b));
        let d = (r * 3.0).min(la * 0.45).min(lb * 0.45);
        if d < 1e-4 {
            round.push(pts[i]);
            continue;
        }
        let s = p + (a - p).normalize() * d;
        let e = p + (b - p).normalize() * d;
        for k in 0..=6 {
            let t = k as f32 / 6.0;
            let q = s * (1.0 - t) * (1.0 - t) + p * 2.0 * t * (1.0 - t) + e * t * t;
            round.push((q, r));
        }
    }
    round.push(*pts.last().unwrap());
    // Resample evenly.
    let mut dense = vec![round[0]];
    for w in round.windows(2) {
        let ((a, ra), (b, rb)) = (w[0], w[1]);
        let len = a.distance(b);
        let step = ra.min(rb) * 0.8;
        let n = (len / step).ceil().max(1.0) as usize;
        for k in 1..=n {
            let t = k as f32 / n as f32;
            dense.push((a.lerp(b, t), ra + (rb - ra) * t));
        }
    }
    // Bridges: lift over pipes crossed on the same surface.
    let lifts: Vec<(Vec3, f32)> = bridges.iter().map(pt).collect();
    for (p, _) in dense.iter_mut() {
        let mut lift = 0.0f32;
        for (b, rb) in &lifts {
            if (p.y - b.y).abs() > rb * 0.6 {
                continue;
            }
            let d = Vec2::new(p.x - b.x, p.z - b.z).length() / (rb * 6.0);
            if d < 1.0 {
                let t = 1.0 - d;
                lift = lift.max(rb * 2.8 * t * t * (3.0 - 2.0 * t));
            }
        }
        p.y += lift;
    }
    // Grow along the length.
    let total: f32 = dense.windows(2).map(|w| w[0].0.distance(w[1].0)).sum();
    let keep = total * grow.clamp(0.0, 1.0);
    let mut out = vec![dense[0]];
    let mut acc = 0.0;
    for w in dense.windows(2) {
        let l = w[0].0.distance(w[1].0);
        if acc + l >= keep {
            let t = ((keep - acc) / l.max(1e-6)).clamp(0.0, 1.0);
            out.push((w[0].0.lerp(w[1].0, t), w[0].1));
            break;
        }
        acc += l;
        out.push(w[1]);
    }
    out
}

/// A tube along `line`, with `uv.x` measured in radii along it.
/// For the blocky style, a flat strip lying on its surface instead, like a trail of dust.
fn tube(lines: &[Vec<(Vec3, f32)>], square: bool) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    let sides = if square { 4 } else { 10 };
    let turn = if square { std::f32::consts::FRAC_PI_4 } else { 0.0 };
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut nrm: Vec<[f32; 3]> = Vec::new();
    let mut uv: Vec<[f32; 2]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    for line in lines.iter().filter(|l| l.len() >= 2) {
        let base = pos.len() as u32;
        let mut normal = Vec3::ZERO;
        let mut along = 0.0;
        for i in 0..line.len() {
            let (p, r) = line[i];
            let a = line[i.saturating_sub(1)].0;
            let b = line[(i + 1).min(line.len() - 1)].0;
            let t = (b - a).normalize_or(Vec3::X);
            // Carry the frame along the curve so the tube doesn't twist.
            if i == 0 {
                normal = t.cross(Vec3::Y).normalize_or(Vec3::X);
            } else {
                along += p.distance(line[i - 1].0) / r.max(1e-6);
                normal = (normal - t * t.dot(normal)).normalize_or(t.any_orthonormal_vector());
            }
            let binormal = t.cross(normal);
            for k in 0..=sides {
                let ang = k as f32 / sides as f32 * std::f32::consts::TAU + turn;
                let o = normal * ang.cos() + binormal * ang.sin();
                let at = if square {
                    // Squashed toward the surface (the binormal points down on level runs).
                    let flat = normal * ang.cos() + binormal * ang.sin() * DUST_THIN;
                    p + binormal * (1.0 - DUST_THIN) * r + flat * r * std::f32::consts::SQRT_2
                } else {
                    p + o * r
                };
                pos.push(at.to_array());
                // Flat faces for the square tube: the normal of the face this corner leads.
                let fo = if square {
                    let fa = ang + std::f32::consts::PI / sides as f32;
                    normal * fa.cos() + binormal * fa.sin()
                } else {
                    o
                };
                nrm.push(fo.to_array());
                uv.push([along, k as f32 / sides as f32]);
            }
        }
        let ring = sides as u32 + 1;
        for i in 0..line.len() as u32 - 1 {
            for k in 0..sides as u32 {
                let a = base + i * ring + k;
                let b = a + ring;
                idx.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
            }
        }
    }
    if pos.is_empty() {
        pos = vec![[0.0; 3]; 3];
        nrm = vec![[0.0, 1.0, 0.0]; 3];
        uv = vec![[0.0; 2]; 3];
        idx = vec![0, 1, 2];
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_indices(Indices::U32(idx));
    mesh
}

/// Port models: a jack with a glowing ring, or (blocky) a torch: a stick with a glowing head.
/// Materials are per style, in `Style` order.
#[derive(Resource)]
pub struct PortAssets {
    body: Handle<Mesh>,
    ring: Handle<Mesh>,
    cube: Handle<Mesh>,
    cap: Handle<Mesh>,
    body_mat: [Handle<StandardMaterial>; 4],
    ring_mat: [Handle<StandardMaterial>; 4],
}

pub fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>) {
    let flat = |c: Color| StandardMaterial {
        base_color: c,
        unlit: true,
        ..default()
    };
    commands.insert_resource(PortAssets {
        body: meshes.add(Cylinder::new(1.0, 1.0).mesh().resolution(20)),
        ring: meshes.add(Torus::new(0.62, 0.92).mesh().major_resolution(24).minor_resolution(8)),
        cube: meshes.add(Cuboid::new(0.32, 1.0, 0.32)),
        cap: meshes.add(Cuboid::new(0.44, 0.34, 0.44)),
        body_mat: [
            mats.add(StandardMaterial {
                base_color: Color::srgb(0.02, 0.04, 0.06),
                emissive: LinearRgba::rgb(0.05, 0.35, 0.5),
                perceptual_roughness: 0.4,
                ..default()
            }),
            mats.add(flat(Color::srgb(0.97, 0.95, 0.9))),
            mats.add(flat(Color::srgb(0.47, 0.34, 0.19))),
            mats.add(flat(Style::Blueprint.background())),
        ],
        ring_mat: [
            mats.add(StandardMaterial {
                base_color: Color::BLACK,
                emissive: LinearRgba::rgb(1.2, 7.0, 9.0),
                ..default()
            }),
            mats.add(flat(Color::srgb(0.1, 0.11, 0.18))),
            mats.add(flat(Color::srgb(1.0, 0.22, 0.1))),
            mats.add(flat(Color::srgb(0.95, 0.97, 1.0))),
        ],
    });
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!("{}…", s.chars().take(max).collect::<String>().trim_end())
}

/// A routing grid for each surface in view (the floor and every open platform), with every
/// pipe already planned on it put back first, so new pipes fit round them.
fn router<'a>(model: &'a Model, geometry: &'a Geometry, view: &View, placed: &HashMap<PipeId, Placed>) -> Router<'a> {
    let mut surfs = HashMap::new();
    let mut surfaces: Vec<Option<Id>> = vec![None];
    surfaces.extend(view.opened.iter().map(|x| Some(*x)));
    for s in surfaces {
        let items: Vec<Rect> = model
            .children_of(s)
            .iter()
            .filter_map(|c| geometry.geos.get(c).map(|g| g.rect()))
            .collect();
        let cell = CELL * geometry.surface_scale(s);
        let bounds = match s {
            None => {
                let b = items.iter().copied().reduce(|a, b| a.union(b)).unwrap_or(geometry.root_bounds);
                Rect::from_center_size(b.center(), b.size() + Vec2::splat(cell * 28.0))
            }
            // Keep pipes off a platform's rim.
            Some(x) => {
                let r = geometry.geos[&x].rect();
                Rect::from_center_size(r.center(), r.size() - Vec2::splat(cell * 2.0))
            }
        };
        surfs.insert(s, Surf::new(s, bounds, cell, &items));
    }
    let mut router = Router {
        model,
        geometry,
        surfs,
        taken: HashSet::new(),
        new_spots: Vec::new(),
        reserved: HashMap::new(),
    };
    let mut earlier: Vec<&Placed> = placed.values().filter(|p| router.surfs.contains_key(&p.surface)).collect();
    earlier.sort_by_key(|p| p.seq);
    for p in earlier {
        router.surfs.get_mut(&p.surface).unwrap().claim(&p.cells);
        router.taken.extend(p.spots.iter().copied());
        for r in &p.reserved {
            router.reserve(p.surface, *r);
        }
    }
    router
}

/// Paint a tag, returning its image and size in layout units.
fn paint_tag(painter: &mut Painter, images: &mut Assets<Image>, spans: &[crate::paint::Span]) -> Option<(Handle<Image>, Vec2)> {
    let painted = painter.paint(spans, 600.0, false)?;
    let size = painted.size;
    Some((images.add(painted.image), size))
}

/// Plan new pipes when the view changes (keeping every pipe already planned exactly where
/// it is), grow and retract them, and follow rising platforms.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    time: Res<Time>,
    diagram: Res<Diagram>,
    view: Res<View>,
    geometry: Res<Geometry>,
    heights: Res<Heights>,
    (playback, theme): (Res<crate::story::Playback>, Res<Theme>),
    (mut painter, mut images, quad, port_assets): (ResMut<Painter>, ResMut<Assets<Image>>, Res<DecalQuad>, Res<PortAssets>),
    mut state: ResMut<PipeState>,
    mut current: ResMut<Links>,
    mut pipes: Query<(Entity, &mut Pipe)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<PipeMaterial>>,
    mut decal_mats: ResMut<Assets<DecalMaterial>>,
    mut decals: Query<(&mut Transform, &mut Visibility), Without<Pipe>>,
) {
    let dt = time.delta_secs().min(0.05);
    let model = &diagram.model;
    let despawn = |commands: &mut Commands, e: Entity, p: &Pipe| {
        if let Some((t, ..)) = &p.tag {
            commands.entity(*t).despawn();
        }
        if let Some((b, ..)) = &p.port {
            commands.entity(*b).despawn();
        }
        commands.entity(e).despawn();
    };

    if state.resets != view.resets {
        for (e, p) in &pipes {
            despawn(&mut commands, e, &p);
        }
        state.resets = view.resets;
        state.version = u64::MAX;
        state.instant = view.instant_resets == view.resets;
        state.placed.clear();
        // The old pipes are still in the query until the despawns apply: plan afresh next
        // frame rather than touching them again.
        return;
    }
    if state.version != view.version || state.story != playback.version {
        state.version = view.version;
        state.story = playback.version;
        let extra: Vec<(Id, Id, String)> = playback
            .active
            .and_then(|i| model.views.get(i))
            .map(|v| v.steps.iter().map(|s| (s.from, s.to, s.title.clone())).collect())
            .unwrap_or_default();
        let ls = links(model, &view.visible, &extra);
        let instant = std::mem::take(&mut state.instant);
        let need = needed(model, &ls);
        let ink = painter.style.tag().0;

        // Plan whatever is new, fitting round everything already planned.
        let mut router = router(model, &geometry, &view, &state.placed);
        for (id, _, _) in &need {
            if state.placed.contains_key(id) {
                continue;
            }
            let mut placed = match *id {
                PipeId::Trunk(surf, a, b) => {
                    if !router.surfs.contains_key(&surf) {
                        continue;
                    }
                    let mut p = router.trunk(surf, a, b);
                    let label = trunk_label(model, a, b, &extra);
                    let painted = (!label.is_empty())
                        .then(|| paint_tag(&mut painter, &mut images, &[span(format!(" {label} "), 12.5, 500, ink)]))
                        .flatten();
                    if let Some((img, size)) = painted {
                        let size = size * geometry.surface_scale(surf);
                        if let Some(at) = router.trunk_tag(&p, size) {
                            let r = Rect::from_center_size(at, size);
                            router.reserve(surf, r);
                            p.reserved.push(r);
                            p.tag = Some((img, size, at));
                        }
                    }
                    p
                }
                PipeId::Port { block, far, outgoing } => {
                    let surf = model.elements[block].parent;
                    if !router.surfs.contains_key(&surf) {
                        continue;
                    }
                    // The painted font has no arrows, so say it in words.
                    let dir = if outgoing { "to" } else { "from" };
                    let spans = [
                        span(format!("  {dir}  "), 9.5, 400, ink.with_alpha(0.7)),
                        span(format!("{}  ", model.elements[far].title), 11.0, 600, ink),
                    ];
                    let painted = paint_tag(&mut painter, &mut images, &spans);
                    let k = geometry.surface_scale(surf);
                    let toward = geometry.geos.get(&far).map_or(Vec2::ZERO, |g| g.center);
                    let mut p = router.port(block, toward, painted.as_ref().map(|(_, s)| *s * k));
                    p.tag = match (p.tag.take(), painted) {
                        (Some((_, _, at)), Some((img, size))) => Some((img, size * k, at)),
                        _ => None,
                    };
                    p
                }
            };
            placed.seq = state.seq;
            state.seq += 1;
            state.placed.insert(*id, placed);
        }

        // Keep the pipes that are still wanted untouched; retire the rest; add the new.
        let wanted: HashMap<PipeId, &Vec<(Id, Id)>> = need.iter().map(|(id, _, l)| (*id, l)).collect();
        let mut kept = HashSet::new();
        for (_, mut p) in &mut pipes {
            match wanted.get(&p.id) {
                Some(links) if !p.dying => {
                    p.links = (*links).clone();
                    kept.insert(p.id);
                }
                _ => {
                    p.dying = true;
                    p.grow.target = 0.0;
                    p.delay = 0.0;
                }
            }
        }
        let st = theme.style.index() as usize;
        for (id, key, links) in &need {
            if kept.contains(id) {
                continue;
            }
            let Some(placed) = state.placed.get(id) else { continue };
            let material = mats.add(PipeMaterial {
                u: PipeUniform::default(),
            });
            let mesh = meshes.add(tube(&[], false));
            let port = placed.port.map(|at| {
                let (body_mesh, cap_mesh) = if theme.style == Style::Blocky {
                    (port_assets.cube.clone(), port_assets.cap.clone())
                } else {
                    (port_assets.body.clone(), port_assets.ring.clone())
                };
                let body = commands
                    .spawn((Transform::default(), Visibility::Hidden))
                    .with_children(|c| {
                        c.spawn((
                            Mesh3d(body_mesh),
                            MeshMaterial3d(port_assets.body_mat[st].clone()),
                            Transform::from_xyz(0.0, 0.5, 0.0),
                        ));
                        c.spawn((
                            Mesh3d(cap_mesh),
                            MeshMaterial3d(port_assets.ring_mat[st].clone()),
                            Transform::from_xyz(0.0, 1.0, 0.0),
                        ));
                    })
                    .id();
                (body, at, placed.surface)
            });
            let tag = placed.tag.as_ref().map(|(img, size, at)| {
                let (e, m) = decal_with(&mut commands, &mut decal_mats, &quad, img.clone(), *size, theme.style, true);
                (e, m, *size, *at, placed.surface)
            });
            let mut grow = Spring::new(if instant { 1.0 } else { 0.0 });
            grow.target = 1.0;
            let delay = if instant {
                0.0
            } else if view.version <= 2 {
                0.9
            } else {
                // Ports pop up after the platform has risen.
                0.3 + if matches!(id, PipeId::Port { .. }) { 0.25 } else { 0.0 }
            };
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
                Pipe {
                    key: *key,
                    from: key.0,
                    to: key.1,
                    links: links.clone(),
                    id: *id,
                    piece: placed.piece.clone(),
                    bridges: placed.bridges.clone(),
                    port,
                    tag,
                    mesh,
                    material,
                    hl: Spring::new(0.0),
                    dim: Spring::new(0.0),
                    grow,
                    delay,
                    dying: false,
                    built: -1.0,
                    samples: Vec::new(),
                },
            ));
        }
        current.0 = ls;
    }

    let detail_pair = match diagram.detail {
        Some(Detail::Relation(a, b)) => Some((a, b)),
        _ => None,
    };
    for (e, mut p) in &mut pipes {
        if p.delay > 0.0 {
            p.delay -= dt;
        } else {
            p.grow.step(dt, 7.0, 1.0);
        }
        p.hl.step(dt, 16.0, 1.0);
        p.dim.step(dt, 14.0, 1.0);
        if detail_pair == Some(p.key) {
            p.hl.target = 1.0;
        }
        let grow = p.grow.value.clamp(0.0, 1.0);
        if p.dying && grow < 0.01 {
            despawn(&mut commands, e, &p);
            continue;
        }
        if heights.moving || (grow - p.built).abs() > 1e-4 {
            let line = centre_line(&p.piece, &p.bridges, &geometry, &heights, grow);
            if let Some(m) = meshes.get_mut(&p.mesh) {
                *m = tube(&[line.clone()], theme.style == Style::Blocky);
            }
            p.samples = line.iter().map(|(q, _)| *q).collect();
            p.built = grow;
        }
        let color = neon(model.elements[p.from].color).lerp(Vec3::new(0.05, 0.85, 1.0), 0.55);
        if let Some(m) = mats.get_mut(&p.material) {
            m.u = PipeUniform {
                color: color.extend(2.2),
                p: Vec4::new(p.hl.value.clamp(0.0, 1.0), p.dim.value.clamp(0.0, 1.0), 16.0, 9.0),
                q: Vec4::new(theme.style.index(), 0.0, 0.0, 0.0),
            };
        }
        let hl = p.hl.value.clamp(0.0, 1.0);
        let dim = p.dim.value.clamp(0.0, 1.0);

        // The port pops up out of its platform as its pipe arrives.
        if let Some((body, at, surf)) = p.port {
            let r = PIPE_R * geometry.surface_scale(surf);
            let pop = (grow * 1.4 - 0.2).clamp(0.0, 1.0);
            if let Ok((mut tf, mut vis)) = decals.get_mut(body) {
                tf.translation = Vec3::new(at.x, heights.top(surf), at.y);
                tf.scale = Vec3::new(r * 2.3, r * 2.9 * pop.max(1e-3), r * 2.3);
                *vis = if pop > 0.01 { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
        // The tag, painted where it was planned to lie.
        if let Some((t, m, size, at, surf)) = &p.tag {
            let k = geometry.surface_scale(*surf);
            let lift = if p.is_trunk() { PIPE_R * k * 2.5 } else { PIPE_R * k * 0.3 };
            let alpha = ((grow - 0.9) / 0.1).clamp(0.0, 1.0) * (1.0 - 0.7 * dim);
            if let Ok((mut tf, mut vis)) = decals.get_mut(*t) {
                tf.translation = Vec3::new(at.x, heights.top(*surf) + lift, at.y);
                tf.scale = Vec3::new(size.x, 1.0, size.y);
                *vis = if alpha > 0.01 { Visibility::Inherited } else { Visibility::Hidden };
            }
            if let Some(mat) = decal_mats.get_mut(m) {
                let t = theme.style.text_tint() * 0.9 + if theme.style == Style::Tron { 0.6 * hl } else { 0.0 };
                mat.u.tint = Vec4::new(t, t, t * 1.08, alpha);
                mat.u.p.y = 0.45 + 0.55 * hl;
            }
        }
    }
}

/// The pipe nearest the cursor on screen, if within a few pixels.
pub fn pick(cam: &Cam, pipes: &Query<(Entity, &mut Pipe)>, screen: Vec2) -> Option<(Id, Id)> {
    let mut best: Option<(f32, (Id, Id))> = None;
    for (_, p) in pipes.iter() {
        if p.dying || p.built < 0.9 {
            continue;
        }
        let pts: Vec<Vec2> = p.samples.iter().step_by(2).filter_map(|q| cam.world_to_screen(*q)).collect();
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let ab = b - a;
            let t = ((screen - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            let d = screen.distance(a + ab * t);
            if d < 7.0 && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, p.key));
            }
        }
    }
    best.map(|(_, k)| k)
}
