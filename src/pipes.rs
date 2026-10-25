//! Relationships as pipes. Each one is routed once per view, at right angles like a circuit
//! trace, across the surface its ends stand on: along a platform's top, over its edge and
//! down its side to the level below, and so on. Routing avoids blocks and prefers not to
//! cross other pipes; where it has to, the later pipe arches over as a little bridge. The
//! pipes never move by themselves; they only follow platforms rising and sinking.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::RenderLayers,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
    sprite::Anchor,
    text::TextLayoutInfo,
};

use crate::camera::Cam;
use crate::model::{Id, Model};
use crate::render::{lin, neon, CardMaterial, CardUniform, PipeMaterial, PipeUniform, Spring};
use crate::scene::{Detail, Diagram, Fonts, SceneAssets, MUTED, OVERLAY, TEXT};
use crate::world::{Geometry, Heights, View};

/// Pipe radius, in layout units of the level it runs on.
const PIPE_R: f32 = 3.6;
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

#[derive(Component)]
pub struct Pipe {
    pub key: (Id, Id),
    pub from: Id,
    pub to: Id,
    route: Vec<Waypoint>,
    bridges: Vec<Waypoint>,
    mesh: Handle<Mesh>,
    material: Handle<PipeMaterial>,
    pub hl: Spring<f32>,
    pub dim: Spring<f32>,
    grow: Spring<f32>,
    delay: f32,
    pub dying: bool,
    /// Whether it belongs to the open level (bright) or the context around it (dim).
    pub focus: bool,
    built: f32,
    /// The centre line as last built, for picking and the label.
    pub samples: Vec<Vec3>,
    label: Option<(Entity, Entity, Handle<CardMaterial>)>,
    label_at: Option<Vec3>,
    label_scale: f32,
}

#[derive(Resource, Default)]
pub struct PipeState {
    version: u64,
    resets: u64,
}

/// The ancestor-or-self of `x` that is visible.
fn visible_rep(model: &Model, visible: &HashSet<Id>, mut x: Id) -> Option<Id> {
    loop {
        if visible.contains(&x) {
            return Some(x);
        }
        x = model.elements[x].parent?;
    }
}

