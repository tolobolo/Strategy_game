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

    /// Folder with this faction's building sprites, relative to `assets/` (agents.md §5).
    pub fn building_dir(self) -> &'static str {
        match self {
            Faction::Reclaimers => "buildings/blue_buildings",
            Faction::Red => "buildings/red_buildings",
            Faction::Purple => "buildings/purple_buildings",
            Faction::Yellow => "buildings/yellow_buildings",
            Faction::Black => "buildings/black_buildings",
        }
    }

    /// Folder with this faction's unit sheets, relative to `assets/` (agents.md §5).
    pub fn sprite_dir(self) -> &'static str {
        match self {
            Faction::Reclaimers => "units/blue_units",
            Faction::Red => "units/red_units",
            Faction::Purple => "units/purple_units",
            Faction::Yellow => "units/yellow_units",
            Faction::Black => "units/black_units",
        }
    }
}
