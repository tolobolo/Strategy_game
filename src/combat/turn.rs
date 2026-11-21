//! Turn order, action points and the end of a battle.

use bevy::prelude::*;

use super::grid::BattleGrid;
use super::hud::CombatLog;
use super::spawn::FromGarrison;
use crate::faction::Faction;
use crate::overworld::world::EncounterSource;
use crate::overworld::{Nearby, PendingEncounter, Treasuries, WorldState};
use crate::states::{CombatPhase, GameState};
use crate::units::{PlayerArmy, ShieldUp, Stack, Team};

pub const AP_PER_TURN: u8 = 3;

/// Shuffled once per battle; cycles through every living stack.
#[derive(Resource, Default, Debug)]
pub struct TurnOrder {
    pub order: Vec<Entity>,
    pub current: usize,
}

impl TurnOrder {
    pub fn current(&self) -> Option<Entity> {
        self.order.get(self.current).copied()
    }

    /// Step to the next stack for which `is_alive` holds.
    pub fn advance(&mut self, is_alive: impl Fn(Entity) -> bool) {
        if self.order.is_empty() {
            return;
        }
        for _ in 0..self.order.len() {
            self.current = (self.current + 1) % self.order.len();
            if is_alive(self.order[self.current]) {
                return;
            }
        }
    }
}

/// The stack whose turn it is.
#[derive(Component)]
pub struct ActiveStack;

/// Action points left this turn.
#[derive(Component)]
pub struct ActionPoints(pub u8);

#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CombatResult {
    /// Gold looted from the loser.
    Victory { loot: u32 },
    /// Gold handed to the winner.
    Defeat { gold_lost: u32 },
}

/// Give the turn to `order.current()`, taking it from `previous` if given.
pub fn activate(commands: &mut Commands, order: &TurnOrder, previous: Option<Entity>) {
    if let Some(prev) = previous
        && let Ok(mut prev) = commands.get_entity(prev)
    {
        prev.remove::<(ActiveStack, ActionPoints)>();
    }
    if let Some(current) = order.current() {
        commands
            .entity(current)
            .remove::<ShieldUp>() // Shield up lasts until the stack's next turn.
            .insert((ActiveStack, ActionPoints(AP_PER_TURN)));
    }
}

/// Runs once when the Turn phase starts: hand the first turn out.
pub fn activate_current_stack(
    mut commands: Commands,
    order: Res<TurnOrder>,
    active: Query<Entity, With<ActiveStack>>,
) {
    for entity in &active {
        commands
            .entity(entity)
            .remove::<(ActiveStack, ActionPoints)>();
    }
    activate(&mut commands, &order, None);
}

/// Ends the battle when one side has no stacks left and writes the outcome back to the world.
/// Units lost in battle are gone for good; survivors go back to the hero, and after a siege you
/// joined the GUARD survivors go back into the garrison (anything over the hero's 20 too). Losing
/// a siege also loses the castle or tower to the attacker.
#[allow(clippy::too_many_arguments)]
pub fn check_victory(
    mut commands: Commands,
    stacks: Query<(&Team, &Stack, Has<FromGarrison>)>,
    encounter: Res<PendingEncounter>,
    mut player_army: ResMut<PlayerArmy>,
    mut world: ResMut<WorldState>,
    mut treasuries: ResMut<Treasuries>,
    mut log: ResMut<CombatLog>,
    mut nearby: ResMut<Nearby>,
    mut next_phase: ResMut<NextState<CombatPhase>>,
) {
    let players_alive = stacks.iter().any(|(t, s, _)| *t == Team::Player && !s.is_dead());
    let enemies_alive = stacks.iter().any(|(t, s, _)| *t == Team::Enemy && !s.is_dead());
    if players_alive && enemies_alive {
        return;
    }
    // The crafted "Regain health": your survivors walk away with full health.
    let heal = world.upgrades.heal_after_fight();
    let survivors = |team: Team, guard: bool| -> Vec<Stack> {
        stacks
            .iter()
            .filter(|(t, s, g)| **t == team && !s.is_dead() && (team == Team::Enemy || *g == guard))
            .map(|(_, s, _)| {
                let mut s = *s;
                if heal && team == Team::Player {
                    s.lead_hp = s.kind.stats().health;
                }
                s
            })
            .collect()
    };

    let result = if players_alive {
        player_army.0.stacks = survivors(Team::Player, false);
        if let Some(place) = encounter.defending {
            let hero_cap = world.upgrades.hero_slots();
            let garrison = place.army_mut(&mut world);
            garrison.stacks = survivors(Team::Player, true);
            // Guards merged into your stacks: what does not fit with the hero stays at home.
            let mut over = player_army.0.slots().saturating_sub(hero_cap);
            for stack in player_army.0.stacks.clone().into_iter().rev() {
                let n = over.min(stack.count);
                if n > 0 {
                    player_army.0.remove(stack.kind, n);
                    garrison.add(stack.kind, n);
                    over -= n;
                }
            }
        }
        let (loser, loot) = world.capture(encounter.source, &mut treasuries);
        // The armies that came to help the place are beaten with it.
        for &i in &encounter.helpers {
            world.armies[i].die();
        }
        treasuries.add(Faction::Reclaimers, loot);
        let won = match encounter.defending {
            Some(place) => format!("The siege of your {} is broken! The {} are beaten", place.noun(), loser.name()),
            None => format!("Victory over the {}", loser.name()),
        };
        log.push(format!("{won}! +{loot} gold."));
        nearby.say_for(format!("{won}: +{loot} gold. Survivors: {}", player_army.0.summary()), 8.0);
        CombatResult::Victory { loot }
    } else {
        let winner = world.faction_of(encounter.source);
        let gold_lost = treasuries.take_pct(Faction::Reclaimers, world.upgrades.loss_pct());
        treasuries.add(winner, gold_lost);
        *player_army = PlayerArmy::default();
        if let (Some(place), EncounterSource::AiArmy(i)) = (encounter.defending, encounter.source) {
            world.siege_lost(i, place, survivors(Team::Enemy, false));
            log.push(format!("Your {} falls to the {}.", place.noun(), winner.name()));
        }
        world.hero_pos = world.hero_home_pos();
        log.push(format!("Defeat. The {} take {gold_lost} gold.", winner.name()));
        nearby.say_for(
            format!(
                "DEFEAT: your army is gone and the {} took {gold_lost} gold. You woke up at your castle. Take new units with 1-5.",
                winner.name()
            ),
            10.0,
        );
        CombatResult::Defeat { gold_lost }
    };
    commands.insert_resource(result);
    next_phase.set(CombatPhase::Finished);
}

pub fn wait_for_continue(
    keys: Res<ButtonInput<KeyCode>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if keys.any_just_pressed([KeyCode::Space, KeyCode::Enter, KeyCode::Escape]) {
        next_state.set(GameState::Overworld);
    }
}

pub fn cleanup(mut commands: Commands) {
    commands.remove_resource::<PendingEncounter>();
    commands.remove_resource::<CombatResult>();
    commands.remove_resource::<TurnOrder>();
    commands.remove_resource::<BattleGrid>();
    commands.remove_resource::<CombatLog>();
}
