//! Terrain tiles, layers, stairs and the noise that generates them. See agents.md §1.6.
//! Pure data, no Bevy systems, so it can be unit-tested.

use std::collections::HashSet;

use bevy::prelude::*;

/// World size of one overworld tile in pixels (Tiny Swords tiles are 64×64).
pub const TILE: f32 = 64.0;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Terrain {
    Water,
    Grass,
    /// High ground: a plateau one layer above the grass, reachable only by stairs.
    Mountain,
}

impl Terrain {
    /// Elevation layer. Water is 0 and every step up adds one.
    pub fn layer(self) -> u8 {
        match self {
            Terrain::Water => 0,
            Terrain::Grass => 1,
            Terrain::Mountain => 2,
        }
    }

    pub fn walkable(self) -> bool {
        self != Terrain::Water
    }
}

/// A rectangular grid of terrain. Tile (0,0) is the bottom-left corner of the world.
#[derive(Clone, Debug)]
pub struct TerrainMap {
    pub width: i32,
    pub height: i32,
    tiles: Vec<Terrain>,
    /// Grass tiles directly south of a mountain that carry a staircase up to it.
    stairs: HashSet<IVec2>,
}

impl TerrainMap {
    pub fn filled(width: i32, height: i32, terrain: Terrain) -> Self {
        Self { width, height, tiles: vec![terrain; (width * height) as usize], stairs: HashSet::new() }
    }

    pub fn in_bounds(&self, tile: IVec2) -> bool {
        (0..self.width).contains(&tile.x) && (0..self.height).contains(&tile.y)
    }

    pub fn get(&self, tile: IVec2) -> Option<Terrain> {
        self.in_bounds(tile)
            .then(|| self.tiles[(tile.y * self.width + tile.x) as usize])
    }

    pub fn set(&mut self, tile: IVec2, terrain: Terrain) {
        if self.in_bounds(tile) {
            self.tiles[(tile.y * self.width + tile.x) as usize] = terrain;
        }
    }

    pub fn layer(&self, tile: IVec2) -> Option<u8> {
        self.get(tile).map(Terrain::layer)
    }

    pub fn is_stairs(&self, tile: IVec2) -> bool {
        self.stairs.contains(&tile)
    }

    /// Mark a grass tile as a staircase. The tile north of it must be a mountain.
    pub fn add_stairs(&mut self, tile: IVec2) -> bool {
        let ok = self.get(tile) == Some(Terrain::Grass) && self.get(tile + IVec2::Y) == Some(Terrain::Mountain);
        if ok {
            self.stairs.insert(tile);
        }
        ok
    }

    pub fn stairs(&self) -> impl Iterator<Item = IVec2> + '_ {
        self.stairs.iter().copied()
    }

    /// Can something walk from `from` onto `to`? Same layer is always fine (never water);
    /// changing layer is only possible between a staircase and the mountain tile above it.
    pub fn can_step(&self, from: IVec2, to: IVec2) -> bool {
        let (Some(a), Some(b)) = (self.get(from), self.get(to)) else { return false };
        if !b.walkable() {
            return false;
        }
        if a.layer() == b.layer() {
            return true;
        }
        let up = to - from == IVec2::Y && self.is_stairs(from) && b == Terrain::Mountain;
        let down = from - to == IVec2::Y && self.is_stairs(to) && a == Terrain::Mountain;
        up || down
    }

    /// Which quarter of the map a tile is in: 0 top-left, 1 top-right, 2 bottom-left, 3 bottom-right.
    /// Each quarter draws its ground with a different tile sheet.
    pub fn quarter(&self, tile: IVec2) -> usize {
        let right = tile.x >= self.width / 2;
        let top = tile.y >= self.height / 2;
        match (top, right) {
            (true, false) => 0,
            (true, true) => 1,
            (false, false) => 2,
            (false, true) => 3,
        }
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
        self.get(self.tile_of(world)).is_some_and(Terrain::walkable)
    }

    pub fn tiles(&self) -> impl Iterator<Item = (IVec2, Terrain)> + '_ {
        (0..self.height).flat_map(move |y| {
            (0..self.width).map(move |x| {
                let tile = IVec2::new(x, y);
                (tile, self.get(tile).unwrap())
            })
        })
    }

    /// Connected groups of mountain tiles (4-neighbour flood fill).
    pub fn mountain_regions(&self) -> Vec<Vec<IVec2>> {
        let mut seen = HashSet::new();
        let mut regions = Vec::new();
        for (start, terrain) in self.tiles() {
            if terrain != Terrain::Mountain || seen.contains(&start) {
                continue;
            }
            let mut region = Vec::new();
            let mut stack = vec![start];
            seen.insert(start);
            while let Some(tile) = stack.pop() {
                region.push(tile);
                for d in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                    let next = tile + d;
                    if self.get(next) == Some(Terrain::Mountain) && seen.insert(next) {
                        stack.push(next);
                    }
                }
            }
            regions.push(region);
        }
        regions
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

