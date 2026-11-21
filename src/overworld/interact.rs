//! What happens when the hero touches a building, tower or seller. See agents.md §1.2–1.5.

use bevy::prelude::*;

use super::Hero;
use super::buildings::BuildingKind;
use super::economy::Treasuries;
use super::world::{Base, EncounterSource, PendingEncounter, Tower, WorldState};
use crate::faction::Faction;
use crate::states::GameState;
use crate::units::{Army, CASTLE_SLOTS, PlayerArmy, TOWER_SLOTS, UnitKind};

const MESSAGE_SECS: f32 = 2.5;

/// What a touchable thing on the map is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Touch {
    Castle(usize),
    Shop { base: usize, kind: BuildingKind },
    Tower(usize),
    Seller(usize),
    /// A gold mine, forest or sheep pasture (index into `WorldState::sites`).
    Site(usize),
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
    commands.insert_resource(PendingEncounter { enemy, source, defending: None, helpers: Vec::new() });
    next_state.set(GameState::Combat);
    true
}

/// The AI armies of `owner` standing within `HELP_TILES` of `place` (not busy besieging something
/// else): they join the fight when you attack it.
pub fn helpers_of(world: &WorldState, place: Garrison) -> Vec<usize> {
    use super::armies::{AiArmyState, HELP_TILES};
    let owner = place.owner(world);
    let at = world.terrain.world_pos(place.tile(world));
    world
        .armies
        .iter()
        .enumerate()
        .filter(|(_, a)| a.alive && a.faction == owner && !a.army.is_empty())
        .filter(|(_, a)| !matches!(a.state, AiArmyState::Capturing(_) | AiArmyState::Leaving(_)))
        .filter(|(_, a)| a.pos.distance(at) <= HELP_TILES * super::terrain::TILE)
        .map(|(i, _)| i)
        .collect()
}

/// JOIN at your besieged castle or tower: you and its garrison fight the besieging AI army
/// together, and you command every stack. Works without an army of your own. Returns true if
/// combat was started. See agents.md §1.5.
pub fn join_siege(
    commands: &mut Commands,
    next_state: &mut NextState<GameState>,
    nearby: &mut Nearby,
    player: &PlayerArmy,
    world: &WorldState,
    place: Garrison,
) -> bool {
    let besiegers = world.besiegers(place);
    let Some(&i) = besiegers.first() else {
        nearby.say("Nobody is attacking it right now.");
        return false;
    };
    if player.0.is_empty() && place.army(world).is_empty() {
        nearby.say("Nobody to fight with: you have no army and the garrison is empty.");
        return false;
    }
    // Every army besieging it fights you together; the first is the "source", the rest helpers.
    let mut enemy = world.armies[i].army.clone();
    for &j in &besiegers[1..] {
        enemy.stacks.extend(world.armies[j].army.stacks.iter().copied());
    }
    commands.insert_resource(PendingEncounter {
        enemy,
        source: EncounterSource::AiArmy(i),
        defending: Some(place),
        helpers: besiegers[1..].to_vec(),
    });
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

/// A castle or a tower, with its garrison. The hero moves units in and out of their own ones
/// (`touched`), and AI armies besiege them (`AiArmy::target`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Garrison {
    Castle(usize),
    Tower(usize),
}

impl Garrison {
    /// The garrison of yours the hero is touching, if any.
    pub fn touched(touch: Option<Touch>, world: &WorldState) -> Option<Garrison> {
        match touch? {
            Touch::Castle(i) if world.bases[i].owner.is_player() => Some(Garrison::Castle(i)),
            Touch::Tower(i) if world.towers[i].owner.is_player() => Some(Garrison::Tower(i)),
            _ => None,
        }
    }

    /// The tile an army walks to to besiege it: the tower's tile or the castle's gate (where its
    /// armies come out).
    pub fn tile_in(self, towers: &[Tower], bases: &[Base]) -> IVec2 {
        match self {
            Garrison::Castle(i) => bases[i].spawn_tile(),
            Garrison::Tower(i) => towers[i].tile,
        }
    }

    pub fn owner_in(self, towers: &[Tower], bases: &[Base]) -> Faction {
        match self {
            Garrison::Castle(i) => bases[i].owner,
            Garrison::Tower(i) => towers[i].owner,
        }
    }

    pub fn tile(self, world: &WorldState) -> IVec2 {
        self.tile_in(&world.towers, &world.bases)
    }

    pub fn owner(self, world: &WorldState) -> Faction {
        self.owner_in(&world.towers, &world.bases)
    }

