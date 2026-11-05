//! Python bindings for vgcsim.
//!
//!     import vgcsim
//!     dex = vgcsim.Dex("data/dex.json")
//!     b = vgcsim.Battle(dex, team_text_a, team_text_b, seed=1)
//!     b.choose_team(0, [0, 1, 2, 3]); b.choose_team(1, [0, 1, 2, 3])
//!     while b.phase != "ended":
//!         for side in (0, 1):
//!             if b.needs_choice(side):
//!                 opts = b.legal_actions(side)          # one list per active slot
//!                 b.choose(side, opts[0][0], opts[1][0])
//!
//! Actions are tuples: ("move", slot 0-3 (4 = Struggle), target, mega), ("switch", index, 0, False),
//! ("pass", 0, 0, False). Targets follow Showdown: 1, 2 = opposing slots; -1, -2 = own slots; 0 = none.

use std::sync::Arc;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use ::vgcsim::battle::{Action, Battle as Engine, Phase};
use ::vgcsim::damage::{Status, Terrain, Weather};
use ::vgcsim::dex::{Dex as EngineDex, BOOST_IDS, STAT_IDS};
use ::vgcsim::policy;
use ::vgcsim::state::{parse_team, vol, vol2, Mon, Rng, NONE};

mod exact;

pub(crate) type ActionTuple = (String, i64, i64, bool);

fn to_tuple(a: Action) -> ActionTuple {
    match a {
        Action::Move { slot, target, mega } => ("move".into(), slot as i64, target as i64, mega),
        Action::Switch { to } => ("switch".into(), to as i64, 0, false),
        Action::Pass => ("pass".into(), 0, 0, false),
    }
}

fn from_tuple(t: &ActionTuple) -> PyResult<Action> {
    Ok(match t.0.as_str() {
        "move" => Action::Move { slot: t.1 as u8, target: t.2 as i8, mega: t.3 },
        "switch" => Action::Switch { to: t.1 as u8 },
        "pass" => Action::Pass,
        other => return Err(PyValueError::new_err(format!("unknown action kind {other}"))),
    })
}

fn status_name(s: Status) -> &'static str {
    match s {
        Status::None => "",
        Status::Burn => "brn",
        Status::Paralysis => "par",
        Status::Poison => "psn",
        Status::Toxic => "tox",
        Status::Sleep => "slp",
        Status::Freeze => "frz",
    }
}

fn parse_status(s: &str) -> PyResult<Status> {
    Ok(match s {
        "" => Status::None,
        "brn" => Status::Burn,
        "par" => Status::Paralysis,
        "psn" => Status::Poison,
        "tox" => Status::Toxic,
        "slp" => Status::Sleep,
        "frz" => Status::Freeze,
        _ => return Err(PyValueError::new_err(format!("unknown status {s}"))),
    })
}

fn weather_name(w: Weather) -> &'static str {
    match w {
        Weather::None => "",
        Weather::Rain => "raindance",
        Weather::Sun => "sunnyday",
        Weather::Sand => "sandstorm",
        Weather::Snow => "snowscape",
    }
}

fn parse_weather(s: &str) -> PyResult<Weather> {
    Ok(match s {
        "" | "none" => Weather::None,
        "raindance" | "rain" => Weather::Rain,
        "sunnyday" | "sun" => Weather::Sun,
        "sandstorm" | "sand" => Weather::Sand,
        "snowscape" | "snow" | "hail" => Weather::Snow,
        _ => return Err(PyValueError::new_err(format!("unknown weather {s}"))),
    })
}

fn terrain_name(t: Terrain) -> &'static str {
    match t {
        Terrain::None => "",
        Terrain::Electric => "electricterrain",
        Terrain::Grassy => "grassyterrain",
        Terrain::Psychic => "psychicterrain",
        Terrain::Misty => "mistyterrain",
    }
}

fn parse_terrain(s: &str) -> PyResult<Terrain> {
    Ok(match s {
        "" | "none" => Terrain::None,
        "electricterrain" | "electric" => Terrain::Electric,
        "grassyterrain" | "grassy" => Terrain::Grassy,
        "psychicterrain" | "psychic" => Terrain::Psychic,
        "mistyterrain" | "misty" => Terrain::Misty,
        _ => return Err(PyValueError::new_err(format!("unknown terrain {s}"))),
    })
}

