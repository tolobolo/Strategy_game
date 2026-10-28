//! World building as text. Every stage is a `Grid` of letters, one per tile, so a map can be
//! printed (`cargo run -- map <seed>`), tweaked by changing the constants below, and saved as a
//! seed file (`assets/seeds/<seed>.txt`) that the game loads. See agents.md §1.6.
//!
//! Letters: `W` water (layer 0), `G` grass (1), `M` mountain before layering, `2`..`5` plateau
//! layers, `R` a ramp (on the layer of the tile below it, climbing to its east or west
//! neighbour). The stages here make no ramps; `TerrainMap::ensure_ramps` adds and checks them.

use std::collections::{HashMap, HashSet, VecDeque};

use bevy::math::IVec2;

use super::terrain::Noise;
use rand::rngs::StdRng;
use rand::seq::{IndexedRandom, SliceRandom};
use rand::{Rng, SeedableRng};

/// The map is built from patches this big: one tile more than a base in each direction, so a
/// base always fits inside one flat patch.
pub const PATCH: IVec2 = IVec2::new(6, 5);
/// Highest plateau layer. Water is 0, grass 1, mountains 2..=MAX_LAYER.
pub const MAX_LAYER: u8 = 5;

// --- stage 1, water: grows in 2D from a random corner with a run counter and a cooldown ---
pub const WATER_BASE: f32 = 0.18;
/// Added by every water patch to the chance of the next patch in its row...
pub const WATER_SPREAD: f32 = 0.45;
/// ...and (more, so lakes join up across rows) to the patch above it in the next row.
pub const WATER_SPREAD_UP: f32 = 0.60;
pub const WATER_RUN_MAX: u32 = 5;
pub const WATER_COOLDOWN: u32 = 2;
pub const WATER_RECOVER: u32 = 3;
pub const WATER_MAX_SHARE: f32 = 0.30;
/// After the mountains: water makes up this share of the water + grass tiles...
pub const WATER_TARGET: f32 = 0.30;
/// ...give or take this much.
pub const WATER_TOLERANCE: f32 = 0.03;
// --- stage 1, mountains: few seeds that grow into clusters of patches ---
pub const MOUNTAIN_SEED_NEAR_WATER: f32 = 0.02;
pub const MOUNTAIN_SEED: f32 = 0.08;
/// Every seeded cluster beyond the second multiplies the seed chance by this.
pub const MOUNTAIN_EXTRA_CLUSTER: f32 = 0.25;
/// Roughness of rims: chance that a picked rim patch / tile is actually taken.
pub const MOUNTAIN_GROWTH: f32 = 0.7;
pub const MOUNTAIN_CLUSTER_SIZE: (usize, usize) = (6, 12);
// --- warp (between raw and layers): bends the straight patch edges into curves ---
/// How far (in tiles, at most) a tile may take its letter from.
pub const WARP_AMOUNT: f32 = 5.0;
/// Size of one noise cell in tiles: bigger is smoother, longer waves.
pub const WARP_SCALE: f32 = 7.0;
// --- smoothing (runs after the layers): borders push into each other, 3 lanes wide, each lane
// 2..=5 tiles. The chances are per direction per border tile per pass. A layer pushes into the
// one just below it; grass and water push into each other. ---
pub const SMOOTH_PASSES: usize = 3;
/// How many of its four directions a tile tries to push in per pass. 0 = pushes off: the push
/// lanes run along the grid, so they draw straight strips (over seeds 1-20 every setting from 1
/// to 4 gave more straight edges than the warp alone); the warp does the shaping instead.
pub const PUSH_TRIES: usize = 0;
pub const GRASS_WINS: f32 = 0.10;
pub const WATER_WINS: f32 = 0.04;
/// A plateau layer taking the layer just below it.
pub const MOUNTAIN_WINS: f32 = 0.12;
pub const PUSH_RANGE: (i32, i32) = (2, 5);
/// A tile joins a layer that this many of its four neighbours share. 4 removes lone specks
/// only; 3 also cuts every one-tile step of a curve, which straightens the edges again.
pub const SPECK_NEIGHBOURS: usize = 4;
/// Smoothing never makes a terrace narrower than this many tiles.
pub const MIN_TERRACE: i32 = 2;
// --- stage 2, layers: the same seed-and-grow, nested inside every mountain ---
/// A higher layer keeps this many tiles from any lower tile, so cliffs are one step tall.
pub const LAYER_MARGIN: i32 = 3;
/// Share of the eligible (deep enough) tiles that the next layer up fills.
pub const UPPER_SHARE: f32 = 0.55;

