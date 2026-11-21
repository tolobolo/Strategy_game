//! Resources on the map: gold mines, forests and sheep pastures, worked by Pawns sent from your
//! castle. Pure data and logic; the sprites are in `overworld/mod.rs`, the panel in
//! `overworld/work_panel.rs`. See agents.md §1.7.

use bevy::prelude::*;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};

use super::armies::follow_path;
use super::economy::Treasuries;
use super::interact::Nearby;
use super::terrain::TerrainMap;
use super::world::{Base, Decoration, DecorationKind, WorldState};
use crate::faction::Faction;
use crate::units::UnitKind;

// ---------- balance ----------

pub const GOLD_MINE_GOLD: u32 = 300;
pub const RICH_MINE_GOLD: u32 = 600;
/// Rich mines stand at least this many tiles from every castle.
const RICH_MINES: usize = 2;
const RICH_MINE_BASE_DISTANCE: i32 = 14;
/// Every castle gets one ordinary mine this many tiles (min, max) from its centre.
const MINE_DISTANCE: (i32, i32) = (8, 12);
const FORESTS: usize = 6;
pub const FOREST_RADIUS: i32 = 2;
const FOREST_MIN_TREES: usize = 4;
const FOREST_MAX_TREES: usize = 8;
pub const TREE_WOOD: u32 = 10;
pub const TREE_REGROW_SECS: f32 = 90.0;
const PASTURES: usize = 4;
pub const PASTURE_RADIUS: i32 = 3;
const SHEEP_PER_PASTURE: usize = 4;
pub const SHEEP_MEAT: u32 = 8;
pub const SHEEP_RESPAWN_SECS: f32 = 60.0;
pub const SHEEP_SPEED: f32 = 30.0;
/// A sheep grazes this long (min, max seconds) between walks.
const SHEEP_GRAZE_SECS: (f32, f32) = (2.0, 6.0);
pub const WORKER_SPEED: f32 = 90.0;
/// How far below its tile's centre each of the four tree sheets puts the trunk's foot (trees are
/// drawn 48 px up, like decorations). A chopper stands there, beside the trunk.
const TRUNK_DROP: [f32; 4] = [65.0, 73.0, 26.0, 24.0];
/// A worker's feet are this far below its position (`UNIT_FEET`).
const FEET: f32 = 12.0;
/// Choppers and butchers stand this far to the left of the trunk or sheep, facing it.
const WORK_SIDE: f32 = 34.0;
/// A worker with nothing to do (every tree a stump, every sheep taken) looks again this often.
const WAIT_SECS: f32 = 1.0;
/// Sites keep this far (tiles) from castle centres and from towers, sellers and each other.
const SITE_BASE_DISTANCE: i32 = 6;
const SITE_MIN_DISTANCE: i32 = 5;

// ---------- data ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    Gold,
    Wood,
    Meat,
}

impl ResourceKind {
    pub fn name(self) -> &'static str {
        match self {
            ResourceKind::Gold => "gold",
            ResourceKind::Wood => "wood",
            ResourceKind::Meat => "meat",
        }
    }
}

/// The tool a worker carries at a site (it picks the Pawn sheets).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Pickaxe,
    Axe,
    Knife,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteKind {
    GoldMine,
    Forest,
    Pasture,
}

impl SiteKind {
    pub fn name(self) -> &'static str {
        match self {
            SiteKind::GoldMine => "Gold mine",
            SiteKind::Forest => "Forest",
            SiteKind::Pasture => "Sheep pasture",
        }
    }

    pub fn resource(self) -> ResourceKind {
        match self {
            SiteKind::GoldMine => ResourceKind::Gold,
            SiteKind::Forest => ResourceKind::Wood,
            SiteKind::Pasture => ResourceKind::Meat,
        }
    }

    pub fn tool(self) -> Tool {
        match self {
            SiteKind::GoldMine => Tool::Pickaxe,
            SiteKind::Forest => Tool::Axe,
            SiteKind::Pasture => Tool::Knife,
        }
    }

    pub fn max_workers(self) -> usize {
        match self {
            SiteKind::GoldMine => 3,
            SiteKind::Forest => 4,
            SiteKind::Pasture => 2,
        }
    }

    /// Seconds of mining / chopping / butchering per trip.
    pub fn work_secs(self) -> f32 {
        match self {
            SiteKind::GoldMine => 4.0,
            SiteKind::Forest => 5.0,
            SiteKind::Pasture => 3.0,
        }
    }

    /// What one worker carries home per trip.
    pub fn trip_load(self) -> u32 {
        match self {
            SiteKind::GoldMine => 5,
            SiteKind::Forest => 5,
            SiteKind::Pasture => 4,
        }
    }
}

/// One tree of a forest. `wood_left == 0` = a stump, regrowing.
#[derive(Clone, Debug)]
pub struct TreeSlot {
    pub tile: IVec2,
    /// Which of the four tree sheets (and stumps) it uses.
    pub variant: u8,
    pub wood_left: u32,
    pub regrow_in: f32,
    /// A worker is on its way to it or chopping it.
    pub claimed: bool,
}

