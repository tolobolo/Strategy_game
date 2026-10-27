//! Everything about the map that must survive a trip into combat: terrain, bases, towers,
//! sellers, decorations, roaming armies and the hero's position. See agents.md §1.

use bevy::prelude::*;
use rand::rngs::StdRng;
use rand::seq::IndexedRandom;
use rand::{Rng, SeedableRng};

use super::armies::{self, AiArmy, BanditSquad};
use super::buildings::{BASE_SIZE, BASE_SPAWN_OFFSET, BuildingKind, base_layout};
use super::economy::Treasuries;
use super::terrain::{self, Noise, Terrain, TerrainMap};
use crate::faction::Faction;
use crate::units::{Army, UnitKind};

pub const MAP_WIDTH: i32 = 64;
pub const MAP_HEIGHT: i32 = 48;
pub const TOWER_COUNT: usize = 6;
/// Base centres start this many tiles apart (relaxed slowly if the map is too cramped).
const BASE_MIN_DISTANCE: i32 = 14;
/// Towers and sellers keep this far from base centres and from each other.
const FEATURE_BASE_DISTANCE: i32 = 6;
const FEATURE_MIN_DISTANCE: i32 = 5;

/// What the hero bumped into to start a fight. Index into the matching `WorldState` list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncounterSource {
    Base(usize),
    Tower(usize),
    AiArmy(usize),
    Bandits(usize),
}

/// Set on the overworld, consumed by the combat plugin.
#[derive(Resource, Clone, Debug)]
pub struct PendingEncounter {
    pub enemy: Army,
    pub source: EncounterSource,
}

/// A 5×4 kingdom base.
#[derive(Clone, Debug)]
pub struct Base {
    /// Who built it (never changes).
    pub home: Faction,
    /// Who holds it now.
    pub owner: Faction,
    /// Set when the owner just changed; the overworld plays the capture effect on every
    /// building of the base, swaps the sprites and clears it.
    pub prev_owner: Option<Faction>,
    /// Bottom-left tile.
    pub origin: IVec2,
    /// Units stored in the castle (max `CASTLE_SLOTS`). This is also what defends the castle.
    pub garrison: Army,
}

impl Base {
    pub fn tiles(&self) -> impl Iterator<Item = IVec2> + '_ {
        (0..BASE_SIZE.y).flat_map(move |y| (0..BASE_SIZE.x).map(move |x| self.origin + IVec2::new(x, y)))
    }

    pub fn contains(&self, tile: IVec2) -> bool {
        let d = tile - self.origin;
        (0..BASE_SIZE.x).contains(&d.x) && (0..BASE_SIZE.y).contains(&d.y)
    }

    pub fn spawn_tile(&self) -> IVec2 {
        self.origin + BASE_SPAWN_OFFSET
    }

    pub fn centre(&self) -> IVec2 {
        self.origin + BASE_SIZE / 2
    }

    /// Buildings with their bottom-left tile in world tiles.
    pub fn buildings(&self) -> impl Iterator<Item = (BuildingKind, IVec2)> + '_ {
        base_layout().into_iter().map(move |(kind, offset)| (kind, self.origin + offset))
    }
}

/// A lone tower. Black at first; whoever holds it gets its income.
#[derive(Clone, Debug)]
pub struct Tower {
    pub owner: Faction,
    /// Set when the owner just changed; the overworld plays the capture effect, swaps the
    /// sprite and clears it.
    pub prev_owner: Option<Faction>,
    pub tile: IVec2,
    /// Units defending it: Black's own at first, later whatever an AI army left behind.
    pub garrison: Army,
}

impl Tower {
    /// Hand the tower to `new_owner`, remembering who held it for the capture animation.
    pub fn change_owner(&mut self, new_owner: Faction, garrison: Army) {
        if self.owner != new_owner {
            self.prev_owner = Some(self.prev_owner.unwrap_or(self.owner));
        }
        self.owner = new_owner;
        self.garrison = garrison;
    }
}

/// An idle unit standing in the wild that hires out its own kind.
#[derive(Clone, Debug)]
pub struct Seller {
    pub unit: UnitKind,
    pub tile: IVec2,
}

