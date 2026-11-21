//! Things that walk around the map on their own: AI kingdom armies that besiege towers and
//! castles, and Black bandit squads. See agents.md §1.5.

use bevy::prelude::*;
use rand::Rng;

use super::economy::Treasuries;
use super::interact::{Garrison, Nearby, start_fight};
use super::resources;
use super::terrain::TILE;
use super::terrain::TerrainMap;
use super::world::{Base, EncounterSource, Tower, WorldState};
use super::{ai, autoresolve};
use super::{Hero, OVERWORLD_UNIT_SCALE, UNIT_FEET, label};
use crate::assets::{AttackDir, UnitAnim, UnitArt, UnitVisual};
use crate::faction::Faction;
use crate::states::GameState;
use crate::units::{Army, CASTLE_SLOTS, PlayerArmy, TOWER_SLOTS, UnitKind};

pub const AI_ARMIES_PER_KINGDOM: usize = 2;
/// An AI army stands at a tower or castle of another AI kingdom this long before it falls...
pub const TOWER_CAPTURE_SECS: f32 = 10.0;
/// ...and this long at one of yours: the time you have to get there and JOIN the fight.
pub const SIEGE_SECS: f32 = 45.0;
/// ...then waits for the capture animation before walking on.
pub const TOWER_LEAVE_SECS: f32 = 1.5;
pub const AI_ARMY_SPEED: f32 = 80.0;
/// Hunting, defending and fleeing armies hurry.
pub const HUNT_SPEED: f32 = 115.0;
/// A beaten AI army's ghost flies home this fast, straight over water, cliffs and layers...
pub const GHOST_SPEED: f32 = 140.0;
/// ...then waits this long at its castle before a new army (drawn from the garrison) sets out.
pub const GHOST_WAIT_SECS: f32 = 30.0;
/// The ghost sheet's frames are 224×241 px; this draws the ghost about a unit tall on the map.
const GHOST_SCALE: f32 = 0.3;
/// An army that came home rests this long before drawing a new army from the garrison.
pub const AI_REST_SECS: f32 = 20.0;
/// An AI army skips towers whose guard is stronger than this × its own strength.
pub const AI_ATTACK_RATIO: f32 = 0.8;
/// An army with fewer units than this goes home after a capture, rests `AI_REST_SECS` and
/// comes back with a fresh army.
pub const AI_RETREAT_BELOW: u32 = 5;
/// Share of an AI army that stays behind when it takes a tower.
pub const TOWER_TAKE_LOSS: (f32, f32) = (0.10, 0.30);
pub const BANDIT_COUNT: usize = 4;
pub const BANDIT_SPEED: f32 = 55.0;
pub const BANDIT_VISION: f32 = 110.0;
pub const BANDIT_RESPAWN_SECS: f32 = 45.0;
pub const BANDIT_GOLD: (u32, u32) = (10, 40);
/// Distance at which two things on the map count as touching.
pub const TOUCH_DISTANCE: f32 = 40.0;
/// How far (tiles) around its castle and towers a kingdom calls its own.
pub const TERRITORY_TILES: i32 = 8;
/// Targets may lie this much further out than the territory, so it grows tower by tower.
pub const TARGET_REACH_TILES: i32 = 5;
/// An army notices you, bandits and your workers this close (tiles).
pub const SIGHT_TILES: f32 = 6.0;
/// A hunt is given up when the prey gets this far away (tiles), or after `HUNT_SECS`.
const GIVE_UP_TILES: f32 = 10.0;
const HUNT_SECS: f32 = 20.0;
/// You are this much stronger than an army: it runs home instead of fighting.
const FLEE_RATIO: f32 = 1.5;
/// An army hunts you or bandits only when they are at most this strong compared with it.
const HUNT_RATIO: f32 = 1.0;
/// Your hero this close (tiles) to a kingdom's tower or castle sounds the alarm there.
const ALERT_TILES: f32 = 6.0;
/// A defending army guards the place this long after reaching it.
const GUARD_SECS: f32 = 15.0;
/// AI armies within this many tiles of their own tower or castle join the fight when you attack it.
pub const HELP_TILES: f32 = 5.0;
/// A fight between two armies, or an army and bandits, lasts this long before it is decided.
pub const FIGHT_SECS: f32 = 6.0;
/// Armies rethink (and re-plan their path) this often.
const THINK_SECS: f32 = 0.5;
/// Patrols walk to random spots this far (tiles) from home.
const PATROL_TILES: i32 = 8;
const SQUARE: f32 = 40.0;

/// What an AI army is hunting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prey {
    Hero,
    Bandits(usize),
    /// One of your worker Pawns (index into `WorldState::workers`).
    Worker(usize),
}

/// Who an AI army is fighting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Foe {
    /// Another kingdom's army (index into `WorldState::armies`).
    Army(usize),
    Bandits(usize),
}

/// What a roaming AI army is doing right now. Its label says it in words (agents.md §1.5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AiArmyState {
    /// Marching to its target tower or castle.
    Walking,
    /// Nothing worth taking: walking around near home.
    Patrolling,
    /// Chasing you, bandits or a worker it saw inside its territory.
    Hunting(Prey),
    /// Going to (and then guarding for `left` seconds) one of its own places that is threatened.
    Defending { place: Garrison, left: f32 },
    /// You are much stronger: running home.
    Fleeing,
    /// Locked in a fight; it is decided (auto-resolve) when `left` runs out.
    Fighting { foe: Foe, left: f32 },
    /// Besieging its target tower or castle; seconds until it is auto-resolved.
    Capturing(f32),
    /// Watching the capture animation; seconds until it walks on.
    Leaving(f32),
    /// Too few units left: walking back to its castle.
    Returning,
    /// At the castle; seconds until it tries to draw a new army from the garrison.
    Resting(f32),
}

/// A kingdom's roaming army. Walks (A*, same rules as the hero) to a reachable tower or castle it
/// does not own, besieges it (`TOWER_CAPTURE_SECS`, or `SIEGE_SECS` if it is yours), fights for
/// it and moves on.
#[derive(Clone, Debug)]
pub struct AiArmy {
    pub faction: Faction,
    pub pos: Vec2,
    pub army: Army,
    pub target: Option<Garrison>,
    /// Tiles still to walk, first one next.
    pub path: Vec<IVec2>,
    pub state: AiArmyState,
    pub alive: bool,
    pub respawn_in: f32,
    /// Seconds until it rethinks what to do.
    pub think: f32,
    /// Seconds spent on the current hunt.
    pub hunted: f32,
    /// Beaten: its ghost flies home (`pos` moves), then waits there `respawn_in` seconds.
    pub ghost: bool,
}

impl AiArmy {
    /// Beaten. Its ghost flies straight home, ignoring water and layers, waits `GHOST_WAIT_SECS`
    /// at the castle, and then a new army (drawn from the garrison) sets out (`respawn_roamers`).
    pub fn die(&mut self) {
        self.alive = false;
        self.ghost = true;
        self.respawn_in = GHOST_WAIT_SECS;
        self.army = Army::empty(self.faction);
        self.target = None;
        self.path.clear();
        self.state = AiArmyState::Walking;
    }
}

/// A ghost flying home, or waiting at its castle (index into `WorldState::armies`).
#[derive(Component)]
pub struct GhostMarker(pub usize);

/// Black bandits wandering between random points, carrying gold.
#[derive(Clone, Debug)]
pub struct BanditSquad {
    pub pos: Vec2,
    pub army: Army,
    pub gold: u32,
    /// Tiles still to walk, first one next. Empty = pick a new wander target.
    pub path: Vec<IVec2>,
    pub alive: bool,
    pub respawn_in: f32,
    /// Fighting this AI army (index): it stands still until the fight is decided.
    pub fighting: Option<usize>,
}

#[derive(Component)]
pub struct AiArmyMarker(pub usize);

#[derive(Component)]
pub struct BanditMarker(pub usize);

/// Child label that shows the current army of a roamer.
#[derive(Component)]
pub struct ArmyLabel;

// ---------- setup ----------

pub fn tower_garrison(rng: &mut impl Rng) -> Army {
    Army::new(
        Faction::Black,
        &[(UnitKind::Warrior, rng.random_range(3..=5)), (UnitKind::Archer, rng.random_range(2..=4))],
    )
}

