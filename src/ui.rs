use miniquad::*;
use crate::{sim::*, parser, rules, render_sim::*, render_library::{Color, GFXPos}, timeline::Timeline};
use smallvec::{SmallVec, smallvec};
use std::{io::BufReader, io::prelude::*, path::PathBuf};

#[cfg(not(target_arch = "wasm32"))]
use std::{fs::File, path::Path};

#[cfg(feature = "color-eyre")]
use color_eyre::eyre::Result;
#[cfg(not(feature = "color-eyre"))]
use simple_eyre::eyre::Result;

mod board;
mod layout_edit;
mod loaded;
mod recovery;
mod tape_grid;
mod undo;
mod windows;
use board::*;
use layout_edit::*;
use loaded::{AreaMode, Loaded, RunState};
use tape_grid::{TapeSelection, longest_tape};
use undo::UndoHistory;
use windows::PathText;

struct PathInfo{
    base: String,
    puzzle: String,
    solution: String,
}
//Where Save writes the solution (on the web, the name it downloads as)
fn solution_path(info: &PathInfo) -> PathBuf{
    if cfg!(target_arch = "wasm32") {
        PathBuf::from(&info.solution)
    } else {
        std::path::Path::new(&info.base).join(&info.solution)
    }
}
enum AppState{
    NotLoaded,Loaded(Loaded) 
}
enum AppStateUpdate{
    NoChange, Load, ResetHome
}
use AppState::*;

pub struct MyMiniquadApp {
    mq_ctx: Box<dyn RenderingBackend>,
    egui_mq: egui_miniquad::EguiMq,
    render_data: RenderDataBase,
    app_state: AppState,
    unloaded_info: PathInfo,
    extra_message: Option<String>,
    dragging: Option<GFXPos>,
    //Where the right button went down, to tell a click from panning
    right_press: Option<(f32, f32)>,
    //The last mouse position, which the wheel zooms around
    mouse: (f32, f32),
    //As of the last key event
    keymods: KeyMods,
    //A mouse button was released since egui's last frame, and where the mouse has moved since (see mouse_motion_event)
    released: bool,
    held_motion: Option<(f32, f32)>,
}

impl MyMiniquadApp {
    pub fn new() -> Self {
        //Here rather than in main, as miniquad replaces the panic hook on the web before this runs
        recovery::install_panic_hook();
        let mut mq_ctx = window::new_rendering_backend();
        let render_data = RenderDataBase::new(mq_ctx.as_mut());
        let (base_str, puzzle_str, solution_str) = omclone::default_paths();
        let base = String::from(base_str);
        let puzzle = String::from(puzzle_str);
        let solution = String::from(solution_str);
        let app_state = AppState::NotLoaded;
        let unloaded_info = PathInfo{base, puzzle, solution};
        let egui_mq = egui_miniquad::EguiMq::new(mq_ctx.as_mut());
        //A press anywhere else applies a text field being typed in, even one that starts a drag (egui's default
        //waits for a click). The board waits for this before acting on the press (see Loaded::board_input).
        egui_mq.egui_ctx().options_mut(|o| o.input_options.surrender_focus_on = egui::SurrenderFocusOn::Presses);
        Self {
            mq_ctx, egui_mq,
            render_data,app_state,unloaded_info,
            extra_message:None, dragging: None, right_press: None, mouse: (0., 0.), keymods: KeyMods::default(),
            released: false, held_motion: None,
        }
    }
    pub fn load(&mut self, f_puzzle: impl Read, f_sol: impl Read) -> Result<()>{
        let puzzle = parser::parse_puzzle(&mut BufReader::new(f_puzzle))?;
        let solution = parser::parse_solution(&mut BufReader::new(f_sol))?;
        let init = parser::puzzle_prep(&puzzle, &solution)?;
        let last_start = init.arms.iter().map(|a| a.instruction_tape.first).max().unwrap_or(0);
        let world = World::setup_sim(&init)?;
        let max_timestep = last_start + world.repeat_length*2+100;

        let camera = CameraSetup::frame_center(&world, logical_screen_size());
        self.app_state = Loaded(Loaded::new(world, solution, puzzle, max_timestep, camera));
        //Unsaved changes to the solution it replaced were discarded
        recovery::clear();
        Ok(())
    }
    //A solution names its puzzle by the puzzle's file name, which loading doesn't check
    #[cfg(not(target_arch = "wasm32"))]
    fn puzzle_mismatch(&self) -> Option<String>{
        let Loaded(loaded) = &self.app_state else {return None};
        let wanted = &loaded.solution.puzzle_name;
        let file = Path::new(&self.unloaded_info.puzzle).file_stem()?.to_string_lossy();
        (!file.eq_ignore_ascii_case(wanted)).then(||
            format!("The solution is for puzzle \"{wanted}\", but the puzzle file is \"{file}\". Check that they match."))
    }
}
//A mouse position in logical pixels (see logical_screen_size)
fn logical(x: f32, y: f32) -> (f32, f32){
    let dpi = window::dpi_scale();
    (x/dpi, y/dpi)
}


