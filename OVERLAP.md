# Overlap behavior: findings and requirements

This document records how we worked out what omclone's sim has to do when parts overlap. It covers the evidence,
the design decisions, and the game experiments that settled the open questions. It ends with the behavior spec
as requirements **R1–R13**. The sim implements them, and the `overlap_tests` module in `src/test.rs` checks them,
mostly with the layouts of the game experiments.

omsim (`C:\Users\Steven\Documents\programming\temp\zachlike\omsim`) was the main reference. It's an independent
reimplementation and isn't authoritative: the game experiments below showed it's wrong in seven places.

## Summary

- **The problem:** omclone's solution sweep passed 20,350 of 20,441 solutions. All 91 failures were overlap
  solutions, or overlap or conduit edge cases from omsim's test set, and omsim passes all of them.
- **The causes:** every failure traces to one of these behaviors omclone lacked:
  - atoms stacking on one hex
  - disposal ignoring a just-unbonded atom
  - conduits claiming molecules before glyphs run, which allows cloning
  - conduit deliveries appearing at the receiving conduit's turn (always alongside cloning in the failures;
    exp13 later confirmed it on its own)
- **The fix:** atoms may share a hex. The one glyphs see is decided by molecule order: newer molecules and merges
  go on top, and unbonded molecules split and go on top after the glyph pass. The other behaviors change as listed
  in R1–R13. With all of it, the sweep reaches 20,441/20,441.
- **Where the game and omsim disagree:**
  - the order after a merge
  - whether two products landing on one hex collide with each other
  - which of two overlapping conduits claims a dropped molecule
  - whether an output can take a molecule hidden under an atom an arm has just grabbed
  - what a dangling bond does to a conduit: omsim clones when it points at an empty hex, and sends nothing when it
    points at an atom
  - how long atoms a conduit delivers stay "just unbonded"
  - whether a multi-bonder looks up all its atoms before making any of its bonds

  We follow the game in all seven.

## How we got here

### 1. Comparing the two sims' source

Reading both sims showed that they already agree on these:
- the order things run in each half-cycle: inputs, then glyphs, then outputs
- glyph order, which is file order with conduits mixed in
- overlapping outputs: the first in file order consumes
- an input sitting on a matching output: it's consumed twice per cycle
- track movement where tracks overlap: the earlier track wins each hex
- which parts count as overlapping
- arm-on-arm collisions
- area

The behaviors that differed:

1. **Atoms stacking.** omsim lets up to 6 atoms share a hex.
   - It does four things omclone didn't:
     - overlapping inputs both spawn
     - an input spawns where a glyph's product then lands on top of it
     - two products on one hex both appear
     - a product can land on a wheel atom
   - omclone prevented stacking in two ways. It treated a pending product as already filling its hex, and it
     spawned inputs one at a time.
2. **Conduits.**
   - Which overlapping conduit claims a molecule.
   - At which glyph's turn a delivery appears.
   - Cloning: a bonder joins a dropped molecule to something else before the conduit's turn.
   - Delivering every piece after an unbonder splits the molecule.
3. **Disposal and recent unbonding.** omsim's disposal ignores an atom unbonded earlier in the same half-cycle;
   omclone's consumed it.
4. **Reset on overlapping track.** omclone follows the game deliberately, and omsim's README admits omsim is wrong
   here. This stays out of scope.

### 2. The sweep and omsim

- **Baseline:**

  ```
  TestResults { total: 20441, passed: 20350, expected_collisions: 6, stat_mismatches: 44, sim_errors: 47 }
  ```

  The log is `test/experiments/tools/sweep.log`.
- **omsim on the failures:** omsim was built from source with Strawberry Perl's gcc. On all 91 failing solutions,
  it gives the recorded cycle count, or the expected collision for unsolved ones (`tools/omsim_results.txt`).
- **The convention for unsolved files:** a solution file without stats is expected to collide in the game. So the
  `debonding-z-order` tests 2, 3 and 5 collide in the game, and tests 1, 4 and 6 complete.

### 3. Tracing each failure to a cause

- **The instrumentation:** we added logging to a copy of omsim (`tools/omsim-instrumentation.patch`). It logs an
  event each time omsim does something omclone couldn't, such as:
  - stacking an input, product or wheel atom
  - revealing a hidden atom
  - an output taking a hidden atom
  - raising an unbonded atom
  - a conduit cloning, or delivering the pieces of a split molecule
  - a delivery landing on a hex covered by a glyph that sits between the sending and receiving conduits in file
    order (that glyph sees the delivery in one sim but not the other)
  - disposal skipping a just-unbonded atom
