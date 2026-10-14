"""Checks omclone's verifier library through its C API (the same one omsim's libverify has).

  compare.py diff OURS OMSIM PUZZLES SOLUTIONS... [--out FILE] [--limit N]
      Evaluates every @V metric for every solution with both libraries and reports where they differ.
  compare.py leaderboard OURS PUZZLES LEADERBOARD [--out FILE]
      Checks the leaderboard records' @V scores (cost, instructions, overlap, trackless, cycles, area, height, width,
      boundingHex) against OURS, as omsim's test-against-leaderboard.py does.

Puzzles are found by file name under PUZZLES, solutions under each directory given (both searched recursively).
Run with -I, so nothing next to the solutions can be imported.
"""
import argparse
import json
import sys
import time
from concurrent.futures import ProcessPoolExecutor
from ctypes import CDLL, POINTER, byref, c_char, c_char_p, c_double, c_int, c_void_p
from pathlib import Path

#Every metric verifier.c measures at victory (the @∞ ones need loop detection, which omclone doesn't have)
METRICS = [
    "parsed cycles", "parsed cost", "parsed area", "parsed instructions", "number of track segments", "cost",
    "duplicate reagents", "duplicate products", "maximum track gap^2", "maximum absolute part coordinate",
    "parts of type arm1", "parts of type track",
    "instructions", "instructions with hotkey b", "instructions with hotkey grf", "instruction tape period",
    "number of arms", "cabinet violations", "conduit violations", "track self-overlap", "track loops", "overlap",
    "cycles", "area", "height at 0 degrees", "height at 60 degrees", "height at 120 degrees",
    "width*2 at 0 degrees", "width*2 at 60 degrees", "width*2 at 120 degrees", "height", "width*2",
    "minimum hexagon", "executed instructions", "instruction executions",
    "instruction executions with hotkey d", "instruction executions with hotkey a",
    "instruction executions with hotkey w", "instruction executions with hotkey s",
    "instruction executions with hotkey r", "instruction executions with hotkey f",
    "instruction executions with hotkey e", "instruction executions with hotkey q",
    "instruction executions with hotkey g", "instruction executions with hotkey t",
    "atoms grabbed", "atoms grabbed of type salt", "number of atoms", "number of atoms of type salt",
    "product 1 cycles", "product 2 area", "cycle 10 area", "cycle 10 atoms grabbed",
]


def load(path):
    lib = CDLL(str(Path(path).resolve()))
    lib.verifier_create_from_bytes.restype = c_void_p
    lib.verifier_create_from_bytes.argtypes = [c_char_p, c_int, c_char_p, c_int]
    lib.verifier_destroy.argtypes = [c_void_p]
    lib.verifier_error.restype = c_char_p
    lib.verifier_error.argtypes = [c_void_p]
    lib.verifier_error_cycle.argtypes = [c_void_p]
    lib.verifier_error_clear.argtypes = [c_void_p]
    lib.verifier_evaluate_metric.argtypes = [c_void_p, c_char_p]
    lib.verifier_evaluate_approximate_metric.restype = c_double
    lib.verifier_evaluate_approximate_metric.argtypes = [c_void_p, c_char_p]
    lib.verifier_find_puzzle_name_in_solution_bytes.restype = POINTER(c_char)
    lib.verifier_find_puzzle_name_in_solution_bytes.argtypes = [c_char_p, c_int, POINTER(c_int)]
    return lib


def find_puzzles(directory):
    return {path.stem: path for path in Path(directory).rglob("*.puzzle")}


def puzzle_name(lib, solution_bytes):
    length = c_int()
    name = lib.verifier_find_puzzle_name_in_solution_bytes(solution_bytes, len(solution_bytes), byref(length))
    return name[:length.value].decode("utf-8", "replace") if name else None


def measure(lib, puzzle_bytes, solution_bytes, metrics):
    """Each metric's (value, error, error cycle), with the error cleared before each"""
    v = lib.verifier_create_from_bytes(puzzle_bytes, len(puzzle_bytes), solution_bytes, len(solution_bytes))
    results = {}
    for metric in metrics:
        lib.verifier_error_clear(v)
        value = lib.verifier_evaluate_metric(v, metric.encode())
        error = lib.verifier_error(v)
        results[metric] = (value, error.decode("utf-8", "replace") if error else None, lib.verifier_error_cycle(v))
    lib.verifier_destroy(v)
    return results


#Each worker process loads the libraries once
_libs = {}


def diff_one(job):
    ours_path, omsim_path, puzzle_path, solution_path = job
    if not _libs:
        _libs["ours"], _libs["omsim"] = load(ours_path), load(omsim_path)
    puzzle_bytes, solution_bytes = Path(puzzle_path).read_bytes(), Path(solution_path).read_bytes()
    times = {}
    results = {}
    for name in ("ours", "omsim"):
        start = time.perf_counter()
        results[name] = measure(_libs[name], puzzle_bytes, solution_bytes, METRICS)
        times[name] = time.perf_counter() - start
    differences = []
    for metric in METRICS:
        ours, omsim = results["ours"][metric], results["omsim"][metric]
        #Error texts differ by design, so only whether there is one counts (and the crash cycle, for both crashing)
        same = ours[0] == omsim[0] and (ours[1] is None) == (omsim[1] is None)
        if same and ours[1] and omsim[1] and ours[2] != omsim[2]:
            same = False
        if not same:
            differences.append((metric, ours, omsim))
    return solution_path, times, differences