#[cfg(target_arch = "wasm32")]
static JS_PUZZLE_INPUT: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);
#[cfg(target_arch = "wasm32")]
static JS_SOLUTION_INPUT: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);
#[cfg(target_arch = "wasm32")]
static JS_SOLUTION_NAME: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn js_load_puzzle(puzzle: sapp_jsutils::JsObject){
    let mut puzzle_vec = Vec::new();
    puzzle.to_byte_buffer(&mut puzzle_vec);
    *JS_PUZZLE_INPUT.lock().unwrap() = Some(puzzle_vec);
}
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn js_load_solution(solution: sapp_jsutils::JsObject){
    let mut solution_vec = Vec::new();
    solution.to_byte_buffer(&mut solution_vec);
    *JS_SOLUTION_INPUT.lock().unwrap() = Some(solution_vec);
}
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn js_load_solution_name(name: sapp_jsutils::JsObject){
    let mut sol_name = String::new();
    name.to_string(&mut sol_name);
    *JS_SOLUTION_NAME.lock().unwrap() = Some(sol_name);
}
//Whether the loaded solution has unsaved changes, so the page can warn before it's closed
#[cfg(target_arch = "wasm32")]
static UNSAVED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn js_has_unsaved() -> bool{
    UNSAVED.load(std::sync::atomic::Ordering::Relaxed)
}
#[cfg(target_arch = "wasm32")]
extern "C" {
    fn save_file(filedata: sapp_jsutils::JsObject, filename: sapp_jsutils::JsObject);
}
//Has the browser download a file
#[cfg(target_arch = "wasm32")]
fn download(data: &[u8], name: &str){
    let filedata = sapp_jsutils::JsObject::buffer(data);
    let filename = sapp_jsutils::JsObject::string(name);
    unsafe {save_file(filedata, filename);}
}


const EDITOR_ENABLED: bool = cfg!(feature = "editor_ui");

impl EventHandler for MyMiniquadApp {
    fn update(&mut self) {
        if let Loaded(loaded) = &mut self.app_state{
            //Before simulating, so a crash running an edit still has that edit
            loaded.update_backup(|| solution_path(&self.unloaded_info));
            loaded.update();
            loaded.refresh_preview();
            let timeline = &loaded.timeline;
            let display_portion = timeline.time().fract() as f32;
            if timeline.motion().arms.is_empty(){
                loaded.float_world.generate_static(timeline.curr());
            } else {
                loaded.float_world.regenerate(timeline.curr(), timeline.motion(), display_portion);
            }
        }
        #[cfg(target_arch = "wasm32")]
        UNSAVED.store(matches!(&self.app_state, Loaded(loaded) if loaded.dirty), std::sync::atomic::Ordering::Relaxed);
    }