- **The tally:** for each failing solution, the earliest event is its first divergence (`tools/instr_results.txt`,
  tallied by `tools/tally.awk`):

| First divergence | Solutions | Puzzles |
|---|---|---|
| Disposal skips a just-unbonded atom | 21 | Electrum Separation ×4, Soothing Salve ×7, A Welcome to House Colvan ×7, Lustrous Syrup ×3 |
| Overlapping inputs both spawn and stack | 18 | debonding-z-order 1–6, Stamina Potion ×3, Surrender Flare ×4, Precision Machine Oil ×5 |
| An input spawns where a product then lands on top of it | 17 | conversion-z-order 2–3, Synthesis via Alcohol ×5, House Colvan ×4, Aether Reactor ×6 |
| Conduit clone | 19 | conduit edge cases 3 and 9, Dwarven Fire Wine ×2, Overloaded ×2, Emergency Antidote ×1, Aether Reactor ×12 |
| Delivery at the receiver's turn | 14 | Emergency Antidote ×13, Dwarven Fire Wine ×1 (all of these also clone later) |
| Conduit delivers unbonded pieces | 2 | Dwarven Fire Wine ×2 |

Some files are the same solution stored in several folders.

- **Behaviors that never came up:**
  - which overlapping conduit claims a molecule
  - two products landing on one hex
  - a product landing on a wheel atom

### 4. The disposal check, tested alone

- **The test:** in a scratch copy of omclone, Disposal was switched from `get_consumable_type_immediate` to
  `get_consumable_type`.
- **Fixed:** 20 of the 21 disposal failures.
- **Still failing:** House Colvan 270g, which also needs stacking later on.
- **Newly failing:** one solution, House Colvan 185g. It only passed because the old disposal happened to avoid a
  stack it couldn't handle.
- **History:** omclone added the "immediate" variant in d12f5f1 (2023), on the same day 1a25d85 started clearing
  the unbond set between half-steps. Projection later went back to the checked version; Disposal never did. So
  it was a leftover. Log: `tools/sweep_disposal.log`.

### 5. Design decisions

#### Stack order: per hex, or by molecule

omsim keeps an order for each hex, with three rules:
- a new atom goes on top
- removing the bottom (visible) atom brings the next one down
- after the glyphs, atoms whose molecule was just unbonded rise to the top

**Alternative: molecule order.** Each molecule has a position in one global order, and glyphs see the atom from
the lowest molecule on the hex. That one rule gives all three of omsim's rules. It suits omclone, which already
tracks molecules explicitly; omsim has to approximate molecules hex by hex. It's also probably how the game
works: molecules sit in a list, lookups take the first molecule with an atom on the hex, and new molecules are
appended.

The two models only differ when a bond merges molecules that have an atom on a stacked hex: omsim leaves each atom
in its layer, while molecule order moves the merged molecule. We chose molecule order, and game
experiments 1a, 1b and 5 then showed the game agrees with it rather than with omsim. They also showed that the
merged molecule goes to the very top, immediately. Exp7 showed that a bond within one molecule doesn't move it.

#### Two stacked molecules unbonded in the same half-cycle

omsim reports "disjoint bonds are not supported" here, unless the upper atom carries its `DISJOINT_SAFE` flag
(called `QUANTUM_SAFE` when added in 522d86d). The flag means the atom had no bonds pointing right, up-right or
down-right in the frame it was created in, and it's only set on input atoms and conduit deliveries.

omsim's comment says what the rule stands in for. In the game, the upper atom's bonds "fall down" to the atom
beneath it, without any atom changing molecule, which creates disjoint molecules. The direction test suggests
the game stores each bond on one of its two atoms. But omsim never tracks this for bonds that bonders create, so
the rule is incomplete.

Grimy's sub-cycle notes (`https://github.com/Grimy/om_overlap`) don't cover this, and a web search found nothing.
**Decision:** always report an error (R9). Then check whether any `debonding-z-order` test fails because of it,
before adding anything more.

**Outcome:** four game-verified solutions failed, so R9 now follows omsim's rule (see "What implementing it showed"
under section 6).

#### The pending-products structure

- **Requirements:**
  - Products are filed in the first half. Conversion glyphs file in glyph order. Conduits file under their
    receivers, which arrive out of order.
  - Each glyph releases its own products at its turn in the second half.
  - The collision code needs all pending products, in any order, up to 256 times per step.
