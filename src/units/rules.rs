//! Pure combat maths. No Bevy types here so it can be unit-tested directly.
//! Rules are described in agents.md §3.2 and §3.3.

use rand::Rng;

use super::Stack;

/// What a strike did to the target stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DamageReport {
    /// Health actually removed (after defence).
    pub dealt: u32,
    /// Units that died.
    pub killed: u32,
}

/// Total damage of a stack: every other unit rolls (see `Stack::rolling_units`) and the rolls are summed.
pub fn stack_damage(stack: &Stack, rng: &mut impl Rng) -> u32 {
    let (min, max) = stack.stats().damage;
    (0..stack.rolling_units()).map(|_| rng.random_range(min..=max)).sum()
}

/// Apply `total` damage to a stack.
///
/// Each unit has its own defence buffer. The lead unit takes the hit first; if it dies and
/// damage is left over, the leftover spills to the next unit, which applies its own defence again.
/// `defence_modifier` combines temporary effects: +3 for Shield up, -1 when flanked.
pub fn apply_damage(stack: &mut Stack, total: u32, defence_modifier: i32) -> DamageReport {
    let stats = stack.stats();
    let defence = (stats.defence as i32 + defence_modifier).max(0) as u32;
    let mut remaining = total;
    let mut report = DamageReport::default();

    while remaining > 0 && stack.count > 0 {
        let after_defence = remaining.saturating_sub(defence);
        if after_defence == 0 {
            break;
        }
        if after_defence >= stack.lead_hp {
            remaining = after_defence - stack.lead_hp;
            report.dealt += stack.lead_hp;
            report.killed += 1;
            stack.count -= 1;
            stack.lead_hp = stats.health;
        } else {
            stack.lead_hp -= after_defence;
            report.dealt += after_defence;
            remaining = 0;
        }
    }

    if stack.count == 0 {
        stack.lead_hp = 0;
    }
    report
}

/// Only stacks of the same kind can be merged (agents.md §3.3).
pub fn can_merge(a: &Stack, b: &Stack) -> bool {
    a.kind == b.kind && !a.is_dead() && !b.is_dead()
}

/// Merge `other` into `into`. The wounded lead unit of `other` is healed by the merge
/// (a stack can only track one wounded unit). Returns false if the kinds differ.
pub fn merge(into: &mut Stack, other: &Stack) -> bool {
    if !can_merge(into, other) {
        return false;
    }
    into.count += other.count;
    true
}

