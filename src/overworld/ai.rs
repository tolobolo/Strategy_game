//! The AI kingdoms' economy: buying units for their castles, crafting (visible) upgrades, and
//! drawing armies from the castles. Where the armies walk and what they attack is in `armies.rs`.
//! See agents.md §1.5.

use bevy::prelude::*;
use rand::Rng;

use super::armies::{AiArmy, AiArmyState};
use super::economy::Treasuries;
use super::interact::Nearby;
use super::world::WorldState;
use crate::faction::Faction;
use crate::units::{Army, CASTLE_SLOTS, HERO_SLOTS, UnitKind};

/// How often every AI kingdom goes shopping.
pub const AI_THINK_SECS: f32 = 5.0;
/// At most this many units bought per kingdom per think.
pub const AI_BUYS_PER_THINK: u32 = 3;
/// What an AI kingdom buys, with weights.
pub const AI_BUY_MIX: [(UnitKind, u32); 4] =
    [(UnitKind::Warrior, 4), (UnitKind::Archer, 3), (UnitKind::Lancer, 2), (UnitKind::Monk, 1)];
/// Units an AI castle always keeps at home.
pub const AI_MIN_GARRISON: u32 = 5;
/// An army only sets out once at least this many units are available above the reserve.
pub const AI_MIN_ARMY: u32 = 5;

/// One step of the AI kingdoms' crafting. Everything they craft shows on the map: bigger armies,
/// one more army, a fuller castle. No hidden percentages (agents.md §1.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiStep {
    /// Armies drawn from the castle may be this big.
    ArmySize(u32),
    /// One more army roams for the kingdom.
    ExtraArmy,
    /// Its castles hold this many units.
    CastleSlots(u32),
}

/// The ladder every AI kingdom climbs, one step at a time, with gold.
pub const AI_LADDER: [(AiStep, u32); 5] = [
    (AiStep::ArmySize(25), 60),
    (AiStep::ExtraArmy, 100),
    (AiStep::CastleSlots(40), 80),
    (AiStep::ArmySize(30), 120),
    (AiStep::ExtraArmy, 180),
];
/// A castle with at least this many units stops buying while a ladder step is left: the kingdom
/// saves its gold to craft it.
pub const SAVE_ABOVE: u32 = 12;

/// What an AI kingdom has crafted so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AiCraft {
    pub army_size: u32,
    pub castle_slots: u32,
    /// Steps of `AI_LADDER` done.
    pub level: usize,
}

impl Default for AiCraft {
    fn default() -> Self {
        Self { army_size: HERO_SLOTS, castle_slots: CASTLE_SLOTS, level: 0 }
    }
}

/// A random unit kind from `AI_BUY_MIX`.
fn pick_kind(rng: &mut impl Rng) -> UnitKind {
    let total: u32 = AI_BUY_MIX.iter().map(|(_, w)| w).sum();
    let mut roll = rng.random_range(0..total);
    for (kind, weight) in AI_BUY_MIX {
        if roll < weight {
            return kind;
        }
        roll -= weight;
    }
    AI_BUY_MIX[0].0
}

/// Buy up to `AI_BUYS_PER_THINK` units into `garrison` with `faction`'s gold while there is room.
/// A kind it cannot afford ends the shopping trip. Returns how many were bought.
pub fn buy_units(garrison: &mut Army, faction: Faction, treasuries: &mut Treasuries, rng: &mut impl Rng, cap: u32) -> u32 {
    let mut bought = 0;
    while bought < AI_BUYS_PER_THINK && garrison.slots() < cap {
        let kind = pick_kind(rng);
        if !treasuries.spend(faction, kind.price()) {
            break;
        }
        garrison.add(kind, 1);
        bought += 1;
    }
    bought
}

/// Take a new army out of `garrison`: everything above `AI_MIN_GARRISON`, at most `max` (the
/// kingdom's crafted army size), or nothing if fewer than `AI_MIN_ARMY` are available.
pub fn draw_army(garrison: &mut Army, max: u32) -> Option<Army> {
    let available = garrison.slots().saturating_sub(AI_MIN_GARRISON).min(max);
    if available < AI_MIN_ARMY {
        return None;
    }
    let army = garrison.split_off_fraction(available as f32 / garrison.slots() as f32);
    (!army.is_empty()).then_some(army)
}

/// Every `AI_THINK_SECS`, each AI kingdom crafts its next `AI_LADDER` step when it can spare the
/// gold, then buys units for every castle it holds.
pub fn ai_kingdoms_buy(
    time: Res<Time>,
    mut timer: Local<f32>,
    mut world: ResMut<WorldState>,
    mut treasuries: ResMut<Treasuries>,
    mut nearby: ResMut<Nearby>,
) {
    *timer += time.delta_secs();
    if *timer < AI_THINK_SECS {
        return;
    }
    *timer = 0.0;
    let mut rng = rand::rng();
    for faction in Faction::KINGDOMS.into_iter().filter(|f| !f.is_player()) {
        if !world.bases.iter().any(|b| b.owner == faction) {
            continue;
        }
        if let Some(text) = craft(&mut world, &mut treasuries, faction) {
            nearby.say_for(text, 5.0);
        }
        let done = world.ai_craft(faction);
        let saving = done.level < AI_LADDER.len();
        for base in world.bases.iter_mut().filter(|b| b.owner == faction) {
            // Enough at home: save for the next crafting step instead.
            let cap = if saving { done.castle_slots.min(SAVE_ABOVE) } else { done.castle_slots };
            buy_units(&mut base.garrison, faction, &mut treasuries, &mut rng, cap);
        }
    }
}

