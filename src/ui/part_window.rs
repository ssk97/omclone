//The Part window: undo, adding parts, the list of parts, and the selected part's fields
use super::*;
use windows::{drag_edge_scroll, locked_hover};

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
pub fn part_window(egui_ctx: &egui::Context, loaded: &mut Loaded){
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
            ui.toggle_value(&mut loaded.help_window_open, "Help").on_hover_text("Keys and mouse controls");
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
pub struct PathText{
    pub glyph: usize,
    pub shown: String,
    pub text: String,
    //The field has focus, so the board previews the text
    pub editing: bool,
    //Where typing goes in the text (in characters): the caret, or the start of the selection it replaces
    pub caret: usize,
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

//The Part window as the app's frames see it: fields typed in, clicks, and glyphs dragged in the list of parts.
//Debug builds only, as egui_test needs
#[cfg(all(test, feature = "editor_ui", debug_assertions))]
mod tests {
    use super::*;
    use egui::{Event, Pos2, RawInput, WidgetType};
    use egui_test::*;
    use loaded::TEST_SCREEN;

    //Draws the Part window for a frame with these events, then applies the board input that waited for it
    fn frame(ctx: &egui::Context, loaded: &mut Loaded, events: Vec<Event>) {
        let _ = ctx.run_ui(RawInput{events, ..Default::default()}, |ui| part_window(ui.ctx(), loaded));
        loaded.flush_board_input();
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
}