def solution_jobs(args, puzzles, ours):
    jobs = []
    for directory in args.solutions:
        for path in sorted(Path(directory).rglob("*")):
            if not path.is_file() or ".solution" not in path.name.lower():
                continue
            name = puzzle_name(ours, path.read_bytes())
            if name in puzzles:
                jobs.append((args.ours, args.omsim, str(puzzles[name]), str(path)))
    return jobs[:args.limit] if args.limit else jobs


def diff(args):
    puzzles = find_puzzles(args.puzzles)
    jobs = solution_jobs(args, puzzles, load(args.ours))
    print(f"{len(jobs)} solutions with a puzzle")
    totals = {"ours": 0.0, "omsim": 0.0}
    by_metric = {}
    differing = 0
    with open(args.out, "w", encoding="utf-8") as out, ProcessPoolExecutor() as pool:
        for i, (path, times, differences) in enumerate(pool.map(diff_one, jobs, chunksize=8)):
            for name in totals:
                totals[name] += times[name]
            if differences:
                differing += 1
                out.write(f"{path}\n")
                for metric, ours, omsim in differences:
                    by_metric[metric] = by_metric.get(metric, 0) + 1
                    out.write(f"  {metric}: ours {ours} omsim {omsim}\n")
            if (i + 1) % 1000 == 0:
                print(f"{i + 1}/{len(jobs)}, {differing} differing", flush=True)
    print(f"{differing}/{len(jobs)} solutions differ somewhere; details in {args.out}")
    for metric in METRICS:
        if metric in by_metric:
            print(f"  {by_metric[metric]:6} {metric}")
    print(f"time (summed over workers): ours {totals['ours']:.1f}s, omsim {totals['omsim']:.1f}s")


def leaderboard_one(job):
    ours_path, puzzle_path, solution_path, score = job
    if not _libs:
        _libs["ours"] = load(ours_path)
    lib = _libs["ours"]
    puzzle_bytes, solution_bytes = Path(puzzle_path).read_bytes(), Path(solution_path).read_bytes()
    v = lib.verifier_create_from_bytes(puzzle_bytes, len(puzzle_bytes), solution_bytes, len(solution_bytes))

    def m(metric):
        return lib.verifier_evaluate_approximate_metric(v, metric.encode())

    measured = {
        "cost": lambda: m("cost"),
        "instructions": lambda: m("instructions"),
        "overlap": lambda: m("overlap") != 0,
        "trackless": lambda: m("number of track segments") == 0,
        "cycles": lambda: m("cycles"),
        "area": lambda: m("area"),
        "width": lambda: m("width*2") / 2,
        "height": lambda: m("height"),
        "boundingHex": lambda: m("minimum hexagon"),
    }
    mismatches = []
    checked = 0
    for metric, expected in score.items():
        if metric not in measured or expected is None:
            continue
        value = measured[metric]()
        checked += 1
        if value != expected and not (isinstance(expected, (int, float)) and value == float(expected)):
            error = lib.verifier_error(v)
            mismatches.append((metric, value, expected, error.decode("utf-8", "replace") if error else None))
    lib.verifier_destroy(v)
    return solution_path, checked, mismatches


def leaderboard(args):
    puzzles = find_puzzles(args.puzzles)
    ours = load(args.ours)
    jobs = []
    for path in sorted(Path(args.leaderboard).rglob("*.json")):
        record = json.loads(path.read_text(encoding="utf-8"))
        solution = Path(args.leaderboard) / record["dataPath"]
        name = puzzle_name(ours, solution.read_bytes()) if solution.exists() else None
        if name in puzzles:
            jobs.append((args.ours, str(puzzles[name]), str(solution), record["score"]))
    print(f"{len(jobs)} records with a puzzle")
    checked = 0
    failed = {}
    with open(args.out, "w", encoding="utf-8") as out, ProcessPoolExecutor() as pool:
        for i, (path, count, mismatches) in enumerate(pool.map(leaderboard_one, jobs, chunksize=8)):
            checked += count
            for metric, value, expected, error in mismatches:
                failed[metric] = failed.get(metric, 0) + 1
                out.write(f"{path} {metric}: measured {value} recorded {expected} error {error}\n")
            if (i + 1) % 1000 == 0:
                print(f"{i + 1}/{len(jobs)}", flush=True)
    print(f"{checked - sum(failed.values())}/{checked} recorded scores match; mismatches in {args.out}")
    for metric, count in sorted(failed.items()):
        print(f"  {count:6} {metric}")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    d = commands.add_parser("diff")
    d.add_argument("ours")
    d.add_argument("omsim")
    d.add_argument("puzzles")
    d.add_argument("solutions", nargs="+")
    d.add_argument("--out", default="compare-diff.txt")
    d.add_argument("--limit", type=int, default=0)
    lb = commands.add_parser("leaderboard")
    lb.add_argument("ours")
    lb.add_argument("puzzles")
    lb.add_argument("leaderboard")
    lb.add_argument("--out", default="compare-leaderboard.txt")
    args = parser.parse_args()
    if args.command == "diff":
        diff(args)
    else:
        leaderboard(args)


if __name__ == "__main__":
    sys.exit(main())
