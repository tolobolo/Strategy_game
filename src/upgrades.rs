//! The crafting tree: upgrades you buy at your House with wood, meat and gold. The tree is
//! loaded from `assets/upgrades.ron`; what you bought lives in `WorldState::upgrades`, and the
//! getters below are what the rest of the game reads instead of the plain constants.
//! See agents.md §1.8.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::faction::Faction;
use crate::overworld::Treasuries;
use crate::overworld::armies::SIEGE_SECS;
use crate::overworld::resources::ResourceKind;
use crate::units::{CASTLE_SLOTS, HERO_SLOTS, UnitKind};

pub const UPGRADES_FILE: &str = "assets/upgrades.ron";
const EMBEDDED: &str = include_str!("../assets/upgrades.ron");
/// Without `keep_money`, a lost fight takes this share of your gold.
pub const BASE_LOSS_PCT: u32 = 50;

/// What a bought upgrade does. Numbers add up when several give the same thing.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub enum Effect {
    /// The root: only opens the tree.
    None,
    /// More units with the hero.
    ArmySlots(u32),
    /// More units in each of your castles.
    CastleSlots(u32),
    /// Loot `loot_pct` % more of the loser's gold; a lost fight takes `loss_pct` % of yours.
    KeepMoney { loot_pct: u32, loss_pct: u32 },
    /// More workers at every site.
    WorkerSlots(u32),
    /// Units cost this many % less.
    UnitPricePct(u32),
    /// Survivors heal to full after a fight.
    HealAfterFight,
    /// Sieges on your places last this much longer.
    SiegeSecs(f32),
    /// Extra gold per income tick from each of your towers / castles.
    TowerIncome(u32),
    CastleIncome(u32),
    /// Wood and meat out of nowhere every `every_secs`.
    PassiveIncome { wood: u32, meat: u32, every_secs: f32 },
    /// Workers walk `speed_pct` % faster and carry `carry` more per trip.
    WorkerBoost { speed_pct: u32, carry: u32 },
}

/// Wood, meat and gold, in that order (like the file's `prices`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub struct Cost(pub u32, pub u32, pub u32);

impl Cost {
    pub fn parts(self) -> [(ResourceKind, u32); 3] {
        [(ResourceKind::Wood, self.0), (ResourceKind::Meat, self.1), (ResourceKind::Gold, self.2)]
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Upgrade {
    pub id: String,
    pub name: String,
    pub tier: u32,
    pub needs: Option<String>,
    /// Card position in the tree, grid units (x right, y down).
    pub pos: (f32, f32),
    pub text: String,
    pub effect: Effect,
    /// Filled in from the tier's price after loading.
    #[serde(skip)]
    pub cost: Cost,
}

#[derive(Debug, Deserialize)]
pub struct UpgradeBook {
    prices: HashMap<u32, Cost>,
    pub upgrades: Vec<Upgrade>,
}

impl UpgradeBook {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut book: UpgradeBook = ron::from_str(text).map_err(|e| e.to_string())?;
        for u in &mut book.upgrades {
            u.cost = *book.prices.get(&u.tier).ok_or_else(|| format!("{}: no price for tier {}", u.id, u.tier))?;
        }
        book.validate()?;
        Ok(book)
    }

    fn validate(&self) -> Result<(), String> {
        let ids: HashSet<&str> = self.upgrades.iter().map(|u| u.id.as_str()).collect();
        if ids.len() != self.upgrades.len() {
            return Err("two upgrades share an id".to_string());
        }
        let roots = self.upgrades.iter().filter(|u| u.needs.is_none()).count();
        if roots != 1 {
            return Err(format!("the tree needs exactly one root (no `needs`), found {roots}"));
        }
        for u in &self.upgrades {
            if let Some(parent) = &u.needs
                && !ids.contains(parent.as_str())
            {
                return Err(format!("{}: needs unknown upgrade {parent}", u.id));
            }
            // Walk up to the root; more steps than upgrades means a loop.
            let mut at = u;
            for _ in 0..=self.upgrades.len() {
                match &at.needs {
                    None => break,
                    Some(parent) => at = self.get(parent).unwrap(),
                }
            }
            if at.needs.is_some() {
                return Err(format!("{}: its `needs` go round in a loop", u.id));
            }
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Upgrade> {
        self.upgrades.iter().find(|u| u.id == id)
    }
}

/// The loaded tree. Read from disk on first use (embedded copy if the file is missing); a broken
/// file panics with the RON error, so a typo shows at startup.
pub fn book() -> &'static UpgradeBook {
    static BOOK: OnceLock<UpgradeBook> = OnceLock::new();
    BOOK.get_or_init(|| {
        let text = std::fs::read_to_string(UPGRADES_FILE).unwrap_or_else(|_| EMBEDDED.to_string());
        UpgradeBook::parse(&text).unwrap_or_else(|e| panic!("{UPGRADES_FILE}: {e}"))
    })
}

/// Where an upgrade stands for you.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    Bought,
    /// Its parent is bought: you can buy it (if you can afford it).
    Open,
    /// Its parent is not bought yet.
    Locked,
}

/// What you have bought. Lives in `WorldState::upgrades`.
#[derive(Debug, Clone, Default)]
pub struct Upgrades {
    bought: HashSet<String>,
}

impl Upgrades {
    pub fn is_bought(&self, id: &str) -> bool {
        self.bought.contains(id)
    }