/// Split a stack in two equal halves. The first half keeps the wounded lead unit (and, in the
/// game, the remaining AP); the second half is the new stack. Odd counts leave the extra unit
/// in the first half. `None` if there is nothing to split.
pub fn split(stack: &Stack) -> Option<(Stack, Stack)> {
    if stack.count < 2 {
        return None;
    }
    let moved = stack.count / 2;
    let kept = Stack { count: stack.count - moved, ..*stack };
    Some((kept, Stack::new(stack.kind, moved)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::UnitKind;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    #[test]
    fn every_other_unit_rolls() {
        assert_eq!(Stack::new(UnitKind::Pawn, 1).rolling_units(), 1);
        assert_eq!(Stack::new(UnitKind::Pawn, 2).rolling_units(), 1);
        assert_eq!(Stack::new(UnitKind::Pawn, 3).rolling_units(), 2);
        assert_eq!(Stack::new(UnitKind::Pawn, 4).rolling_units(), 2);
        assert_eq!(Stack::new(UnitKind::Pawn, 5).rolling_units(), 3);
    }

    #[test]
    fn stack_damage_sums_the_rolls() {
        let stack = Stack::new(UnitKind::Warrior, 5); // 3 rolls of 3..=5
        let mut rng = StdRng::seed_from_u64(7);
        for _ in 0..50 {
            let dmg = stack_damage(&stack, &mut rng);
            assert!((9..=15).contains(&dmg), "got {dmg}");
        }
    }

    #[test]
    fn defence_is_applied_per_unit() {
        // Warrior: hp 10, defence 3.
        let mut stack = Stack::new(UnitKind::Warrior, 3);
        let report = apply_damage(&mut stack, 7, 0);
        assert_eq!(report, DamageReport { dealt: 4, killed: 0 });
        assert_eq!(stack.count, 3);
        assert_eq!(stack.lead_hp, 6);
    }

    #[test]
    fn damage_below_defence_does_nothing() {
        let mut stack = Stack::new(UnitKind::Warrior, 2);
        let report = apply_damage(&mut stack, 3, 0);
        assert_eq!(report, DamageReport::default());
        assert_eq!(stack, Stack::new(UnitKind::Warrior, 2));
    }

    #[test]
    fn leftover_spills_to_the_next_unit() {
        // Pawn: hp 4, defence 0. 10 damage kills two pawns and wounds the third.
        let mut stack = Stack::new(UnitKind::Pawn, 5);
        let report = apply_damage(&mut stack, 10, 0);
        assert_eq!(report, DamageReport { dealt: 10, killed: 2 });
        assert_eq!(stack.count, 3);
        assert_eq!(stack.lead_hp, 2);
    }

    #[test]
    fn spill_over_pays_defence_again_for_each_unit() {
        // Warrior: hp 10, defence 3. 25 damage: unit 1 takes 22 (dies, 12 left),
        // unit 2 takes 9 (1 hp left), nothing spills further.
        let mut stack = Stack::new(UnitKind::Warrior, 3);
        let report = apply_damage(&mut stack, 25, 0);
        assert_eq!(report, DamageReport { dealt: 19, killed: 1 });
        assert_eq!(stack.count, 2);
        assert_eq!(stack.lead_hp, 1);
    }

    #[test]
    fn a_stack_can_be_wiped_out() {
        let mut stack = Stack::new(UnitKind::Pawn, 2);
        let report = apply_damage(&mut stack, 100, 0);
        assert_eq!(report.killed, 2);
        assert!(stack.is_dead());
        assert_eq!(stack.lead_hp, 0);
    }

    #[test]
    fn shield_up_adds_three_defence() {
        let mut stack = Stack::new(UnitKind::Pawn, 1);
        apply_damage(&mut stack, 3, 3);
        assert_eq!(stack.lead_hp, 4, "3 damage fully absorbed by +3 shield");
    }

    #[test]
    fn flanking_removes_one_defence() {
        // Warrior defence 3 -> 2 when flanked: 7 damage deals 5 instead of 4.
        let mut stack = Stack::new(UnitKind::Warrior, 1);
        let report = apply_damage(&mut stack, 7, -1);
        assert_eq!(report.dealt, 5);
        // Defence never goes below zero.
        let mut pawn = Stack::new(UnitKind::Pawn, 1);
        apply_damage(&mut pawn, 2, -1);
        assert_eq!(pawn.lead_hp, 2);
    }

    #[test]
    fn merge_requires_same_kind() {
        let warriors = Stack::new(UnitKind::Warrior, 2);
        let archers = Stack::new(UnitKind::Archer, 2);
        assert!(can_merge(&warriors, &Stack::new(UnitKind::Warrior, 1)));
        assert!(!can_merge(&warriors, &archers));
        assert!(!can_merge(&warriors, &Stack { count: 0, ..warriors }));
    }

    #[test]
    fn merge_adds_counts_and_keeps_the_lead_unit() {
        let mut a = Stack { lead_hp: 4, ..Stack::new(UnitKind::Warrior, 3) };
        let b = Stack::new(UnitKind::Warrior, 2);
        assert!(merge(&mut a, &b));
        assert_eq!(a.count, 5);
        assert_eq!(a.lead_hp, 4);
        assert!(!merge(&mut a, &Stack::new(UnitKind::Pawn, 1)));
    }

    #[test]
    fn split_halves_and_keeps_the_wounded_unit_in_the_first_half() {
        let stack = Stack { lead_hp: 2, ..Stack::new(UnitKind::Warrior, 5) };
        let (kept, moved) = split(&stack).unwrap();
        assert_eq!((kept.count, kept.lead_hp), (3, 2));
        assert_eq!((moved.count, moved.lead_hp), (2, 10));
        assert!(split(&Stack::new(UnitKind::Warrior, 1)).is_none());
    }
}