pub fn links(model: &Model, visible: &HashSet<Id>) -> Vec<Link> {
    let mut map: HashMap<(Id, Id), Link> = HashMap::new();
    let mut order = Vec::new();
    for r in &model.relations {
        let (Some(a), Some(b)) = (visible_rep(model, visible, r.source), visible_rep(model, visible, r.target)) else {
            continue;
        };
        if model.is_ancestor_or_self(a, b) || model.is_ancestor_or_self(b, a) {
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
            l.label = r.title.clone();
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
    /// Places along block and platform sides already taken: (owner, side, cell).
    taken: HashSet<(Id, usize, i64)>,
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

    fn path(&mut self, surf: Option<Id>, a: (usize, usize), ad: usize, b: (usize, usize), bd: usize, bridges: &mut Vec<Waypoint>) -> Vec<Waypoint> {
        let s = self.surfs.get_mut(&surf).unwrap();
        let cells = s.astar(a, ad, b, bd).unwrap_or_else(|| {
            // No clear way: an L, straight through.
            vec![a, (b.0, a.1), b]
        });
        for c in s.claim(&cells) {
            bridges.push(Waypoint { surface: surf, p: c });
        }
        corners(s, &cells)
    }

    /// Route from `a` out to the surface `common`, leaving each platform on the way by the
    /// side facing `toward`. Returns the waypoints, the cell reached on `common` and the
    /// heading there.
    fn climb_out(
        &mut self,
        a: Id,
        toward: Vec2,
        common: Option<Id>,
        bridges: &mut Vec<Waypoint>,
    ) -> (Vec<Waypoint>, (usize, usize), usize) {
        let g = self.geometry.geos[&a];
        let mut surf = self.model.elements[a].parent;
        let side = pick_side(g.rect(), toward);
        let wall = self.spot(a, g.rect(), side, toward, surf);
        let r = PIPE_R * g.scale;
        let mut out = vec![Waypoint {
            surface: surf,
            p: wall - side * r * 1.5,
        }];
        let mut cell = self.out_cell(surf, wall + side * 0.01, side);
        let mut heading = dir_index(side);
        out.push(Waypoint {
            surface: surf,
            p: self.surfs[&surf].center(cell.0, cell.1),
        });
        while surf != common {
            let Some(x) = surf else { break };
            let parent = self.model.elements[x].parent;
            let xg = self.geometry.geos[&x];
            let xside = pick_side(xg.rect(), toward);
            let edge = self.spot(x, xg.rect(), xside, toward, parent);
            // Last cell on the platform before its edge.
            let inner = {
                let s = &self.surfs[&surf];
                s.cell_of(edge - xside * s.cell * 0.6)
            };
            out.extend(self.path(surf, cell, heading, inner, dir_index(xside), bridges));
            // Over the edge and straight down the wall, then out to the first free cell.
            let ps = &self.surfs[&parent];
            let hug = edge + xside * PIPE_R * self.geometry.surface_scale(parent) * 2.2;
            let below = ps.cell_of(edge + xside * ps.cell * (CLEAR + 1.6));
            out.push(Waypoint { surface: surf, p: edge });
            out.push(Waypoint { surface: surf, p: hug });
            out.push(Waypoint { surface: parent, p: hug });
            surf = parent;
            cell = below;
            heading = dir_index(xside);
        }
        (out, cell, heading)
    }

    fn route(&mut self, link: &Link) -> (Vec<Waypoint>, Vec<Waypoint>) {
        let model = self.model;
        let ga = self.geometry.geos[&link.from];
        let gb = self.geometry.geos[&link.to];
        let surfaces_a: Vec<Option<Id>> = std::iter::successors(Some(model.elements[link.from].parent), |s| s.map(|x| model.elements[x].parent)).collect();
        let surfaces_b: Vec<Option<Id>> = std::iter::successors(Some(model.elements[link.to].parent), |s| s.map(|x| model.elements[x].parent)).collect();
        let common = surfaces_a.iter().find(|s| surfaces_b.contains(s)).copied().flatten();
        let mut bridges = Vec::new();
        let (mut wa, ca, da) = self.climb_out(link.from, gb.center, common, &mut bridges);
        let (wb, cb, db) = self.climb_out(link.to, ga.center, common, &mut bridges);
        let mid = self.path(common, ca, da, cb, db ^ 1, &mut bridges);
        wa.extend(mid);
        wa.extend(wb.into_iter().rev());
        wa.dedup();
        (wa, bridges)
    }
}

fn route_all(model: &Model, geometry: &Geometry, view: &View, heights_hint: &[Link]) -> Vec<(Vec<Waypoint>, Vec<Waypoint>)> {
    // A routing grid per surface: the floor, and the top of each opened platform.
    let mut surfs = HashMap::new();
    let mut surfaces: Vec<Option<Id>> = vec![None];
    surfaces.extend(view.opened.iter().map(|x| Some(*x)));
    for s in surfaces {
        let items: Vec<Rect> = model
            .children_of(s)
            .iter()
            .filter_map(|c| geometry.geos.get(c).map(|g| g.rect()))
            .collect();
        let scale = geometry.surface_scale(s);
        let cell = CELL * scale;
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
    };
    // Short links first, so they get the straightest runs.
    let mut order: Vec<usize> = (0..heights_hint.len()).collect();
    order.sort_by(|&a, &b| {
        let len = |l: &Link| {
            let (p, q) = (geometry.geos[&l.from].center, geometry.geos[&l.to].center);
            (p - q).abs().element_sum()
        };
        len(&heights_hint[a]).total_cmp(&len(&heights_hint[b]))
    });
    let mut out = vec![(Vec::new(), Vec::new()); heights_hint.len()];
    for i in order {
        out[i] = router.route(&heights_hint[i]);
    }
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
fn tube(line: &[(Vec3, f32)]) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    let sides = 10;
    let mut pos = Vec::new();
    let mut nrm = Vec::new();
    let mut uv = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    if line.len() >= 2 {
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
                let ang = k as f32 / sides as f32 * std::f32::consts::TAU;
                let o = normal * ang.cos() + binormal * ang.sin();
                pos.push((p + o * r).to_array());
                nrm.push(o.to_array());
                uv.push([along, k as f32 / sides as f32]);
            }
        }
        let ring = sides as u32 + 1;
        for i in 0..line.len() as u32 - 1 {
            for k in 0..sides as u32 {
                let a = i * ring + k;
                let b = a + ring;
                idx.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
            }
        }
    } else {
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

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!("{}…", s.chars().take(max).collect::<String>().trim_end())
}

/// Route pipes when the view changes, grow and retract them, follow rising platforms, and
/// keep their labels in place.
#[allow(clippy::too_many_arguments)]
pub fn update(
    mut commands: Commands,
    time: Res<Time>,
    diagram: Res<Diagram>,
    view: Res<View>,
    geometry: Res<Geometry>,
    heights: Res<Heights>,
    cam: Res<Cam>,
    (fonts, assets): (Res<Fonts>, Res<SceneAssets>),
    mut state: ResMut<PipeState>,
    mut current: ResMut<Links>,
    mut pipes: Query<(Entity, &mut Pipe)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<PipeMaterial>>,
    mut cards: ResMut<Assets<CardMaterial>>,
    mut texts: Query<(&mut Transform, &mut TextColor, &TextLayoutInfo, &mut Visibility)>,
    mut bgs: Query<(&mut Transform, &mut Visibility), Without<TextColor>>,
) {
    let dt = time.delta_secs().min(0.05);
    let model = &diagram.model;

    if state.resets != view.resets {
        for (e, p) in &pipes {
            if let Some((t, b, _)) = p.label {
                commands.entity(t).despawn();
                commands.entity(b).despawn();
            }
            commands.entity(e).despawn();
        }
        state.resets = view.resets;
        state.version = u64::MAX;
    }
    if state.version != view.version {
        state.version = view.version;
        let ls = links(model, &view.visible);
        let routes = route_all(model, &geometry, &view, &ls);
        let focus_level: HashSet<Id> = model.children_of(view.focus).iter().copied().collect();
        let mut kept = HashSet::new();
        for (_, mut p) in &mut pipes {
            if let Some(i) = ls.iter().position(|l| (l.from, l.to) == p.key) {
                if routes[i].0 == p.route && !p.dying {
                    kept.insert(i);
                    p.focus = focus_level.contains(&p.from) || focus_level.contains(&p.to);
                    p.bridges = routes[i].1.clone();
                    continue;
                }
            }
            p.dying = true;
            p.grow.target = 0.0;
            p.delay = 0.0;
        }
        for (i, l) in ls.iter().enumerate() {
            if kept.contains(&i) {
                continue;
            }
            let material = mats.add(PipeMaterial {
                u: PipeUniform::default(),
            });
            let mesh = meshes.add(tube(&[]));
            let mut grow = Spring::new(0.0);
            grow.target = 1.0;
            let label = (!l.label.is_empty() || l.count > 1).then(|| {
                let text = match (l.label.is_empty(), l.count) {
                    (true, n) => format!("{n} relationships"),
                    (false, 1) => truncate(&l.label, 40),
                    (false, n) => format!("{}  +{}", truncate(&l.label, 34), n - 1),
                };
                let t = commands
                    .spawn((
                        Text2d::new(text),
                        TextFont {
                            font: fonts.ui.clone(),
                            font_size: 13.0,
                            weight: FontWeight::MEDIUM,
                            ..default()
                        },
                        TextColor(MUTED.with_alpha(0.0)),
                        Anchor::CENTER,
                        Transform::from_xyz(0.0, 0.0, 3.1),
                        Visibility::Hidden,
                        RenderLayers::layer(OVERLAY),
                    ))
                    .id();
                let m = cards.add(CardMaterial {
                    u: CardUniform::default(),
                });
                let b = commands
                    .spawn((
                        Mesh2d(assets.quad.clone()),
                        MeshMaterial2d(m.clone()),
                        Transform::from_xyz(0.0, 0.0, 3.0),
                        Visibility::Hidden,
                        RenderLayers::layer(OVERLAY),
                    ))
                    .id();
                (t, b, m)
            });
            commands.spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
                Pipe {
                    key: (l.from, l.to),
                    from: l.from,
                    to: l.to,
                    route: routes[i].0.clone(),
                    bridges: routes[i].1.clone(),
                    mesh,
                    material,
                    hl: Spring::new(0.0),
                    dim: Spring::new(0.0),
                    grow,
                    delay: if diagram.view_version <= 1 { 0.9 } else { 0.45 },
                    dying: false,
                    focus: focus_level.contains(&l.from) || focus_level.contains(&l.to),
                    built: -1.0,
                    samples: Vec::new(),
                    label,
                    label_at: None,
                    label_scale: 1.0,
                },
            ));
        }
        current.0 = ls;
    }

    let detail_pair = match diagram.detail {
        Some(Detail::Relation(a, b)) => Some((a, b)),
        _ => None,
    };
    let two_way: HashSet<(Id, Id)> = current.0.iter().map(|l| (l.from, l.to)).collect();
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
            if let Some((t, b, _)) = p.label {
                commands.entity(t).despawn();
                commands.entity(b).despawn();
            }
            commands.entity(e).despawn();
            continue;
        }
        if heights.moving || (grow - p.built).abs() > 1e-4 {
            let line = centre_line(&p.route, &p.bridges, &geometry, &heights, grow);
            if let Some(m) = meshes.get_mut(&p.mesh) {
                *m = tube(&line);
            }
            p.samples = line.iter().map(|(q, _)| *q).collect();
            p.built = grow;
            // Hang the label on the longest straight run, preferring the open level. Of a
            // two-way pair, each sits toward its own end so the two don't overlap.
            let mut best: Option<(f32, Vec3, f32)> = None;
            let along = if two_way.contains(&(p.to, p.from)) {
                if p.from < p.to { 0.3 } else { 0.7 }
            } else {
                0.5
            };
            for w in p.route.windows(2) {
                if w[0].surface != w[1].surface {
                    continue;
                }
                let bonus = if w[0].surface == view.focus { 4.0 } else { 1.0 };
                let len = w[0].p.distance(w[1].p) * bonus;
                if best.is_none_or(|(l, _, _)| len > l) {
                    let s = geometry.surface_scale(w[0].surface);
                    let mid = w[0].p.lerp(w[1].p, along);
                    let y = heights.top(w[0].surface) + PIPE_R * s * 1.3;
                    best = Some((len, Vec3::new(mid.x, y, mid.y), s));
                }
            }
            p.label_at = best.map(|(_, at, _)| at);
            p.label_scale = best.map_or(1.0, |(_, _, s)| s);
        }
        let from = &model.elements[p.from];
        let color = neon(from.color).lerp(Vec3::new(0.05, 0.85, 1.0), 0.55);
        let glow = if p.focus { 2.4 } else { 0.9 };
        if let Some(m) = mats.get_mut(&p.material) {
            m.u = PipeUniform {
                color: color.extend(glow),
                p: Vec4::new(p.hl.value.clamp(0.0, 1.0), p.dim.value.clamp(0.0, 1.0), 16.0, 9.0),
            };
        }

        // Label, on the overlay.
        if let Some((t, b, m)) = &p.label {
            let at = p.label_at.and_then(|a| cam.world_to_overlay(a).map(|o| (o, cam.px_per_unit(a))));
            let hl = p.hl.value.clamp(0.0, 1.0);
            let size_px = at.map_or(0.0, |(_, ppu)| 12.5 * p.label_scale * ppu).min(15.0);
            let show = (p.focus || hl > 0.1) && size_px > 7.5 && grow > 0.95;
            let alpha = if show {
                ((size_px - 7.5) / 2.0).clamp(0.0, 1.0) * (1.0 - 0.75 * p.dim.value.clamp(0.0, 1.0))
            } else {
                0.0
            };
            if let Ok((mut tf, mut tc, info, mut vis)) = texts.get_mut(*t) {
                *vis = if alpha > 0.01 { Visibility::Inherited } else { Visibility::Hidden };
                if let Some((o, _)) = at {
                    let k = size_px / 13.0;
                    tf.translation = o.extend(3.1);
                    tf.scale = Vec3::splat(k);
                    tc.0 = MUTED.mix(&TEXT, hl).with_alpha(0.9 * alpha);
                    let size = info.size * k + Vec2::new(16.0, 8.0) * k;
                    if let Ok((mut bt, mut bv)) = bgs.get_mut(*b) {
                        *bv = *vis;
                        bt.translation = o.extend(3.0);
                        bt.scale = (size + Vec2::splat(8.0)).extend(1.0);
                    }
                    if let Some(mat) = cards.get_mut(m) {
                        mat.u = CardUniform {
                            fill: lin(Color::srgba(0.01, 0.03, 0.05, 0.92)),
                            border: lin(Color::srgb(0.1, 0.85, 1.0).with_alpha(0.25 + 0.5 * hl)),
                            glow: Vec4::ZERO,
                            dims: Vec4::new(size.x, size.y, size.y * 0.5, 1.0),
                            p: Vec4::new(alpha, 0.0, 0.0, 0.0),
                            q: Vec4::new(4.0, 0.0, 0.0, 0.0),
                        };
                    }
                }
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
