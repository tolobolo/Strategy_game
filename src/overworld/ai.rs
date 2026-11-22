//! The AI kingdoms' economy: buying units for their castles, crafting (visible) upgrades, and
//! drawing armies from the castles. Where the armies walk and what they attack is in `armies.rs`.
//! See agents.md §1.5.

use bevy::prelude::*;
use rand::Rng;

use super::armies::{AiArmy, AiArmyState};
use super::economy::Treasuries;
use super::interact::Nearby;
use super::world::WorldState;
use crate::faction::Faction;
use crate::units::{Army, CASTLE_SLOTS, HERO_SLOTS, UnitKind};

/// How often every AI kingdom goes shopping.
pub const AI_THINK_SECS: f32 = 5.0;
/// At most this many units bought per kingdom per think.
pub const AI_BUYS_PER_THINK: u32 = 3;
/// What an AI kingdom buys, with weights.
pub const AI_BUY_MIX: [(UnitKind, u32); 4] =
    [(UnitKind::Warrior, 4), (UnitKind::Archer, 3), (UnitKind::Lancer, 2), (UnitKind::Monk, 1)];
/// Units an AI castle always keeps at home.
pub const AI_MIN_GARRISON: u32 = 5;
/// An army only sets out once at least this many units are available above the reserve.
pub const AI_MIN_ARMY: u32 = 5;

/// One step of the AI kingdoms' crafting. Everything they craft shows on the map: bigger armies,
/// one more army, a fuller castle. No hidden percentages (agents.md §1.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiStep {
    /// Armies drawn from the castle may be this big.
    ArmySize(u32),
    /// One more army roams for the kingdom.
    ExtraArmy,
    /// Its castles hold this many units.
    CastleSlots(u32),
}

/// The upgrades every AI kingdom earns, one per gathering trip, in this order.
pub const AI_LADDER: [AiStep; 8] = [
    AiStep::ArmySize(25),
    AiStep::ExtraArmy,
    AiStep::CastleSlots(40),
    AiStep::ArmySize(30),
    AiStep::ExtraArmy,
    AiStep::ArmySize(35),
    AiStep::CastleSlots(50),
    AiStep::ExtraArmy,
];
/// A stuck kingdom sends this many Pawns out to gather...
pub const EXPEDITION_PAWNS: usize = 3;
/// ...each doing this many trips (min, max) before it goes home.
pub const EXPEDITION_TRIPS: (u32, u32) = (2, 4);

/// What an AI kingdom has crafted so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AiCraft {
    pub army_size: u32,
    pub castle_slots: u32,
    /// Steps of `AI_LADDER` done.
    pub level: usize,
    /// Its Pawns are out gathering; the next step comes when they are all back.
    pub expedition: bool,
}

impl Default for AiCraft {
    fn default() -> Self {
        Self { army_size: HERO_SLOTS, castle_slots: CASTLE_SLOTS, level: 0, expedition: false }
    }
}

/// A random unit kind from `AI_BUY_MIX`.
fn pick_kind(rng: &mut impl Rng) -> UnitKind {
    let total: u32 = AI_BUY_MIX.iter().map(|(_, w)| w).sum();
    let mut roll = rng.random_range(0..total);
    for (kind, weight) in AI_BUY_MIX {
        if roll < weight {
            return kind;
        }
        roll -= weight;
    }
    AI_BUY_MIX[0].0
}

/// Buy up to `AI_BUYS_PER_THINK` units into `garrison` with `faction`'s gold while there is room.
/// A kind it cannot afford ends the shopping trip. Returns how many were bought.
pub fn buy_units(garrison: &mut Army, faction: Faction, treasuries: &mut Treasuries, rng: &mut impl Rng, cap: u32) -> u32 {
    let mut bought = 0;
    while bought < AI_BUYS_PER_THINK && garrison.slots() < cap {
        let kind = pick_kind(rng);
        if !treasuries.spend(faction, kind.price()) {
            break;
        }
        garrison.add(kind, 1);
        bought += 1;
    }
    bought
}

/// Take a new army out of `garrison`: everything above `AI_MIN_GARRISON`, at most `max` (the
/// kingdom's crafted army size), or nothing if fewer than `AI_MIN_ARMY` are available.
pub fn draw_army(garrison: &mut Army, max: u32) -> Option<Army> {
    let available = garrison.slots().saturating_sub(AI_MIN_GARRISON).min(max);
    if available < AI_MIN_ARMY {
        return None;
    }
    let army = garrison.split_off_fraction(available as f32 / garrison.slots() as f32);
    (!army.is_empty()).then_some(army)
}

