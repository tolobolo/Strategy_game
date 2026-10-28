//! Things that walk around the map on their own: AI kingdom armies that grab towers, and
//! Black bandit squads. See agents.md §1.5.

use bevy::prelude::*;
use rand::Rng;

use super::interact::{Nearby, start_fight};
use super::terrain::TerrainMap;
use super::world::{EncounterSource, WorldState};
use super::{Hero, OVERWORLD_UNIT_SCALE, UNIT_FEET, label};
use crate::assets::{UnitArt, UnitVisual};
use crate::faction::Faction;
use crate::states::GameState;
use crate::units::{Army, PlayerArmy, UnitKind};

pub const AI_ARMIES_PER_KINGDOM: usize = 2;
/// An AI army stands at a tower this long before it falls...
pub const TOWER_CAPTURE_SECS: f32 = 10.0;
/// ...then waits for the capture animation before walking on.
pub const TOWER_LEAVE_SECS: f32 = 1.5;
pub const AI_ARMY_SPEED: f32 = 80.0;
pub const AI_RESPAWN_SECS: f32 = 60.0;
/// An army with fewer units than this goes home after a capture, rests `AI_RESPAWN_SECS` and
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
const SQUARE: f32 = 40.0;

/// What a roaming AI army is doing right now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AiArmyState {
    Walking,
    /// Standing at its target tower; seconds until it falls.
    Capturing(f32),
    /// Watching the capture animation; seconds until it walks on.
    Leaving(f32),
    /// Too few units left: walking back to its castle.
    Returning,
    /// At the castle; seconds until it has a fresh army.
    Resting(f32),
}

/// A kingdom's roaming army. Walks (A*, same rules as the hero) to the nearest reachable tower it
/// does not own, stands there for `TOWER_CAPTURE_SECS`, takes it, and moves on.
#[derive(Clone, Debug)]
pub struct AiArmy {
    pub faction: Faction,
    pub pos: Vec2,
    pub army: Army,
    pub target: Option<usize>,
    /// Tiles still to walk, first one next.
    pub path: Vec<IVec2>,
    pub state: AiArmyState,
    pub alive: bool,
    pub respawn_in: f32,
}

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
            }
        })
        .collect()
}

// ---------- entities ----------

