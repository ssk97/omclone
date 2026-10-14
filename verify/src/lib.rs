//omsim's verifier API (verifier.h), for loading in its place, measuring with omclone's sim (see omclone::verifier).
//Like omsim, it doesn't check its arguments: a null verifier or string stops the process (with a message, here).
use omclone::verifier::{self, ErrorSource, Verifier};
use std::ffi::{c_char, c_double, c_int, c_void, CStr, CString};
use std::path::Path;
use std::ptr::null;

struct Handle {
    verifier: Verifier,
    //Every error description handed out, so the pointers stay valid until the verifier is destroyed
    descriptions: Vec<CString>,
}
unsafe fn handle<'a>(verifier: *mut c_void) -> &'a mut Handle {
    unsafe {(verifier as *mut Handle).as_mut()}.expect("null verifier")
}
unsafe fn bytes<'a>(data: *const c_char, length: c_int) -> &'a [u8] {
    match usize::try_from(length) {
        Ok(length) if length > 0 => unsafe {std::slice::from_raw_parts(data as *const u8, length)},
        _ => &[],
    }
}
unsafe fn string(text: *const c_char) -> String {
    assert!(!text.is_null(), "null string");
    unsafe {CStr::from_ptr(text)}.to_string_lossy().into_owned()
}
fn into_handle(verifier: Verifier) -> *mut c_void {
    Box::into_raw(Box::new(Handle{verifier, descriptions: Vec::new()})) as *mut c_void
}

//A `const char *` that C reads as a global variable
#[repr(transparent)]
pub struct ErrorSourceName(*const c_char);
unsafe impl Sync for ErrorSourceName {}
#[no_mangle]
pub static verifier_error_source_puzzle_file: ErrorSourceName = ErrorSourceName(c"puzzle file".as_ptr());
#[no_mangle]
pub static verifier_error_source_solution_file: ErrorSourceName = ErrorSourceName(c"solution file".as_ptr());
#[no_mangle]
pub static verifier_error_source_metric: ErrorSourceName = ErrorSourceName(c"metric".as_ptr());
#[no_mangle]
pub static verifier_error_source_simulation: ErrorSourceName = ErrorSourceName(c"simulation".as_ptr());

#[no_mangle]
pub unsafe extern "C" fn verifier_find_puzzle_name_in_solution_bytes(solution_bytes: *const c_char,
    solution_length: c_int, name_length: *mut c_int) -> *const c_char
{
    let Some(name) = verifier::find_puzzle_name(unsafe {bytes(solution_bytes, solution_length)}) else {return null()};
    if let Some(name_length) = unsafe {name_length.as_mut()} {
        *name_length = name.len() as c_int;
    }
    unsafe {solution_bytes.add(name.start)}
}

#[no_mangle]
pub unsafe extern "C" fn verifier_create(puzzle_filename: *const c_char, solution_filename: *const c_char) -> *mut c_void {
    let (puzzle, solution) = unsafe {(string(puzzle_filename), string(solution_filename))};
    into_handle(Verifier::from_files(Path::new(&puzzle), Path::new(&solution)))
}
#[no_mangle]
pub unsafe extern "C" fn verifier_create_from_bytes(puzzle_bytes: *const c_char, puzzle_length: c_int,
    solution_bytes: *const c_char, solution_length: c_int) -> *mut c_void
{
    let (puzzle, solution) = unsafe {(bytes(puzzle_bytes, puzzle_length), bytes(solution_bytes, solution_length))};
    into_handle(Verifier::from_bytes(puzzle, solution))
}
//The verifier keeps what it needs from the bytes, so this is the same as verifier_create_from_bytes
#[no_mangle]
pub unsafe extern "C" fn verifier_create_from_bytes_without_copying(puzzle_bytes: *const c_char, puzzle_length: c_int,
    solution_bytes: *const c_char, solution_length: c_int) -> *mut c_void
{
    unsafe {verifier_create_from_bytes(puzzle_bytes, puzzle_length, solution_bytes, solution_length)}
}
#[no_mangle]
pub unsafe extern "C" fn verifier_destroy(verifier: *mut c_void) {
    assert!(!verifier.is_null(), "null verifier");
    drop(unsafe {Box::from_raw(verifier as *mut Handle)});
}