pub const WATER: u8 = b'W';
pub const GRASS: u8 = b'G';
pub const MOUNTAIN: u8 = b'M';
pub const RAMP: u8 = b'R';

const DIRS: [IVec2; 4] = [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y];

/// One letter per tile. Tile (0,0) is the bottom-left; `to_text` prints the top row first.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Grid {
    pub width: i32,
    pub height: i32,
    cells: Vec<u8>,
}

/// What a letter means as an elevation layer.
pub fn layer_of(letter: u8) -> Option<u8> {
    match letter {
        WATER => Some(0),
        GRASS => Some(1),
        MOUNTAIN => Some(2),
        b'2'..=b'9' => Some(letter - b'0'),
        _ => None,
    }
}

impl Grid {
    pub fn filled(width: i32, height: i32, letter: u8) -> Self {
        Self { width, height, cells: vec![letter; (width * height) as usize] }
    }

    pub fn in_bounds(&self, t: IVec2) -> bool {
        (0..self.width).contains(&t.x) && (0..self.height).contains(&t.y)
    }

    pub fn get(&self, t: IVec2) -> Option<u8> {
        self.in_bounds(t).then(|| self.cells[(t.y * self.width + t.x) as usize])
    }

    pub fn set(&mut self, t: IVec2, letter: u8) {
        if self.in_bounds(t) {
            self.cells[(t.y * self.width + t.x) as usize] = letter;
        }
    }

    /// The layer of a tile; a ramp is on the layer of the tile below it.
    pub fn layer(&self, t: IVec2) -> Option<u8> {
        self.ground(t).and_then(layer_of)
    }

    /// The letter of a tile, with a ramp read as the ground below it.
    fn ground(&self, t: IVec2) -> Option<u8> {
        match self.get(t)? {
            RAMP => self.get(t - IVec2::Y),
            letter => Some(letter),
        }
    }

    pub fn tiles(&self) -> impl Iterator<Item = IVec2> + '_ {
        (0..self.height).flat_map(move |y| (0..self.width).map(move |x| IVec2::new(x, y)))
    }

    /// Top row first, one line per row, no comment line.
    pub fn to_text(&self) -> String {
        let mut out = String::with_capacity(((self.width + 1) * self.height) as usize);
        for y in (0..self.height).rev() {
            for x in 0..self.width {
                out.push(self.get(IVec2::new(x, y)).unwrap() as char);
            }
            out.push('\n');
        }
        out
    }

    /// Parse a text grid. Lines starting with `#` and blank lines are skipped; the first line is
    /// the top row; every line must have the same length and only known letters.
    pub fn from_text(text: &str) -> Result<Grid, String> {
        let rows: Vec<&str> = text
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        let height = rows.len() as i32;
        let width = rows.first().map_or(0, |r| r.len()) as i32;
        if height == 0 || width == 0 {
            return Err("empty grid".into());
        }
        let mut grid = Grid::filled(width, height, GRASS);
        for (i, row) in rows.iter().enumerate() {
            if row.len() as i32 != width {
                return Err(format!("line {} has {} letters, expected {width}", i + 1, row.len()));
            }
            let y = height - 1 - i as i32;
            for (x, b) in row.bytes().enumerate() {
                if b != RAMP && layer_of(b).is_none() {
                    return Err(format!("line {} column {}: unknown letter {:?}", i + 1, x + 1, b as char));
                }
                grid.set(IVec2::new(x as i32, y), b);
            }
        }
        Ok(grid)
    }

    pub fn patch_count(&self) -> IVec2 {
        IVec2::new(self.width / PATCH.x, self.height / PATCH.y)
    }

    pub fn patch_tiles(patch: IVec2) -> impl Iterator<Item = IVec2> {
        let origin = patch * PATCH;
        (0..PATCH.y).flat_map(move |y| (0..PATCH.x).map(move |x| origin + IVec2::new(x, y)))
    }

    /// Sizes (largest first) of the 4-connected regions of tiles accepted by `keep`.
    pub fn regions(&self, keep: impl Fn(u8) -> bool) -> Vec<usize> {
        let mut seen = HashSet::new();
        let mut sizes = Vec::new();
        for start in self.tiles() {
            if !self.ground(start).is_some_and(&keep) || !seen.insert(start) {
                continue;
            }
            let mut stack = vec![start];
            let mut n = 0;
            while let Some(t) = stack.pop() {
                n += 1;
                for d in DIRS {
                    let next = t + d;
                    if self.ground(next).is_some_and(&keep) && seen.insert(next) {
                        stack.push(next);
                    }
                }
            }
            sizes.push(n);
        }
        sizes.sort_unstable_by(|a, b| b.cmp(a));
        sizes
    }

    pub fn stats(&self) -> Stats {
        let total = (self.width * self.height) as f32;
        let water = self.tiles().filter(|t| self.get(*t) == Some(WATER)).count();
        Stats {
            water_share: water as f32 / total,
            water_bodies: self.regions(|l| l == WATER),
            mountains: self.regions(|l| layer_of(l).is_some_and(|n| n >= 2)),
            top_layer: self.tiles().filter_map(|t| self.layer(t)).max().unwrap_or(0),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Stats {
    pub water_share: f32,
    pub water_bodies: Vec<usize>,
    pub mountains: Vec<usize>,
    pub top_layer: u8,
}

impl std::fmt::Display for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let bodies: Vec<usize> = self.water_bodies.iter().take(4).copied().collect();
        let mountains: Vec<usize> = self.mountains.iter().take(5).copied().collect();
        write!(
            f,
            "water {:4.1}% bodies {:?} | mountains {:?} top layer {}",
            100.0 * self.water_share,
            bodies,
            mountains,
            self.top_layer
        )
    }
}

