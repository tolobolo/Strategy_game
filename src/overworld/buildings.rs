//! Buildings that make up a kingdom base, and the lone towers. See agents.md §1.2.

use bevy::prelude::*;

use crate::units::UnitKind;

/// A base is this many tiles wide and tall.
pub const BASE_SIZE: IVec2 = IVec2::new(5, 4);

/// The free tile in the middle of a base where the hero and the AI armies spawn.
pub const BASE_SPAWN_OFFSET: IVec2 = IVec2::new(2, 1);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BuildingKind {
    Castle,
    House,
    Archery,
    Barracks,
    Monastery,
    /// Towers are not part of a base, but share the sprite machinery.
    Tower,
}

impl BuildingKind {
    pub const ALL: [BuildingKind; 6] = [
        BuildingKind::Castle,
        BuildingKind::House,
        BuildingKind::Archery,
        BuildingKind::Barracks,
        BuildingKind::Monastery,
        BuildingKind::Tower,
    ];

    pub fn name(self) -> &'static str {
        match self {
            BuildingKind::Castle => "Castle",
            BuildingKind::House => "House",
            BuildingKind::Archery => "Archery",
            BuildingKind::Barracks => "Barracks",
            BuildingKind::Monastery => "Monastery",
            BuildingKind::Tower => "Tower",
        }
    }

    /// Which unit this building sells to its owner. The castle and towers sell nothing.
    pub fn sells(self) -> Option<UnitKind> {
        match self {
            BuildingKind::House => Some(UnitKind::Pawn),
            BuildingKind::Archery => Some(UnitKind::Archer),
            BuildingKind::Barracks => Some(UnitKind::Warrior),
            BuildingKind::Monastery => Some(UnitKind::Monk),
            BuildingKind::Castle | BuildingKind::Tower => None,
        }
    }

    /// Footprint in tiles.
    pub fn size(self) -> IVec2 {
        match self {
            BuildingKind::Castle => IVec2::new(2, 2),
            _ => IVec2::ONE,
        }
    }

    /// Sprite under `buildings/<colour>_buildings/` (agents.md §5).
    pub fn sprite_file(self) -> &'static str {
        match self {
            BuildingKind::Castle => "castle.png",
            BuildingKind::House => "house1.png",
            BuildingKind::Archery => "archery.png",
            BuildingKind::Barracks => "barracks.png",
            BuildingKind::Monastery => "monastery.png",
            BuildingKind::Tower => "tower.png",
        }
    }
}

/// Where each building sits inside the 5×4 base, as an offset from the base's bottom-left tile.
///
/// ```text
/// y=3:  .  C  C  .  M
/// y=2:  .  C  C  .  .
/// y=1:  .  .  *  .  .      * = spawn tile
/// y=0:  H  .  A  .  B
/// ```
pub fn base_layout() -> [(BuildingKind, IVec2); 5] {
    [
        (BuildingKind::Castle, IVec2::new(1, 2)),
        (BuildingKind::Monastery, IVec2::new(4, 3)),
        (BuildingKind::House, IVec2::new(0, 0)),
        (BuildingKind::Archery, IVec2::new(2, 0)),
        (BuildingKind::Barracks, IVec2::new(4, 0)),
    ]
}