/// Every `AI_THINK_SECS`, each AI kingdom looks after its gathering trip (`expedition_step`) and
/// buys units for every castle it holds.
pub fn ai_kingdoms_buy(
    time: Res<Time>,
    mut timer: Local<f32>,
    mut world: ResMut<WorldState>,
    mut treasuries: ResMut<Treasuries>,
    mut nearby: ResMut<Nearby>,
) {
    *timer += time.delta_secs();
    if *timer < AI_THINK_SECS {
        return;
    }
    *timer = 0.0;
    let mut rng = rand::rng();
    for faction in Faction::KINGDOMS.into_iter().filter(|f| !f.is_player()) {
        if !world.bases.iter().any(|b| b.owner == faction) {
            continue;
        }
        if let Some(text) = expedition_step(&mut world, faction, &mut rng) {
            nearby.say_for(text, 5.0);
        }
        // An army on its way to a seller gets the gold: no castle shopping meanwhile.
        let restocking = world.armies.iter().any(|a| a.faction == faction && a.alive && matches!(a.state, AiArmyState::Restocking(_)));
        if restocking {
            continue;
        }
        let cap = world.ai_craft(faction).castle_slots;
        for base in world.bases.iter_mut().filter(|b| b.owner == faction) {
            buy_units(&mut base.garrison, faction, &mut treasuries, &mut rng, cap);
        }
    }
}

/// Stuck: most of the kingdom's armies only patrol, because everything in reach is too strong.
pub fn stuck(world: &WorldState, faction: Faction) -> bool {
    let alive: Vec<&AiArmy> = world.armies.iter().filter(|a| a.faction == faction && a.alive && !a.army.is_empty()).collect();
    let patrolling = alive.iter().filter(|a| a.state == AiArmyState::Patrolling).count();
    !alive.is_empty() && patrolling * 2 >= alive.len()
}

/// The AI kingdoms' way of growing. A **stuck** kingdom sends `EXPEDITION_PAWNS` Pawns to the
/// nearest gold mines, forests and pastures; each does 2-4 trips (the normal work animations)
/// and then goes into the castle. When they are all back, the kingdom gets its next `AI_LADDER`
/// step for free, and its armies look again at what they can take. Returns a HUD message.
pub fn expedition_step(world: &mut WorldState, faction: Faction, rng: &mut impl Rng) -> Option<String> {
    let mut done = world.ai_craft(faction);
    let out = world.workers.iter().filter(|w| w.alive && w.faction == faction).count();
    if done.expedition {
        if out > 0 {
            return None;
        }
        done.expedition = false;
        world.ai_craft.insert(faction, done);
        return craft(world, faction).map(|t| format!("The {} workers came home: {t}", faction.name()));
    }
    if done.level >= AI_LADDER.len() || !stuck(world, faction) {
        return None;
    }
    let home = world.bases.iter().find(|b| b.owner == faction)?.centre();
    let mut sites: Vec<usize> = (0..world.sites.len()).filter(|&i| !world.sites[i].is_empty()).collect();
    sites.sort_by_key(|&i| (world.sites[i].tile - home).abs().element_sum());
    let mut sent = 0;
    'sites: for site in sites {
        while world.workers_at(site) < world.sites[site].kind.max_workers() {
            if sent == EXPEDITION_PAWNS {
                break 'sites;
            }
            if !world.ai_send_worker(faction, site, rng.random_range(EXPEDITION_TRIPS.0..=EXPEDITION_TRIPS.1)) {
                break;
            }
            sent += 1;
        }
    }
    if sent == 0 {
        return None;
    }
    done.expedition = true;
    world.ai_craft.insert(faction, done);
    Some(format!("The {} kingdom sends {sent} Pawns out to gather.", faction.name()))
}