/// Purely visual things scattered over the map. The `u8` picks a variant sheet (0-based).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecorationKind {
    Tree(u8),
    Bush(u8),
    Rock(u8),
    WaterRock(u8),
    Duck,
}

#[derive(Clone, Copy, Debug)]
pub struct Decoration {
    pub kind: DecorationKind,
    pub tile: IVec2,
}

#[derive(Resource, Debug)]
pub struct WorldState {
    /// Kept so a map can be reproduced from a log line.
    #[allow(dead_code)]
    pub seed: u64,
    pub terrain: TerrainMap,
    /// Index 0 is always the player's home base.
    pub bases: Vec<Base>,
    pub towers: Vec<Tower>,
    pub sellers: Vec<Seller>,
    pub decorations: Vec<Decoration>,
    pub armies: Vec<AiArmy>,
    pub bandits: Vec<BanditSquad>,
    pub hero_pos: Vec2,
}

/// What every castle starts with: 10 knights, 5 spear guards, 10 archers.
pub fn starting_garrison(faction: Faction) -> Army {
    Army::new(faction, &[(UnitKind::Warrior, 10), (UnitKind::Lancer, 5), (UnitKind::Archer, 10)])
}

impl WorldState {
    /// The AI army standing at tower `i` taking it (or watching its capture), if any. While
    /// that lasts the tower cannot be entered.
    pub fn tower_being_taken_by(&self, i: usize) -> Option<Faction> {
        use super::armies::AiArmyState;
        self.armies
            .iter()
            .find(|a| a.alive && a.target == Some(i) && matches!(a.state, AiArmyState::Capturing(_) | AiArmyState::Leaving(_)))
            .map(|a| a.faction)
    }

    pub fn generate(seed: u64) -> Self {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut terrain = terrain::generate(seed, MAP_WIDTH, MAP_HEIGHT);
        let bases = place_bases(&mut terrain, &mut rng);
        place_stairs(&mut terrain, &bases, &mut rng);

        // Layer 3: towers and sellers prefer the peaks of a third noise field.
        let features = Noise::new(seed ^ 0xA5A5_5A5A_0F0F_F0F0);
        let mut taken: Vec<IVec2> = terrain.stairs().collect();
        let towers: Vec<Tower> =
            pick_feature_tiles(&terrain, &bases, &features, &mut rng, TOWER_COUNT, false, &mut taken)
                .into_iter()
                .map(|tile| Tower { owner: Faction::Black, prev_owner: None, tile, garrison: armies::tower_garrison(&mut rng) })
                .collect();
        let sellers: Vec<Seller> =
            pick_feature_tiles(&terrain, &bases, &features, &mut rng, UnitKind::ALL.len(), true, &mut taken)
                .into_iter()
                .zip(UnitKind::ALL)
                .map(|(tile, unit)| Seller { unit, tile })
                .collect();

        let moisture = Noise::new(seed.wrapping_add(0x51ED_270B));
        let decorations = place_decorations(&terrain, &bases, &taken, &moisture, &mut rng);

        let mut world = Self {
            seed,
            terrain,
            bases,
            towers,
            sellers,
            decorations,
            armies: Vec::new(),
            bandits: Vec::new(),
            hero_pos: Vec2::ZERO,
        };
        world.hero_pos = world.hero_home_pos();
        world.armies = armies::initial_ai_armies(&world, &mut rng);
        world.bandits = armies::initial_bandits(&world, &mut rng);
        world
    }

    pub fn home_base(&self) -> &Base {
        &self.bases[0]
    }

    pub fn hero_home_pos(&self) -> Vec2 {
        self.terrain.world_pos(self.home_base().spawn_tile())
    }

    pub fn base_of(&self, faction: Faction) -> Option<&Base> {
        self.bases.iter().find(|b| b.home == faction)
    }

    pub fn in_any_base(&self, tile: IVec2) -> bool {
        self.bases.iter().any(|b| b.contains(tile))
    }