- **Options considered:**
  - A flat list with a scan per glyph.
  - A per-glyph `Vec<Vec<_>>`: mostly empty, so every reader walks empty slots, and it adds indirection and
    allocations.
  - A map: a hash lookup at every glyph's turn.
  - Storing products on `Glyph`, as omsim does: this puts in-step state on a layout type.
  - Unplaced atoms in `WorldAtoms`: this breaks the rule that every atom in the map is on the board.
  - A queue kept sorted on every insert: order nobody reads until the second half, and surprising.
- **Chosen:** a `VecDeque` of products, each with its glyph and molecule.
  - It's stable-sorted once at the end of the first half and drained from the front at each glyph's turn.
  - Conduit claims use a queue of the same shape. Claims are made in glyph order, so that queue needs no sort.

### 6. Game experiments

In a first round, eleven unsolved solutions on one custom puzzle tested what no existing solution covers. A second
round follows below. All files are in
`test/experiments/` (gitignored), and `tools/exp_gen` writes them.

- **The puzzle:** `c100000000000001.puzzle`, "OMCLONE OVERLAP EXPERIMENTS".
  - Inputs: P0 Air, P1 Salt, P2 Fire–Fire bonded at (0,0)–(1,0), P3 Iron, P4 Lead, and P5 an open Fire
    triangle: Fire at (0,0), (1,0) and (0,1), with bonds (0,0)–(1,0) and (1,0)–(0,1).
  - Output: one Gold, placed at (−8,6) and never used.
- **Layouts:** inputs placed earlier in the file spawn first, so they lie lower in a stack. Every experiment except
  4 and 4b has an empty arm at (−8,0) that rotates, so motion phases happen. It rotates every cycle, except in exp8,
  whose 41-cycle loop has it rotate only in the first.

| Exp | Layout (file order) | omsim | Game | Conclusion |
|---|---|---|---|---|
| 1a | Air (2,0); Salt (0,0); Fire pair (0,0)–(1,0); bonder (1,0)–(2,0); disposal on (0,0) | no collision | no collision; Fire–Fire–Air, Salt gone | after the merge, the Salt was visible |
| 1b | Air (0,0); Salt (0,0); Fire pair (1,0)–(2,0); bonder (0,0)–(1,0); disposal on (0,0) | collision | no collision; Air–Fire–Fire, Salt gone | the merged molecule moved above the newer Salt; omsim is wrong |
| 5 | Air (0,0); Fire pair (1,0)–(2,0); Salt (0,0); bonder (0,0)–(1,0); disposal on (0,0) | collision | no collision; Air–Fire–Fire, Salt gone | the merged molecule goes to the very top, above even the newest atom |
| 2 | Rejection (0,0); Iron (0,0); Van Berlo (2,0), with its Salt on (1,0) | collision, first cycle | immediate collision | Rejection treats a wheel atom on its output hex as empty; omsim does the same for conversion glyphs |
| 3 | Purification (0,0) rot 0 and (0,2) rot 3, both output to (0,1); Lead on (0,0), (1,0), (0,2), (−1,2) | collision, second cycle | collision during the first cycle's motion | both fire, and the two products collide with each other; omsim misses this |
| 4 | conduits 101 (0,0), 100 (0,0), 100 (5,0), 101 (0,5); Salt (0,0); arm (−1,0): grab, drop | delivered at (5,0) | delivered at (0,5) | omsim's lowest-ID rule is wrong |
| 4b | as 4, with 100 listed first | delivered at (5,0) | delivered at (5,0) | the first conduit in the file claims |
| 6 | Fire pair (0,0)–(1,0); Salt (0,0); unbonder (0,0)–(1,0); disposal on (0,0) | collision | collision at (0,0); Fire drawn at (0,0), Fire at (1,0) | unbonded molecules move up only after all glyphs; until then the disposal sees the Fire and skips it |
| 7 | open Fire triangle (0,0), (1,0), (0,1); Salt (0,0); bonder (0,0)–(0,1); disposal on (0,0) | collision | collision at (0,0), with the Salt drawn there; the triangle closed (three bonds) | a bond within one molecule doesn't move it |
| 8 | Salt (0,0); Air (0,0); disposal on (0,0); arm (−1,0): grab, then rotate | collision | collision at (0,0) in the first cycle, with the Air drawn there | the arm grabbed the visible Salt, so the disposal couldn't take it; grabbing both atoms isn't ruled out |
| 9 | Purification (0,0) rot 0 with Lead on (0,0) and (1,0); Van Berlo (0,2), with its Fire on the output hex (0,1) | Leads consumed; collision during the first cycle's motion | the same | conversion glyphs also treat a wheel atom as empty, and a single product collides with an atom already on its hex |

