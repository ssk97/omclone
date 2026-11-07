//The egui windows, except the Part window (see part_window.rs)
use super::*;

//A closable message, cleared when closed
pub fn message_window(egui_ctx: &egui::Context, id: &str, message: &mut Option<String>){
    if let Some(msg) = message{
        let mut opened = true;
        egui::Window::new("Message").id(egui::Id::new(id))
            .open(&mut opened).show(egui_ctx, |ui| {
            ui.label(msg.as_str());
        });
        if !opened {*message = None;}
    }
}
//The largest max timestep or loop length that can be entered. It's far beyond real solutions, and keeps
//the tape grid in arms_window from getting huge.
const MAX_TIMESTEP_FIELD: usize = 1_000_000;
pub fn timeline_window(egui_ctx: &egui::Context, loaded: &mut Loaded){
    egui::Window::new("World loaded").default_pos((0.,0.)).show(egui_ctx, |ui| {
        ui.style_mut().spacing.slider_width = 500.;
        //Where the timeline is headed, so that +1 clicked again before it gets there goes on another step
        let cursor = loaded.cursor_step();
        let mut target_time = cursor;
        if loaded.max_timestep < target_time{
            loaded.max_timestep = target_time;
        }
        ui.horizontal(|ui| {
            //Sliders only sense drags, and egui never takes a press released in the same frame (a click during a slow
            //frame) as a drag, so such a click was ignored. Sensing clicks too makes egui give the slider its position.
            let _ = ui.add(egui::Slider::new(&mut target_time, 0..=loaded.max_timestep)
                .show_value(false)).interact(egui::Sense::click());
            ui.add(egui::DragValue::new(&mut target_time)
                .range(0..=loaded.max_timestep)
                .speed(0.1));
            ui.label("Timestep");
            let repeat_length = loaded.timeline.base().repeat_length;
            if ui.button("-Loop").on_hover_text("Back one loop length").clicked() {
                target_time = target_time.saturating_sub(repeat_length);
            }
            if ui.button("+Loop").on_hover_text("Forward one loop length").clicked() {
                target_time += repeat_length;
            }
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
            //Running or long tapes can take it past the limit, which only applies to changing it here
            ui.add(egui::DragValue::new(&mut loaded.max_timestep)
                .range(0..=MAX_TIMESTEP_FIELD).clamp_existing_to_range(false)
                .speed(0.1));
            ui.separator();
            if loaded.max_timestep < target_time{
                loaded.max_timestep = target_time;
            }
            if target_time != cursor{
                loaded.try_set_target_time(target_time);
            }

            let min_size = loaded.timeline.base().min_repeat_length();
            ui.label("Loop length:");

            let mut repeat_length = loaded.timeline.base().repeat_length;
            //Typed lengths apply on Enter, since each change re-simulates
            let lock = loaded.edit_lock();
            let repeat_length_response = ui.add_enabled(lock.is_none(), egui::DragValue::new(&mut repeat_length)
                .range(min_size..=MAX_TIMESTEP_FIELD.max(min_size)).clamp_existing_to_range(false)
                .update_while_editing(false));
            if locked_hover(repeat_length_response, lock).changed() && repeat_length != loaded.timeline.base().repeat_length{
                loaded.edit_repeat_length(repeat_length);
            }
            ui.separator();
            ui.label("Collide/Area:");
            for (mode, label, hover) in [
                (AreaMode::Shown, "Show", "Checks collisions and measures area, shading the hexes it covers"),
                (AreaMode::Hidden, "Hide", "Checks collisions and measures area, without the shading"),
                (AreaMode::Off, "Off", "No collision checks or area. Turning them back on simulates again from the start."),
            ]{
                if ui.selectable_label(loaded.area_mode == mode, label).on_hover_text(hover).clicked(){
                    loaded.set_area_mode(mode);
                }
            }
            if loaded.area_mode != AreaMode::Off{
                ui.monospace(format!("{:5}",loaded.timeline.curr().area_touched.len()));
                ui.label(" area");
            }
            ui.separator();
            ui.label("Speed:");
            //Dragging changes it in proportion to its size, so it's as easy to adjust at 1 as at 1000
            let drag_speed = loaded.run_speed*0.01;
            ui.add(egui::DragValue::new(&mut loaded.run_speed)
                .range(0.5..=1500.0f32).max_decimals(1).speed(drag_speed).suffix(" steps/s"));
            let (mut running_free, enabled) = match loaded.run_state{
                RunState::FreeRun => (true, true),
                RunState::Manual(_) => (false, true),
                RunState::Crashed{..} => (false, false),
            };
            let was_freerun = running_free;
            ui.add_enabled_ui(enabled, |ui| ui.checkbox(&mut running_free, "Run"));
            if running_free{
                loaded.run_state = RunState::FreeRun;
            } else if was_freerun{
                loaded.try_set_target_time(loaded.timeline.time().ceil() as usize);
            }
        });
    });
}
//While the run is crashed: what happened, and a button that brings it into view (the board circles it)
pub fn crash_window(egui_ctx: &egui::Context, loaded: &mut Loaded){
    let RunState::Crashed{step, error} = loaded.run_state else {return};
    let mut open = loaded.crash_window_open;
    egui::Window::new("Crash").open(&mut open).show(egui_ctx, |ui| {
        ui.label(format!("Timestep {step}: {error}"));
        if ui.button("Show").on_hover_text("Centers the view on it").clicked(){
            loaded.camera.center_on(error.location);
        }
    });
    loaded.crash_window_open = open;
}
//Opened from the Part window
pub fn camera_window(egui_ctx: &egui::Context, loaded: &mut Loaded, screen_size: (f32, f32)){
    //A copy, as the window's contents use `loaded` too
    let mut open = loaded.camera_window_open;
    egui::Window::new("Camera controls").open(&mut open).show(egui_ctx, |ui| {
        ui.horizontal(|ui|{
            ui.label("x:");
            ui.add(egui::DragValue::new(&mut loaded.camera.offset.x)
                .speed(0.1));
            ui.label("y:");
            ui.add(egui::DragValue::new(&mut loaded.camera.offset.y)
                .speed(0.1));
            if ui.button("Center").clicked(){
                loaded.camera = CameraSetup::frame_center(loaded.timeline.base(), screen_size);
            }
        });
        ui.horizontal(|ui|{
            ui.label("zoom:");
            ui.add(egui::Slider::new(&mut loaded.camera.scale_base,ZOOM_RANGE)
                .logarithmic(true));
        });
        //Always on without the editor, as there are no parts to drag
        let mut pans = loaded.left_drag_pans();
        ui.add_enabled(EDITOR_ENABLED, egui::Checkbox::new(&mut pans, "Left drag pans"))
            .on_hover_text("Dragging the board with the left button pans instead of moving parts. A click still selects.");
        loaded.left_drag_pans = pans;
        ui.checkbox(&mut loaded.keys_pan, "Arrow keys and WASD pan")
            .on_hover_text("While held, unless you're typing or moving a part (where A/D/W/S turn and resize it)");
    });
    loaded.camera_window_open = open;
}
//A row of a help window: keys or mouse actions, and what they do
struct HelpEntry{
    keys: &'static str,
    does: &'static str,
    //The builds it's shown in: with the editor, without it (the display build), or both
    in_editor: bool,
    in_display: bool,
}
const fn both(keys: &'static str, does: &'static str) -> HelpEntry{
    HelpEntry{keys, does, in_editor: true, in_display: true}
}
const fn editor(keys: &'static str, does: &'static str) -> HelpEntry{
    HelpEntry{keys, does, in_editor: true, in_display: false}
}
const fn display(keys: &'static str, does: &'static str) -> HelpEntry{
    HelpEntry{keys, does, in_editor: false, in_display: true}
}
//These are the only list of the keys: the README points here
const BOARD_HELP: &[HelpEntry] = &[
    both("Click", "Select a part: the topmost on the hex, or the selected one if it's there"),
    both("Tab / Shift+Tab, or wheel", "While the button is held: pick another part on that hex"),
    editor("Drag", "Move a part, or pan with \"Left drag pans\" (Camera controls)"),
    display("Drag", "Pan"),
    editor("Ctrl+drag", "Copy a part"),
    editor("A / D", "While dragging or placing: turn counterclockwise / clockwise"),
    editor("W / S", "While dragging or placing an arm: lengthen / shorten it"),
    editor("Delete or Backspace", "Delete the selected part"),
    editor("Esc", "Let go of the held part, then stop placing parts or drawing track"),
    editor("Right-click", "Stop placing parts or drawing track"),
    both("Right or middle drag", "Pan"),
    both("Wheel", "Zoom"),
    both("Two-finger drag; pinch", "Pan; zoom"),
    both("Arrows or WASD", "Pan, with \"Arrow keys and WASD pan\" (Camera controls)"),
    both("Hold 1", "Fade the molecules to see the glyphs under them"),
    editor("Ctrl+Z; Ctrl+Y or Ctrl+Shift+Z", "Undo; redo"),
];
const PART_WINDOW_HELP: &[HelpEntry] = &[
    both("Click a part in the list", "Select it"),
    editor("Drag a glyph in the list", "Reorder it"),
    editor("Shift+click", "Place a part from Add part, and keep placing more"),
    editor("Enter, or a click elsewhere", "Apply a typed value"),
    editor("w e a d z x", "In Path: steps to neighboring hexes, or (dx,dy) to jump. Esc drops what was typed."),
];
const TAPE_HELP: &[HelpEntry] = &[
    both("Click a cell", "Move the caret, and the timeline, there"),
    both("Click the ruler", "Go to that timestep"),
    both("Click an arm number", "Select its row"),
    editor("Drag an arm number", "Reorder the arms"),
    both("Drag; Shift+click; Shift+arrows", "Select"),
    both("Arrows", "Move the caret"),
    both("Home / End", "Go to step 0 / the end of the selected tapes. With Shift, select to there."),
    both("Ctrl+A", "Select every tape"),
    both("Ctrl+C", "Copy the selection, a line per arm"),
    editor("Backspace / Delete", "Erase"),
    editor("Insert", "Toggle overwrite mode"),
    editor("Ctrl+X / Ctrl+V", "Cut / paste"),
    editor("Ctrl+Z; Ctrl+Y or Ctrl+Shift+Z", "Undo; redo"),
    both("Esc", "Clear the selection, then leave the tapes"),
];
const CMD_NOTE: &str = "Ctrl means Cmd on a Mac";
//The entries that do something in this build
fn help_grid(ui: &mut egui::Ui, id: &str, entries: &[HelpEntry]){
    egui::Grid::new(id).striped(true).show(ui, |ui| {
        for entry in entries.iter().filter(|e| if EDITOR_ENABLED {e.in_editor} else {e.in_display}){
            ui.strong(entry.keys);
            ui.label(entry.does);
            ui.end_row();
        }
    });
}
//Sized to fit, scrolling only when the screen is too short. A resizable window would start out shorter than its
//scroll area's contents, and stay that way. Nothing in it can fill the width (like a separator), which would make it
//as wide as the screen.
fn help_window_frame<'a>(title: &str, open: &'a mut bool) -> egui::Window<'a>{
    egui::Window::new(title).open(open).auto_sized()
}
//Opened from the Part window
pub fn help_window(egui_ctx: &egui::Context, loaded: &mut Loaded){
    help_window_frame("Help", &mut loaded.help_window_open).show(egui_ctx, |ui| {
        ui.weak(CMD_NOTE);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Board");
            help_grid(ui, "board_help", BOARD_HELP);
            ui.heading("Part window");
            help_grid(ui, "part_window_help", PART_WINDOW_HELP);
            ui.add_space(ui.spacing().item_spacing.y);
            ui.weak("Tape keys: the ? above the tapes, in the Arms window");
        });
    });
}
//Opened by the ? in the Arms window. It first opens beside that window, so it doesn't cover the tapes.
pub fn tape_help_window(egui_ctx: &egui::Context, loaded: &mut Loaded){
    let mut window = help_window_frame("Tape help", &mut loaded.tape_help_window_open);
    if let Some(arms) = egui_ctx.memory(|m| m.area_rect(arms_window_id())){
        window = window.default_pos(arms.right_top() + egui::vec2(8., 0.));
    }
    window.show(egui_ctx, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Instructions");
            tape_grid::tape_letters(ui);
            ui.heading("Keys");
            ui.weak(CMD_NOTE);
            help_grid(ui, "tape_help", TAPE_HELP);
        });
    });
}
#[cfg(feature = "editor_ui")]
pub fn file_window(egui_ctx: &egui::Context, loaded: &mut Loaded, unloaded_info: &mut PathInfo){
    egui::Window::new("File info").show(egui_ctx, |ui| {
        ui.label("solution:");
        ui.text_edit_singleline(&mut loaded.solution.solution_name);
        ui.label("filename:");
        ui.text_edit_singleline(&mut unloaded_info.solution);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                //Serialize fully first so a failure doesn't leave a partial file
                let saved = loaded.solution_bytes().and_then(|data| {
                    #[cfg(not(target_arch = "wasm32"))]
                    std::fs::write(solution_path(unloaded_info), &data)?;
                    #[cfg(target_arch = "wasm32")]
                    download(&data, &unloaded_info.solution);
                    Ok(())
                });
                loaded.message = Some(match saved {
                    Ok(()) => {
                        //The name as saved
                        loaded.solution.solution_name = loaded::saved_solution_name(&loaded.solution.solution_name);
                        loaded.dirty = false;
                        loaded.load_notes.clear();
                        recovery::clear();
                        String::from("Saved!")
                    }
                    Err(err) => format!("Save failed: {err}"),
                });
            }
            ui.separator();
            if ui.button("Load").clicked() {
                loaded.popup_reload = true;
            }
        });
        if loaded.dirty{
            ui.weak("Unsaved changes");
        }
        file_notes(ui, loaded);
        ui.separator();
        solution_stats(ui, loaded);
    });
}
//Where the tapes crash, what loading changed, and what omsim would reject. A long list ends with how many more there
//are.
#[cfg(feature = "editor_ui")]
fn file_notes(ui: &mut egui::Ui, loaded: &Loaded){
    const SHOWN: usize = 4;
    let clashes = clash_notes(loaded.timeline.base());
    let notes: Vec<&String> = clashes.iter().chain(&loaded.load_notes).chain(&loaded.rule_notes).collect();
    let color = ui.visuals().warn_fg_color;
    for note in notes.iter().take(SHOWN){
        ui.colored_label(color, note.as_str());
    }
    if notes.len() > SHOWN{
        ui.colored_label(color, format!("…and {} more", notes.len() - SHOWN));
    }
}
//For each arm whose tape has a clash, where the run crashes on it
#[cfg(feature = "editor_ui")]
fn clash_notes(world: &World) -> Vec<String>{
    world.arms.iter().enumerate().filter_map(|(i, arm)| arm.first_clash().map(|(step, kind)| {
        let kind = if kind == Instr::Repeat {"Repeat"} else {"Reset"};
        format!("Arm {} crashes at timestep {step}: an instruction is inside its {kind}'s expansion", i + 1)
    })).collect()
}
//The solution's stats as the loaded file had them, and as they are now. Cycles and area are only known once the
//timeline has run past the last output.
#[cfg(feature = "editor_ui")]
fn solution_stats(ui: &mut egui::Ui, loaded: &Loaded){
    let base = loaded.timeline.base();
    let completion = loaded.timeline.completion();
    let unknown = |ui: &mut egui::Ui, why: &str| {ui.weak("–").on_hover_text(why);};
    let not_run = "Run or step past the last output to measure this";
    egui::Grid::new("solution_stats").show(ui, |ui| {
        for heading in ["", "Cycles", "Cost", "Area", "Instructions"]{
            ui.label(heading);
        }
        ui.end_row();
        ui.label("As loaded");
        match &loaded.solution.stats{
            Some(s) => for n in [s.cycles, s.cost, s.area, s.instructions]{
                ui.label(n.to_string());
            },
            None => {ui.weak("unsolved");}
        }
        ui.end_row();
        ui.label("Now");
        match completion{
            Some(c) => {ui.label(c.cycles.to_string());}
            None => unknown(ui, not_run),
        }
        ui.label(base.cost.to_string());
        match completion{
            _ if loaded.area_mode == AreaMode::Off => unknown(ui, "Turn on Collide/Area to measure this"),
            Some(c) => {ui.label(c.area.to_string());}
            None => unknown(ui, not_run),
        }
        ui.label(base.instruction_count().to_string());
        ui.end_row();
    });
}
fn arms_window_id() -> egui::Id{
    egui::Id::new("arms_window")
}
//`extra_message` reports what "Optimize Pistons" changed
#[cfg_attr(not(feature = "editor_ui"), allow(unused_variables))]
pub fn arms_window(egui_ctx: &egui::Context, loaded: &mut Loaded, extra_message: &mut Option<String>){
    let mut arm_window = egui::Window::new("Arms").id(arms_window_id());
    #[cfg(not(feature = "editor_ui"))]{
        arm_window = arm_window.default_open(false);
    }
    //It can be made shorter than the tapes, which then scroll, but not taller.
    //The height that fits them is measured as the window is drawn, so it applies from the next frame.
    let fit_id = egui::Id::new("arms_window_fit_height");
    if let Some(height) = egui_ctx.data(|d| d.get_temp::<f32>(fit_id)){
        arm_window = arm_window.max_height(height);
    }
    let shown = arm_window.default_width(600.).show(egui_ctx, |ui| {
        let top = ui.min_rect().top();
        #[cfg(not(feature = "editor_ui"))]{
            ui.label("Editing Disabled");
        }
        ui.horizontal( |ui| {
            #[cfg(feature = "editor_ui")]{
                ui.checkbox(&mut loaded.tape_mode, "Tape/overwrite mode").on_hover_text("The Insert key toggles it while typing in the tapes");
                ui.separator();
                let lock = loaded.edit_lock();
                let optimize = ui.add_enabled(lock.is_none(), egui::Button::new("Optimize Pistons"));
                if locked_hover(optimize, lock).clicked() {
                    let mut pistons_added = 0;
                    let mut pistons_removed = 0;
                    //Changed on a copy, so a failed setup leaves the solution as it was
                    let mut layout = loaded.timeline.base().layout();
                    for a in &mut layout.arms{
                        if a.tape().instructions.contains(&Instr::Extend) || a.tape().instructions.contains(&Instr::Retract) {
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
                    if pistons_added + pistons_removed > 0 {
                        match loaded.set_up(&layout) {
                            Ok(world) => loaded.commit_world(world),
                            Err(err) => loaded.message = Some(err),
                        }
                    }
                    *extra_message = Some(format!("{pistons_added} pistons added, {pistons_removed} pistons removed"));
                }
                ui.separator();
            }
            //Hovering it doesn't take the tapes' focus, so the letters can be looked up while typing
            ui.toggle_value(&mut loaded.tape_help_window_open, "?").on_hover_ui(|ui| {
                tape_grid::tape_letters(ui);
                ui.weak("Click for all the tape keys");
            });
        });
        let grid_top = ui.cursor().top();
        let grid_height = tape_grid::tape_grid(ui, loaded);
        //Where the content would end with every arm showing, and the frame's margin below it
        (grid_top + grid_height - top, top, egui::Frame::window(ui.style()).total_margin().bottom)
    });
    if let Some(egui::InnerResponse{inner: Some((content, top, margin)), response}) = shown{
        let fit = (top - response.rect.top() + content + margin).ceil();
        egui_ctx.data_mut(|d| d.insert_temp(fit_id, fit));
    }
}
//Shows the reason a disabled control can't be used
pub fn locked_hover(response: egui::Response, lock: Option<&str>) -> egui::Response{
    match lock{
        Some(reason) => response.on_disabled_hover_text(reason),
        None => response,
    }
}
//While something is dragged, scrolls the scroll area `ui` is in when the pointer is near or past the top or bottom of
//`rect` (its visible part), faster the further in it goes. egui doesn't scroll while dragging.
pub fn drag_edge_scroll(ui: &egui::Ui, rect: egui::Rect){
    const MARGIN: f32 = 20.;
    //Pixels a second at the edge. It's slower inside the margin, and faster past the edge, up to three times as fast.
    const SPEED: f32 = 300.;
    let Some(pointer) = ui.ctx().pointer_latest_pos().filter(|p| rect.x_range().contains(p.x)) else {return};
    //Positive scrolls up
    let depth = if pointer.y < rect.top() + MARGIN {
        rect.top() + MARGIN - pointer.y
    } else if pointer.y > rect.bottom() - MARGIN {
        rect.bottom() - MARGIN - pointer.y
    } else {
        return
    };
    let speed = SPEED * (depth / MARGIN).clamp(-3., 3.);
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    ui.scroll_with_delta_animation(egui::vec2(0., speed * dt), egui::style::ScrollAnimation::none());
    ui.ctx().request_repaint();
}
//What the mouse is doing on the board: the part picked from the stack under it, or the part or tool it's using
pub fn cursor_tooltip(egui_ctx: &egui::Context, loaded: &Loaded){
    let base = loaded.timeline.base();
    let (title, note) = if let Some(press) = loaded.tool.press() {
        let copy = if press.duplicate {", copying"} else {""};
        let title = format!("{} ({}/{}){copy}", part_label(base, press.part()), press.cand+1, press.stack.len());
        //Why it isn't moving, unless nothing can be edited anyway
        let lock = loaded.press_lock().filter(|_| EDITOR_ENABLED && press.drag.is_none());
        (title, lock.map(str::to_owned))
    } else if loaded.pointer_over_ui {
        return;
    } else if let Some(mode) = loaded.tool.track_mode() {
        let title = match mode.glyph{
            None => String::from("Click to start a new track, then drag to draw it"),
            Some(i) => {
                let verb = match mode.tool{
                    TrackTool::AddFront => "Adding to the front of",
                    TrackTool::AddBack => "Adding to the back of",
                    TrackTool::Remove => "Removing hexes from",
                };
                format!("{verb} {}", part_label(base, PartId::Glyph(i)))
            }
        };
        (title, Some(String::from("Esc or right-click to finish")))
    } else if let Some(placing) = loaded.tool.placing() {
        (format!("Placing {}", placing.part.name()), Some(String::from("Shift+click to place more; Esc or right-click to stop")))
    } else {
        return;
    };
    let refusal = loaded.tool.stroke().and_then(|s| s.refusal).map(str::to_owned);
    let error = loaded.preview.as_ref().and_then(|p| p.world.as_ref().err().cloned()).or(refusal);
    egui::Tooltip::always_open(egui_ctx.clone(), egui::LayerId::background(),
        egui::Id::new("part_stack_tooltip"), egui::PopupAnchor::Pointer).show(|ui| {
        ui.label(title);
        if let Some(note) = note {
            ui.weak(note);
        }
        if let Some(err) = error {
            ui.colored_label(egui::Color32::RED, err);
        }
    });
}
pub fn reload_window(egui_ctx: &egui::Context, loaded: &mut Loaded) -> AppStateUpdate{
    let mut do_loading = AppStateUpdate::NoChange;
    let dirty = loaded.dirty;
    egui::Window::new("Reload?").open(&mut loaded.popup_reload).show(egui_ctx, |ui| {
        if dirty{
            ui.colored_label(ui.visuals().warn_fg_color, "Unsaved changes will be lost");
        }
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
    do_loading
}
#[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
pub fn not_loaded_window(egui_ctx: &egui::Context, unloaded_info: &mut PathInfo) -> AppStateUpdate{
    let mut do_loading = AppStateUpdate::NoChange;
    egui::Window::new("World Not Loaded").show(egui_ctx, |ui| {
        #[cfg(not(target_arch = "wasm32"))]{
            let dat = unloaded_info;
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
    do_loading
}

//Debug builds only, as egui_test needs
#[cfg(all(test, feature = "editor_ui", debug_assertions))]
mod tests {
    use super::*;
    use egui::{Event, RawInput, WidgetType};
    use egui_test::*;

    //A click on the timeline slider whose press and release came in one frame (during a slow frame) was ignored
    #[test]
    fn a_click_within_one_frame_moves_the_timeline() {
        let ctx = context();
        let mut loaded = Loaded::for_test(vec![], vec![]);
        loaded.max_timestep = 100;
        let mut timeline_frame = |events: Vec<Event>| {
            let _ = ctx.run_ui(RawInput{events, ..Default::default()}, |ui| timeline_window(ui.ctx(), &mut loaded));
        };
        for _ in 0..3 {
            timeline_frame(vec![]);
        }
        let slider = widget_rect(&ctx, |w| w.typ == WidgetType::Slider).expect("no slider drawn");
        timeline_frame(vec![Event::PointerMoved(slider.center())]);
        timeline_frame(click(slider.center()));
        let step = loaded.cursor_step();
        assert!((45..=55).contains(&step), "the middle of the slider is near timestep 50, not {step}");
    }

    #[test]
    fn clashes_are_noted_for_each_arm() {
        use Instr::*;
        let arm = |x, instructions| Arm::new(Pos::new(x, 0), 0, 1, ArmType::PlainArm, Tape::new(0, instructions));
        //The second arm's Reset runs two turns back over steps 2 and 3, where it has a Grab
        let loaded = Loaded::for_test(vec![], vec![arm(0, vec![Grab]), arm(3, vec![RotateClockwise, RotateClockwise, Reset, Grab])]);
        assert_eq!(clash_notes(loaded.timeline.base()), ["Arm 2 crashes at timestep 3: an instruction is inside its Reset's expansion"]);
    }
}
