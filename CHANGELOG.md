# Changelog

## 0.1.0 (2026-11)

First public release.

- Fast engine for Champions VGC Reg M-C doubles (`vgcsim::battle`), damage formula exact vs Showdown.
- Exact reference engine (`vgcsim::sim`), a port of Showdown's simulator, used as an oracle.
- Python bindings (`vgcsim`): battles, legal actions, position setters, playouts, damage rolls, and the
  exact reference engine (`ExactDex`, `ExactBattle`).
- Coverage team pool (418 teams covering every legal species, ability, move and item).
- Verification tools against a local Showdown checkout.