    fn draw(&mut self) {
        let ctx = self.mq_ctx.as_mut();
        ctx.clear(Some((1., 1., 1., 1.)), None, None);
        ctx.begin_default_pass(PassAction::clear_color(0.5, 0.5, 0.5, 1.0));
        if let Loaded(loaded) = &self.app_state{
            loaded.draw_world(&mut self.render_data, ctx);
        }
        ctx.end_render_pass();
        
        let mut do_loading = AppStateUpdate::NoChange;
        let screen_size = logical_screen_size();
        self.egui_mq.run(ctx, |_mq_ctx, egui_ctx|{
            windows::message_window(egui_ctx, "app_message", &mut self.extra_message);
            match &mut self.app_state{
                Loaded(loaded) => {
                    windows::timeline_window(egui_ctx, loaded);
                    windows::message_window(egui_ctx, "world_message", &mut loaded.message);
                    windows::crash_window(egui_ctx, loaded);
                    windows::camera_window(egui_ctx, loaded, screen_size);
                    #[cfg(feature = "editor_ui")]
                    windows::file_window(egui_ctx, loaded, &mut self.unloaded_info);
                    windows::arms_window(egui_ctx, loaded, &mut self.extra_message);
                    windows::part_window(egui_ctx, loaded);
                    windows::cursor_tooltip(egui_ctx, loaded);
                    do_loading = windows::reload_window(egui_ctx, loaded);
                }
                NotLoaded => {
                    do_loading = windows::not_loaded_window(egui_ctx, &mut self.unloaded_info);
                }
            };
        });
        //Movement held back from that frame goes into the next one
        self.released = false;
        if let Some((x, y)) = self.held_motion.take(){
            self.egui_mq.mouse_motion_event(x, y);
        }
        //egui has now applied any text field that the board input waited for
        if let Loaded(loaded) = &mut self.app_state{
            loaded.flush_board_input();
        }
        self.egui_mq.draw(ctx);
        match do_loading{
            AppStateUpdate::NoChange => {},
            AppStateUpdate::ResetHome => {
                self.app_state = NotLoaded;
                recovery::clear();
            }
            AppStateUpdate::Load => {
                //Loads, returning a warning if there's something to check
                let mut do_load = || -> Result<Option<String>>{
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let dat = &self.unloaded_info;
                        let base_path = Path::new(&dat.base);
                        let f_puzzle = File::open(base_path.join(&dat.puzzle))?;
                        let solution_path = base_path.join(&dat.solution);
                        let f_sol = File::open(&solution_path)?;
                        self.load(f_puzzle, f_sol)?;
                        Ok(self.puzzle_mismatch())
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        let puzzle = JS_PUZZLE_INPUT.lock().unwrap();
                        let f_puzzle = puzzle.as_ref().unwrap();
                        let solution = JS_SOLUTION_INPUT.lock().unwrap();
                        let f_sol = solution.as_ref().unwrap();
                        //Kept, like the files, for loading again after a Reset
                        let name = JS_SOLUTION_NAME.lock().unwrap().clone();
                        self.unloaded_info.solution = name.unwrap_or(String::from("omclone.solution"));
                        self.load(f_puzzle.as_slice(), f_sol.as_slice())?;
                        Ok(None)
                    }
                };

                match do_load(){
                    Err(err) => {
                        self.extra_message = Some(err.to_string());
                        #[cfg(target_arch = "wasm32")]
                        {
                            *JS_SOLUTION_INPUT.lock().unwrap() = None;
                        }
                    }
                    Ok(warning) => self.extra_message = warning,
                }
            }
        }

