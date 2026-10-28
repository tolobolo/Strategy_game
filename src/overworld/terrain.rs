//! Terrain tiles: layers, ramps, walking rules and paths. The map itself is built as text by
//! `worldgen.rs` (or loaded from `assets/seeds/<seed>.txt`) and turned into a `TerrainMap` here.
//! See agents.md §1.6.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use rand::rngs::StdRng;
use rand::seq::IndexedRandom;
use rand::{Rng, SeedableRng};

use super::worldgen::{self, Grid};

/// World size of one overworld tile in pixels (Tiny Swords tiles are 64×64).
pub const TILE: f32 = 64.0;
pub use super::worldgen::PATCH;
/// Seed files live here; `cargo run -- map <seed> --save` writes them.
pub const SEEDS_DIR: &str = "assets/seeds";
/// A plateau region gets one extra ramp per this many tiles...
const EXTRA_RAMP_EVERY: usize = 40;
/// ...at least this many tiles (Chebyshev) from any other ramp.
const EXTRA_RAMP_SPACING: i32 = 6;
/// Safety stop for `ensure_ramps`: at most this many ramps and reshapes in total.
const MAX_RESHAPES: usize = 2000;

/// What `TerrainMap::ensure_ramps` changed.
#[derive(Default, Debug)]
pub struct RampReport {
    /// Ramps that did not fit and were removed.
    pub dropped: Vec<IVec2>,
    /// Regions no ramp could reach: (layer before, a tile of it, size, filled up rather than
    /// lowered).
    pub reshaped: Vec<(u8, IVec2, usize, bool)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Terrain {
    Water,
    Grass,
    /// Land at layer 2 or higher: a plateau with cliffs, climbed by the ramps at the cliff ends.
    Mountain,
}

impl Terrain {
    pub fn walkable(self) -> bool {
        self != Terrain::Water
    }

    fn from_layer(layer: u8) -> Terrain {
        match layer {
            0 => Terrain::Water,
            1 => Terrain::Grass,
            _ => Terrain::Mountain,
        }
    }
}

/// Which way a ramp opens: the low side its slope runs down to. A ramp sits beside a tile one
/// layer higher, so `Left` has the higher tile to its east (the sheet's column 0, slope down to
/// the west, rock on the right) and `Right` has it to its west (column 3, the mirror image).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RampSide {
    Left,
    Right,
}

impl RampSide {
    /// One step from the ramp out of its open side.
    pub fn outward(self) -> IVec2 {
        match self {
            RampSide::Left => IVec2::NEG_X,
            RampSide::Right => IVec2::X,
        }
    }
}

/// A rectangular grid of tiles with an elevation layer each. Tile (0,0) is the bottom-left
/// corner of the world.
#[derive(Clone, Debug)]
pub struct TerrainMap {
    pub width: i32,
    pub height: i32,
    layer: Vec<u8>,
    /// Ramps (`R` in the text grid) keyed by their tile, on the lower layer; the tile they climb
    /// to is their east or west neighbour, away from the open side.
    ramps: HashMap<IVec2, RampSide>,
}

impl TerrainMap {
    pub fn filled(width: i32, height: i32, terrain: Terrain) -> Self {
        let layer = match terrain {
            Terrain::Water => 0,
            Terrain::Grass => 1,
            Terrain::Mountain => 2,
        };
        Self {
            width,
            height,
            layer: vec![layer; (width * height) as usize],
            ramps: HashMap::new(),
        }
    }

    pub fn in_bounds(&self, tile: IVec2) -> bool {
        (0..self.width).contains(&tile.x) && (0..self.height).contains(&tile.y)
    }

    fn index(&self, tile: IVec2) -> usize {
        (tile.y * self.width + tile.x) as usize
    }

    pub fn layer(&self, tile: IVec2) -> Option<u8> {
        self.in_bounds(tile).then(|| self.layer[self.index(tile)])
    }

    pub fn set_layer(&mut self, tile: IVec2, layer: u8) {
        if self.in_bounds(tile) {
            let i = self.index(tile);
            self.layer[i] = layer;
        }
    }

    pub fn get(&self, tile: IVec2) -> Option<Terrain> {
        self.layer(tile).map(Terrain::from_layer)
    }

    /// Water = layer 0, grass = 1, mountain = 2 (use `set_layer` for higher plateaus).
    pub fn set(&mut self, tile: IVec2, terrain: Terrain) {
        self.set_layer(
            tile,
            match terrain {
                Terrain::Water => 0,
                Terrain::Grass => 1,
                Terrain::Mountain => 2,
            },
        );
    }