/// raw -> warp -> layers -> smooth. Ramps come last, when the grid becomes a `TerrainMap`.
pub fn build(seed: u64, width: i32, height: i32) -> Grid {
    smooth(&layers(&warp(&raw(seed, width, height), seed), seed), seed)
}

// ---------- stage 1: raw patches ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PatchKind {
    Water,
    Grass,
    Mountain,
}

/// The patch walk: row by row away from a random corner. `dx`/`dy` point along the walk.
struct Walk {
    order: Vec<IVec2>,
    dx: i32,
    dy: i32,
}

fn walk_order(count: IVec2, rng: &mut impl Rng) -> Walk {
    let flip_x: bool = rng.random();
    let flip_y: bool = rng.random();
    let mut order = Vec::with_capacity((count.x * count.y) as usize);
    for y in 0..count.y {
        for x in 0..count.x {
            let px = if flip_x { count.x - 1 - x } else { x };
            let py = if flip_y { count.y - 1 - y } else { y };
            order.push(IVec2::new(px, py));
        }
    }
    Walk { order, dx: if flip_x { -1 } else { 1 }, dy: if flip_y { -1 } else { 1 } }
}

struct Patches {
    count: IVec2,
    kinds: Vec<PatchKind>,
}

impl Patches {
    fn get(&self, p: IVec2) -> Option<PatchKind> {
        (p.x >= 0 && p.y >= 0 && p.x < self.count.x && p.y < self.count.y)
            .then(|| self.kinds[(p.y * self.count.x + p.x) as usize])
    }
    fn set(&mut self, p: IVec2, kind: PatchKind) {
        self.kinds[(p.y * self.count.x + p.x) as usize] = kind;
    }
    fn all(&self) -> Vec<IVec2> {
        (0..self.count.y).flat_map(|y| (0..self.count.x).map(move |x| IVec2::new(x, y))).collect()
    }
    fn neighbours(&self, p: IVec2) -> impl Iterator<Item = IVec2> + '_ {
        DIRS.into_iter().map(move |d| p + d).filter(move |n| self.get(*n).is_some())
    }
}

