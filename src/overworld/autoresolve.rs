//! Fights the hero is not in (AI against AI, an AI army against a tower guard) are settled by a
//! formula instead of on the combat grid. See agents.md §1.5.

use rand::Rng;

use crate::units::Army;

/// Each side's strength is multiplied by a random factor in this range.
pub const AUTO_LUCK: (f32, f32) = (0.8, 1.2);
/// The winner loses this × (loser strength / winner strength) of its units.
pub const AUTO_LOSS_SCALE: f32 = 0.6;

/// Σ per stack: count × average damage × (health + defence).
pub fn strength(army: &Army) -> f32 {
    army.stacks
        .iter()
        .map(|stack| {
            let stats = stack.stats();
            let damage = (stats.damage.0 + stats.damage.1) as f32 / 2.0;
            stack.count as f32 * damage * (stats.health + stats.defence) as f32
        })
        .sum()
}

/// Fight `attacker` against `defender`. Returns true when the attacker wins. The loser is left
/// empty; the winner loses `AUTO_LOSS_SCALE × loser / winner` of its units, spread over its
/// stacks. An empty side always loses.
pub fn resolve(attacker: &mut Army, defender: &mut Army, rng: &mut impl Rng) -> bool {
    if defender.is_empty() {
        return true;
    }
    if attacker.is_empty() {
        return false;
    }
    let a = strength(attacker) * rng.random_range(AUTO_LUCK.0..=AUTO_LUCK.1);
    let d = strength(defender) * rng.random_range(AUTO_LUCK.0..=AUTO_LUCK.1);
    let attacker_wins = a >= d;
    let (winner, loser, ratio) = if attacker_wins { (attacker, defender, d / a) } else { (defender, attacker, a / d) };
    winner.split_off_fraction((AUTO_LOSS_SCALE * ratio).min(1.0));
    *loser = Army::empty(loser.faction);
    attacker_wins
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::*;
    use crate::faction::Faction;
    use crate::units::UnitKind;

    fn army(faction: Faction, warriors: u32) -> Army {
        Army::new(faction, &[(UnitKind::Warrior, warriors)])
    }

    #[test]
    fn strength_adds_up_the_stacks() {
        // Warrior: damage 3-5 (avg 4), health 10, defence 3 -> 52 per unit.
        assert_eq!(strength(&army(Faction::Red, 2)), 104.0);
        assert_eq!(strength(&Army::empty(Faction::Red)), 0.0);
    }

    #[test]
    fn the_stronger_side_wins_and_losses_follow_the_ratio() {
        let mut rng = StdRng::seed_from_u64(1);
        let mut wins = 0;
        for _ in 0..200 {
            let (mut big, mut small) = (army(Faction::Red, 20), army(Faction::Yellow, 5));
            if resolve(&mut big, &mut small, &mut rng) {
                wins += 1;
                assert!(small.is_empty(), "the loser is wiped out");
                assert!((14..20).contains(&big.slots()), "a lopsided win costs little: {}", big.slots());
            }
        }
        assert_eq!(wins, 200, "four times the strength always wins");

        let (mut a, mut b) = (army(Faction::Red, 10), army(Faction::Yellow, 10));
        let a_won = resolve(&mut a, &mut b, &mut rng);
        let winner = if a_won { &a } else { &b };
        assert!(winner.slots() <= 6, "an even fight is costly: {}", winner.slots());
    }

    #[test]
    fn an_empty_side_always_loses() {
        let mut rng = StdRng::seed_from_u64(2);
        let (mut a, mut empty) = (army(Faction::Red, 3), Army::empty(Faction::Black));
        assert!(resolve(&mut a, &mut empty, &mut rng));
        assert_eq!(a.slots(), 3, "taking an empty tower costs nothing");
        assert!(!resolve(&mut empty, &mut a, &mut rng));
    }
}
