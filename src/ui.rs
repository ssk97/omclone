use miniquad::*;
use crate::{sim::*, parser, render_sim::*, render_library::GFXPos};
use std::{io::BufReader, io::BufWriter, io::prelude::*};

#[cfg(not(target_arch = "wasm32"))]
use std::{fs::File, path::Path};

#[cfg(feature = "color_eyre")]
use color_eyre::eyre::Result;
#[cfg(not(feature = "color_eyre"))]
use simple_eyre::eyre::Result;
use core::ops::Range;
struct TapeBuffer<'a>{
    tape_ref: &'a mut Tape,
    earliest_edit: &'a mut Option<usize>,
    tape_mode: bool,
    held_str: String,
}
impl AsRef<str> for TapeBuffer<'_>{
    fn as_ref(&self) -> &str {
        &self.held_str
    }
}
//Note: this implementation assumes all characters in the string are ASCII/1 byte long
//It should be fine with non-ASCII attempted inserts, which will be removed as invalid instructions
impl egui::widgets::text_edit::TextBuffer for TapeBuffer<'_>{
    fn is_mutable(&self) -> bool {
        true
    }
    fn insert_text(&mut self, text: &str, char_index: usize) -> usize {
        let char_index = char_index;
        *self.earliest_edit = Some(char_index);
        let tape = &mut self.tape_ref;
        let instr_mapped:Vec<Instr> = text.chars().filter_map(|x| Instr::from_char(x.to_ascii_lowercase())).collect();
        let inserted = instr_mapped.len();

        let empty_extension = if char_index < tape.first{
            let tmp = tape.first-char_index;
            tape.first = char_index;
            tmp
        } else {
            0
        };
        let instr_chain = instr_mapped.into_iter().chain(std::iter::repeat(Instr::Empty).take(empty_extension));
        let splice_index = char_index-tape.first;
        let splice_range = if self.tape_mode {
            splice_index..usize::min(splice_index+text.len(),tape.instructions.len())
        } else {
            splice_index..splice_index
        };
        tape.instructions.splice(splice_range,instr_chain);

        self.held_str = tape.noop_clear_and_string();
        inserted
    }
    fn delete_char_range(&mut self, char_range: Range<usize>) {
        *self.earliest_edit = Some(char_range.start);
        let tape = &mut self.tape_ref;
        if self.tape_mode{
            for x in char_range{
                if x >= tape.first{
                    tape.instructions[x-tape.first] = Instr::Empty;
                }
            }
        } else {
            if char_range.end <= tape.first{
                tape.first -= char_range.end.saturating_sub(char_range.start);
            } else {
                let start = if char_range.start < tape.first{
                    tape.first = char_range.start;
                    0
                } else {
                    char_range.start-tape.first
                };
                let end = (char_range.end-tape.first).min(tape.instructions.len());
                tape.instructions.drain(start..end);
            }
        }
        self.held_str = tape.noop_clear_and_string();
    }

    fn clear(&mut self) {
        *self.earliest_edit = Some(0);
        self.tape_ref.instructions.clear();
        self.tape_ref.first = 0;
        self.held_str = self.tape_ref.noop_clear_and_string();
    }
    fn take(&mut self) -> String {
        let s = self.as_ref().to_owned();
        self.clear();
        s
    }
    fn as_str(&self) -> &str {
        self.as_ref()
    }
    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<TapeBuffer>()
    }
}

struct PathInfo{
    base: String,
    puzzle: String,
    solution: String,
}
enum RunState{
    Manual(usize), FreeRun, Crashed(XYPos)
}
struct Loaded{
    base_world: World,
    curr_world: World,
    backup_world: World,
    curr_time: f64,
    curr_substep: usize,
    backup_step: u64,
    substep_count: usize,
    float_world: FloatWorld,
    saved_motions: WorldStepInfo,

    max_timestep: usize,
    camera: CameraSetup,
    tracks: TrackBindings,
    solution: parser::FullSolution,
    message: Option<String>,

