//! The kingdoms and the neutral tower faction. See agents.md §2.

use bevy::prelude::*;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Faction {
    /// The player's kingdom (Blue units in the Tiny Swords pack).
    Reclaimers,
    /// Castle Knights: high defence, slow heavy armour.
    Red,
    /// Pawns / Goblins: many weak, fast units.
    Purple,
    /// Barbarians / Raiders: glass cannons, high attack, low armour.
    Yellow,
    /// Not a kingdom: owns the towers scattered around the map and the bandit squads.
    Black,
}

impl Faction {
    /// The four factions that own a base. Index 0 is always the player.
    pub const KINGDOMS: [Faction; 4] = [Faction::Reclaimers, Faction::Red, Faction::Purple, Faction::Yellow];

    pub fn is_player(self) -> bool {
        self == Faction::Reclaimers
    }

    pub fn name(self) -> &'static str {
        match self {
            Faction::Reclaimers => "Reclaimers",
            Faction::Red => "Red kingdom",
            Faction::Purple => "Purple kingdom",
            Faction::Yellow => "Yellow kingdom",
            Faction::Black => "Black towers",
        }
    }

    /// Fill colour of the placeholder squares.
    pub fn color(self) -> Color {
        match self {
            Faction::Reclaimers => Color::srgb(0.20, 0.45, 0.90),
            Faction::Red => Color::srgb(0.80, 0.20, 0.20),
            Faction::Purple => Color::srgb(0.55, 0.30, 0.80),
            Faction::Yellow => Color::srgb(0.90, 0.75, 0.10),
            Faction::Black => Color::srgb(0.15, 0.15, 0.18),
        }
    }

    /// Text colour that stays readable on top of `color()`.
    pub fn text_color(self) -> Color {
        match self {
            Faction::Yellow => Color::BLACK,
            _ => Color::WHITE,
        }
    }

    /// Folder with this faction's unit sheets, relative to the asset root (agents.md §5).
    #[allow(dead_code)]
    pub fn sprite_dir(self) -> &'static str {
        match self {
            Faction::Reclaimers => "Units/Blue Units",
            Faction::Red => "Units/Red Units",
            Faction::Purple => "Units/Purple Units",
            Faction::Yellow => "Units/Yellow Units",
            Faction::Black => "Units/Black Units",
        }
    }
}