    pub fn castles_owned(&self) -> usize {
        self.bases.iter().filter(|b| b.owner.is_player()).count()
    }

    pub fn towers_owned(&self) -> usize {
        self.towers.iter().filter(|t| t.owner.is_player()).count()
    }

    /// The game is won when every castle is yours.
    pub fn land_united(&self) -> bool {
        self.bases.iter().all(|b| b.owner.is_player())
    }

    /// Who defends this source, and who gets half your gold if you lose to it.
    pub fn faction_of(&self, source: EncounterSource) -> Faction {
        match source {
            EncounterSource::Base(i) => self.bases[i].owner,
            EncounterSource::Tower(i) => self.towers[i].owner,
            EncounterSource::AiArmy(i) => self.armies[i].faction,
            EncounterSource::Bandits(_) => Faction::Black,
        }
    }

    pub fn enemy_army_for(&self, source: EncounterSource) -> Army {
        match source {
            EncounterSource::Base(i) => self.bases[i].garrison.clone(),
            EncounterSource::Tower(i) => self.towers[i].garrison.clone(),
            EncounterSource::AiArmy(i) => self.armies[i].army.clone(),
            EncounterSource::Bandits(i) => self.bandits[i].army.clone(),
        }
    }

    /// Apply a player victory. Returns the loser and the gold looted.
    pub fn capture(&mut self, source: EncounterSource, treasuries: &mut Treasuries) -> (Faction, u32) {
        match source {
            EncounterSource::Base(i) => {
                let loser = self.bases[i].owner;
                let base = &mut self.bases[i];
                base.prev_owner = Some(base.prev_owner.unwrap_or(loser));
                base.owner = Faction::Reclaimers;
                base.garrison = Army::empty(Faction::Reclaimers);
                // A kingdom without a castle has nowhere to raise armies from.
                for army in self.armies.iter_mut().filter(|a| a.faction == loser) {
                    army.alive = false;
                    army.respawn_in = f32::INFINITY;
                }
                (loser, treasuries.take_half(loser))
            }
            EncounterSource::Tower(i) => {
                let loser = self.towers[i].owner;
                self.towers[i].change_owner(Faction::Reclaimers, Army::empty(Faction::Reclaimers));
                (loser, treasuries.take_half(loser))
            }
            EncounterSource::AiArmy(i) => {
                let army = &mut self.armies[i];
                army.alive = false;
                army.respawn_in = armies::AI_RESPAWN_SECS;
                army.army = Army::empty(army.faction);
                (army.faction, treasuries.take_half(army.faction))
            }
            EncounterSource::Bandits(i) => {
                let squad = &mut self.bandits[i];
                squad.alive = false;
                squad.respawn_in = armies::BANDIT_RESPAWN_SECS;
                let loot = std::mem::take(&mut squad.gold);
                (Faction::Black, loot)
            }
        }
    }

    /// A random walkable tile outside every base and at least `min_tiles` from `away_from`.
    pub fn random_walkable_tile(&self, rng: &mut impl Rng, away_from: IVec2, min_tiles: i32) -> IVec2 {
        for _ in 0..2000 {
            let tile = IVec2::new(rng.random_range(0..self.terrain.width), rng.random_range(0..self.terrain.height));
            let ok = self.terrain.get(tile).is_some_and(Terrain::walkable)
                && !self.in_any_base(tile)
                && chebyshev(tile, away_from) >= min_tiles;
            if ok {
                return tile;
            }
        }
        self.home_base().spawn_tile()
    }
}

fn chebyshev(a: IVec2, b: IVec2) -> i32 {
    (a - b).abs().max_element()
}