        // Draw things in front of egui here
        self.mq_ctx.commit_frame();
    }

    fn mouse_motion_event(&mut self, x: f32, y: f32) {
        //egui gives a click, or the end of a drag, the pointer's position at the end of its frame. In a slow frame the
        //mouse can move on after the button comes up, so egui only sees that movement in the next frame.
        if self.released{
            self.held_motion = Some((x, y));
        } else {
            self.egui_mq.mouse_motion_event(x, y);
        }
        let (x, y) = logical(x, y);
        self.mouse = (x, y);
        if let Loaded(loaded) = &mut self.app_state{
            if let Some(drag_last) = &mut self.dragging{
                loaded.camera.pan_pixels(x-drag_last[0], y-drag_last[1]);
                *drag_last = [x,y];
            }
            let egui_ctx = self.egui_mq.egui_ctx();
            let input = BoardInput::Move{px: (x, y), screen: logical_screen_size(), over_ui: egui_ctx.is_pointer_over_egui()};
            loaded.board_input(input, egui_ctx.text_edit_focused());
        }
    }

    fn mouse_wheel_event(&mut self, dx: f32, dy: f32) {
        //egui takes wheel movement as lines, a notch each. miniquad gives them on Linux, but Windows' raw units are
        //120 a notch, miniquad multiplies macOS wheel mice by 10, and browsers report around 100 pixels a notch.
        let per_line = if cfg!(windows) {120.} else if cfg!(target_os = "macos") {10.}
            else if cfg!(target_arch = "wasm32") {100.} else {1.};
        self.egui_mq.mouse_wheel_event(dx / per_line, dy / per_line);
        if let Loaded(loaded) = &mut self.app_state{
            //While a part is held, the wheel picks between the parts under it
            if loaded.wheel(dy / per_line){
                return;
            }
        }
        if !self.egui_mq.egui_ctx().is_pointer_over_egui(){
            if let Loaded(loaded) = &mut self.app_state{
                loaded.camera.zoom_at(self.mouse, logical_screen_size(), dy / per_line);
            }
        }
    }

    fn mouse_button_down_event(
        &mut self,
        mb: MouseButton,
        x: f32,
        y: f32,
    ) {
        //The press is where the mouse is now, later than any movement held back
        self.held_motion = None;
        self.egui_mq.mouse_button_down_event(mb, x, y);
        let (x, y) = logical(x, y);
        let egui_ctx = self.egui_mq.egui_ctx();
        if !egui_ctx.is_pointer_over_egui(){
            match mb {
                MouseButton::Right | MouseButton::Middle => {
                    self.dragging = Some([x,y]);
                    if mb == MouseButton::Right{
                        self.right_press = Some((x, y));
                    }
                }
                MouseButton::Left => if let Loaded(loaded) = &mut self.app_state{
                    let input = BoardInput::Press{px: (x, y), screen: logical_screen_size(), keymods: self.keymods};
                    loaded.board_input(input, egui_ctx.text_edit_focused());
                },
                _ => {}
            }
        }
    }

    fn mouse_button_up_event(
        &mut self,
        mb: MouseButton,
        x: f32,
        y: f32,
    ) {
        self.held_motion = None;
        self.egui_mq.mouse_button_up_event(mb, x, y);
        self.released = true;
        let (x, y) = logical(x, y);
        match mb {
            MouseButton::Left => if let Loaded(loaded) = &mut self.app_state{
                loaded.board_input(BoardInput::Release, self.egui_mq.egui_ctx().text_edit_focused());
            },
            _ => {
                self.dragging = None;
                let clicked = self.right_press.take().is_some_and(|(px, py)|
                    (x-px)*(x-px) + (y-py)*(y-py) <= DRAG_THRESHOLD_PX*DRAG_THRESHOLD_PX);
                if let (MouseButton::Right, true, Loaded(loaded)) = (mb, clicked, &mut self.app_state){
                    loaded.right_click();
                }
            }
        }
    }

    fn char_event(
        &mut self,
        character: char,
        _keymods: KeyMods,
        _repeat: bool,
    ) {
        //Hotkeys for a held part aren't typed anywhere
        if let Loaded(loaded) = &self.app_state{
            let ctx = self.egui_mq.egui_ctx();
            if loaded.tool.press().is_some() && !ctx.text_edit_focused() && !tape_grid::has_focus(ctx){
                return;
            }
        }
        self.egui_mq.char_event(character);
    }

    fn key_down_event(
        &mut self,
        keycode: KeyCode,
        keymods: KeyMods,
        _repeat: bool,
    ) {
        self.keymods = keymods;
        if let Loaded(loaded) = &mut self.app_state{
            //Typing in a text box or the tapes takes priority over the part hotkeys. The tapes handle undo
            //themselves, so it happens in order with the keys typed before it. 1 isn't an instruction, so it still
            //fades the atoms from the tapes.
            let ctx = self.egui_mq.egui_ctx();
            let typing = ctx.text_edit_focused() || (tape_grid::has_focus(ctx) && keycode != KeyCode::Key1);
            if !typing && loaded.handle_key(keycode, keymods){
                return;
            }
        }
        self.egui_mq.key_down_event(keycode, keymods);
    }

    fn key_up_event(&mut self, keycode: KeyCode, keymods: KeyMods) {
        self.keymods = keymods;
        //The modifiers may still count the key being released
        match keycode{
            KeyCode::LeftShift | KeyCode::RightShift => self.keymods.shift = false,
            KeyCode::LeftControl | KeyCode::RightControl => self.keymods.ctrl = false,
            KeyCode::LeftSuper | KeyCode::RightSuper => self.keymods.logo = false,
            _ => {}
        }
        //Even while typing, which may have started after 1 was pressed
        if let (KeyCode::Key1, Loaded(loaded)) = (keycode, &mut self.app_state){
            loaded.atoms_faded = false;
        }
        self.egui_mq.key_up_event(keycode, keymods);
    }

    //Called when the window loses focus, after which 1's release may go elsewhere
    fn window_minimized_event(&mut self) {
        if let Loaded(loaded) = &mut self.app_state{
            loaded.atoms_faded = false;
        }
    }
}
