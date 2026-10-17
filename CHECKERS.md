# Variant checkers

omclone used to ship two puzzle-specific "checkers": browser tools that ran one solution against many
input/output variants of a computation puzzle, and reported which variants failed plus worst-case stats.
They were removed because they weren't useful any more (and one no longer compiled). This note records how
they worked and what adding a new one would take.

## What they were

| Checker | Puzzle | Variants |
|---|---|---|
| `cx_checker` | OM2022 *Transmutation CX* | Six salt/fire atoms in a ring evolve by rule 110 (each output atom depends on itself and its two ring neighbors). All 64 inputs were checked one per run, plus random batches of 6 in one run. |
| `hab_checker` | OM2023 week 8 *Habitability Detector* | A 3-element pattern (64 variants) is searched for in a 6-element sequence (4096 variants), and the output is gold if found, salt if not. There was an ordered 64 × 4096 sweep plus a random runner. |

Both showed a pass/fail grid (one mark per variant), the error and step for each failure, and the worst
cycles, cost and area, plus wSum = area + cycles + cost/5.

`cx_checker` had already been described as broken since commit `a05979d` (2022). `hab_checker` stopped
compiling when `Atom` gained its `immutable_wheel` and `molecule` fields.

## Where they went

The removed files were:

- `src/cx_checker.rs`
- `src/hab_checker.rs`
- `html/cx_checker.html`
- `html/hab_checker.html`
- `OM2022_TransmutationCX.puzzle`

To get them back, find the commit that deleted them, then show each file as it was just before it:

```bash
git log --diff-filter=D --oneline -- src/hab_checker.rs
```

```bash
git show <that commit>^:src/hab_checker.rs
```

The Habitability Detector puzzle file and the built `.js`/`.wasm` outputs were never committed.

## Infrastructure they used

### Cargo

- **One `cdylib` library target per checker.** A package can only have one `[lib]`, and `required-features`
  doesn't work on library targets. So the active checker's `[lib]` was kept commented out in `Cargo.toml`
  and uncommented by hand to build it:
  ```toml
  #[lib]
  #crate-type = ["cdylib"]
  #name="omclone_habcheck"
  #path = "src/hab_checker.rs"
  ```
- **A `bindgen` feature** pulled in the optional dependencies `wasm-bindgen`, `web-sys` and
  `console_error_panic_hook`. Nothing else used them, so they were removed along with the checkers.
- **Crate layout.** Each checker file was its own crate root and declared `mod parser; mod sim;` itself.
  Since then, `src/lib.rs` has made `sim`, `parser` and `timeline` a library that the binaries `use`
  (`use omclone::{parser, sim};`), so a new checker should do that instead.
- **The puzzle was compiled in** with `include_bytes!("../<name>.puzzle")` from the repo root. `*.puzzle` is
  in `.gitignore`, so the file had to be force-added (CX) or was left uncommitted (Habitability).

### Rust ↔ JavaScript

- **Exports:** `#[wasm_bindgen] pub fn` functions.
  - Habitability: `init_sol(&[u8])`, `run_test(first, variants: &[usize], test_area) -> OutputStats`,
    `setup_stats()`.
  - CX: `evaluate_solution(&[u8])`, `random_test(&[u8], &[usize]) -> bool`.
- **Startup:** Habitability used `#[wasm_bindgen(start)]` to parse the embedded puzzle once and install
  `console_error_panic_hook`.
- **State between calls:** `static Mutex<Option<FullPuzzle>>` and `Mutex<Option<FullSolution>>`.
- **Results:** `#[wasm_bindgen] #[repr(C)] pub struct OutputStats { completed, cycles, cost, area }`,
  returned by value.
- **Output callbacks into the page:**
  - Habitability used `#[wasm_bindgen(inline_js = "export function add_text(str){...}")]`. wasm-bindgen
    emits that into `snippets/`.
  - CX declared bare `extern "C" { fn add_text(s: &str); fn add_square(white: bool); }` and let the page
    define them as globals, which needs the `no-modules` target.

### Builds

- **Habitability:** `wasm-pack build --target web` with the `bindgen` feature enabled writes
  `pkg/omclone_habcheck.js`, `omclone_habcheck_bg.wasm`, `.d.ts` files and `snippets/`. These were copied
  into `html/`, which is gitignored apart from the pages. The page loads them with
  `import init, {...} from './omclone_habcheck.js'`, which is an ES module, so it has to be served over
  HTTP rather than opened as a file.
- **CX:** `wasm-pack build --target no-modules` produces a script that defines a global `wasm_bindgen`. The
  page loaded it with a plain `<script>` and called `await wasm_bindgen('omclone_cxchecker_bg.wasm')`.