/// Stage 1: every patch rolls water / grass / mountain; the leftover strips are grass.
pub fn raw(seed: u64, width: i32, height: i32) -> Grid {
    let mut rng = StdRng::seed_from_u64(seed);
    let mut grid = Grid::filled(width, height, GRASS);
    let count = grid.patch_count();
    let mut patches = Patches { count, kinds: vec![PatchKind::Grass; (count.x * count.y) as usize] };
    let walk = walk_order(count, &mut rng);
    roll_water(&mut patches, &walk, &mut rng);
    roll_mountains(&mut patches, &walk, &mut rng);
    let leftover_grass = width * height - count.x * count.y * PATCH.x * PATCH.y;
    balance_water(&mut patches, walk.order[0], leftover_grass as usize, &mut rng);
    for p in patches.all() {
        let letter = match patches.get(p).unwrap() {
            PatchKind::Water => WATER,
            PatchKind::Grass => GRASS,
            PatchKind::Mountain => MOUNTAIN,
        };
        for t in Grid::patch_tiles(p) {
            grid.set(t, letter);
        }
    }
    grid
}

/// Bring water to `WATER_TARGET` (± `WATER_TOLERANCE`) of the water + grass tiles, counting the
/// grass strips left over at the edges: too much water and a random lake patch next to grass
/// becomes grass; too little and a random grass patch next to a lake becomes water (never the
/// start corner). Lakes only shrink or grow at their shores, so they stay in one piece.
fn balance_water(patches: &mut Patches, start: IVec2, leftover_grass: usize, rng: &mut impl Rng) {
    let size = (PATCH.x * PATCH.y) as usize;
    loop {
        let count = |kind| patches.all().into_iter().filter(|p| patches.get(*p) == Some(kind)).count();
        let water = count(PatchKind::Water) * size;
        let grass = count(PatchKind::Grass) * size + leftover_grass;
        let share = water as f32 / (water + grass).max(1) as f32;
        let (from, to) = if share > WATER_TARGET + WATER_TOLERANCE {
            (PatchKind::Water, PatchKind::Grass)
        } else if share < WATER_TARGET - WATER_TOLERANCE {
            (PatchKind::Grass, PatchKind::Water)
        } else {
            return;
        };
        let shore: Vec<IVec2> = patches
            .all()
            .into_iter()
            .filter(|p| *p != start && patches.get(*p) == Some(from))
            .filter(|p| patches.neighbours(*p).any(|n| patches.get(n) == Some(to)))
            .collect();
        let Some(&p) = shore.choose(rng) else { return };
        patches.set(p, to);
    }
}

fn roll_water(patches: &mut Patches, walk: &Walk, rng: &mut impl Rng) {
    let total = walk.order.len() as f32;
    let mut bonus: HashMap<IVec2, f32> = HashMap::new();
    let (mut run, mut cooldown, mut recover, mut placed) = (0u32, 0u32, 0u32, 0u32);
    for (i, &patch) in walk.order.iter().enumerate() {
        if i == 0 {
            continue; // the start corner is never a lake
        }
        if cooldown > 0 {
            cooldown -= 1;
            run = 0;
            continue;
        }
        let mut chance = WATER_BASE + bonus.get(&patch).copied().unwrap_or(0.0);
        if recover > 0 {
            chance *= 1.0 - recover as f32 / (WATER_RECOVER + 1) as f32;
            recover -= 1;
        }
        let under_cap = (placed as f32) / total < WATER_MAX_SHARE;
        if under_cap && rng.random::<f32>() < chance {
            patches.set(patch, PatchKind::Water);
            placed += 1;
            run += 1;
            *bonus.entry(patch + IVec2::new(walk.dx, 0)).or_default() += WATER_SPREAD;
            *bonus.entry(patch + IVec2::new(0, walk.dy)).or_default() += WATER_SPREAD_UP;
            if run >= WATER_RUN_MAX || chance >= 1.0 {
                // A breather along the row; the bonuses for the next row stay so the lake can
                // continue upwards and join up.
                cooldown = WATER_COOLDOWN;
                recover = WATER_RECOVER;
                run = 0;
            }
        } else {
            run = 0;
        }
    }
}