    pub fn is_land(&self, tile: IVec2) -> bool {
        self.layer(tile).is_some_and(|l| l >= 1)
    }

    /// A land tile whose north neighbour is higher: the foot of a cliff.
    pub fn is_cliff_foot(&self, tile: IVec2) -> bool {
        match (self.layer(tile), self.layer(tile + IVec2::Y)) {
            (Some(here), Some(above)) => here >= 1 && above > here,
            _ => false,
        }
    }

    pub fn ramp_at(&self, tile: IVec2) -> Option<RampSide> {
        self.ramps.get(&tile).copied()
    }

    /// An `R` tile: the ramp itself, on the lower layer, beside the higher tile it climbs to.
    pub fn is_ramp(&self, tile: IVec2) -> bool {
        self.ramps.contains_key(&tile)
    }

    /// The higher tile a ramp climbs to: its east or west neighbour, away from the open side.
    pub fn ramp_top(&self, ramp: IVec2) -> Option<IVec2> {
        self.ramp_at(ramp).map(|side| ramp - side.outward())
    }

    /// The ground tile just outside a ramp's open side: where the slope is walked onto.
    pub fn is_ramp_entry(&self, tile: IVec2) -> bool {
        [RampSide::Left, RampSide::Right]
            .iter()
            .any(|side| self.ramp_at(tile - side.outward()) == Some(*side))
    }

    /// A tile that must stay clear of decorations: a cliff wall, a ramp, the ground tile below
    /// it (the lower half of the slope is drawn there) or at its open side, or the edge of a
    /// plateau (a land tile with a differently-layered land neighbour).
    pub fn is_edge_art(&self, tile: IVec2) -> bool {
        let Some(here) = self.layer(tile) else {
            return false;
        };
        if here == 0 {
            return false;
        }
        self.is_cliff_foot(tile)
            || self.is_ramp(tile)
            || self.is_ramp(tile + IVec2::Y)
            || self.is_ramp_entry(tile)
            || [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y]
                .iter()
                .any(|d| self.layer(tile + *d).is_some_and(|l| l >= 1 && l != here))
    }

