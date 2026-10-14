//The egui windows
use super::*;

//A number that's typed in rather than dragged, applied once on Enter or when the field loses focus
//(each change is a layout edit). Only typed values are clamped to `range`, not the value shown.
fn typed_number<N: egui::emath::Numeric>(value: &mut N, range: std::ops::RangeInclusive<N>) -> egui::DragValue<'_>{
    egui::DragValue::new(value).range(range).clamp_existing_to_range(false).speed(0.).update_while_editing(false)
}
//The selected part's fields (where it starts, in `world`), returning an edit if one was changed. An arm also shows
//where it is in `now`, the current timestep's world.
//`locked` explains why the fields can't be edited, if they can't
fn part_fields(ui: &mut egui::Ui, world: &World, now: &World, part: PartId, locked: Option<&str>) -> Option<PartEdit>{
    ui.label(part_label(world, part));
    //Shown even when it matches the fields, so the window doesn't change height as the timeline runs
    if let PartId::Arm(i) = part{
        let arm = &now.arms[i];
        ui.weak(format!("Now: ({}, {}), rot {}, length {}", arm.pos.x, arm.pos.y, arm.rot, arm.len))
            .on_hover_text("Where it is at this timestep. The fields are where it starts.");
    }
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
            ui.add(egui::DragValue::new(&mut loaded.run_speed)
                .range(0.01..=10.0f32).fixed_decimals(2).speed(0.002));
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
pub(super) fn crash_window(egui_ctx: &egui::Context, loaded: &mut Loaded){
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
pub(super) fn camera_window(egui_ctx: &egui::Context, loaded: &mut Loaded, screen_size: (f32, f32)){
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
    });
    loaded.camera_window_open = open;
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
//What loading changed, and what omsim would reject. A long list ends with how many more there are.
#[cfg(feature = "editor_ui")]
fn file_notes(ui: &mut egui::Ui, loaded: &Loaded){
    const SHOWN: usize = 4;
    let notes: Vec<&String> = loaded.load_notes.iter().chain(&loaded.rule_notes).collect();
    let color = ui.visuals().warn_fg_color;
    for note in notes.iter().take(SHOWN){
        ui.colored_label(color, note.as_str());
    }
    if notes.len() > SHOWN{
        ui.colored_label(color, format!("…and {} more", notes.len() - SHOWN));
    }
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
    //Before the part's fields are drawn, since the press may end the one being typed in and apply it
    if egui_ctx.input(|i| i.pointer.primary_pressed()){
        loaded.list_press_numbering = loaded.numbering_version;
    }
    egui::Window::new("Part").min_width(readout_width(egui_ctx)).show(egui_ctx, |ui| {
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
            ui.add_enabled(EDITOR_ENABLED, egui::Checkbox::new(&mut loaded.allow_overlap, "Allow overlap"))
                .on_hover_text("Lets parts overlap each other, and break production puzzles' chamber rules");
        });
        ui.horizontal(|ui| {
            //First, so the readout changing length doesn't move it
            ui.toggle_value(&mut loaded.camera_window_open, "Camera")
                .on_hover_text("Camera controls, for moving and zooming without a mouse wheel or right button");
            readout_label(ui, loaded, mouse_readout(loaded));
        });
        //Kept, blank, while nothing is selected, so selecting a part doesn't move what's below
        readout_label(ui, loaded, offset_readout(loaded).unwrap_or_default());
        ui.separator();
        ui.horizontal(|ui| {
            for (all, label, hover) in [
                (false, "Parts here", "The parts on the hex last clicked, topmost first. Drag a glyph to reorder it."),
                (true, "All glyphs", "Every glyph, topmost first. Drag one to reorder it."),
            ]{
                if ui.selectable_label(loaded.all_glyphs == all, label).on_hover_text(format!("{hover} {GLYPH_ORDER_HELP}")).clicked(){
                    loaded.all_glyphs = all;
                }
            }
        });
        let action = part_list(ui, loaded);
        ui.separator();
        //Each part's fields keep their own egui state, so text typed for one part is never applied to another
        match loaded.selection {
            Some(part) => {ui.push_id(part, |ui| part_section(ui, loaded, part));}
            None => {ui.weak("No part selected");}
        }
        //After the fields are drawn, so that one being typed in (which the click ends) goes to its own part first.
        //That can renumber the parts (as can an undo or Delete during a drag), and then the part clicked or dragged
        //may have another number, so the click or drop does nothing.
        let action = action.filter(|_| loaded.list_press_numbering == loaded.numbering_version);
        match action {
            Some(ListAction::Select(part)) => loaded.select(Some(part)),
            Some(ListAction::Reorder(edit)) => if let Err(err) = loaded.apply_edit(&edit) {
                loaded.message = Some(err);
            },
            None => {}
        }
    });
}
const GLYPH_ORDER_HELP: &str = "Glyphs with higher numbers are drawn on top and act after lower ones. Where tracks \
    overlap, the lower-numbered track wins.";
