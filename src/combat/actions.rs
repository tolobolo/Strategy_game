//! Everything a stack can do. Player input and the enemy AI both write `CombatAction`
//! messages; `resolve_actions` applies the rules once, in one place.

use std::collections::HashSet;

use bevy::prelude::*;

use super::grid::{BattleGrid, GridPos};
use super::hud::CombatLog;
use super::spawn::spawn_stack;
use super::turn::{self, ActionPoints, ActiveStack, TurnOrder};
use crate::faction::Faction;
use crate::units::rules::{apply_damage, merge, split, stack_damage};
use crate::units::{ShieldUp, Stack, Team};

/// Defence lost by a target that has an enemy on its far side (agents.md §3.2).
const FLANK_PENALTY: i32 = -1;
/// Defence gained from Shield up until the stack's next turn.
const SHIELD_BONUS: u32 = 3;

#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatAction {
    /// Move the active stack to a tile (1 AP, within `speed` steps).
    Move(GridPos),
    /// Attack an enemy stack (1 AP). Melee needs orthogonal adjacency; ranged needs
    /// distance <= range and no enemy touching the shooter.
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

/// Keyboard part of the player's input. Clicks are handled by observers in `spawn.rs`.
pub fn player_input(
    keys: Res<ButtonInput<KeyCode>>,
    active: Query<&Team, With<ActiveStack>>,
    mut actions: MessageWriter<CombatAction>,
) {
    if !matches!(active.single(), Ok(Team::Player)) {
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

/// Pause between enemy actions so they can be followed on screen.
#[derive(Resource)]
pub struct EnemyThinkTimer(pub Timer);

impl Default for EnemyThinkTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(0.6, TimerMode::Repeating))
    }
}

/// Very small AI: strike the nearest player stack if it can, otherwise walk towards it.
pub fn enemy_ai(
    time: Res<Time>,
    mut timer: ResMut<EnemyThinkTimer>,
    grid: Res<BattleGrid>,
    active: Query<(Entity, &Team, &GridPos, &Stack), With<ActiveStack>>,
    stacks: Query<(Entity, &GridPos, &Team), With<Stack>>,
    mut actions: MessageWriter<CombatAction>,
) {
    let Ok((me, team, my_pos, my_stack)) = active.single() else {
        return;
    };
    if *team != Team::Enemy {
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
    let Some((target, target_pos, _)) = target else {
        actions.write(CombatAction::EndTurn);
        return;
    };

    let stats = my_stack.stats();
    let engaged = stacks
        .iter()
        .any(|(_, p, t)| *t == Team::Player && p.is_adjacent(*my_pos));
    let can_strike = if my_stack.is_ranged() {
        !engaged && my_pos.distance_to(*target_pos) <= stats.range as i32
    } else {
        my_pos.is_adjacent(*target_pos)
    };
    if can_strike {
        actions.write(CombatAction::Strike(target));
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

/// Applies every queued action to the active stack, then passes the turn on when AP runs out.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn resolve_actions(
    mut commands: Commands,
    mut actions: MessageReader<CombatAction>,
    grid: Res<BattleGrid>,
    mut order: ResMut<TurnOrder>,
    mut log: ResMut<CombatLog>,
    mut active: Query<
        (
            Entity,
            &Team,
            &Faction,
            &mut Stack,
            &mut GridPos,
            &mut Transform,
            &mut ActionPoints,
        ),
        With<ActiveStack>,
    >,
    mut others: Query<(Entity, &Team, &GridPos, &mut Stack, Option<&ShieldUp>), Without<ActiveStack>>,
) {
    let Ok((me, my_team, my_faction, mut my_stack, mut my_pos, mut my_transform, mut ap)) =
        active.single_mut()
    else {
        actions.clear();
        return;
    };
    let my_team = *my_team;
    let who = |stack: &Stack| format!("{} {}", my_team.label(), stack.short_label());

    let mut killed: HashSet<Entity> = HashSet::new();
    let mut end_turn = false;
    let mut rng = rand::rng();

    for action in actions.read().copied() {
        if ap.0 == 0 && action != CombatAction::EndTurn {
            continue;
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
                    my_transform.translation = grid.to_world(target).extend(1.0);
                    ap.0 -= 1;
                    log.push(format!("{} moves {steps} step(s).", who(&my_stack)));
                }
            }

            CombatAction::Strike(target) => {
                let Some(other) = snapshot.iter().find(|o| o.entity == target).copied() else {
                    continue;
                };
                let stats = my_stack.stats();
                let engaged = snapshot
                    .iter()
                    .any(|o| o.team != my_team && o.pos.is_adjacent(*my_pos));
                let reason = if other.team == my_team {
                    Some("that is your own stack (click it to merge)".to_string())
                } else if !my_stack.is_ranged() && !my_pos.is_adjacent(other.pos) {
                    Some("target is not adjacent (diagonals do not count)".to_string())
                } else if my_stack.is_ranged() && engaged {
                    Some("cannot shoot while an enemy is adjacent".to_string())
                } else if my_stack.is_ranged() && my_pos.distance_to(other.pos) > stats.range as i32 {
                    Some(format!("out of range (range {})", stats.range))
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
                    commands.entity(target).despawn();
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
                let new_stack = spawn_stack(&mut commands, &grid, my_team, *my_faction, moved, free);
                // The new half acts right before this stack from the next round on.
                let slot = order.current;
                order.order.insert(slot, new_stack);
                order.current += 1;
                ap.0 -= 1;
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
                    log.push(format!("{} absorbs {absorbed}.", who(&my_stack)));
                }
            }

            CombatAction::ShieldUp => {
                commands.entity(me).insert(ShieldUp(SHIELD_BONUS));
                ap.0 -= 1;
                log.push(format!(
                    "{} raises shields: +{SHIELD_BONUS} defence until its next turn.",
                    who(&my_stack)
                ));
            }

            CombatAction::EndTurn => end_turn = true,
        }
    }

    if end_turn || ap.0 == 0 {
        order.advance(|e| e == me || (others.contains(e) && !killed.contains(&e)));
        turn::activate(&mut commands, &order, Some(me));
    }
}