// Every per-Pokemon volatile counter / flag of the fast engine, by name, in the engine's own convention (raw field
// values): Battle.state() reports the nonzero ones under "vols", Battle.set_vols() writes them, so positions rebuilt
// from a real game (or from Showdown's serialized battle) can carry them.
macro_rules! vol_fields {
    ($m:ident, $f:ident) => {
        $f!($m, "confusion", confusion); $f!($m, "taunt", taunt); $f!($m, "encore", encore); $f!($m, "encore_slot", encore_slot);
        $f!($m, "throatchop", throat_chop); $f!($m, "magnetrise", magnet_rise); $f!($m, "disable", disable);
        $f!($m, "disable_slot", disable_slot); $f!($m, "perish", perish); $f!($m, "yawn", yawn); $f!($m, "substitute", sub_hp);
        $f!($m, "healblock", heal_block); $f!($m, "stockpile", stockpile); $f!($m, "syrupbomb", syrup);
        $f!($m, "syrup_src", syrup_src); $f!($m, "syrup_side", syrup_side); $f!($m, "partiallytrapped", bound);
        $f!($m, "bound_div", bound_div); $f!($m, "bound_src", bound_src); $f!($m, "bound_side", bound_side);
        $f!($m, "octolock_src", octolock_src); $f!($m, "octolock_side", octolock_side); $f!($m, "trap_src", trap_src);
        $f!($m, "trap_side", trap_side); $f!($m, "leech_slot", leech_slot); $f!($m, "leech_side", leech_side);
        $f!($m, "attract_src", attract_src); $f!($m, "attract_side", attract_side); $f!($m, "lock_slot", lock_slot);
        $f!($m, "lockedmove", lock_dur); $f!($m, "lock_true", lock_true); $f!($m, "charge_slot", charge_slot); $f!($m, "charge_target", charge_target);
        $f!($m, "uproar", uproar); $f!($m, "uproar_slot", uproar_slot); $f!($m, "lockon", lock_on);
        $f!($m, "status_turns", status_turns); $f!($m, "stall", protect_streak); $f!($m, "active_turns", active_turns);
        $f!($m, "allyswitch", ally_sw_dur); $f!($m, "allyswitch_counter", ally_sw);
        $f!($m, "usedmask", used_mask); $f!($m, "timesattacked", times_attacked); $f!($m, "fallen", fallen);
        $f!($m, "last_target", last_target);       // Showdown lastMoveTargetLoc: Instruct
    };
}
const VOL_BITS: &[(&str, u32)] = &[
    ("flashfire", vol::FLASH_FIRE), ("charge", vol::CHARGED), ("focusenergy", vol::FOCUS_ENERGY), ("dragoncheer", vol::DRAGON_CHEER),
    ("dragoncheer2", vol::DRAGON_CHEER2), ("destinybond", vol::DESTINY_BOND), ("minimize", vol::MINIMIZE), ("curse", vol::CURSE),
    ("noretreat", vol::NO_RETREAT), ("glaiverush", vol::GLAIVE_RUSH), ("mustrecharge", vol::MUST_RECHARGE), ("imprison", vol::IMPRISON),
    ("leechseed", vol::LEECH_SEED), ("twoturnmove", vol::CHARGING), ("transform", vol::TRANSFORMED), ("roost", vol::ROOST),
    ("libero", vol::LIBERO_USED),
    // this-turn flags (Showdown keeps the lead switch-ins' ones through turn 1): Lash Out, Burning
    // Jealousy, Alluring Voice, Assurance, Stomping-style checks
    ("statsraised", vol::STATS_RAISED), ("statslowered", vol::STATS_LOWERED), ("hurtthisturn", vol::DAMAGED_THIS_TURN),
];
const VOL2_BITS: &[(&str, u32)] = &[
    ("attract", vol2::ATTRACT), ("aquaring", vol2::AQUA_RING), ("saltcure", vol2::SALT_CURE), ("electrify", vol2::ELECTRIFY),
    ("powertrick", vol2::POWER_TRICK), ("smackdown", vol2::SMACK_DOWN), ("torment", vol2::TORMENT), ("ingrain", vol2::INGRAIN),
    ("gastroacid", vol2::GASTRO), ("split", vol2::SPLIT),
];
fn vols_of(m: &Mon) -> Vec<(&'static str, i64)> {
    let mut out = Vec::new();
    macro_rules! get { ($m:ident, $n:expr, $x:ident) => { let v = $m.$x as i64; if v != 0 && !(v == NONE as i64 && $n.contains('_')) { out.push(($n, v)); } }; }
    vol_fields!(m, get);
    // Metronome (item): "metronome" = consecutive count + 1 (0 = no volatile), "metronome_slot" = the
    // move slot of its last move (255 = none)
    if m.metro_on {
        out.push(("metronome", m.metro_n as i64 + 1));
        let k = (0..4).find(|&k| m.moves[k] == m.metro_move && m.metro_move != u16::MAX).map(|k| k as i64).unwrap_or(255);
        if k != 255 { out.push(("metronome_slot", k)); }
    }
    // a lock / Uproar / charge that a caller (Copycat) started: the move it runs (dex index)
    if m.call_move != u16::MAX && (m.lock_slot == ::vgcsim::battle::CALL_SLOT || m.charge_slot == ::vgcsim::battle::CALL_SLOT
        || (m.uproar > 0 && m.uproar_slot == ::vgcsim::battle::CALL_SLOT)) {
        out.push(("call_move", m.call_move as i64));
    }
    if m.used_item_turn { out.push(("useditem", 1)); }
    if m.illusion { out.push(("illusion", 1)); out.push(("illusion_as", m.illusion_as as i64)); }   // disguised
    if m.ate_berry { out.push(("ateberry", 1)); }
    if m.syrup_done { out.push(("syrupdone", 1)); }       // Supersweet Syrup used (once per battle)          // Belch: ate a berry this battle (kept across switches)
    for (n, b) in VOL_BITS { if m.vol & b != 0 { out.push((n, 1)); } }
    for (n, b) in VOL2_BITS { if m.vol2 & b != 0 { out.push((n, 1)); } }
    out
}
fn set_vol(m: &mut Mon, name: &str, v: i64) -> bool {
    macro_rules! put { ($m:ident, $n:expr, $x:ident) => { if name == $n { $m.$x = v as _; return true; } }; }
    vol_fields!(m, put);
    if name == "metronome" {
        m.metro_on = v != 0;
        m.metro_n = if v > 0 { (v - 1).min(255) as u8 } else { 0 };
        if v == 0 { m.metro_move = u16::MAX; }
        return true;
    }
    if name == "call_move" {
        m.call_move = if v >= 0 && v < u16::MAX as i64 { v as u16 } else { u16::MAX };
        return true;
    }
    // Illusion disguise: from_brought turns it on for every lead Zoroark; set it to the
    // real state (a revealed Zoroark is not disguised: Transform / Imposter work on it). illusion_as = party index imitated.
    if name == "illusion" {
        m.illusion = v != 0;
        if v == 0 { m.illusion_as = ::vgcsim::state::NONE; }
        return true;
    }
    if name == "illusion_as" {
        m.illusion_as = if (0..6).contains(&v) { v as u8 } else { ::vgcsim::state::NONE };
        return true;
    }
    if name == "syrupdone" {
        m.syrup_done = v != 0;
        return true;
    }
    if name == "ateberry" {
        m.ate_berry = v != 0;
        return true;
    }
    if name == "useditem" {
        m.used_item_turn = v != 0;                            // Pickup
        return true;
    }
    if name == "metronome_slot" {
        m.metro_move = if (0..4).contains(&v) { m.moves[v as usize] } else { u16::MAX };
        return true;
    }
    for (n, b) in VOL_BITS { if name == *n { if v != 0 { m.vol |= b; } else { m.vol &= !b; } return true; } }
    for (n, b) in VOL2_BITS { if name == *n { if v != 0 { m.vol2 |= b; } else { m.vol2 &= !b; } return true; } }
    false
}

