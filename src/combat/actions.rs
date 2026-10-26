//! Everything a stack can do. Player input and the enemy AI both write `CombatAction`
//! messages; `resolve_actions` applies the rules once, in one place. Every action is followed by
//! a lock (see the `*_SECS` constants) so it can be seen on screen before the next one.

use std::collections::HashSet;

use bevy::prelude::*;

use super::grid::{BattleGrid, GridPos, in_strike_range};
use super::hud::CombatLog;
use super::spawn::spawn_stack;
use super::turn::{self, ActionPoints, ActiveStack, TurnOrder};
use crate::assets::{AttackDir, Fx, UnitAnim, UnitArt, UnitVisual, anim_duration, spawn_effect};
use crate::faction::Faction;
use crate::units::rules::{apply_damage, merge, split, stack_damage};
use crate::states::GameState;
use crate::units::{ShieldUp, Stack, Team};

/// Defence lost by a target that has an enemy on its far side (agents.md §3.2).
const FLANK_PENALTY: i32 = -1;
/// Defence gained from Shield up until the stack's next turn.
const SHIELD_BONUS: u32 = 3;
/// No action ever locks the battle longer than this.
pub const MAX_ACTION_SECS: f32 = 2.0;
/// A move costs this for its first step...
pub const MOVE_FIRST_STEP_SECS: f32 = 1.0;
/// ...and this for every step after it.
pub const MOVE_EXTRA_STEP_SECS: f32 = 0.5;
/// An arrow needs this long per tile of distance.
pub const ARROW_SECS_PER_TILE: f32 = 0.25;
/// A strike lasts at least this long even if the attack animation is shorter.
pub const MIN_STRIKE_SECS: f32 = 0.5;
/// Unstack, Merge and Shield up.
pub const QUICK_ACTION_SECS: f32 = 0.5;
pub const END_TURN_SECS: f32 = 0.3;

/// 1 s for the first step, 0.5 s for each further one, capped.
pub fn move_duration(steps: i32) -> f32 {
    (MOVE_FIRST_STEP_SECS + MOVE_EXTRA_STEP_SECS * (steps - 1).max(0) as f32).min(MAX_ACTION_SECS)
}

/// The attack animation, or the arrow's flight if that is longer, capped.
pub fn strike_duration(kind: crate::units::UnitKind, ranged_distance: Option<i32>) -> f32 {
    let animation = anim_duration(kind, UnitAnim::Attack(AttackDir::Right));
    let flight = ranged_distance.map_or(0.0, |d| d as f32 * ARROW_SECS_PER_TILE);
    animation.max(flight).max(MIN_STRIKE_SECS).min(MAX_ACTION_SECS)
}

#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatAction {
    /// Move the active stack to a tile (1 AP, within `speed` steps).
    Move(GridPos),
    /// Attack an enemy stack (1 AP). Melee needs orthogonal adjacency; ranged needs the
    /// distance to lie within the kind's `range` span.
    Strike(Entity),
    /// Split into two equal halves (1 AP). The new half goes to the first free tile, starting
    /// to the right and going clockwise, and acts right before this stack from the next round.
    Unstack,
    /// Merge with an adjacent friendly stack of the same kind (1 AP).
    Merge(Entity),
    /// +3 defence until this stack's next turn (1 AP).
    ShieldUp,
    EndTurn,
}

/// While locked, no input or AI is accepted. When the lock runs out and `then_end_turn` is set,
/// the turn passes on.
#[derive(Resource, Default)]
pub struct ActionLock {
    timer: Timer,
    locked: bool,
    then_end_turn: Option<Entity>,
}

impl ActionLock {
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    pub fn lock(&mut self, seconds: f32) {
        self.timer = Timer::from_seconds(seconds, TimerMode::Once);
        self.locked = true;
    }
}

/// Stack sliding from one tile to another during the action delay.
#[derive(Component)]
pub struct MoveTween {
    from: Vec2,
    to: Vec2,
    timer: Timer,
}

/// An arrow on its way from an archer to its target. Removed when it lands.
#[derive(Component)]
pub struct Projectile {
    from: Vec2,
    to: Vec2,
    timer: Timer,
}

/// Dust_02 is drawn this big over a dying stack (the sheet is 64 px, a tile 72).
const DEATH_DUST_SCALE: f32 = 1.4;

