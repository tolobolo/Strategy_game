//! Things that walk around the map on their own: AI kingdom armies that grab towers, and
//! Black bandit squads. See agents.md §1.5.

use bevy::prelude::*;
use rand::Rng;

use super::interact::{Nearby, start_fight};
use super::terrain::TerrainMap;
use super::world::{EncounterSource, Tower, WorldState};
use super::{ai, autoresolve};
use super::{Hero, OVERWORLD_UNIT_SCALE, UNIT_FEET, label};
use crate::assets::{AttackDir, UnitAnim, UnitArt, UnitVisual};
use crate::faction::Faction;
use crate::states::GameState;
use crate::units::{Army, CASTLE_SLOTS, PlayerArmy, UnitKind};

pub const AI_ARMIES_PER_KINGDOM: usize = 2;
/// An AI army stands at a tower this long before it falls...
pub const TOWER_CAPTURE_SECS: f32 = 10.0;
/// ...then waits for the capture animation before walking on.
pub const TOWER_LEAVE_SECS: f32 = 1.5;
pub const AI_ARMY_SPEED: f32 = 80.0;
/// A destroyed AI army comes back (drawn from its castle garrison) after this long.
pub const AI_RESPAWN_SECS: f32 = 60.0;
/// An army that came home rests this long before drawing a new army from the garrison.
pub const AI_REST_SECS: f32 = 20.0;
/// An AI army skips towers whose guard is stronger than this × its own strength.
pub const AI_ATTACK_RATIO: f32 = 0.8;
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
    /// At the castle; seconds until it tries to draw a new army from the garrison.
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
    // Home with its units in the castle: waiting for enough troops for a new army.
    if army.is_empty() {
        return format!("{} army\ngathering troops...", faction.name());
    }
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

/// The tower an AI army of `army` strength at `here` should take next, and the path there: among
/// the towers not owned by `faction` that it can walk to, skip any whose guard is stronger than
/// `AI_ATTACK_RATIO` × its own, and pick the lowest walking distance × (1 + guard / own).
pub fn choose_target(
    faction: Faction,
    army: &Army,
    here: IVec2,
    towers: &[Tower],
    terrain: &TerrainMap,
) -> Option<(usize, Vec<IVec2>)> {
    let own = autoresolve::strength(army).max(1.0);
    towers
        .iter()
        .enumerate()
        .filter(|(_, t)| t.owner != faction)
        .filter(|(_, t)| autoresolve::strength(&t.garrison) <= AI_ATTACK_RATIO * own)
        .filter_map(|(i, t)| terrain.find_path(here, t.tile).map(|p| (i, p)))
        .map(|(i, p)| {
            let score = (p.len() as f32 + 1.0) * (1.0 + autoresolve::strength(&towers[i].garrison) / own);
            (score, i, p)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, i, p)| (i, p))
}

/// Put an army's units back into its castle garrison (as many as fit).
fn join_garrison(garrison: &mut Army, army: &mut Army) {
    for stack in std::mem::replace(army, Army::empty(army.faction)).stacks {
        let room = CASTLE_SLOTS.saturating_sub(garrison.slots());
        garrison.add(stack.kind, stack.count.min(room));
    }
}