Taken together, these support one model of the game. Molecules sit in a single ordered list, and glyphs see the
atom from the earliest molecule on each hex. These go to the end of the list:
- new molecules (inputs, products, conduit deliveries)
- merged molecules, immediately; a bond within one molecule leaves it in place
- molecules split by unbonding, after the glyph pass

#### Second round

A review of the plan left five behaviors no test settled, so eight more solutions tested them. They use two more
puzzles:
- **`c100000000000002`, "OMCLONE OVERLAP EXPERIMENTS 2".**
  - Inputs: P0 Air, P1 Salt, P2 Fire–Fire bonded at (0,0)–(1,0), P3 Lead, P4 Air–Air bonded at (0,0)–(1,0).
  - Outputs: O0 Gold and O1 the Air pair. Any output an experiment doesn't use sits far away, since omsim refuses a
    solution that leaves one out.
- **`c100000000000003`, "OMCLONE OVERLAP EXPERIMENTS 3".**
  - Input: P0, a Salt with a bond toward the empty hex (1,0).
  - Output: Gold.
  - It's a separate puzzle in case the game rejected it. The game didn't.

Every tape below is padded to a 41-cycle loop, so each runs once at the start, and the spinner rotates every cycle.
Cycles are counted from 1.

| Exp | Layout (file order) | omsim | Game | Conclusion |
|---|---|---|---|---|
| 10 | Salt (0,0); Air (−2,2); bonder and unbonder both at (−1,1)–(−2,2); conduit 100 at (−1,1) and (6,0); arm (−1,0): grab, rotate, drop, so the Salt is dropped on (−1,1) in cycle 3 | clone | Salt at both (−1,1) and (6,0) | a bond to an outside atom that an unbonder removed earlier in the same half still makes the conduit clone |
| 11 | Salt (0,0); Air pair at (1,0) rot 3, covering (1,0) and (0,0) above the Salt; O1 on the same hexes; arm (−1,0): grab, rotate | no collision; O1 takes the pair in cycle 1, and again in cycle 2 once it respawns | collision | once an arm grabs at a stacked hex, an output can't take the molecule above it there |
| 11b | as 11, without the arm | no collision; O1 takes the pair once | no collision; one output; a Salt left | outputs take a molecule with a hidden atom (R11) |
| 12 | Fire pair (0,0); unbonder at (0,0)–(1,0); conduit 100 at (0,0) covering (0,0),(1,0); bonder at (0,0)–(1,0); conduit 100 at (0,5), the same shape; disposal (0,5); arm (−1,0): grab, drop | both pieces sent; the disposal never takes the Fire on (0,5) | both pieces sent unbonded; the Fire on (0,5) survives cycle 2 and is disposed in cycle 3 | delivered pieces stay "just unbonded" for the rest of the half they arrive in, then the state clears |
| 13a | Salt (0,0); conduit 100 at (0,0); disposal (0,5); conduit 100 at (0,5); arm (−1,0): grab, drop; length-2 arm (−2,5): blank, blank, grab, rotate | delivered at the receiver's turn, after the disposal | the second arm carries a Salt away | the delivery appears at the receiver's turn |
| 13b | as 13a, with the two conduits swapped in file order | disposed in the half it arrives | the second arm comes up empty | the same, and a glyph acts on a delivery in the half it arrives |
| 13c | Lead (0,0) and (1,0); Purification (0,0); disposal (0,1); length-2 arm (−2,1): blank, grab, rotate | each Tin disposed in the half it's made | not run: only needed if 13a and 13b had both carried | |
| 14 | the dangling-bond Salt (0,0); conduit 100 at (−1,1) and (6,0); arm (−1,0): grab, rotate, drop, leaving the bond pointing at the empty (−1,2) | clone | the Salt moved to (6,0) with its bond; (−1,1) empty | a bond toward an empty hex doesn't make the conduit clone |

Exp10 and exp14 together give the clone rule in R13b. Exp12 shows the game keeps a "just unbonded" state on each
atom, as omsim does, which suggests the clone check reads it the same way omsim does. Another rule would also fit:
"clone if a bond to an outside atom was made after the claim". The two differ only for a one-sided bond toward an
occupied hex, whether it was occupied at the claim or filled after it, so we chose the one that matches omsim's
mechanism. The third round showed that neither is right (see R13b).

#### What implementing it showed

The sweep settled three more things.

- **Disjoint bonds (R9).** "Always error" failed four game-verified solutions: `debonding-z-order` 1, 4 and 6, and
  Refined Bronze 355g.
  - Tests 1 and 3 have the same atoms, bonds and glyphs. The only difference is which end of one input chain is that
    input's origin.
  - The game completes test 1 and collides on test 3, which is exactly omsim's `DISJOINT_SAFE` distinction. So R9
    now uses omsim's rule.