/// A stack with no units left. It stays on the board until the blow lands (sword mid-swing or
/// arrow arriving), when the death dust replaces it; the action lock despawns any leftovers.
#[derive(Component)]
pub struct Dying;

/// Keyboard part of the player's input. Clicks are handled by observers in `spawn.rs`.
pub fn player_input(
    keys: Res<ButtonInput<KeyCode>>,
    lock: Res<ActionLock>,
    active: Query<&Team, With<ActiveStack>>,
    mut actions: MessageWriter<CombatAction>,
) {
    if lock.is_locked() || !matches!(active.single(), Ok(Team::Player)) {
        return;
    }
    if keys.any_just_pressed([KeyCode::Space, KeyCode::Enter]) {
        actions.write(CombatAction::EndTurn);
    }
    if keys.just_pressed(KeyCode::Digit1) {
        actions.write(CombatAction::Unstack);
    }
    if keys.just_pressed(KeyCode::Digit3) {
        actions.write(CombatAction::ShieldUp);
    }
}

/// Extra pause before each enemy action, on top of the action lock.
#[derive(Resource)]
pub struct EnemyThinkTimer(pub Timer);

impl Default for EnemyThinkTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(0.3, TimerMode::Repeating))
    }
}

/// Very small AI: strike the nearest player stack if it can, otherwise walk towards it.
pub fn enemy_ai(
    time: Res<Time>,
    mut timer: ResMut<EnemyThinkTimer>,
    lock: Res<ActionLock>,
    grid: Res<BattleGrid>,
    active: Query<(Entity, &Team, &GridPos, &Stack), With<ActiveStack>>,
    stacks: Query<(Entity, &GridPos, &Team), (With<Stack>, Without<Dying>)>,
    mut actions: MessageWriter<CombatAction>,
) {
    let Ok((me, team, my_pos, my_stack)) = active.single() else {
        return;
    };
    if *team != Team::Enemy || lock.is_locked() {
        timer.0.reset();
        return;
    }
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }

    let target = stacks
        .iter()
        .filter(|(_, _, t)| **t == Team::Player)
        .min_by_key(|(_, pos, _)| pos.steps_to(*my_pos));
    let Some((_, target_pos, _)) = target else {
        actions.write(CombatAction::EndTurn);
        return;
    };

    let stats = my_stack.stats();
    // Ranged stacks shoot the nearest target they can hit; melee stacks need adjacency.
    let shootable = stacks
        .iter()
        .filter(|(_, p, t)| **t == Team::Player && in_strike_range(stats.range, *my_pos, **p))
        .min_by_key(|(_, p, _)| p.distance_to(*my_pos))
        .map(|(e, _, _)| e);
    if let Some(victim) = shootable {
        actions.write(CombatAction::Strike(victim));
        return;
    }

    let occupied: HashSet<GridPos> = stacks
        .iter()
        .filter(|(e, _, _)| *e != me)
        .map(|(_, p, _)| *p)
        .collect();
    let speed = stats.speed as i32;
    let best = grid
        .all_positions()
        .filter(|p| !occupied.contains(p))
        .filter(|p| (1..=speed).contains(&p.steps_to(*my_pos)))
        .min_by_key(|p| p.steps_to(*target_pos));

    match best {
        Some(p) if p.steps_to(*target_pos) < my_pos.steps_to(*target_pos) => {
            actions.write(CombatAction::Move(p));
        }
        _ => {
            actions.write(CombatAction::EndTurn);
        }
    }
}

/// Read-only view of a non-active stack, taken before any mutable borrow.
#[derive(Clone, Copy)]
struct Other {
    entity: Entity,
    team: Team,
    pos: GridPos,
    shield: u32,
}

/// Set the animation of the unit sprite under a stack entity.
fn play(
    children: &Query<&Children>,
    visuals: &mut Query<&mut UnitVisual>,
    stack: Entity,
    anim: UnitAnim,
    once: bool,
) {
    face_and_play(children, visuals, stack, anim, once, None);
}

/// Like `play`, optionally turning the sprite (`flip_x`) first.
fn face_and_play(
    children: &Query<&Children>,
    visuals: &mut Query<&mut UnitVisual>,
    stack: Entity,
    anim: UnitAnim,
    once: bool,
    flip_x: Option<bool>,
) {
    let Ok(children) = children.get(stack) else { return };
    for child in children.iter() {
        if let Ok(mut visual) = visuals.get_mut(child) {
            if let Some(flip) = flip_x {
                visual.flip_x = flip;
            }
            visual.play(anim, once);
        }
    }
}