impl TreeSlot {
    pub fn standing(&self) -> bool {
        self.wood_left > 0
    }
}

/// One sheep of a pasture. `meat_left == 0` = gone, a new one comes after `respawn_in`.
#[derive(Clone, Debug)]
pub struct Sheep {
    pub pos: Vec2,
    pub path: Vec<IVec2>,
    pub meat_left: u32,
    pub respawn_in: f32,
    /// Seconds of grazing left before it walks again.
    pub graze: f32,
    /// A worker is on its way to it or busy with it: it stands still.
    pub claimed: bool,
    /// Faces left (the sheet faces right).
    pub facing_left: bool,
}

impl Sheep {
    pub fn alive(&self) -> bool {
        self.meat_left > 0
    }
}

#[derive(Clone, Debug)]
pub struct Site {
    pub kind: SiteKind,
    /// The mine's rocks, the forest's middle or the pasture's middle.
    pub tile: IVec2,
    /// Gold mines only.
    pub gold_left: u32,
    pub gold_max: u32,
    /// Which of the six gold stone pictures a mine shows.
    pub stones: u8,
    pub trees: Vec<TreeSlot>,
    pub sheep: Vec<Sheep>,
}

impl Site {
    /// A gold mine with nothing left. Forests and pastures never run out.
    pub fn is_empty(&self) -> bool {
        self.kind == SiteKind::GoldMine && self.gold_left == 0
    }

