//! What happens when the hero touches a building, tower or seller. See agents.md §1.2–1.5.

use bevy::prelude::*;

use super::Hero;
use super::buildings::BuildingKind;
use super::economy::Treasuries;
use super::world::{EncounterSource, PendingEncounter, WorldState};
use crate::faction::Faction;
use crate::states::GameState;
use crate::units::{CASTLE_SLOTS, HERO_SLOTS, PlayerArmy, UnitKind};

const MESSAGE_SECS: f32 = 2.5;

/// What a touchable thing on the map is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Touch {
    Castle(usize),
    Shop { base: usize, kind: BuildingKind },
    Tower(usize),
    Seller(usize),
}

#[derive(Component)]
pub struct Touchable {
    pub touch: Touch,
    pub radius: f32,
}

/// The thing the hero is touching right now, plus a short-lived message for the HUD.
#[derive(Resource, Default)]
pub struct Nearby {
    pub touch: Option<Touch>,
    pub message: Option<String>,
    message_ttl: f32,
}

impl Nearby {
    pub fn say(&mut self, text: impl Into<String>) {
        self.say_for(text, MESSAGE_SECS);
    }

    pub fn say_for(&mut self, text: impl Into<String>, seconds: f32) {
        self.message = Some(text.into());
        self.message_ttl = seconds;
    }
}

/// Shared by every fight trigger. Returns true if combat was started.
pub fn start_fight(
    commands: &mut Commands,
    next_state: &mut NextState<GameState>,
    nearby: &mut Nearby,
    player: &PlayerArmy,
    world: &WorldState,
    source: EncounterSource,
) -> bool {
    if player.0.is_empty() {
        nearby.say("You need an army! Take units from your castle (number keys) first.");
        return false;
    }
    let enemy = world.enemy_army_for(source);
    if enemy.is_empty() {
        return false;
    }
    commands.insert_resource(PendingEncounter { enemy, source });
    next_state.set(GameState::Combat);
    true
}

pub fn find_touch(
    hero: Single<&Transform, With<Hero>>,
    touchables: Query<(&Transform, &Touchable)>,
    mut nearby: ResMut<Nearby>,
) {
    let hero_pos = hero.translation.truncate();
    let touch = touchables
        .iter()
        .map(|(t, touchable)| (t.translation.truncate().distance(hero_pos), touchable))
        .filter(|(dist, touchable)| *dist <= touchable.radius)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, touchable)| touchable.touch);
    if nearby.touch != touch {
        nearby.touch = touch;
    }
}

pub fn tick_messages(time: Res<Time>, mut nearby: ResMut<Nearby>) {
    if nearby.message.is_some() {
        nearby.message_ttl -= time.delta_secs();
        if nearby.message_ttl <= 0.0 {
            nearby.message = None;
        }
    }
}

const TRANSFER_KEYS: [KeyCode; 5] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5];

/// At your own castle: number key takes one unit of that kind, Shift stores one, Ctrl moves all.
pub fn castle_transfer(
    keys: Res<ButtonInput<KeyCode>>,
    mut nearby: ResMut<Nearby>,
    mut world: ResMut<WorldState>,
    mut player_army: ResMut<PlayerArmy>,
) {
    let Some(Touch::Castle(base)) = nearby.touch else { return };
    if !world.bases[base].owner.is_player() {
        return;
    }
    let Some(kind) = TRANSFER_KEYS
        .iter()
        .zip(UnitKind::ALL)
        .find(|(key, _)| keys.just_pressed(**key))
        .map(|(_, kind)| kind)
    else {
        return;
    };
    let store = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let all = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    transfer(&mut world, &mut player_army, &mut nearby, base, kind, store, all);
}

/// Move units of `kind` between the hero and the garrison of `base`: one, or all with `all`.
/// `store` moves hero -> castle, otherwise castle -> hero. Used by the number keys and the
/// castle panel's buttons.
pub fn transfer(
    world: &mut WorldState,
    player_army: &mut PlayerArmy,
    nearby: &mut Nearby,
    base: usize,
    kind: UnitKind,
    store: bool,
    all: bool,
) {
    let garrison = &mut world.bases[base].garrison;
    let hero = &mut player_army.0;

    let (from, to, cap, verb) = if store {
        (hero, garrison, CASTLE_SLOTS, "stored")
    } else {
        (garrison, hero, HERO_SLOTS, "took")
    };
    let wanted = if all { from.count_of(kind) } else { 1 };
    let room = cap.saturating_sub(to.slots());
    let n = wanted.min(room);
    if from.count_of(kind) == 0 {
        nearby.say(format!("No {} there.", kind.name()));
        return;
    }
    if n == 0 {
        nearby.say(format!("No room: limit is {cap} units."));
        return;
    }
    let moved = from.remove(kind, n);
    to.add(kind, moved);
    nearby.say(format!("{verb} {moved} {}.", kind.name()));
}

/// `R` at an own shop building or an idle seller buys one unit (quick buy).
pub fn buy_units(
    keys: Res<ButtonInput<KeyCode>>,
    mut nearby: ResMut<Nearby>,
    world: Res<WorldState>,
    mut treasuries: ResMut<Treasuries>,
    mut player_army: ResMut<PlayerArmy>,
) {
    if !keys.just_pressed(KeyCode::KeyR) {
        return;
    }
    let unit = match nearby.touch {
        Some(Touch::Shop { base, kind }) if world.bases[base].owner.is_player() => kind.sells(),
        Some(Touch::Shop { .. }) => {
            nearby.say("This building belongs to another kingdom. Take their castle first.");
            None
        }
        Some(Touch::Seller(i)) => Some(world.sellers[i].unit),
        _ => None,
    };
    let Some(unit) = unit else { return };
    try_buy(unit, &mut nearby, &mut treasuries, &mut player_army);
}

/// Hire one unit for the hero if there is room and gold. Used by `R` and the shop panel's button.
pub fn try_buy(unit: UnitKind, nearby: &mut Nearby, treasuries: &mut Treasuries, player_army: &mut PlayerArmy) -> bool {
    if player_army.0.slots() >= HERO_SLOTS {
        nearby.say(format!("Your army is full ({HERO_SLOTS} units)."));
        return false;
    }
    if !treasuries.spend(Faction::Reclaimers, unit.price()) {
        nearby.say(format!("Not enough gold: {} costs {}g.", unit.name(), unit.price()));
        return false;
    }
    player_army.0.add(unit, 1);
    nearby.say(format!("Hired a {} for {}g.", unit.name(), unit.price()));
    true
}

/// Touching an enemy castle or a tower you do not own attacks it. An undefended one is simply taken.
pub fn trigger_fights(
    mut commands: Commands,
    mut nearby: ResMut<Nearby>,
    mut world: ResMut<WorldState>,
    mut treasuries: ResMut<Treasuries>,
    player_army: Res<PlayerArmy>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let source = match nearby.touch {
        Some(Touch::Castle(i)) if !world.bases[i].owner.is_player() => EncounterSource::Base(i),
        Some(Touch::Tower(i)) if !world.towers[i].owner.is_player() => {
            // An army in the middle of taking it blocks the door.
            if world.tower_being_taken_by(i).is_some() {
                return;
            }
            EncounterSource::Tower(i)
        }
        _ => return,
    };
    if world.enemy_army_for(source).is_empty() {
        let (loser, loot) = world.capture(source, &mut treasuries);
        treasuries.add(Faction::Reclaimers, loot);
        nearby.say(format!("Undefended! Taken from the {} (+{loot}g).", loser.name()));
        return;
    }
    start_fight(&mut commands, &mut next_state, &mut nearby, &player_army, &world, source);
}
