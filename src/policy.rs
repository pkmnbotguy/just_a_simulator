//! Legal actions, and the simple random policy used for tests and rollouts.

use crate::battle::{Action, Battle, Phase};
use crate::dex::{Dex, Target};
use crate::state::Rng;
use arrayvec::ArrayVec;

/// The legal actions of one slot.
pub type SlotActions = ArrayVec<Action, 40>;

/// Bench Pokemon (alive, not active) of a side, minus `taken`.
#[inline]
fn bench_list(b: &Battle, side: usize, taken: &[u8]) -> ArrayVec<u8, 6> {
    let sd = &b.sides[side];
    let mut out = ArrayVec::new();
    for k in 0..sd.mons.len() {
        let k8 = k as u8;
        if sd.active[0] != k8 && sd.active[1] != k8 && sd.mons[k].alive() && !taken.contains(&k8) {
            out.push(k8);
        }
    }
    out
}

/// Opposing target locations (1, 2) with an alive Pokemon.
#[inline]
fn foe_locs(b: &Battle, side: usize) -> ArrayVec<i8, 3> {
    let mut t = ArrayVec::new();
    for f in 0..2 {
        if b.active(1 - side, f).is_some() {
            t.push(f as i8 + 1);
        }
    }
    t
}

/// Every legal action for each active slot of a side (in the current phase).
/// Moves aimed at one opponent are listed once per alive foe; switches once per bench Pokemon.
/// The two slots are listed independently (don't send both to the same bench Pokemon).
pub fn legal_actions(b: &Battle, dex: &Dex, side: usize) -> [SlotActions; 2] {
    let mut out: [SlotActions; 2] = [SlotActions::new(), SlotActions::new()];
    let bench = bench_list(b, side, &[]);
    if b.phase == Phase::Switch {
        for k in 0..2 {
            if b.switch_needed[side][k] && b.reviving[side][k] {
                // Revival Blessing: any fainted Pokemon (one still in an active slot too)
                for (j, m) in b.sides[side].mons.iter().enumerate() {
                    if m.fainted {
                        out[k].push(Action::Switch { to: j as u8 });
                    }
                }
            } else if b.switch_needed[side][k] {
                for &to in &bench {
                    out[k].push(Action::Switch { to });
                }
            }
            if out[k].is_empty() {
                out[k].push(Action::Pass);
            }
        }
        return out;
    }
    for k in 0..2 {
        let Some(i) = b.active(side, k) else {
            out[k].push(Action::Pass);
            continue;
        };
        let mega_ok = b.can_mega(side, i);
        let usable = b.usable_moves(dex, side, i);
        if usable.is_empty() {
            out[k].push(Action::Move { slot: 4, target: 0, mega: false });
        }
        for &slot in &usable {
            let mv = &dex.moves[b.mon(side, i).moves[slot as usize] as usize];
            // a non-Ghost's Curse targets itself (Showdown nonGhostTarget: the request takes no target)
            let self_curse = mv.kind == crate::effects::Mv::Curse && !b.mon(side, i).has_type(dex.ty.ghost);
            let targets: ArrayVec<i8, 3> = match mv.target {
                Target::Normal if !self_curse => {
                    let mut t = foe_locs(b, side);
                    if b.active(side, 1 - k).is_some() {
                        t.push(-((1 - k) as i8 + 1));
                    }
                    if t.is_empty() {
                        t.push(1);
                    }
                    t
                }
                Target::AdjacentAlly => {
                    // Showdown wants the ally's location even when the ally has fainted (the move then fails with
                    // [notarget])
                    let mut t = ArrayVec::new();
                    t.push(-((1 - k) as i8 + 1));
                    t
                }
                Target::AdjacentAllyOrSelf => {
                    // Acupressure / Champions' Milk Drink: the user itself, or the ally while it is there
                    let mut t = ArrayVec::new();
                    t.push(-(k as i8 + 1));
                    if b.active(side, 1 - k).is_some() {
                        t.push(-((1 - k) as i8 + 1));
                    }
                    t
                }
                _ => {
                    let mut t = ArrayVec::new();
                    t.push(0i8);
                    t
                }
            };
            for t in targets {
                out[k].push(Action::Move { slot, target: t, mega: false });
                if mega_ok {
                    out[k].push(Action::Move { slot, target: t, mega: true });
                }
            }
        }
        if b.mon(side, i).charge_slot == crate::state::NONE && !b.trapped(dex, side, i) {
            for &to in &bench {
                out[k].push(Action::Switch { to });
            }
        }
    }
    out
}