    /// One line for labels and panels: "240 gold left", "5 of 6 trees standing", "3 of 4 sheep".
    pub fn status(&self) -> String {
        match self.kind {
            SiteKind::GoldMine if self.gold_left == 0 => "empty".to_string(),
            SiteKind::GoldMine => format!("{} gold left", self.gold_left),
            SiteKind::Forest => {
                format!("{} of {} trees standing", self.trees.iter().filter(|t| t.standing()).count(), self.trees.len())
            }
            SiteKind::Pasture => format!("{} of {} sheep", self.sheep.iter().filter(|s| s.alive()).count(), self.sheep.len()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WorkerState {
    /// Walking from the castle to the site.
    ToSite,
    /// Walking to the tree or sheep (index) it claimed.
    Approach(usize),
    /// Nothing to do yet: seconds until it looks again.
    Waiting(f32),
    /// Working for `left` more seconds, on a tree or sheep (`slot`) or at the mine (`None`).
    Working { left: f32, slot: Option<usize> },
    /// Carrying `load` home.
    ToCastle(u32),
    /// Recalled (or the mine is empty): walking home to rejoin the garrison.
    GoingHome,
}

/// A Pawn working a site for the castle it came from. It still counts towards that castle's 30.
#[derive(Clone, Debug)]
pub struct Worker {
    pub faction: Faction,
    pub site: usize,
    pub castle: usize,
    pub pos: Vec2,
    pub path: Vec<IVec2>,
    pub state: WorkerState,
    /// Goes back into the garrison at the next visit to the castle.
    pub recalled: bool,
    pub alive: bool,
}

impl Worker {
    /// Where it looks while it works (for facing): the tree, the sheep or the rocks.
    pub fn work_target(&self, site: &Site, terrain: &TerrainMap) -> Option<Vec2> {
        let WorkerState::Working { slot, .. } = self.state else { return None };
        Some(match (site.kind, slot) {
            (SiteKind::Forest, Some(i)) => terrain.world_pos(site.trees[i].tile),
            (SiteKind::Pasture, Some(i)) => site.sheep[i].pos,
            _ => terrain.world_pos(site.tile),
        })
    }
}

impl WorldState {
    /// Your Pawns out working for castle `castle`: they still take its slots.
    pub fn workers_out(&self, castle: usize) -> u32 {
        self.workers.iter().filter(|w| w.alive && w.castle == castle).count() as u32
    }

    /// How many workers `site` takes (more with the crafted "Gather resources").
    pub fn max_workers(&self, site: usize) -> usize {
        self.sites[site].kind.max_workers() + self.upgrades.extra_workers()
    }

    /// What one trip brings from a site of `kind` (more with the crafted "Strong workers").
    pub fn trip_load(&self, kind: SiteKind) -> u32 {
        kind.trip_load() + self.upgrades.carry_bonus()
    }

    /// Workers at `site` that are not on their way home for good.
    pub fn workers_at(&self, site: usize) -> usize {
        self.workers.iter().filter(|w| w.alive && w.site == site && !w.recalled).count()
    }

    /// The castle of yours nearest to `site` by walking distance: workers come from and deliver to it.
    pub fn supplying_castle(&self, site: usize) -> Option<usize> {
        let goal = self.sites[site].tile;
        self.bases
            .iter()
            .enumerate()
            .filter(|(_, b)| b.owner.is_player())
            .filter_map(|(i, b)| self.terrain.find_path(b.spawn_tile(), goal).map(|p| (p.len(), i)))
            .min()
            .map(|(_, i)| i)
    }

    /// SEND: one Pawn from the supplying castle's garrison walks out to work `site`.
    pub fn send_worker(&mut self, site: usize) -> Result<String, String> {
        let s = &self.sites[site];
        if s.is_empty() {
            return Err(format!("The {} is empty.", s.kind.name().to_lowercase()));
        }
        let max = self.max_workers(site);
        if self.workers_at(site) >= max {
            return Err(format!("Full: at most {max} workers here."));
        }
        let Some(castle) = self.supplying_castle(site) else {
            return Err("None of your castles can reach it.".to_string());
        };
        if self.bases[castle].garrison.count_of(UnitKind::Pawn) == 0 {
            return Err("No Pawns in your castle: hire some at the House and store them there.".to_string());
        }
        let gate = self.bases[castle].spawn_tile();
        let Some(path) = self.terrain.find_path(gate, work_spot(&self.sites[site], &self.terrain, self.workers_at(site)))
        else {
            return Err("None of your castles can reach it.".to_string());
        };
        self.bases[castle].garrison.remove(UnitKind::Pawn, 1);
        let worker = Worker {
            faction: Faction::Reclaimers,
            site,
            castle,
            pos: self.terrain.world_pos(gate),
            path,
            state: WorkerState::ToSite,
            recalled: false,
            alive: true,
        };
        // Reuse the slot of a worker that is gone, so entity indices stay small.
        match self.workers.iter().position(|w| !w.alive) {
            Some(i) => self.workers[i] = worker,
            None => self.workers.push(worker),
        }
        Ok(format!("A Pawn walks out to the {}.", self.sites[site].kind.name().to_lowercase()))
    }

    /// RECALL: one worker of `site` goes home and rejoins the garrison (after delivering what it carries).
    pub fn recall_worker(&mut self, site: usize) -> Result<String, String> {
        let WorldState { workers, sites, bases, terrain, .. } = self;
        let at_site = |w: &Worker| w.alive && w.site == site && !w.recalled;
        // Rather one that is not carrying anything, so no load is wasted.
        let pick = workers
            .iter()
            .position(|w| at_site(w) && !matches!(w.state, WorkerState::ToCastle(_)))
            .or_else(|| workers.iter().position(|w| at_site(w)));
        let Some(i) = pick else {
            return Err("Nobody works here.".to_string());
        };
        let worker = &mut workers[i];
        worker.recalled = true;
        if !matches!(worker.state, WorkerState::ToCastle(_)) {
            release(worker, &mut sites[site]);
            go_home(worker, bases, terrain);
        }
        Ok("A worker heads home.".to_string())
    }
}

/// Where workers stand at a site: next to the mine's rocks (spread over its sides by `n`), or the
/// middle of a forest or pasture (from there they walk to a tree or sheep).
fn work_spot(site: &Site, terrain: &TerrainMap, n: usize) -> IVec2 {
    if site.kind != SiteKind::GoldMine {
        return site.tile;
    }
    let sides = [IVec2::NEG_X, IVec2::X, IVec2::NEG_Y, IVec2::Y];
    let open: Vec<IVec2> = sides.iter().map(|d| site.tile + *d).filter(|t| terrain.is_walkable(*t)).collect();
    if open.is_empty() { site.tile } else { open[n % open.len()] }
}

/// An AI army caught worker `i`: it is gone (its Pawn too), and frees what it had claimed.
pub fn kill_worker(workers: &mut [Worker], sites: &mut [Site], i: usize) {
    let worker = &mut workers[i];
    release(worker, &mut sites[worker.site]);
    worker.alive = false;
}

/// Free whatever tree or sheep the worker had claimed.
fn release(worker: &Worker, site: &mut Site) {
    let slot = match worker.state {
        WorkerState::Approach(i) => Some(i),
        WorkerState::Working { slot, .. } => slot,
        _ => None,
    };
    if let Some(i) = slot {
        match site.kind {
            SiteKind::Forest => site.trees[i].claimed = false,
            SiteKind::Pasture => site.sheep[i].claimed = false,
            SiteKind::GoldMine => {}
        }
    }
}

fn go_home(worker: &mut Worker, bases: &[Base], terrain: &TerrainMap) {
    let gate = bases[worker.castle].spawn_tile();
    worker.path = terrain.find_path(terrain.tile_of(worker.pos), gate).unwrap_or_default();
    worker.state = WorkerState::GoingHome;
}

/// At the site: start mining, or claim the nearest free tree or sheep and walk to it, or wait.
fn start_job(worker: &mut Worker, site: &mut Site, bases: &[Base], terrain: &TerrainMap) {
    let here = terrain.tile_of(worker.pos);
    match site.kind {
        SiteKind::GoldMine if site.gold_left == 0 => go_home(worker, bases, terrain),
        SiteKind::GoldMine => worker.state = WorkerState::Working { left: site.kind.work_secs(), slot: None },
        SiteKind::Forest => {
            // Nearest free tree first; one that cannot be walked to is skipped for the next.
            let mut trees: Vec<usize> = (0..site.trees.len()).filter(|&i| site.trees[i].standing() && !site.trees[i].claimed).collect();
            trees.sort_by_key(|&i| (site.trees[i].tile - here).abs().element_sum());
            let tree = trees.into_iter().find_map(|i| terrain.find_path(here, site.trees[i].tile).map(|p| (i, p)));
            match tree {
                Some((i, path)) => {
                    site.trees[i].claimed = true;
                    worker.path = path;
                    worker.state = WorkerState::Approach(i);
                }
                None => worker.state = WorkerState::Waiting(WAIT_SECS),
            }
        }
        SiteKind::Pasture => {
            // Nearest free sheep first; one that cannot be walked to is skipped for the next.
            let mut sheep: Vec<usize> = (0..site.sheep.len()).filter(|&i| site.sheep[i].alive() && !site.sheep[i].claimed).collect();
            sheep.sort_by(|&a, &b| site.sheep[a].pos.distance(worker.pos).total_cmp(&site.sheep[b].pos.distance(worker.pos)));
            let sheep = sheep.into_iter().find_map(|i| terrain.find_path(here, terrain.tile_of(site.sheep[i].pos)).map(|p| (i, p)));
            match sheep {
                Some((i, path)) => {
                    let s = &mut site.sheep[i];
                    s.claimed = true;
                    s.path.clear(); // it stops where it is
                    worker.path = path;
                    worker.state = WorkerState::Approach(i);
                }
                None => worker.state = WorkerState::Waiting(WAIT_SECS),
            }
        }
    }
}

/// Take one trip's load from the site. Returns how much (0 if nothing was left).
fn harvest(site: &mut Site, slot: Option<usize>, load: u32) -> u32 {
    match (site.kind, slot) {
        (SiteKind::GoldMine, _) => {
            let n = load.min(site.gold_left);
            site.gold_left -= n;
            n
        }
        (SiteKind::Forest, Some(i)) => {
            let tree = &mut site.trees[i];
            let n = load.min(tree.wood_left);
            tree.wood_left -= n;
            if tree.wood_left == 0 {
                tree.regrow_in = TREE_REGROW_SECS;
            }
            n
        }
        (SiteKind::Pasture, Some(i)) => {
            let sheep = &mut site.sheep[i];
            let n = load.min(sheep.meat_left);
            sheep.meat_left -= n;
            if sheep.meat_left == 0 {
                sheep.respawn_in = SHEEP_RESPAWN_SECS;
                sheep.path.clear();
            }
            n
        }
        _ => 0,
    }
}

/// What a delivery at a castle gate looked like, for the little pop-up.
#[derive(Clone, Copy, Debug)]
pub struct Delivery {
    pub pos: Vec2,
    pub kind: ResourceKind,
}

/// Move every worker one step: walk, work, carry home, deliver, go again. Messages for the HUD go
/// into `messages`, deliveries into `deliveries`.
pub fn step_workers(
    world: &mut WorldState,
    treasuries: &mut Treasuries,
    dt: f32,
    messages: &mut Vec<String>,
    deliveries: &mut Vec<Delivery>,
) {
    let speed = WORKER_SPEED * world.upgrades.worker_speed();
    let carry = world.upgrades.carry_bonus();
    let WorldState { workers, sites, bases, terrain, .. } = world;
    for w in workers.iter_mut().filter(|w| w.alive) {
        let site = &mut sites[w.site];
        // Lost the castle: its workers are lost with it.
        if bases[w.castle].owner != w.faction {
            release(w, site);
            w.alive = false;
            continue;
        }
        match w.state {
            WorkerState::ToSite => {
                if follow_path(&mut w.pos, &mut w.path, terrain, speed, dt) {
                    start_job(w, site, bases, terrain);
                }
            }
            WorkerState::Approach(i) => {
                let gone = site.kind == SiteKind::Pasture && !site.sheep[i].alive();
                if gone {
                    release(w, site);
                    start_job(w, site, bases, terrain);
                } else if follow_path(&mut w.pos, &mut w.path, terrain, speed, dt) {
                    // Step up to the trunk or the sheep, but only onto ground it can walk on, on
                    // the same layer, so it can always walk on from there.
                    let step = match site.kind {
                        SiteKind::Forest => {
                            let tree = &site.trees[i];
                            let drop = TRUNK_DROP[tree.variant as usize];
                            terrain.world_pos(tree.tile) + Vec2::new(-WORK_SIDE, FEET - drop)
                        }
                        _ => site.sheep[i].pos + Vec2::new(-WORK_SIDE, 0.0),
                    };
                    let (from, to) = (terrain.tile_of(w.pos), terrain.tile_of(step));
                    if from == to || (terrain.is_walkable(to) && terrain.layer(to) == terrain.layer(from)) {
                        w.pos = step;
                    }
                    w.state = WorkerState::Working { left: site.kind.work_secs(), slot: Some(i) };
                }
            }
            WorkerState::Waiting(left) => {
                let left = left - dt;
                if left > 0.0 {
                    w.state = WorkerState::Waiting(left);
                } else {
                    start_job(w, site, bases, terrain);
                }
            }
            WorkerState::Working { left, slot } => {
                let left = left - dt;
                if left > 0.0 {
                    w.state = WorkerState::Working { left, slot };
                    continue;
                }
                let load = harvest(site, slot, site.kind.trip_load() + carry);
                release(w, site);
                if site.kind == SiteKind::GoldMine && site.gold_left == 0 && load > 0 {
                    messages.push("A gold mine is empty: its workers come home.".to_string());
                }
                if load == 0 {
                    start_job(w, site, bases, terrain);
                } else {
                    let gate = bases[w.castle].spawn_tile();
                    w.path = terrain.find_path(terrain.tile_of(w.pos), gate).unwrap_or_default();
                    w.state = WorkerState::ToCastle(load);
                }
            }
            WorkerState::ToCastle(load) => {
                if !follow_path(&mut w.pos, &mut w.path, terrain, speed, dt) {
                    continue;
                }
                let kind = site.kind.resource();
                treasuries.add_resource(w.faction, kind, load);
                deliveries.push(Delivery { pos: w.pos, kind });
                if w.recalled || site.is_empty() {
                    bases[w.castle].garrison.add(UnitKind::Pawn, 1);
                    w.alive = false;
                } else {
                    w.path =
                        terrain.find_path(terrain.tile_of(w.pos), work_spot(site, terrain, 0)).unwrap_or_default();
                    w.state = WorkerState::ToSite;
                }
            }
            WorkerState::GoingHome => {
                if follow_path(&mut w.pos, &mut w.path, terrain, speed, dt) {
                    bases[w.castle].garrison.add(UnitKind::Pawn, 1);
                    w.alive = false;
                }
            }
        }
    }
}

/// Trees regrow, sheep come back and wander inside their pasture, grazing in between.
pub fn step_sites(world: &mut WorldState, dt: f32, rng: &mut impl Rng) {
    let WorldState { sites, terrain, .. } = world;
    for site in sites.iter_mut() {
        for tree in site.trees.iter_mut().filter(|t| !t.standing()) {
            tree.regrow_in -= dt;
            if tree.regrow_in <= 0.0 {
                tree.wood_left = TREE_WOOD;
            }
        }
        let centre = site.tile;
        for sheep in site.sheep.iter_mut() {
            if !sheep.alive() {
                sheep.respawn_in -= dt;
                if sheep.respawn_in <= 0.0 {
                    sheep.meat_left = SHEEP_MEAT;
                    sheep.pos = terrain.world_pos(centre);
                    sheep.graze = SHEEP_GRAZE_SECS.0;
                }
                continue;
            }
            if sheep.claimed {
                continue;
            }
            if sheep.path.is_empty() {
                sheep.graze -= dt;
                if sheep.graze <= 0.0 {
                    let goal = centre
                        + IVec2::new(
                            rng.random_range(-PASTURE_RADIUS..=PASTURE_RADIUS),
                            rng.random_range(-PASTURE_RADIUS..=PASTURE_RADIUS),
                        );
                    sheep.path = terrain.find_path(terrain.tile_of(sheep.pos), goal).unwrap_or_default();
                    // Never wander far: a long way round (e.g. past a cliff) is skipped.
                    if sheep.path.len() > (PASTURE_RADIUS * 3) as usize {
                        sheep.path.clear();
                    }
                    sheep.graze = rng.random_range(SHEEP_GRAZE_SECS.0..SHEEP_GRAZE_SECS.1);
                }
            } else {
                let before = sheep.pos;
                follow_path(&mut sheep.pos, &mut sheep.path, terrain, SHEEP_SPEED, dt);
                let dx = sheep.pos.x - before.x;
                if dx.abs() > 0.01 {
                    sheep.facing_left = dx < 0.0;
                }
            }
        }
    }
}

/// Deliveries waiting for their pop-up sprite (`overworld/mod.rs` shows and clears them).
#[derive(Resource, Default)]
pub struct PendingDeliveries(pub Vec<Delivery>);

/// The overworld system: sites and workers, one step per frame.
pub fn resources_act(
    time: Res<Time>,
    mut world: ResMut<WorldState>,
    mut treasuries: ResMut<Treasuries>,
    mut nearby: ResMut<Nearby>,
    mut pops: ResMut<PendingDeliveries>,
) {
    let dt = time.delta_secs();
    let mut rng = rand::rng();
    step_sites(&mut world, dt, &mut rng);
    let mut messages = Vec::new();
    step_workers(&mut world, &mut treasuries, dt, &mut messages, &mut pops.0);
    for message in messages {
        nearby.say_for(message, 5.0);
    }
}

// ---------- world generation ----------

fn chebyshev(a: IVec2, b: IVec2) -> i32 {
    (a - b).abs().max_element()
}

/// Place gold mines, forests and pastures. Uses its own random stream (from `seed`), so the rest
/// of the world (towers, armies, bandits) is the same as before sites existed. Forest trees are
/// taken out of `decorations` (they become the site's trees); decorations on mines and pastures
/// are cleared.
pub fn place_sites(
    terrain: &TerrainMap,
    bases: &[Base],
    taken: &mut Vec<IVec2>,
    decorations: &mut Vec<Decoration>,
    seed: u64,
) -> Vec<Site> {
    let mut rng = StdRng::seed_from_u64(seed ^ 0x5EED_0F_5175);
    let home = bases[0].spawn_tile();
    let ok = |tile: IVec2, taken: &[IVec2]| {
        terrain.is_walkable(tile)
            && !terrain.is_ramp(tile)
            && !terrain.is_ramp(tile + IVec2::Y)
            && !terrain.is_ramp_entry(tile)
            && !terrain.is_edge_art(tile)
            && bases.iter().all(|b| !b.contains(tile) && chebyshev(b.centre(), tile) >= SITE_BASE_DISTANCE)
            // Spacing from towers, sellers and other sites; ramps are in `taken` too, but the
            // ramp checks above already keep sites off them and their entries.
            && taken.iter().filter(|t| !terrain.is_ramp(**t)).all(|t| chebyshev(*t, tile) >= SITE_MIN_DISTANCE)
    };
    let reachable = |from: IVec2, tile: IVec2| terrain.find_path(from, tile).is_some();
    let all_tiles: Vec<IVec2> = terrain.tiles().map(|(t, _)| t).collect();
    let mut sites = Vec::new();

    // Gold mines: one near every castle, then the rich ones far from all of them.
    let mine = |tile: IVec2, gold: u32, rng: &mut StdRng| Site {
        kind: SiteKind::GoldMine,
        tile,
        gold_left: gold,
        gold_max: gold,
        stones: rng.random_range(0..6),
        trees: Vec::new(),
        sheep: Vec::new(),
    };
    let has_side = |tile: IVec2| [IVec2::NEG_X, IVec2::X].iter().any(|d| terrain.is_walkable(tile + *d));
    for base in bases {
        let mut candidates: Vec<IVec2> = all_tiles
            .iter()
            .copied()
            .filter(|t| (MINE_DISTANCE.0..=MINE_DISTANCE.1).contains(&chebyshev(base.centre(), *t)))
            .filter(|t| ok(*t, taken) && has_side(*t))
            .collect();
        candidates.shuffle(&mut rng);
        if let Some(tile) = candidates.into_iter().find(|t| reachable(base.spawn_tile(), *t)) {
            taken.push(tile);
            sites.push(mine(tile, GOLD_MINE_GOLD, &mut rng));
        }
    }
    let mut far: Vec<IVec2> = all_tiles
        .iter()
        .copied()
        .filter(|t| bases.iter().all(|b| chebyshev(b.centre(), *t) >= RICH_MINE_BASE_DISTANCE))
        .collect();
    far.shuffle(&mut rng);
    for tile in far {
        if sites.iter().filter(|s: &&Site| s.gold_max == RICH_MINE_GOLD).count() == RICH_MINES {
            break;
        }
        if ok(tile, taken) && has_side(tile) && reachable(home, tile) {
            taken.push(tile);
            sites.push(mine(tile, RICH_MINE_GOLD, &mut rng));
        }
    }

    // Forests: where the trees are densest. Their trees become the site's.
    let is_tree = |d: &Decoration| matches!(d.kind, DecorationKind::Tree(_));
    let trees_near = |decorations: &[Decoration], centre: IVec2| {
        decorations.iter().filter(|d| is_tree(d) && chebyshev(d.tile, centre) <= FOREST_RADIUS).count()
    };
    let mut centres: Vec<(usize, IVec2)> =
        decorations.iter().filter(|d| is_tree(d)).map(|d| (trees_near(decorations, d.tile), d.tile)).collect();
    centres.sort_by(|a, b| b.0.cmp(&a.0));
    // Not enough tree clumps on this map: forests are also planted on open ground.
    let mut open_ground = all_tiles.clone();
    open_ground.shuffle(&mut rng);
    let candidates: Vec<IVec2> = centres.into_iter().map(|(_, t)| t).chain(open_ground).collect();
    let plantable = |tile: IVec2| {
        terrain.is_walkable(tile)
            && !terrain.is_edge_art(tile)
            && !terrain.is_ramp(tile)
            && !terrain.is_ramp(tile + IVec2::Y)
            && !terrain.is_ramp_entry(tile)
            && bases.iter().all(|b| !b.contains(tile))
    };
    let around = |centre: IVec2| {
        (-FOREST_RADIUS..=FOREST_RADIUS).flat_map(move |dx| (-FOREST_RADIUS..=FOREST_RADIUS).map(move |dy| centre + IVec2::new(dx, dy)))
    };
    for centre in candidates {
        if sites.iter().filter(|s: &&Site| s.kind == SiteKind::Forest).count() == FORESTS {
            break;
        }
        if !ok(centre, taken) || around(centre).filter(|t| plantable(*t)).count() < FOREST_MIN_TREES {
            continue;
        }
        if !reachable(home, centre) {
            continue;
        }
        let mut trees: Vec<TreeSlot> = Vec::new();
        decorations.retain(|d| {
            let mine = is_tree(d) && chebyshev(d.tile, centre) <= FOREST_RADIUS && trees.len() < FOREST_MAX_TREES;
            if let (true, DecorationKind::Tree(v)) = (mine, d.kind) {
                trees.push(TreeSlot { tile: d.tile, variant: v, wood_left: TREE_WOOD, regrow_in: 0.0, claimed: false });
            }
            !mine
        });
        // Too sparse: plant more on free walkable tiles around the middle.
        let mut spots: Vec<IVec2> =
            around(centre).filter(|t| plantable(*t)).filter(|t| trees.iter().all(|tree| tree.tile != *t)).collect();
        spots.shuffle(&mut rng);
        for tile in spots {
            if trees.len() >= FOREST_MIN_TREES {
                break;
            }
            decorations.retain(|d| d.tile != tile);
            trees.push(TreeSlot { tile, variant: rng.random_range(0..4), wood_left: TREE_WOOD, regrow_in: 0.0, claimed: false });
        }
        taken.push(centre);
        sites.push(Site { kind: SiteKind::Forest, tile: centre, gold_left: 0, gold_max: 0, stones: 0, trees, sheep: Vec::new() });
    }

    // Sheep pastures: open grass with room to wander.
    let roomy = |tile: IVec2| {
        let open = (-PASTURE_RADIUS..=PASTURE_RADIUS)
            .flat_map(|dx| (-PASTURE_RADIUS..=PASTURE_RADIUS).map(move |dy| tile + IVec2::new(dx, dy)))
            .filter(|t| terrain.is_walkable(*t) && terrain.layer(*t) == terrain.layer(tile))
            .count();
        open >= 30
    };
    let mut open: Vec<IVec2> = all_tiles.clone();
    open.shuffle(&mut rng);
    for tile in open {
        if sites.iter().filter(|s: &&Site| s.kind == SiteKind::Pasture).count() == PASTURES {
            break;
        }
        if !ok(tile, taken) || !roomy(tile) || !reachable(home, tile) {
            continue;
        }
        decorations.retain(|d| chebyshev(d.tile, tile) > 1 && !(is_tree(d) && chebyshev(d.tile, tile) <= PASTURE_RADIUS));
        let sheep = (0..SHEEP_PER_PASTURE)
            .map(|_| {
                let spot = tile + IVec2::new(rng.random_range(-1..=1), rng.random_range(-1..=1));
                let spot = if terrain.is_walkable(spot) { spot } else { tile };
                Sheep {
                    pos: terrain.world_pos(spot),
                    path: Vec::new(),
                    meat_left: SHEEP_MEAT,
                    respawn_in: 0.0,
                    graze: rng.random_range(SHEEP_GRAZE_SECS.0..SHEEP_GRAZE_SECS.1),
                    claimed: false,
                    facing_left: rng.random(),
                }
            })
            .collect();
        taken.push(tile);
        sites.push(Site { kind: SiteKind::Pasture, tile, gold_left: 0, gold_max: 0, stones: 0, trees: Vec::new(), sheep });
    }

    // Nothing else stands on a mine's rocks.
    for site in sites.iter().filter(|s| s.kind == SiteKind::GoldMine) {
        decorations.retain(|d| chebyshev(d.tile, site.tile) > 1);
    }
    sites
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(world: &WorldState, kind: SiteKind) -> usize {
        world.sites.iter().filter(|s| s.kind == kind).count()
    }

    #[test]
    fn every_world_has_reachable_sites() {
        for seed in [1, 2, 3, 99] {
            let world = WorldState::generate(seed);
            assert!(count(&world, SiteKind::GoldMine) >= world.bases.len(), "seed {seed}: a mine per castle");
            assert!(count(&world, SiteKind::Forest) >= 3, "seed {seed}: forests");
            assert!(count(&world, SiteKind::Pasture) >= 2, "seed {seed}: pastures");
            let home = world.home_base().spawn_tile();
            for (i, site) in world.sites.iter().enumerate() {
                if site.kind != SiteKind::GoldMine || site.gold_max == RICH_MINE_GOLD {
                    assert!(world.terrain.find_path(home, site.tile).is_some(), "seed {seed}: site {i} unreachable");
                }
                if site.kind == SiteKind::Forest {
                    assert!(site.trees.len() >= FOREST_MIN_TREES, "seed {seed}: forest {i} too small");
                }
            }
        }
    }

    /// A world where the player's castle has `pawns` Pawns, and the index of the site of `kind`
    /// nearest to it.
    fn setup(kind: SiteKind, pawns: u32) -> (WorldState, Treasuries, usize) {
        let mut world = WorldState::generate(3);
        world.armies.clear();
        world.bandits.clear();
        world.bases[0].garrison.add(UnitKind::Pawn, pawns);
        let home = world.home_base().spawn_tile();
        let site = (0..world.sites.len())
            .filter(|i| world.sites[*i].kind == kind)
            .filter_map(|i| world.terrain.find_path(home, world.sites[i].tile).map(|p| (p.len(), i)))
            .min()
            .unwrap()
            .1;
        (world, Treasuries::default(), site)
    }

    fn run(world: &mut WorldState, treasuries: &mut Treasuries, secs: f32) -> Vec<String> {
        let mut rng = StdRng::seed_from_u64(7);
        let (mut messages, mut deliveries) = (Vec::new(), Vec::new());
        for _ in 0..(secs * 10.0) as usize {
            step_sites(world, 0.1, &mut rng);
            step_workers(world, treasuries, 0.1, &mut messages, &mut deliveries);
        }
        messages
    }

    #[test]
    fn miners_bring_gold_home_until_the_mine_is_empty() {
        let (mut world, mut treasuries, site) = setup(SiteKind::GoldMine, 3);
        let pawns = world.bases[0].garrison.count_of(UnitKind::Pawn);
        world.sites[site].gold_left = 20;
        assert!(world.send_worker(site).is_ok());
        assert!(world.send_worker(site).is_ok());
        assert_eq!(world.bases[0].garrison.count_of(UnitKind::Pawn), pawns - 2);
        assert_eq!(world.workers_out(0), 2);
        let gold = treasuries.player_gold();
        let messages = run(&mut world, &mut treasuries, 300.0);
        assert_eq!(world.sites[site].gold_left, 0);
        assert_eq!(treasuries.player_gold(), gold + 20, "every nugget arrives");
        assert!(messages.iter().any(|m| m.contains("empty")));
        assert_eq!(world.workers_out(0), 0, "everyone came home");
        assert_eq!(world.bases[0].garrison.count_of(UnitKind::Pawn), pawns);
        assert!(world.send_worker(site).is_err(), "an empty mine takes nobody");
    }

    #[test]
    fn choppers_turn_trees_into_stumps_that_regrow() {
        let (mut world, mut treasuries, site) = setup(SiteKind::Forest, 1);
        assert!(world.send_worker(site).is_ok());
        run(&mut world, &mut treasuries, 200.0);
        assert!(treasuries.amount(Faction::Reclaimers, ResourceKind::Wood) >= TREE_WOOD);
        assert!(world.sites[site].trees.iter().any(|t| !t.standing()), "a tree was cut down");
        assert!(world.recall_worker(site).is_ok());
        run(&mut world, &mut treasuries, TREE_REGROW_SECS + 30.0);
        assert!(world.sites[site].trees.iter().all(|t| t.standing()), "stumps grow back");
        assert_eq!(world.workers_out(0), 0, "the recalled worker is back in the castle");
    }

    #[test]
    fn sheep_wander_get_taken_and_come_back() {
        let (mut world, mut treasuries, site) = setup(SiteKind::Pasture, 1);
        let start: Vec<Vec2> = world.sites[site].sheep.iter().map(|s| s.pos).collect();
        assert!(world.send_worker(site).is_ok());
        run(&mut world, &mut treasuries, 400.0);
        assert!(treasuries.amount(Faction::Reclaimers, ResourceKind::Meat) >= SHEEP_MEAT);
        let moved = world.sites[site].sheep.iter().zip(&start).any(|(s, p)| s.pos != *p);
        assert!(moved, "sheep walk around");
        let centre = world.terrain.world_pos(world.sites[site].tile);
        for sheep in &world.sites[site].sheep {
            assert!(sheep.pos.distance(centre) <= (PASTURE_RADIUS as f32 + 1.5) * super::super::terrain::TILE);
        }
    }

    #[test]
    fn send_needs_pawns_and_respects_the_limit() {
        let (mut world, _, site) = setup(SiteKind::Pasture, 0);
        world.bases[0].garrison.remove(UnitKind::Pawn, 1000);
        assert!(world.send_worker(site).unwrap_err().contains("No Pawns"));
        world.bases[0].garrison.add(UnitKind::Pawn, 5);
        for _ in 0..world.max_workers(site) {
            assert!(world.send_worker(site).is_ok());
        }
        assert!(world.send_worker(site).unwrap_err().contains("Full"));
        assert!(world.recall_worker(site).is_ok());
        assert_eq!(world.workers_at(site), world.max_workers(site) - 1);
    }
}
