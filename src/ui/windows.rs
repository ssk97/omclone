//The egui windows
use super::*;

#[cfg(all(target_arch = "wasm32", feature="editor_ui"))]
extern "C" {
    fn save_file(filedata: sapp_jsutils::JsObject, filename: sapp_jsutils::JsObject);
}

//A number that's typed in rather than dragged, applied once on Enter or when the field loses focus
//(each change is a layout edit). Only typed values are clamped to `range`, not the value shown.
fn typed_number<N: egui::emath::Numeric>(value: &mut N, range: std::ops::RangeInclusive<N>) -> egui::DragValue<'_>{
    egui::DragValue::new(value).range(range).clamp_existing_to_range(false).speed(0.).update_while_editing(false)
}
//The selected part's fields, returning an edit if one was changed
//`locked` explains why the fields can't be edited, if they can't
fn part_fields(ui: &mut egui::Ui, world: &World, part: PartId, locked: Option<&str>) -> Option<PartEdit>{
    ui.label(part_label(world, part));
    if let Some(reason) = locked{
        ui.weak(reason);
    }
    let editable = locked.is_none();
    let (pos, rot) = match part{
        //Only a glyph's rotation mod 6 matters (an arm's is kept within -5..=5, like omsim)
        PartId::Glyph(i) => (world.glyphs[i].pos, normalize_dir(world.glyphs[i].rot)),
        PartId::Arm(i) => (world.arms[i].pos, world.arms[i].rot),
    };
    let is_track = matches!(part, PartId::Glyph(i) if world.glyphs[i].is_track());
    let mut edit = PartEdit::new(part);
    edit.pivot = pos;
    ui.add_enabled_ui(editable, |ui| {
        let (mut x, mut y, mut r) = (pos.x, pos.y, rot);
        ui.horizontal(|ui| {
            ui.label("x:");
            ui.add(typed_number(&mut x, -MAX_COORD..=MAX_COORD));
            ui.label("y:");
            ui.add(typed_number(&mut y, -MAX_COORD..=MAX_COORD));
            //Track rotation isn't stored (the path rotates instead), so it's only turned while dragging it on the board
            if !is_track{
                ui.label("rot:");
                ui.add(typed_number(&mut r, -5..=5));
            }
        });
        edit.delta = Pos::new(x, y) - pos;
        edit.rot = r - rot;
        if let PartId::Arm(i) = part{
            let arm = &world.arms[i];
            ui.horizontal(|ui| {
                let mut len = arm.len;
                ui.label("length:");
                ui.add_enabled(!arm.arm_type.is_wheel(), typed_number(&mut len, ARM_LENGTHS));
                edit.len = len - arm.len;
                let mut arm_type = arm.arm_type;
                egui::ComboBox::from_id_salt("arm_type").selected_text(arm_type_name(arm_type)).show_ui(ui, |ui| {
                    for t in ALL_ARM_TYPES{
                        ui.selectable_value(&mut arm_type, t, arm_type_name(t));
                    }
                });
                if arm_type != arm.arm_type{
                    edit.arm_type = Some(arm_type);
                }
            });
        }
    });
    (!edit.is_noop()).then_some(edit)
}
//A closable message, cleared when closed
pub(super) fn message_window(egui_ctx: &egui::Context, id: &str, message: &mut Option<String>){
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
pub(super) fn timeline_window(egui_ctx: &egui::Context, loaded: &mut Loaded){
    egui::Window::new("World loaded").default_pos((0.,0.)).show(egui_ctx, |ui| {
        ui.style_mut().spacing.slider_width = 500.;
        let mut target_time = loaded.timeline.time().floor() as usize;
        if loaded.max_timestep < target_time{
            loaded.max_timestep = target_time;
        }
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut target_time, 0..=loaded.max_timestep)
                .show_value(false));
            ui.add(egui::DragValue::new(&mut target_time)
                .range(0..=loaded.max_timestep)
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
            //Running or long tapes can take it past the limit, which only applies to changing it here
            ui.add(egui::DragValue::new(&mut loaded.max_timestep)
                .range(0..=MAX_TIMESTEP_FIELD).clamp_existing_to_range(false)
                .speed(0.1));
            ui.separator();
            if loaded.max_timestep < target_time{
                loaded.max_timestep = target_time;
            }
            if target_time != loaded.timeline.time().floor() as usize{
                loaded.try_set_target_time(target_time);
            }

            let min_size = loaded.timeline.base().min_repeat_length();
            ui.label("Loop length:");

            let mut repeat_length = loaded.timeline.base().repeat_length;
            //Typed lengths apply on Enter, since each change re-simulates
            let repeat_length_response = ui.add_enabled(EDITOR_ENABLED, egui::DragValue::new(&mut repeat_length)
                .range(min_size..=MAX_TIMESTEP_FIELD.max(min_size)).clamp_existing_to_range(false)
                .update_while_editing(false));
            if repeat_length_response.changed() && repeat_length != loaded.timeline.base().repeat_length{
                let earliest_step = loaded.change_repeat_length(repeat_length);
                loaded.apply_base_change(earliest_step);
            }
            ui.separator();
            let area_check = ui.checkbox(&mut loaded.show_area, "Collide/Area");
            if area_check.changed() && loaded.timeline.set_mark_area(loaded.show_area){
                loaded.clear_crash();
            }
            if loaded.show_area{
                ui.monospace(format!("{:5}",loaded.timeline.curr().area_touched.len()));
            }
            ui.separator();
            ui.label("Speed:");
            ui.add(egui::DragValue::new(&mut loaded.run_speed)
                .range(0.01..=10.0f32).fixed_decimals(2).speed(0.002));
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
                loaded.try_set_target_time(loaded.timeline.time().ceil() as usize);
            }
        });
    });
}
pub(super) fn camera_window(egui_ctx: &egui::Context, loaded: &mut Loaded, screen_size: (f32, f32)){
    egui::Window::new("Camera controls").show(egui_ctx, |ui| {
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
            ui.add(egui::Slider::new(&mut loaded.camera.scale_base,0.002 ..= 0.2)
                .logarithmic(true));
        });
    });
}
#[cfg(feature = "editor_ui")]
pub(super) fn file_window(egui_ctx: &egui::Context, loaded: &mut Loaded, unloaded_info: &mut PathInfo){
    egui::Window::new("File info").show(egui_ctx, |ui| {
        ui.label("solution:");
        ui.text_edit_singleline(&mut loaded.solution.solution_name);
        ui.label("filename:");
        ui.text_edit_singleline(&mut unloaded_info.solution);
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                let dat = &*unloaded_info;
                if !loaded.solution.solution_name.contains("omclone"){
                    loaded.solution.solution_name += "(omclone)";
                }
                let new_solution = parser::create_solution(loaded.timeline.base(),
                    loaded.solution.puzzle_name.clone(),
                    loaded.solution.solution_name.clone());
                //Serialize fully first so a failure doesn't leave a partial file
                let mut data = Vec::new();
                let saved = parser::write_solution(&mut data, &new_solution).and_then(|()| {
                    #[cfg(not(target_arch = "wasm32"))]
                    std::fs::write(Path::new(&dat.base).join(&dat.solution), &data)?;
                    #[cfg(target_arch = "wasm32")]{
                        let filedata = sapp_jsutils::JsObject::buffer(&data);
                        let filename = sapp_jsutils::JsObject::string(&dat.solution);
                        unsafe {save_file(filedata,filename);}
                    }
                    Ok(())
                });
                loaded.message = Some(match saved {
                    Ok(()) => String::from("Saved!"),
                    Err(err) => format!("Save failed: {err}"),
                });
            }
            ui.separator();
            if ui.button("Load").clicked() {
                loaded.popup_reload = true;
            }
        });
    });
}
//`extra_message` reports what "Optimize Pistons" changed
#[cfg_attr(not(feature = "editor_ui"), allow(unused_variables))]
pub(super) fn arms_window(egui_ctx: &egui::Context, loaded: &mut Loaded, extra_message: &mut Option<String>){
    let mut arm_window = egui::Window::new("Arms");
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
                ui.checkbox(&mut loaded.tape_mode, "Tape/overwrite mode").on_hover_text("The Insert key toggles it");
                ui.separator();
                //The blanks these add or remove don't change what runs
                if ui.button("space ends").clicked() {
                    let base = loaded.timeline.base_mut();
                    for a in &mut base.arms{
                        let instr = &mut a.instruction_tape;
                        for _ in instr.instructions.len()..base.repeat_length{
                            a.instruction_tape.instructions.push(Instr::Empty);
                        }
                    }
                }
                ui.separator();
                if ui.button("clear ends").clicked() {
                    for a in &mut loaded.timeline.base_mut().arms{
                        let instr = &mut a.instruction_tape;
                        while instr.instructions.last() == Some(&Instr::Empty){
                            instr.instructions.pop();
                        }
                    }
                }
                let lock = loaded.edit_lock();
                let optimize = ui.add_enabled(lock.is_none(), egui::Button::new("Optimize Pistons"));
                if locked_hover(optimize, lock).clicked() {
                    let mut pistons_added = 0;
                    let mut pistons_removed = 0;
                    //Changed on a copy, so a failed setup leaves the solution as it was
                    let mut layout = loaded.timeline.base().layout();
                    for a in &mut layout.arms{
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
                    if pistons_added + pistons_removed > 0 {
                        match World::setup_sim(&layout) {
                            Ok(world) => loaded.commit_world(world),
                            Err(err) => loaded.message = Some(err.to_string()),
                        }
                    }
                    *extra_message = Some(format!("{pistons_added} pistons added, {pistons_removed} pistons removed"));
                }
                ui.separator();
            }
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
fn locked_hover(response: egui::Response, lock: Option<&str>) -> egui::Response{
    match lock{
        Some(reason) => response.on_disabled_hover_text(reason),
        None => response,
    }
}
pub(super) fn part_window(egui_ctx: &egui::Context, loaded: &mut Loaded){
    egui::Window::new("Part").show(egui_ctx, |ui| {
        ui.horizontal(|ui| {
            let lock = loaded.history_lock();
            let undo = ui.add_enabled(lock.is_none() && loaded.history.can_undo(), egui::Button::new("Undo"));
            if locked_hover(undo.on_hover_text("Ctrl+Z"), lock).clicked(){
                loaded.undo();
            }
            let redo = ui.add_enabled(lock.is_none() && loaded.history.can_redo(), egui::Button::new("Redo"));
            if locked_hover(redo.on_hover_text("Ctrl+Y"), lock).clicked(){
                loaded.redo();
            }
            ui.separator();
            add_part_menu(ui, loaded);
            ui.separator();
            ui.add_enabled(EDITOR_ENABLED, egui::Checkbox::new(&mut loaded.allow_overlap, "Allow overlap"));
        });
        ui.separator();
        ui.label("Parts here:");
        let highlighted = loaded.highlighted_part();
        let mut clicked = None;
        match &loaded.stack_view {
            Some(stack) => for &part in stack {
                if ui.selectable_label(highlighted == Some(part), part_label(loaded.timeline.base(), part)).clicked() {
                    clicked = Some(part);
                }
            },
            None => {ui.weak("(click the board)");}
        }
        if clicked.is_some() {
            loaded.select(clicked);
        }
        ui.separator();
        match loaded.selection {
            Some(part) => part_section(ui, loaded, part),
            None => {ui.weak("No part selected");}
        }
    });
}
fn part_section(ui: &mut egui::Ui, loaded: &mut Loaded, part: PartId){
    let mut edit = None;
    let locked = loaded.part_lock(part, EditKind::Move);
    if let Some(part_edit) = part_fields(ui, loaded.timeline.base(), part, locked) {
        edit = Some(LayoutEdit::Part(part_edit));
    }
    if let PartId::Arm(i) = part{
        let arm_count = loaded.timeline.base().arms.len();
        let lock = loaded.part_lock(part, EditKind::Reorder);
        ui.horizontal(|ui| {
            ui.label("Arm #:");
            let mut number = i+1;
            let field = ui.add_enabled(lock.is_none(), typed_number(&mut number, 1..=arm_count));
            locked_hover(field.on_hover_text("Arms run in this order"), lock);
            if number != i+1{
                edit = Some(LayoutEdit::MoveArm{from: i, to: number-1});
            }
        });
    }
    let lock = loaded.part_lock(part, EditKind::Delete);
    let delete = ui.add_enabled(lock.is_none(), egui::Button::new("Delete"));
    if locked_hover(delete.on_hover_text("Delete or Backspace"), lock).clicked(){
        edit = Some(LayoutEdit::Delete(part));
    }
    if let Some(edit) = edit{
        if let Err(err) = loaded.apply_edit(&edit) {
            loaded.message = Some(err);
        }
        return;
    }
    if let PartId::Glyph(i) = part{
        if loaded.timeline.base().glyphs[i].is_track(){
            track_section(ui, loaded, i);
        }
    }
}
//The path text being edited, and the track's text when editing started
pub(super) struct PathText{
    glyph: usize,
    shown: String,
    text: String,
}
const PATH_TEXT_HELP: &str = "Steps from the first hex: w e a d z x (the keys around S), or (dx,dy) to jump. \
    Applied on Enter; anything from the first unreadable part on is dropped.";
fn track_section(ui: &mut egui::Ui, loaded: &mut Loaded, glyph: usize){
    let path = track_path(loaded.timeline.base(), glyph);
    ui.separator();
    let mut info = format!("Track: {} hex{} · {} · cost {}", path.len(), if path.len() == 1 {""} else {"es"},
        if path_loops(&path) {"loops"} else {"doesn't loop"}, path.len()*5);
    let gaps = gap_count(&path);
    if gaps > 0{
        info += &format!(" · {gaps} gap{}", if gaps == 1 {""} else {"s"});
    }
    ui.label(info);
    let lock = loaded.edit_lock();
    let mut new_path = None;
    let mut finished = false;
    ui.add_enabled_ui(lock.is_none(), |ui| {
        ui.horizontal(|ui| {
            let current = loaded.track_mode.as_ref().filter(|m| m.glyph == Some(glyph)).map(|m| m.tool);
            for (tool, label) in [(TrackTool::AddFront, "Add to front"), (TrackTool::AddBack, "Add to back"),
                (TrackTool::Remove, "Remove hexes")]
            {
                let button = ui.selectable_label(current == Some(tool), label)
                    .on_hover_text("Then click or drag over the board. Esc or right-click to finish.");
                if button.clicked(){
                    loaded.set_track_tool(glyph, (current != Some(tool)).then_some(tool));
                }
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut loaded.allow_gaps, "Allow gaps")
                .on_hover_text("Lets the board tools add hexes that aren't next to the end, and remove middle hexes");
            if ui.button("Reverse").on_hover_text("Swaps which way Forward and Back move arms along it").clicked(){
                new_path = Some(path.iter().rev().copied().collect());
            }
        });
        let shown = path_to_text(&path);
        if loaded.path_text.as_ref().is_none_or(|p| p.glyph != glyph || p.shown != shown){
            loaded.path_text = Some(PathText{glyph, shown: shown.clone(), text: shown});
        }
        let path_text = loaded.path_text.as_mut().unwrap();
        ui.horizontal(|ui| {
            ui.label("Path:");
            let field = ui.add(egui::TextEdit::singleline(&mut path_text.text).code_editor().desired_width(f32::INFINITY));
            if field.on_hover_text(PATH_TEXT_HELP).lost_focus(){
                //Escape also ends editing, but drops the change
                finished = true;
                if !ui.input(|i| i.key_pressed(egui::Key::Escape)){
                    new_path = Some(parse_path_text(&path_text.text, path[0]));
                }
            }
        });
        //Shows the track as it ends up, whether or not the change is made
        if finished{
            loaded.path_text = None;
        }
    });
    if let Some(new_path) = new_path.filter(|p| *p != path){
        if let Err(err) = loaded.apply_edit(&LayoutEdit::SetTrack{glyph: Some(glyph), path: new_path}) {
            loaded.message = Some(err);
        }
    }
}
//Picks a part to place on the board (or a track to draw)
fn add_part_menu(ui: &mut egui::Ui, loaded: &mut Loaded){
    let lock = loaded.edit_lock();
    let menu = ui.add_enabled_ui(lock.is_none(), |ui| {
        ui.menu_button("Add part", |ui| {
            ui.set_max_width(320.);
            ui.weak("Then click the board. Shift+click places more; Esc or right-click stops.");
            ui.label("Arms");
            ui.horizontal_wrapped(|ui| {
                for arm_type in ALL_ARM_TYPES{
                    new_part_button(ui, loaded, NewPart::Arm(arm_type));
                }
            });
            ui.label("Glyphs");
            ui.horizontal_wrapped(|ui| {
                for glyph_type in ALL_NEW_GLYPHS{
                    new_part_button(ui, loaded, NewPart::Glyph(glyph_type));
                }
            });
            let drawing = loaded.track_mode.as_ref().is_some_and(|m| m.glyph.is_none());
            let track = ui.selectable_label(drawing, "Track").on_hover_text("Click the board to start it, and drag to draw it");
            if track.clicked(){
                if drawing {loaded.end_track_mode()} else {loaded.start_new_track()}
                ui.close();
            }
        });
    });
    locked_hover(menu.response, lock);
}
fn new_part_button(ui: &mut egui::Ui, loaded: &mut Loaded, part: NewPart){
    let placing = loaded.placing.as_ref().is_some_and(|p| p.part == part);
    if ui.selectable_label(placing, part.name()).clicked(){
        if placing {loaded.placing = None} else {loaded.start_placing(part)}
        ui.close();
    }
}
//What the mouse is doing on the board: the part picked from the stack under it, or the part or tool it's using
pub(super) fn cursor_tooltip(egui_ctx: &egui::Context, loaded: &Loaded){
    let base = loaded.timeline.base();
    let (title, note) = if let Some(press) = &loaded.press {
        let copy = if press.duplicate {", copying"} else {""};
        let title = format!("{} ({}/{}){copy}", part_label(base, press.part()), press.cand+1, press.stack.len());
        //Why it isn't moving, unless nothing can be edited anyway
        let lock = loaded.press_lock().filter(|_| EDITOR_ENABLED && press.drag.is_none());
        (title, lock.map(str::to_owned))
    } else if loaded.pointer_over_ui {
        return;
    } else if let Some(mode) = &loaded.track_mode {
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
    } else if let Some(placing) = &loaded.placing {
        (format!("Placing {}", placing.part.name()), Some(String::from("Shift+click to place more; Esc or right-click to stop")))
    } else {
        return;
    };
    let refusal = loaded.stroke.as_ref().and_then(|s| s.refusal).map(str::to_owned);
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
pub(super) fn reload_window(egui_ctx: &egui::Context, loaded: &mut Loaded) -> AppStateUpdate{
    let mut do_loading = AppStateUpdate::NoChange;
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
    do_loading
}
#[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
pub(super) fn not_loaded_window(egui_ctx: &egui::Context, unloaded_info: &mut PathInfo) -> AppStateUpdate{
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