#[pyclass(frozen)]
struct Dex {
    inner: Arc<EngineDex>,
}

#[pymethods]
impl Dex {
    #[new]
    fn new(path: &str) -> PyResult<Self> {
        Ok(Dex { inner: Arc::new(EngineDex::load(path).map_err(PyValueError::new_err)?) })
    }
}

#[pyclass]
struct Battle {
    dex: Arc<EngineDex>,
    b: Engine,
}

impl Battle {
    fn check_mon(&self, side: usize, idx: usize) -> PyResult<()> {
        if side > 1 || idx >= self.b.sides[side].mons.len() {
            return Err(PyValueError::new_err(format!("no Pokemon {idx} on side {side}")));
        }
        Ok(())
    }
}

#[pymethods]
impl Battle {
    /// A new battle from two full teams (Showdown export text), starting at team preview.
    #[new]
    #[pyo3(signature = (dex, team_a, team_b, seed=1))]
    fn new(dex: &Dex, team_a: &str, team_b: &str, seed: u64) -> PyResult<Self> {
        let (a, b) = (parse_team(team_a), parse_team(team_b));
        let engine = Engine::new(&dex.inner, [&a, &b], seed).map_err(PyValueError::new_err)?;
        Ok(Battle { dex: dex.inner.clone(), b: engine })
    }

    /// A battle in progress: each side's brought Pokemon (export text, leads first).
    /// Starts at the move phase with no switch-in effects; then set HP, boosts, field...
    #[staticmethod]
    #[pyo3(signature = (dex, brought_a, brought_b, seed=1, turn=1))]
    fn from_brought(dex: &Dex, brought_a: &str, brought_b: &str, seed: u64, turn: u16) -> PyResult<Self> {
        let (a, b) = (parse_team(brought_a), parse_team(brought_b));
        let engine = Engine::from_brought(&dex.inner, [&a, &b], seed, turn).map_err(PyValueError::new_err)?;
        Ok(Battle { dex: dex.inner.clone(), b: engine })
    }

    fn copy(&self) -> Self {
        Battle { dex: self.dex.clone(), b: self.b.clone() }
    }

    /// Reseed the battle's random generator (e.g. before each rollout from a copy).
    fn reseed(&mut self, seed: u64) {
        self.b.rng = Rng::new(seed);
    }