fn random_ai_army(faction: Faction, rng: &mut impl Rng) -> Army {
    Army::new(
        faction,
        &[
            (UnitKind::Warrior, rng.random_range(4..=8)),
            (UnitKind::Archer, rng.random_range(2..=5)),
            (UnitKind::Lancer, rng.random_range(1..=3)),
        ],
    )
}

fn random_bandit_army(rng: &mut impl Rng) -> Army {
    Army::new(
        Faction::Black,
        &[(UnitKind::Warrior, rng.random_range(2..=4)), (UnitKind::Pawn, rng.random_range(3..=7))],
    )
}

pub fn initial_ai_armies(world: &WorldState, rng: &mut impl Rng) -> Vec<AiArmy> {
    let mut armies = Vec::new();
    for faction in Faction::KINGDOMS.into_iter().filter(|f| !f.is_player()) {
        let Some(base) = world.base_of(faction) else { continue };
        let home = world.terrain.world_pos(base.spawn_tile());
        for i in 0..AI_ARMIES_PER_KINGDOM {
            armies.push(AiArmy {
                faction,
                pos: home + Vec2::new(i as f32 * 30.0 - 15.0, 0.0),
                army: random_ai_army(faction, rng),
                target: None,
                path: Vec::new(),
                state: AiArmyState::Walking,
                alive: true,
                respawn_in: 0.0,
                think: 0.0,
                hunted: 0.0,
                ghost: false,
            });
        }
    }
    armies
}

pub fn initial_bandits(world: &WorldState, rng: &mut impl Rng) -> Vec<BanditSquad> {
    let home = world.home_base().spawn_tile();
    (0..BANDIT_COUNT)
        .map(|_| {
            let pos = world.terrain.world_pos(world.random_walkable_tile(rng, home, 10));
            BanditSquad {
                pos,
                army: random_bandit_army(rng),
                gold: rng.random_range(BANDIT_GOLD.0..=BANDIT_GOLD.1),
                path: Vec::new(),
                alive: true,
                respawn_in: 0.0,
                fighting: None,
            }
        })
        .collect()
}

// ---------- entities ----------

pub fn spawn_ai_army_entity(commands: &mut Commands, art: &mut UnitArt, army: &AiArmy, index: usize, world: &WorldState) {
    commands.spawn((
        AiArmyMarker(index),
        army.faction,
        Transform::from_translation(army.pos.extend(3.0)),
        Visibility::default(),
        DespawnOnExit(GameState::Overworld),
        children![
            art.bundle(UnitVisual::new(army.faction, UnitKind::Warrior, false), OVERWORLD_UNIT_SCALE, UNIT_FEET, 0.1),
            (ArmyLabel, label(army_text(army, world), Color::WHITE, Vec2::new(0.0, SQUARE))),
        ],
    ));
}

pub fn spawn_bandit_entity(
    commands: &mut Commands,
    art: &mut UnitArt,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    squad: &BanditSquad,
    index: usize,
) {
    commands.spawn((
        BanditMarker(index),
        Faction::Black,
        Transform::from_translation(squad.pos.extend(3.0)),
        Visibility::default(),
        DespawnOnExit(GameState::Overworld),
        children![
            art.bundle(UnitVisual::new(Faction::Black, UnitKind::Warrior, false), OVERWORLD_UNIT_SCALE, UNIT_FEET, 0.1),
            (
                Mesh2d(meshes.add(Circle::new(BANDIT_VISION))),
                MeshMaterial2d(materials.add(Color::srgba(1.0, 0.2, 0.2, 0.12))),
                Transform::from_xyz(0.0, 0.0, -0.5),
            ),
            (ArmyLabel, label(bandit_text(squad), Color::srgb(1.0, 0.5, 0.4), Vec2::new(0.0, SQUARE))),
        ],
    ));
}

/// The label over an AI army: who, what it has, and what it is doing in plain words.
fn army_text(army: &AiArmy, world: &WorldState) -> String {
    let faction = army.faction;
    // Home with its units in the castle: waiting for enough troops for a new army.
    if army.army.is_empty() {
        return format!("{} army\ngathering troops...", faction.name());
    }
    let doing = match army.state {
        AiArmyState::Walking => match army.target {
            Some(t) if t.owner(world) == faction => format!("reinforcing its {}", t.noun()),
            Some(t) => format!("marching to a {}", t.noun()),
            None => "marching".to_string(),
        },
        AiArmyState::Patrolling => "patrolling".to_string(),
        AiArmyState::Hunting(Prey::Hero) => "CHASING YOU!".to_string(),
        AiArmyState::Hunting(Prey::Bandits(_)) => "hunting bandits".to_string(),
        AiArmyState::Hunting(Prey::Worker(_)) => "hunting your workers".to_string(),
        AiArmyState::Defending { place, .. } if !army.path.is_empty() => format!("coming to help its {}", place.noun()),
        AiArmyState::Defending { left, .. } => format!("guarding... {}s", left.ceil() as u32),
        AiArmyState::Fleeing => "fleeing home".to_string(),
        AiArmyState::Fighting { foe: Foe::Army(j), left } => {
            format!("fighting the {} army... {}s", world.armies[j].faction.name(), left.ceil() as u32)
        }
        AiArmyState::Fighting { foe: Foe::Bandits(_), left } => format!("fighting bandits... {}s", left.ceil() as u32),
        AiArmyState::Capturing(left) => format!("besieging... {}s", left.ceil() as u32),
        AiArmyState::Leaving(_) => "taken!".to_string(),
        AiArmyState::Returning => "returning home".to_string(),
        AiArmyState::Resting(left) => format!("resting... {}s", left.ceil() as u32),
    };
    format!("{} army\n{}\n{doing}", faction.name(), army.army.summary())
}

fn chebyshev(a: IVec2, b: IVec2) -> i32 {
    (a - b).abs().max_element()
}

/// How far (tiles) `tile` is from the nearest castle or tower `faction` holds; `i32::MAX` if none.
/// Within `TERRITORY_TILES` is the kingdom's territory.
pub fn territory_distance(faction: Faction, tile: IVec2, towers: &[Tower], bases: &[Base]) -> i32 {
    let castles = bases.iter().filter(|b| b.owner == faction).map(|b| chebyshev(b.centre(), tile));
    let towers = towers.iter().filter(|t| t.owner == faction).map(|t| chebyshev(t.tile, tile));
    castles.chain(towers).min().unwrap_or(i32::MAX)
}

fn bandit_text(squad: &BanditSquad) -> String {
    let fighting = if squad.fighting.is_some() { "\nfighting!" } else { "" };
    format!("Bandits ({}g)\n{}{fighting}", squad.gold, squad.army.summary())
}

// ---------- systems (Update, Overworld) ----------

/// Walk along `path` at `speed`, popping waypoints as they are reached. True when nothing is left.
pub fn follow_path(pos: &mut Vec2, path: &mut Vec<IVec2>, terrain: &TerrainMap, speed: f32, dt: f32) -> bool {
    let Some(&next) = path.first() else { return true };
    let goal = terrain.world_pos(next);
    let to_goal = goal - *pos;
    let step = speed * dt;
    if to_goal.length() <= step {
        *pos = goal;
        path.remove(0);
        path.is_empty()
    } else {
        *pos += to_goal.normalize() * step;
        false
    }
}

/// The tower or castle an AI army of `army` strength at `here` should besiege next, and the path
/// there: among those not owned by `faction` that it can walk to, within reach of its territory
/// (`TERRITORY_TILES + TARGET_REACH_TILES` from a place it holds) and not being besieged by another
/// kingdom (`busy`), skip any whose garrison is stronger than `AI_ATTACK_RATIO` × its own, and pick
/// the lowest walking distance × (1 + garrison / own).
#[allow(clippy::too_many_arguments)]
pub fn choose_target(
    faction: Faction,
    army: &Army,
    here: IVec2,
    towers: &[Tower],
    bases: &[Base],
    terrain: &TerrainMap,
    busy: &[(Garrison, Faction)],
) -> Option<(Garrison, Vec<IVec2>)> {
    let own = autoresolve::strength(army).max(1.0);
    let reach = TERRITORY_TILES + TARGET_REACH_TILES;
    let towers_ = towers.iter().enumerate().map(|(i, t)| (Garrison::Tower(i), t.owner, &t.garrison));
    let castles = bases.iter().enumerate().map(|(i, b)| (Garrison::Castle(i), b.owner, &b.garrison));
    towers_
        .chain(castles)
        .filter(|(_, owner, _)| *owner != faction)
        .filter(|(place, _, _)| territory_distance(faction, place.tile_in(towers, bases), towers, bases) <= reach)
        .filter(|(place, _, _)| !busy.iter().any(|(p, f)| p == place && *f != faction))
        .map(|(place, _, garrison)| (place, autoresolve::strength(garrison)))
        .filter(|(_, guard)| *guard <= AI_ATTACK_RATIO * own)
        .filter_map(|(place, guard)| {
            let path = terrain.find_path(here, place.tile_in(towers, bases))?;
            let score = (path.len() as f32 + 1.0) * (1.0 + guard / own);
            Some((score, place, path))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, place, path)| (place, path))
}