    pub fn standing(&self, upgrade: &Upgrade) -> Standing {
        if self.is_bought(&upgrade.id) {
            Standing::Bought
        } else if upgrade.needs.as_ref().is_none_or(|p| self.is_bought(p)) {
            Standing::Open
        } else {
            Standing::Locked
        }
    }

    /// Why you cannot buy `upgrade` now, or `Ok`.
    pub fn can_buy(&self, upgrade: &Upgrade, treasuries: &Treasuries) -> Result<(), String> {
        match self.standing(upgrade) {
            Standing::Bought => return Err(format!("{} is already yours.", upgrade.name)),
            Standing::Locked => {
                let parent = upgrade.needs.as_deref().and_then(|p| book().get(p)).map_or("", |p| p.name.as_str());
                return Err(format!("Locked: buy {parent} first."));
            }
            Standing::Open => {}
        }
        let short: Vec<String> = upgrade
            .cost
            .parts()
            .into_iter()
            .filter(|(kind, n)| treasuries.amount(Faction::Reclaimers, *kind) < *n)
            .map(|(kind, n)| format!("{} {}", n - treasuries.amount(Faction::Reclaimers, kind), kind.name()))
            .collect();
        if short.is_empty() { Ok(()) } else { Err(format!("Not enough: {} more.", short.join(", "))) }
    }

    /// Pay for `upgrade` and own it.
    pub fn buy(&mut self, upgrade: &Upgrade, treasuries: &mut Treasuries) -> Result<String, String> {
        self.can_buy(upgrade, treasuries)?;
        for (kind, n) in upgrade.cost.parts() {
            treasuries.spend_resource(Faction::Reclaimers, kind, n);
        }
        self.bought.insert(upgrade.id.clone());
        Ok(format!("{} crafted!", upgrade.name))
    }

    fn effects(&self) -> impl Iterator<Item = Effect> + '_ {
        book().upgrades.iter().filter(|u| self.is_bought(&u.id)).map(|u| u.effect)
    }

    fn sum(&self, pick: impl Fn(Effect) -> u32) -> u32 {
        self.effects().map(pick).sum()
    }

    /// Units the hero can carry (20 + Combat units).
    pub fn hero_slots(&self) -> u32 {
        HERO_SLOTS + self.sum(|e| if let Effect::ArmySlots(n) = e { n } else { 0 })
    }

    /// Units each of your castles holds (30 + Units in castle I and II). AI castles keep 30.
    pub fn castle_slots(&self) -> u32 {
        CASTLE_SLOTS + self.sum(|e| if let Effect::CastleSlots(n) = e { n } else { 0 })
    }

    /// How long a siege on one of your places lasts.
    pub fn siege_secs(&self) -> f32 {
        SIEGE_SECS + self.effects().map(|e| if let Effect::SiegeSecs(s) = e { s } else { 0.0 }).sum::<f32>()
    }

    pub fn extra_workers(&self) -> usize {
        self.sum(|e| if let Effect::WorkerSlots(n) = e { n } else { 0 }) as usize
    }

    /// Multiplier on worker walking speed.
    pub fn worker_speed(&self) -> f32 {
        1.0 + self.sum(|e| if let Effect::WorkerBoost { speed_pct, .. } = e { speed_pct } else { 0 }) as f32 / 100.0
    }

    /// Extra load per worker trip.
    pub fn carry_bonus(&self) -> u32 {
        self.sum(|e| if let Effect::WorkerBoost { carry, .. } = e { carry } else { 0 })
    }

    /// What one unit of `kind` costs you (rounded down, never below 1).
    pub fn unit_price(&self, kind: UnitKind) -> u32 {
        let off = self.sum(|e| if let Effect::UnitPricePct(p) = e { p } else { 0 }).min(90);
        (kind.price() * (100 - off) / 100).max(1)
    }

    /// Extra loot, in % of the loser's gold, on top of the usual half.
    pub fn loot_bonus_pct(&self) -> u32 {
        self.sum(|e| if let Effect::KeepMoney { loot_pct, .. } = e { loot_pct } else { 0 })
    }

    /// Share of your gold a lost fight takes.
    pub fn loss_pct(&self) -> u32 {
        self.effects()
            .filter_map(|e| if let Effect::KeepMoney { loss_pct, .. } = e { Some(loss_pct) } else { None })
            .min()
            .unwrap_or(BASE_LOSS_PCT)
    }

    pub fn tower_income_bonus(&self) -> u32 {
        self.sum(|e| if let Effect::TowerIncome(n) = e { n } else { 0 })
    }