#[no_mangle]
pub unsafe extern "C" fn verifier_set_cycle_limit(verifier: *mut c_void, cycle_limit: c_int) {
    unsafe {handle(verifier)}.verifier.set_cycle_limit(cycle_limit);
}
//The only function omsim lets take a null verifier
#[no_mangle]
pub unsafe extern "C" fn verifier_disable_limits(verifier: *mut c_void) {
    if let Some(handle) = unsafe {(verifier as *mut Handle).as_mut()} {
        handle.verifier.disable_limits();
    }
}

#[no_mangle]
pub unsafe extern "C" fn verifier_set_fails_on_wrong_output(verifier: *mut c_void, output_index: c_int,
    fails_on_wrong_output: c_int)
{
    unsafe {handle(verifier)}.verifier.set_fails_on_wrong_output(output_index, fails_on_wrong_output != 0);
}
#[no_mangle]
pub unsafe extern "C" fn verifier_set_fails_on_wrong_output_bonds(verifier: *mut c_void, output_index: c_int,
    fails_on_wrong_output_bonds: c_int)
{
    unsafe {handle(verifier)}.verifier.set_fails_on_wrong_output_bonds(output_index, fails_on_wrong_output_bonds != 0);
}
#[no_mangle]
pub unsafe extern "C" fn verifier_wrong_output_index(verifier: *mut c_void) -> c_int {
    unsafe {handle(verifier)}.verifier.wrong_output_index()
}
#[no_mangle]
pub unsafe extern "C" fn verifier_wrong_output_atom(verifier: *mut c_void, u: c_int, v: c_int) -> c_int {
    unsafe {handle(verifier)}.verifier.wrong_output_atom(u, v)
}
#[no_mangle]
pub unsafe extern "C" fn verifier_wrong_output_clear(verifier: *mut c_void) {
    unsafe {handle(verifier)}.verifier.wrong_output_clear();
}

