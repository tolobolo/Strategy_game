//! Stacks on the combat grid, the Shield up marker and the two sides of a fight.

use bevy::prelude::*;

use super::{UnitKind, UnitStats};

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

    pub fn stats(&self) -> &'static UnitStats {
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
        self.stats().range.1 > 1
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

