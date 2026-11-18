//! The unit kinds. Their numbers live in `assets/units.ron` (see `stats.rs`).

use bevy::prelude::*;
use serde::Deserialize;

use super::{Ability, UnitStats, stats};

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum UnitKind {
    Warrior,
    Archer,
    Lancer,
    Monk,
    Pawn,
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

    /// Base stats and abilities from the balance sheet.
    pub fn stats(self) -> &'static UnitStats {
        stats::book().get(self)
    }

    /// Gold to hire one unit at a shop or from an idle seller (agents.md §1.3).
    pub fn price(self) -> u32 {
        self.stats().price
    }

    pub fn abilities(self) -> &'static [Ability] {
        &self.stats().abilities
    }

    /// Defence bonus of the ShieldBlock ability, or `None` if this kind does not have it.
    pub fn shield_bonus(self) -> Option<u32> {
        self.abilities().iter().find_map(|a| match a {
            Ability::ShieldBlock { bonus } => Some(*bonus),
        })
    }

    /// `   Shield +3` style suffix for stat panels; empty when the kind has no abilities.
    pub fn abilities_text(self) -> String {
        self.abilities().iter().map(|a| format!("   {}", a.label())).collect()
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