/// Applies every queued action to the active stack, then locks for the action delay.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn resolve_actions(
    mut commands: Commands,
    mut art: UnitArt,
    mut actions: MessageReader<CombatAction>,
    grid: Res<BattleGrid>,
    mut lock: ResMut<ActionLock>,
    mut order: ResMut<TurnOrder>,
    mut log: ResMut<CombatLog>,
    children: Query<&Children>,
    mut visuals: Query<&mut UnitVisual>,
    mut active: Query<
        (Entity, &Team, &Faction, &mut Stack, &mut GridPos, &Transform, &mut ActionPoints),
        With<ActiveStack>,
    >,
    mut others: Query<
        (Entity, &Team, &GridPos, &mut Stack, Option<&ShieldUp>),
        (Without<ActiveStack>, Without<Dying>),
    >,
) {
    if lock.is_locked() {
        actions.clear();
        return;
    }
    let Ok((me, my_team, my_faction, mut my_stack, mut my_pos, my_transform, mut ap)) = active.single_mut()
    else {
        actions.clear();
        return;
    };
    let my_team = *my_team;
    let who = |stack: &Stack| format!("{} {}", my_team.label(), stack.short_label());

    let mut killed: HashSet<Entity> = HashSet::new();
    let mut end_turn = false;
    let mut acted = false;
    let mut delay = QUICK_ACTION_SECS;
    let mut rng = rand::rng();

    for action in actions.read().copied() {
        if acted || (ap.0 == 0 && action != CombatAction::EndTurn) {
            continue; // one action per lock
        }
        // Snapshot of everyone else, minus stacks killed earlier in this same batch.
        let snapshot: Vec<Other> = others
            .iter()
            .filter(|(e, ..)| !killed.contains(e))
            .map(|(entity, team, pos, _, shield)| Other {
                entity,
                team: *team,
                pos: *pos,
                shield: shield.map_or(0, |s| s.0),
            })
            .collect();
        let occupied = |p: GridPos| snapshot.iter().any(|o| o.pos == p);

        match action {
            CombatAction::Move(target) => {
                let steps = my_pos.steps_to(target);
                let speed = my_stack.stats().speed as i32;
                if !grid.contains(target) || occupied(target) {
                    log.push(format!("{}: that tile is blocked.", who(&my_stack)));
                } else if steps == 0 || steps > speed {
                    log.push(format!("{}: too far (speed {speed}).", who(&my_stack)));
                } else {
                    *my_pos = target;
                    delay = move_duration(steps);
                    commands.entity(me).insert(MoveTween {
                        from: my_transform.translation.truncate(),
                        to: grid.to_world(target),
                        timer: Timer::from_seconds(delay, TimerMode::Once),
                    });
                    play(&children, &mut visuals, me, UnitAnim::Run, false);
                    ap.0 -= 1;
                    acted = true;
                    log.push(format!("{} moves {steps} step(s).", who(&my_stack)));
                }
            }

            CombatAction::Strike(target) => {
                let Some(other) = snapshot.iter().find(|o| o.entity == target).copied() else {
                    continue;
                };
                let stats = my_stack.stats();
                let (min_range, max_range) = stats.range;
                let distance = my_pos.distance_to(other.pos);
                let reason = if other.team == my_team {
                    Some("that is your own stack (click it to merge)".to_string())
                } else if !my_stack.is_ranged() && distance != 1 {
                    Some("target is not next to you (the 8 surrounding tiles)".to_string())
                } else if my_stack.is_ranged() && distance < min_range as i32 {
                    Some(format!("too close: range is {min_range}-{max_range} tiles"))
                } else if my_stack.is_ranged() && distance > max_range as i32 {
                    Some(format!("out of range: range is {min_range}-{max_range} tiles"))
                } else {
                    None
                };
                if let Some(reason) = reason {
                    log.push(format!("{}: {reason}.", who(&my_stack)));
                    continue;
                }

                // Flanking: one of my stacks on the far side of the target removes 1 defence.
                let far_side = GridPos(other.pos.0 + my_pos.direction_to(other.pos));
                let flanked = snapshot
                    .iter()
                    .any(|o| o.team == my_team && o.pos == far_side);
                let modifier = other.shield as i32 + if flanked { FLANK_PENALTY } else { 0 };

                let Ok((_, _, _, mut target_stack, _)) = others.get_mut(target) else {
                    continue;
                };
                let damage = stack_damage(&my_stack, &mut rng);
                let before = target_stack.short_label();
                let report = apply_damage(&mut target_stack, damage, modifier);
                ap.0 -= 1;
                acted = true;
                delay = strike_duration(my_stack.kind, my_stack.is_ranged().then_some(distance));
                // The attack sheet follows the direction attacker -> target.
                let (dir, flip) = AttackDir::towards(my_transform.translation.truncate(), grid.to_world(other.pos));
                face_and_play(&children, &mut visuals, me, UnitAnim::Attack(dir), true, flip);
                if my_stack.is_ranged() {
                    let from = my_transform.translation.truncate();
                    let to = grid.to_world(other.pos);
                    let angle = (to - from).to_angle();
                    commands.spawn((
                        Sprite::from_image(art.arrow(*my_faction)),
                        Transform::from_translation(from.extend(2.0)).with_rotation(Quat::from_rotation_z(angle)),
                        Projectile { from, to, timer: Timer::from_seconds(delay, TimerMode::Once) },
                        Pickable::IGNORE,
                        DespawnOnExit(GameState::Combat),
                    ));
                }
                let notes = match (flanked, other.shield > 0) {
                    (true, true) => " (flanked, shielded)",
                    (true, false) => " (flanked)",
                    (false, true) => " (shielded)",
                    (false, false) => "",
                };
                log.push(format!(
                    "{} strikes {before} for {damage}{notes}: {} hp lost, {} killed.",
                    who(&my_stack),
                    report.dealt,
                    report.killed
                ));
                if target_stack.is_dead() {
                    killed.insert(target);
                    commands.entity(target).insert(Dying);
                    // The last unit falls when the blow lands: mid-swing for melee, on arrival for arrows.
                    let impact = if my_stack.is_ranged() { delay } else { delay * 0.5 };
                    spawn_effect(&mut commands, Fx::Dust2, grid.to_world(other.pos), 3.0, DEATH_DUST_SCALE, impact, Some(target));
                    log.push(format!("{} {before} is destroyed!", other.team.label()));
                }
            }

            CombatAction::Unstack => {
                let Some((kept, moved)) = split(&my_stack) else {
                    log.push(format!("{}: nothing to split.", who(&my_stack)));
                    continue;
                };
                let free = my_pos
                    .ring_clockwise_from_right()
                    .into_iter()
                    .find(|p| grid.contains(*p) && !occupied(*p));
                let Some(free) = free else {
                    log.push(format!("{}: no free tile around to unstack into.", who(&my_stack)));
                    continue;
                };
                let label = who(&my_stack);
                *my_stack = kept;
                let new_stack = spawn_stack(&mut commands, &mut art, &grid, my_team, *my_faction, moved, free);
                // The new half acts right before this stack from the next round on.
                let slot = order.current;
                order.order.insert(slot, new_stack);
                order.current += 1;
                ap.0 -= 1;
                acted = true;
                log.push(format!(
                    "{label} splits: {} stays (AP left), {} steps aside.",
                    my_stack.short_label(),
                    moved.short_label()
                ));
            }

            CombatAction::Merge(target) => {
                let Some(other) = snapshot.iter().find(|o| o.entity == target).copied() else {
                    continue;
                };
                let Ok((_, _, _, mut target_stack, _)) = others.get_mut(target) else {
                    continue;
                };
                if other.team != my_team {
                    log.push(format!("{}: can only merge with your own stacks.", who(&my_stack)));
                } else if !my_pos.is_adjacent(other.pos) {
                    log.push(format!("{}: merge target is not adjacent.", who(&my_stack)));
                } else if !merge(&mut my_stack, &target_stack) {
                    log.push(format!(
                        "{}: only stacks of the same kind can merge.",
                        who(&my_stack)
                    ));
                } else {
                    let absorbed = target_stack.short_label();
                    target_stack.count = 0;
                    killed.insert(target);
                    commands.entity(target).despawn();
                    let slot = order.order.iter().position(|e| *e == target);
                    if let Some(i) = slot {
                        order.order.remove(i);
                        if i < order.current {
                            order.current -= 1;
                        }
                    }
                    ap.0 -= 1;
                    acted = true;
                    log.push(format!("{} absorbs {absorbed}.", who(&my_stack)));
                }
            }

            CombatAction::ShieldUp => {
                commands.entity(me).insert(ShieldUp(SHIELD_BONUS));
                play(&children, &mut visuals, me, UnitAnim::Guard, false);
                ap.0 -= 1;
                acted = true;
                log.push(format!(
                    "{} raises shields: +{SHIELD_BONUS} defence until its next turn.",
                    who(&my_stack)
                ));
            }

            CombatAction::EndTurn => {
                end_turn = true;
                acted = true;
                delay = END_TURN_SECS;
            }
        }
    }

    if acted {
        lock.lock(delay.min(MAX_ACTION_SECS));
        if end_turn || ap.0 == 0 {
            lock.then_end_turn = Some(me);
        }
    }
}