    pub fn ramps(&self) -> impl Iterator<Item = (IVec2, RampSide)> + '_ {
        self.ramps.iter().map(|(t, s)| (*t, *s))
    }

    /// Could a ramp go on `tile`, and which way would it open? Exactly one of its east / west
    /// neighbours must be one layer higher (the tile it climbs to); the tile on the other side
    /// (where it is walked onto) and the tile below it (where the lower half of the slope is
    /// drawn) must be open ground on its own layer; the tile above it must not be higher (that
    /// would make it a cliff wall); and no other ramp may touch it.
    pub fn ramp_fits(&self, tile: IVec2) -> Option<RampSide> {
        let here = self.layer(tile).filter(|l| *l >= 1)?;
        let higher = |d: IVec2| self.layer(tile + d) == Some(here + 1);
        let side = match (higher(IVec2::NEG_X), higher(IVec2::X)) {
            (false, true) => RampSide::Left,
            (true, false) => RampSide::Right,
            _ => return None,
        };
        let ground = |t: IVec2| self.layer(t) == Some(here) && !self.is_cliff_foot(t);
        let ok = ground(tile + side.outward())
            && ground(tile - IVec2::Y)
            && self.layer(tile + IVec2::Y).is_none_or(|l| l <= here)
            && [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y]
                .iter()
                .all(|d| !self.is_ramp(tile + *d));
        ok.then_some(side)
    }

    /// Put a ramp on `tile` if one fits there (`ramp_fits`).
    pub fn add_ramp(&mut self, tile: IVec2) -> bool {
        match self.ramp_fits(tile) {
            Some(side) => {
                self.ramps.insert(tile, side);
                true
            }
            None => false,
        }
    }

    /// A tile you can stand on: land that is not the stone face of a cliff.
    pub fn is_walkable(&self, tile: IVec2) -> bool {
        self.is_land(tile) && !self.is_cliff_foot(tile)
    }

    /// Can something walk from `from` onto `to`? Same layer is fine as long as `to` is not water
    /// or a cliff wall, so a ramp is walked onto from any side of its own layer. A change of
    /// layer (always by exactly one) happens only between a ramp and the higher tile it climbs
    /// to.
    pub fn can_step(&self, from: IVec2, to: IVec2) -> bool {
        let (Some(a), Some(b)) = (self.layer(from), self.layer(to)) else {
            return false;
        };
        if !self.is_walkable(to) {
            return false;
        }
        if a == b {
            return true;
        }
        self.ramp_top(from) == Some(to) || self.ramp_top(to) == Some(from)
    }

    /// Make sure every plateau can be reached. Ramps that do not fit (`ramp_fits`) are dropped;
    /// then every plateau region that cannot be walked to gets a ramp linking it to ground that
    /// can (up from below or down from above), lowest layer first. A region where no ramp fits
    /// anywhere is reshaped (`reshape`) and the check runs again. Last, about one more ramp per
    /// `EXTRA_RAMP_EVERY` tiles of a region, `EXTRA_RAMP_SPACING` tiles from the others. A ramp
    /// beside the bottom corner of a plateau (a cliff under the tile it climbs to, so the rock
    /// half of the slope leans on it) is preferred. Returns the ramps that were dropped and the
    /// regions that were reshaped.
    pub fn ensure_ramps(&mut self, rng: &mut impl Rng) -> RampReport {
        let mut report = RampReport::default();
        for _ in 0..MAX_RESHAPES {
            report.dropped.extend(self.drop_bad_ramps());
            let reached = self.reachable();
            let mut unreached = self.plateau_regions();
            unreached.retain(|r| !reached.contains(&r[0]));
            let Some(lowest) = unreached.first().cloned() else { break };
            let spot = unreached.iter().find_map(|r| self.pick_ramp(r, &reached, rng, 0));
            match spot {
                Some(t) => {
                    self.add_ramp(t);
                }
                None => report.reshaped.push(self.reshape(&lowest)),
            }
        }

        let reached = self.reachable();
        for region in self.plateau_regions() {
            for _ in 0..region.len() / EXTRA_RAMP_EVERY {
                if let Some(t) = self.pick_ramp(&region, &reached, rng, EXTRA_RAMP_SPACING) {
                    self.add_ramp(t);
                }
            }
        }
        report
    }

    /// Drop every ramp that no longer fits; returns where they were.
    fn drop_bad_ramps(&mut self) -> Vec<IVec2> {
        let mut dropped: Vec<IVec2> = self.ramps.keys().copied().collect();
        dropped.sort_by_key(|t| (t.y, t.x));
        dropped.retain(|t| {
            self.ramps.remove(t);
            match self.ramp_fits(*t) {
                Some(side) => {
                    self.ramps.insert(*t, side);
                    false
                }
                None => true,
            }
        });
        dropped
    }

    /// Change the layers of a plateau region no ramp can reach, so the check can go on. A
    /// pocket (no lower land or water around it, only walls and higher ground) is filled up to
    /// the layer around it. Anything else is lowered one layer, together with all the higher
    /// ground that stands on it, so no cliff becomes two layers tall. Returns (layer before, a
    /// tile of the region, its size, filled).
    fn reshape(&mut self, region: &[IVec2]) -> (u8, IVec2, usize, bool) {
        let layer = self.layer(region[0]).unwrap();
        let in_region: HashSet<IVec2> = region.iter().copied().collect();
        let pocket = region.iter().all(|t| {
            [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y]
                .iter()
                .all(|d| in_region.contains(&(*t + *d)) || self.layer(*t + *d).is_none_or(|l| l >= layer))
        });
        if pocket {
            for t in region {
                self.set_layer(*t, layer + 1);
            }
        } else {
            // The region and every tile above its layer joined to it.
            let mut lower: Vec<IVec2> = region.to_vec();
            let mut seen = in_region;
            let mut i = 0;
            while i < lower.len() {
                let t = lower[i];
                i += 1;
                for d in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                    let next = t + d;
                    if self.layer(next).is_some_and(|l| l > layer) && seen.insert(next) {
                        lower.push(next);
                    }
                }
            }
            for t in lower {
                let l = self.layer(t).unwrap();
                self.set_layer(t, l - 1);
            }
        }
        (layer, region[0], region.len(), pocket)
    }

    /// A random spot for a ramp linking `region` with ground in `reached`: up into the region
    /// from reached ground below, or down into it from reached ground above. At least `spacing`
    /// tiles (Chebyshev) from every other ramp; a bottom corner of a plateau if there is one.
    fn pick_ramp(
        &self,
        region: &[IVec2],
        reached: &HashSet<IVec2>,
        rng: &mut impl Rng,
        spacing: i32,
    ) -> Option<IVec2> {
        let in_region: HashSet<IVec2> = region.iter().copied().collect();
        let links = |ramp: IVec2, top: IVec2| {
            (in_region.contains(&top) && reached.contains(&ramp))
                || (in_region.contains(&ramp) && reached.contains(&top))
        };
        let mut spots: Vec<IVec2> = region
            .iter()
            .flat_map(|t| [*t + IVec2::X, *t + IVec2::NEG_X, *t])
            .filter(|t| {
                self.ramp_fits(*t)
                    .is_some_and(|side| links(*t, *t - side.outward()))
            })
            .filter(|t| self.ramps.keys().all(|r| (*r - *t).abs().max_element() >= spacing))
            .collect();
        spots.sort_by_key(|t| (t.y, t.x));
        spots.dedup();
        let corner = |t: &IVec2| {
            let top = *t - self.ramp_fits(*t).unwrap().outward();
            self.is_cliff_foot(top - IVec2::Y)
        };
        let corners: Vec<IVec2> = spots.iter().copied().filter(corner).collect();
        let pool = if corners.is_empty() { spots } else { corners };
        pool.choose(rng).copied()
    }

    /// Every tile that can be walked to from the grass (layer 1).
    pub fn reachable(&self) -> HashSet<IVec2> {
        let mut stack: Vec<IVec2> = self
            .tiles()
            .map(|(t, _)| t)
            .filter(|t| self.layer(*t) == Some(1) && self.is_walkable(*t))
            .collect();
        let mut seen: HashSet<IVec2> = stack.iter().copied().collect();
        while let Some(t) = stack.pop() {
            for d in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                let next = t + d;
                if !seen.contains(&next) && self.can_step(t, next) {
                    seen.insert(next);
                    stack.push(next);
                }
            }
        }
        seen
    }

    /// The plateau regions (walkable tiles of layer 2 and up, joined by steps on the same
    /// layer), lowest layer first, each listed from its bottom-left tile.
    pub fn plateau_regions(&self) -> Vec<Vec<IVec2>> {
        let mut seen: HashSet<IVec2> = HashSet::new();
        let mut regions = Vec::new();
        for (start, _) in self.tiles() {
            let Some(layer) = self.layer(start) else { continue };
            if layer < 2 || !self.is_walkable(start) || !seen.insert(start) {
                continue;
            }
            let mut region = vec![start];
            let mut i = 0;
            while i < region.len() {
                let t = region[i];
                i += 1;
                for d in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                    let next = t + d;
                    if self.layer(next) == Some(layer) && !seen.contains(&next) && self.can_step(t, next) {
                        seen.insert(next);
                        region.push(next);
                    }
                }
            }
            regions.push(region);
        }
        regions.sort_by_key(|r| self.layer(r[0]));
        regions
    }

    /// Plateau regions nobody can walk to: (layer, a tile in it, size).
    pub fn unreached_regions(&self) -> Vec<(u8, IVec2, usize)> {
        let reached = self.reachable();
        self.plateau_regions()
            .into_iter()
            .filter(|r| !reached.contains(&r[0]))
            .map(|r| (self.layer(r[0]).unwrap(), r[0], r.len()))
            .collect()
    }

    /// Layers from a letter grid (`W` 0, `G` 1, `M` 2, digits as they are) and its ramps (`R`,
    /// on the layer of the tile below). Ramps are taken as written; `ensure_ramps` checks them.
    pub fn from_grid(grid: &Grid) -> Self {
        let mut map = TerrainMap::filled(grid.width, grid.height, Terrain::Grass);
        for t in grid.tiles() {
            map.set_layer(t, grid.layer(t).unwrap_or(1));
        }
        for t in grid.tiles().filter(|t| grid.get(*t) == Some(worldgen::RAMP)) {
            let side = if map.layer(t + IVec2::X) > map.layer(t - IVec2::X) { RampSide::Left } else { RampSide::Right };
            map.ramps.insert(t, side);
        }
        map
    }

    /// The letters of this map, ramps as `R`.
    pub fn to_grid(&self) -> Grid {
        let mut grid = Grid::filled(self.width, self.height, worldgen::GRASS);
        for (t, _) in self.tiles() {
            let letter = match self.layer(t).unwrap() {
                0 => worldgen::WATER,
                1 => worldgen::GRASS,
                n => b'0' + n,
            };
            grid.set(t, if self.is_ramp(t) { worldgen::RAMP } else { letter });
        }
        grid
    }

    /// Shortest 4-neighbour path obeying `can_step` (never water, layer changes only on ramps).
    /// Returns the tiles after `from` up to and including `to`; `Some(vec![])` when already there,
    /// `None` when unreachable. A* from the `pathfinding` crate, Manhattan heuristic.
    pub fn find_path(&self, from: IVec2, to: IVec2) -> Option<Vec<IVec2>> {
        if from == to {
            return Some(Vec::new());
        }
        if !self.is_walkable(to) {
            return None;
        }
        let (path, _cost) = pathfinding::prelude::astar(
            &from,
            |&tile| {
                [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y]
                    .into_iter()
                    .map(move |d| tile + d)
                    .filter(move |&next| self.can_step(tile, next))
                    .map(|next| (next, 1u32))
            },
            |&tile| ((tile - to).abs().x + (tile - to).abs().y) as u32,
            |&tile| tile == to,
        )?;
        Some(path.into_iter().skip(1).collect())
    }

    // ---------- patches ----------

    /// Number of whole patches across and up. The leftover strips on the right and at the top
    /// are plain grass.
    pub fn patch_count(&self) -> IVec2 {
        IVec2::new(self.width / PATCH.x, self.height / PATCH.y)
    }

    pub fn patch_origin(patch: IVec2) -> IVec2 {
        patch * PATCH
    }

    pub fn patch_tiles(patch: IVec2) -> impl Iterator<Item = IVec2> {
        let origin = Self::patch_origin(patch);
        (0..PATCH.y).flat_map(move |y| (0..PATCH.x).map(move |x| origin + IVec2::new(x, y)))
    }

    pub fn all_patches(&self) -> Vec<IVec2> {
        let count = self.patch_count();
        (0..count.y)
            .flat_map(|y| (0..count.x).map(move |x| IVec2::new(x, y)))
            .collect()
    }

    /// Patches whose every tile is grass at layer 1 after smoothing and clear of ramps: where a
    /// base can stand.
    pub fn flat_patches(&self) -> Vec<IVec2> {
        self.all_patches()
            .into_iter()
            .filter(|p| Self::patch_tiles(*p).all(|t| {
                self.layer(t) == Some(1)
                    && !self.is_ramp(t)
                    && !self.is_ramp(t + IVec2::Y)
                    && !self.is_ramp_entry(t)
            }))
            .collect()
    }

    /// Centre of a tile in world coordinates. The map is centred on the origin.
    pub fn world_pos(&self, tile: IVec2) -> Vec2 {
        Vec2::new(
            (tile.x as f32 - self.width as f32 / 2.0 + 0.5) * TILE,
            (tile.y as f32 - self.height as f32 / 2.0 + 0.5) * TILE,
        )
    }

    pub fn tile_of(&self, world: Vec2) -> IVec2 {
        IVec2::new(
            (world.x / TILE + self.width as f32 / 2.0).floor() as i32,
            (world.y / TILE + self.height as f32 / 2.0).floor() as i32,
        )
    }

    #[allow(dead_code)]
    pub fn walkable_at(&self, world: Vec2) -> bool {
        self.is_walkable(self.tile_of(world))
    }

    pub fn tiles(&self) -> impl Iterator<Item = (IVec2, Terrain)> + '_ {
        (0..self.height).flat_map(move |y| {
            (0..self.width).map(move |x| {
                let tile = IVec2::new(x, y);
                (tile, self.get(tile).unwrap())
            })
        })
    }
}