/// Seeds grow by taking random grass patches on their rim until they reach a rolled size.
fn roll_mountains(patches: &mut Patches, walk: &Walk, rng: &mut impl Rng) {
    let mut clusters = 0usize;
    for &patch in &walk.order {
        if patches.get(patch) != Some(PatchKind::Grass) {
            continue;
        }
        let near_water = patches.neighbours(patch).any(|n| patches.get(n) == Some(PatchKind::Water));
        let mut seed = if near_water { MOUNTAIN_SEED_NEAR_WATER } else { MOUNTAIN_SEED };
        if clusters >= 2 {
            seed *= MOUNTAIN_EXTRA_CLUSTER.powi(clusters as i32 - 1);
        }
        if rng.random::<f32>() < seed {
            let target = rng.random_range(MOUNTAIN_CLUSTER_SIZE.0..=MOUNTAIN_CLUSTER_SIZE.1);
            grow_cluster(patches, patch, target, rng);
            clusters += 1;
        }
    }
    if clusters == 0 {
        // Never a map without a mountain: seed one on the grass patch farthest from water.
        let water: Vec<IVec2> = patches.all().into_iter().filter(|p| patches.get(*p) == Some(PatchKind::Water)).collect();
        let far = patches
            .all()
            .into_iter()
            .filter(|p| patches.get(*p) == Some(PatchKind::Grass))
            .max_by_key(|p| water.iter().map(|w| (*w - *p).abs().max_element()).min().unwrap_or(99));
        if let Some(p) = far {
            grow_cluster(patches, p, MOUNTAIN_CLUSTER_SIZE.0, rng);
        }
    }
}

fn grow_cluster(patches: &mut Patches, start: IVec2, target: usize, rng: &mut impl Rng) {
    let mut cluster = vec![start];
    patches.set(start, PatchKind::Mountain);
    while cluster.len() < target {
        let mut rim: Vec<IVec2> = cluster
            .iter()
            .flat_map(|c| patches.neighbours(*c).collect::<Vec<_>>())
            .filter(|n| patches.get(*n) == Some(PatchKind::Grass))
            .collect();
        if rim.is_empty() {
            break;
        }
        rim.shuffle(rng);
        let n = rim[0];
        if rim.len() == 1 || rng.random::<f32>() < MOUNTAIN_GROWTH {
            patches.set(n, PatchKind::Mountain);
            cluster.push(n);
        }
    }
}

// ---------- warp: bend the straight patch edges ----------

/// Warp: every tile takes the letter of a spot up to `WARP_AMOUNT` tiles away, shifted by
/// smooth noise (`WARP_SCALE` tiles per noise cell), so the straight edges of the patches
/// become wavy curves. The noise is smooth, so it makes no specks.
pub fn warp(grid: &Grid, seed: u64) -> Grid {
    let (nx, ny) = (Noise::new(seed ^ 0x3A9F_1C27), Noise::new(seed ^ 0x6D2E_84B1));
    let shift = |n: &Noise, t: IVec2| {
        let v = n.fbm(t.x as f32 / WARP_SCALE, t.y as f32 / WARP_SCALE, 3);
        ((v - 0.5) * 2.0 * WARP_AMOUNT).round() as i32
    };
    let max = IVec2::new(grid.width - 1, grid.height - 1);
    let mut out = grid.clone();
    for t in grid.tiles() {
        let from = (t + IVec2::new(shift(&nx, t), shift(&ny, t))).clamp(IVec2::ZERO, max);
        out.set(t, grid.get(from).unwrap());
    }
    out
}

// ---------- smoothing on letters, after the layers ----------

/// The letter for a layer.
fn letter_of(layer: u8) -> u8 {
    match layer {
        0 => WATER,
        1 => GRASS,
        n => b'0' + n,
    }
}

/// Can two neighbouring layers stand side by side? Land changes by at most one layer (every
/// cliff one step tall); water may touch grass and the lowest plateau (layer 2), no higher.
fn fits_beside(a: u8, b: u8) -> bool {
    if a == 0 || b == 0 {
        a.max(b) <= 2
    } else {
        a.abs_diff(b) <= 1
    }
}

/// Would `tile` as `layer` still fit beside all its neighbours? Also keeps every terrace at
/// least `MIN_TERRACE` tiles wide: no land two or more layers lower within that distance, so a
/// ramp (the ramp tile and the tile on its open side) always has room on the layer below.
fn fits(grid: &Grid, tile: IVec2, layer: u8) -> bool {
    let beside = DIRS
        .iter()
        .filter_map(|d| grid.layer(tile + *d))
        .all(|l| fits_beside(layer, l));
    let r = MIN_TERRACE;
    let wide = (-r..=r).all(|dx| {
        (-r..=r).all(|dy| {
            grid.layer(tile + IVec2::new(dx, dy))
                .is_none_or(|l| l == 0 || l + 1 >= layer)
        })
    });
    beside && wide
}