/// Craft `faction`'s next ladder step if it can spare the gold. Returns a message for the HUD.
pub fn craft(world: &mut WorldState, treasuries: &mut Treasuries, faction: Faction) -> Option<String> {
    let mut done = world.ai_craft(faction);
    let &(step, price) = AI_LADDER.get(done.level)?;
    if !treasuries.spend(faction, price) {
        return None;
    }
    done.level += 1;
    let text = match step {
        AiStep::ArmySize(n) => {
            done.army_size = n;
            format!("The {} kingdom now raises armies of up to {n}.", faction.name())
        }
        AiStep::CastleSlots(n) => {
            done.castle_slots = n;
            format!("The {} kingdom's castles now hold {n} units.", faction.name())
        }
        AiStep::ExtraArmy => {
            // A new army slot: it is raised from the castle like any army coming back.
            let home = world.bases.iter().find(|b| b.owner == faction).map(|b| world.terrain.world_pos(b.spawn_tile()));
            world.armies.push(AiArmy {
                faction,
                pos: home.unwrap_or_default(),
                army: Army::empty(faction),
                target: None,
                path: Vec::new(),
                state: AiArmyState::Walking,
                alive: false,
                respawn_in: 0.0,
                think: 0.0,
                hunted: 0.0,
                ghost: false,
            });
            format!("The {} kingdom raises another army!", faction.name())
        }
    };
    world.ai_craft.insert(faction, done);
    Some(text)
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::*;

    #[test]
    fn buys_only_what_it_can_afford_and_has_room_for() {
        let mut rng = StdRng::seed_from_u64(3);
        let mut treasuries = Treasuries::default();
        let mut garrison = Army::empty(Faction::Red);
        let before = treasuries.gold(Faction::Red);
        let bought = buy_units(&mut garrison, Faction::Red, &mut treasuries, &mut rng, CASTLE_SLOTS);
        assert!(bought >= 1 && bought <= AI_BUYS_PER_THINK);
        let spent: u32 = UnitKind::ALL.iter().map(|k| garrison.count_of(*k) * k.price()).sum();
        assert_eq!(treasuries.gold(Faction::Red), before - spent);
        assert_eq!(garrison.count_of(UnitKind::Pawn), 0, "no pawns");

        let mut full = Army::new(Faction::Red, &[(UnitKind::Warrior, CASTLE_SLOTS)]);
        assert_eq!(buy_units(&mut full, Faction::Red, &mut treasuries, &mut rng, CASTLE_SLOTS), 0, "no room");
        let mut broke = Treasuries::default();
        broke.spend(Faction::Red, broke.gold(Faction::Red));
        assert_eq!(buy_units(&mut garrison, Faction::Red, &mut broke, &mut rng, CASTLE_SLOTS), 0, "no gold");
    }

    #[test]
    fn kingdoms_climb_the_crafting_ladder_with_gold() {
        let mut world = WorldState::generate(3);
        let mut treasuries = Treasuries::default();
        let red = Faction::Red;
        assert!(craft(&mut world, &mut treasuries, red).is_none(), "50 gold is not enough for the first step");
        treasuries.add(red, 10_000);
        let armies = world.armies.len();
        for _ in 0..AI_LADDER.len() + 1 {
            craft(&mut world, &mut treasuries, red);
        }
        let done = world.ai_craft(red);
        assert_eq!(done.level, AI_LADDER.len(), "the whole ladder, and no further");
        assert_eq!((done.army_size, done.castle_slots), (30, 40));
        assert_eq!(world.armies.len(), armies + 2, "two more armies");
        assert!(world.armies[armies..].iter().all(|a| a.faction == red && !a.alive));
    }

    #[test]
    fn armies_are_drawn_above_the_reserve() {
        let mut small = Army::new(Faction::Red, &[(UnitKind::Warrior, AI_MIN_GARRISON + AI_MIN_ARMY - 1)]);
        assert!(draw_army(&mut small, HERO_SLOTS).is_none(), "not enough yet");
        let mut garrison = Army::new(Faction::Red, &[(UnitKind::Warrior, 20), (UnitKind::Archer, 10)]);
        let army = draw_army(&mut garrison, HERO_SLOTS).expect("20 above the reserve");
        assert!((18..=20).contains(&army.slots()), "about 20: {}", army.slots());
        assert!(garrison.slots() >= AI_MIN_GARRISON - 1, "the reserve stays: {}", garrison.slots());
    }
}