- **When unbonded molecules split (R3c).** Refined Bronze 355g still failed after that.
  - A multi-bonder there re-joins pieces of a molecule that earlier unbonders had cut in the same half.
  - If the pieces were already separate molecules, that merge would lift them on top (R3b). Unbonders later in the
    file would then see a Quicksilver instead of the Salt they need, and a stack would be left.
  - It passes once a molecule unbonded in a half splits only after the glyph pass. Re-joining its pieces is then a
    bond within one molecule, which doesn't move it (exp7).
  - The same model accounts for R10 and exp12, since an unbonded atom isn't on its own until the split.
  - Looking up a multi-bonder's atoms before making any of its bonds, as omsim does, didn't fix it alone. With the
    deferred split, it makes no difference to the sweep, so omclone keeps looking them up bond by bond.
- **The clone check reads bonds.** A version based on molecule membership failed seven conduit solutions:
  - it cloned when a claimed atom's molecule reached outside the claim;
  - but a bond an unbonder had just removed still made the game clone where that version sent.

  The third round led back to molecule membership, and found what had gone wrong (see below).

#### Third round

A review of the implementation listed the rules it follows without game evidence, and twelve more solutions tested
them. They use two more puzzles:
- **`c100000000000004`, "OMCLONE OVERLAP EXPERIMENTS 4".**
  - Inputs: P0 the dangling-bond Salt from `c100000000000003`, P1 Air, P2 Salt, P3 Fire–Fire, and P4 a closed Fire
    triangle at (0,0), (1,0) and (0,1).
  - Outputs: O0 Salt, and O1 a star of four Salts, one bonded to each of the other three. It's placed to cover a
    multi-bonder and its three spokes.
- **`c100000000000005`, "OMCLONE OVERLAP EXPERIMENTS 5".** Inputs: P0 the dangling-bond Salt, and P1 Iron for a
  Rejection. Output: Gold.

As in the second round, tapes are padded to a 41-cycle loop, and cycles are counted from 1. Exp17a–c use a Salt output
under the Salt as the probe: it can take the Salt only if the Salt is the visible atom after the glyph pass.

| Exp | Layout (file order) | omsim | Game | Conclusion |
|---|---|---|---|---|
| 15a | the dangling-bond Salt (0,0); Air (−1,2); conduit 100 at (−1,1) and (6,0); arm (−1,0): grab, rotate, drop, leaving the bond pointing at the Air | nothing sent; the Salt stays at (−1,1) | the Salt moved to (6,0) with its bond; (−1,1) empty | a dangling bond toward an occupied hex doesn't make the conduit clone, and doesn't join the two atoms |
| 15b | as 15a, plus a disposal on (−1,2) last in the file, so the Air is gone at each claim and back by the conduit's turn | clone | the same as 15a | the same when the hex fills after the claim |
| 16 | Salt (0,0); disposal (0,0); conduit 100 at (0,0) and (0,5); arm (−1,0): grab, drop | nothing sent | nothing sent | a conduit sends nothing when a claimed atom is consumed before its turn (R13a) |
| 17a | Fire pair (0,0); Salt (0,0); unbonder (0,0)–(1,0); O0 (0,0) | no collision | no collision; two lone Fires left | unbonded pieces move up before outputs run |
| 17b | as 17a, with the Salt and O0 on (1,0) | no collision | no collision; two lone Fires left | every piece of a split molecule moves up |
| 17c | closed Fire triangle (0,0); Salt (0,0); unbonder (0,0)–(1,0), which leaves the triangle connected; O0 (0,0) | no collision | no collision; the triangle left with two bonds | an unbonded molecule moves up even when it doesn't split |
| 18 | Salt (0,0) and Air (1,0); bonder (0,0)–(1,0); conduit 100 on (0,0) and (1,0); unbonder (0,0)–(1,0); conduit 100 at (0,5), the same shape; arms (−1,0) and (2,0): grab, drop | both cloned | unbonded Salts and Airs on all four hexes | claims are per molecule: the bonder joined each claim to an atom outside it, so both were cloned |
| 19 | Salts at (0,0), (1,0), (0,−1) and (−1,1); Air (0,0); multi-bonder (0,0); O1 covering them | no collision; O1 takes the star | collision | a multi-bonder makes its bonds one at a time, and each sees the order the bond before it left |
| 20 | Fire pair (0,0); unbonder (−1,1)–(−1,2); conduit 100 at (−1,1) and (6,0); disposal (6,0); arm (−1,0): grab, rotate, drop, so the pair is cut while held in cycle 2 and the arm's Fire is dropped beside the other | the Fire moves and is disposed in the half it arrives | the same | a bond removed in the half before the drop doesn't make the conduit clone, and the "just unbonded" state doesn't carry over |
| 21a | as 15a, with a second dangling-bond Salt at (−1,2) instead of the Air, rotated so its bond would point at (−1,1) | nothing sent | the second Salt's bond pointed right, not at (−1,1); the dropped Salt moved | the game doesn't rotate a one-atom input. So this was another exp15a, and agrees with it |
| 21b | (`c100000000000005`) the dangling-bond Salt (0,0); Iron (−2,1); bonder (−1,1)–(−1,2); Rejection (−2,2), whose Quicksilver lands on (−1,2); conduit 100 at (−1,1) and (6,0); arm (−1,0): grab, rotate, drop; arm (−3,2): grab, rotate, which brings the Iron to the Rejection in cycle 2, after the bonder's turn | nothing sent | cloned | a bonder completing the Salt's dangling bond after the claim makes the conduit clone, even though the Salt had that bond when claimed |
| 21c | the dangling-bond Salt (0,0); another, unrotated, at (−3,0), its bond pointing at (−2,0); conduit 100 at (−2,0) and (6,0); arm (−1,0): grab, rotate three times, drop, so the two bonds face each other in cycle 5 | nothing sent | the Salt moved to (6,0) with its bond; (−2,0) empty | two dangling bonds facing each other don't join the atoms, or make the conduit clone |