#[no_mangle]
pub unsafe extern "C" fn verifier_error(verifier: *mut c_void) -> *const c_char {
    let handle = unsafe {handle(verifier)};
    let Some(error) = handle.verifier.error() else {return null()};
    //A description can't hold a NUL, but one quoting a file's contents could have one
    let description = error.description.replace('\0', " ");
    let index = match handle.descriptions.iter().position(|d| d.as_bytes() == description.as_bytes()) {
        Some(index) => index,
        None => {
            handle.descriptions.push(CString::new(description).expect("NULs were replaced"));
            handle.descriptions.len() - 1
        }
    };
    handle.descriptions[index].as_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn verifier_error_cycle(verifier: *mut c_void) -> c_int {
    unsafe {handle(verifier)}.verifier.error().map_or(0, |e| e.cycle)
}
#[no_mangle]
pub unsafe extern "C" fn verifier_error_location_u(verifier: *mut c_void) -> c_int {
    unsafe {handle(verifier)}.verifier.error().map_or(0, |e| e.location.x)
}
#[no_mangle]
pub unsafe extern "C" fn verifier_error_location_v(verifier: *mut c_void) -> c_int {
    unsafe {handle(verifier)}.verifier.error().map_or(0, |e| e.location.y)
}
#[no_mangle]
pub unsafe extern "C" fn verifier_error_source(verifier: *mut c_void) -> *const c_char {
    let Some(error) = unsafe {handle(verifier)}.verifier.error() else {return null()};
    match error.source {
        ErrorSource::PuzzleFile => verifier_error_source_puzzle_file.0,
        ErrorSource::SolutionFile => verifier_error_source_solution_file.0,
        ErrorSource::Metric => verifier_error_source_metric.0,
        ErrorSource::Simulation => verifier_error_source_simulation.0,
    }
}
#[no_mangle]
pub unsafe extern "C" fn verifier_error_clear(verifier: *mut c_void) {
    unsafe {handle(verifier)}.verifier.error_clear();
}

#[no_mangle]
pub unsafe extern "C" fn verifier_evaluate_metric(verifier: *mut c_void, metric: *const c_char) -> c_int {
    let metric = unsafe {string(metric)};
    unsafe {handle(verifier)}.verifier.evaluate_metric(&metric)
}
#[no_mangle]
pub unsafe extern "C" fn verifier_evaluate_approximate_metric(verifier: *mut c_void, metric: *const c_char) -> c_double {
    let metric = unsafe {string(metric)};
    unsafe {handle(verifier)}.verifier.evaluate_approximate_metric(&metric)
}

#[no_mangle]
pub unsafe extern "C" fn verifier_number_of_output_intervals(verifier: *mut c_void) -> c_int {
    unsafe {handle(verifier)}.verifier.number_of_output_intervals()
}
#[no_mangle]
pub unsafe extern "C" fn verifier_output_interval(verifier: *mut c_void, which_interval: c_int) -> c_int {
    unsafe {handle(verifier)}.verifier.output_interval(which_interval)
}
#[no_mangle]
pub unsafe extern "C" fn verifier_output_intervals_repeat_after(verifier: *mut c_void) -> c_int {
    unsafe {handle(verifier)}.verifier.output_intervals_repeat_after()
}

//omsim's Emscripten build exports these for JavaScript to make buffers in the module's memory. Each block starts with
//its size, which free needs.
#[cfg(target_arch = "wasm32")]
mod wasm_alloc {
    use std::alloc::{alloc, dealloc, Layout};
    const HEADER: usize = 8;
    fn layout(size: usize) -> Option<Layout> {
        Layout::from_size_align(size.checked_add(HEADER)?, HEADER).ok()
    }
    #[no_mangle]
    pub unsafe extern "C" fn malloc(size: usize) -> *mut u8 {
        let Some(layout) = layout(size) else {return std::ptr::null_mut()};
        let block = unsafe {alloc(layout)};
        if block.is_null() {
            return block;
        }
        unsafe {
            (block as *mut usize).write(size);
            block.add(HEADER)
        }
    }
    #[no_mangle]
    pub unsafe extern "C" fn free(data: *mut u8) {
        if data.is_null() {
            return;
        }
        unsafe {
            let block = data.sub(HEADER);
            let size = (block as *const usize).read();
            dealloc(block, layout(size).expect("it was allocated with this layout"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omclone::{parser, sim::*};
    use std::ptr::null_mut;

    //A puzzle whose reagent is one atom of `input` (numbered as in the file) and whose product is one salt
    fn puzzle_bytes(input: u8) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend(3i32.to_le_bytes());
        data.push(4);
        data.extend(b"test");
        data.extend(0u64.to_le_bytes());
        //Every part allowed
        data.extend(u64::MAX.to_le_bytes());
        for atom_type in [input, AtomType::Salt as u8] {
            //One molecule of one atom at (0,0), with no bonds
            data.extend(1i32.to_le_bytes());
            data.extend(1i32.to_le_bytes());
            data.extend([atom_type, 0, 0]);
            data.extend(0i32.to_le_bytes());
        }
        //Output multiplier, then not a production puzzle
        data.extend(1i32.to_le_bytes());
        data.push(0);
        data
    }
    //An arm carries each reagent from the input to the output in 4 cycles: grab, turn, drop, turn back
    fn solution_bytes() -> Vec<u8> {
        use Instr::*;
        let salt = || AtomPattern::from(vec![vec![Atom::new(Pos::new(0, 0), AtomType::Salt)]]);
        let world = World::setup_sim(&InitialWorld{
            glyphs: vec![
                Glyph{pos: Pos::new(0, 0), rot: 0, glyph_type: GlyphType::Input(salt(), 0)},
                Glyph{pos: Pos::new(-1, 1), rot: 1, glyph_type: GlyphType::Output(salt(), 6, 0)},
            ],
            arms: vec![Arm::new(Pos::new(-1, 0), 0, 1, ArmType::PlainArm,
                Tape::new(0, vec![Grab, RotateCounterClockwise, Drop, RotateClockwise]))],
            ..Default::default()
        }).unwrap();
        let mut data = Vec::new();
        parser::write_solution(&mut data, &parser::create_solution(&world, String::from("test"), String::from("mover"))).unwrap();
        data
    }
    unsafe fn create(puzzle: &[u8], solution: &[u8]) -> *mut c_void {
        unsafe {verifier_create_from_bytes(puzzle.as_ptr() as *const c_char, puzzle.len() as c_int,
            solution.as_ptr() as *const c_char, solution.len() as c_int)}
    }
    unsafe fn text(text: *const c_char) -> Option<String> {
        (!text.is_null()).then(|| unsafe {CStr::from_ptr(text)}.to_string_lossy().into_owned())
    }

    #[test]
    fn evaluates_and_reports_errors() {
        unsafe {
            let v = create(&puzzle_bytes(AtomType::Salt as u8), &solution_bytes());
            assert_eq!(verifier_evaluate_metric(v, c"cycles".as_ptr()), 23);
            assert_eq!(verifier_evaluate_approximate_metric(v, c"width".as_ptr()), 2.);
            assert!(verifier_error(v).is_null() && verifier_error_source(v).is_null());

            assert_eq!(verifier_evaluate_metric(v, c"bogus".as_ptr()), -1);
            let unknown = verifier_error(v);
            assert_eq!(text(unknown).as_deref(), Some("unknown metric"));
            //The exported name itself, so C can compare the pointers
            assert_eq!(verifier_error_source(v), verifier_error_source_metric.0);
            verifier_error_clear(v);
            assert!(verifier_error(v).is_null());
            assert_eq!(verifier_evaluate_metric(v, c"atoms grabbed of type unobtainium".as_ptr()), -1);
            assert_eq!(text(unknown).as_deref(), Some("unknown metric"), "earlier descriptions stay valid");
            verifier_destroy(v);
        }
    }

    #[test]
    fn a_wrong_product_crash_says_where() {
        unsafe {
            let v = create(&puzzle_bytes(AtomType::Air as u8), &solution_bytes());
            verifier_set_fails_on_wrong_output(v, 0, 1);
            assert_eq!(verifier_evaluate_metric(v, c"cycles".as_ptr()), -1);
            assert_eq!(text(verifier_error(v)).as_deref(), Some("output didn't match"));
            assert_eq!(verifier_error_source(v), verifier_error_source_simulation.0);
            assert_eq!((verifier_error_cycle(v), verifier_error_location_u(v), verifier_error_location_v(v)), (3, -1, 1));
            assert_eq!((verifier_wrong_output_index(v), verifier_wrong_output_atom(v, 0, 0)), (0, AtomType::Air as c_int));
            verifier_wrong_output_clear(v);
            assert_eq!(verifier_wrong_output_index(v), -1);
            verifier_destroy(v);
        }
    }

    #[test]
    fn unreadable_files_leave_a_verifier_that_only_reports_it() {
        unsafe {
            let v = create(&puzzle_bytes(AtomType::Salt as u8), b"not a solution");
            assert_eq!(text(verifier_error(v)).as_deref(), Some("invalid solution file"));
            assert_eq!(verifier_error_source(v), verifier_error_source_solution_file.0);
            assert_eq!((verifier_evaluate_metric(v, c"cost".as_ptr()), verifier_number_of_output_intervals(v)), (-1, 0));
            verifier_destroy(v);

            let missing = c"no such file.puzzle".as_ptr();
            let v = verifier_create(missing, missing);
            assert_eq!(text(verifier_error(v)).as_deref(), Some("invalid puzzle file"));
            verifier_destroy(v);

            //omsim allows this one function a null verifier
            verifier_disable_limits(null_mut());
        }
    }

    #[test]
    fn reads_files_by_path() {
        let dir = std::env::temp_dir().join(format!("omclone-verify-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (puzzle, solution) = (dir.join("test.puzzle"), dir.join("mover.solution"));
        std::fs::write(&puzzle, puzzle_bytes(AtomType::Salt as u8)).unwrap();
        std::fs::write(&solution, solution_bytes()).unwrap();
        let path = |p: &Path| CString::new(p.to_str().unwrap()).unwrap();
        unsafe {
            let v = verifier_create(path(&puzzle).as_ptr(), path(&solution).as_ptr());
            assert_eq!(verifier_evaluate_metric(v, c"cycles".as_ptr()), 23);
            verifier_destroy(v);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn finds_the_puzzle_name_in_the_callers_bytes() {
        let data = solution_bytes();
        let mut length = 0;
        unsafe {
            let name = verifier_find_puzzle_name_in_solution_bytes(data.as_ptr() as *const c_char, data.len() as c_int, &mut length);
            assert_eq!(std::slice::from_raw_parts(name as *const u8, length as usize), b"test");
            assert!(verifier_find_puzzle_name_in_solution_bytes(c"junk".as_ptr(), 4, &mut length).is_null());
        }
    }
}
