//! The AI kingdoms' economy: buying units for the castle and drawing armies from it. Where the
//! armies walk and what they attack is in `armies.rs`. See agents.md §1.5.

use bevy::prelude::*;
use rand::Rng;

use super::economy::Treasuries;
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
pub const AI_MIN_GARRISON: u32 = 10;
/// An army only sets out once at least this many units are available above the reserve.
pub const AI_MIN_ARMY: u32 = 8;

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
pub fn buy_units(garrison: &mut Army, faction: Faction, treasuries: &mut Treasuries, rng: &mut impl Rng) -> u32 {
    let mut bought = 0;
    while bought < AI_BUYS_PER_THINK && garrison.slots() < CASTLE_SLOTS {
        let kind = pick_kind(rng);
        if !treasuries.spend(faction, kind.price()) {
            break;
        }
        garrison.add(kind, 1);
        bought += 1;
    }
    bought
}

/// Take a new army out of `garrison`: everything above `AI_MIN_GARRISON`, at most `HERO_SLOTS`,
/// or nothing if fewer than `AI_MIN_ARMY` are available.
pub fn draw_army(garrison: &mut Army) -> Option<Army> {
    let available = garrison.slots().saturating_sub(AI_MIN_GARRISON).min(HERO_SLOTS);
    if available < AI_MIN_ARMY {
        return None;
    }
    let army = garrison.split_off_fraction(available as f32 / garrison.slots() as f32);
    (!army.is_empty()).then_some(army)
}

/// Every `AI_THINK_SECS`, each AI kingdom that holds its own castle buys units for it.
pub fn ai_kingdoms_buy(
    time: Res<Time>,
    mut timer: Local<f32>,
    mut world: ResMut<WorldState>,
    mut treasuries: ResMut<Treasuries>,
) {
    *timer += time.delta_secs();
    if *timer < AI_THINK_SECS {
        return;
    }
    *timer = 0.0;
    let mut rng = rand::rng();
    for base in world.bases.iter_mut().filter(|b| b.owner == b.home && !b.owner.is_player()) {
        buy_units(&mut base.garrison, base.owner, &mut treasuries, &mut rng);
    }
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
        let bought = buy_units(&mut garrison, Faction::Red, &mut treasuries, &mut rng);
        assert!(bought >= 1 && bought <= AI_BUYS_PER_THINK);
        let spent: u32 = UnitKind::ALL.iter().map(|k| garrison.count_of(*k) * k.price()).sum();
        assert_eq!(treasuries.gold(Faction::Red), before - spent);
        assert_eq!(garrison.count_of(UnitKind::Pawn), 0, "no pawns");

        let mut full = Army::new(Faction::Red, &[(UnitKind::Warrior, CASTLE_SLOTS)]);
        assert_eq!(buy_units(&mut full, Faction::Red, &mut treasuries, &mut rng), 0, "no room");
        let mut broke = Treasuries::default();
        broke.spend(Faction::Red, broke.gold(Faction::Red));
        assert_eq!(buy_units(&mut garrison, Faction::Red, &mut broke, &mut rng), 0, "no gold");
    }

    #[test]
    fn armies_are_drawn_above_the_reserve() {
        let mut small = Army::new(Faction::Red, &[(UnitKind::Warrior, AI_MIN_GARRISON + AI_MIN_ARMY - 1)]);
        assert!(draw_army(&mut small).is_none(), "not enough yet");
        let mut garrison = Army::new(Faction::Red, &[(UnitKind::Warrior, 20), (UnitKind::Archer, 10)]);
        let army = draw_army(&mut garrison).expect("20 above the reserve");
        assert!((18..=20).contains(&army.slots()), "about 20: {}", army.slots());
        assert!(garrison.slots() >= AI_MIN_GARRISON - 1, "the reserve stays: {}", garrison.slots());
    }
}