//What was done in the Part window's list of parts
enum ListAction{
    Select(PartId),
    Reorder(LayoutEdit),
}
//The glyph being dragged in the list of parts
struct DraggedGlyph(usize);
//How many rows the list of parts shows before it scrolls
const PART_LIST_ROWS: f32 = 12.;
//The parts on the hex last clicked, or every glyph. Clicking one selects it, and glyphs can be dragged above or below
//another glyph to reorder them, unless the parts are renumbered in between (see part_window).
fn part_list(ui: &mut egui::Ui, loaded: &Loaded) -> Option<ListAction>{
    let base = loaded.timeline.base();
    let parts: Vec<PartId> = if loaded.all_glyphs {
        (0..base.glyphs.len()).rev().map(PartId::Glyph).collect()
    } else {
        match loaded.list_hex {
            //Arms are listed where they are now, as they're picked on the board
            Some(hex) => loaded.timeline.curr().parts_at(hex),
            None => {
                ui.weak("(click the board)");
                return None;
            }
        }
    };
    if parts.is_empty() {
        ui.weak(if loaded.all_glyphs {"(no glyphs)"} else {"(nothing here now)"});
        return None;
    }
    let row_height = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
    let ctx = ui.ctx().clone();
    //A drag that the glyphs were renumbered during would move the wrong one (see part_window), so it's abandoned
    let current = loaded.list_press_numbering == loaded.numbering_version;
    if !current {
        egui::DragAndDrop::clear_payload(&ctx);
    }
    let dragged = egui::DragAndDrop::payload::<DraggedGlyph>(&ctx).map(|d| d.0);
    //Scrolls to the selection when it, or the list shown, changes
    let shown_id = egui::Id::new("part_list_shown");
    let shown = (loaded.selection, loaded.all_glyphs);
    let scroll_to_selection = ctx.data(|d| d.get_temp(shown_id)) != Some(shown);
    ctx.data_mut(|d| d.insert_temp(shown_id, shown));
    let highlighted = loaded.highlighted_part();
    let can_reorder = loaded.edit_lock().is_none();
    egui::ScrollArea::vertical().id_salt("part_list").max_height(PART_LIST_ROWS * row_height).show(ui, |ui| {
        let mut action = None;
        for &part in &parts {
            //A dragged glyph's drop target is lit up under its label
            let backdrop = ui.painter().add(egui::Shape::Noop);
            let mut row = ui.selectable_label(highlighted == Some(part), part_label(base, part));
            if scroll_to_selection && loaded.selection == Some(part) {
                row.scroll_to_me(None);
            }
            if row.clicked() {
                action = Some(ListAction::Select(part));
            }
            let PartId::Glyph(glyph) = part else {continue};
            if can_reorder {
                row = row.interact(egui::Sense::drag());
                if current {
                    row.dnd_set_drag_payload(DraggedGlyph(glyph));
                }
            }
            let Some(from) = dragged else {continue};
            //The gaps between rows count as the nearer row's, so the target doesn't flicker off between them
            let area = row.rect.expand2(egui::vec2(0., ui.spacing().item_spacing.y / 2.));
            let Some(pointer) = ui.ctx().pointer_latest_pos().filter(|_| ui.rect_contains_pointer(area)) else {continue};
            //Higher rows have higher numbers, so the top half puts it after this glyph
            let after = pointer.y < row.rect.center().y;
            let Some(to) = reorder_beside(from, glyph, after) else {continue};
            let selection = ui.visuals().selection;
            ui.painter().set(backdrop, egui::Shape::rect_filled(row.rect, 2., selection.bg_fill.gamma_multiply(0.4)));
            let edge = if after {row.rect.top()} else {row.rect.bottom()};
            ui.painter().hline(row.rect.x_range(), edge, egui::Stroke::new(2., selection.stroke.color));
            if ui.input(|i| i.pointer.any_released()) {
                egui::DragAndDrop::clear_payload(&ctx);
                action = Some(ListAction::Reorder(LayoutEdit::ReorderGlyph{from, to}));
            }
        }
        if dragged.is_some() {
            drag_edge_scroll(ui, ui.clip_rect());
        }
        action
    }).inner
}
//While something is dragged, scrolls the scroll area `ui` is in when the pointer is near or past the top or bottom of
//`rect` (its visible part), faster the further in it goes. egui doesn't scroll while dragging.
pub(super) fn drag_edge_scroll(ui: &egui::Ui, rect: egui::Rect){
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
//Over a window, the readout is where the mouse last was on the board
fn readout_label(ui: &mut egui::Ui, loaded: &Loaded, text: String){
    let text = egui::RichText::new(text).monospace();
    ui.label(if loaded.pointer_over_ui {text.weak()} else {text});
}
//The hex under the mouse
fn mouse_readout(loaded: &Loaded) -> String{
    match loaded.hovered_hex{
        Some(hex) => format!("Mouse: ({}, {})", hex.x, hex.y),
        None => String::from("Mouse: –"),
    }
}
//How far the hex under the mouse is from the selected part
fn offset_readout(loaded: &Loaded) -> Option<String>{
    let (hex, part) = loaded.hovered_hex.zip(loaded.selection)?;
    let world = loaded.timeline.base();
    let pos = match part{
        PartId::Glyph(i) => world.glyphs[i].pos,
        PartId::Arm(i) => world.arms[i].pos,
    };
    Some(offset_text(hex - pos, &part_label(world, part), hex_distance(pos, hex)))
}
//The part comes first, so it stays put as the numbers after it change length
fn offset_text(offset: Pos, from: &str, distance: i32) -> String{
    format!("From {from}: ({:+}, {:+}), {distance} away", offset.x, offset.y)
}
//Wide enough for the offset readout with the longest part names, so the Part window doesn't grow (and, against the
//right edge of the screen, jump left) as the mouse moves or the selection changes
fn readout_width(egui_ctx: &egui::Context) -> f32{
    let longest = offset_text(Pos::new(-99, -99), "Polymer output 0 (glyph 99)", 999);
    let style = egui_ctx.global_style();
    let font = egui::TextStyle::Monospace.resolve(&style);
    let text_width = egui_ctx.fonts_mut(|f| f.glyph_width(&font, '0')) * longest.chars().count() as f32;
    //A window's width includes its frame
    text_width + egui::Frame::window(&style).total_margin().sum().x
}
fn part_section(ui: &mut egui::Ui, loaded: &mut Loaded, part: PartId){
    let mut edit = None;
    let locked = loaded.part_lock(part, EditKind::Move);
    if let Some(part_edit) = part_fields(ui, loaded.timeline.base(), loaded.timeline.curr(), part, locked) {
        edit = Some(LayoutEdit::Part(part_edit));
    }
    let base = loaded.timeline.base();
    let (label, index, count, hover) = match part{
        PartId::Arm(i) => ("Arm #:", i, base.arms.len(), "Arms run in this order"),
        PartId::Glyph(i) => ("Glyph #:", i, base.glyphs.len(), GLYPH_ORDER_HELP),
    };
    let lock = loaded.part_lock(part, EditKind::Reorder);
    ui.horizontal(|ui| {
        ui.label(label);
        let mut number = index+1;
        let field = ui.add_enabled(lock.is_none(), typed_number(&mut number, 1..=count));
        locked_hover(field.on_hover_text(hover), lock);
        if number != index+1{
            let (from, to) = (index, number-1);
            edit = Some(match part{
                PartId::Arm(_) => LayoutEdit::ReorderArm{from, to},
                PartId::Glyph(_) => LayoutEdit::ReorderGlyph{from, to},
            });
        }
    });
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
    pub(super) glyph: usize,
    pub(super) shown: String,
    pub(super) text: String,
    //The field has focus, so the board previews the text
    pub(super) editing: bool,
    //Where typing goes in the text (in characters): the caret, or the start of the selection it replaces
    pub(super) caret: usize,
}
const PATH_TEXT_HELP: &str = "Steps from the first hex: w e a d z x (the keys around S), or (dx,dy) to jump. \
    Shown on the board as you type, and applied on Enter (Esc drops it); anything from the first unreadable \
    part on is dropped.";
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
            let current = loaded.tool.track_mode().filter(|m| m.glyph == Some(glyph)).map(|m| m.tool);
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
            loaded.path_text = Some(PathText{glyph, shown: shown.clone(), text: shown, editing: false, caret: 0});
        }
        let path_text = loaded.path_text.as_mut().unwrap();
        let mut started = false;
        ui.horizontal(|ui| {
            ui.label("Path:");
            let output = egui::TextEdit::singleline(&mut path_text.text).code_editor().desired_width(f32::INFINITY).show(ui);
            started = output.response.gained_focus();
            path_text.editing = output.response.has_focus();
            if let Some(range) = output.cursor_range{
                path_text.caret = range.as_sorted_char_range().start.0;
            }
            if output.response.response.on_hover_text(PATH_TEXT_HELP).lost_focus(){
                //Escape also ends editing, but drops the change
                finished = true;
                if !ui.input(|i| i.key_pressed(egui::Key::Escape)){
                    new_path = Some(parse_path_text(&path_text.text, path[0]).0);
                }
            }
        });
        if let (_, Some(stop)) = parse_path_text(&path_text.text, path[0]){
            let rest = &path_text.text[stop..];
            let mut shown_rest: String = rest.chars().take(12).collect();
            if shown_rest.len() < rest.len(){
                shown_rest.push('…');
            }
            ui.colored_label(ui.visuals().warn_fg_color, format!("Stops reading at \"{shown_rest}\" (the rest is dropped)"));
        }
        //A click on the board applies the typed path, so it shouldn't also draw track or place a part
        if started{
            loaded.end_tool();
        }
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
            let drawing = loaded.tool.track_mode().is_some_and(|m| m.glyph.is_none());
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
    let placing = loaded.tool.placing().is_some_and(|p| p.part == part);
    if ui.selectable_label(placing, part.name()).clicked(){
        if placing {loaded.end_tool()} else {loaded.start_placing(part)}
        ui.close();
    }
}
//What the mouse is doing on the board: the part picked from the stack under it, or the part or tool it's using
pub(super) fn cursor_tooltip(egui_ctx: &egui::Context, loaded: &Loaded){
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
pub(super) fn reload_window(egui_ctx: &egui::Context, loaded: &mut Loaded) -> AppStateUpdate{
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

//The Part window as the app's frames see it: fields typed in, clicks, and glyphs dragged in the list of parts.
//Debug builds only: the tests find widgets by type and label, which egui records only with debug assertions on.
#[cfg(all(test, feature = "editor_ui", debug_assertions))]
mod tests {
    use super::*;
    use egui::{Event, Pos2, RawInput, WidgetType};
    use loaded::TEST_SCREEN;

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        ctx.options_mut(|o| o.input_options.surrender_focus_on = egui::SurrenderFocusOn::Presses);
        //Records each widget's type and label, to find them by
        ctx.global_style_mut(|s| s.debug.show_interactive_widgets = true);
        ctx
    }
    //Draws the Part window for a frame with these events, then applies the board input that waited for it
    fn frame(ctx: &egui::Context, loaded: &mut Loaded, events: Vec<Event>) {
        let _ = ctx.run_ui(RawInput{events, ..Default::default()}, |ui| part_window(ui.ctx(), loaded));
        loaded.flush_board_input();
    }
    //Where the first widget drawn last frame that matches is
    fn widget_rect(ctx: &egui::Context, matches: impl Fn(&egui::WidgetInfo) -> bool) -> Option<egui::Rect> {
        ctx.viewport(|v| {
            let widgets = &v.prev_pass.widgets;
            widgets.layers().flat_map(|(_, rects)| rects)
                .find(|w| widgets.info(w.id).is_some_and(&matches)).map(|w| w.rect)
        })
    }
    //The center of the first widget drawn last frame that matches
    fn widget(ctx: &egui::Context, matches: impl Fn(&egui::WidgetInfo) -> bool) -> Pos2 {
        widget_rect(ctx, matches).expect("no such widget drawn").center()
    }
    //A row of the list of parts (or any widget with this label)
    fn row(ctx: &egui::Context, label: &str) -> egui::Rect {
        widget_rect(ctx, |w| w.label.as_deref() == Some(label)).unwrap_or_else(|| panic!("no row {label:?}"))
    }
    fn top_half(rect: egui::Rect) -> Pos2 {
        rect.center() - egui::vec2(0., rect.height() / 4.)
    }
    fn bottom_half(rect: egui::Rect) -> Pos2 {
        rect.center() + egui::vec2(0., rect.height() / 4.)
    }
    //A new window takes a few frames to settle into place
    fn settle(ctx: &egui::Context, loaded: &mut Loaded) {
        for _ in 0..3 {
            frame(ctx, loaded, vec![]);
        }
    }
    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton{pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE}
    }
    //Presses at `from` and moves to `to` over a few frames
    fn press_and_move(ctx: &egui::Context, loaded: &mut Loaded, from: Pos2, to: Pos2) {
        frame(ctx, loaded, vec![Event::PointerMoved(from), button(from, true)]);
        for i in 1..=4 {
            frame(ctx, loaded, vec![Event::PointerMoved(from + (to - from) * (i as f32 / 4.))]);
        }
    }
    //Presses at `from`, moves to `to` over a few frames, and releases there
    fn drag(ctx: &egui::Context, loaded: &mut Loaded, from: Pos2, to: Pos2) {
        press_and_move(ctx, loaded, from, to);
        frame(ctx, loaded, vec![button(to, false)]);
    }
    //A press and release in one frame
    fn click(pos: Pos2) -> Vec<Event> {
        vec![Event::PointerMoved(pos), button(pos, true), button(pos, false)]
    }
    //Clicks the field at `at` and types `text` into it
    fn type_at(ctx: &egui::Context, loaded: &mut Loaded, at: Pos2, text: &str) {
        frame(ctx, loaded, click(at));
        frame(ctx, loaded, vec![]);
        frame(ctx, loaded, vec![Event::Text(text.into())]);
        assert!(ctx.text_edit_focused());
    }
    //Clicks the first widget of this type and types `text` into it
    fn type_into(ctx: &egui::Context, loaded: &mut Loaded, typ: WidgetType, text: &str) {
        settle(ctx, loaded);
        let at = widget(ctx, |w| w.typ == typ);
        type_at(ctx, loaded, at, text);
    }
    //The center of the number field on the same line as this label, as drawn last frame
    fn number_field(ctx: &egui::Context, label: &str) -> Pos2 {
        let line = row(ctx, label).y_range();
        ctx.viewport(|v| {
            let widgets = &v.prev_pass.widgets;
            widgets.layers().flat_map(|(_, rects)| rects)
                .find(|w| line.contains(w.rect.center().y)
                    && widgets.info(w.id).is_some_and(|i| i.typ == WidgetType::DragValue))
                .map(|w| w.rect.center())
        }).unwrap_or_else(|| panic!("no number field beside {label:?}"))
    }
    //A press and release on the board at a hex, in one frame (like a touchpad tap). egui sees it off its windows.
    fn tap_board(ctx: &egui::Context, loaded: &mut Loaded, x: i32, y: i32) {
        let px = loaded.test_px(x, y);
        let typing = ctx.text_edit_focused();
        loaded.board_input(BoardInput::Press{px, screen: TEST_SCREEN, keymods: KeyMods::default()}, typing);
        loaded.board_input(BoardInput::Release, typing);
        frame(ctx, loaded, click(Pos2::new(2000., 2000.)));
    }
    fn bonder(x: i32, y: i32) -> Glyph {
        Glyph{pos: Pos::new(x, y), rot: 0, glyph_type: GlyphType::Bonding}
    }
    fn positions(loaded: &Loaded) -> Vec<Pos> {
        loaded.timeline.base().glyphs.iter().map(|g| g.pos).collect()
    }
    fn glyph(x: i32, y: i32, glyph_type: GlyphType) -> Glyph {
        Glyph{pos: Pos::new(x, y), rot: 0, glyph_type}
    }
    fn glyph_types(loaded: &Loaded) -> Vec<GlyphType> {
        loaded.timeline.base().glyphs.iter().map(|g| g.glyph_type.clone()).collect()
    }
    //A solution with these glyphs, its Part window listing the parts at (0,0)
    fn listing_origin(ctx: &egui::Context, glyphs: Vec<Glyph>, arms: Vec<Arm>) -> Loaded {
        let mut loaded = Loaded::for_test(glyphs, arms);
        loaded.list_hex = Some(Pos::new(0, 0));
        settle(ctx, &mut loaded);
        loaded
    }

    #[test]
    fn a_glyph_dropped_on_a_rows_top_half_goes_above_it() {
        use GlyphType::*;
        let ctx = context();
        let mut loaded = listing_origin(&ctx, vec![glyph(0, 0, Bonding), glyph(0, 0, Calcification),
            glyph(0, 0, Duplication)], vec![]);
        let to = top_half(row(&ctx, "Duplication (glyph 3)"));
        drag(&ctx, &mut loaded, row(&ctx, "Bonder (glyph 1)").center(), to);
        assert_eq!(glyph_types(&loaded), vec![Calcification, Duplication, Bonding]);
        assert_eq!(loaded.selection, Some(PartId::Glyph(2)));
        frame(&ctx, &mut loaded, vec![]);
        row(&ctx, "Bonder (glyph 3)");
    }

    #[test]
    fn the_two_drop_points_between_rows_go_either_side_of_glyphs_elsewhere() {
        use GlyphType::*;
        //Glyphs 2 and 3 aren't on the hex, so they aren't listed between glyphs 4 and 1
        let glyphs = vec![glyph(0, 0, Bonding), glyph(5, 5, Calcification), glyph(9, 9, Disposal),
            glyph(0, 0, Projection), glyph(0, 0, Duplication)];
        let dropped = |target: &str, half: fn(egui::Rect) -> Pos2| {
            let ctx = context();
            let mut loaded = listing_origin(&ctx, glyphs.clone(), vec![]);
            let to = half(row(&ctx, target));
            drag(&ctx, &mut loaded, row(&ctx, "Duplication (glyph 5)").center(), to);
            glyph_types(&loaded)
        };
        assert_eq!(dropped("Projection (glyph 4)", bottom_half),
            vec![Bonding, Calcification, Disposal, Duplication, Projection]);
        assert_eq!(dropped("Bonder (glyph 1)", top_half),
            vec![Bonding, Duplication, Calcification, Disposal, Projection]);
    }

    #[test]
    fn a_glyph_dropped_on_an_arm_stays_put() {
        use GlyphType::*;
        let ctx = context();
        let arm = Arm::new(Pos::new(0, 0), 0, 1, ArmType::PlainArm, Tape::new(0, vec![]));
        let mut loaded = listing_origin(&ctx, vec![glyph(0, 0, Bonding), glyph(0, 0, Calcification)], vec![arm]);
        let to = top_half(row(&ctx, "Arm 1 (Arm)"));
        drag(&ctx, &mut loaded, row(&ctx, "Bonder (glyph 1)").center(), to);
        assert_eq!(glyph_types(&loaded), vec![Bonding, Calcification]);
        assert!(!loaded.history.can_undo());
    }

    #[test]
    fn all_glyphs_lists_and_reorders_glyphs_elsewhere() {
        use GlyphType::*;
        let ctx = context();
        let mut loaded = Loaded::for_test(vec![glyph(0, 0, Bonding), glyph(5, 5, Calcification)], vec![]);
        loaded.list_hex = Some(Pos::new(0, 0));
        loaded.all_glyphs = true;
        settle(&ctx, &mut loaded);
        let to = top_half(row(&ctx, "Calcification (glyph 2)"));
        drag(&ctx, &mut loaded, row(&ctx, "Bonder (glyph 1)").center(), to);
        assert_eq!(glyph_types(&loaded), vec![Calcification, Bonding]);
    }

    //A typed Glyph # that the click applied renumbered the glyphs before the click took effect, which then
    //selected whichever glyph had the clicked row's number
    #[test]
    fn a_click_that_applies_a_typed_glyph_number_selects_nothing_else() {
        use GlyphType::*;
        for separate_frames in [false, true] {
            let ctx = context();
            let mut loaded = Loaded::for_test(vec![glyph(0, 0, Bonding), glyph(3, 0, Calcification),
                glyph(6, 0, Duplication)], vec![]);
            loaded.all_glyphs = true;
            loaded.select(Some(PartId::Glyph(0)));
            settle(&ctx, &mut loaded);
            type_at(&ctx, &mut loaded, number_field(&ctx, "Glyph #:"), "3");
            let other = row(&ctx, "Calcification (glyph 2)").center();
            if separate_frames {
                frame(&ctx, &mut loaded, vec![Event::PointerMoved(other), button(other, true)]);
                frame(&ctx, &mut loaded, vec![button(other, false)]);
            } else {
                frame(&ctx, &mut loaded, click(other));
            }
            assert_eq!(glyph_types(&loaded), vec![Calcification, Duplication, Bonding], "the typed number applies");
            assert_eq!(loaded.selection, Some(PartId::Glyph(2)), "the bonder stays selected");
        }
    }

    //An undo during a drag renumbered the glyphs, and the drop moved the wrong one, or panicked when the dragged
    //glyph was gone
    #[test]
    fn a_drag_that_undo_renumbers_the_glyphs_during_does_nothing() {
        use GlyphType::*;
        let ctx = context();
        let mut loaded = Loaded::for_test(vec![glyph(0, 0, Bonding), glyph(3, 0, Calcification)], vec![]);
        loaded.all_glyphs = true;
        let add = LayoutEdit::Add{part: NewPart::Glyph(Duplication), pos: Pos::new(6, 0), rot: 0, len: 1};
        assert_eq!(loaded.apply_edit(&add), Ok(()));
        settle(&ctx, &mut loaded);
        let from = row(&ctx, "Duplication (glyph 3)").center();
        press_and_move(&ctx, &mut loaded, from, from + egui::vec2(0., 10.));
        loaded.undo();
        frame(&ctx, &mut loaded, vec![]);
        //The rows moved up when the added glyph's row went away
        let to = bottom_half(row(&ctx, "Bonder (glyph 1)"));
        frame(&ctx, &mut loaded, vec![Event::PointerMoved(to)]);
        frame(&ctx, &mut loaded, vec![button(to, false)]);
        assert_eq!(glyph_types(&loaded), vec![Bonding, Calcification]);
    }

    #[test]
    fn a_typed_number_goes_to_its_part_when_the_board_selects_another() {
        let ctx = context();
        let mut loaded = Loaded::for_test(vec![bonder(0, 0), bonder(4, 0)], vec![]);
        loaded.select(Some(PartId::Glyph(0)));
        type_into(&ctx, &mut loaded, WidgetType::DragValue, "2");
        tap_board(&ctx, &mut loaded, 4, 0);
        assert_eq!(positions(&loaded), vec![Pos::new(2, 0), Pos::new(4, 0)]);
        assert_eq!(loaded.selection, Some(PartId::Glyph(1)));
    }

    #[test]
    fn a_typed_number_goes_to_its_part_when_the_part_list_selects_another() {
        let ctx = context();
        let mut loaded = Loaded::for_test(vec![bonder(0, 0), bonder(4, 0)], vec![]);
        loaded.select(Some(PartId::Glyph(0)));
        loaded.all_glyphs = true;
        type_into(&ctx, &mut loaded, WidgetType::DragValue, "2");
        let other = widget(&ctx, |w| w.label.as_deref() == Some("Bonder (glyph 2)"));
        frame(&ctx, &mut loaded, click(other));
        assert_eq!(positions(&loaded), vec![Pos::new(2, 0), Pos::new(4, 0)]);
        assert_eq!(loaded.selection, Some(PartId::Glyph(1)));
    }

    #[test]
    fn a_typed_path_is_applied_when_the_board_deselects_it() {
        let ctx = context();
        let track = Glyph{pos: Pos::new(0, 0), rot: 0, glyph_type: GlyphType::Track(vec![Pos::new(0, 0), Pos::new(1, 0)])};
        let mut loaded = Loaded::for_test(vec![track], vec![]);
        loaded.select(Some(PartId::Glyph(0)));
        type_into(&ctx, &mut loaded, WidgetType::TextEdit, "d");
        tap_board(&ctx, &mut loaded, 5, 5);
        assert_eq!(track_path(loaded.timeline.base(), 0), vec![Pos::new(0, 0), Pos::new(1, 0), Pos::new(2, 0)]);
        assert_eq!(loaded.selection, None);
    }

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
}