/// The random policy shared with tools/showdown_random_battles.js: Mega Evolve when possible,
/// switch 10% of the time, otherwise a random usable move at a random opponent.
pub fn random_actions(b: &Battle, dex: &Dex, side: usize, rng: &mut Rng) -> [Action; 2] {
    let mut out = [Action::Pass; 2];
    let mut taken: ArrayVec<u8, 2> = ArrayVec::new();
    if b.phase == Phase::Switch {
        for k in 0..2 {
            if b.switch_needed[side][k] && b.reviving[side][k] {
                let dead: ArrayVec<u8, 6> = (0..b.sides[side].mons.len()).filter(|&j| b.sides[side].mons[j].fainted).map(|j| j as u8).collect();
                if !dead.is_empty() {
                    out[k] = Action::Switch { to: dead[rng.below(dead.len() as u32) as usize] };
                }
            } else if b.switch_needed[side][k] {
                let bench = bench_list(b, side, &taken);
                if !bench.is_empty() {
                    let to = bench[rng.below(bench.len() as u32) as usize];
                    taken.push(to);
                    out[k] = Action::Switch { to };
                }
            }
        }
        return out;
    }
    let mut mega_done = false;
    for k in 0..2 {
        let Some(i) = b.active(side, k) else { continue };
        let bench = bench_list(b, side, &taken);
        if !bench.is_empty() && b.mon(side, i).charge_slot == crate::state::NONE && !b.trapped(dex, side, i) && rng.below(10) == 0 {
            let to = bench[rng.below(bench.len() as u32) as usize];
            taken.push(to);
            out[k] = Action::Switch { to };
            continue;
        }
        let usable = b.usable_moves(dex, side, i);
        let mega = !mega_done && b.can_mega(side, i);
        if mega {
            mega_done = true;
        }
        if usable.is_empty() {
            out[k] = Action::Move { slot: 4, target: 0, mega };
            continue;
        }
        let slot = usable[rng.below(usable.len() as u32) as usize];
        let mv = &dex.moves[b.mon(side, i).moves[slot as usize] as usize];
        let target = match mv.target {
            Target::Normal => {
                let foes = foe_locs(b, side);
                if foes.is_empty() { 1 } else { foes[rng.below(foes.len() as u32) as usize] }
            }
            Target::AdjacentAlly => -((1 - k) as i8 + 1),
            Target::AdjacentAllyOrSelf => {
                if b.active(side, 1 - k).is_some() && rng.below(2) == 0 { -((1 - k) as i8 + 1) } else { -(k as i8 + 1) }
            }
            _ => 0,
        };
        out[k] = Action::Move { slot, target, mega };
    }
    out
}

/// Play the battle to the end (or `max_turns` more turns) with the random policy for both sides.
/// Returns the winner (0, 1, 2 = tie) or None if unfinished.
pub fn random_playout(b: &mut Battle, dex: &Dex, rng: &mut Rng, max_turns: u16) -> Option<u8> {
    let stop = b.turn.saturating_add(max_turns);
    let mut guard = 0;
    while b.phase != Phase::Ended && b.turn < stop && guard < 5000 {
        guard += 1;
        let mut any = false;
        for side in 0..2 {
            if b.needs_choice(side) {
                any = true;
                let acts = random_actions(b, dex, side, rng);
                b.choose(dex, side, acts);
            }
        }
        if !any {
            break;
        }
    }
    b.winner
}