/// Counts the action delay down; when it ends, passes the turn on if that was requested.
pub fn tick_action_lock(
    time: Res<Time>,
    mut commands: Commands,
    mut lock: ResMut<ActionLock>,
    mut order: ResMut<TurnOrder>,
    alive: Query<Entity, (With<Stack>, Without<Dying>)>,
    dying: Query<Entity, With<Dying>>,
) {
    if !lock.locked || !lock.timer.tick(time.delta()).just_finished() {
        return;
    }
    lock.locked = false;
    for entity in &dying {
        commands.entity(entity).despawn();
    }
    if let Some(me) = lock.then_end_turn.take() {
        order.advance(|e| alive.contains(e));
        turn::activate(&mut commands, &order, Some(me));
    }
}

/// Slide moving stacks across the grid during the action delay.
pub fn tween_stack_moves(
    time: Res<Time>,
    mut commands: Commands,
    children: Query<&Children>,
    mut visuals: Query<&mut UnitVisual>,
    mut moving: Query<(Entity, &mut Transform, &mut MoveTween)>,
) {
    for (entity, mut transform, mut tween) in &mut moving {
        tween.timer.tick(time.delta());
        let t = tween.timer.fraction();
        let pos = tween.from.lerp(tween.to, t);
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
        if tween.timer.is_finished() {
            commands.entity(entity).remove::<MoveTween>();
            play(&children, &mut visuals, entity, UnitAnim::Idle, false);
        }
    }
}

