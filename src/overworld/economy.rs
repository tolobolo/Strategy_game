//! Gold: who has it and where it comes from. See agents.md §1.3.

use std::collections::HashMap;

use bevy::prelude::*;

use super::world::WorldState;
use crate::faction::Faction;
use crate::states::GameState;

/// Every castle pays its owner this much per income tick.
pub const BASE_INCOME: u32 = 10;
/// Every tower pays its owner this much per income tick.
pub const TOWER_INCOME: u32 = 5;
pub const INCOME_INTERVAL_SECS: f32 = 30.0;
pub const KINGDOM_STARTING_GOLD: u32 = 50;
pub const BLACK_STARTING_GOLD: u32 = 20;

/// Gold per faction. The player is `Faction::Reclaimers`.
#[derive(Resource, Debug)]
pub struct Treasuries {
    gold: HashMap<Faction, u32>,
}

impl Default for Treasuries {
    fn default() -> Self {
        let mut gold = HashMap::new();
        for faction in Faction::KINGDOMS {
            gold.insert(faction, KINGDOM_STARTING_GOLD);
        }
        gold.insert(Faction::Black, BLACK_STARTING_GOLD);
        Self { gold }
    }
}

impl Treasuries {
    pub fn gold(&self, faction: Faction) -> u32 {
        self.gold.get(&faction).copied().unwrap_or(0)
    }

    pub fn player_gold(&self) -> u32 {
        self.gold(Faction::Reclaimers)
    }

    pub fn add(&mut self, faction: Faction, amount: u32) {
        *self.gold.entry(faction).or_default() += amount;
    }

    /// Returns false (and changes nothing) if the faction cannot afford it.
    pub fn spend(&mut self, faction: Faction, amount: u32) -> bool {
        let entry = self.gold.entry(faction).or_default();
        if *entry < amount {
            return false;
        }
        *entry -= amount;
        true
    }

    /// Winning a fight takes half of the loser's gold.
    pub fn take_half(&mut self, from: Faction) -> u32 {
        let entry = self.gold.entry(from).or_default();
        let loot = *entry / 2;
        *entry -= loot;
        loot
    }
}

#[derive(Resource)]
pub struct IncomeTimer(pub Timer);

impl Default for IncomeTimer {
    fn default() -> Self {
        Self(Timer::from_seconds(INCOME_INTERVAL_SECS, TimerMode::Repeating))
    }
}

pub struct EconomyPlugin;

impl Plugin for EconomyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Treasuries>()
            .init_resource::<IncomeTimer>()
            .add_systems(Update, collect_income.run_if(in_state(GameState::Overworld)));
    }
}

/// Every `INCOME_INTERVAL_SECS` each castle and tower pays its current owner.
fn collect_income(
    time: Res<Time>,
    mut timer: ResMut<IncomeTimer>,
    world: Res<WorldState>,
    mut treasuries: ResMut<Treasuries>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    for base in &world.bases {
        treasuries.add(base.owner, BASE_INCOME);
    }
    for tower in &world.towers {
        treasuries.add(tower.owner, TOWER_INCOME);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spend_and_loot() {
        let mut t = Treasuries::default();
        assert_eq!(t.player_gold(), KINGDOM_STARTING_GOLD);
        assert!(t.spend(Faction::Reclaimers, 20));
        assert!(!t.spend(Faction::Reclaimers, 1000));
        assert_eq!(t.player_gold(), 30);
        assert_eq!(t.take_half(Faction::Red), 25);
        assert_eq!(t.gold(Faction::Red), 25);
    }
}