/// Push `winner` into `loser` tiles starting next to `from` along `dir`, as a band three lanes
/// wide (the tile and its two side neighbours), each lane its own random 2..=5 tiles long, so
/// the result is a ragged bulge, not a straight-edged block. A lane stops at the first tile
/// that is not `loser` or where `winner` would make a cliff two layers tall.
fn push(grid: &mut Grid, rng: &mut impl Rng, from: IVec2, dir: IVec2, winner: u8, loser: u8) {
    let side = IVec2::new(dir.y, dir.x);
    let layer = layer_of(winner).unwrap();
    for offset in [0, -1, 1] {
        let len = rng.random_range(PUSH_RANGE.0..=PUSH_RANGE.1);
        let start = from + side * offset;
        if grid.get(start) != Some(winner) {
            continue;
        }
        for k in 1..=len {
            let t = start + dir * k;
            if grid.get(t) != Some(loser) || !fits(grid, t, layer) {
                break;
            }
            grid.set(t, winner);
        }
    }
}

/// Smoothing, on the layered grid: borders push into each other (grass beats water more often
/// than the reverse, a plateau layer beats the layer just below it and is never pushed back),
/// then lone tiles join their surroundings: a tile takes the letter that at least three of its
/// four neighbours share. Nothing may make a cliff two layers tall (`fits`), and a plateau never
/// turns into water.
pub fn smooth(grid: &Grid, seed: u64) -> Grid {
    let mut rng = StdRng::seed_from_u64(seed ^ 0x5A00_7A11);
    let mut grid = grid.clone();
    let mut tiles: Vec<IVec2> = grid.tiles().collect();
    for _ in 0..SMOOTH_PASSES {
        tiles.shuffle(&mut rng);
        for &tile in &tiles {
            let mut dirs = DIRS;
            dirs.shuffle(&mut rng);
            for dir in dirs.into_iter().take(PUSH_TRIES) {
                let (Some(here), Some(a)) = (grid.get(tile), grid.layer(tile)) else { continue };
                let (Some(there), Some(b)) = (grid.get(tile + dir), grid.layer(tile + dir)) else { continue };
                let chance = match (a, b) {
                    (1, 0) => GRASS_WINS,
                    (0, 1) => WATER_WINS,
                    (a, b) if a >= 2 && b + 1 == a => MOUNTAIN_WINS,
                    _ => 0.0,
                };
                if rng.random::<f32>() < chance {
                    push(&mut grid, &mut rng, tile, dir, here, there);
                }
            }
        }
        // Specks: a tile whose neighbours all share another layer joins them.
        for &tile in &tiles {
            let here = grid.layer(tile).unwrap();
            let around: Vec<u8> = DIRS.iter().filter_map(|d| grid.layer(tile + *d)).collect();
            let Some(&new) = around
                .iter()
                .find(|v| **v != here && around.iter().filter(|w| *w == *v).count() >= SPECK_NEIGHBOURS)
            else {
                continue;
            };
            if !(here >= 2 && new == 0) && fits(&grid, tile, new) {
                grid.set(tile, letter_of(new));
            }
        }
    }
    grid
}

// ---------- stage 2: layers, the same seed-and-grow nested inside every mountain ----------

