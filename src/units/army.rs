//! Armies: lists of stacks for the hero, garrisons and roaming AI armies.

use bevy::prelude::*;

use super::{Stack, UnitKind};
use crate::faction::Faction;

/// A list of stacks belonging to one faction: a hero army, a garrison, a roaming army.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Army {
    pub faction: Faction,
    pub stacks: Vec<Stack>,
}

impl Army {
    pub fn new(faction: Faction, roster: &[(UnitKind, u32)]) -> Self {
        Self {
            faction,
            stacks: roster.iter().map(|&(kind, count)| Stack::new(kind, count)).collect(),
        }
    }

    pub fn empty(faction: Faction) -> Self {
        Self { faction, stacks: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.slots() == 0
    }

    /// Total units; every unit takes one slot.
    pub fn slots(&self) -> u32 {
        self.stacks.iter().map(|s| s.count).sum()
    }

    pub fn count_of(&self, kind: UnitKind) -> u32 {
        self.stacks.iter().filter(|s| s.kind == kind).map(|s| s.count).sum()
    }

    /// Add units, merging into an existing stack of that kind if there is one.
    pub fn add(&mut self, kind: UnitKind, n: u32) {
        if n == 0 {
            return;
        }
        match self.stacks.iter_mut().find(|s| s.kind == kind && !s.is_dead()) {
            Some(stack) => stack.count += n,
            None => self.stacks.push(Stack::new(kind, n)),
        }
    }

    /// Remove up to `n` units of a kind. Returns how many were actually removed.
    pub fn remove(&mut self, kind: UnitKind, n: u32) -> u32 {
        let mut left = n;
        for stack in self.stacks.iter_mut().filter(|s| s.kind == kind) {
            let take = left.min(stack.count);
            stack.count -= take;
            left -= take;
            if stack.count > 0 && stack.lead_hp == 0 {
                stack.lead_hp = kind.stats().health;
            }
        }
        self.prune();
        n - left
    }

    /// Drop empty stacks.
    pub fn prune(&mut self) {
        self.stacks.retain(|s| !s.is_dead());
    }

    /// Remove roughly `fraction` of the units, spread over the stacks, and return them as a
    /// new army of the same faction (used when an AI army garrisons a tower).
    pub fn split_off_fraction(&mut self, fraction: f32) -> Army {
        let mut taken = Army::empty(self.faction);
        for stack in &mut self.stacks {
            let n = ((stack.count as f32 * fraction).round() as u32).min(stack.count);
            if n > 0 {
                stack.count -= n;
                taken.add(stack.kind, n);
            }
        }
        self.prune();
        taken
    }

    /// `W x10  A x6  L x3`, or `(none)` when empty.
    pub fn summary(&self) -> String {
        let text = self
            .stacks
            .iter()
            .filter(|s| !s.is_dead())
            .map(Stack::short_label)
            .collect::<Vec<_>>()
            .join("  ");
        if text.is_empty() { "(none)".to_string() } else { text }
    }
}

/// The stacks the hero carries into every battle. Survivors are written back after each fight.
/// Starts empty: the hero takes units from the home castle (agents.md §1.4).
#[derive(Resource, Debug, Clone)]
pub struct PlayerArmy(pub Army);

impl Default for PlayerArmy {
    fn default() -> Self {
        Self(Army::empty(Faction::Reclaimers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn army_add_remove_and_slots() {
        let mut army = Army::empty(Faction::Reclaimers);
        assert!(army.is_empty());
        army.add(UnitKind::Warrior, 5);
        army.add(UnitKind::Warrior, 3);
        army.add(UnitKind::Archer, 2);
        assert_eq!(army.stacks.len(), 2, "same kind merges into one stack");
        assert_eq!(army.slots(), 10);
        assert_eq!(army.remove(UnitKind::Warrior, 20), 8, "cannot remove more than there is");
        assert_eq!(army.count_of(UnitKind::Warrior), 0);
        assert_eq!(army.stacks.len(), 1, "empty stacks are pruned");
        assert_eq!(army.summary(), "A x2");
    }

    #[test]
    fn split_off_fraction_moves_part_of_every_stack() {
        let mut army = Army::new(Faction::Red, &[(UnitKind::Warrior, 10), (UnitKind::Archer, 4)]);
        let taken = army.split_off_fraction(0.25);
        assert_eq!(taken.faction, Faction::Red);
        assert_eq!(taken.count_of(UnitKind::Warrior), 3); // 2.5 rounds to 3
        assert_eq!(taken.count_of(UnitKind::Archer), 1);
        assert_eq!(army.slots(), 10);
    }
}