    tape_mode: bool,
    run_state: RunState,
    show_area: bool,
    run_speed: f64,
    popup_reload: bool,
}
impl Loaded{
    fn reset_to(&mut self, reset_to:u64){
        if self.backup_step < reset_to{
            self.reset_to_backup();
        } else {
            self.reset_world();
        }
    }
    fn reset_world(&mut self) {
        if let RunState::Crashed(_) = self.run_state{
            self.run_state = RunState::Manual(self.curr_world.timestep as usize);
        }
        self.saved_motions.clear();
        self.curr_world = self.base_world.clone();
        self.curr_time = 0.;
        self.substep_count = 8;
        self.curr_substep = 0;
        self.message = None;

        self.backup_world = self.base_world.clone();
        self.backup_step = 0;
    }
    fn reset_to_backup(&mut self){
        if let RunState::Crashed(_) = self.run_state{
            self.run_state = RunState::Manual(self.curr_world.timestep as usize);
        }
        self.saved_motions.clear();
        self.curr_world = self.backup_world.clone();
        self.curr_time = self.backup_step as f64;
        self.substep_count = 8;
        self.curr_substep = 0;
        self.message = None;
    }
    fn try_set_target_time(&mut self, target: usize){
        if let RunState::Crashed(_) = self.run_state{
            if self.curr_world.timestep as usize > target{
                self.run_state = RunState::Manual(target);
            }
        } else {
            self.run_state = RunState::Manual(target);
        }
    }
    fn advance(&mut self, substep_time: &mut f64) -> SimResult<()>{
        if self.curr_substep == 0{
            let failcheck = self.curr_world.prepare_step(&mut self.saved_motions, self.show_area);
            if failcheck.is_err(){
                self.saved_motions.clear();
                return failcheck;
            }
            self.substep_count = self.curr_world.substep_count(&self.saved_motions);
            *substep_time = 1.0/(self.substep_count as f64);
        }
        self.curr_substep += 1;
        if self.show_area && self.curr_substep < self.substep_count{
            self.curr_world.collide_substep(&self.saved_motions, self.curr_substep, self.substep_count)?;
        }
        if self.curr_substep == self.substep_count{
            self.curr_substep = 0;
            self.curr_world.finalize_step(&mut self.saved_motions, self.show_area)?;
        }
        Ok(())
    }
    fn update(&mut self){
            let target_time = match self.run_state{
                RunState::Manual(target_timestep) => {
                    let target_timestep = target_timestep as f64;
                    if target_timestep > self.curr_time+1.001{
                        target_timestep
                    } else {
                        f64::min(self.curr_time+self.run_speed,target_timestep)
                    }
                },
                RunState::FreeRun => self.curr_time+self.run_speed,
                RunState::Crashed(_) => return,
            };
            if target_time < self.curr_time{
                self.reset_to(target_time as u64);
            } else { //If backup is too out-of-date, catch up
                if self.curr_time as u64 >= self.backup_step+200 {
                    self.reset_to_backup()
                }
            }

            let mut substep_time = 1.0/(self.substep_count as f64);
            let backup_target_timestep = (target_time.floor() as u64).max(100) - 100;
            if self.curr_world.timestep < backup_target_timestep{
                while self.curr_world.timestep < backup_target_timestep{
                    let output = self.advance(&mut substep_time);
                    if let Err(output) = output{
                        self.run_state = RunState::Crashed(output.location);
                        self.message = Some(output.to_string());
                        self.curr_time = self.curr_world.timestep as f64+(substep_time*self.curr_substep as f64);
                        return;
                    }
                }
                self.backup_step = backup_target_timestep;
                self.backup_world = self.curr_world.clone();
                assert_eq!(self.curr_substep, 0);
                assert_eq!(backup_target_timestep, self.curr_world.timestep);
            }

            let target_timestep = target_time.floor() as u64;
            let target_substep = (target_time.fract()*self.substep_count as f64).floor() as usize;
            while self.curr_world.timestep < target_timestep || (self.curr_world.timestep == target_timestep && self.curr_substep < target_substep){
                let output = self.advance(&mut substep_time);
                if let Err(output) = output{
                    self.run_state = RunState::Crashed(output.location);
                    self.message = Some(output.to_string());
                    self.curr_time = self.curr_world.timestep as f64+(substep_time*self.curr_substep as f64);
                    return;
                }
            }
            self.curr_time = target_time;
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

        let float_world = FloatWorld::new();
        let mut saved_motions = WorldStepInfo::new();
        saved_motions.clear();
        let camera = CameraSetup::frame_center(&world, window::screen_size());
        let tracks = setup_tracks(self.mq_ctx.as_mut(), &world.track_maps);
        let new_loaded = Loaded{
            base_world: world.clone(),
            backup_world: world.clone(),
            curr_world: world,
            curr_time: 0.,
            curr_substep: 0,
            backup_step: 0,
            substep_count: 8,
            float_world,saved_motions,
        
            max_timestep,camera,tracks,solution,
            message: None,
        
            tape_mode: false,
            run_state: RunState::Manual(0),
            show_area: false,
            run_speed: 0.05,
            popup_reload: false,
        };
        self.app_state = Loaded(new_loaded);
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

#[cfg(all(target_arch = "wasm32", feature="editor_ui"))]
extern "C" {
    fn save_file(filedata: sapp_jsutils::JsObject, filename: sapp_jsutils::JsObject);
}

const EDITOR_ENABLED: bool = cfg!(feature = "editor_ui");

impl EventHandler for MyMiniquadApp {
    fn update(&mut self) {
        if let Loaded(loaded) = &mut self.app_state{
            loaded.update();
            let display_portion = loaded.curr_time.fract() as f32;
            if loaded.saved_motions.arms.is_empty(){
                loaded.float_world.generate_static(&loaded.curr_world);
            } else {
                loaded.float_world.regenerate(&loaded.curr_world, &loaded.saved_motions, display_portion);
            }
        }
    }

    fn draw(&mut self) {
        let ctx = self.mq_ctx.as_mut();
        ctx.clear(Some((1., 1., 1., 1.)), None, None);
        ctx.begin_default_pass(PassAction::clear_color(0.5, 0.5, 0.5, 1.0));
        if let Loaded(loaded) = &mut self.app_state{
            let world = &loaded.curr_world;
            let float_world = &loaded.float_world;
            self.render_data.draw(ctx, &loaded.camera, &loaded.tracks, loaded.show_area, world, float_world)
        }
        ctx.end_render_pass();
        
        let mut do_loading = AppStateUpdate::NoChange;
        let screen_size = window::screen_size();
        self.egui_mq.run(ctx, |_mq_ctx, egui_ctx|{
            if let Some(msg) = &self.extra_message{
                let mut opened = true;
                egui::Window::new("Message").open(&mut opened).show(egui_ctx, |ui| {
                    ui.label(msg);
                });
                if !opened {self.extra_message = None;}
            }
            match &mut self.app_state{
                Loaded(loaded) => {
                    egui::Window::new("World loaded").default_pos((0.,0.)).show(egui_ctx, |ui| {
                        ui.style_mut().spacing.slider_width = 500.;
                        let mut target_time = loaded.curr_time.floor() as usize;
                        if loaded.max_timestep < target_time{
                            loaded.max_timestep = target_time;
                        }
                        ui.horizontal(|ui| {
                            ui.add(egui::Slider::new(&mut target_time, 0..=loaded.max_timestep)
                                .show_value(false));
                            ui.add(egui::DragValue::new(&mut target_time)
                                .clamp_range(0..=loaded.max_timestep)
                                .speed(0.1));
                            ui.label("Timestep");
                        });
                        ui.horizontal(|ui| {
                            if ui.button("-1").clicked() {
                                if target_time > 0 {
                                    target_time -= 1;
                                }
                            }
                            if ui.button("+1").clicked() {
                                target_time += 1;
                            }
                            ui.label("Max Timestep:");
                            ui.add(egui::DragValue::new(&mut loaded.max_timestep)
                                .speed(0.1));
                            ui.separator();
                            if loaded.max_timestep < target_time{
                                loaded.max_timestep = target_time;
                            }
                            if target_time != loaded.curr_time.floor() as usize{
                                loaded.try_set_target_time(target_time);
                            }

                            let min_size = loaded.base_world.arms.iter()
                                .fold(0, |val, arm| usize::max(val, arm.instruction_tape.instructions.len()));
                            ui.label("Loop length:");

                            let repeat_length_response = ui.add_enabled(EDITOR_ENABLED, egui::DragValue::new(&mut loaded.base_world.repeat_length)
                                .clamp_range(min_size..=usize::MAX));
                            if repeat_length_response.changed(){
                                let length = loaded.base_world.repeat_length;
                                loaded.backup_world.repeat_length = length;
                                loaded.curr_world.repeat_length = length;
                                if loaded.max_timestep < length{
                                    loaded.max_timestep = length;
                                }
                                if length <= loaded.curr_time as usize{
                                    loaded.reset_to(length as u64);
                                }
                            }
                            ui.separator();
                            let area_check = ui.checkbox(&mut loaded.show_area, "Collide/Area");
                            if loaded.show_area{
                                if area_check.changed() {
                                    loaded.reset_to(0);
                                }
                                ui.monospace(format!("{:5}",loaded.curr_world.area_touched.len()));
                            }
                            ui.separator();
                            ui.label("Speed:");
                            ui.add(egui::DragValue::new(&mut loaded.run_speed)
                                .clamp_range(0.01..=10.0f32).fixed_decimals(2).speed(0.002));
                            let (mut running_free, enabled) = match loaded.run_state{
                                RunState::FreeRun => (true, true),
                                RunState::Manual(_) => (false, true),
                                RunState::Crashed(_) => (false, false),
                            };
                            let was_freerun = running_free;
                            ui.add_enabled_ui(enabled, |ui| ui.checkbox(&mut running_free, "Run"));
                            if running_free{
                                loaded.run_state = RunState::FreeRun;
                            } else if was_freerun{
                                loaded.try_set_target_time(loaded.curr_time.ceil() as usize);
                            }
                        });
                    });
                    if let Some(msg) = &loaded.message{
                        let mut opened = true;
                        egui::Window::new("Message").open(&mut opened).show(egui_ctx, |ui| {
                            ui.label(msg);
                        });
                        if !opened {loaded.message = None;}
                    }
                    egui::Window::new("Camera controls").show(egui_ctx, |ui| {
						ui.horizontal(|ui|{
							ui.label("x:");
							ui.add(egui::DragValue::new(&mut loaded.camera.offset[0])
								.speed(0.1));
							ui.label("y:");
							ui.add(egui::DragValue::new(&mut loaded.camera.offset[1])
								.speed(0.1));
                            if ui.button("Center").clicked(){
                                loaded.camera = CameraSetup::frame_center(&loaded.base_world, screen_size);
                            }
						});
						ui.horizontal(|ui|{
                            ui.label("zoom:");
                            ui.add(egui::Slider::new(&mut loaded.camera.scale_base,0.002 ..= 0.2)
                                .logarithmic(true));
                        });
                    });
					
                    #[cfg(feature = "editor_ui")]{
                        egui::Window::new("File info").show(egui_ctx, |ui| {
                            ui.label("solution:");
                            ui.text_edit_singleline(&mut loaded.solution.solution_name);
                            ui.label("filename:");
                            ui.text_edit_singleline(&mut self.unloaded_info.solution);
                            ui.horizontal(|ui| {
                                if ui.button("Save").clicked() {
                                    let dat = &self.unloaded_info;
                                    
                                    #[cfg(not(target_arch = "wasm32"))]
                                    let mut f = {
                                        let base_path = Path::new(&dat.base);
                                        let solution_path = base_path.join(&dat.solution);
                                        File::create(solution_path).unwrap()
                                    };
                                    #[cfg(target_arch = "wasm32")]
                                    let mut f = {
                                        Vec::<u8>::new()
                                    };

                                    {//scope the writer so it's dropped before trying to save in js
                                        if !loaded.solution.solution_name.contains("omclone"){
                                            loaded.solution.solution_name += "(omclone)";
                                        }
                                        let mut writer = BufWriter::new(&mut f);
                                        let base = &loaded.base_world;
                                        let new_solution = parser::create_solution(base, 
                                            loaded.solution.puzzle_name.clone(),
                                            loaded.solution.solution_name.clone());
                                        parser::write_solution(&mut writer, &new_solution).unwrap();
                                        writer.flush().unwrap();
                                    }

                                    #[cfg(target_arch = "wasm32")]{
                                        let data = f;
                                        let filedata = sapp_jsutils::JsObject::buffer(&data);
                                        let filename = sapp_jsutils::JsObject::string(&dat.solution);
										unsafe {save_file(filedata,filename);}
                                    }

                                    loaded.message = Some(String::from("Saved!"));
                                }
                                ui.separator();
                                if ui.button("Load").clicked() {
                                    loaded.popup_reload = true;
                                }
                            });
                        });
                    }
                    {
                        let mut arm_window = egui::Window::new("Arms");
                        #[cfg(not(feature = "editor_ui"))]{
                            arm_window = arm_window.default_open(false);
                        }
                        arm_window.default_width(600.).show(egui_ctx, |ui| {
                            #[cfg(not(feature = "editor_ui"))]{
                                ui.label("Editing Disabled");
                            }
                            ui.horizontal( |ui| {
                                #[cfg(feature = "editor_ui")]{
                                    ui.checkbox(&mut loaded.tape_mode, "Tape/overwrite mode");
                                    ui.separator();
                                    if ui.button("space ends").clicked() {
                                        for a in &mut loaded.base_world.arms{
                                            let instr = &mut a.instruction_tape;
                                            for _ in instr.instructions.len()..loaded.base_world.repeat_length{
                                                a.instruction_tape.instructions.push(Instr::Empty);
                                            }
                                        }
                                    }
                                    ui.separator();
                                    if ui.button("clear ends").clicked() {
                                        for a in &mut loaded.base_world.arms{
                                            let instr = &mut a.instruction_tape;
                                            while instr.instructions.last() == Some(&Instr::Empty){
                                                instr.instructions.pop();
                                            }
                                        }
                                    }
                                    if ui.button("Optimize Pistons").clicked() {
                                        let mut pistons_added = 0;
                                        let mut pistons_removed = 0;
                                        for a in &mut loaded.base_world.arms{
                                            if a.instruction_tape.instructions.contains(&Instr::Extend) || a.instruction_tape.instructions.contains(&Instr::Retract) {
                                                if a.arm_type == ArmType::PlainArm {
                                                    a.arm_type = ArmType::Piston;
                                                    pistons_added += 1;
                                                }
                                            } else {
                                                if a.arm_type == ArmType::Piston {
                                                    a.arm_type = ArmType::PlainArm;
                                                    pistons_removed += 1;
                                                }
                                            }
                                        }
                                        self.extra_message = Some(format!("{pistons_added} pistons added, {pistons_removed} pistons removed"));
                                    }
                                    ui.separator();
                                }
                            });
                            let curr_timestep = loaded.curr_time.floor() as usize;
                            let end_spacing = loaded.max_timestep.saturating_sub(curr_timestep);
                            let marker = " ".repeat(curr_timestep+3)+"V"+&" ".repeat(end_spacing);
                            //+&(" ".repeat(loaded.max_timestep-loaded.curr_timestep));
                            let mut earliest_edit = None;
                            let mut target_time = None;
                            let mut change_arm_id = None;
                            egui::ScrollArea::horizontal().show(ui, |ui| {
                                ui.add(egui::Label::new(egui::RichText::new(marker).monospace()).wrap_mode(egui::TextWrapMode::Extend));
                                egui::ScrollArea::vertical().show(ui, |ui| {
                                    for (a_num, a) in loaded.base_world.arms.iter_mut().enumerate(){
                                        let text = a.instruction_tape.to_string();
                                        let mut text_buf = TapeBuffer{
                                            tape_ref: &mut a.instruction_tape,
                                            earliest_edit: &mut earliest_edit,
                                            tape_mode: loaded.tape_mode,
                                            held_str: text,
                                        };
                                        ui.horizontal(|ui| {
                                            ui.label(format!("{:02}",a_num+1));
                                            let mut line_edit = egui::TextEdit::singleline(&mut text_buf)
                                            .code_editor().desired_width(f32::INFINITY);
                                            #[cfg(not(feature = "editor_ui"))]{
                                                line_edit = line_edit.interactive(false);
                                            }
                                            let text_output = line_edit.show(ui);
                                            if let Some(cursor) = text_output.cursor_range{
                                                target_time = Some(cursor.primary.index);
                                            }
                                            if text_output.response.changed(){
                                                change_arm_id = Some(a_num);
                                            }
                                        });
                                    }
                                    let marker2 = " ".repeat(curr_timestep+3)+"^";
                                    ui.add(egui::Label::new(egui::RichText::new(marker2).monospace()));
                                });
                            });
                            if let Some(target) = target_time {
                                loaded.try_set_target_time(target);
                            }
                            if let Some(edit_step) = earliest_edit{
                                let arm_id = change_arm_id.unwrap();//If edit is performed, the change must be recorded
                                let min_time = loaded.base_world.arms.iter()
                                    .fold(0, |val, arm| usize::max(val, arm.instruction_tape.instructions.len()+arm.instruction_tape.first));
                                if loaded.max_timestep < min_time{
                                    loaded.max_timestep = min_time;
                                }
                                let min_loop = loaded.base_world.arms.iter()
                                    .fold(0, |val, arm| usize::max(val, arm.instruction_tape.instructions.len()));
                                if loaded.base_world.repeat_length < min_loop{
                                    loaded.base_world.repeat_length = min_loop;
                                    loaded.backup_world.repeat_length = min_loop;
                                    loaded.curr_world.repeat_length = min_loop;
                                }
                                loaded.backup_world.arms[arm_id].instruction_tape = loaded.base_world.arms[arm_id].instruction_tape.clone();
                                if edit_step <= loaded.curr_time as usize {
                                    if let RunState::Crashed(_) = loaded.run_state{
                                        loaded.run_state = RunState::Manual(loaded.curr_world.timestep as usize);
                                    }
                                    loaded.reset_to(edit_step as u64);
                                } else {
                                    loaded.curr_world.arms[arm_id].instruction_tape = loaded.base_world.arms[arm_id].instruction_tape.clone();
                                }
                            }
                        });
                    }
                    /*egui::Window::new("debug").min_width(128.).show(egui_ctx, |ui|{
                        ui.label(format!("curr time: {}",loaded.curr_time as u64));
                        ui.label(format!("backup time: {}",loaded.backup_step));
                    });*/
                    egui::Window::new("Reload?").open(&mut loaded.popup_reload).show(egui_ctx, |ui| {
                        #[cfg(not(target_arch = "wasm32"))]{
                                if ui.button("Reload current").clicked() {
                                    do_loading = AppStateUpdate::Load;
                                }
                                if ui.button("Load New").clicked() {
                                    do_loading = AppStateUpdate::ResetHome;
                                }
                        }
                        #[cfg(target_arch = "wasm32")]{
                            ui.label("Choose files first, then press:");
                            if ui.button("Reset").clicked() {
                                do_loading = AppStateUpdate::ResetHome;
                            }

                        }
                    });
                }
                NotLoaded => {
                    egui::Window::new("World Not Loaded").show(egui_ctx, |ui| {
                        #[cfg(not(target_arch = "wasm32"))]{
                            let dat = &mut self.unloaded_info;
                            ui.horizontal(|ui| {
                                ui.label("Base path: ");
                                ui.text_edit_singleline(&mut dat.base);
                            });
                            ui.horizontal(|ui| {
                                ui.label("Puzzle: ");
                                ui.text_edit_singleline(&mut dat.puzzle);
                            });
                            ui.horizontal(|ui| {
                                ui.label("Solution: ");
                                ui.text_edit_singleline(&mut dat.solution);
                            });
                            if ui.button("Load").clicked() {
                                do_loading = AppStateUpdate::Load;
                            }
                        }
                        #[cfg(target_arch = "wasm32")]
                        {
                            let puzzle_loaded = JS_PUZZLE_INPUT.lock().unwrap().is_some();
                            let solution_loaded = JS_SOLUTION_INPUT.lock().unwrap().is_some();
                            match (puzzle_loaded, solution_loaded) {
                                (false, false) => {ui.label("Upload puzzle and solution files");},
                                (false, true) => {ui.label("Upload puzzle file");},
                                (true, false) => {ui.label("Upload solution file");},
                                (true, true) => {do_loading = AppStateUpdate::Load;}
                            }
                        }
                    });
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
        if let Some(drag_last) = &mut self.dragging{
            if let Loaded(loaded) = &mut self.app_state{
                let cam = &mut loaded.camera;
                cam.offset[0] += (x-drag_last[0])/cam.scale_base*0.002;
                cam.offset[1] -= (y-drag_last[1])/cam.scale_base*0.002;
                *drag_last = [x,y];
            }
        }
    }

    fn mouse_wheel_event(&mut self, dx: f32, dy: f32) {
        self.egui_mq.mouse_wheel_event(dx, dy);
        if !self.egui_mq.egui_ctx().is_pointer_over_area(){
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
        if !self.egui_mq.egui_ctx().is_pointer_over_area() && (mb == MouseButton::Right || mb == MouseButton::Middle){
            self.dragging = Some([x,y]);
        }
    }

    fn mouse_button_up_event(
        &mut self,
        mb: MouseButton,
        x: f32,
        y: f32,
    ) {
        self.egui_mq.mouse_button_up_event(mb, x, y);
        self.dragging = None;
    }

    fn char_event(
        &mut self,
        character: char,
        _keymods: KeyMods,
        _repeat: bool,
    ) {
        self.egui_mq.char_event(character);
    }

    fn key_down_event(
        &mut self,
        keycode: KeyCode,
        keymods: KeyMods,
        _repeat: bool,
    ) {
        self.egui_mq.key_down_event(keycode, keymods);
    }

    fn key_up_event(&mut self, keycode: KeyCode, keymods: KeyMods) {
        self.egui_mq.key_up_event(keycode, keymods);
    }
}