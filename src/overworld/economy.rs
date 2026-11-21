//! Gold, wood and meat: who has them and where gold comes from. See agents.md §1.3 and §1.7.

use std::collections::HashMap;

use bevy::prelude::*;

use super::resources::ResourceKind;
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

/// Gold, wood and meat per faction. The player is `Faction::Reclaimers`. Wood and meat are only
/// stockpiled for now (crafting comes later).
#[derive(Resource, Debug)]
pub struct Treasuries {
    gold: HashMap<Faction, u32>,
    stock: HashMap<(Faction, ResourceKind), u32>,
}

impl Default for Treasuries {
    fn default() -> Self {
        let mut gold = HashMap::new();
        for faction in Faction::KINGDOMS {
            gold.insert(faction, KINGDOM_STARTING_GOLD);
        }
        gold.insert(Faction::Black, BLACK_STARTING_GOLD);
        Self { gold, stock: HashMap::new() }
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

    /// How much of `kind` a faction has (gold included).
    pub fn amount(&self, faction: Faction, kind: ResourceKind) -> u32 {
        match kind {
            ResourceKind::Gold => self.gold(faction),
            _ => self.stock.get(&(faction, kind)).copied().unwrap_or(0),
        }
    }

    /// Take `amount` of `kind`; false (and nothing taken) if there is not enough.
    pub fn spend_resource(&mut self, faction: Faction, kind: ResourceKind, amount: u32) -> bool {
        match kind {
            ResourceKind::Gold => self.spend(faction, amount),
            _ => {
                let entry = self.stock.entry((faction, kind)).or_default();
                if *entry < amount {
                    return false;
                }
                *entry -= amount;
                true
            }
        }
    }

    /// Add `amount` of `kind` (gold goes into the gold total).
    pub fn add_resource(&mut self, faction: Faction, kind: ResourceKind, amount: u32) {
        match kind {
            ResourceKind::Gold => self.add(faction, amount),
            _ => *self.stock.entry((faction, kind)).or_default() += amount,
        }
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

    /// Take `pct` % of a faction's gold (rounded down). Winning a fight takes half of the loser's
    /// (more with the crafted "Keep your money").
    pub fn take_pct(&mut self, from: Faction, pct: u32) -> u32 {
        let entry = self.gold.entry(from).or_default();
        let loot = *entry * pct.min(100) / 100;
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

/// Ticks the crafted "Passive gathering" upgrade (agents.md §1.8).
#[derive(Resource, Default)]
pub struct PassiveTimer(pub f32);

pub struct EconomyPlugin;

impl Plugin for EconomyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Treasuries>()
            .init_resource::<IncomeTimer>()
            .init_resource::<PassiveTimer>()
            .add_systems(Update, (collect_income, passive_gathering).run_if(in_state(GameState::Overworld)));
    }
}

/// Every `INCOME_INTERVAL_SECS` each castle and tower pays its current owner, plus the crafted
/// income bonuses on yours.
pub fn collect_income(
    time: Res<Time>,
    mut timer: ResMut<IncomeTimer>,
    world: Res<WorldState>,
    mut treasuries: ResMut<Treasuries>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let up = &world.upgrades;
    for base in &world.bases {
        let bonus = if base.owner.is_player() { up.castle_income_bonus() } else { 0 };
        treasuries.add(base.owner, BASE_INCOME + bonus);
    }
    for tower in &world.towers {
        let bonus = if tower.owner.is_player() { up.tower_income_bonus() } else { 0 };
        treasuries.add(tower.owner, TOWER_INCOME + bonus);
    }
}

/// The crafted "Passive gathering": wood and meat every few seconds, from nowhere.
fn passive_gathering(
    time: Res<Time>,
    mut timer: ResMut<PassiveTimer>,
    world: Res<WorldState>,
    mut treasuries: ResMut<Treasuries>,
) {
    let Some((wood, meat, every)) = world.upgrades.passive() else { return };
    timer.0 += time.delta_secs();
    if timer.0 >= every {
        timer.0 -= every;
        treasuries.add_resource(Faction::Reclaimers, ResourceKind::Wood, wood);
        treasuries.add_resource(Faction::Reclaimers, ResourceKind::Meat, meat);
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
        assert_eq!(t.take_pct(Faction::Red, 50), 25);
        assert_eq!(t.gold(Faction::Red), 25);
    }

    #[test]
    fn wood_and_meat_stockpile_and_gold_joins_the_gold() {
        let mut t = Treasuries::default();
        t.add_resource(Faction::Reclaimers, ResourceKind::Wood, 5);
        t.add_resource(Faction::Reclaimers, ResourceKind::Wood, 5);
        t.add_resource(Faction::Reclaimers, ResourceKind::Meat, 4);
        t.add_resource(Faction::Reclaimers, ResourceKind::Gold, 5);
        assert_eq!(t.amount(Faction::Reclaimers, ResourceKind::Wood), 10);
        assert_eq!(t.amount(Faction::Reclaimers, ResourceKind::Meat), 4);
        assert_eq!(t.player_gold(), KINGDOM_STARTING_GOLD + 5);
        assert_eq!(t.amount(Faction::Red, ResourceKind::Wood), 0);
    }
}