/// Give `faction` its next `AI_LADDER` step. Returns a message for the HUD.
pub fn craft(world: &mut WorldState, faction: Faction) -> Option<String> {
    let mut done = world.ai_craft(faction);
    let &step = AI_LADDER.get(done.level)?;
    done.level += 1;
    let text = match step {
        AiStep::ArmySize(n) => {
            done.army_size = n;
            format!("its armies now grow up to {n}.")
        }
        AiStep::CastleSlots(n) => {
            done.castle_slots = n;
            format!("its castles now hold {n} units.")
        }
        AiStep::ExtraArmy => {
            // A new army slot: it is raised from the castle like any army coming back.
            let home = world.bases.iter().find(|b| b.owner == faction).map(|b| world.terrain.world_pos(b.spawn_tile()));
            world.armies.push(AiArmy {
                faction,
                pos: home.unwrap_or_default(),
                army: Army::empty(faction),
                target: None,
                path: Vec::new(),
                state: AiArmyState::Walking,
                alive: false,
                respawn_in: 0.0,
                think: 0.0,
                hunted: 0.0,
                ghost: false,
            });
            "it raises another army!".to_string()
        }
    };
    world.ai_craft.insert(faction, done);
    Some(text)
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::*;

    #[test]
    fn buys_only_what_it_can_afford_and_has_room_for() {
        let mut rng = StdRng::seed_from_u64(3);
        let mut treasuries = Treasuries::default();
        let mut garrison = Army::empty(Faction::Red);
        let before = treasuries.gold(Faction::Red);
        let bought = buy_units(&mut garrison, Faction::Red, &mut treasuries, &mut rng, CASTLE_SLOTS);
        assert!(bought >= 1 && bought <= AI_BUYS_PER_THINK);
        let spent: u32 = UnitKind::ALL.iter().map(|k| garrison.count_of(*k) * k.price()).sum();
        assert_eq!(treasuries.gold(Faction::Red), before - spent);
        assert_eq!(garrison.count_of(UnitKind::Pawn), 0, "no pawns");

        let mut full = Army::new(Faction::Red, &[(UnitKind::Warrior, CASTLE_SLOTS)]);
        assert_eq!(buy_units(&mut full, Faction::Red, &mut treasuries, &mut rng, CASTLE_SLOTS), 0, "no room");
        let mut broke = Treasuries::default();
        broke.spend(Faction::Red, broke.gold(Faction::Red));
        assert_eq!(buy_units(&mut garrison, Faction::Red, &mut broke, &mut rng, CASTLE_SLOTS), 0, "no gold");
    }

    #[test]
    fn kingdoms_climb_the_upgrade_ladder() {
        let mut world = WorldState::generate(3);
        let red = Faction::Red;
        let armies = world.armies.len();
        for _ in 0..AI_LADDER.len() + 1 {
            craft(&mut world, red);
        }
        let done = world.ai_craft(red);
        assert_eq!(done.level, AI_LADDER.len(), "the whole ladder, and no further");
        assert_eq!((done.army_size, done.castle_slots), (35, 50));
        assert_eq!(world.armies.len(), armies + 3, "three more armies");
        assert!(world.armies[armies..].iter().all(|a| a.faction == red && !a.alive));
    }

    #[test]
    fn a_stuck_kingdom_sends_pawns_out_and_earns_an_upgrade() {
        let mut world = WorldState::generate(3);
        let red = Faction::Red;
        let mut rng = StdRng::seed_from_u64(1);
        assert!(expedition_step(&mut world, red, &mut rng).is_none(), "not stuck: no trip");
        for army in world.armies.iter_mut().filter(|a| a.faction == red) {
            army.state = AiArmyState::Patrolling;
        }
        assert!(expedition_step(&mut world, red, &mut rng).is_some());
        let sent: Vec<&crate::overworld::resources::Worker> = world.workers.iter().filter(|w| w.alive && w.faction == red).collect();
        assert_eq!(sent.len(), EXPEDITION_PAWNS);
        assert!(sent.iter().all(|w| (EXPEDITION_TRIPS.0..=EXPEDITION_TRIPS.1).contains(&w.trips_left.unwrap())));
        assert!(expedition_step(&mut world, red, &mut rng).is_none(), "no upgrade while they are out");
        let pawns = world.bases.iter().map(|b| b.garrison.count_of(UnitKind::Pawn)).sum::<u32>();
        let mut treasuries = Treasuries::default();
        for _ in 0..20 * 600 {
            crate::overworld::resources::step_workers(&mut world, &mut treasuries, 0.1, &mut Vec::new(), &mut Vec::new());
            if !world.workers.iter().any(|w| w.alive && w.faction == red) {
                break;
            }
        }
        assert!(!world.workers.iter().any(|w| w.alive && w.faction == red), "all came home");
        assert_eq!(world.bases.iter().map(|b| b.garrison.count_of(UnitKind::Pawn)).sum::<u32>(), pawns, "no Pawns added to castles");
        assert!(expedition_step(&mut world, red, &mut rng).unwrap().contains("25"));
        assert_eq!(world.ai_craft(red).level, 1);
    }

    #[test]
    fn armies_are_drawn_above_the_reserve() {
        let mut small = Army::new(Faction::Red, &[(UnitKind::Warrior, AI_MIN_GARRISON + AI_MIN_ARMY - 1)]);
        assert!(draw_army(&mut small, HERO_SLOTS).is_none(), "not enough yet");
        let mut garrison = Army::new(Faction::Red, &[(UnitKind::Warrior, 20), (UnitKind::Archer, 10)]);
        let army = draw_army(&mut garrison, HERO_SLOTS).expect("20 above the reserve");
        assert!((18..=20).contains(&army.slots()), "about 20: {}", army.slots());
        assert!(garrison.slots() >= AI_MIN_GARRISON - 1, "the reserve stays: {}", garrison.slots());
    }
}