    pub fn castle_income_bonus(&self) -> u32 {
        self.sum(|e| if let Effect::CastleIncome(n) = e { n } else { 0 })
    }

    pub fn heal_after_fight(&self) -> bool {
        self.effects().any(|e| e == Effect::HealAfterFight)
    }

    /// Passive wood and meat: (wood, meat, every seconds), if bought.
    pub fn passive(&self) -> Option<(u32, u32, f32)> {
        self.effects().find_map(|e| match e {
            Effect::PassiveIncome { wood, meat, every_secs } => Some((wood, meat, every_secs)),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rich() -> Treasuries {
        let mut t = Treasuries::default();
        for kind in [ResourceKind::Wood, ResourceKind::Meat, ResourceKind::Gold] {
            t.add_resource(Faction::Reclaimers, kind, 10_000);
        }
        t
    }

    fn buy(u: &mut Upgrades, t: &mut Treasuries, id: &str) {
        u.buy(book().get(id).unwrap(), t).unwrap_or_else(|e| panic!("{id}: {e}"));
    }

    #[test]
    fn shipped_tree_parses() {
        let book = UpgradeBook::parse(EMBEDDED).expect("upgrades.ron must be valid");
        assert_eq!(book.upgrades.len(), 13);
        assert_eq!(book.get("basic_training").unwrap().cost, Cost(10, 10, 0));
        assert_eq!(book.get("castle_slots_2").unwrap().cost, Cost(100, 80, 200));
    }

    #[test]
    fn broken_trees_are_rejected() {
        let one = |id: &str, needs: &str| {
            format!("(id: \"{id}\", name: \"x\", tier: 1, needs: {needs}, pos: (0.0, 0.0), text: \"\", effect: None)")
        };
        let tree = |a: String, b: String| format!("(prices: {{ 1: (1, 1, 1) }}, upgrades: [{a}, {b}])");
        let unknown = tree(one("a", "None"), one("b", "Some(\"zzz\")"));
        assert!(UpgradeBook::parse(&unknown).unwrap_err().contains("zzz"));
        let two_roots = tree(one("a", "None"), one("b", "None"));
        assert!(UpgradeBook::parse(&two_roots).unwrap_err().contains("root"));
        let lp = format!("(prices: {{ 1: (1, 1, 1) }}, upgrades: [{}, {}, {}])", one("r", "None"), one("a", "Some(\"b\")"), one("b", "Some(\"a\")"));
        assert!(UpgradeBook::parse(&lp).unwrap_err().contains("loop"));
    }

    #[test]
    fn buying_needs_the_parent_and_the_price() {
        let mut u = Upgrades::default();
        let mut poor = Treasuries::default();
        let army = book().get("army_slots").unwrap();
        assert!(u.can_buy(army, &poor).unwrap_err().contains("Basic Training"));
        let root = book().get("basic_training").unwrap();
        assert!(u.can_buy(root, &poor).unwrap_err().contains("wood"));
        poor.add_resource(Faction::Reclaimers, ResourceKind::Wood, 12);
        poor.add_resource(Faction::Reclaimers, ResourceKind::Meat, 10);
        assert!(u.buy(root, &mut poor).is_ok());
        assert_eq!(poor.amount(Faction::Reclaimers, ResourceKind::Wood), 2);
        assert_eq!(poor.amount(Faction::Reclaimers, ResourceKind::Meat), 0);
        assert!(u.buy(root, &mut poor).unwrap_err().contains("already"));
        assert_eq!(u.standing(army), Standing::Open);
    }

    #[test]
    fn effects_change_the_numbers() {
        let (mut u, mut t) = (Upgrades::default(), rich());
        assert_eq!((u.hero_slots(), u.castle_slots(), u.loss_pct()), (HERO_SLOTS, CASTLE_SLOTS, BASE_LOSS_PCT));
        for id in ["basic_training", "army_slots", "castle_slots_1", "siege_time", "castle_slots_2", "gather", "pawn_speed"] {
            buy(&mut u, &mut t, id);
        }
        for id in ["unit_price", "regain_health", "keep_money", "tower_income", "castle_income", "passive_gathering"] {
            buy(&mut u, &mut t, id);
        }
        assert_eq!(u.hero_slots(), HERO_SLOTS + 5);
        assert_eq!(u.castle_slots(), CASTLE_SLOTS + 25);
        assert_eq!(u.siege_secs(), SIEGE_SECS + 20.0);
        assert_eq!(u.extra_workers(), 1);
        assert!((u.worker_speed() - 1.1).abs() < 1e-6);
        assert_eq!(u.carry_bonus(), 2);
        assert_eq!(u.unit_price(UnitKind::Warrior), UnitKind::Warrior.price() * 95 / 100);
        assert_eq!((u.loot_bonus_pct(), u.loss_pct()), (5, 45));
        assert_eq!((u.tower_income_bonus(), u.castle_income_bonus()), (5, 2));
        assert!(u.heal_after_fight());
        assert_eq!(u.passive(), Some((3, 3, 40.0)));
    }
}