/// The nearest tower of `faction` that is not full (`TOWER_SLOTS`), and the path there. Armies
/// fill their kingdom's towers before they attack anything new.
pub fn choose_reinforce(faction: Faction, here: IVec2, towers: &[Tower], terrain: &TerrainMap) -> Option<(Garrison, Vec<IVec2>)> {
    towers
        .iter()
        .enumerate()
        .filter(|(_, t)| t.owner == faction && t.garrison.slots() < TOWER_SLOTS)
        .filter_map(|(i, t)| terrain.find_path(here, t.tile).map(|p| (Garrison::Tower(i), p)))
        .min_by_key(|(_, p)| p.len())
}

/// Move up to `n` units from `from` to `to`, stack by stack. Returns how many moved.
fn give_units(from: &mut Army, to: &mut Army, n: u32) -> u32 {
    let mut left = n;
    for stack in from.stacks.clone() {
        if left == 0 {
            break;
        }
        let moved = from.remove(stack.kind, stack.count.min(left));
        to.add(stack.kind, moved);
        left -= moved;
    }
    n - left
}

/// Put an army's units back into its castle garrison (as many as fit).
fn join_garrison(garrison: &mut Army, army: &mut Army) {
    for stack in std::mem::replace(army, Army::empty(army.faction)).stacks {
        let room = CASTLE_SLOTS.saturating_sub(garrison.slots());
        garrison.add(stack.kind, stack.count.min(room));
    }
}

/// What an army sees when it thinks (see `decide`).
struct Seen<'a> {
    hero_pos: Vec2,
    player_strength: f32,
    player_armed: bool,
    /// Living bandit squads: index, position, strength.
    bandits: &'a [(usize, Vec2, f32)],
    /// Your living workers: index, position.
    workers: &'a [(usize, Vec2)],
    /// Places being besieged right now, and by whom.
    busy: &'a [(Garrison, Faction)],
    /// The place this army was picked to defend, and whether you (rather than another kingdom) are the threat.
    defend: Option<(Garrison, bool)>,
}

/// Which army defends which threatened place. A kingdom's tower or castle is threatened when you
/// (with an army) come within `ALERT_TILES`, or another kingdom besieges it; its nearest free army
/// within reach goes to guard it. Returns, per army, the place and whether you are the threat.
fn assign_defenders(world: &WorldState, hero_pos: Vec2, player_armed: bool, busy: &[(Garrison, Faction)]) -> Vec<Option<(Garrison, bool)>> {
    let mut out = vec![None; world.armies.len()];
    let places = world
        .bases
        .iter()
        .enumerate()
        .map(|(i, b)| (Garrison::Castle(i), b.owner))
        .chain(world.towers.iter().enumerate().map(|(i, t)| (Garrison::Tower(i), t.owner)))
        .filter(|(_, owner)| !owner.is_player() && *owner != Faction::Black);
    for (place, owner) in places {
        let at = world.terrain.world_pos(place.tile(world));
        let by_hero = player_armed && at.distance(hero_pos) <= ALERT_TILES * TILE;
        let besieged = busy.iter().any(|(p, f)| *p == place && *f != owner);
        if !by_hero && !besieged {
            continue;
        }
        let free = |a: &AiArmy| {
            matches!(a.state, AiArmyState::Walking | AiArmyState::Patrolling)
                || matches!(a.state, AiArmyState::Defending { place: p, .. } if p == place)
        };
        let reach = (TERRITORY_TILES + TARGET_REACH_TILES) as f32 * TILE;
        let nearest = world
            .armies
            .iter()
            .enumerate()
            .filter(|(i, a)| a.alive && a.faction == owner && !a.army.is_empty() && free(a) && out[*i].is_none())
            .map(|(i, a)| (i, a.pos.distance(at)))
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = nearest {
            out[i] = Some((place, by_hero));
        }
    }
    out
}

/// One think of an army in a free state (walking, patrolling, hunting, defending, fleeing), in
/// this order: keep at a hunt while it makes sense; run home from you if you are much stronger;
/// attack you inside its territory if you are not too strong; hunt bandits it can beat; hunt your
/// workers in its territory; guard a threatened place it was picked for; else march on a target
/// in reach, or patrol near home. Returns a message for the HUD when it starts something you
/// should know about.
#[allow(clippy::too_many_arguments)]
fn decide(
    army: &mut AiArmy,
    here: IVec2,
    home: IVec2,
    own: f32,
    seen: &Seen,
    towers: &[Tower],
    bases: &[Base],
    terrain: &TerrainMap,
    rng: &mut impl Rng,
) -> Option<String> {
    let (pos, faction) = (army.pos, army.faction);
    let sight = SIGHT_TILES * TILE;
    let near = |p: Vec2| p.distance(pos) <= sight;
    let closest = |a: &Vec2, b: &Vec2| a.distance(pos).total_cmp(&b.distance(pos));
    let path_to = |goal: IVec2| terrain.find_path(here, goal).unwrap_or_default();
    let reach = TERRITORY_TILES + TARGET_REACH_TILES;
    let in_territory = |tile: IVec2| territory_distance(faction, tile, towers, bases) <= TERRITORY_TILES;

    // Keep at a hunt until the prey is gone or far, it took too long, or we strayed too far.
    if let AiArmyState::Hunting(prey) = army.state {
        let target = match prey {
            Prey::Hero => seen.player_armed.then_some(seen.hero_pos),
            Prey::Bandits(i) => seen.bandits.iter().find(|b| b.0 == i).map(|b| b.1),
            Prey::Worker(i) => seen.workers.iter().find(|w| w.0 == i).map(|w| w.1),
        };
        match target {
            Some(t)
                if t.distance(pos) <= GIVE_UP_TILES * TILE
                    && army.hunted < HUNT_SECS
                    && territory_distance(faction, here, towers, bases) <= reach =>
            {
                army.path = path_to(terrain.tile_of(t));
                return None;
            }
            _ => {
                army.state = AiArmyState::Walking;
                army.hunted = 0.0;
                army.target = None;
                army.path.clear();
            }
        }
    }
    // Running home: keep running.
    if army.state == AiArmyState::Fleeing && !army.path.is_empty() {
        return None;
    }
    // You are much stronger and close: run home.
    if seen.player_armed && near(seen.hero_pos) && seen.player_strength > FLEE_RATIO * own {
        army.state = AiArmyState::Fleeing;
        army.target = None;
        army.path = path_to(home);
        return None;
    }
    // You inside its territory and not too strong: attack.
    if seen.player_armed && near(seen.hero_pos) && in_territory(here) && seen.player_strength <= HUNT_RATIO * own {
        army.state = AiArmyState::Hunting(Prey::Hero);
        army.hunted = 0.0;
        army.target = None;
        army.path = path_to(terrain.tile_of(seen.hero_pos));
        return Some(format!("The {} army has seen you and attacks!", faction.name()));
    }
    // Bandits close by that it can beat.
    if let Some(b) = seen.bandits.iter().filter(|b| near(b.1) && b.2 <= HUNT_RATIO * own).min_by(|a, b| closest(&a.1, &b.1)) {
        army.state = AiArmyState::Hunting(Prey::Bandits(b.0));
        army.hunted = 0.0;
        army.target = None;
        army.path = path_to(terrain.tile_of(b.1));
        return None;
    }
    // Your workers inside its territory.
    if let Some(w) = seen.workers.iter().filter(|w| near(w.1) && in_territory(terrain.tile_of(w.1))).min_by(|a, b| closest(&a.1, &b.1)) {
        army.state = AiArmyState::Hunting(Prey::Worker(w.0));
        army.hunted = 0.0;
        army.target = None;
        army.path = path_to(terrain.tile_of(w.1));
        return Some(format!("The {} army goes after your workers!", faction.name()));
    }
    // One of its places is threatened and this army was picked to guard it.
    if let Some((place, by_hero)) = seen.defend {
        if !matches!(army.state, AiArmyState::Defending { place: p, .. } if p == place) {
            army.state = AiArmyState::Defending { place, left: GUARD_SECS };
            army.target = None;
            army.path = path_to(place.tile_in(towers, bases));
            return by_hero.then(|| format!("The {} army comes to defend its {}!", faction.name(), place.noun()));
        }
        return None;
    }
    if matches!(army.state, AiArmyState::Defending { .. }) {
        return None; // finishing its guard duty
    }
    // Fill our own towers first, then march on a target in reach, or patrol near home.
    let still_valid = army.target.is_some_and(|t| match t {
        Garrison::Tower(i) if towers[i].owner == faction => towers[i].garrison.slots() < TOWER_SLOTS,
        _ => t.owner_in(towers, bases) != faction && !seen.busy.iter().any(|(p, f)| *p == t && *f != faction),
    });
    if army.state == AiArmyState::Walking && still_valid {
        return None;
    }
    let reinforce = choose_reinforce(faction, here, towers, terrain);
    match reinforce.or_else(|| choose_target(faction, &army.army, here, towers, bases, terrain, seen.busy)) {
        Some((t, path)) => {
            army.target = Some(t);
            army.path = path;
            army.state = AiArmyState::Walking;
        }
        None => {
            army.target = None;
            if army.state != AiArmyState::Patrolling || army.path.is_empty() {
                army.state = AiArmyState::Patrolling;
                let spot = home + IVec2::new(rng.random_range(-PATROL_TILES..=PATROL_TILES), rng.random_range(-PATROL_TILES..=PATROL_TILES));
                army.path = path_to(spot);
            }
        }
    }
    None
}

