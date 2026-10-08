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
mod tape_buffer;
mod windows;
use board::*;
use layout_edit::*;
use loaded::{Loaded, RunState};
use tape_buffer::*;

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
}

impl MyMiniquadApp {
    pub fn new() -> Self {
        let mut mq_ctx = window::new_rendering_backend();
        let render_data = RenderDataBase::new(mq_ctx.as_mut());
        let (base_str, puzzle_str, solution_str) = get_default_path_strs();
        let base = String::from(base_str);
        let puzzle = String::from(puzzle_str);
        let solution = String::from(solution_str);
        let app_state = AppState::NotLoaded;
        let unloaded_info = PathInfo{base, puzzle, solution};
        let egui_mq = egui_miniquad::EguiMq::new(mq_ctx.as_mut());
        Self {
            mq_ctx, egui_mq,
            render_data,app_state,unloaded_info,
            extra_message:None, dragging: None,
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

        let camera = CameraSetup::frame_center(&world, window::screen_size());
        let tracks = setup_tracks(self.mq_ctx.as_mut(), &world.track_maps);
        self.app_state = Loaded(Loaded::new(world, solution, max_timestep, camera, tracks));
        Ok(())
    }
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
        if let Loaded(loaded) = &mut self.app_state{
            loaded.refresh_part_gfx(ctx);
            loaded.draw_world(&self.render_data, ctx);
        }
        ctx.end_render_pass();
        
        let mut do_loading = AppStateUpdate::NoChange;
        let screen_size = window::screen_size();
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
                    windows::stack_tooltip(egui_ctx, loaded);
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
                let mut do_load = || -> Result<()>{
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let dat = &self.unloaded_info;
                        let base_path = Path::new(&dat.base);
                        let f_puzzle = File::open(base_path.join(&dat.puzzle))?;
                        let solution_path = base_path.join(&dat.solution);
                        let f_sol = File::open(&solution_path)?;
                        self.load(f_puzzle, f_sol)?;
                        Ok(())
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
                        Ok(())
                    }
                };

                if let Err(err)= do_load(){
                    self.extra_message = Some(err.to_string());
                    #[cfg(target_arch = "wasm32")]
                    {
                        *JS_SOLUTION_INPUT.lock().unwrap() = None;
                    }
                } else {
                    self.extra_message = None;
                }
            }
        }

        // Draw things in front of egui here
        self.mq_ctx.commit_frame();
    }

    fn mouse_motion_event(&mut self, x: f32, y: f32) {
        self.egui_mq.mouse_motion_event( x, y);
        if let Loaded(loaded) = &mut self.app_state{
            if let Some(drag_last) = &mut self.dragging{
                loaded.camera.pan_pixels(x-drag_last[0], y-drag_last[1]);
                *drag_last = [x,y];
            }
            loaded.pointer_moved((x, y), window::screen_size());
        }
    }

    fn mouse_wheel_event(&mut self, dx: f32, dy: f32) {
        self.egui_mq.mouse_wheel_event(dx, dy);
        if let Loaded(loaded) = &mut self.app_state{
            //While a part is held, the wheel picks between the parts under it
            if loaded.wheel(dy){
                return;
            }
        }
        if !self.egui_mq.egui_ctx().is_pointer_over_egui(){
            if let Loaded(loaded) = &mut self.app_state{
                let cam = &mut loaded.camera;
                if dy.is_sign_negative() && cam.scale_base > 0.002{
                    cam.scale_base *= 0.9;
                }
                if dy.is_sign_positive() && cam.scale_base < 0.2{
                    cam.scale_base *= 1.1;
                }
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
        if !self.egui_mq.egui_ctx().is_pointer_over_egui(){
            match mb {
                MouseButton::Right | MouseButton::Middle => self.dragging = Some([x,y]),
                MouseButton::Left => if let Loaded(loaded) = &mut self.app_state{
                    loaded.press((x, y), window::screen_size());
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
        match mb {
            MouseButton::Left => if let Loaded(loaded) = &mut self.app_state{
                loaded.release();
            },
            _ => self.dragging = None,
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
            if loaded.press.is_some() && !self.egui_mq.egui_ctx().text_edit_focused(){
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
        if let Loaded(loaded) = &mut self.app_state{
            //Typing in a tape takes priority over the part hotkeys
            if !self.egui_mq.egui_ctx().text_edit_focused() && loaded.handle_key(keycode, keymods){
                return;
            }
        }
        self.egui_mq.key_down_event(keycode, keymods);
    }

    fn key_up_event(&mut self, keycode: KeyCode, keymods: KeyMods) {
        self.egui_mq.key_up_event(keycode, keymods);
    }
}