/// Stage 2: for every layer n from 2 up, the tiles of layer n that are at least `LAYER_MARGIN`
/// from any lower tile are eligible; a random eligible tile seeds a blob that grows over random
/// eligible rim tiles until `UPPER_SHARE` of the eligible tiles are taken and becomes n+1.
/// Each mountain gets its own blob. The margin keeps every cliff one layer tall.
pub fn layers(grid: &Grid, seed: u64) -> Grid {
    let mut rng = StdRng::seed_from_u64(seed ^ 0x1A7E_7A55);
    let mut grid = grid.clone();
    let all: Vec<IVec2> = grid.tiles().collect();
    for t in all {
        if grid.get(t) == Some(MOUNTAIN) {
            grid.set(t, b'2');
        }
    }
    for n in 2..MAX_LAYER {
        let at_least_n = |g: &Grid, t: IVec2| g.layer(t).is_some_and(|l| l >= n);
        // Distance from every tile at layer >= n to the nearest tile below n (or the map edge).
        let mut dist: HashMap<IVec2, i32> = HashMap::new();
        let mut queue = VecDeque::new();
        for t in grid.tiles() {
            if at_least_n(&grid, t) && DIRS.iter().any(|d| !at_least_n(&grid, t + *d)) {
                dist.insert(t, 1);
                queue.push_back(t);
            }
        }
        while let Some(t) = queue.pop_front() {
            let d = dist[&t];
            for next in DIRS.map(|d| t + d) {
                if at_least_n(&grid, next) && !dist.contains_key(&next) {
                    dist.insert(next, d + 1);
                    queue.push_back(next);
                }
            }
        }
        let eligible: HashSet<IVec2> = dist.iter().filter(|(_, d)| **d > LAYER_MARGIN).map(|(t, _)| *t).collect();
        // One blob per mountain: group the eligible tiles into connected regions.
        let mut seen: HashSet<IVec2> = HashSet::new();
        let mut starts: Vec<IVec2> = eligible.iter().copied().collect();
        starts.sort_by_key(|t| (t.y, t.x));
        for start in starts {
            if seen.contains(&start) {
                continue;
            }
            let mut region = Vec::new();
            let mut stack = vec![start];
            seen.insert(start);
            while let Some(t) = stack.pop() {
                region.push(t);
                for next in DIRS.map(|d| t + d) {
                    if eligible.contains(&next) && seen.insert(next) {
                        stack.push(next);
                    }
                }
            }
            let target = ((region.len() as f32) * UPPER_SHARE).round().max(1.0) as usize;
            let region_set: HashSet<IVec2> = region.iter().copied().collect();
            let seed_tile = region[rng.random_range(0..region.len())];
            let mut blob: Vec<IVec2> = vec![seed_tile];
            let mut in_blob: HashSet<IVec2> = HashSet::from([seed_tile]);
            while blob.len() < target {
                let mut rim: Vec<IVec2> = blob
                    .iter()
                    .flat_map(|b| DIRS.map(|d| *b + d))
                    .filter(|t| region_set.contains(t) && !in_blob.contains(t))
                    .collect();
                if rim.is_empty() {
                    break;
                }
                rim.shuffle(&mut rng);
                let t = rim[0];
                if rim.len() == 1 || rng.random::<f32>() < MOUNTAIN_GROWTH {
                    in_blob.insert(t);
                    blob.push(t);
                }
            }
            for t in blob {
                grid.set(t, b'0' + n + 1);
            }
        }
    }
    grid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trip_and_errors() {
        let text = "# a comment\nWWGG\nG234\nMMGW\n";
        let grid = Grid::from_text(text).unwrap();
        assert_eq!((grid.width, grid.height), (4, 3));
        assert_eq!(grid.get(IVec2::new(0, 2)), Some(WATER), "first line is the top row");
        assert_eq!(grid.get(IVec2::new(3, 1)), Some(b'4'));
        assert_eq!(grid.layer(IVec2::new(0, 0)), Some(2), "M is layer 2");
        let ramp = Grid::from_text("GR3\nG22\n").unwrap();
        assert_eq!(ramp.get(IVec2::new(1, 1)), Some(RAMP));
        assert_eq!(ramp.layer(IVec2::new(1, 1)), Some(2), "a ramp is on the layer below it");
        assert_eq!(Grid::from_text(&ramp.to_text()).unwrap(), ramp);
        assert_eq!(Grid::from_text(&grid.to_text()).unwrap(), grid);
        assert!(Grid::from_text("GGG\nGG\n").is_err(), "ragged");
        assert!(Grid::from_text("GXG\n").is_err(), "unknown letter");
        assert!(Grid::from_text("# only\n").is_err(), "empty");
    }

    #[test]
    fn raw_is_deterministic_patchy_and_only_wgm() {
        let a = raw(5, 64, 48);
        assert_eq!(a, raw(5, 64, 48));
        assert!(a.tiles().all(|t| matches!(a.get(t), Some(WATER | GRASS | MOUNTAIN))));
        // Every patch is uniform.
        let count = a.patch_count();
        for y in 0..count.y {
            for x in 0..count.x {
                let mut letters = Grid::patch_tiles(IVec2::new(x, y)).map(|t| a.get(t).unwrap());
                let first = letters.next().unwrap();
                assert!(letters.all(|l| l == first));
            }
        }
    }

    #[test]
    fn smooth_only_converts_as_allowed() {
        let a = layers(&warp(&raw(7, 64, 48), 7), 7);
        let b = smooth(&a, 7);
        for t in a.tiles() {
            let (before, after) = (a.layer(t).unwrap(), b.layer(t).unwrap());
            assert!(!(before >= 2 && after == 0), "a plateau fell into water at {t}");
            for d in DIRS {
                if let Some(l) = b.layer(t + d) {
                    assert!(fits_beside(after, l), "{after} beside {l} at {t}");
                }
            }
        }
    }

    #[test]
    fn layers_keep_cliffs_one_step_tall() {
        let mut saw_3 = false;
        for seed in 1..=8u64 {
            let g = build(seed, 64, 48);
            for t in g.tiles() {
                let here = g.layer(t).unwrap();
                assert!(here <= MAX_LAYER);
                saw_3 |= here >= 3;
                for d in DIRS {
                    if let Some(l) = g.layer(t + d)
                        && here >= 1
                        && l >= 1
                    {
                        assert!((l as i32 - here as i32).abs() <= 1, "two-layer jump at {t} (seed {seed})");
                    }
                }
                if here >= 3 {
                    for d in DIRS {
                        assert!(g.layer(t + d).is_none_or(|l| l >= 2), "layer 3 touching low ground at {t}");
                    }
                }
            }
        }
        assert!(saw_3, "no map reached layer 3");
    }

    #[test]
    fn built_maps_have_connected_water_and_big_mountains() {
        for seed in 1..=8u64 {
            let s = build(seed, 64, 48).stats();
            assert!((0.08..=0.38).contains(&s.water_share), "water share {} (seed {seed})", s.water_share);
            let water: usize = s.water_bodies.iter().sum();
            let two: usize = s.water_bodies.iter().take(2).sum();
            assert!(two as f32 >= 0.55 * water as f32, "water is scattered: {:?} (seed {seed})", s.water_bodies);
            assert!((1..=4).contains(&s.mountains.len()), "mountains {:?} (seed {seed})", s.mountains);
            assert!(s.mountains[0] >= 40, "largest mountain only {} tiles (seed {seed})", s.mountains[0]);
        }
    }
}
/// Tuning aid for the shape of edges: `cargo test straightness -- --ignored --nocapture`.
#[cfg(test)]
mod edge_stats {
    use super::*;
    /// Straight border runs of at least 5 tiles (horizontal + vertical) and the longest.
    fn straight(g: &Grid) -> (usize, i32) {
        let (mut count, mut longest) = (0, 0);
        for (step, across) in [(IVec2::X, IVec2::Y), (IVec2::Y, IVec2::X)] {
            for t in g.tiles() {
                let pair = (g.layer(t), g.layer(t + across));
                if pair.1.is_none() || pair.0 == pair.1 { continue }
                let prev = (g.layer(t - step), g.layer(t - step + across));
                if prev == pair { continue }
                let mut n = 0;
                while (g.layer(t + step * n), g.layer(t + step * n + across)) == pair { n += 1 }
                if n >= 5 { count += 1 }
                longest = longest.max(n);
            }
        }
        (count, longest)
    }
    #[test]
    #[ignore]
    fn straightness() {
        let (mut a, mut b, mut changed, mut water) = (0, 0, 0, 0.0);
        for seed in 1..=20u64 {
            let l = layers(&warp(&raw(seed, 64, 48), seed), seed);
            let s = smooth(&l, seed);
            a += straight(&l).0; b += straight(&s).0;
            changed += l.tiles().filter(|t| l.get(*t) != s.get(*t)).count();
            water += s.stats().water_share;
        }
        println!("straight runs >=5: layered {a} smoothed {b} | changed tiles/seed {} | water {:.1}%", changed / 20, water * 5.0);
        let mut shares: Vec<f32> = (1..=100u64)
            .map(|seed| {
                let g = build(seed, 64, 48);
                let w = g.tiles().filter(|t| g.layer(*t) == Some(0)).count() as f32;
                let gr = g.tiles().filter(|t| g.layer(*t) == Some(1)).count() as f32;
                w / (w + gr)
            })
            .collect();
        shares.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mean = shares.iter().sum::<f32>() / 100.0;
        println!(
            "water : (water + grass), seeds 1-100: mean {:.0}% min {:.0}% median {:.0}% max {:.0}%",
            mean * 100.0, shares[0] * 100.0, shares[50] * 100.0, shares[99] * 100.0
        );
    }
}
