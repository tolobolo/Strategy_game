//! Unit kinds, stats, stacks and armies. See agents.md §1.4 and §3.1.

pub mod rules;

use bevy::prelude::*;

use crate::faction::Faction;

/// One unit takes one slot. The hero can carry this many units.
pub const HERO_SLOTS: u32 = 20;
/// A castle garrison can hold this many units.
pub const CASTLE_SLOTS: u32 = 30;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnitKind {
    Warrior,
    Archer,
    Lancer,
    Monk,
    Pawn,
}

/// Base stats of one unit of a kind. Numbers are placeholders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitStats {
    pub health: u32,
    pub defence: u32,
    /// Movement range in tiles.
    pub speed: u32,
    /// Damage range `(min, max)`, rolled by every other unit in the stack.
    pub damage: (u32, u32),
    /// Strike range in tiles. 1 = melee (must be orthogonally adjacent).
    pub range: u32,
}

impl UnitKind {
    /// Order used by the castle transfer keys 1–5.
    pub const ALL: [UnitKind; 5] = [
        UnitKind::Warrior,
        UnitKind::Archer,
        UnitKind::Lancer,
        UnitKind::Monk,
        UnitKind::Pawn,
    ];

    pub fn stats(self) -> UnitStats {
        match self {
            UnitKind::Warrior => UnitStats { health: 10, defence: 3, speed: 3, damage: (3, 5), range: 1 },
            UnitKind::Archer => UnitStats { health: 6, defence: 1, speed: 3, damage: (2, 4), range: 6 },
            UnitKind::Lancer => UnitStats { health: 12, defence: 4, speed: 2, damage: (4, 6), range: 1 },
            UnitKind::Monk => UnitStats { health: 7, defence: 2, speed: 3, damage: (1, 2), range: 1 },
            UnitKind::Pawn => UnitStats { health: 4, defence: 0, speed: 4, damage: (1, 3), range: 1 },
        }
    }

    /// Gold to hire one unit at a shop or from an idle seller (agents.md §1.3).
    pub fn price(self) -> u32 {
        match self {
            UnitKind::Pawn => 5,
            UnitKind::Archer => 10,
            UnitKind::Warrior => 15,
            UnitKind::Monk => 20,
            UnitKind::Lancer => 25,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            UnitKind::Warrior => "Warrior",
            UnitKind::Archer => "Archer",
            UnitKind::Lancer => "Lancer",
            UnitKind::Monk => "Monk",
            UnitKind::Pawn => "Pawn",
        }
    }

    /// Letter drawn on the placeholder square.
    pub fn initial(self) -> char {
        match self {
            UnitKind::Warrior => 'W',
            UnitKind::Archer => 'A',
            UnitKind::Lancer => 'L',
            UnitKind::Monk => 'M',
            UnitKind::Pawn => 'P',
        }
    }
}

/// A group of units of one kind on one tile.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stack {
    pub kind: UnitKind,
    pub count: u32,
    /// Health left on the unit at the front of the stack.
    pub lead_hp: u32,
}

impl Stack {
    pub fn new(kind: UnitKind, count: u32) -> Self {
        Self { kind, count, lead_hp: kind.stats().health }
    }

    pub fn stats(&self) -> UnitStats {
        self.kind.stats()
    }

    pub fn is_dead(&self) -> bool {
        self.count == 0
    }

    /// How many units roll for damage: every other unit (1 -> 1, 2 -> 1, 3 -> 2, 4 -> 2 ...).
    pub fn rolling_units(&self) -> u32 {
        self.count.div_ceil(2)
    }

    pub fn is_ranged(&self) -> bool {
        self.stats().range > 1
    }

    /// Compact label such as `W x12`.
    pub fn short_label(&self) -> String {
        format!("{} x{}", self.kind.initial(), self.count)
    }
}

/// Temporary defence bonus from the Shield up action. Removed when the stack's next turn starts.
#[derive(Component, Debug, Clone, Copy)]
pub struct ShieldUp(pub u32);

/// Whose side a stack fights on. Drives the green/red "yours vs theirs" visuals.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Team {
    Player,
    Enemy,
}

impl Team {
    pub fn color(self) -> Color {
        match self {
            Team::Player => Color::srgb(0.20, 0.90, 0.30),
            Team::Enemy => Color::srgb(0.95, 0.20, 0.20),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Team::Player => "YOU",
            Team::Enemy => "ENEMY",
        }
    }
}

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