/// Arrows fly in a straight line and vanish when they land.
pub fn fly_projectiles(
    time: Res<Time>,
    mut commands: Commands,
    mut arrows: Query<(Entity, &mut Transform, &mut Projectile)>,
) {
    for (entity, mut transform, mut arrow) in &mut arrows {
        arrow.timer.tick(time.delta());
        let pos = arrow.from.lerp(arrow.to, arrow.timer.fraction());
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
        if arrow.timer.is_finished() {
            commands.entity(entity).despawn();
        }
    }
}

/// When a Shield up wears off (at the stack's next turn), drop the guard pose.
pub fn shield_visuals(
    mut removed: RemovedComponents<ShieldUp>,
    children: Query<&Children>,
    mut visuals: Query<&mut UnitVisual>,
) {
    for entity in removed.read() {
        if children.contains(entity) {
            play(&children, &mut visuals, entity, UnitAnim::Idle, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::UnitKind;

    #[test]
    fn moves_cost_one_second_then_half_per_step_capped_at_two() {
        assert_eq!(move_duration(1), 1.0);
        assert_eq!(move_duration(2), 1.5);
        assert_eq!(move_duration(3), 2.0);
        assert_eq!(move_duration(6), 2.0);
    }

    #[test]
    fn strikes_last_as_long_as_the_animation_or_the_arrow() {
        // Warrior attack: 4 frames at 10 fps = 0.4 s, raised to the 0.5 s minimum.
        assert_eq!(strike_duration(UnitKind::Warrior, None), MIN_STRIKE_SECS);
        // Monk "heal" is 11 frames: 1.1 s.
        assert!((strike_duration(UnitKind::Monk, None) - 1.1).abs() < 1e-5);
        // Archer: 8 frames = 0.8 s, but an arrow over 4 tiles takes 1.0 s.
        assert!((strike_duration(UnitKind::Archer, Some(4)) - 1.0).abs() < 1e-5);
        assert!((strike_duration(UnitKind::Archer, Some(2)) - 0.8).abs() < 1e-5);
        // Nothing beats the cap.
        assert_eq!(strike_duration(UnitKind::Archer, Some(40)), MAX_ACTION_SECS);
    }
}