omsim is wrong about exp15a, exp15b, exp19, exp21b and exp21c.

**What it changed.** Everything matched omclone except the clone check (R13b):
- **The old rule:** omclone cloned when a claimed atom had a bond toward an occupied hex outside the claim, so it
  cloned in exp15a and exp15b.
- **The candidates:** two narrower rules also fit every earlier experiment and the sweep:
  - count only bonds the claimed atom gained after the claim; exp21b ruled it out;
  - count only two-sided bonds; exp21c ruled it out.
- **The rule that fits:** the game clones when a bonder has joined the claimed molecule to something outside the
  claim. Dangling bonds never join molecules.
- **Why molecule membership failed before:** the 7 sweep solutions it failed (36 claims) each have two claims joined
  by a bond that an unbonder then removed. omclone split the first claim's leftovers as soon as it cloned them. So
  the second claim no longer saw the join, and was sent, while the game cloned it.
- **The fix:** with those leftovers splitting at the end of the glyph pass (R3d), membership passes the sweep and
  gives the game's result in every experiment.

exp21a also showed that the game doesn't rotate a one-atom input. omclone does, which only matters for a one-atom
input with a dangling bond.

## Requirements

Each requirement lists its evidence. "Sweep" means game-verified solutions that only pass with it.

- **R1. Stacking.** Several atoms may share a hex.
- **R2. The visible atom.**
  - On each hex, glyphs, arm grabs, input blocking, conduit claims, outputs and repeating outputs see only the
    visible atom. That's the one whose molecule is lowest in molecule order, i.e. the oldest. (omsim; sweep; Game:
    exp8 for arm grabs)
  - Newer atoms are "above" it, and the game draws them over it, as in experiments 6 and 8.
  - **An arm grabbing at a hex with several atoms is a collision.** It takes the visible atom (exp8). After that,
    an output can't take the molecule above it there (exp11; exp11b shows it could without the grab). So nothing
    can clear the stack before motion (R8).
    - Decided: report it at the grab, with R8's error.
    - Evidence: Game, experiments 8, 11 and 11b. omsim gets this wrong.
- **R3. Molecule order.**
  - **a.** A newly created molecule goes on top. That covers inputs (in file order within one spawn pass), wheel
    atoms at setup, conversion products, Rejection's quicksilver, and conduit deliveries. (omsim; sweep)
  - **b.** A bond that merges two different molecules puts the merged molecule on top immediately, so later
    glyphs in the same half-cycle see the change. A bond within one molecule leaves it where it is. (Game:
    experiments 1a, 1b, 5 and 7)
    - A glyph that makes several bonds makes them one at a time, so each sees the order the bond before it left.
      (Game: exp19; omsim looks up all the atoms first.)
  - **c.** Unbonding removes the bond at once. The molecule splits into its pieces only after all glyphs of that
    half-cycle, before outputs, and the pieces then go on top, keeping their relative order.
    - Until then the molecule is whole: glyphs see the old order, and a bond between its pieces is a bond within one
      molecule (R3b).
    - Its atoms aren't on their own, so R10 follows.
    - Every piece goes on top, and so does an unbonded molecule that doesn't come apart.
    - Evidence: Game, exp6, exp12 and exp17a–c; sweep: Refined Bronze 355g; omsim's `debonding-z-order` tests.
  - **d.** When a conduit clone strips bonds from the atoms it leaves behind, their molecule also splits only after
    the glyph pass, and its pieces keep their place.
    - The timing: splitting at once hides from a later claim that its molecule was joined to the first (R13b).
      Sweep: 7 solutions.
    - The order: omsim; untested in the game.
  - **e.** Removing the visible atom reveals the next one in order. (omsim; sweep)