/// Seeded 2D value noise with fractal layering. Enough for a placeholder map; swap for the
/// `noise` crate later if nicer shapes are wanted.
#[derive(Clone, Copy, Debug)]
pub struct Noise {
    seed: u64,
}

impl Noise {
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }

    fn lattice(&self, x: i32, y: i32) -> f32 {
        let mut h = (x as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (y as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ self.seed;
        h ^= h >> 31;
        h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
        h ^= h >> 29;
        (h & 0x00FF_FFFF) as f32 / 0x00FF_FFFF as f32
    }

    /// Smoothly interpolated noise in `0.0..=1.0`.
    pub fn value(&self, x: f32, y: f32) -> f32 {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
        let (ix, iy) = (x0 as i32, y0 as i32);
        let top = self.lattice(ix, iy) + (self.lattice(ix + 1, iy) - self.lattice(ix, iy)) * sx;
        let bottom = self.lattice(ix, iy + 1)
            + (self.lattice(ix + 1, iy + 1) - self.lattice(ix, iy + 1)) * sx;
        top + (bottom - top) * sy
    }

    /// Several octaves of `value` summed, normalised to `0.0..=1.0`.
    pub fn fbm(&self, x: f32, y: f32, octaves: u32) -> f32 {
        let (mut total, mut amplitude, mut frequency, mut max) = (0.0, 1.0, 1.0, 0.0);
        for _ in 0..octaves {
            total += self.value(x * frequency, y * frequency) * amplitude;
            max += amplitude;
            amplitude *= 0.5;
            frequency *= 2.0;
        }
        total / max
    }
}

// ---------- building ----------

/// Build the map from the text pipeline (`worldgen::build`) and place the ramps.
pub fn generate(seed: u64, width: i32, height: i32) -> TerrainMap {
    from_grid_with_ramps(&worldgen::build(seed, width, height), seed)
}

/// Path of the seed file for `seed`.
pub fn seed_path(seed: u64) -> std::path::PathBuf {
    std::path::Path::new(SEEDS_DIR).join(format!("{seed}.txt"))
}

/// Load `assets/seeds/<seed>.txt` if it exists and has the right size, else generate.
pub fn load_or_generate(seed: u64, width: i32, height: i32) -> TerrainMap {
    let path = seed_path(seed);
    match std::fs::read_to_string(&path) {
        Ok(text) => match Grid::from_text(&text) {
            Ok(grid) if grid.width == width && grid.height == height => {
                info!("loaded seed file {}", path.display());
                from_grid_with_ramps(&grid, seed)
            }
            Ok(grid) => {
                warn!(
                    "{} is {}x{}, expected {width}x{height}; generating instead",
                    path.display(),
                    grid.width,
                    grid.height
                );
                generate(seed, width, height)
            }
            Err(e) => {
                warn!("{}: {e}; generating instead", path.display());
                generate(seed, width, height)
            }
        },
        Err(_) => generate(seed, width, height),
    }
}

/// The map of `grid` with its ramps checked and completed (`TerrainMap::ensure_ramps`).
pub fn from_grid_with_ramps(grid: &Grid, seed: u64) -> TerrainMap {
    let mut map = TerrainMap::from_grid(grid);
    let mut rng = StdRng::seed_from_u64(seed ^ 0x7A3B_5C1D);
    let report = map.ensure_ramps(&mut rng);
    for t in report.dropped {
        warn!("ramp at {t} does not fit there (needs a tile one layer higher east or west); dropped");
    }
    for (layer, t, size, filled) in report.reshaped {
        let how = if filled { "filled up one layer" } else { "lowered one layer" };
        info!("layer {layer} plateau at {t} ({size} tiles) had no room for a ramp: {how}");
    }
    for (layer, t, size) in map.unreached_regions() {
        warn!("layer {layer} plateau at {t} ({size} tiles) has no room for a ramp; it cannot be reached");
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic_and_in_range() {
        let a = Noise::new(42);
        let b = Noise::new(42);
        for i in 0..200 {
            let (x, y) = (i as f32 * 0.37, i as f32 * 0.11);
            let v = a.fbm(x, y, 4);
            assert_eq!(v, b.fbm(x, y, 4));
            assert!((0.0..=1.0).contains(&v));
        }
        assert_ne!(Noise::new(1).value(3.3, 4.4), Noise::new(2).value(3.3, 4.4));
    }

    #[test]
    fn world_and_tile_coordinates_round_trip() {
        let map = TerrainMap::filled(10, 6, Terrain::Grass);
        for (tile, _) in map.tiles() {
            assert_eq!(map.tile_of(map.world_pos(tile)), tile);
        }
        assert_eq!(map.tile_of(Vec2::ZERO), IVec2::new(5, 3));
    }

    /// A map from text, top row first.
    fn map_of(text: &str) -> TerrainMap {
        TerrainMap::from_grid(&Grid::from_text(text).unwrap())
    }

    #[test]
    fn ramps_sit_east_or_west_of_the_higher_tile() {
        // The 1 2 3 / 4 5 6 case: 5 is higher; a ramp fits on 4 or 6, never on 2 above it.
        let map = map_of("GGGGG\nGG2GG\nGGGGG\n");
        let five = IVec2::new(2, 1);
        assert_eq!(map.ramp_fits(five - IVec2::X), Some(RampSide::Left), "4: climbs east");
        assert_eq!(map.ramp_fits(five + IVec2::X), Some(RampSide::Right), "6: climbs west");
        assert_eq!(map.ramp_fits(five + IVec2::Y), None, "2: never north of it");
        assert_eq!(map.ramp_fits(five - IVec2::Y), None, "nor south of it");

        // Under a plateau the tile is a cliff wall: no ramp there, nor beside the wall row.
        let map = map_of("GG222G\nGGGGGG\nGGGGGG\n");
        assert_eq!(map.ramp_fits(IVec2::new(1, 2)), Some(RampSide::Left), "beside the plateau's corner");
        assert_eq!(map.ramp_fits(IVec2::new(2, 1)), None, "the wall itself");
        assert_eq!(map.ramp_fits(IVec2::new(1, 1)), None, "nothing higher beside it");

        // Squeezed between two higher tiles, over water or with no ground below: no ramp.
        assert_eq!(map_of("2G2\nGGG\n").ramp_fits(IVec2::new(1, 1)), None);
        assert_eq!(map_of("WG2\nWWG\n").ramp_fits(IVec2::new(1, 1)), None, "water below");
        assert_eq!(map_of("WG2\nGGG\n").ramp_fits(IVec2::new(1, 1)), None, "water on the open side");
        assert_eq!(map_of("G2\nGG\n").ramp_fits(IVec2::new(0, 1)), None, "map edge on the open side");
        assert_eq!(map_of("33G\n2RG\n22G\n").ramp_fits(IVec2::new(1, 1)), None, "a ramp is never under a higher tile");
    }

    #[test]
    fn ramps_are_walked_from_every_side_but_climbed_sideways() {
        let mut map = map_of("GGGGG\nGG222\nGGGGG\nGGGGG\n");
        let ramp = IVec2::new(1, 2);
        let top = IVec2::new(2, 2);
        let open = IVec2::new(0, 2);
        assert!(!map.can_step(ramp, top), "no ramp yet");
        assert!(map.add_ramp(ramp));
        assert_eq!(map.ramp_top(ramp), Some(top));
        assert!(map.can_step(open, ramp) && map.can_step(ramp, open), "on and off from the open side");
        assert!(map.can_step(ramp, top) && map.can_step(top, ramp), "up and down the slope");
        for d in [IVec2::Y, IVec2::NEG_Y] {
            assert!(map.can_step(ramp + d, ramp), "onto it from the north and south");
            assert!(map.can_step(ramp, ramp + d), "and off it again");
        }
        assert!(!map.can_step(IVec2::new(2, 3), top), "no other way up");
        assert!(!map.can_step(IVec2::new(2, 1), top), "the wall is solid");
        assert!(!map.is_walkable(IVec2::new(2, 1)));
        assert!(map.is_ramp_entry(open) && map.is_edge_art(IVec2::new(1, 1)));
        assert!(!map.add_ramp(IVec2::new(1, 3)), "never next to another ramp");
        assert!(!map.can_step(IVec2::new(4, 3), IVec2::new(4, 2)), "a plain plateau edge is closed");
    }

    #[test]
    fn ramps_round_trip_and_bad_ones_are_dropped() {
        let text = "GGGGG\nGR222\nGGGGG\n";
        let mut map = map_of(text);
        assert!(map.ensure_ramps(&mut StdRng::seed_from_u64(1)).dropped.is_empty());
        assert_eq!(map.ramp_at(IVec2::new(1, 1)), Some(RampSide::Left));
        assert_eq!(map.layer(IVec2::new(1, 1)), Some(1));
        assert_eq!(map.to_grid().to_text(), text);

        // A hand-written R with nothing higher beside it is dropped; the plateau then gets one.
        let mut map = map_of("RGGGG\nGG222\nGGGGG\n");
        let report = map.ensure_ramps(&mut StdRng::seed_from_u64(1));
        assert_eq!(report.dropped, vec![IVec2::new(0, 2)]);
        assert_eq!(map.ramps().count(), 1);
        assert!(map.unreached_regions().is_empty());
    }

    #[test]
    fn plateaus_no_ramp_can_reach_are_reshaped() {
        // Grass only north and south of the mountain (map edge west, water east): no ramp fits,
        // so it is lowered, and its layer-3 top with it.
        let mut map = map_of("GGGGGGW\n222222W\n223322W\n222222W\nGGGGGGW\n");
        let report = map.ensure_ramps(&mut StdRng::seed_from_u64(1));
        assert_eq!(report.reshaped.len(), 1);
        assert!(!report.reshaped[0].3, "lowered");
        assert_eq!(map.layer(IVec2::new(0, 2)), Some(1));
        assert_eq!(map.layer(IVec2::new(2, 2)), Some(2), "the top came down with it");
        assert!(map.unreached_regions().is_empty());
        assert_eq!(map.ramps().count(), 1, "and the old top got its ramp");

        // A layer-2 tile shut in by layer-3 ground and a wall (the tile north of it) is filled
        // up instead.
        let mut map = map_of(
            "GGGGGGGGGGG\nGG2222222GG\nGG2233322GG\nGG2232322GG\nGG2232322GG\nGG2233322GG\nGG2222222GG\nGGGGGGGGGGG\n",
        );
        let report = map.ensure_ramps(&mut StdRng::seed_from_u64(1));
        assert!(report.reshaped.iter().any(|r| r.3), "filled: {report:?}");
        assert_eq!(map.layer(IVec2::new(5, 3)), Some(3));
        assert!(map.unreached_regions().is_empty());
    }

    #[test]
    fn paths_avoid_water_and_use_ramps() {
        let mut map = map_of("GG22WGG\nGG22WGG\nGGGGWGG\nGGGGWGG\nGGGGWGG\nGGGGGGG\n");
        assert!(map.add_ramp(IVec2::new(1, 4)));

        let path = map
            .find_path(IVec2::new(0, 5), IVec2::new(6, 5))
            .expect("around the channel");
        assert!(path.iter().all(|t| map.is_land(*t)));
        assert!(path.contains(&IVec2::new(4, 0)), "must cross at the gap");
        assert_eq!(*path.last().unwrap(), IVec2::new(6, 5));

        let up = map
            .find_path(IVec2::new(3, 1), IVec2::new(3, 5))
            .expect("up the ramp");
        assert!(
            up.contains(&IVec2::new(1, 4)) && up.contains(&IVec2::new(2, 4)),
            "only way up is the ramp: {up:?}"
        );

        let mut island = TerrainMap::filled(5, 5, Terrain::Water);
        island.set(IVec2::new(0, 0), Terrain::Grass);
        island.set(IVec2::new(4, 4), Terrain::Grass);
        assert!(
            island
                .find_path(IVec2::new(0, 0), IVec2::new(4, 4))
                .is_none()
        );
        assert_eq!(
            map.find_path(IVec2::new(1, 1), IVec2::new(1, 1)),
            Some(vec![])
        );
    }

    #[test]
    fn grid_round_trip_and_ramps_on_built_maps() {
        let grid = Grid::from_text("GGGG\n2222\nGGGG\nWWGG\n").unwrap();
        let map = TerrainMap::from_grid(&grid);
        assert_eq!(map.layer(IVec2::new(0, 0)), Some(0));
        assert_eq!(map.layer(IVec2::new(0, 2)), Some(2));
        assert_eq!(map.to_grid(), grid);

        for seed in 1..=12u64 {
            let map = generate(seed, 64, 48);
            assert_eq!(
                map.to_grid(),
                generate(seed, 64, 48).to_grid(),
                "deterministic (seed {seed})"
            );
            assert_eq!(
                TerrainMap::from_grid(&map.to_grid()).to_grid(),
                map.to_grid(),
                "ramps survive the text (seed {seed})"
            );
            for (ramp, side) in map.ramps() {
                assert_eq!(map.ramp_fits(ramp), Some(side), "ramp at {ramp} (seed {seed})");
            }
            // Every place the layer goes up can be climbed, unless no ramp fits anywhere.
            for (layer, tile, _) in map.unreached_regions() {
                let region = map
                    .plateau_regions()
                    .into_iter()
                    .find(|r| r[0] == tile)
                    .unwrap();
                let reached = map.reachable();
                assert!(
                    region
                        .iter()
                        .flat_map(|t| [*t + IVec2::X, *t + IVec2::NEG_X])
                        .all(|t| !reached.contains(&t) || map.ramp_fits(t).is_none()),
                    "layer {layer} plateau at {tile} could have had a ramp (seed {seed})"
                );
            }
            assert!(
                map.flat_patches().len() >= 4,
                "only {} flat patches for bases (seed {seed})",
                map.flat_patches().len()
            );
        }
    }

    /// The ramp rules over many seeds: `cargo test terrain_100_seeds -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn terrain_100_seeds() {
        let mut unreached = 0;
        for seed in 1..=100u64 {
            let map = generate(seed, 64, 48);
            for (ramp, side) in map.ramps() {
                assert_eq!(map.ramp_fits(ramp), Some(side), "ramp at {ramp} (seed {seed})");
                let top = map.ramp_top(ramp).unwrap();
                assert!(map.can_step(ramp, top) && map.can_step(top, ramp), "seed {seed} ramp {ramp}");
            }
            for (layer, tile, size) in map.unreached_regions() {
                println!("seed {seed}: layer {layer} plateau at {tile} ({size} tiles) unreachable");
                unreached += 1;
            }
            assert!(map.flat_patches().len() >= 4, "seed {seed}: too few flat patches");
            assert_eq!(TerrainMap::from_grid(&map.to_grid()).to_grid(), map.to_grid());
        }
        assert_eq!(unreached, 0, "plateaus nobody can reach");
    }

    /// Tuning aid: `cargo test terrain_stats -- --ignored --nocapture` (or `cargo run -- map <seed>`).
    #[test]
    #[ignore]
    fn terrain_stats() {
        for seed in 1..=12u64 {
            let map = generate(seed, 64, 48);
            println!(
                "seed {seed:2}: {} | flat patches {} | ramps {}",
                map.to_grid().stats(),
                map.flat_patches().len(),
                map.ramps().count()
            );
        }
    }
}
