//! Everything about the map that must survive a trip into combat: terrain, bases, towers,
//! sellers, decorations, roaming armies and the hero's position. See agents.md §1.

use bevy::prelude::*;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};

use super::armies::{self, AiArmy, BanditSquad};
use super::buildings::{BASE_SIZE, BASE_SPAWN_OFFSET, BuildingKind, base_layout};
use super::economy::Treasuries;
use super::interact::Garrison;
use super::resources::{self, Site, Worker};
use super::terrain::{self, Noise, Terrain, TerrainMap};
use crate::faction::Faction;
use crate::units::{Army, UnitKind};

pub const MAP_WIDTH: i32 = 64;
pub const MAP_HEIGHT: i32 = 48;
pub const TOWER_COUNT: usize = 10;
/// Bases start this many patches apart (relaxed if the map is too cramped).
const BASE_MIN_PATCH_DISTANCE: i32 = 3;
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
    /// Set when you JOIN a siege: that castle's or tower's garrison fights on your side.
    pub defending: Option<Garrison>,
    /// AI armies (indices) that came to help the castle or tower you attack: they fight in
    /// `enemy` too, and are destroyed if you win.
    pub helpers: Vec<usize>,
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
impl Base {
    pub fn change_owner(&mut self, new_owner: Faction, garrison: Army) {
        if self.owner != new_owner {
            self.prev_owner = Some(self.prev_owner.unwrap_or(self.owner));
        }
        self.owner = new_owner;
        self.garrison = garrison;
    }
}

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
    /// Gold mines, forests and sheep pastures (agents.md §1.7).
    pub sites: Vec<Site>,
    /// Your Pawns working the sites; `alive == false` slots are reused.
    pub workers: Vec<Worker>,
    /// What you crafted at the House (agents.md §1.8).
    pub upgrades: crate::upgrades::Upgrades,
    /// What each AI kingdom has crafted (`ai::AI_LADDER`); missing = nothing yet.
    pub ai_craft: std::collections::HashMap<Faction, super::ai::AiCraft>,
    /// Your siege of a tower or castle in progress (`attack_panel`).
    pub player_siege: Option<super::attack_panel::PlayerSiege>,
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
            .find(|a| {
                a.alive
                    && a.target == Some(Garrison::Tower(i))
                    && matches!(a.state, AiArmyState::Capturing(_) | AiArmyState::Leaving(_))
            })
            .map(|a| a.faction)
    }

    /// What `faction` has crafted so far.
    pub fn ai_craft(&self, faction: Faction) -> super::ai::AiCraft {
        self.ai_craft.get(&faction).copied().unwrap_or_default()
    }

    /// The AI army besieging `place` right now (standing at it, counting down), if any.
    pub fn besieger(&self, place: Garrison) -> Option<usize> {
        self.besiegers(place).first().copied()
    }

    /// Every AI army besieging `place` right now (they speed the siege up together).
    pub fn besiegers(&self, place: Garrison) -> Vec<usize> {
        use super::armies::AiArmyState;
        (0..self.armies.len())
            .filter(|&i| {
                let a = &self.armies[i];
                a.alive && a.target == Some(place) && matches!(a.state, AiArmyState::Capturing(_))
            })
            .collect()
    }

    /// You lost the siege fight you joined: the attacker keeps its `survivors`, leaves part of
    /// them in `place` as its new garrison (like a capture by auto-resolve) and walks on.
    pub fn siege_lost(&mut self, army: usize, place: Garrison, survivors: Vec<crate::units::Stack>) {
        use super::armies::{AiArmyState, TOWER_LEAVE_SECS, TOWER_TAKE_LOSS};
        let mut rng = rand::rng();
        let attacker = &mut self.armies[army];
        attacker.army.stacks = survivors;
        let share = rng.random_range(TOWER_TAKE_LOSS.0..=TOWER_TAKE_LOSS.1);
        let left_behind = attacker.army.split_off_fraction(share);
        let faction = attacker.faction;
        attacker.target = None;
        attacker.state = AiArmyState::Leaving(TOWER_LEAVE_SECS);
        match place {
            Garrison::Castle(i) => self.bases[i].change_owner(faction, left_behind),
            Garrison::Tower(i) => self.towers[i].change_owner(faction, left_behind),
        }
    }

    pub fn generate(seed: u64) -> Self {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut terrain = terrain::load_or_generate(seed, MAP_WIDTH, MAP_HEIGHT);
        let bases = place_bases(&mut terrain, &mut rng);

        // Towers on the mountains and where they even things out; sellers prefer the peaks of a
        // separate noise field.
        let features = Noise::new(seed ^ 0xA5A5_5A5A_0F0F_F0F0);
        let mut taken: Vec<IVec2> = terrain.ramps().map(|(ramp, _)| ramp).collect();
        let towers: Vec<Tower> = place_towers(&terrain, &bases, &mut rng, &mut taken)
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
        let mut decorations = place_decorations(&terrain, &bases, &taken, &moisture, &mut rng);
        let sites = resources::place_sites(&terrain, &bases, &mut taken, &mut decorations, seed);

        let mut world = Self {
            seed,
            terrain,
            bases,
            towers,
            sellers,
            decorations,
            armies: Vec::new(),
            bandits: Vec::new(),
            sites,
            workers: Vec::new(),
            upgrades: Default::default(),
            player_siege: None,
            ai_craft: Default::default(),
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
        // Half the loser's gold, more with the crafted "Keep your money".
        let loot_pct = 50 + self.upgrades.loot_bonus_pct();
        match source {
            EncounterSource::Base(i) => {
                let loser = self.bases[i].owner;
                let base = &mut self.bases[i];
                base.prev_owner = Some(base.prev_owner.unwrap_or(loser));
                base.owner = Faction::Reclaimers;
                base.garrison = Army::empty(Faction::Reclaimers);
                // Only a kingdom left without any castle is beaten: its armies are gone for good.
                // One that still holds another castle keeps its armies (they now call that home).
                if !self.bases.iter().any(|b| b.owner == loser) {
                    for army in self.armies.iter_mut().filter(|a| a.faction == loser) {
                        army.alive = false;
                        army.ghost = false;
                        army.respawn_in = f32::INFINITY;
                    }
                }
                (loser, treasuries.take_pct(loser, loot_pct))
            }
            EncounterSource::Tower(i) => {
                let loser = self.towers[i].owner;
                self.towers[i].change_owner(Faction::Reclaimers, Army::empty(Faction::Reclaimers));
                (loser, treasuries.take_pct(loser, loot_pct))
            }
            EncounterSource::AiArmy(i) => {
                let army = &mut self.armies[i];
                army.die();
                (army.faction, treasuries.take_pct(army.faction, loot_pct))
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
            let ok = self.terrain.is_walkable(tile)
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

/// One base per kingdom, each on its own flat patch-sized area of grass (`flat_spots`, at any
/// tile), spaced apart in patches (`patch_gap`) and relaxed if the map is cramped.
fn place_bases(terrain: &mut TerrainMap, rng: &mut impl Rng) -> Vec<Base> {
    let count = terrain.patch_count();
    let mut free: Vec<IVec2> = terrain.flat_spots();
    free.shuffle(rng);
    let mut chosen: Vec<IVec2> = Vec::new();
    let mut min_dist = BASE_MIN_PATCH_DISTANCE;
    while chosen.len() < Faction::KINGDOMS.len() {
        let next = free
            .iter()
            .copied()
            .find(|p| chosen.iter().all(|c| terrain::patch_gap(*c, *p) >= min_dist));
        match next {
            Some(p) => chosen.push(p),
            None if min_dist > 1 => min_dist -= 1,
            None => {
                // Not enough flat grass at all: take any free patch and flatten it.
                let any = (0..count.y)
                    .flat_map(|y| (0..count.x).map(move |x| TerrainMap::patch_origin(IVec2::new(x, y))))
                    .find(|o| chosen.iter().all(|c| terrain::patch_gap(*c, *o) >= 1))
                    .unwrap_or(IVec2::ZERO);
                for tile in TerrainMap::patch_tiles(any / terrain::PATCH) {
                    terrain.set(tile, Terrain::Grass);
                }
                chosen.push(any);
            }
        }
    }

    Faction::KINGDOMS
        .into_iter()
        .zip(chosen)
        .map(|(faction, origin)| {
            let base = Base { home: faction, owner: faction, prev_owner: None, origin, garrison: starting_garrison(faction) };
            for tile in base.tiles() {
                terrain.set(tile, Terrain::Grass);
            }
            base
        })
        .collect()
}

/// Towers on the two biggest mountains...
const MOUNTAINS_WITH_TOWERS: usize = 2;
/// ...two on each, on the plateau top...
const TOWERS_PER_MOUNTAIN: usize = 2;
/// The other towers stand this far (tiles, min and max) from the castle that has the fewest
/// towers within `FAIR_RADIUS` tiles, so every kingdom gets its share.
const FAIR_DISTANCE: (i32, i32) = (8, 12);
const FAIR_RADIUS: i32 = 16;

/// Where the `TOWER_COUNT` towers stand:
/// - two on each of the two biggest mountains (connected tiles of layer 2 and up), on the plateau
///   top, as far apart as that mountain allows, so they sit on opposite sides of it;
/// - the rest on open grass 8-12 tiles from the castle with the fewest towers near it (ties at
///   random), so the kingdoms come out even.
///
/// Every tower can be walked to from your castle (ramps included), keeps off ramps, cliff faces and
/// plateau edges, 6 tiles from castles and 5 from other towers. Fewer mountains (or no room on
/// them) means more towers placed the fair way.
fn place_towers(terrain: &TerrainMap, bases: &[Base], rng: &mut impl Rng, taken: &mut Vec<IVec2>) -> Vec<IVec2> {
    let home = bases[0].spawn_tile();
    let mut reachable_cache = std::collections::HashMap::new();
    let mut reachable = |t: IVec2| *reachable_cache.entry(t).or_insert_with(|| terrain.find_path(home, t).is_some());
    let spot = |t: IVec2, towers: &[IVec2]| {
        terrain.is_walkable(t)
            && !terrain.is_ramp(t)
            && !terrain.is_ramp(t + IVec2::Y)
            && !terrain.is_ramp_entry(t)
            && !terrain.is_cliff_foot(t)
            && !terrain.is_edge_art(t)
            && bases.iter().all(|b| !b.contains(t) && chebyshev(b.centre(), t) >= FEATURE_BASE_DISTANCE)
            && towers.iter().all(|o| chebyshev(*o, t) >= FEATURE_MIN_DISTANCE)
    };
    let mut towers: Vec<IVec2> = Vec::new();

    // Mountains: connected tiles of layer 2 and up, biggest first.
    let high = |t: IVec2| terrain.layer(t).is_some_and(|l| l >= 2);
    let mut seen = std::collections::HashSet::new();
    let mut mountains: Vec<Vec<IVec2>> = Vec::new();
    for (start, _) in terrain.tiles() {
        if !high(start) || !seen.insert(start) {
            continue;
        }
        let (mut region, mut stack) = (Vec::new(), vec![start]);
        while let Some(t) = stack.pop() {
            region.push(t);
            for d in [IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
                let n = t + d;
                if high(n) && seen.insert(n) {
                    stack.push(n);
                }
            }
        }
        mountains.push(region);
    }
    mountains.sort_by_key(|m| std::cmp::Reverse(m.len()));
    for mountain in mountains.iter().take(MOUNTAINS_WITH_TOWERS) {
        let mut candidates: Vec<IVec2> = mountain.iter().copied().filter(|t| spot(*t, &towers)).collect();
        candidates.shuffle(rng);
        candidates.truncate(200);
        // The pair furthest apart: opposite sides of the mountain.
        let mut pairs: Vec<(i32, IVec2, IVec2)> = Vec::new();
        for (i, a) in candidates.iter().enumerate() {
            for b in &candidates[i + 1..] {
                let d = chebyshev(*a, *b);
                if d >= FEATURE_MIN_DISTANCE {
                    pairs.push((d, *a, *b));
                }
            }
        }
        pairs.sort_by_key(|p| std::cmp::Reverse(p.0));
        let pick = pairs.into_iter().find(|(_, a, b)| reachable(*a) && reachable(*b)).map(|(_, a, b)| vec![a, b]);
        // No room for two: one is better than none.
        let pick = pick.or_else(|| candidates.iter().copied().find(|t| reachable(*t)).map(|t| vec![t]));
        for t in pick.unwrap_or_default().into_iter().take(TOWERS_PER_MOUNTAIN) {
            towers.push(t);
        }
    }

    // The rest: near the castle that has the fewest towers around it.
    let mut tries = 0;
    while towers.len() < TOWER_COUNT && tries < 50 {
        tries += 1;
        let near = |b: &Base| towers.iter().filter(|t| chebyshev(b.centre(), **t) <= FAIR_RADIUS).count();
        let fewest = bases.iter().map(near).min().unwrap_or(0);
        let mut poorest: Vec<&Base> = bases.iter().filter(|b| near(b) == fewest).collect();
        poorest.shuffle(rng);
        let base = poorest[0];
        let mut candidates: Vec<IVec2> = terrain
            .tiles()
            .map(|(t, _)| t)
            .filter(|t| (FAIR_DISTANCE.0..=FAIR_DISTANCE.1).contains(&chebyshev(base.centre(), *t)))
            .filter(|t| terrain.layer(*t) == Some(1) && spot(*t, &towers))
            .collect();
        candidates.shuffle(rng);
        if let Some(t) = candidates.into_iter().find(|t| reachable(*t)) {
            towers.push(t);
        } else {
            // Nothing fits around that castle: anywhere reachable, as far from other towers as possible.
            let mut anywhere: Vec<IVec2> = terrain.tiles().map(|(t, _)| t).filter(|t| spot(*t, &towers)).collect();
            anywhere.shuffle(rng);
            anywhere.truncate(300);
            anywhere.sort_by_key(|t| std::cmp::Reverse(towers.iter().map(|o| chebyshev(*o, *t)).min().unwrap_or(i32::MAX)));
            match anywhere.into_iter().find(|t| reachable(*t)) {
                Some(t) => towers.push(t),
                None => break,
            }
        }
    }
    taken.extend(towers.iter().copied());
    towers
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
            && !terrain.is_ramp(tile)
            && !terrain.is_ramp(tile + IVec2::Y) // nor under a ramp's slope
            && !terrain.is_ramp_entry(tile) // nor in its way
            && !terrain.is_cliff_foot(tile) // keep off the cliff faces
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
/// rubber ducks in the water. Never on bases, walls, ramps, plateau edges or feature tiles; trees
/// also stay a tile away from those.
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
                // Nothing on walls, ramps or plateau edges; a tree's wide canopy also keeps its
                // eight neighbours clear of them.
                let around_edge = |radius: i32| {
                    (-radius..=radius).any(|dx| (-radius..=radius).any(|dy| terrain.is_edge_art(tile + IVec2::new(dx, dy))))
                };
                let blocked = bases.iter().any(|b| chebyshev(b.centre(), tile) <= 3)
                    || around_edge(0)
                    || taken.contains(&tile);
                if blocked {
                    None
                } else {
                    let m = moisture.fbm(tile.x as f32 / 8.0, tile.y as f32 / 8.0, 3);
                    let roll: f32 = rng.random();
                    let tree_chance = if ground == Terrain::Grass { 0.01 + 0.10 * m * m } else { 0.02 };
                    if roll < tree_chance && !around_edge(1) {
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
                    assert!(a.origin != b.origin, "two bases in one patch (seed {seed})");
                }
            }
            assert!(world.terrain.walkable_at(world.hero_home_pos()));
            assert!(!world.land_united());
        }
    }

    #[test]
    fn towers_sit_on_the_mountains_and_even_out_the_rest() {
        for seed in [1, 2, 3, 5, 11, 99] {
            let world = WorldState::generate(seed);
            let map = &world.terrain;
            let home = world.home_base().spawn_tile();
            assert_eq!(world.towers.len(), TOWER_COUNT, "seed {seed}");
            for t in &world.towers {
                assert!(map.find_path(home, t.tile).is_some(), "seed {seed}: tower at {} unreachable", t.tile);
                assert!(world.bases.iter().all(|b| chebyshev(b.centre(), t.tile) >= FEATURE_BASE_DISTANCE));
            }
            for (i, a) in world.towers.iter().enumerate() {
                for b in &world.towers[i + 1..] {
                    assert!(chebyshev(a.tile, b.tile) >= FEATURE_MIN_DISTANCE, "seed {seed}: towers too close");
                }
            }
            let up = world.towers.iter().filter(|t| map.layer(t.tile).is_some_and(|l| l >= 2)).count();
            assert!(up >= 2, "seed {seed}: only {up} towers on mountains");
        }
    }

    #[test]
    fn ramps_are_climbable_and_features_keep_off_them() {
        let world = WorldState::generate(11);
        let ramps: Vec<IVec2> = world.terrain.ramps().map(|(r, _)| r).collect();
        assert!(!ramps.is_empty());
        for ramp in &ramps {
            let top = world.terrain.ramp_top(*ramp).unwrap();
            assert!(world.terrain.can_step(*ramp, top) && world.terrain.can_step(top, *ramp));
            let off = |t: IVec2| !world.terrain.is_ramp(t) && !world.terrain.is_ramp_entry(t);
            assert!(world.towers.iter().all(|t| off(t.tile)));
            assert!(world.sellers.iter().all(|s| off(s.tile)));
        }
        assert!(world.decorations.iter().all(|d| !world.terrain.is_edge_art(d.tile)));
        assert!(world.terrain.unreached_regions().is_empty(), "seed 11: every plateau has a ramp");
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
