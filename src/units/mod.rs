//! Unit kinds, stats, abilities, stacks and armies. See agents.md §1.4 and §3.1.
//! The numbers for every unit kind are in `assets/units.ron`.

pub mod ability;
pub mod army;
pub mod kind;
pub mod rules;
pub mod stack;
pub mod stats;

pub use ability::Ability;
pub use army::{Army, PlayerArmy};
pub use kind::UnitKind;
pub use stack::{ShieldUp, Stack, Team};
pub use stats::UnitStats;

/// One unit takes one slot. The hero can carry this many units.
pub const HERO_SLOTS: u32 = 20;
/// A castle garrison can hold this many units.
pub const CASTLE_SLOTS: u32 = 30;
/// A tower you own can hold this many units as its guard.
pub const TOWER_SLOTS: u32 = 10;
