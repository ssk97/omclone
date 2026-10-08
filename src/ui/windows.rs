//The egui windows
use super::*;

#[cfg(all(target_arch = "wasm32", feature="editor_ui"))]
extern "C" {
    fn save_file(filedata: sapp_jsutils::JsObject, filename: sapp_jsutils::JsObject);
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
        PartId::Glyph(i) => (world.glyphs[i].pos, world.glyphs[i].rot),
        PartId::Arm(i) => (world.arms[i].pos, world.arms[i].rot),
    };
    let is_track = matches!(part, PartId::Glyph(i) if world.glyphs[i].is_track());
    let mut edit = PartEdit::new(part);
    edit.pivot = pos;
    ui.add_enabled_ui(editable, |ui| {
        let (mut x, mut y, mut r) = (pos.x, pos.y, rot);
        ui.horizontal(|ui| {
            ui.label("x:");
            ui.add(egui::DragValue::new(&mut x).speed(0.05));
            ui.label("y:");
            ui.add(egui::DragValue::new(&mut y).speed(0.05));
            //Track rotation isn't stored (the path rotates instead), so it's only changed by dragging
            if !is_track{
                ui.label("rot:");
                ui.add(egui::DragValue::new(&mut r).speed(0.05));
            }
        });
        edit.delta = Pos::new(x, y) - pos;
        edit.rot = r - rot;
        if let PartId::Arm(i) = part{
            let arm = &world.arms[i];
            ui.horizontal(|ui| {
                let mut len = arm.len;
                ui.label("length:");
                ui.add_enabled(!arm.arm_type.is_wheel(), egui::DragValue::new(&mut len).range(1..=3).speed(0.05));
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
            ui.add(egui::DragValue::new(&mut loaded.max_timestep)
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
            let repeat_length_response = ui.add_enabled(EDITOR_ENABLED, egui::DragValue::new(&mut repeat_length)
                .range(min_size..=usize::MAX));
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
    arm_window.default_width(600.).show(egui_ctx, |ui| {
        #[cfg(not(feature = "editor_ui"))]{
            ui.label("Editing Disabled");
        }
        ui.horizontal( |ui| {
            #[cfg(feature = "editor_ui")]{
                ui.checkbox(&mut loaded.tape_mode, "Tape/overwrite mode");
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
                if ui.button("Optimize Pistons").clicked() {
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
        let curr_timestep = loaded.timeline.time().floor() as usize;
        let end_spacing = loaded.max_timestep.saturating_sub(curr_timestep);
        let marker = " ".repeat(curr_timestep+3)+"V"+&" ".repeat(end_spacing);
        //+&(" ".repeat(loaded.max_timestep-loaded.curr_timestep));
        let mut earliest_edit = None;
        let mut target_time = None;
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new(marker).monospace()).wrap_mode(egui::TextWrapMode::Extend));
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (a_num, a) in loaded.timeline.base_mut().arms.iter_mut().enumerate(){
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
                            target_time = Some(cursor.primary.index.0);
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
        if let Some(mut edit_step) = earliest_edit{
            let base = loaded.timeline.base();
            let min_time = base.arms.iter()
                .fold(0, |val, arm| usize::max(val, arm.instruction_tape.instructions.len()+arm.instruction_tape.first));
            if loaded.max_timestep < min_time{
                loaded.max_timestep = min_time;
            }
            let min_loop = base.min_repeat_length();
            if base.repeat_length < min_loop{
                edit_step = edit_step.min(loaded.change_repeat_length(min_loop));
            }
            loaded.apply_base_change(edit_step);
        }
    });
}
pub(super) fn part_window(egui_ctx: &egui::Context, loaded: &mut Loaded){
    egui::Window::new("Part").show(egui_ctx, |ui| {
        ui.add_enabled(EDITOR_ENABLED, egui::Checkbox::new(&mut loaded.allow_overlap, "Allow overlap"));
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
            loaded.selection = clicked;
        }
        ui.separator();
        match loaded.selection {
            Some(part) => {
                let locked = loaded.part_lock(part);
                if let Some(edit) = part_fields(ui, loaded.timeline.base(), part, locked) {
                    if let Err(err) = loaded.apply_edit(&edit) {
                        loaded.message = Some(err);
                    }
                }
            }
            None => {ui.weak("No part selected");}
        }
    });
}
//Names the part picked from the stack under the held mouse button
pub(super) fn stack_tooltip(egui_ctx: &egui::Context, loaded: &Loaded){
    if let Some(press) = &loaded.press {
        let label = part_label(loaded.timeline.base(), press.part());
        let error = press.drag.as_ref().and_then(|d| d.preview.as_ref())
            .and_then(|p| p.world.as_ref().err().cloned());
        egui::Tooltip::always_open(egui_ctx.clone(), egui::LayerId::background(),
            egui::Id::new("part_stack_tooltip"), egui::PopupAnchor::Pointer).show(|ui| {
            ui.label(format!("{label} ({}/{})", press.cand+1, press.stack.len()));
            if let Some(err) = error {
                ui.colored_label(egui::Color32::RED, err);
            }
        });
    }
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