pub fn spawn_ai_army_entity(commands: &mut Commands, art: &mut UnitArt, army: &AiArmy, index: usize) {
    commands.spawn((
        AiArmyMarker(index),
        army.faction,
        Transform::from_translation(army.pos.extend(3.0)),
        Visibility::default(),
        DespawnOnExit(GameState::Overworld),
        children![
            art.bundle(UnitVisual::new(army.faction, UnitKind::Warrior, false), OVERWORLD_UNIT_SCALE, UNIT_FEET, 0.1),
            (ArmyLabel, label(army_text(army.faction, &army.army, army.state), Color::WHITE, Vec2::new(0.0, SQUARE))),
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

fn army_text(faction: Faction, army: &Army, state: AiArmyState) -> String {
    let doing = match state {
        AiArmyState::Walking => String::new(),
        AiArmyState::Capturing(left) => format!("\ncapturing... {}s", left.ceil() as u32),
        AiArmyState::Leaving(_) => "\ntaken!".to_string(),
        AiArmyState::Returning => "\nreturning home".to_string(),
        AiArmyState::Resting(left) => format!("\nresting... {}s", left.ceil() as u32),
    };
    format!("{} army\n{}{doing}", faction.name(), army.summary())
}

fn bandit_text(squad: &BanditSquad) -> String {
    format!("Bandits ({}g)\n{}", squad.gold, squad.army.summary())
}

// ---------- systems (Update, Overworld) ----------

/// Walk along `path` at `speed`, popping waypoints as they are reached. True when nothing is left.
fn follow_path(pos: &mut Vec2, path: &mut Vec<IVec2>, terrain: &TerrainMap, speed: f32, dt: f32) -> bool {
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

/// AI armies path (A*, same rules as the hero: no water, layers only via stairs) to the nearest
/// reachable tower they do not own, stand at it for `TOWER_CAPTURE_SECS`, take it (leaving
/// 10–30 % of their units behind as the garrison), wait for the capture animation and walk on.
/// With fewer than `AI_RETREAT_BELOW` units they go home, rest and come back fresh. They never fight.
pub fn ai_armies_act(
    time: Res<Time>,
    mut world: ResMut<WorldState>,
    mut roamers: Query<(&AiArmyMarker, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::rng();
    let WorldState { armies, towers, terrain, bases, .. } = &mut *world;

    for army in armies.iter_mut().filter(|a| a.alive) {
        let here = terrain.tile_of(army.pos);
        let home_tile = bases.iter().find(|b| b.home == army.faction).map(|b| b.spawn_tile());
        // Standing at a tower: count down, take it, then watch the animation before leaving.
        match army.state {
            AiArmyState::Capturing(left) => {
                let left = left - dt;
                if left > 0.0 {
                    army.state = AiArmyState::Capturing(left);
                } else if let Some(t) = army.target {
                    let share = rng.random_range(TOWER_TAKE_LOSS.0..=TOWER_TAKE_LOSS.1);
                    let left_behind = army.army.split_off_fraction(share);
                    towers[t].change_owner(army.faction, left_behind);
                    army.target = None;
                    army.state = AiArmyState::Leaving(TOWER_LEAVE_SECS);
                    if army.army.is_empty() {
                        army.alive = false;
                        army.respawn_in = AI_RESPAWN_SECS;
                    }
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
                    army.state = AiArmyState::Resting(AI_RESPAWN_SECS);
                }
                continue;
            }
            AiArmyState::Resting(left) => {
                let left = left - dt;
                if left > 0.0 {
                    army.state = AiArmyState::Resting(left);
                } else {
                    army.army = random_ai_army(army.faction, &mut rng);
                    army.target = None;
                    army.path.clear();
                    army.state = AiArmyState::Walking;
                }
                continue;
            }
            AiArmyState::Walking => {}
        }

        // Pick the nearest tower by walking distance; towers nobody can reach are skipped.
        let target_still_valid = army.target.is_some_and(|t| towers[t].owner != army.faction);
        if !target_still_valid {
            let best = towers
                .iter()
                .enumerate()
                .filter(|(_, t)| t.owner != army.faction)
                .filter_map(|(i, t)| terrain.find_path(here, t.tile).map(|p| (i, p)))
                .min_by_key(|(_, p)| p.len());
            match best {
                Some((i, path)) => {
                    army.target = Some(i);
                    army.path = path;
                }
                None => {
                    army.target = None;
                    army.path.clear();
                    continue; // nothing reachable: stand where we are
                }
            }
        }
        let Some(t) = army.target else { continue };
        let goal = terrain.world_pos(towers[t].tile);
        if army.path.is_empty() && goal.distance(army.pos) > TOUCH_DISTANCE {
            // Pushed off course (e.g. restored after a fight): plan again.
            match terrain.find_path(here, towers[t].tile) {
                Some(path) => army.path = path,
                None => {
                    army.target = None;
                    continue;
                }
            }
        }
        if follow_path(&mut army.pos, &mut army.path, terrain, AI_ARMY_SPEED, dt) {
            army.state = AiArmyState::Capturing(TOWER_CAPTURE_SECS);
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
    let WorldState { bandits, terrain, bases, .. } = &mut *world;

    for squad in bandits.iter_mut().filter(|b| b.alive) {
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

    for i in 0..world.armies.len() {
        let army = &world.armies[i];
        if army.alive || !army.respawn_in.is_finite() {
            continue;
        }
        if army.respawn_in > dt {
            world.armies[i].respawn_in -= dt;
            continue;
        }
        // Only kingdoms that still hold their own castle raise new armies.
        let Some(base) = world.base_of(army.faction).filter(|b| b.owner == army.faction) else {
            world.armies[i].respawn_in = f32::INFINITY;
            continue;
        };
        let home = world.terrain.world_pos(base.spawn_tile());
        let fresh = random_ai_army(army.faction, &mut rng);
        let army = &mut world.armies[i];
        army.alive = true;
        army.respawn_in = 0.0;
        army.pos = home;
        army.army = fresh;
        army.target = None;
        army.path.clear();
        army.state = AiArmyState::Walking;
        spawn_ai_army_entity(&mut commands, &mut art, army, i);
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
        set(children, army_text(army.faction, &army.army, army.state));
    }
    for (marker, children) in &bandits {
        set(children, bandit_text(&world.bandits[marker.0]));
    }
}

/// Walking into a bandit's vision circle or touching an AI army starts a fight.
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
        .find(|(_, a)| a.alive && a.pos.distance(hero_pos) <= TOUCH_DISTANCE)
        .map(|(i, _)| EncounterSource::AiArmy(i));
    if let Some(source) = bandit.or(army) {
        start_fight(&mut commands, &mut next_state, &mut nearby, &player_army, &world, source);
    }
}
