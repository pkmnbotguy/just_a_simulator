# Changelog

## 0.1.1 (2026-11-05)

Fixes from a report by another bot maker (thanks!):

- No self stat change after a knockout that ends the battle (Clanging Scales, Scale Shot, Close Combat...). Recoil still happens.
- Red Card: when one spread move hits several holders, the fastest holder's card fires (Showdown runs AfterMoveSecondary
  over all targets speed-sorted), not the first one hit.
- `tools/scenarios.py`: new scenarios for these, plus Fake Out past turn one with one move / a Choice lock (the engine follows
  Champions, which turns it into Struggle; older Showdown pins let it fail); `ill_ok` for scripted choices the engine refuses
  but Showdown converts.
- `tools/bigseed_check.py`: `--require` (the ids a multi-turn case was made with).

## 0.1.0 (2026-11)

First public release.

- Fast engine for Champions VGC Reg M-C doubles (`vgcsim::battle`), damage formula exact vs Showdown.
- Exact reference engine (`vgcsim::sim`), a port of Showdown's simulator, used as an oracle.
- Python bindings (`vgcsim`): battles, legal actions, position setters, playouts, damage rolls, and the
  exact reference engine (`ExactDex`, `ExactBattle`).
- Coverage team pool (418 teams covering every legal species, ability, move and item).
- Verification tools against a local Showdown checkout.