- **R4. Input spawning.**
  - In each half-cycle, first decide for every input whether any of its hexes holds an atom; wheel atoms count.
    Then spawn every unblocked input in file order.
  - Atoms waiting to be produced don't block.
  - (omsim; Grimy's notes; sweep: 35 solutions)
- **R5. Conversion glyph outputs and Rejection.**
  - They fire when every output hex is empty or its visible atom is a wheel atom. Atoms waiting to be produced
    don't count.
  - So two products may land on one hex.
  - (omsim. Game: exp2 for Rejection onto a wheel atom, exp9 for a conversion glyph onto one, and exp3 for two
    products.)
- **R6. When products appear.**
  - Pending products are created in the second half, at the turn of the glyph that releases them: the
    conversion glyph itself, or, for a conduit delivery, the receiving conduit.
  - Products are created in glyph order, so they stack above inputs spawned earlier in that half.
  - A glyph later in the file acts on a product or delivery in the half it appears.
  - (omsim; conversion-z-order tests; Game: exp13a and 13b for deliveries. In the sweep, delivery order is also the
    first divergence in 14 solutions that need cloning too.)
- **R7. Collisions with pending products.**
  - During motion, pending products collide with stationary atoms on their hex, moving atoms, arm bases, and
    each other. In practice, only two products on the same hex meet each other.
  - (Game: exp3 for two products, and exp9 for a product and a stationary wheel atom. omsim misses
    product-on-product collisions.)
- **R8. Stacks before motion.**
  - A stack still present at the end of the first half-cycle is a collision.
  - Moving an atom onto an occupied hex stays a collision.
  - (omsim; Game: experiments 2, 6, 7 and 8)
- **R9. Disjoint bonds.**
  - After the glyphs of a half-cycle, take each hex holding atoms from molecules unbonded in that half-cycle. Every
    such atom above the lowest one must be *disjoint-safe*, or it's an error.
  - An atom is disjoint-safe when an input or a conduit delivery created it with no bond pointing right, up-right
    or down-right in its own frame. For inputs that's the input's frame; for deliveries, the frame they leave from.
  - This is omsim's rule. omsim's comment explains it: the game seems to store each bond on one of its two atoms,
    and the upper atom's bonds can get attached to the atom beneath.
  - Evidence: sweep, `debonding-z-order` 1, 3, 4 and 6, and Refined Bronze 355g. "Always error" fails four of them.
- **R10. Disposal.** Disposal doesn't consume an atom unbonded earlier in the same half-cycle. (omsim; sweep: 21
  solutions; scratch test) R3c's deferred split gives this too.
- **R11. Outputs.**
  - An output identifies the molecule from the visible atom on its origin atom's hex, or its first atom's hex if
    the origin has no atom.
  - It matches by the positions of that molecule's atoms, so hidden atoms count.
  - It consumes the whole molecule, wherever its atoms sit in their stacks.
  - (omsim; `debonding-z-order`)