/// Thresholds for the elevation layer.
const WATER_LEVEL: f32 = 0.40;
const MOUNTAIN_LEVEL: f32 = 0.66;

/// Layer 1 (elevation) decides water / grass / mountain. Layer 2 (moisture) is kept for
/// decoration density (`WorldState::generate`).
pub fn generate(seed: u64, width: i32, height: i32) -> TerrainMap {
    let elevation = Noise::new(seed);
    let mut map = TerrainMap::filled(width, height, Terrain::Grass);
    for y in 0..height {
        for x in 0..width {
            let e = elevation.fbm(x as f32 / 14.0, y as f32 / 14.0, 4);
            let terrain = if e < WATER_LEVEL {
                Terrain::Water
            } else if e > MOUNTAIN_LEVEL {
                Terrain::Mountain
            } else {
                Terrain::Grass
            };
            map.set(IVec2::new(x, y), terrain);
        }
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

    #[test]
    fn layers_water_grass_mountain() {
        assert_eq!(Terrain::Water.layer(), 0);
        assert_eq!(Terrain::Grass.layer(), 1);
        assert_eq!(Terrain::Mountain.layer(), 2);
        let map = generate(7, 64, 48);
        let kinds: HashSet<_> = map.tiles().map(|(_, t)| t).collect();
        assert_eq!(kinds.len(), 3, "expected water, grass and mountain, got {kinds:?}");
    }

    #[test]
    fn layer_changes_only_through_stairs() {
        let mut map = TerrainMap::filled(5, 5, Terrain::Grass);
        map.set(IVec2::new(2, 3), Terrain::Mountain);
        map.set(IVec2::new(3, 3), Terrain::Mountain);
        map.set(IVec2::new(0, 0), Terrain::Water);
        let grass_below = IVec2::new(2, 2);
        let mountain = IVec2::new(2, 3);

        assert!(!map.can_step(grass_below, mountain), "no stairs yet");
        assert!(!map.can_step(IVec2::new(3, 2), IVec2::new(3, 3)));
        assert!(!map.can_step(IVec2::new(1, 0), IVec2::new(0, 0)), "water is never enterable");
        assert!(map.can_step(IVec2::new(1, 1), IVec2::new(2, 1)), "same layer is fine");

        assert!(map.add_stairs(grass_below));
        assert!(!map.add_stairs(IVec2::new(4, 4)), "stairs need a mountain to the north");
        assert!(map.can_step(grass_below, mountain), "up the stairs");
        assert!(map.can_step(mountain, grass_below), "down the stairs");
        assert!(map.can_step(mountain, IVec2::new(3, 3)), "walking on the plateau");
        assert!(!map.can_step(IVec2::new(3, 3), IVec2::new(3, 2)), "no jumping off the edge");
        assert!(!map.can_step(IVec2::new(1, 2), mountain), "no diagonal shortcut");
    }

    #[test]
    fn quarters_and_regions() {
        let mut map = TerrainMap::filled(8, 8, Terrain::Grass);
        assert_eq!(map.quarter(IVec2::new(0, 7)), 0);
        assert_eq!(map.quarter(IVec2::new(7, 7)), 1);
        assert_eq!(map.quarter(IVec2::new(0, 0)), 2);
        assert_eq!(map.quarter(IVec2::new(7, 0)), 3);
        for t in [IVec2::new(1, 1), IVec2::new(2, 1), IVec2::new(6, 6)] {
            map.set(t, Terrain::Mountain);
        }
        let regions = map.mountain_regions();
        assert_eq!(regions.len(), 2);
        assert_eq!(regions.iter().map(Vec::len).sum::<usize>(), 3);
    }
}