/// AI armies path (A*, same rules as the hero) to a tower picked by `choose_target`, stand at it
/// for `TOWER_CAPTURE_SECS`, fight its guard by auto-resolve, and on a win take it (leaving 10–30 %
/// of their units behind as the new guard), wait for the capture animation and walk on. With fewer
/// than `AI_RETREAT_BELOW` units they go home, join the garrison, rest and set out again with an
/// army drawn from the garrison. Armies of different kingdoms that meet fight by auto-resolve.
pub fn ai_armies_act(
    time: Res<Time>,
    mut world: ResMut<WorldState>,
    mut nearby: ResMut<Nearby>,
    mut roamers: Query<(&AiArmyMarker, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::rng();
    let WorldState { armies, towers, terrain, bases, .. } = &mut *world;

    for army in armies.iter_mut().filter(|a| a.alive) {
        let here = terrain.tile_of(army.pos);
        let home = bases.iter().position(|b| b.home == army.faction && b.owner == army.faction);
        let home_tile = home.map(|h| bases[h].spawn_tile());
        // Standing at a tower: count down, fight for it, then watch the animation before leaving.
        match army.state {
            AiArmyState::Capturing(left) => {
                let left = left - dt;
                if left > 0.0 {
                    army.state = AiArmyState::Capturing(left);
                } else if let Some(t) = army.target {
                    let tower = &mut towers[t];
                    let defender = tower.owner;
                    if defender == army.faction {
                        army.target = None;
                        army.state = AiArmyState::Walking;
                        continue;
                    }
                    if autoresolve::resolve(&mut army.army, &mut tower.garrison, &mut rng) {
                        let share = rng.random_range(TOWER_TAKE_LOSS.0..=TOWER_TAKE_LOSS.1);
                        let left_behind = army.army.split_off_fraction(share);
                        tower.change_owner(army.faction, left_behind);
                        army.state = AiArmyState::Leaving(TOWER_LEAVE_SECS);
                        if defender.is_player() {
                            nearby.say_for(format!("The {} army took your tower!", army.faction.name()), 4.0);
                        }
                    } else {
                        if defender.is_player() {
                            nearby.say_for(format!("Your tower held against the {} army.", army.faction.name()), 4.0);
                        }
                        army.state = AiArmyState::Walking;
                    }
                    army.target = None;
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
            AiArmyState::Walking => {}
        }

        // Pick a tower: near and weak first; towers nobody can reach or that are too strong are skipped.
        let target_still_valid = army.target.is_some_and(|t| towers[t].owner != army.faction);
        if !target_still_valid {
            match choose_target(army.faction, &army.army, here, towers, terrain) {
                Some((i, path)) => {
                    army.target = Some(i);
                    army.path = path;
                }
                None => {
                    army.target = None;
                    army.path.clear();
                    continue; // nothing worth taking: stand where we are
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

    // Armies of different kingdoms that meet fight it out.
    for i in 0..armies.len() {
        for j in i + 1..armies.len() {
            let (a, b) = armies.split_at_mut(j);
            let (a, b) = (&mut a[i], &mut b[0]);
            let meet = a.alive
                && b.alive
                && a.faction != b.faction
                && !a.army.is_empty()
                && !b.army.is_empty()
                && a.pos.distance(b.pos) <= TOUCH_DISTANCE;
            if !meet {
                continue;
            }
            let a_won = autoresolve::resolve(&mut a.army, &mut b.army, &mut rng);
            for (army, lost) in [(a, !a_won), (b, a_won)] {
                if lost || army.army.is_empty() {
                    army.alive = false;
                    army.respawn_in = AI_RESPAWN_SECS;
                    army.target = None;
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
        army.respawn_in = 0.0;
        army.pos = home;
        army.target = None;
        army.path.clear();
        (army.army, army.state) = match fresh {
            Some(fresh) => (fresh, AiArmyState::Walking),
            None => (Army::empty(faction), AiArmyState::Resting(ai::AI_THINK_SECS)),
        };
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
            AiArmyState::Walking | AiArmyState::Returning if moving => {
                set(children, UnitAnim::Run, facing(army.pos, &army.path, &world.terrain));
            }
            AiArmyState::Capturing(_) => {
                let tower = army.target.map(|t| world.terrain.world_pos(world.towers[t].tile));
                let (dir, flip) = AttackDir::towards(army.pos, tower.unwrap_or(army.pos));
                set(children, UnitAnim::Attack(dir), flip);
            }
            _ => set(children, UnitAnim::Idle, None),
        }
    }
    for (marker, children) in &bandits {
        let squad = &world.bandits[marker.0];
        if squad.path.is_empty() {
            set(children, UnitAnim::Idle, None);
        } else {
            set(children, UnitAnim::Run, facing(squad.pos, &squad.path, &world.terrain));
        }
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
        if let Some((t, path)) = choose_target(army.faction, &army.army, here, &world.towers, &world.terrain) {
            assert_ne!(world.towers[t].owner, army.faction);
            assert!(autoresolve::strength(&world.towers[t].garrison) <= AI_ATTACK_RATIO * autoresolve::strength(&army.army));
            assert_eq!(*path.last().unwrap(), world.towers[t].tile);
        }
        let weak = Army::new(army.faction, &[(UnitKind::Pawn, 1)]);
        assert!(choose_target(army.faction, &weak, here, &world.towers, &world.terrain).is_none(), "one pawn attacks nothing");
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
    /// size), armies in the field, castle garrisons and gold after 5, 10 and 20 minutes of AI on
    /// seeds 1-5. Destroyed armies do not come back here (respawning needs the art), so the
    /// "armies out" count is lower than in the game.
    #[test]
    #[ignore]
    fn sim_report() {
        for seed in 1..=5u64 {
            for minutes in [5u32, 10, 20] {
                let (world, tr) = simulate(seed, minutes);
                let owners: Vec<String> = world.towers.iter().map(|t| format!("{}:{}", &t.owner.name()[..2], t.garrison.slots())).collect();
                let alive = world.armies.iter().filter(|a| a.alive && !a.army.is_empty()).count();
                let garr: Vec<u32> = world.bases.iter().map(|b| b.garrison.slots()).collect();
                let gold: Vec<u32> = Faction::KINGDOMS.iter().map(|f| tr.gold(*f)).collect();
                println!("seed {seed} {minutes:2}min: towers {:?} | armies out {alive}/6 | garrisons {:?} | gold {:?}", owners, garr, gold);
            }
        }
    }
}
