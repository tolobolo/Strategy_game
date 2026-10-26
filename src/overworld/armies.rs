//! Things that walk around the map on their own: AI kingdom armies that grab towers, and
//! Black bandit squads. See agents.md §1.5.

use bevy::prelude::*;
use rand::Rng;

use super::interact::{Nearby, start_fight};
use super::world::{EncounterSource, WorldState};
use super::{Hero, label};
use crate::faction::Faction;
use crate::states::GameState;
use crate::units::{Army, PlayerArmy, UnitKind};

pub const AI_ARMIES_PER_KINGDOM: usize = 2;
pub const AI_ARMY_SPEED: f32 = 80.0;
pub const AI_RESPAWN_SECS: f32 = 60.0;
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

/// A kingdom's roaming army. Walks to the nearest tower it does not own and takes it.
#[derive(Clone, Debug)]
pub struct AiArmy {
    pub faction: Faction,
    pub pos: Vec2,
    pub army: Army,
    pub target: Option<usize>,
    pub alive: bool,
    pub respawn_in: f32,
}

/// Black bandits wandering between random points, carrying gold.
#[derive(Clone, Debug)]
pub struct BanditSquad {
    pub pos: Vec2,
    pub army: Army,
    pub gold: u32,
    pub target: Vec2,
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
                target: pos,
                alive: true,
                respawn_in: 0.0,
            }
        })
        .collect()
}

// ---------- entities ----------

pub fn spawn_ai_army_entity(commands: &mut Commands, army: &AiArmy, index: usize) {
    commands.spawn((
        AiArmyMarker(index),
        army.faction,
        Sprite::from_color(army.faction.color(), Vec2::splat(SQUARE)),
        Transform::from_translation(army.pos.extend(3.0)),
        DespawnOnExit(GameState::Overworld),
        children![
            (
                Sprite::from_color(Color::WHITE, Vec2::splat(SQUARE + 6.0)),
                Transform::from_xyz(0.0, 0.0, -0.1),
                Pickable::IGNORE,
            ),
            (ArmyLabel, label(army_text(army.faction, &army.army), Color::WHITE, Vec2::new(0.0, SQUARE))),
        ],
    ));
}

pub fn spawn_bandit_entity(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    squad: &BanditSquad,
    index: usize,
) {
    commands.spawn((
        BanditMarker(index),
        Faction::Black,
        Sprite::from_color(Faction::Black.color(), Vec2::splat(SQUARE)),
        Transform::from_translation(squad.pos.extend(3.0)),
        DespawnOnExit(GameState::Overworld),
        children![
            (
                Mesh2d(meshes.add(Circle::new(BANDIT_VISION))),
                MeshMaterial2d(materials.add(Color::srgba(1.0, 0.2, 0.2, 0.12))),
                Transform::from_xyz(0.0, 0.0, -0.5),
            ),
            (ArmyLabel, label(bandit_text(squad), Color::srgb(1.0, 0.5, 0.4), Vec2::new(0.0, SQUARE))),
        ],
    ));
}

fn army_text(faction: Faction, army: &Army) -> String {
    format!("{} army\n{}", faction.name(), army.summary())
}

fn bandit_text(squad: &BanditSquad) -> String {
    format!("Bandits ({}g)\n{}", squad.gold, squad.army.summary())
}

// ---------- systems (Update, Overworld) ----------

/// AI armies walk to the nearest tower they do not own and take it by touching it, leaving
/// 10–30 % of their units behind as the tower's garrison. They never fight.
pub fn ai_armies_act(
    time: Res<Time>,
    mut world: ResMut<WorldState>,
    mut roamers: Query<(&AiArmyMarker, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::rng();
    let WorldState { armies, towers, terrain, .. } = &mut *world;

    for army in armies.iter_mut().filter(|a| a.alive) {
        let target_still_valid = army.target.is_some_and(|t| towers[t].owner != army.faction);
        if !target_still_valid {
            army.target = towers
                .iter()
                .enumerate()
                .filter(|(_, t)| t.owner != army.faction)
                .min_by_key(|(_, t)| terrain.world_pos(t.tile).distance(army.pos) as i32)
                .map(|(i, _)| i);
        }
        let Some(t) = army.target else { continue };
        let goal = terrain.world_pos(towers[t].tile);
        let to_goal = goal - army.pos;
        if to_goal.length() <= TOUCH_DISTANCE {
            let share = rng.random_range(TOWER_TAKE_LOSS.0..=TOWER_TAKE_LOSS.1);
            let left_behind = army.army.split_off_fraction(share);
            towers[t].owner = army.faction;
            towers[t].garrison = left_behind;
            army.target = None;
            if army.army.is_empty() {
                army.alive = false;
                army.respawn_in = AI_RESPAWN_SECS;
            }
        } else {
            army.pos += to_goal.normalize() * AI_ARMY_SPEED * dt;
        }
    }

    for (marker, mut transform) in &mut roamers {
        let pos = world.armies[marker.0].pos;
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
    }
}

/// Bandits drift between random walkable points.
pub fn bandits_wander(
    time: Res<Time>,
    mut world: ResMut<WorldState>,
    mut roamers: Query<(&BanditMarker, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::rng();
    let WorldState { bandits, terrain, bases, .. } = &mut *world;

    for squad in bandits.iter_mut().filter(|b| b.alive) {
        let to_target = squad.target - squad.pos;
        if to_target.length() < 8.0 {
            let here = terrain.tile_of(squad.pos);
            for _ in 0..20 {
                let candidate = here + IVec2::new(rng.random_range(-8..=8), rng.random_range(-8..=8));
                let ok = terrain.get(candidate).is_some_and(|t| t.walkable()) && !bases.iter().any(|b| b.contains(candidate));
                if ok {
                    squad.target = terrain.world_pos(candidate);
                    break;
                }
            }
        } else {
            squad.pos += to_target.normalize() * BANDIT_SPEED * dt;
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
        spawn_ai_army_entity(&mut commands, army, i);
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
            target: pos,
            alive: true,
            respawn_in: 0.0,
        };
        spawn_bandit_entity(&mut commands, &mut meshes, &mut materials, squad, i);
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
        set(children, army_text(army.faction, &army.army));
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