/// One base per kingdom, spaced out, on grass only (so the hero can always reach it), then
/// flattened to grass.
fn place_bases(terrain: &mut TerrainMap, rng: &mut impl Rng) -> Vec<Base> {
    let mut bases: Vec<Base> = Vec::new();
    for faction in Faction::KINGDOMS {
        let mut min_dist = BASE_MIN_DISTANCE;
        let mut origin = None;
        for attempt in 1..=6000 {
            if attempt % 400 == 0 && min_dist > 4 {
                min_dist -= 1;
            }
            let candidate = IVec2::new(
                rng.random_range(1..MAP_WIDTH - BASE_SIZE.x - 1),
                rng.random_range(1..MAP_HEIGHT - BASE_SIZE.y - 1),
            );
            let probe = Base { home: faction, owner: faction, prev_owner: None, origin: candidate, garrison: Army::empty(faction) };
            let flat = probe.tiles().all(|t| terrain.get(t) == Some(Terrain::Grass));
            let far = bases.iter().all(|b| chebyshev(b.centre(), probe.centre()) >= min_dist);
            if flat && far {
                origin = Some(candidate);
                break;
            }
        }
        // Fallback for a map that is almost all water or rock: build on it anyway.
        let origin = origin.unwrap_or_else(|| {
            IVec2::new(
                2 + (bases.len() as i32 % 2) * (MAP_WIDTH - BASE_SIZE.x - 4),
                2 + (bases.len() as i32 / 2) * (MAP_HEIGHT - BASE_SIZE.y - 4),
            )
        });
        let base = Base { home: faction, owner: faction, prev_owner: None, origin, garrison: starting_garrison(faction) };
        for tile in base.tiles() {
            terrain.set(tile, Terrain::Grass);
        }
        bases.push(base);
    }
    bases
}

/// Every mountain region gets one staircase: a grass tile right below one of its south edges.
fn place_stairs(terrain: &mut TerrainMap, bases: &[Base], rng: &mut impl Rng) {
    for region in terrain.mountain_regions() {
        let candidates: Vec<IVec2> = region
            .iter()
            .map(|&t| t - IVec2::Y)
            .filter(|&below| terrain.get(below) == Some(Terrain::Grass) && !bases.iter().any(|b| b.contains(below)))
            .collect();
        if let Some(&tile) = candidates.choose(rng) {
            terrain.add_stairs(tile);
        }
    }
}

/// Pick `count` tiles for towers or sellers: random candidates scored by the feature noise,
/// kept away from bases and from each other.
fn pick_feature_tiles(
    terrain: &TerrainMap,
    bases: &[Base],
    noise: &Noise,
    rng: &mut impl Rng,
    count: usize,
    grass_only: bool,
    taken: &mut Vec<IVec2>,
) -> Vec<IVec2> {
    let valid = |tile: IVec2| {
        let ground = terrain.get(tile);
        let walkable = if grass_only { ground == Some(Terrain::Grass) } else { ground.is_some_and(Terrain::walkable) };
        walkable
            && !terrain.is_stairs(tile)
            && terrain.get(tile + IVec2::Y) != Some(Terrain::Mountain) // keep off the cliff faces
            && bases.iter().all(|b| chebyshev(b.centre(), tile) >= FEATURE_BASE_DISTANCE && !b.contains(tile))
    };
    let mut candidates: Vec<(f32, IVec2)> = (0..800)
        .map(|_| IVec2::new(rng.random_range(0..terrain.width), rng.random_range(0..terrain.height)))
        .filter(|&t| valid(t))
        .map(|t| (noise.fbm(t.x as f32 / 9.0, t.y as f32 / 9.0, 3), t))
        .collect();
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));

    let mut picked = Vec::new();
    for (_, tile) in candidates {
        if picked.len() == count {
            break;
        }
        if taken.iter().all(|&t| chebyshev(t, tile) >= FEATURE_MIN_DISTANCE) {
            picked.push(tile);
            taken.push(tile);
        }
    }
    picked
}

