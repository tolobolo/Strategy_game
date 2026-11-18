//! Unit stats loaded from `assets/units.ron`, the single place to balance units.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

use super::{Ability, UnitKind};

/// Where the balance sheet lives on disk (relative to the working directory, like Bevy assets).
pub const UNITS_FILE: &str = "assets/units.ron";
/// Copy built into the binary, used when the file is not found on disk.
const EMBEDDED: &str = include_str!("../../assets/units.ron");

/// Base stats of one unit of a kind, plus its special abilities.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UnitStats {
    pub health: u32,
    pub defence: u32,
    /// Movement range in tiles.
    pub speed: u32,
    /// Damage range `(min, max)`, rolled by every other unit in the stack.
    pub damage: (u32, u32),
    /// Strike range in tiles as `(min, max)`, like damage. `(1, 1)` is melee: the target must be
    /// orthogonally adjacent. Ranged kinds cannot hit anything closer than `min`.
    pub range: (u32, u32),
    /// Gold to hire one unit at a shop or from an idle seller.
    pub price: u32,
    /// Special abilities. Most kinds have none.
    #[serde(default)]
    pub abilities: Vec<Ability>,
}

/// The parsed balance sheet: stats for every unit kind.
#[derive(Debug, Deserialize)]
pub struct UnitBook {
    pub units: HashMap<UnitKind, UnitStats>,
}

impl UnitBook {
    /// Parse and validate a RON balance sheet.
    pub fn parse(text: &str) -> Result<Self, String> {
        let book: UnitBook = ron::from_str(text).map_err(|e| e.to_string())?;
        book.validate()?;
        Ok(book)
    }

    fn validate(&self) -> Result<(), String> {
        for kind in UnitKind::ALL {
            let Some(s) = self.units.get(&kind) else {
                return Err(format!("{kind:?} is missing"));
            };
            if s.health == 0 {
                return Err(format!("{kind:?}: health must be above 0"));
            }
            if s.damage.0 > s.damage.1 {
                return Err(format!("{kind:?}: damage min {} is above max {}", s.damage.0, s.damage.1));
            }
            if s.range.0 == 0 || s.range.0 > s.range.1 {
                return Err(format!("{kind:?}: range must be (min, max) with 1 <= min <= max"));
            }
        }
        Ok(())
    }

    pub fn get(&self, kind: UnitKind) -> &UnitStats {
        &self.units[&kind]
    }
}

/// The loaded book. Read from disk on first use (embedded copy if the file is missing).
/// A broken file panics with the RON error, so a typo shows up at startup.
pub fn book() -> &'static UnitBook {
    static BOOK: OnceLock<UnitBook> = OnceLock::new();
    BOOK.get_or_init(|| {
        let text = std::fs::read_to_string(UNITS_FILE).unwrap_or_else(|_| EMBEDDED.to_string());
        UnitBook::parse(&text).unwrap_or_else(|e| panic!("{UNITS_FILE}: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_file_parses_and_covers_every_kind() {
        let book = UnitBook::parse(EMBEDDED).expect("units.ron must be valid");
        assert_eq!(book.units.len(), UnitKind::ALL.len());
    }

    #[test]
    fn only_the_warrior_has_shield_block() {
        let book = UnitBook::parse(EMBEDDED).unwrap();
        for kind in UnitKind::ALL {
            let has = book.get(kind).abilities.iter().any(|a| matches!(a, Ability::ShieldBlock { .. }));
            assert_eq!(has, kind == UnitKind::Warrior, "{kind:?}");
        }
    }

    const ONE: &str = "(health: 5, defence: 1, speed: 3, damage: (1, 2), range: (1, 1), price: 5)";

    fn sheet(warrior: &str) -> String {
        format!("(units: {{ Warrior: {warrior}, Archer: {ONE}, Lancer: {ONE}, Monk: {ONE}, Pawn: {ONE} }})")
    }

    #[test]
    fn abilities_are_optional() {
        assert!(UnitBook::parse(&sheet(ONE)).is_ok());
    }

    #[test]
    fn a_missing_kind_is_rejected() {
        let text = format!("(units: {{ Warrior: {ONE}, Archer: {ONE}, Lancer: {ONE}, Monk: {ONE} }})");
        assert!(UnitBook::parse(&text).unwrap_err().contains("Pawn"));
    }

    #[test]
    fn damage_min_above_max_is_rejected() {
        let bad = "(health: 5, defence: 1, speed: 3, damage: (5, 3), range: (1, 1), price: 5)";
        assert!(UnitBook::parse(&sheet(bad)).unwrap_err().contains("damage"));
    }
}
