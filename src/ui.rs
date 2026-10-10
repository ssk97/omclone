use miniquad::*;
use crate::{sim::*, parser, render_sim::*, render_library::GFXPos, timeline::Timeline};
use smallvec::{SmallVec, smallvec};
use std::{io::BufReader, io::prelude::*};

#[cfg(not(target_arch = "wasm32"))]
use std::{fs::File, path::Path};

#[cfg(feature = "color-eyre")]
use color_eyre::eyre::Result;
#[cfg(not(feature = "color-eyre"))]
use simple_eyre::eyre::Result;

mod board;
mod layout_edit;
mod loaded;
mod tape_grid;
mod undo;
mod windows;
use board::*;
use layout_edit::*;
use loaded::{Loaded, RunState};
use tape_grid::{TapeSelection, longest_tape};
use undo::UndoHistory;
use windows::PathText;

struct PathInfo{
    base: String,
    puzzle: String,
    solution: String,
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
}

impl MyMiniquadApp {
    pub fn new() -> Self {
        let mut mq_ctx = window::new_rendering_backend();
        let render_data = RenderDataBase::new(mq_ctx.as_mut());
        let (base_str, puzzle_str, solution_str) = omclone::default_paths();
        let base = String::from(base_str);
        let puzzle = String::from(puzzle_str);
        let solution = String::from(solution_str);
        let app_state = AppState::NotLoaded;
        let unloaded_info = PathInfo{base, puzzle, solution};
        let egui_mq = egui_miniquad::EguiMq::new(mq_ctx.as_mut());
        Self {
            mq_ctx, egui_mq,
            render_data,app_state,unloaded_info,
            extra_message:None, dragging: None, right_press: None, mouse: (0., 0.), keymods: KeyMods::default(),
        }
    }
    pub fn load(&mut self, f_puzzle: impl Read, f_sol: impl Read) -> Result<()>{
        let puzzle = parser::parse_puzzle(&mut BufReader::new(f_puzzle))?;
        let solution = parser::parse_solution(&mut BufReader::new(f_sol))?;
        println!("Check: {:?}", solution.stats);
        let init = parser::puzzle_prep(&puzzle, &solution)?;
        let mut max_timestep = 0;
        for (a_num, a) in init.arms.iter().enumerate(){
            println!("Arms {:02}: {:?}", a_num, a.instruction_tape.to_string());
            max_timestep = max_timestep.max(a.instruction_tape.first)
        }
        let world = World::setup_sim(&init)?;
        max_timestep += world.repeat_length*2+100;

        let camera = CameraSetup::frame_center(&world, logical_screen_size());
        self.app_state = Loaded(Loaded::new(world, solution, max_timestep, camera));
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


const EDITOR_ENABLED: bool = cfg!(feature = "editor_ui");

impl EventHandler for MyMiniquadApp {
    fn update(&mut self) {
        if let Loaded(loaded) = &mut self.app_state{
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
                    windows::camera_window(egui_ctx, loaded, screen_size);
                    #[cfg(feature = "editor_ui")]
                    windows::file_window(egui_ctx, loaded, &mut self.unloaded_info);
                    windows::arms_window(egui_ctx, loaded, &mut self.extra_message);
                    windows::part_window(egui_ctx, loaded);
                    windows::cursor_tooltip(egui_ctx, loaded);
                    /*egui::Window::new("debug").min_width(128.).show(egui_ctx, |ui|{
                        ui.label(format!("curr time: {}",loaded.timeline.time() as u64));
                    });*/
                    do_loading = windows::reload_window(egui_ctx, loaded);
                }
                NotLoaded => {
                    do_loading = windows::not_loaded_window(egui_ctx, &mut self.unloaded_info);
                }
            };
        });
        self.egui_mq.draw(ctx);
        match do_loading{
            AppStateUpdate::NoChange => {},
            AppStateUpdate::ResetHome => {
                self.app_state = NotLoaded;
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
                        let name = JS_SOLUTION_NAME.lock().unwrap().take();
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
        self.egui_mq.mouse_motion_event( x, y);
        let (x, y) = logical(x, y);
        self.mouse = (x, y);
        if let Loaded(loaded) = &mut self.app_state{
            if let Some(drag_last) = &mut self.dragging{
                loaded.camera.pan_pixels(x-drag_last[0], y-drag_last[1]);
                *drag_last = [x,y];
            }
            let over_ui = self.egui_mq.egui_ctx().is_pointer_over_egui();
            loaded.pointer_moved((x, y), logical_screen_size(), over_ui);
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
        self.egui_mq.mouse_button_down_event(mb, x, y);
        let (x, y) = logical(x, y);
        if !self.egui_mq.egui_ctx().is_pointer_over_egui(){
            match mb {
                MouseButton::Right | MouseButton::Middle => {
                    self.dragging = Some([x,y]);
                    if mb == MouseButton::Right{
                        self.right_press = Some((x, y));
                    }
                }
                MouseButton::Left => if let Loaded(loaded) = &mut self.app_state{
                    loaded.press((x, y), logical_screen_size(), self.keymods);
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
        self.egui_mq.mouse_button_up_event(mb, x, y);
        let (x, y) = logical(x, y);
        match mb {
            MouseButton::Left => if let Loaded(loaded) = &mut self.app_state{
                loaded.release();
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
            //themselves, so it happens in order with the keys typed before it.
            let ctx = self.egui_mq.egui_ctx();
            let typing = ctx.text_edit_focused() || tape_grid::has_focus(ctx);
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
        self.egui_mq.key_up_event(keycode, keymods);
    }
}