    #[getter]
    fn phase(&self) -> &'static str {
        match self.b.phase {
            Phase::TeamPreview => "preview",
            Phase::Turn => "turn",
            Phase::Switch => "switch",
            Phase::Ended => "ended",
        }
    }

    #[getter]
    fn winner(&self) -> Option<u8> {
        self.b.winner
    }

    #[getter]
    fn turn(&self) -> u16 {
        self.b.turn
    }

    fn choose_team(&mut self, side: usize, order: Vec<u8>) {
        self.b.choose_team(&self.dex, side, &order);
    }

    fn needs_choice(&self, side: usize) -> bool {
        self.b.needs_choice(side)
    }

    /// For each active slot, the legal actions (moves at each valid target, with and without Mega,
    /// and switches). Don't send both slots to the same bench Pokemon.
    fn legal_actions(&self, side: usize) -> Vec<Vec<ActionTuple>> {
        policy::legal_actions(&self.b, &self.dex, side).iter().map(|v| v.iter().map(|&a| to_tuple(a)).collect()).collect()
    }

    fn choose(&mut self, side: usize, a0: ActionTuple, a1: ActionTuple) -> PyResult<()> {
        let acts = [from_tuple(&a0)?, from_tuple(&a1)?];
        self.b.choose(&self.dex, side, acts);
        Ok(())
    }

    /// The random policy's choice for a side (same policy as the Showdown comparison).
    fn random_actions(&mut self, side: usize, seed: u64) -> Vec<ActionTuple> {
        let mut rng = Rng::new(seed);
        policy::random_actions(&self.b, &self.dex, side, &mut rng).iter().map(|&a| to_tuple(a)).collect()
    }

    /// Play to the end with the random policy; returns the winner (0, 1, 2 = tie) or None.
    #[pyo3(signature = (seed, max_turns=100))]
    fn random_playout(&mut self, seed: u64, max_turns: u16) -> Option<u8> {
        let mut rng = Rng::new(seed);
        policy::random_playout(&mut self.b, &self.dex, &mut rng, max_turns)
    }

    /// `n` random playouts from copies of this position: (side 0 wins, side 1 wins, ties, unfinished).
    #[pyo3(signature = (n, seed, max_turns=100))]
    fn playouts(&self, py: Python<'_>, n: u32, seed: u64, max_turns: u16) -> (u32, u32, u32, u32) {
        let (b, dex) = (&self.b, &self.dex);
        py.detach(|| {
            let mut res = (0, 0, 0, 0);
            let mut rng = Rng::new(seed);
            for _ in 0..n {
                let mut c = b.clone();
                c.rng = Rng::new(rng.next_u32() as u64 | ((rng.next_u32() as u64) << 32));
                match policy::random_playout(&mut c, dex, &mut rng, max_turns) {
                    Some(0) => res.0 += 1,
                    Some(1) => res.1 += 1,
                    Some(_) => res.2 += 1,
                    None => res.3 += 1,
                }
            }
            res
        })
    }

    /// The 16 damage rolls of move `slot` of Pokemon `idx` on `side` against Pokemon `target_idx` on `target_side`, in
    /// the current position, without side effects (the engine's preview_rolls); None for status moves, immunities,
    /// absorbing abilities, empty slots or out-of-range indices. For the game review's matchup notes.
    /// `state_bp` (default true): base powers computed from the battle state (Flail, Gyro Ball, Payback...).
    /// `spread`: True / False = the hit was / was not a spread hit (0.75x); None = as in this position.
    #[pyo3(signature = (side, idx, slot, target_side, target_idx, state_bp=true, spread=None))]
    fn damage_rolls(&self, side: usize, idx: usize, slot: usize, target_side: usize, target_idx: usize, state_bp: bool,
                    spread: Option<bool>) -> Option<Vec<u16>> {
        let (s, ts) = (side.min(1), target_side.min(1));
        if idx >= self.b.sides[s].mons.len() || target_idx >= self.b.sides[ts].mons.len() {
            return None;
        }
        self.b.preview_rolls_spread(&self.dex, s, idx, slot, ts, target_idx, state_bp, spread).map(|r| r.to_vec())
    }

    /// The engine's speed of Pokemon `idx` on `side` in the current position (stat, boosts, Tailwind, paralysis,
    /// Choice Scarf / Iron Ball, weather abilities...; Trick Room NOT applied: it only reverses the order). For inferring an
    /// opponent's Speed from move order (spread_infer.py).
    fn speed(&self, side: usize, idx: usize) -> PyResult<u32> {
        self.check_mon(side, idx)?;
        Ok(self.b.speed(&self.dex, side.min(1), idx))
    }


    /// The whole state as plain Python data.
    fn state<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dex = &self.dex;
        let d = PyDict::new(py);
        d.set_item("turn", self.b.turn)?;
        d.set_item("phase", self.phase())?;
        d.set_item("winner", self.b.winner)?;
        let f = PyDict::new(py);
        f.set_item("weather", weather_name(self.b.field.weather))?;
        f.set_item("weather_turns", self.b.field.weather_turns)?;
        f.set_item("terrain", terrain_name(self.b.field.terrain))?;
        f.set_item("terrain_turns", self.b.field.terrain_turns)?;
        f.set_item("trick_room", self.b.field.trick_room)?;
        f.set_item("gravity", self.b.field.gravity)?;
        f.set_item("magic_room", self.b.field.magic_room)?;
        f.set_item("wonder_room", self.b.field.wonder_room)?;
        f.set_item("fairy_lock", self.b.field.fairy_lock)?;
        let lm = self.b.last_any_move;
        f.set_item("last_move", if (lm as usize) < dex.moves.len() { Some(dex.moves[lm as usize].id.as_str()) } else { None })?;
        d.set_item("field", f)?;
        let sides = PyList::empty(py);
        for s in 0..2 {
            let sd = &self.b.sides[s];
            let sdict = PyDict::new(py);
            let active: Vec<Option<u8>> = sd.active.iter().map(|&x| if x == NONE { None } else { Some(x) }).collect();
            sdict.set_item("active", active)?;
            sdict.set_item("tailwind", sd.tailwind)?;
            sdict.set_item("reflect", sd.reflect)?;
            sdict.set_item("light_screen", sd.light_screen)?;
            sdict.set_item("aurora_veil", sd.aurora_veil)?;
            sdict.set_item("mega_used", sd.mega_used)?;
            sdict.set_item("switch_needed", self.b.switch_needed[s].to_vec())?;
            sdict.set_item("reviving", self.b.reviving[s].to_vec())?;
            // entry hazards
            sdict.set_item("stealth_rock", sd.stealth_rock)?;
            sdict.set_item("spikes", sd.spikes)?;
            sdict.set_item("toxic_spikes", sd.toxic_spikes)?;
            sdict.set_item("sticky_web", sd.sticky_web)?;
            sdict.set_item("safeguard", sd.safeguard)?;
            sdict.set_item("wish", sd.wish.iter().map(|w| (w.0, w.1)).collect::<Vec<_>>())?;
            sdict.set_item("future", sd.future.iter().map(|w| (w.0, w.1, w.2)).collect::<Vec<_>>())?;
            sdict.set_item("healing_wish", sd.healing_wish.to_vec())?;
            sdict.set_item("shed_sub", sd.shed_sub.to_vec())?;
            sdict.set_item("fainted_total", sd.fainted_total)?;
            let mons = PyList::empty(py);
            for (mi, m) in sd.mons.iter().enumerate() {
                let md = PyDict::new(py);
                md.set_item("species", &dex.species[m.species as usize].name)?;
                // the team set's species, also while Transformed (species = the copied one)
                let set_sp = match &sd.transformed { Some(t) if t.mon as usize == mi => t.species, _ => m.species };
                md.set_item("set_species", &dex.species[set_sp as usize].name)?;
                md.set_item("hp", m.hp)?;
                md.set_item("maxhp", m.maxhp)?;
                md.set_item("status", status_name(m.status))?;
                md.set_item("fainted", m.fainted)?;
                md.set_item("ability", &dex.abilities[m.ability_id as usize].id)?;
                md.set_item("base_ability", &dex.abilities[m.base_ability_id as usize].id)?;
                md.set_item("item", if m.item_id == u16::MAX { "" } else { dex.items[m.item_id as usize].id.as_str() })?;
                // Magic Room / Klutz: the item the Pokemon still holds while it is hidden ("" = none)
                md.set_item("stash_item", if m.stash_item.0 == u16::MAX { "" } else { dex.items[m.stash_item.0 as usize].id.as_str() })?;
                // the item it used / ate last (Showdown lastItem: Recycle, Harvest, Pickup)
                md.set_item("last_item", if (m.last_item as usize) < dex.items.len() { dex.items[m.last_item as usize].id.as_str() } else { "" })?;
                let stats = PyDict::new(py);
                for (k, n) in STAT_IDS.iter().enumerate() {
                    stats.set_item(n, m.stats[k])?;
                }
                md.set_item("stats", stats)?;
                let boosts = PyDict::new(py);
                for (k, n) in BOOST_IDS.iter().enumerate().skip(1) {
                    boosts.set_item(n, m.boosts[k])?;
                }
                md.set_item("boosts", boosts)?;
                let moves: Vec<(String, u8)> = (0..m.n_moves as usize).map(|k| (dex.moves[m.moves[k] as usize].id.clone(), m.pp[k])).collect();
                md.set_item("moves", moves)?;
                md.set_item("choice_lock", if m.choice_lock != NONE && (m.choice_lock as usize) < 4 && m.moves[m.choice_lock as usize] != u16::MAX {
                    dex.moves[m.moves[m.choice_lock as usize] as usize].id.as_str() } else { "" })?;
                md.set_item("is_mega", m.is_mega)?;
                md.set_item("can_mega", m.mega_species != u16::MAX && !sd.mega_used)?;
                // public knowledge: revealed bits (0-3 move slots used, 4 item shown, 5 seen, 6 ability, 7 explicit),
                // item / ability known to an observer, Protect streak, moved since switching in, Unburden active, history
                md.set_item("rev", m.rev)?;
                md.set_item("item_known", ::vgcsim::battle::item_known(dex, m))?;
                md.set_item("ability_known", ::vgcsim::battle::ability_known(dex, m))?;
                md.set_item("protect_streak", m.protect_streak)?;
                md.set_item("moved", m.active_move_actions > 0)?;
                md.set_item("unburden", m.vol & vol::UNBURDEN != 0)?;
                let h = PyDict::new(py);
                h.set_item("slot", if m.h_slot == NONE { -1 } else { m.h_slot as i32 })?;
                h.set_item("target", m.h_target as i32)?;
                h.set_item("streak", m.h_streak as i32)?;
                h.set_item("protect", m.h_protect as i32)?;
                h.set_item("result", m.last_move_result as i32)?;
                for k in 0..4 {
                    h.set_item(format!("uses{k}"), m.h_uses[k] as i32)?;
                }
                h.set_item("protects", m.h_protects as i32)?;
                h.set_item("status", m.h_status as i32)?;
                md.set_item("history", h)?;
                let vd = PyDict::new(py);
                for (n, v) in vols_of(m) {
                    vd.set_item(n, v)?;
                }
                md.set_item("vols", vd)?;
                let tn = |t: u8| if t == ::vgcsim::dex::NO_TYPE { "???".to_string() } else { dex.type_names[t as usize].clone() };
                md.set_item("types", m.types.iter().filter(|&&t| t != ::vgcsim::dex::NO_TYPE).map(|&t| tn(t)).collect::<Vec<_>>())?;
                md.set_item("gender", match m.gender { 1 => "M", 2 => "F", _ => "" })?;
                md.set_item("added_type", if m.added_type == ::vgcsim::dex::NO_TYPE { String::new() } else { tn(m.added_type) })?;
                mons.append(md)?;
            }
            sdict.set_item("mons", mons)?;
            sides.append(sdict)?;
        }
        d.set_item("sides", sides)?;
        Ok(d)
    }

    /// Change one Pokemon (for matching a real position). `boosts`: dict like {"atk": -1}.
    /// `revealed`: what the opponent knows about this Pokemon (hidden-information features): bits 0-3 move
    /// slots, 4 item, 5 seen, 6 ability (set from a real battle: these bits are then taken as they are).
    #[pyo3(signature = (side, idx, hp=None, status=None, boosts=None, item=None, fainted=None, mega=None, first_turn=None, protect_streak=None, stats=None, revealed=None, history=None, unburden=None, choice_lock=None, pp_used=None, status_turns=None, sleep_elapsed=None))]
    #[allow(clippy::too_many_arguments)]
    fn set_mon(
        &mut self, side: usize, idx: usize, hp: Option<u16>, status: Option<&str>, boosts: Option<std::collections::HashMap<String, i8>>,
        item: Option<&str>, fainted: Option<bool>, mega: Option<bool>, first_turn: Option<bool>, protect_streak: Option<u8>,
        stats: Option<std::collections::HashMap<String, u16>>, revealed: Option<u8>,
        history: Option<std::collections::HashMap<String, i32>>, unburden: Option<bool>, choice_lock: Option<&str>,
        pp_used: Option<Vec<u8>>, status_turns: Option<u8>, sleep_elapsed: Option<u8>,
    ) -> PyResult<()> {
        self.check_mon(side, idx)?;
        if let Some(used) = &pp_used {
            // PP spent per move slot (a rebuilt position started every move at full PP)
            let dex = self.dex.clone();
            let m = &mut self.b.sides[side].mons[idx];
            for (k, u) in used.iter().enumerate().take(m.n_moves as usize) {
                let max = dex.moves[m.moves[k] as usize].max_pp;
                m.pp[k] = max.saturating_sub(*u);
            }
        }
        if let Some(u) = unburden {
            // Unburden active: the item was lost since this Pokemon last switched in (a rebuilt position
            // can't know it from item="" alone)
            let m = &mut self.b.sides[side].mons[idx];
            if u { m.vol |= vol::UNBURDEN; } else { m.vol &= !vol::UNBURDEN; }
        }
        if let Some(h) = &history {
            // per-Pokemon move history (see Mon::h_slot), e.g. from a real battle's log: slot (-1 = none since it came
            // in), target (1 / 2 = opposing slot a / b, 3 = ally, 0 = itself / none / spread), streak, protect (0/1), result (last turn: -1 none, 0 failed, 1 worked),
            // uses0..uses3, protects, status
            let m = &mut self.b.sides[side].mons[idx];
            let g = |k: &str| h.get(k).copied();
            if let Some(v) = g("slot") {
                m.h_slot = if (0..4).contains(&v) { v as u8 } else { NONE };
                // the same chosen move is Showdown's lastMove: Torment, Gigaton Hammer, Blood Moon
                m.last_move = if (0..4).contains(&v) { m.moves[v as usize] } else { u16::MAX };
            }
            if let Some(v) = g("target") { m.h_target = v as i8; }
            // Showdown lastMoveTargetLoc (engine encoding: + foe slot + 1, - own slot + 1, 0 none):
            // Instruct re-aims the last move there (an instructed High Horsepower went at a foe instead of the user's ally)
            if let Some(v) = g("loc") { m.last_target = v.clamp(-2, 2) as i8; }
            if let Some(v) = g("streak") { m.h_streak = v.clamp(0, 255) as u8; }
            if let Some(v) = g("protect") { m.h_protect = v != 0; }
            if let Some(v) = g("result") { m.last_move_result = v.clamp(-1, 1) as i8; m.last_move_failed = v == 0; }
            // a move stopped in BeforeMove: failed for Stomping Tantrum, but no move result
            if let Some(v) = g("failed") { m.last_move_failed = v != 0; }
            for k in 0..4 {
                if let Some(v) = g(&format!("uses{k}")) { m.h_uses[k] = v.clamp(0, 255) as u8; }
            }
            if let Some(v) = g("protects") { m.h_protects = v.clamp(0, 255) as u8; }
            if let Some(v) = g("status") { m.h_status = v.clamp(0, 255) as u8; }
        }
        if mega == Some(true) {
            self.b.set_mega_form(&self.dex, side, idx);
        }
        let dex = self.dex.clone();
        let was_fainted = self.b.sides[side].mons[idx].fainted;
        let m = &mut self.b.sides[side].mons[idx];
        // exact stats (e.g. from the battle request, or a guess): "hp" sets max HP
        if let Some(st) = stats {
            for (k, v) in st {
                if let Some(pos) = STAT_IDS.iter().position(|n| *n == k) {
                    m.stats[pos] = v;
                    if pos == 0 {
                        m.maxhp = v;
                        m.hp = m.hp.min(v);
                    }
                }
            }
        }
        if let Some(h) = hp {
            m.hp = h.min(m.maxhp);
            if m.hp == 0 {
                m.fainted = true;
            }
        }
        if let Some(st) = status {
            m.status = parse_status(st)?;
            // the status's own counter: sleep = turns left, freeze = Champions' 3-turn cap
            // left, toxic = residual stages so far. A rebuilt position used to reset them (sleep always 2 left, freeze 0 =
            // thawed at the first move, toxic back to 1/16). `status_turns` = the exact value (Rest's 3-turn sleep,
            // freeze, toxic); `sleep_elapsed` = sleep turns already counted down of an UNKNOWN 2-3 turn sleep: the engine
            // draws the length at the next BeforeMove, given that it lasted that long (0x80 | elapsed until then).
            m.status_turns = match m.status {
                Status::Sleep => 2,
                Status::Freeze => 3,
                _ => 0,
            };
        }
        if let Some(t) = status_turns {
            m.status_turns = t;
        } else if let Some(e) = sleep_elapsed {
            if m.status == Status::Sleep {
                m.status_turns = 0x80 | e.min(0x7f);
            }
        }
        if let Some(b) = boosts {
            for (k, v) in b {
                if let Some(pos) = BOOST_IDS.iter().position(|n| *n == k) {
                    if pos > 0 {
                        m.boosts[pos] = v.clamp(-6, 6);
                    }
                }
            }
        }
        if let Some(it) = item {
            if it.is_empty() {
                m.item_id = u16::MAX;
                m.item = ::vgcsim::effects::It::None;
            } else {
                let i = dex.item_index(it).ok_or_else(|| PyValueError::new_err(format!("unknown item {it}")))?;
                m.item_id = i as u16;
                m.item = dex.items[i].kind;
            }
        }
        if let Some(f) = fainted {
            m.fainted = f;
            if f {
                m.hp = 0;
            }
        }
        if let Some(ft) = first_turn {
            // Fake Out works only before the Pokemon's first move since switching in
            m.active_move_actions = if ft { 0 } else { 1 };
        }
        if let Some(p) = protect_streak {
            m.protect_streak = p;
        }
        if let Some(r) = revealed {
            m.rev = (r & 0x7f) | ::vgcsim::state::rev::EXPLICIT;
        }
        if let Some(cl) = choice_lock {
            // Choice item lock from a real battle's log: the move this Pokemon is locked into ("" = none).
            // Only a Choice item holder is locked; a move it doesn't have leaves it unlocked.
            m.choice_lock = NONE;
            let choice = matches!(m.item, ::vgcsim::effects::It::ChoiceBand | ::vgcsim::effects::It::ChoiceSpecs | ::vgcsim::effects::It::ChoiceScarf);
            if !cl.is_empty() && choice {
                if let Some(id) = dex.move_index(cl) {
                    if let Some(k) = m.moves.iter().position(|&x| x != u16::MAX && x as usize == id) {
                        m.choice_lock = k as u8;
                    }
                }
            }
        }
        // a Pokemon marked fainted counts for Last Respects / Supreme Overlord (the side's faint counter), as in a real game
        if !was_fainted && self.b.sides[side].mons[idx].fainted {
            self.b.sides[side].fainted_total = self.b.sides[side].fainted_total.saturating_add(1);
        }
        Ok(())
    }

    /// Put Pokemon `idx` in active slot `slot` (None = empty slot).
    /// Current types (type changes: Burn Up / Double Shock, Soak, Reflect Type, Protean...; [] or ["???"] =
    /// typeless) and the added third type (Forest's Curse / Trick-or-Treat; "" = none).
    #[pyo3(signature = (side, idx, types, added=None))]
    fn set_types(&mut self, side: usize, idx: usize, types: Vec<String>, added: Option<String>) -> PyResult<()> {
        self.check_mon(side, idx)?;
        let dex = self.dex.clone();
        let id = |n: &str| -> PyResult<u8> {
            if n == "???" || n.is_empty() { return Ok(::vgcsim::dex::NO_TYPE); }
            dex.type_names.iter().position(|x| x.eq_ignore_ascii_case(n)).map(|k| k as u8)
                .ok_or_else(|| PyValueError::new_err(format!("unknown type {n}")))
        };
        let mut t = [::vgcsim::dex::NO_TYPE; 2];
        for (k, n) in types.iter().take(2).enumerate() {
            t[k] = id(n)?;
        }
        if t[0] == ::vgcsim::dex::NO_TYPE && t[1] != ::vgcsim::dex::NO_TYPE {
            t.swap(0, 1);
        }
        let m = &mut self.b.sides[side].mons[idx];
        m.types = t;
        if let Some(a) = added {
            m.added_type = id(&a)?;
        }
        Ok(())
    }

    /// (side, idx) Transformed into (tside, tidx) earlier in the battle: species, types, stats, moves,
    /// ability (not started again), weight; current boosts kept; pp_used = PP spent on the copied moves (of 5).
    /// pre_mega: the target Mega Evolved after the Transform (copy its base form).
    #[pyo3(signature = (side, idx, tside, tidx, pp_used=None, pre_mega=false))]
    fn transform(&mut self, side: usize, idx: usize, tside: usize, tidx: usize, pp_used: Option<Vec<u8>>, pre_mega: bool) -> PyResult<bool> {
        self.check_mon(side, idx)?;
        self.check_mon(tside, tidx)?;
        let mut pu = [0u8; 4];
        for (k, x) in pp_used.unwrap_or_default().into_iter().take(4).enumerate() {
            pu[k] = x;
        }
        let dex = self.dex.clone();
        Ok(self.b.transform_from_outside(&dex, side, idx, tside, tidx, pu, pre_mega))
    }

    /// Set volatile counters / flags by name (see vol_fields!, VOL_BITS; engine convention), e.g.
    /// {"taunt": 2, "leechseed": 1, "leech_slot": 0, "leech_side": 1}. Unknown names raise.
    fn set_vols(&mut self, side: usize, idx: usize, vols: std::collections::HashMap<String, i64>) -> PyResult<()> {
        self.check_mon(side, idx)?;
        let m = &mut self.b.sides[side].mons[idx];
        let gastro_before = m.vol2 & ::vgcsim::state::vol2::GASTRO != 0;
        for (k, v) in vols {
            if !set_vol(m, &k, v) {
                return Err(PyValueError::new_err(format!("unknown volatile {k}")));
            }
        }
        // Gastro Acid suppresses the ability (the engine's move sets ability = None, keeping the
        // id): the flag alone left a Gastro-Acided Falinks' Defiant working
        let gastro = m.vol2 & ::vgcsim::state::vol2::GASTRO != 0;
        if gastro {
            m.ability = ::vgcsim::effects::Ab::None;
        } else if gastro_before {
            m.ability = self.dex.abilities[m.ability_id as usize].kind;
        }
        Ok(())
    }

    /// Side / field timers: safeguard turns, wish (turns, hp) per slot, future
    /// attack (turns, source mon, move-ish payload as stored) per slot, gravity / magic room / wonder room / fairy lock.
    /// healing_wish: per slot, a Healing Wish waiting for a Pokemon that needs it .
    #[pyo3(signature = (side, safeguard=None, wish=None, future=None, gravity=None, magic_room=None, wonder_room=None, healing_wish=None, fairy_lock=None))]
    #[allow(clippy::too_many_arguments)]
    fn set_extra(&mut self, side: usize, safeguard: Option<u8>, wish: Option<Vec<(u8, u16)>>, future: Option<Vec<(u8, u8, u8)>>,
                 gravity: Option<u8>, magic_room: Option<u8>, wonder_room: Option<u8>, healing_wish: Option<Vec<bool>>,
                 fairy_lock: Option<u8>) -> PyResult<()> {
        let sd = &mut self.b.sides[side];
        if let Some(h) = healing_wish { for (k, x) in h.into_iter().take(2).enumerate() { sd.healing_wish[k] = x; } }
        if let Some(v) = safeguard { sd.safeguard = v; }
        if let Some(w) = wish { for (k, x) in w.into_iter().take(2).enumerate() { sd.wish[k] = x; } }
        if let Some(w) = future { for (k, x) in w.into_iter().take(2).enumerate() { sd.future[k] = x; } }
        if let Some(v) = gravity { self.b.field.gravity = v; }
        if let Some(v) = magic_room { self.b.field.magic_room = v; }
        if let Some(v) = wonder_room { self.b.field.wonder_room = v; }
        if let Some(v) = fairy_lock { self.b.field.fairy_lock = v; }      // nobody can switch while > 0
        Ok(())
    }

    /// the item this Pokemon used / ate last (Showdown lastItem: what Recycle restores); None / "" = none.
    #[pyo3(signature = (side, idx, item=None))]
    fn set_last_item(&mut self, side: usize, idx: usize, item: Option<&str>) -> PyResult<()> {
        self.check_mon(side, idx)?;
        let id = match item {
            Some(it) if !it.is_empty() => self.dex.item_index(it)
                .ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("unknown item {it}")))? as u16,
            _ => u16::MAX,
        };
        self.b.sides[side].mons[idx].last_item = id;
        Ok(())
    }

    /// the CURRENT ability, changed by a move earlier in the stint (Entrainment, Skill Swap, Role Play,
    /// Worry Seed, Simple Beam, Doodle...): the team set keeps the base ability, which comes back at switch-out. Silent (no
    /// start effects: they already happened).
    fn set_ability(&mut self, side: usize, idx: usize, ability: &str) -> PyResult<()> {
        self.check_mon(side, idx)?;
        let id = self.dex.ability_index(ability)
            .ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("unknown ability {ability}")))?;
        let kind = self.dex.abilities[id].kind;
        let m = &mut self.b.sides[side].mons[idx];
        m.ability_id = id as u16;
        // under Gastro Acid the (new) ability stays suppressed         m.ability = if m.vol2 & ::vgcsim::state::vol2::GASTRO != 0 { ::vgcsim::effects::Ab::None } else { kind };
        Ok(())
    }

    /// the dex index of a move id (None = unknown): for set_vols' "call_move".
    fn move_index(&self, move_id: &str) -> Option<usize> {
        self.dex.move_index(move_id)
    }

    /// the last move anyone used (Showdown's battle.lastMove: what Copycat copies); None / "" = none.
    #[pyo3(signature = (move_id=None))]
    fn set_last_move(&mut self, move_id: Option<&str>) -> PyResult<()> {
        self.b.last_any_move = match move_id {
            Some(id) if !id.is_empty() => self.dex.move_index(id)
                .ok_or_else(|| pyo3::exceptions::PyValueError::new_err(format!("unknown move {id}")))? as u16,
            _ => u16::MAX,
        };
        Ok(())
    }

    /// Revival Blessing's request: `slot`'s Pokemon used it and now picks a fainted Pokemon (legal_actions lists
    /// them as switches). False if that slot is empty or nothing has fainted. Call after refresh() (which clears requests).
    fn set_reviving(&mut self, side: usize, slot: usize) -> PyResult<bool> {
        if side > 1 || slot > 1 {
            return Err(PyValueError::new_err("side / slot out of range"));
        }
        Ok(self.b.set_reviving(side, slot))
    }

    /// a mid-turn switch-out request for `slot`'s living Pokemon (U-turn, Eject Button...; see Battle::set_switching).
    /// Call after refresh(). False if there is nobody to switch in.
    #[pyo3(signature = (side, slot, baton=false, shed_tail=false))]
    fn set_switching(&mut self, side: usize, slot: usize, baton: bool, shed_tail: bool) -> PyResult<bool> {
        if side > 1 || slot > 1 {
            return Err(PyValueError::new_err("side / slot out of range"));
        }
        Ok(self.b.set_switching(side, slot, baton, shed_tail))
    }

    /// after set_switching: a move still to come this turn for the Pokemon in (side, slot), ("move", mslot,
    /// target, mega) as in choose(); it runs after the switch-in. False if refused (nobody there, no such move, already queued).
    fn queue_move(&mut self, side: usize, slot: usize, action: ActionTuple) -> PyResult<bool> {
        if side > 1 || slot > 1 {
            return Err(PyValueError::new_err("side / slot out of range"));
        }
        match from_tuple(&action)? {
            Action::Move { slot: ms, target, .. } => Ok(self.b.queue_move(&self.dex, side, slot, ms, target)),
            _ => Ok(false),
        }
    }

    fn set_active(&mut self, side: usize, slot: usize, idx: Option<usize>) -> PyResult<()> {
        if let Some(i) = idx {
            self.check_mon(side, i)?;
        }
        self.b.sides[side].active[slot.min(1)] = idx.map(|i| i as u8).unwrap_or(NONE);
        if let Some(i) = idx {
            self.b.sides[side].party_put(slot.min(1), i);
        }
        Ok(())
    }

    #[pyo3(signature = (weather=None, weather_turns=None, terrain=None, terrain_turns=None, trick_room=None))]
    fn set_field(&mut self, weather: Option<&str>, weather_turns: Option<u8>, terrain: Option<&str>, terrain_turns: Option<u8>, trick_room: Option<u8>) -> PyResult<()> {
        let f = &mut self.b.field;
        if let Some(w) = weather {
            f.weather = parse_weather(w)?;
            f.weather_turns = weather_turns.unwrap_or(5);
            f.weather_src = 0;
        }
        if let Some(t) = terrain {
            f.terrain = parse_terrain(t)?;
            f.terrain_turns = terrain_turns.unwrap_or(5);
            f.terrain_src = 0;
        }
        if let Some(tr) = trick_room {
            f.trick_room = tr;
        }
        Ok(())
    }

    /// Also sets the entry hazards on that side.
    #[pyo3(signature = (side, tailwind=None, reflect=None, light_screen=None, aurora_veil=None, mega_used=None,
                        stealth_rock=None, spikes=None, toxic_spikes=None, sticky_web=None))]
    fn set_side(&mut self, side: usize, tailwind: Option<u8>, reflect: Option<u8>, light_screen: Option<u8>, aurora_veil: Option<u8>, mega_used: Option<bool>,
                stealth_rock: Option<bool>, spikes: Option<u8>, toxic_spikes: Option<u8>, sticky_web: Option<bool>) {
        let sd = &mut self.b.sides[side.min(1)];
        if let Some(x) = stealth_rock {
            sd.stealth_rock = x;
        }
        if let Some(x) = spikes {
            sd.spikes = x.min(3);
        }
        if let Some(x) = toxic_spikes {
            sd.toxic_spikes = x.min(2);
        }
        if let Some(x) = sticky_web {
            sd.sticky_web = x;
        }
        if let Some(x) = tailwind {
            sd.tailwind = x;
        }
        if let Some(x) = reflect {
            sd.reflect = x;
            sd.screen_src[0] = 0;
        }
        if let Some(x) = light_screen {
            sd.light_screen = x;
            sd.screen_src[1] = 0;
        }
        if let Some(x) = aurora_veil {
            sd.aurora_veil = x;
            sd.screen_src[2] = 0;
        }
        if let Some(x) = mega_used {
            sd.mega_used = x;
        }
    }

    /// After editing: re-check the winner and pending replacements.
    fn refresh(&mut self) {
        self.b.refresh();
    }

    /// Turn on the readable battle log (see `log()`).
    fn enable_log(&mut self) {
        self.b.log = Some(vec![]);
    }

    fn log(&self) -> Vec<String> {
        self.b.log.clone().unwrap_or_default()
    }
}

#[pymodule]
fn vgcsim(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Dex>()?;
    m.add_class::<Battle>()?;
    m.add_class::<exact::ExactDex>()?;
    m.add_class::<exact::ExactBattle>()?;
    Ok(())
}