- **R12. Conduit claims.**
  - Right after grabs and drops, before inputs and glyphs, each conduit claims in file order. A conduit claims
    each just-dropped molecule that lies entirely on its hexes, with no atom grabbed.
  - At most one conduit claims a given molecule: the first in the file.
  - Each molecule is a separate claim, even when one conduit claims several.
  - (omsim's timing; Game: experiments 4 and 4b for the order, exp18 for separate claims)
- **R13. Conduit sending, at the conduit's first-half turn.**
  - **a.** If any claimed hex is now empty, send nothing. (omsim; Game: exp16)
  - **b.** Clone when the claimed molecule now has an atom outside the claimed hexes, which happens when a bonder
    has joined it to something since the claim.
    - It still counts when an unbonder has removed that bond again, since molecules split only after the glyph pass
      (R3c, R3d). A bond removed before the drop doesn't count (exp20).
    - A dangling bond from a puzzle input never joins molecules, so it never counts. It doesn't matter whether it
      points at an empty hex, an atom, or another dangling bond.
    - A bonder completing a dangling bond the claimed atom already had does join them.
    - To clone: send a copy of the molecule as it was claimed, and strip from the atoms left behind the bonds the
      copy has.
    - Evidence: omsim; sweep: 19 solutions; Game: exp10 for a removed bond; exp14, exp15a, exp15b, exp21a and exp21c
      for dangling bonds; exp21b for a completed one; exp18 for each claim being checked alone.
    - omsim gets dangling bonds wrong. It clones for one toward an empty hex, and sends nothing for one toward an
      atom.
  - **c.** Otherwise, remove the atoms now on the claimed hexes and send their molecules, so pieces cut by an
    earlier unbonder all go.
    - A molecule unbonded earlier in that half hasn't split yet (R3c), so it goes whole. It stays "just unbonded"
      until it splits at the end of the half it arrives in.
    - Evidence: omsim; sweep: 2 solutions; Game: exp12.
    - omsim never clears that state on a delivered atom, which is wrong.

**Unchanged on purpose:**
- Reset on overlapping track (omclone follows the game).
- omsim's errors for track moves off track and extend/retract on non-pistons.
- How stacks are drawn.

**Known to differ from the game, not fixed yet:**
- The game doesn't rotate a one-atom input (exp21a), and omclone does. It only shows when the atom has a dangling
  bond. Whether the game rotates a one-atom output isn't known.

**Not known from the game:**
- The order in R3d. Nothing can land on a held or dropped atom without colliding, so both orders look the same in
  practice.
- What actually happens with disjoint bonds (R9). The rule fits every solution we have, but omsim admits it's
  incomplete: bonds made by bonders never change whether an atom is disjoint-safe, and products are never
  disjoint-safe.
- Whether a triplex bonder, like a multi-bonder (exp19), makes its bonds one at a time.
- Whether an arm grabbing at a stacked hex holds the atoms above, or only stops outputs taking them. Either way it's
  a collision (R2).
- Whether a clone's copy, taken at the claim, keeps a "just unbonded" state from the previous half. omsim's copy
  does; omclone's doesn't.

## Files and tools

All of these are in `test/experiments/`, which is gitignored:

- `c100000000000001.puzzle`, and the experiment solutions `exp1a`, `exp1b`, `exp2`, `exp3`, `exp4`, `exp4b`,
  `exp5`, `exp6`, `exp7`, `exp8` and `exp9`.
- `c100000000000002.puzzle` with `exp10`, `exp11`, `exp11b`, `exp12`, `exp13a`, `exp13b` and `exp13c`;
  `c100000000000003.puzzle` with `exp14`.
- `c100000000000004.puzzle` with `exp15a`, `exp15b`, `exp16`, `exp17a`, `exp17b`, `exp17c`, `exp18`, `exp19`,
  `exp20`, `exp21a` and `exp21c`; `c100000000000005.puzzle` with `exp21b`.
- `tools/exp_gen/`: a scratch crate that writes the puzzles and solutions using omclone's writer, then runs each
  in omclone. After every cycle it prints the atoms with their bond directions, what each arm holds, and each
  output's remaining count.
  - It depends on omclone by absolute path.
  - Run it with `cargo run --release`, with `CARGO_TARGET_DIR` pointing outside the repo.
- `tools/omsim-instrumentation.patch`: the logging added to omsim's `sim.c`. It also logs each atom a disposal
  removes. To use it:
  1. Copy omsim's sources to a scratch folder.
  2. Apply the patch with `patch -p0 --binary`, from the folder that holds `omsim_instr/`.
  3. Build: `gcc -O2 -std=c11 -Wno-missing-braces -D_DEFAULT_SOURCE -o omsim_instr.exe collision.c decode.c
     parse.c sim.c steady-state.c verifier.c main.c -lm`
  4. Run: `omsim_instr.exe -p <puzzle> -m cycles <solution>`. Events go to stderr.
- `tools/run_omsim.sh`, `tools/run_instr.sh`, `tools/run_experiments_omsim.sh` and `tools/tally.awk`: the scripts
  that produced the tables above.
  - They expect the session scratchpad layout they were written in: the omsim build beside them, and
    `sweep.log` and `failing_lines.txt` in the same folder. Adjust the paths before reusing them.
- `tools/sweep.log` (the baseline sweep), `tools/sweep_disposal.log` (with the disposal change),
  `tools/failing_lines.txt`, `tools/omsim_results.txt` and `tools/instr_results.txt`.
- `tools/omsim_round3.txt`: the instrumented omsim's output for the third round, which gives that table's omsim
  column. No conduit event means nothing was sent.