- **The main app's web build is different.** `cargo build --target wasm32-unknown-unknown` with miniquad's
  `gl.js` and `sapp_jsutils` doesn't use wasm-bindgen at all. Its `.cargo/config.toml` flag
  `link-arg=--allow-undefined` applies to every wasm32 build in this repo, including wasm-pack ones.

### Pages

Each page did the following:

1. A file input reads the solution into a `Uint8Array` and passes it to Rust.
2. An ordered runner and a random runner loop over variants, yielding with `setTimeout` between batches so
   the page stays responsive and the runners can be stopped with a checkbox.
3. A grid of `.`/`X` (or `O`/`X`) shows each variant's result, along with the failure messages.
4. Worst-case stats are tracked in JS (Habitability) or in Rust (CX).

## How a check runs (current sim API)

```rust
let mut puzzle = parser::parse_puzzle(&mut puzzle_bytes)?;
let solution = parser::parse_solution(&mut solution_bytes)?;
//Swap in this run's variants
puzzle.inputs[i] = input_variants;   //AtomPattern: one molecule (Vec<Atom>) per variant
puzzle.outputs[j] = output_variants;
let init = parser::puzzle_prep(&puzzle, &solution)?;
let mut world = World::setup_sim(&init)?;
let mut motion = WorldStepInfo::new();
while !world.is_complete() {
    world.run_step(true, &mut motion)?; //a SimError's Display gives the error and where
    if world.timestep > STEP_CAP { /* report as a failure */ }
}
let stats = world.get_stats();
```

- **Variant order.** An input spawns its variants in list order and then cycles. An output accepts *only
  its current variant* and moves to the next one after each product. So an output's variant list must be
  in the order the solution will produce them, which for these puzzles is the order of the inputs.
- **Building atoms.** Use `Atom::new(pos, atom_type)`, then set `connections` for bonds. Struct literals
  no longer work, because atoms carry `immutable_wheel` and `molecule`. Pattern positions are relative to
  the glyph, as in the puzzle file.
- **Collisions need `mark_area`.** `run_step(false, ..)` skips area tracking and also the swept collision
  checks: only atoms ending up in the same hex are caught. Habitability's "Check area" box was off by
  default, so it could pass solutions that collide. Pass `true` unless speed matters more than
  correctness.
- **How many products.** Each output needs 6 × `output_multiplier` products, however many variants you give
  it. Variants past that count are never reached (Habitability passed exactly 6 per run). To check more,
  run more worlds.
- **Stats.** `get_stats()` counts cycles from the first instruction. Area is only meaningful with
  `mark_area` on. Cost and instructions come from setup.
- **Error handling.** `puzzle_prep` and `setup_sim` return `Result` (bad indices, unmatched conduits,
  malformed polymers). The checkers `unwrap()`ed these, which aborts on wasm, so report them instead.

## Adding a new checker

1. **Keep the logic native and tested.** Put the variant generation and the run loop above in a plain Rust
   module: something like `fn variants(id) -> (AtomPattern, AtomPattern)` and
   `fn run_variant(...) -> Result<SolutionStats, String>`. Cover it with `cargo test`, either with
   hand-built worlds like the ones in `src/test.rs` or with a known solution under `test/`. Explain in a
   comment how variant ids map to atoms; CX went by ring position (`rot_to_pos(idx)`), Habitability by
   atoms sorted by x.
2. **Choose a front end.**
   - **Native command line (simplest).** Add a `[[bin]]` with a feature in `required-features`, as the
     `omclone` binary does with `display_ui`. Unlike `[lib]`, `required-features` works for binaries. Read the puzzle and
     solution paths from the arguments, or from `src/default_paths.txt` as the app's `default_paths` (in `src/ui.rs`)
     does: `build.rs` sets `OMCLONE_DEFAULT_PATHS` for every target in the package. No JS toolchain is needed.
   - **Inside the app.** Add a window to the existing UI. It works natively and in the existing web build
     with no new tooling, but a long sweep has to be split across frames (a few variants per `update()`)
     so the UI doesn't freeze.
   - **Standalone web page, as before.** Restore the `bindgen` dependencies and build with `wasm-pack`. The
     package's one `[lib]` is now the `omclone` library (`src/lib.rs`), so make the checker its own small
     crate with a `cdylib` target that depends on `omclone`: a workspace member like `verify/`, the omsim-style
     verifier library, which builds for wasm too.
3. **The puzzle file.** Either load it at runtime, or compile it in with `include_bytes!` and force-add it
   (`git add -f`), since `*.puzzle` is gitignored.
4. **Results to report.** Show per-variant pass/fail with the failing step and error message, and the
   worst cycles, cost and area across variants.
5. **Documentation.** Add a line about building and running it to `README`.