/// AI armies path (A*, same rules as the hero) to a tower or castle picked by `choose_target`,
/// besiege it for `TOWER_CAPTURE_SECS` (`SIEGE_SECS` if it is yours: time to come and JOIN),
/// fight its garrison by auto-resolve, and on a win take it (leaving 10–30 % of their units behind
/// as the new garrison), wait for the capture animation and walk on. A kingdom whose castle falls
/// loses its armies. With fewer
/// than `AI_RETREAT_BELOW` units they go home, join the garrison, rest and set out again with an
/// army drawn from the garrison. Armies of different kingdoms that meet fight by auto-resolve.
pub fn ai_armies_act(
    time: Res<Time>,
    mut world: ResMut<WorldState>,
    mut nearby: ResMut<Nearby>,
    mut treasuries: ResMut<Treasuries>,
    player: Option<Res<PlayerArmy>>,
    mut roamers: Query<(&AiArmyMarker, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::rng();

    // What every army can see this frame, taken before anyone moves.
    let hero_pos = world.hero_pos;
    let player_strength = player.as_ref().map_or(0.0, |p| autoresolve::strength(&p.0));
    let player_armed = player.as_ref().is_some_and(|p| !p.0.is_empty());
    let busy: Vec<(Garrison, Faction)> = world
        .armies
        .iter()
        .filter(|a| a.alive && matches!(a.state, AiArmyState::Capturing(_)))
        .filter_map(|a| a.target.map(|t| (t, a.faction)))
        .collect();
    let bandit_seen: Vec<(usize, Vec2, f32)> = world
        .bandits
        .iter()
        .enumerate()
        .filter(|(_, b)| b.alive)
        .map(|(i, b)| (i, b.pos, autoresolve::strength(&b.army)))
        .collect();
    let workers_seen: Vec<(usize, Vec2)> =
        world.workers.iter().enumerate().filter(|(_, w)| w.alive).map(|(i, w)| (i, w.pos)).collect();
    let defenders = assign_defenders(&world, hero_pos, player_armed, &busy);

    let WorldState { armies, towers, terrain, bases, upgrades, bandits, workers, sites, .. } = &mut *world;
    let siege_secs = upgrades.siege_secs();
    // Kingdoms that lost their castle this frame.
    let mut fallen: Vec<Faction> = Vec::new();

    for (idx, army) in armies.iter_mut().enumerate() {
        if !army.alive {
            continue;
        }
        let here = terrain.tile_of(army.pos);
        let home = bases.iter().position(|b| b.home == army.faction && b.owner == army.faction);
        let home_tile = home.map(|h| bases[h].spawn_tile());
        // Standing at a tower: count down, fight for it, then watch the animation before leaving.
        match army.state {
            AiArmyState::Capturing(left) => {
                // Every army of the kingdom besieging the same place speeds the siege up.
                let together = army.target.map_or(1, |t| busy.iter().filter(|(p, f)| *p == t && *f == army.faction).count().max(1));
                let left = left - dt * together as f32;
                // Taken by a fellow army in the meantime: move on.
                if army.target.is_some_and(|t| t.owner_in(towers, bases) == army.faction) {
                    army.target = None;
                    army.state = AiArmyState::Walking;
                    continue;
                }
                if left > 0.0 {
                    army.state = AiArmyState::Capturing(left);
                } else if let Some(place) = army.target {
                    let defender = place.owner_in(towers, bases);
                    if defender == army.faction {
                        army.target = None;
                        army.state = AiArmyState::Walking;
                        continue;
                    }
                    let garrison = match place {
                        Garrison::Castle(i) => &mut bases[i].garrison,
                        Garrison::Tower(i) => &mut towers[i].garrison,
                    };
                    let what = place.noun();
                    if autoresolve::resolve(&mut army.army, garrison, &mut rng) {
                        let share = rng.random_range(TOWER_TAKE_LOSS.0..=TOWER_TAKE_LOSS.1);
                        let left_behind = army.army.split_off_fraction(share);
                        match place {
                            Garrison::Castle(i) => {
                                bases[i].change_owner(army.faction, left_behind);
                                fallen.push(defender);
                            }
                            Garrison::Tower(i) => towers[i].change_owner(army.faction, left_behind),
                        }
                        army.state = AiArmyState::Leaving(TOWER_LEAVE_SECS);
                        if defender.is_player() {
                            nearby.say_for(format!("The {} army took your {what}!", army.faction.name()), 6.0);
                        }
                    } else {
                        if defender.is_player() {
                            nearby.say_for(format!("Your {what} held against the {} army.", army.faction.name()), 6.0);
                        }
                        army.state = AiArmyState::Walking;
                    }
                    army.target = None;
                    if army.army.is_empty() {
                        army.die();
                    }
                }
                continue;
            }
            AiArmyState::Fighting { foe, left } => {
                let left = left - dt;
                match foe {
                    Foe::Bandits(b) => {
                        let squad = &mut bandits[b];
                        if !squad.alive {
                            army.state = AiArmyState::Walking;
                        } else if left > 0.0 {
                            army.state = AiArmyState::Fighting { foe, left };
                        } else {
                            squad.fighting = None;
                            if autoresolve::resolve(&mut army.army, &mut squad.army, &mut rng) {
                                squad.alive = false;
                                squad.respawn_in = BANDIT_RESPAWN_SECS;
                                treasuries.add(army.faction, std::mem::take(&mut squad.gold));
                                army.state = AiArmyState::Walking;
                            } else {
                                army.die();
                            }
                        }
                    }
                    // Decided below, where both armies are at hand.
                    Foe::Army(_) => army.state = AiArmyState::Fighting { foe, left },
                }
                continue;
            }
            AiArmyState::Leaving(left) => {
                let left = left - dt;
                if left > 0.0 {
                    army.state = AiArmyState::Leaving(left);
                } else if army.army.slots() < AI_RETREAT_BELOW {
                    // Too weak to go on: head home.
                    army.path = home_tile.and_then(|h| terrain.find_path(here, h)).unwrap_or_default();
                    army.state = AiArmyState::Returning;
                } else {
                    army.state = AiArmyState::Walking;
                }
                continue;
            }
            AiArmyState::Returning => {
                if follow_path(&mut army.pos, &mut army.path, terrain, AI_ARMY_SPEED, dt) {
                    if let Some(h) = home {
                        join_garrison(&mut bases[h].garrison, &mut army.army);
                    }
                    army.state = AiArmyState::Resting(AI_REST_SECS);
                }
                continue;
            }
            AiArmyState::Resting(left) => {
                let left = left - dt;
                if left > 0.0 {
                    army.state = AiArmyState::Resting(left);
                } else if let Some(fresh) = home.and_then(|h| ai::draw_army(&mut bases[h].garrison)) {
                    army.army = fresh;
                    army.target = None;
                    army.path.clear();
                    army.state = AiArmyState::Walking;
                } else {
                    // Not enough troops at home yet: look again in a moment.
                    army.state = AiArmyState::Resting(ai::AI_THINK_SECS);
                }
                continue;
            }
            // Walking, Patrolling, Hunting, Defending and Fleeing: below.
            _ => {}
        }

        let own = autoresolve::strength(&army.army).max(1.0);
        army.think -= dt;
        if army.think <= 0.0 {
            army.think = THINK_SECS;
            let seen = Seen {
                hero_pos,
                player_strength,
                player_armed,
                bandits: &bandit_seen,
                workers: &workers_seen,
                busy: &busy,
                defend: defenders[idx],
            };
            if let Some(text) = decide(army, here, home_tile.unwrap_or(here), own, &seen, towers, bases, terrain, &mut rng) {
                nearby.say_for(text, 4.0);
            }
        }

        match army.state {
            AiArmyState::Walking => {
                let Some(t) = army.target else { continue };
                let goal_tile = t.tile_in(towers, bases);
                let goal = terrain.world_pos(goal_tile);
                if army.path.is_empty() && goal.distance(army.pos) > TOUCH_DISTANCE {
                    // Pushed off course (e.g. restored after a fight): plan again.
                    match terrain.find_path(here, goal_tile) {
                        Some(path) => army.path = path,
                        None => {
                            army.target = None;
                            continue;
                        }
                    }
                }
                if follow_path(&mut army.pos, &mut army.path, terrain, AI_ARMY_SPEED, dt) {
                    // Our own tower: hand over units until it is full, then go home to refill if small.
                    if t.owner_in(towers, bases) == army.faction {
                        if let Garrison::Tower(i) = t {
                            let room = TOWER_SLOTS.saturating_sub(towers[i].garrison.slots());
                            give_units(&mut army.army, &mut towers[i].garrison, room);
                        }
                        army.target = None;
                        if army.army.slots() < AI_RETREAT_BELOW {
                            army.path = home_tile.and_then(|h| terrain.find_path(here, h)).unwrap_or_default();
                            army.state = AiArmyState::Returning;
                        } else {
                            army.state = AiArmyState::Walking;
                        }
                        continue;
                    }
                    if t.owner_in(towers, bases).is_player() {
                        army.state = AiArmyState::Capturing(siege_secs);
                        nearby.say_for(
                            format!(
                                "SIEGE! The {} army attacks your {}. Get there within {siege_secs:.0} s, Tab, JOIN.",
                                army.faction.name(),
                                t.noun()
                            ),
                            8.0,
                        );
                    } else {
                        army.state = AiArmyState::Capturing(TOWER_CAPTURE_SECS);
                    }
                }
            }
            AiArmyState::Patrolling => {
                follow_path(&mut army.pos, &mut army.path, terrain, AI_ARMY_SPEED, dt);
            }
            AiArmyState::Fleeing => {
                if follow_path(&mut army.pos, &mut army.path, terrain, HUNT_SPEED, dt) {
                    army.state = AiArmyState::Walking;
                }
            }
            AiArmyState::Defending { place, left } => {
                let arrived = follow_path(&mut army.pos, &mut army.path, terrain, HUNT_SPEED, dt);
                if arrived {
                    let left = left - dt;
                    army.state =
                        if left > 0.0 { AiArmyState::Defending { place, left } } else { AiArmyState::Walking };
                }
            }
            AiArmyState::Hunting(prey) => {
                army.hunted += dt;
                follow_path(&mut army.pos, &mut army.path, terrain, HUNT_SPEED, dt);
                match prey {
                    // Contact with you starts a real fight (`roamer_encounters`).
                    Prey::Hero => {}
                    Prey::Bandits(b) => {
                        let squad = &mut bandits[b];
                        if squad.alive && squad.fighting.is_none() && squad.pos.distance(army.pos) <= TOUCH_DISTANCE {
                            // Caught: both stop and fight for `FIGHT_SECS`.
                            squad.fighting = Some(idx);
                            squad.path.clear();
                            army.path.clear();
                            army.state = AiArmyState::Fighting { foe: Foe::Bandits(b), left: FIGHT_SECS };
                            army.hunted = 0.0;
                        }
                    }
                    Prey::Worker(w) => {
                        if workers[w].alive && workers[w].pos.distance(army.pos) <= TOUCH_DISTANCE {
                            resources::kill_worker(workers, sites, w);
                            nearby.say_for(format!("The {} army killed one of your workers!", army.faction.name()), 5.0);
                            army.state = AiArmyState::Walking;
                            army.hunted = 0.0;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // A kingdom without a castle has nowhere to raise armies from.
    for army in armies.iter_mut().filter(|a| fallen.contains(&a.faction) && !a.faction.is_player()) {
        if bases.iter().any(|b| b.owner == army.faction) {
            continue; // it still holds another castle
        }
        army.alive = false;
        army.respawn_in = f32::INFINITY;
        army.target = None;
    }

    // Armies of different kingdoms that meet stop and fight for `FIGHT_SECS`; then auto-resolve
    // decides it. If one of them is gone meanwhile (you beat it), the other walks on.
    let fighting = |a: &AiArmy, other: usize| matches!(a.state, AiArmyState::Fighting { foe: Foe::Army(x), .. } if x == other);
    for i in 0..armies.len() {
        for j in i + 1..armies.len() {
            let (a, b) = armies.split_at_mut(j);
            let (a, b) = (&mut a[i], &mut b[0]);
            let (a_on, b_on) = (fighting(a, j), fighting(b, i));
            if a_on || b_on {
                if !(a.alive && b.alive) {
                    for (army, on) in [(&mut *a, a_on), (&mut *b, b_on)] {
                        if army.alive && on {
                            army.state = AiArmyState::Walking;
                        }
                    }
                    continue;
                }
                let left = match a.state {
                    AiArmyState::Fighting { left, .. } => left,
                    _ => 0.0,
                };
                if left > 0.0 {
                    continue;
                }
                let a_won = autoresolve::resolve(&mut a.army, &mut b.army, &mut rng);
                for (army, lost) in [(a, !a_won), (b, a_won)] {
                    if lost || army.army.is_empty() {
                        army.die();
                    } else {
                        army.state = AiArmyState::Walking;
                    }
                }
                continue;
            }
            let busy = |x: &AiArmy| matches!(x.state, AiArmyState::Fighting { .. });
            let meet = a.alive
                && b.alive
                && a.faction != b.faction
                && !a.army.is_empty()
                && !b.army.is_empty()
                && !busy(a)
                && !busy(b)
                && a.pos.distance(b.pos) <= TOUCH_DISTANCE;
            if meet {
                for (army, foe) in [(&mut *a, j), (&mut *b, i)] {
                    army.path.clear();
                    army.state = AiArmyState::Fighting { foe: Foe::Army(foe), left: FIGHT_SECS };
                }
            }
        }
    }

    for (marker, mut transform) in &mut roamers {
        let pos = world.armies[marker.0].pos;
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
    }
}

/// Bandits drift between random reachable points, pathing like everyone else.
pub fn bandits_wander(
    time: Res<Time>,
    mut world: ResMut<WorldState>,
    mut roamers: Query<(&BanditMarker, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::rng();
    let WorldState { bandits, terrain, bases, armies, .. } = &mut *world;

    for (i, squad) in bandits.iter_mut().enumerate().filter(|(_, b)| b.alive) {
        // In a fight: stand still until the army decides it (or is gone).
        if let Some(a) = squad.fighting {
            let still = armies[a].alive
                && matches!(armies[a].state, AiArmyState::Fighting { foe: Foe::Bandits(b), .. } if b == i);
            if still {
                continue;
            }
            squad.fighting = None;
        }
        if squad.path.is_empty() {
            let here = terrain.tile_of(squad.pos);
            for _ in 0..20 {
                let candidate = here + IVec2::new(rng.random_range(-8..=8), rng.random_range(-8..=8));
                let ok = terrain.is_walkable(candidate) && !bases.iter().any(|b| b.contains(candidate));
                if ok && let Some(path) = terrain.find_path(here, candidate) && !path.is_empty() {
                    squad.path = path;
                    break;
                }
            }
        } else {
            follow_path(&mut squad.pos, &mut squad.path, terrain, BANDIT_SPEED, dt);
        }
    }

    for (marker, mut transform) in &mut roamers {
        let pos = world.bandits[marker.0].pos;
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
    }
}

/// Beaten armies' ghosts fly straight to their castle gate at `GHOST_SPEED` (over water, cliffs
/// and layers), then wait there: `respawn_in` only counts down once they are home. A kingdom
/// without its castle has no home: its ghosts vanish.
pub fn ghosts_fly_home(world: &mut WorldState, dt: f32) {
    let WorldState { armies, bases, terrain, .. } = world;
    for army in armies.iter_mut().filter(|a| !a.alive && a.ghost) {
        let home = bases.iter().find(|b| b.home == army.faction && b.owner == army.faction);
        let (Some(base), true) = (home, army.respawn_in.is_finite()) else {
            army.ghost = false;
            army.respawn_in = f32::INFINITY;
            continue;
        };
        let gate = terrain.world_pos(base.spawn_tile());
        let to = gate - army.pos;
        let step = GHOST_SPEED * dt;
        if to.length() > step {
            army.pos += to.normalize() * step;
        } else {
            army.pos = gate;
            army.respawn_in -= dt;
        }
    }
}

/// The label over a ghost.
fn ghost_text(army: &AiArmy, world: &WorldState) -> String {
    let home = world.bases.iter().find(|b| b.home == army.faction).map(|b| world.terrain.world_pos(b.spawn_tile()));
    if home.is_some_and(|h| h == army.pos) {
        format!("{} army (ghost)\nback in {}s", army.faction.name(), army.respawn_in.max(0.0).ceil() as u32)
    } else {
        format!("{} army (ghost)\nflying home", army.faction.name())
    }
}

/// Ghost sprites follow the ghosts: spawned when an army dies, moved as it flies home (facing the
/// way it flies), despawned when the new army sets out.
pub fn sync_ghosts(
    mut commands: Commands,
    assets: Res<crate::assets::GameAssets>,
    world: Res<WorldState>,
    mut ghosts: Query<(Entity, &GhostMarker, &mut Transform, &Children)>,
    mut sprites: Query<&mut Sprite>,
    mut labels: Query<&mut Text2d, With<ArmyLabel>>,
) {
    let mut seen = vec![false; world.armies.len()];
    for (entity, marker, mut transform, children) in &mut ghosts {
        let army = &world.armies[marker.0];
        if army.alive || !army.ghost || seen[marker.0] {
            commands.entity(entity).despawn();
            continue;
        }
        seen[marker.0] = true;
        let dx = army.pos.x - transform.translation.x;
        transform.translation.x = army.pos.x;
        transform.translation.y = army.pos.y;
        let text = ghost_text(army, &world);
        for child in children.iter() {
            if let Ok(mut sprite) = sprites.get_mut(child)
                && dx.abs() > 0.01
                && sprite.flip_x != (dx < 0.0)
            {
                sprite.flip_x = dx < 0.0;
            }
            if let Ok(mut label) = labels.get_mut(child)
                && label.0 != text
            {
                label.0 = text.clone();
            }
        }
    }
    for (i, army) in world.armies.iter().enumerate() {
        if army.alive || !army.ghost || seen[i] {
            continue;
        }
        let mut sprite = assets.ghost.sprite();
        sprite.color = Color::srgba(1.0, 1.0, 1.0, 0.85);
        commands.spawn((
            GhostMarker(i),
            Transform::from_translation(army.pos.extend(6.0)),
            Visibility::default(),
            DespawnOnExit(GameState::Overworld),
            children![
                (
                    sprite,
                    crate::assets::Animated::new(assets.ghost.frames, 8.0),
                    Transform::from_scale(Vec3::splat(GHOST_SCALE)),
                    Pickable::IGNORE,
                ),
                (ArmyLabel, label(ghost_text(army, &world), Color::srgb(0.78, 0.88, 1.0), Vec2::new(0.0, SQUARE))),
            ],
        ));
    }
}

/// Despawn beaten roamers, tick their timers and bring them back when the time is up.
pub fn respawn_roamers(
    mut commands: Commands,
    mut art: UnitArt,
    time: Res<Time>,
    mut world: ResMut<WorldState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    hero: Single<&Transform, With<Hero>>,
    ai_entities: Query<(Entity, &AiArmyMarker)>,
    bandit_entities: Query<(Entity, &BanditMarker)>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::rng();

    for (entity, marker) in &ai_entities {
        if !world.armies[marker.0].alive {
            commands.entity(entity).despawn();
        }
    }
    for (entity, marker) in &bandit_entities {
        if !world.bandits[marker.0].alive {
            commands.entity(entity).despawn();
        }
    }

    ghosts_fly_home(&mut world, dt);
    for i in 0..world.armies.len() {
        let army = &world.armies[i];
        if army.alive || !army.respawn_in.is_finite() {
            continue;
        }
        // Ghosts count down at home (`ghosts_fly_home`); anyone else counts down here.
        if army.ghost {
            if army.respawn_in > 0.0 {
                continue;
            }
        } else if army.respawn_in > dt {
            world.armies[i].respawn_in -= dt;
            continue;
        }
        // Only kingdoms that still hold their own castle raise new armies. The new one is drawn
        // from the garrison; without enough troops it waits at the castle until there are.
        let faction = army.faction;
        let Some(b) = world.bases.iter().position(|b| b.home == faction && b.owner == faction) else {
            world.armies[i].respawn_in = f32::INFINITY;
            continue;
        };
        let home = world.terrain.world_pos(world.bases[b].spawn_tile());
        let fresh = ai::draw_army(&mut world.bases[b].garrison);
        let army = &mut world.armies[i];
        army.alive = true;
        army.ghost = false;
        army.respawn_in = 0.0;
        army.pos = home;
        army.target = None;
        army.path.clear();
        army.think = 0.0;
        army.hunted = 0.0;
        (army.army, army.state) = match fresh {
            Some(fresh) => (fresh, AiArmyState::Walking),
            None => (Army::empty(faction), AiArmyState::Resting(ai::AI_THINK_SECS)),
        };
        let army = army.clone();
        spawn_ai_army_entity(&mut commands, &mut art, &army, i, &world);
    }

    let hero_tile = world.terrain.tile_of(hero.translation.truncate());
    for i in 0..world.bandits.len() {
        let squad = &world.bandits[i];
        if squad.alive {
            continue;
        }
        if squad.respawn_in > dt {
            world.bandits[i].respawn_in -= dt;
            continue;
        }
        let pos = world.terrain.world_pos(world.random_walkable_tile(&mut rng, hero_tile, 12));
        let squad = &mut world.bandits[i];
        *squad = BanditSquad {
            pos,
            army: random_bandit_army(&mut rng),
            gold: rng.random_range(BANDIT_GOLD.0..=BANDIT_GOLD.1),
            path: Vec::new(),
            alive: true,
            respawn_in: 0.0,
            fighting: None,
        };
        spawn_bandit_entity(&mut commands, &mut art, &mut meshes, &mut materials, squad, i);
    }
}

/// Keep the floating labels in sync with the armies they describe.
pub fn refresh_army_labels(
    world: Res<WorldState>,
    ai: Query<(&AiArmyMarker, &Children)>,
    bandits: Query<(&BanditMarker, &Children)>,
    mut labels: Query<&mut Text2d, With<ArmyLabel>>,
) {
    let mut set = |children: &Children, text: String| {
        for child in children.iter() {
            if let Ok(mut label) = labels.get_mut(child)
                && label.0 != text
            {
                label.0 = text.clone();
            }
        }
    };
    for (marker, children) in &ai {
        let army = &world.armies[marker.0];
        set(children, army_text(army, &world));
    }
    for (marker, children) in &bandits {
        set(children, bandit_text(&world.bandits[marker.0]));
    }
}

/// Which way a roamer at `pos` walking `path` faces: `Some(true)` = left (flipped sheet), `None`
/// when the next step is straight up or down (keep facing).
fn facing(pos: Vec2, path: &[IVec2], terrain: &TerrainMap) -> Option<bool> {
    let next = terrain.world_pos(*path.first()?);
    let dx = next.x - pos.x;
    (dx.abs() >= 1.0).then_some(dx < 0.0)
}

/// Roamers animate what they do: run while walking, attack (looping) while taking a tower, idle
/// while watching the capture, resting at the castle or standing with nowhere to go.
pub fn animate_roamers(
    world: Res<WorldState>,
    ai: Query<(&AiArmyMarker, &Children)>,
    bandits: Query<(&BanditMarker, &Children)>,
    mut visuals: Query<&mut UnitVisual>,
) {
    let mut set = |children: &Children, anim: UnitAnim, flip: Option<bool>| {
        for child in children.iter() {
            if let Ok(mut visual) = visuals.get_mut(child) {
                let flip_x = flip.unwrap_or(visual.flip_x);
                if visual.anim != anim || visual.flip_x != flip_x {
                    visual.play(anim, false);
                    visual.flip_x = flip_x;
                }
            }
        }
    };
    for (marker, children) in &ai {
        let army = &world.armies[marker.0];
        let moving = !army.path.is_empty();
        match army.state {
            _ if moving && !matches!(army.state, AiArmyState::Capturing(_) | AiArmyState::Leaving(_)) => {
                set(children, UnitAnim::Run, facing(army.pos, &army.path, &world.terrain));
            }
            AiArmyState::Capturing(_) => {
                let goal = army.target.map(|t| world.terrain.world_pos(t.tile(&world)));
                let (dir, flip) = AttackDir::towards(army.pos, goal.unwrap_or(army.pos));
                set(children, UnitAnim::Attack(dir), flip);
            }
            AiArmyState::Fighting { foe, .. } => {
                let at = match foe {
                    Foe::Army(j) => world.armies[j].pos,
                    Foe::Bandits(b) => world.bandits[b].pos,
                };
                let (dir, flip) = AttackDir::towards(army.pos, at);
                set(children, UnitAnim::Attack(dir), flip);
            }
            _ => set(children, UnitAnim::Idle, None),
        }
    }
    for (marker, children) in &bandits {
        let squad = &world.bandits[marker.0];
        if let Some(a) = squad.fighting {
            let (dir, flip) = AttackDir::towards(squad.pos, world.armies[a].pos);
            set(children, UnitAnim::Attack(dir), flip);
        } else if squad.path.is_empty() {
            set(children, UnitAnim::Idle, None);
        } else {
            set(children, UnitAnim::Run, facing(squad.pos, &squad.path, &world.terrain));
        }
    }
}

/// Walking into a bandit's vision circle or touching an AI army starts a fight. An army besieging
/// one of your places does not: you fight it from there with JOIN, alongside the garrison.
pub fn roamer_encounters(
    mut commands: Commands,
    hero: Single<&Transform, With<Hero>>,
    world: Res<WorldState>,
    player_army: Res<PlayerArmy>,
    mut nearby: ResMut<Nearby>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let hero_pos = hero.translation.truncate();
    let bandit = world
        .bandits
        .iter()
        .enumerate()
        .find(|(_, b)| b.alive && b.pos.distance(hero_pos) <= BANDIT_VISION)
        .map(|(i, _)| EncounterSource::Bandits(i));
    let army = world
        .armies
        .iter()
        .enumerate()
        .find(|(_, a)| {
            let besieging_you = matches!(a.state, AiArmyState::Capturing(_))
                && a.target.is_some_and(|t| t.owner(&world).is_player());
            a.alive && !besieging_you && a.pos.distance(hero_pos) <= TOUCH_DISTANCE
        })
        .map(|(i, _)| EncounterSource::AiArmy(i));
    if let Some(source) = bandit.or(army) {
        start_fight(&mut commands, &mut next_state, &mut nearby, &player_army, &world, source);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::overworld::economy::Treasuries;

    /// Run the AI (buying + armies) for `minutes` of game time at 10 ticks a second.
    fn simulate(seed: u64, minutes: u32) -> (WorldState, Treasuries) {
        let mut app = App::new();
        app.insert_resource(WorldState::generate(seed))
            .insert_resource(Nearby::default())
            .insert_resource(Treasuries::default())
            .insert_resource(Time::<()>::default())
            .add_systems(Update, (ai::ai_kingdoms_buy, ai_armies_act).chain());
        for _ in 0..minutes * 600 {
            app.world_mut().resource_mut::<Time>().advance_by(Duration::from_millis(100));
            app.update();
        }
        let world = app.world_mut().remove_resource::<WorldState>().unwrap();
        let treasuries = app.world_mut().remove_resource::<Treasuries>().unwrap();
        (world, treasuries)
    }

    #[test]
    fn targets_are_near_weak_and_never_too_strong() {
        let world = WorldState::generate(3);
        let army = &world.armies[0];
        let here = world.terrain.tile_of(army.pos);
        if let Some((t, path)) = choose_target(army.faction, &army.army, here, &world.towers, &world.bases, &world.terrain, &[]) {
            assert_ne!(t.owner(&world), army.faction);
            let reach = TERRITORY_TILES + TARGET_REACH_TILES;
            assert!(territory_distance(army.faction, t.tile(&world), &world.towers, &world.bases) <= reach, "stays near home");
            assert!(autoresolve::strength(t.army(&world)) <= AI_ATTACK_RATIO * autoresolve::strength(&army.army));
            assert_eq!(*path.last().unwrap(), t.tile(&world));
        }
        let weak = Army::new(army.faction, &[(UnitKind::Pawn, 1)]);
        assert!(
            choose_target(army.faction, &weak, here, &world.towers, &world.bases, &world.terrain, &[]).is_none(),
            "one pawn attacks nothing"
        );
    }

    #[test]
    fn an_empty_castle_is_a_target() {
        let mut world = WorldState::generate(3);
        let army = world.armies[0].clone();
        // Strip every other kingdom's castle and every tower guard but one castle's garrison.
        let victim = world.bases.iter().position(|b| b.owner != army.faction).unwrap();
        for tower in &mut world.towers {
            tower.owner = army.faction;
        }
        world.bases[victim].garrison = Army::empty(world.bases[victim].owner);
        // A tower of ours next to it, so the castle is within reach of our territory.
        world.towers[0].tile = world.bases[victim].centre() + IVec2::new(0, 8);
        let here = world.terrain.tile_of(army.pos);
        let (t, _) = choose_target(army.faction, &army.army, here, &world.towers, &world.bases, &world.terrain, &[])
            .expect("an undefended castle should be attacked");
        assert_eq!(t, Garrison::Castle(victim));
        // ...unless another kingdom is already besieging it.
        let busy = [(Garrison::Castle(victim), Faction::Black)];
        let other = choose_target(army.faction, &army.army, here, &world.towers, &world.bases, &world.terrain, &busy);
        assert_ne!(other.map(|(t, _)| t), Some(Garrison::Castle(victim)));
    }

    /// One AI army alone at home, everyone else gone, and `secs` of AI.
    fn lone_army(setup: impl FnOnce(&mut WorldState), secs: u32) -> (WorldState, Treasuries) {
        let mut world = WorldState::generate(3);
        for army in world.armies.iter_mut().skip(1) {
            army.alive = false;
            army.respawn_in = f32::INFINITY;
        }
        world.bandits.iter_mut().for_each(|b| b.alive = false);
        setup(&mut world);
        let mut app = App::new();
        app.insert_resource(world)
            .insert_resource(Nearby::default())
            .insert_resource(Treasuries::default())
            .insert_resource(Time::<()>::default())
            .add_systems(Update, ai_armies_act);
        for _ in 0..secs * 10 {
            app.world_mut().resource_mut::<Time>().advance_by(Duration::from_millis(100));
            app.update();
        }
        let world = app.world_mut().remove_resource::<WorldState>().unwrap();
        let treasuries = app.world_mut().remove_resource::<Treasuries>().unwrap();
        (world, treasuries)
    }

    /// A walkable tile 3-4 tiles from the first army, inside its territory.
    fn spot_near_army(world: &WorldState) -> Vec2 {
        let here = world.terrain.tile_of(world.armies[0].pos);
        let tile = (-4..=4)
            .flat_map(|dx| (-4..=4).map(move |dy| here + IVec2::new(dx, dy)))
            .filter(|t| chebyshev(*t, here) >= 3 && world.terrain.find_path(here, *t).is_some())
            .next()
            .expect("room around the castle");
        world.terrain.world_pos(tile)
    }

    #[test]
    fn armies_hunt_bandits_close_by() {
        let faction = WorldState::generate(3).armies[0].faction;
        let (world, treasuries) = lone_army(
            |world| {
                let pos = spot_near_army(world);
                let squad = &mut world.bandits[0];
                *squad = BanditSquad { pos, army: Army::new(Faction::Black, &[(UnitKind::Pawn, 1)]), gold: 30, path: Vec::new(), alive: true, respawn_in: 0.0, fighting: None };
            },
            20,
        );
        assert!(!world.bandits[0].alive, "the bandits were caught");
        assert!(world.armies[0].alive, "and the army won");
        assert_eq!(treasuries.gold(faction), crate::overworld::economy::KINGDOM_STARTING_GOLD + 30, "their purse went to the kingdom");
    }

    #[test]
    fn armies_hunt_your_workers_in_their_territory() {
        let (world, _) = lone_army(
            |world| {
                let pos = spot_near_army(world);
                world.workers.push(resources::Worker {
                    faction: Faction::Reclaimers,
                    site: 0,
                    castle: 0,
                    pos,
                    path: Vec::new(),
                    state: resources::WorkerState::Waiting(999.0),
                    recalled: false,
                    alive: true,
                });
            },
            20,
        );
        assert!(!world.workers[0].alive, "the worker was caught");
    }

    #[test]
    fn armies_fill_their_towers_first() {
        let (world, _) = lone_army(
            |world| {
                let faction = world.armies[0].faction;
                let here = world.terrain.tile_of(world.armies[0].pos);
                let t = (0..world.towers.len()).find(|&t| world.terrain.find_path(here, world.towers[t].tile).is_some()).unwrap();
                world.towers[t].owner = faction;
                world.towers[t].garrison = Army::empty(faction);
                world.armies[0].army = Army::new(faction, &[(UnitKind::Warrior, 16)]);
            },
            180,
        );
        let faction = world.armies[0].faction;
        let tower = world.towers.iter().find(|t| t.owner == faction).unwrap();
        assert_eq!(tower.garrison.slots(), TOWER_SLOTS, "the tower was filled up");
    }

    #[test]
    fn two_armies_besiege_twice_as_fast() {
        let siege = |helpers: bool| {
            let (world, _) = lone_army(
                |world| {
                    let t = world.towers.iter().position(|t| t.owner == Faction::Black).unwrap();
                    world.towers[t].garrison = Army::empty(Faction::Black);
                    let n = if helpers { 2 } else { 1 };
                    for army in world.armies.iter_mut().take(n) {
                        army.alive = true;
                        army.respawn_in = 0.0;
                        army.pos = world.terrain.world_pos(world.towers[t].tile);
                        army.target = Some(Garrison::Tower(t));
                        army.state = AiArmyState::Capturing(TOWER_CAPTURE_SECS);
                    }
                },
                (TOWER_CAPTURE_SECS * 0.6) as u32,
            );
            world.towers.iter().filter(|t| t.owner != Faction::Black).count()
        };
        assert_eq!(siege(false), 0, "alone it takes the full time");
        assert_eq!(siege(true), 1, "together they are done in half");
    }

    #[test]
    fn beaten_armies_fly_home_as_ghosts_and_wait() {
        let mut world = WorldState::generate(3);
        let faction = world.armies[0].faction;
        let gate = world.terrain.world_pos(world.base_of(faction).unwrap().spawn_tile());
        // Beaten far away, across whatever water or cliffs lie between.
        world.armies[0].pos = gate + Vec2::new(-20.0 * TILE, 10.0 * TILE);
        world.armies[0].die();
        assert!(world.armies[0].ghost && !world.armies[0].alive && world.armies[0].army.is_empty());
        let start = world.armies[0].pos;
        let flight = start.distance(gate) / GHOST_SPEED;
        // Half way: on the straight line home, and the wait has not started.
        for _ in 0..(flight * 5.0) as usize {
            ghosts_fly_home(&mut world, 0.1);
        }
        let p = world.armies[0].pos;
        let off_line = (p - start).perp_dot((gate - start).normalize()).abs();
        assert!(off_line < 1.0, "flies straight, ignoring the terrain");
        assert_eq!(world.armies[0].respawn_in, GHOST_WAIT_SECS);
        // Home, then 30 s of waiting.
        for _ in 0..((flight + GHOST_WAIT_SECS) * 10.0) as usize + 20 {
            ghosts_fly_home(&mut world, 0.1);
        }
        assert_eq!(world.armies[0].pos, gate);
        assert!(world.armies[0].respawn_in <= 0.0, "ready to come back");
    }

    #[test]
    fn armies_that_meet_fight_for_a_while_first() {
        let meet = |secs: u32| {
            lone_army(
                |world| {
                    let other = world.armies.iter().position(|a| a.faction != world.armies[0].faction).unwrap();
                    let pos = world.armies[0].pos;
                    let army = &mut world.armies[other];
                    army.alive = true;
                    army.respawn_in = 0.0;
                    army.pos = pos + Vec2::new(10.0, 0.0);
                },
                secs,
            )
            .0
        };
        let early = meet(2);
        let fighters = early.armies.iter().filter(|a| a.alive && matches!(a.state, AiArmyState::Fighting { .. })).count();
        assert_eq!(fighters, 2, "after 2 s both are still fighting");
        let late = meet((FIGHT_SECS + 2.0) as u32);
        assert_eq!(late.armies.iter().filter(|a| a.alive).count(), 1, "after the fight one is left");
        assert_eq!(late.armies.iter().filter(|a| a.ghost).count(), 1, "the loser flies home as a ghost");
    }

    #[test]
    fn nearby_armies_defend_their_places() {
        let mut world = WorldState::generate(3);
        let faction = world.armies[0].faction;
        world.towers[0].owner = faction;
        let at = world.terrain.world_pos(world.towers[0].tile);
        // An army standing at its tower helps when you attack it.
        world.armies[0].pos = at + Vec2::new(40.0, 0.0);
        assert!(crate::overworld::interact::helpers_of(&world, Garrison::Tower(0)).contains(&0));
        // Your hero close to it sounds the alarm: the nearest free army of that kingdom is sent.
        let defenders = assign_defenders(&world, at + Vec2::new(64.0, 0.0), true, &[]);
        assert_eq!(defenders[0], Some((Garrison::Tower(0), true)));
        // Without an army you are no threat.
        assert!(assign_defenders(&world, at, false, &[]).iter().all(|d| d.is_none()));
    }

    /// Ten minutes of AI on a few seeds: kingdoms spend gold and towers change hands.
    #[test]
    fn kingdoms_spend_gold_and_take_towers() {
        let (mut taken, mut spent) = (0, 0);
        for seed in [1u64, 5, 9] {
            let (world, treasuries) = simulate(seed, 10);
            taken += world.towers.iter().filter(|t| t.owner != Faction::Black).count();
            spent += Faction::KINGDOMS
                .into_iter()
                .filter(|f| !f.is_player())
                .filter(|f| treasuries.gold(*f) < crate::overworld::economy::KINGDOM_STARTING_GOLD)
                .count();
        }
        assert!(taken > 0, "no tower was taken in 30 simulated minutes");
        assert!(spent > 0, "no kingdom spent gold");
    }

    /// Tuning aid: `cargo test sim_report -- --ignored --nocapture` prints tower owners (with guard
    /// size), armies in the field, castle owners (with garrison size) and gold after 5, 10 and 20 minutes of AI on
    /// seeds 1-5. Destroyed armies do not come back here (respawning needs the art), so the
    /// "armies out" count is lower than in the game.
    #[test]
    #[ignore]
    fn sim_report() {
        for seed in 1..=5u64 {
            for minutes in [5u32, 10, 20] {
                let (world, tr) = simulate(seed, minutes);
                let owners: Vec<String> = world.towers.iter().map(|t| format!("{}:{}", &t.owner.name()[..3], t.garrison.slots())).collect();
                let alive = world.armies.iter().filter(|a| a.alive && !a.army.is_empty()).count();
                let garr: Vec<String> = world.bases.iter().map(|b| format!("{}:{}", &b.owner.name()[..3], b.garrison.slots())).collect();
                let gold: Vec<u32> = Faction::KINGDOMS.iter().map(|f| tr.gold(*f)).collect();
                println!("seed {seed} {minutes:2}min: towers {:?} | armies out {alive}/6 | garrisons {:?} | gold {:?}", owners, garr, gold);
            }
        }
    }
}