/// Trees, bushes and rocks on land (denser trees where the moisture noise is high), rocks and
/// rubber ducks in the water. Never on bases, stairs, cliff faces or feature tiles.
fn place_decorations(
    terrain: &TerrainMap,
    bases: &[Base],
    taken: &[IVec2],
    moisture: &Noise,
    rng: &mut impl Rng,
) -> Vec<Decoration> {
    let mut out = Vec::new();
    for (tile, ground) in terrain.tiles() {
        let kind = match ground {
            Terrain::Water => {
                let roll: f32 = rng.random();
                if roll < 0.025 {
                    Some(DecorationKind::WaterRock(rng.random_range(0..4)))
                } else if roll < 0.030 {
                    Some(DecorationKind::Duck)
                } else {
                    None
                }
            }
            Terrain::Grass | Terrain::Mountain => {
                let blocked = bases.iter().any(|b| chebyshev(b.centre(), tile) <= 3)
                    || terrain.is_stairs(tile)
                    || terrain.get(tile + IVec2::Y) == Some(Terrain::Mountain)
                    || taken.contains(&tile);
                if blocked {
                    None
                } else {
                    let m = moisture.fbm(tile.x as f32 / 8.0, tile.y as f32 / 8.0, 3);
                    let roll: f32 = rng.random();
                    let tree_chance = if ground == Terrain::Grass { 0.01 + 0.10 * m * m } else { 0.02 };
                    if roll < tree_chance {
                        Some(DecorationKind::Tree(rng.random_range(0..4)))
                    } else if roll < tree_chance + 0.03 {
                        Some(DecorationKind::Bush(rng.random_range(0..4)))
                    } else if roll < tree_chance + 0.045 {
                        Some(DecorationKind::Rock(rng.random_range(0..4)))
                    } else {
                        None
                    }
                }
            }
        };
        if let Some(kind) = kind {
            out.push(Decoration { kind, tile });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_world_is_complete_and_spaced() {
        for seed in [1, 2, 3, 99] {
            let world = WorldState::generate(seed);
            assert_eq!(world.bases.len(), 4);
            assert_eq!(world.bases[0].home, Faction::Reclaimers);
            assert_eq!(world.towers.len(), TOWER_COUNT);
            assert_eq!(world.sellers.len(), UnitKind::ALL.len());
            assert_eq!(world.armies.len(), 3 * armies::AI_ARMIES_PER_KINGDOM);
            assert_eq!(world.bandits.len(), armies::BANDIT_COUNT);
            assert!(!world.decorations.is_empty());
            for base in &world.bases {
                assert!(base.tiles().all(|t| world.terrain.get(t) == Some(Terrain::Grass)));
            }
            for (i, a) in world.bases.iter().enumerate() {
                for b in &world.bases[i + 1..] {
                    assert!(chebyshev(a.centre(), b.centre()) >= 4, "bases too close (seed {seed})");
                }
            }
            assert!(world.terrain.walkable_at(world.hero_home_pos()));
            assert!(!world.land_united());
        }
    }

    #[test]
    fn every_mountain_region_with_a_south_edge_has_stairs() {
        let world = WorldState::generate(11);
        let stairs: Vec<IVec2> = world.terrain.stairs().collect();
        assert!(!stairs.is_empty());
        for tile in &stairs {
            assert_eq!(world.terrain.get(*tile), Some(Terrain::Grass));
            assert_eq!(world.terrain.get(*tile + IVec2::Y), Some(Terrain::Mountain));
            assert!(world.terrain.can_step(*tile, *tile + IVec2::Y));
        }
        for region in world.terrain.mountain_regions() {
            let has_south_grass = region.iter().any(|&t| {
                world.terrain.get(t - IVec2::Y) == Some(Terrain::Grass) && !world.in_any_base(t - IVec2::Y)
            });
            let has_stairs = region.iter().any(|&t| world.terrain.is_stairs(t - IVec2::Y));
            assert!(!has_south_grass || has_stairs, "a reachable region is missing its staircase");
        }
    }

    #[test]
    fn capture_changes_owner_and_loots() {
        let mut world = WorldState::generate(5);
        let mut treasuries = Treasuries::default();
        let red_gold = treasuries.gold(Faction::Red);
        let (loser, loot) = world.capture(EncounterSource::Base(1), &mut treasuries);
        assert_eq!(loser, Faction::Red);
        assert_eq!(loot, red_gold / 2);
        assert!(world.bases[1].owner.is_player());
        assert!(world.bases[1].garrison.is_empty());
        assert!(world.armies.iter().filter(|a| a.faction == Faction::Red).all(|a| !a.alive));
    }
}