    /// "castle" or "tower", for messages.
    pub fn noun(self) -> &'static str {
        match self {
            Garrison::Castle(_) => "castle",
            Garrison::Tower(_) => "tower",
        }
    }

    pub fn touch(self) -> Touch {
        match self {
            Garrison::Castle(i) => Touch::Castle(i),
            Garrison::Tower(i) => Touch::Tower(i),
        }
    }

    /// How many units it holds at most.
    /// Your castles hold more with the crafted "Units in castle" upgrades.
    pub fn cap(self, world: &WorldState) -> u32 {
        match self {
            Garrison::Castle(i) if world.bases[i].owner.is_player() => world.upgrades.castle_slots(),
            Garrison::Castle(_) => CASTLE_SLOTS,
            Garrison::Tower(_) => TOWER_SLOTS,
        }
    }

    pub fn army(self, world: &WorldState) -> &Army {
        match self {
            Garrison::Castle(i) => &world.bases[i].garrison,
            Garrison::Tower(i) => &world.towers[i].garrison,
        }
    }

    pub fn army_mut(self, world: &mut WorldState) -> &mut Army {
        match self {
            Garrison::Castle(i) => &mut world.bases[i].garrison,
            Garrison::Tower(i) => &mut world.towers[i].garrison,
        }
    }
}

/// At your own castle or tower: number key takes one unit of that kind, Shift stores one, Ctrl
/// moves all.
pub fn castle_transfer(
    keys: Res<ButtonInput<KeyCode>>,
    mut nearby: ResMut<Nearby>,
    mut world: ResMut<WorldState>,
    mut player_army: ResMut<PlayerArmy>,
) {
    let Some(place) = Garrison::touched(nearby.touch, &world) else { return };
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
    transfer(&mut world, &mut player_army, &mut nearby, place, kind, store, all);
}

/// Move units of `kind` between the hero and the garrison of `place`: one, or all with `all`.
/// `store` moves hero -> garrison, otherwise garrison -> hero. Used by the number keys and the
/// garrison panel's buttons.
pub fn transfer(
    world: &mut WorldState,
    player_army: &mut PlayerArmy,
    nearby: &mut Nearby,
    place: Garrison,
    kind: UnitKind,
    store: bool,
    all: bool,
) {
    // Your Pawns out working still hold their castle slots.
    let reserved = match place {
        Garrison::Castle(i) => world.workers_out(i),
        Garrison::Tower(_) => 0,
    };
    let cap = place.cap(world).saturating_sub(reserved);
    let hero_cap = world.upgrades.hero_slots();
    let garrison = place.army_mut(world);
    let hero = &mut player_army.0;

    let (from, to, cap, verb) = if store {
        (hero, garrison, cap, "stored")
    } else {
        (garrison, hero, hero_cap, "took")
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
    try_buy(unit, &world, &mut nearby, &mut treasuries, &mut player_army);
}

/// Hire one unit for the hero if there is room and gold. Used by `R` and the shop panel's button.
pub fn try_buy(
    unit: UnitKind,
    world: &WorldState,
    nearby: &mut Nearby,
    treasuries: &mut Treasuries,
    player_army: &mut PlayerArmy,
) -> bool {
    let (slots, price) = (world.upgrades.hero_slots(), world.upgrades.unit_price(unit));
    if player_army.0.slots() >= slots {
        nearby.say(format!("Your army is full ({slots} units)."));
        return false;
    }
    if !treasuries.spend(Faction::Reclaimers, price) {
        nearby.say(format!("Not enough gold: {} costs {price}g.", unit.name()));
        return false;
    }
    player_army.0.add(unit, 1);
    nearby.say(format!("Hired a {} for {price}g.", unit.name()));
    true
}

/// The end of your siege (`attack_panel::player_siege_tick`): fight the place's garrison together
/// with any of its kingdom's armies within reach (`helpers_of`), or simply take it if nobody
/// defends it.
pub fn start_fight_at(
    commands: &mut Commands,
    next_state: &mut NextState<GameState>,
    nearby: &mut Nearby,
    world: &mut WorldState,
    treasuries: &mut Treasuries,
    player_army: &PlayerArmy,
    place: Garrison,
) {
    let source = match place {
        Garrison::Castle(i) => EncounterSource::Base(i),
        Garrison::Tower(i) => EncounterSource::Tower(i),
    };
    // Its kingdom's armies close by come to its defence (agents.md §1.5).
    let helpers = helpers_of(world, place);
    let mut enemy = world.enemy_army_for(source);
    for &i in &helpers {
        enemy.stacks.extend(world.armies[i].army.stacks.iter().copied());
    }
    if enemy.is_empty() {
        let (loser, loot) = world.capture(source, treasuries);
        treasuries.add(Faction::Reclaimers, loot);
        nearby.say(format!("Undefended! Taken from the {} (+{loot}g).", loser.name()));
        return;
    }
    if player_army.0.is_empty() {
        nearby.say("You need an army! Take units from your castle (number keys) first.");
        return;
    }
    if !helpers.is_empty() {
        nearby.say_for(format!("The {} army defends its {} with the garrison!", place.owner(world).name(), place.noun()), 6.0);
    }
    commands.insert_resource(PendingEncounter { enemy, source, defending: None, helpers });
    next_state.set(GameState::Combat);
}
