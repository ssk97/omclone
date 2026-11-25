//The Part window: undo, adding parts, the list of parts, and the selected part's fields
use super::*;
use windows::{drag_edge_scroll, drop_line_y, locked_hover};

//A number that's typed in rather than dragged, applied once on Enter or when the field loses focus
//(each change is a layout edit). Only typed values are clamped to `range`, not the value shown.
fn typed_number<N: egui::emath::Numeric>(value: &mut N, range: std::ops::RangeInclusive<N>) -> egui::DragValue<'_>{
    egui::DragValue::new(value).range(range).clamp_existing_to_range(false).speed(0.).update_while_editing(false)
}
//The selected part's fields (where it starts, in `world`), returning an edit if one was changed. An arm also shows
//where it is in `now`, the current timestep's world.
//`locked` explains why the fields can't be edited, if they can't. `readout` is where the mouse is from the part.
fn part_fields(ui: &mut egui::Ui, world: &World, now: &World, part: PartId, locked: Option<&str>,
    readout: egui::RichText) -> Option<PartEdit>
{
    ui.label(part_label(world, part));
    //Shown even when it matches the fields, so the window doesn't change height as the timeline runs
    if let PartId::Arm(i) = part{
        let arm = &now.arms[i];
        ui.weak(format!("Now: ({}, {}), rot {}, length {}", arm.pos.x, arm.pos.y, arm.rot, arm.len))
            .on_hover_text("Where it is at this timestep. The fields are where it starts.");
    }
    ui.label(readout).on_hover_text("From where the part starts to the hex the mouse was last over on the board");
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
    let mut edit = PartEdit::new(vec![part]);
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
        //It turns in place, where it's moved to
        edit.pivot = pos + edit.delta;
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
    //It fits its contents, including as the divider changes the list's width. Its columns have fixed widths, so
    //resizing it by hand would only add empty space.
    egui::Window::new("Part").id(part_window_id()).auto_sized().show(egui_ctx, |ui| {
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
            //Watching changes nothing in the solution, so it works without the editor
            if matches!(loaded.tool, BoardTool::PickHex(_)){
                if ui.button("Cancel").on_hover_text("Stop picking, with no hex watched").clicked(){
                    loaded.end_tool();
                }
            } else if ui.button("Pause at hex")
                .on_hover_text("Click a hex, and moving forward pauses when an atom lands there").clicked(){
                loaded.start_picking_hex();
            }
            ui.separator();
            ui.add_enabled(EDITOR_ENABLED, egui::Checkbox::new(&mut loaded.allow_overlap, "Allow overlap"))
                .on_hover_text("Lets parts overlap each other, and break production puzzles' chamber rules");
        });
        ui.horizontal(|ui| {
            //First, so the readout changing length doesn't move it
            ui.toggle_value(&mut loaded.camera_window_open, "Camera")
                .on_hover_text("Camera controls, for moving and zooming without a mouse wheel or right button");
            ui.toggle_value(&mut loaded.help_window_open, "Help").on_hover_text("Keys and mouse controls");
            ui.label(readout_text(loaded, mouse_readout(loaded)));
        });
        let widths = ColumnWidths::new(ui);
        let list_width = widths.list_beside_fields(ui);
        //A separator fills the width it's given, which in an auto-sized window has no limit, so it's given the
        //columns' width
        let spacing = ui.spacing().item_spacing.x;
        let columns = list_width.map_or(widths.fields, |list| list + 3. * spacing + widths.fields);
        ui.allocate_ui(egui::vec2(columns, 0.), |ui| ui.separator());
        let mut action = None;
        //The list is drawn first, but its click or drop is applied last (see below)
        if let Some(list_width) = list_width{
            let row = ui.horizontal_top(|ui| {
                action = ui.vertical(|ui| {
                    ui.set_width(list_width);
                    list_column(ui, loaded)
                }).inner;
                let gap = ui.allocate_space(egui::vec2(ui.spacing().item_spacing.x, 0.)).1;
                ui.vertical(|ui| {
                    ui.set_width(widths.fields);
                    selection_section(ui, loaded);
                });
                gap
            });
            //Across the whole height, which is only known once both columns are drawn
            let divider = egui::Rect::from_x_y_ranges(row.inner.x_range(), row.response.rect.y_range());
            widths.divider(ui, divider, list_width);
        } else {
            //Stacked, at the fields' width, when the screen is too narrow for both side by side
            ui.vertical(|ui| {
                ui.set_width(widths.fields);
                action = list_column(ui, loaded);
                ui.separator();
                selection_section(ui, loaded);
            });
        }
        //After the fields are drawn, so that one being typed in (which the click ends) goes to its own part first.
        //That can renumber the parts (as can an undo or Delete during a drag), and then the part clicked or dragged
        //may have another number, so the click or drop does nothing.
        let action = action.filter(|_| loaded.list_press_numbering == loaded.numbering_version);
        match action {
            Some(ListAction::Select(part)) => loaded.select(Some(part)),
            Some(ListAction::Toggle(part)) => loaded.toggle_selected(part),
            Some(ListAction::Reorder(edit, select)) => match loaded.apply_edit(&edit) {
                Ok(()) => if select.is_some() {loaded.select(select)},
                Err(err) => loaded.message = Some(err),
            },
            None => {}
        }
    });
}
//Given rather than made from the title, so the window's area can be found by it
fn part_window_id() -> egui::Id{
    egui::Id::new("part_window")
}
//The list's title and rows, returning what was done to them
fn list_column(ui: &mut egui::Ui, loaded: &Loaded) -> Option<ListAction>{
    ui.label(list_title(loaded)).on_hover_text(format!("{LIST_HELP} Arms run in number order. {GLYPH_ORDER_HELP}"));
    part_list(ui, loaded)
}
//The selected part's fields, or what can be done to the selected parts
fn selection_section(ui: &mut egui::Ui, loaded: &mut Loaded){
    //Each part's fields keep their own egui state, so text typed for one part is never applied to another
    match (loaded.single_selection(), loaded.selection.len()) {
        (Some(part), _) => {ui.push_id(part, |ui| part_section(ui, loaded, part));}
        (None, 0) => {ui.weak("No part selected");}
        (None, _) => group_section(ui, loaded),
    }
}
//The widths of the Part window's two columns. They're fixed (except as the divider between them is dragged), so the
//window doesn't change width as the selection changes.
struct ColumnWidths{
    //The list's, at first and after a double-click on the divider, and the narrowest it can be dragged to
    list_default: f32,
    list_min: f32,
    fields: f32,
}
impl ColumnWidths{
    fn new(ui: &egui::Ui) -> Self{
        let spacing = ui.spacing();
        //A row's text, either side of it, and the scroll bar. Longer names are cut off (see part_list). By default,
        //that's from about 20 characters, which most rows fit, and the divider widens the list for the rest.
        let button = egui::TextStyle::Button;
        let list = |text| text_width(ui, text, &button) + 2. * spacing.button_padding.x + spacing.scroll.allocated_width();
        //The widest rows of fields: the track tools, and the mouse's offset when it's far from the part. A wrapped
        //offset would move the fields under it as the mouse moves.
        let tools = ["Add to front", "Add to back", "Remove hexes"];
        let tools = tools.iter().map(|t| text_width(ui, t, &button) + 2. * spacing.button_padding.x).sum::<f32>()
            + (tools.len() - 1) as f32 * spacing.item_spacing.x;
        let offset = text_width(ui, &offset_text(Pos::new(-99, -99), 999), &egui::TextStyle::Monospace);
        ColumnWidths{list_default: list("Triplex bonder (100)"), list_min: list("Bonder (9)"), fields: tools.max(offset)}
    }
    fn stored_id() -> egui::Id{
        egui::Id::new("part_list_width")
    }
    //The list's width, if the screen is wide enough for it beside the fields: as the divider was last dragged to, as
    //far as the screen allows
    fn list_beside_fields(&self, ui: &egui::Ui) -> Option<f32>{
        //Everything but the list: the fields, the gap with the divider and the spacing either side of it, and the
        //window's frame
        let rest = self.fields + 3. * ui.spacing().item_spacing.x + egui::Frame::window(ui.style()).total_margin().sum().x;
        let widest = ui.ctx().content_rect().width() - rest;
        let stored = ui.data(|d| d.get_temp(Self::stored_id())).unwrap_or(self.list_default);
        (widest >= self.list_min).then(|| stored.clamp(self.list_min, widest.min(3. * self.list_default)))
    }
    //The line between the columns, over `rect`. Dragging it moves the list's right edge, and double-clicking it puts
    //the list back to its default width.
    fn divider(&self, ui: &mut egui::Ui, rect: egui::Rect, list_width: f32){
        let response = ui.interact(rect, egui::Id::new("part_list_divider"), egui::Sense::click_and_drag());
        //The line follows the pointer
        let width = if response.double_clicked() {
            Some(self.list_default)
        } else {
            response.interact_pointer_pos().filter(|_| response.dragged())
                .map(|pointer| list_width + pointer.x - rect.center().x)
        };
        if let Some(width) = width{
            ui.data_mut(|d| d.insert_temp(Self::stored_id(), width));
        }
        let active = response.hovered() || response.dragged();
        if active{
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        let visuals = &ui.visuals().widgets;
        let stroke = if active {visuals.hovered.fg_stroke} else {visuals.noninteractive.bg_stroke};
        ui.painter().vline(rect.center().x, rect.y_range(), stroke);
        response.on_hover_text("Drag to widen or narrow the list. Double-click to reset it.");
    }
}
//How wide this text is in this style, without wrapping
fn text_width(ui: &egui::Ui, text: &str, style: &egui::TextStyle) -> f32{
    let font = style.resolve(ui.style());
    ui.ctx().fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font, egui::Color32::PLACEHOLDER).size().x)
}
const LIST_HELP: &str = "Clicking the board, or an arm number in the tapes, lists the parts on that hex in place of \
    what was listed. Arms come first, then glyphs, each in number order. Shift+click or Shift+drag on the board adds parts to the list. Ctrl+A selects \
    and lists every part, and Esc then clears the selection, keeping them listed. A second Esc empties the list. The \
    selected parts are always listed. Click a row to select only it, or Shift+click it to add or remove it. Drag a \
    row to reorder it, along with the other selected parts of its kind if it's selected; dropped on one of those, \
    they're gathered around it. A glyph's number, counting \
    all the glyphs, is in brackets after its name.";
const GLYPH_ORDER_HELP: &str = "Glyphs with higher numbers are drawn on top and act after lower ones. Where tracks \
    overlap, the lower-numbered track wins.";
fn list_title(loaded: &Loaded) -> String{
    match &loaded.list {
        PartList::Parts(parts) if parts.is_empty() => "Parts".to_owned(),
        PartList::Parts(parts) => format!("Parts ({})", parts.len()),
        PartList::All => format!("All parts ({})", loaded.timeline.base().all_parts().count()),
    }
}
//What was done in the Part window's list of parts
enum ListAction{
    Select(PartId),
    Toggle(PartId),
    //A reorder, and the part to select after it, if any
    Reorder(LayoutEdit, Option<PartId>),
}
//The part whose row is being dragged in the list of parts
struct DraggedRow(PartId);
//How many rows the list of parts shows before it scrolls
const PART_LIST_ROWS: f32 = 12.;
//The parts the Part window lists (see PartList): arms, then glyphs, each in number order. That's the order they act
//in, and the tape grid's order for the arms.
fn listed_parts(loaded: &Loaded) -> Vec<PartId>{
    let mut parts: Vec<PartId> = match &loaded.list {
        PartList::Parts(parts) => parts.iter().copied().collect(),
        PartList::All => loaded.timeline.base().all_parts().collect(),
    };
    //PartId and all_parts order glyphs before arms, each by number, and a stable sort keeps the numbers in order
    parts.sort_by_key(|part| part.kind() == PartKind::Glyph);
    parts
}
//The parts that picks on the board found. Clicking one selects it, and a part can be dragged above or below another
//of its kind to reorder them, unless the parts are renumbered in between (see part_window).
fn part_list(ui: &mut egui::Ui, loaded: &Loaded) -> Option<ListAction>{
    let base = loaded.timeline.base();
    let parts = listed_parts(loaded);
    if parts.is_empty() {
        ui.weak(match loaded.list {
            PartList::Parts(_) => "(click the board)",
            PartList::All => "(none)",
        });
        return None;
    }
    //show_rows needs all rows the same height. A selectable label is at least interact_size high, which a row's text
    //is shorter than.
    let row_height = ui.spacing().interact_size.y;
    let ctx = ui.ctx().clone();
    //A drag that the parts were renumbered during would move the wrong ones (see part_window), so it's abandoned
    let current = loaded.list_press_numbering == loaded.numbering_version;
    if !current {
        egui::DragAndDrop::clear_payload(&ctx);
    }
    let dragged = egui::DragAndDrop::payload::<DraggedRow>(&ctx).map(|d| d.0);
    //A selected row takes the other selected parts of its kind along
    let moved: Vec<usize> = match dragged {
        Some(row) if loaded.selection.contains(&row) =>
            loaded.selection.iter().filter(|p| p.kind() == row.kind()).map(|p| p.index()).collect(),
        Some(row) => vec![row.index()],
        None => Vec::new(),
    };
    //Scrolls to the selected part when it, or the list shown, changes
    let shown_id = egui::Id::new("part_list_shown");
    let shown = (loaded.single_selection(), loaded.list.clone());
    let scroll_to_selection = ctx.data(|d| d.get_temp(shown_id)) != Some(shown.clone());
    ctx.data_mut(|d| d.insert_temp(shown_id, shown));
    let highlighted = loaded.highlighted_parts();
    let can_reorder = loaded.edit_lock().is_none();
    let row_spacing = ui.spacing().item_spacing.y;
    let scroll = egui::ScrollArea::vertical().id_salt("part_list").max_height(PART_LIST_ROWS * (row_height + row_spacing));
    //Only the rows in view are laid out, as a solution can have thousands of parts. So a row can be dropped only on
    //another in view, which drag_edge_scroll brings in.
    scroll.show_rows(ui, row_height, parts.len(), |ui, in_view| {
        //The selected row may be out of view, so it's scrolled to by where it would be. This ui's top is the first row
        //in view's.
        let selected_row = loaded.single_selection().filter(|_| scroll_to_selection)
            .and_then(|selected| parts.iter().position(|&p| p == selected));
        if let Some(i) = selected_row {
            let top = ui.max_rect().top() + (i as f32 - in_view.start as f32) * (row_height + row_spacing);
            let rect = egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=top + row_height);
            ui.scroll_to_rect(rect, None);
        }
        //Rows fill the column's width, so they're easy to click and drop on
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
            let mut action = None;
            for &part in &parts[in_view] {
                //A dragged row's drop target is lit up under its label
                let backdrop = ui.painter().add(egui::Shape::Noop);
                //A name too long for the column is cut off rather than wrapped, which would make its row taller than
                //show_rows expects, and shown whole on hover
                let label = list_label(base, part);
                let cut = text_width(ui, &label, &egui::TextStyle::Button) + 2. * ui.spacing().button_padding.x
                    > ui.available_width();
                let mut row = ui.add(egui::Button::selectable(highlighted.contains(&part), label.as_str()).truncate());
                if cut {
                    row = row.on_hover_text(part_label(base, part));
                }
                if row.clicked() {
                    let toggle = ui.input(|i| i.modifiers.shift);
                    action = Some(if toggle {ListAction::Toggle(part)} else {ListAction::Select(part)});
                }
                if can_reorder {
                    row = row.interact(egui::Sense::drag());
                    if current {
                        row.dnd_set_drag_payload(DraggedRow(part));
                    }
                }
                let Some(from) = dragged.filter(|d| d.kind() == part.kind()) else {continue};
                //The gaps between rows count as the nearer row's, so the target doesn't flicker off between them
                let area = row.rect.expand2(egui::vec2(0., ui.spacing().item_spacing.y / 2.));
                let Some(pointer) = ui.ctx().pointer_latest_pos().filter(|_| ui.rect_contains_pointer(area)) else {continue};
                //Lower rows have higher numbers, so the bottom half puts them after this part, as in the tape grid
                let after = pointer.y >= row.rect.center().y;
                let Some(to) = reorder_block(&moved, part.index(), after) else {continue};
                let selection = ui.visuals().selection;
                ui.painter().set(backdrop, egui::Shape::rect_filled(row.rect, 2., selection.bg_fill.gamma_multiply(0.4)));
                //A drop on one of the moved rows gathers them around it, which has no side to show
                if !moved.contains(&part.index()) {
                    let edge = drop_line_y(row.rect.y_range(), after);
                    ui.painter().hline(row.rect.x_range(), edge, egui::Stroke::new(2., selection.stroke.color));
                }
                //Between rows that aren't numbered one after the other, the targets either side of the gap put the
                //parts in different places, and only the lit row tells them apart. This says where they go.
                let result = match moved[..] {
                    [from] => format!("{} {} to {}", kind_name(part.kind()), from + 1, to + 1),
                    _ => format!("{} to {}–{}", count_of(part.kind(), moved.len()), to + 1, to + moved.len()),
                };
                egui::Tooltip::always_open(ctx.clone(), ui.layer_id(), egui::Id::new("part_list_drop"),
                    egui::PopupAnchor::Pointer).show(|ui| ui.label(result));
                if ui.input(|i| i.pointer.any_released()) {
                    egui::DragAndDrop::clear_payload(&ctx);
                    let edit = LayoutEdit::Reorder{kind: part.kind(), moved: moved.clone(), to};
                    //A row that wasn't selected is, once it's moved
                    let select = (!loaded.selection.contains(&from)).then(|| part.kind().part(to));
                    action = Some(ListAction::Reorder(edit, select));
                }
            }
            if dragged.is_some() {
                drag_edge_scroll(ui, ui.clip_rect());
            }
            action
        }).inner
    }).inner
}
//Over a window, the readouts are where the mouse last was on the board, so they're dimmed
fn readout_text(loaded: &Loaded, text: String) -> egui::RichText{
    let text = egui::RichText::new(text).monospace();
    if loaded.pointer_over_ui {text.weak()} else {text}
}
//The hex under the mouse
fn mouse_readout(loaded: &Loaded) -> String{
    match loaded.hovered_hex{
        Some(hex) => format!("Mouse: ({}, {})", hex.x, hex.y),
        None => String::from("Mouse: –"),
    }
}
//How far the hex under the mouse is from `pos`, where the selected part starts
fn offset_readout(loaded: &Loaded, pos: Pos) -> String{
    match loaded.hovered_hex{
        Some(hex) => {
            offset_text(hex - pos, hex_distance(pos, hex))
        }
        None => String::from("From here: –"),
    }
}
//Short, as it's in the fields' column, which is sized to fit it (see ColumnWidths). The Mouse line above says what it's
//from.
fn offset_text(offset: Pos, distance: i32) -> String{
    format!("From here: ({:+}, {:+}), {distance} away", offset.x, offset.y)
}
fn part_section(ui: &mut egui::Ui, loaded: &mut Loaded, part: PartId){
    let mut edit = None;
    let locked = loaded.part_lock(part, EditKind::Move);
    let base = loaded.timeline.base();
    let start = match part {PartId::Glyph(i) => base.glyphs[i].pos, PartId::Arm(i) => base.arms[i].pos};
    let readout = readout_text(loaded, offset_readout(loaded, start));
    if let Some(part_edit) = part_fields(ui, base, loaded.timeline.curr(), part, locked, readout) {
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
            //Taking the part out leaves its new number as its index among the rest
            edit = Some(LayoutEdit::Reorder{kind: part.kind(), moved: vec![index], to: number-1});
        }
    });
    let lock = loaded.part_lock(part, EditKind::Delete);
    let delete = ui.add_enabled(lock.is_none(), egui::Button::new("Delete"));
    if locked_hover(delete.on_hover_text("Delete or Backspace"), lock).clicked(){
        edit = Some(LayoutEdit::Delete([part].into()));
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
fn kind_name(kind: PartKind) -> &'static str{
    match kind {PartKind::Glyph => "Glyph", PartKind::Arm => "Arm"}
}
//As in "1 glyph" or "3 arms"
fn count_of(kind: PartKind, count: usize) -> String{
    format!("{count} {}{}", kind_name(kind).to_lowercase(), if count == 1 {""} else {"s"})
}
//With several parts selected there are no fields, only Delete. The counts by kind say what dragging a row in the list
//moves.
fn group_section(ui: &mut egui::Ui, loaded: &mut Loaded){
    let counts: Vec<String> = [PartKind::Arm, PartKind::Glyph].into_iter()
        .map(|kind| (kind, loaded.selection.iter().filter(|p| p.kind() == kind).count()))
        .filter(|&(_, count)| count > 0).map(|(kind, count)| count_of(kind, count)).collect();
    ui.label(format!("{} selected", counts.join(", ")));
    let parts = loaded.editable_parts(loaded.selection.iter().copied(), EditKind::Delete);
    let delete = ui.add_enabled(parts.is_ok(), egui::Button::new("Delete"));
    let hover = "Delete or Backspace. Inputs, outputs and conduits stay.";
    if locked_hover(delete.on_hover_text(hover), parts.as_ref().err().copied()).clicked(){
        let edit = parts.map_err(str::to_owned).and_then(|parts| loaded.apply_edit(&LayoutEdit::Delete(parts.into_iter().collect())));
        if let Err(err) = edit {
            loaded.message = Some(err);
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
        loaded.list_stacks([Pos::new(0, 0)]);
        settle(ctx, &mut loaded);
        loaded
    }

    #[test]
    fn a_glyph_dropped_on_a_rows_top_half_goes_above_it() {
        use GlyphType::*;
        let ctx = context();
        let mut loaded = listing_origin(&ctx, vec![glyph(0, 0, Bonding), glyph(0, 0, Calcification),
            glyph(0, 0, Duplication)], vec![]);
        //Rows go up in number, so above glyph 3 is just before it
        let to = top_half(row(&ctx, "Duplication (3)"));
        drag(&ctx, &mut loaded, row(&ctx, "Bonder (1)").center(), to);
        assert_eq!(glyph_types(&loaded), vec![Calcification, Bonding, Duplication]);
        assert_eq!(loaded.single_selection(), Some(PartId::Glyph(1)));
        frame(&ctx, &mut loaded, vec![]);
        row(&ctx, "Bonder (2)");
    }

    //The y of each 2-pixel horizontal line drawn, which only a drop target draws
    fn drop_lines(output: &egui::FullOutput) -> Vec<f32> {
        fn walk(shape: &egui::Shape, ys: &mut Vec<f32>) {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, ys)),
                egui::Shape::LineSegment{points, stroke} if stroke.width == 2. && points[0].y == points[1].y =>
                    ys.push(points[0].y),
                _ => {}
            }
        }
        let mut ys = Vec::new();
        output.shapes.iter().for_each(|clipped| walk(&clipped.shape, &mut ys));
        ys
    }

    //Once the list went up in number, the line was still drawn on the side the row went before then
    #[test]
    fn the_drop_line_is_on_the_side_the_row_goes() {
        use GlyphType::*;
        for (half, edge) in [(top_half as fn(egui::Rect) -> Pos2, egui::Rect::top as fn(&egui::Rect) -> f32),
            (bottom_half, egui::Rect::bottom)]
        {
            let ctx = context();
            let mut loaded = listing_origin(&ctx, vec![glyph(0, 0, Bonding), glyph(0, 0, Calcification),
                glyph(0, 0, Duplication)], vec![]);
            let target = row(&ctx, "Duplication (3)");
            let to = half(target);
            press_and_move(&ctx, &mut loaded, row(&ctx, "Bonder (1)").center(), to);
            let output = ctx.run_ui(RawInput{events: vec![Event::PointerMoved(to)], ..Default::default()},
                |ui| part_window(ui.ctx(), &mut loaded));
            assert_eq!(drop_lines(&output), vec![edge(&target)]);
        }
    }

    #[test]
    fn the_two_drop_points_between_rows_go_either_side_of_glyphs_elsewhere() {
        use GlyphType::*;
        //Glyphs 2 and 3 aren't on the hex, so they aren't listed between glyphs 1 and 4
        let glyphs = vec![glyph(0, 0, Bonding), glyph(5, 5, Calcification), glyph(9, 9, Disposal),
            glyph(0, 0, Projection), glyph(0, 0, Duplication)];
        let dropped = |target: &str, half: fn(egui::Rect) -> Pos2| {
            let ctx = context();
            let mut loaded = listing_origin(&ctx, glyphs.clone(), vec![]);
            let to = half(row(&ctx, target));
            drag(&ctx, &mut loaded, row(&ctx, "Duplication (5)").center(), to);
            glyph_types(&loaded)
        };
        assert_eq!(dropped("Projection (4)", top_half),
            vec![Bonding, Calcification, Disposal, Duplication, Projection]);
        assert_eq!(dropped("Bonder (1)", bottom_half),
            vec![Bonding, Duplication, Calcification, Disposal, Projection]);
    }

    #[test]
    fn a_glyph_dropped_on_an_arm_stays_put() {
        use GlyphType::*;
        let ctx = context();
        let arm = Arm::new(Pos::new(0, 0), 0, 1, ArmType::PlainArm, Tape::new(0, vec![]));
        let mut loaded = listing_origin(&ctx, vec![glyph(0, 0, Bonding), glyph(0, 0, Calcification)], vec![arm]);
        let to = top_half(row(&ctx, "Arm 1 (Arm)"));
        drag(&ctx, &mut loaded, row(&ctx, "Bonder (1)").center(), to);
        assert_eq!(glyph_types(&loaded), vec![Bonding, Calcification]);
        assert!(!loaded.history.can_undo());
    }

    #[test]
    fn the_whole_list_reorders_glyphs_elsewhere() {
        use GlyphType::*;
        let ctx = context();
        let mut loaded = Loaded::for_test(vec![glyph(0, 0, Bonding), glyph(5, 5, Calcification)], vec![]);
        loaded.list = PartList::All;
        settle(&ctx, &mut loaded);
        let to = bottom_half(row(&ctx, "Calcification (2)"));
        drag(&ctx, &mut loaded, row(&ctx, "Bonder (1)").center(), to);
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
            loaded.list = PartList::All;
            loaded.select(Some(PartId::Glyph(0)));
            settle(&ctx, &mut loaded);
            type_at(&ctx, &mut loaded, number_field(&ctx, "Glyph #:"), "3");
            let other = row(&ctx, "Calcification (2)").center();
            if separate_frames {
                frame(&ctx, &mut loaded, vec![Event::PointerMoved(other), button(other, true)]);
                frame(&ctx, &mut loaded, vec![button(other, false)]);
            } else {
                frame(&ctx, &mut loaded, click(other));
            }
            assert_eq!(glyph_types(&loaded), vec![Calcification, Duplication, Bonding], "the typed number applies");
            assert_eq!(loaded.single_selection(), Some(PartId::Glyph(2)), "the bonder stays selected");
        }
    }

    //An undo during a drag renumbered the glyphs, and the drop moved the wrong one, or panicked when the dragged
    //glyph was gone
    #[test]
    fn a_drag_that_undo_renumbers_the_glyphs_during_does_nothing() {
        use GlyphType::*;
        let ctx = context();
        let mut loaded = Loaded::for_test(vec![glyph(0, 0, Bonding), glyph(3, 0, Calcification)], vec![]);
        loaded.list = PartList::All;
        let add = LayoutEdit::Add{part: NewPart::Glyph(Duplication), pos: Pos::new(6, 0), rot: 0, len: 1};
        assert_eq!(loaded.apply_edit(&add), Ok(()));
        settle(&ctx, &mut loaded);
        let from = row(&ctx, "Duplication (3)").center();
        press_and_move(&ctx, &mut loaded, from, from + egui::vec2(0., 10.));
        loaded.undo();
        frame(&ctx, &mut loaded, vec![]);
        //The rows moved up when the added glyph's row went away
        let to = bottom_half(row(&ctx, "Bonder (1)"));
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
        assert_eq!(loaded.single_selection(), Some(PartId::Glyph(1)));
    }

    #[test]
    fn a_typed_number_goes_to_its_part_when_the_part_list_selects_another() {
        let ctx = context();
        let mut loaded = Loaded::for_test(vec![bonder(0, 0), bonder(4, 0)], vec![]);
        loaded.select(Some(PartId::Glyph(0)));
        loaded.list = PartList::All;
        type_into(&ctx, &mut loaded, WidgetType::DragValue, "2");
        let other = widget(&ctx, |w| w.label.as_deref() == Some("Bonder (2)"));
        frame(&ctx, &mut loaded, click(other));
        assert_eq!(positions(&loaded), vec![Pos::new(2, 0), Pos::new(4, 0)]);
        assert_eq!(loaded.single_selection(), Some(PartId::Glyph(1)));
    }

    fn all_listed(ctx: &egui::Context, glyphs: Vec<Glyph>, arms: Vec<Arm>) -> Loaded {
        let mut loaded = Loaded::for_test(glyphs, arms);
        loaded.select_all();
        settle(ctx, &mut loaded);
        loaded
    }
    fn selected(parts: &[PartId]) -> BTreeSet<PartId> {
        parts.iter().copied().collect()
    }

    #[test]
    fn select_all_lists_every_part_and_clicks_dont_change_the_list() {
        use GlyphType::*;
        let ctx = context();
        let arm = Arm::new(Pos::new(9, 9), 0, 1, ArmType::PlainArm, Tape::new(0, vec![]));
        let mut loaded = all_listed(&ctx, vec![glyph(0, 0, Bonding), glyph(5, 5, Calcification)], vec![arm]);
        assert_eq!(loaded.selection.len(), 3);
        row(&ctx, "1 arm, 2 glyphs selected");
        frame(&ctx, &mut loaded, click(row(&ctx, "Bonder (1)").center()));
        assert_eq!(loaded.selection, selected(&[PartId::Glyph(0)]), "a click selects only that row's part");
        frame(&ctx, &mut loaded, vec![]);
        row(&ctx, "Arm 1 (Arm)");
        row(&ctx, "Calcification (2)");
        //Shift+click adds a row's part, then takes it out
        let arm_row = row(&ctx, "Arm 1 (Arm)").center();
        let shift_click = |ctx: &egui::Context, loaded: &mut Loaded| {
            let input = RawInput{events: click(arm_row), modifiers: egui::Modifiers::SHIFT, ..Default::default()};
            let _ = ctx.run_ui(input, |ui| part_window(ui.ctx(), loaded));
        };
        shift_click(&ctx, &mut loaded);
        assert_eq!(loaded.selection, selected(&[PartId::Glyph(0), PartId::Arm(0)]));
        shift_click(&ctx, &mut loaded);
        assert_eq!(loaded.selection, selected(&[PartId::Glyph(0)]));
        assert_eq!(loaded.list, PartList::All);
    }

    //Rows out of view aren't laid out, so the selected one is scrolled to by its place in the list
    #[test]
    fn a_selected_row_out_of_view_is_scrolled_to() {
        let ctx = context();
        let mut loaded = all_listed(&ctx, (0..40).map(|i| glyph(i * 3, 0, GlyphType::Bonding)).collect(), vec![]);
        //Whether the row was drawn last frame, all in view. Rows are selectable buttons; the selected part's own label
        //below has the same text.
        let in_view = |ctx: &egui::Context, label: &str| ctx.viewport(|v| {
            let widgets = &v.prev_pass.widgets;
            widgets.layers().flat_map(|(_, rects)| rects)
                .find(|w| widgets.info(w.id).is_some_and(|i|
                    i.typ == WidgetType::Button && i.label.as_deref() == Some(label)))
                .is_some_and(|w| w.interact_rect == w.rect)
        });
        assert!(in_view(&ctx, "Bonder (1)") && !in_view(&ctx, "Bonder (40)"), "the list starts at the top");
        //The bottom row, then the top one, then one in the middle
        for (glyph, label) in [(39, "Bonder (40)"), (0, "Bonder (1)"), (20, "Bonder (21)")] {
            loaded.select(Some(PartId::Glyph(glyph)));
            //Long enough for egui's scrolling animation
            for _ in 0..60 {
                frame(&ctx, &mut loaded, vec![]);
            }
            assert!(in_view(&ctx, label), "{label}");
        }
    }

    //A row of the list, as drawn last frame. The selected part's own label has the same text, but isn't a button.
    fn list_row(ctx: &egui::Context, label: &str) -> egui::Rect {
        widget_rect(ctx, |w| w.typ == WidgetType::Button && w.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("no row {label:?}"))
    }
    fn window_width(ctx: &egui::Context) -> f32 {
        ctx.memory(|m| m.area_rect(part_window_id())).expect("the Part window was drawn").width()
    }

    #[test]
    fn the_window_keeps_its_width_as_the_selection_changes() {
        let ctx = context();
        let track = glyph(0, 2, GlyphType::Track(vec![Pos::new(0, 0), Pos::new(1, 0)]));
        let arm = Arm::new(Pos::new(0, 0), 0, 1, ArmType::PlainArm, Tape::new(0, vec![]));
        let mut loaded = all_listed(&ctx, vec![track, bonder(5, 0)], vec![arm]);
        //An arm, a track (which has the most fields), nothing, and a group
        let mut widths = Vec::new();
        for parts in [vec![PartId::Arm(0)], vec![PartId::Glyph(0)], vec![], vec![PartId::Glyph(0), PartId::Glyph(1)]] {
            loaded.set_selection(parts.into_iter().collect());
            settle(&ctx, &mut loaded);
            widths.push(window_width(&ctx));
        }
        assert!(widths.iter().all(|&w| w == widths[0]), "{widths:?}");
    }

    #[test]
    fn on_a_narrow_screen_the_fields_go_under_the_list() {
        let ctx = context();
        let mut loaded = all_listed(&ctx, vec![bonder(0, 0)], vec![]);
        let delete = |ctx: &egui::Context| widget_rect(ctx, |w| w.label.as_deref() == Some("Delete")).unwrap();
        let (row, fields) = (list_row(&ctx, "Bonder (1)"), delete(&ctx));
        assert!(fields.left() > row.right(), "side by side: {row:?}, {fields:?}");
        let narrow = RawInput{screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(380., 800.))),
            ..Default::default()};
        for _ in 0..3 {
            let _ = ctx.run_ui(narrow.clone(), |ui| part_window(ui.ctx(), &mut loaded));
        }
        let (row, fields) = (list_row(&ctx, "Bonder (1)"), delete(&ctx));
        assert!(fields.top() > row.bottom(), "stacked: {row:?}, {fields:?}");
    }

    #[test]
    fn the_divider_sets_the_lists_width_and_a_double_click_resets_it() {
        let ctx = context();
        let wheel = Arm::new(Pos::new(0, 0), 0, 1, ArmType::VanBerlo, Tape::new(0, vec![]));
        let mut loaded = all_listed(&ctx, vec![], vec![wheel]);
        loaded.select(None);
        settle(&ctx, &mut loaded);
        //Longer than the default width, so it starts cut off
        let label = "Arm 1 (Van Berlo's wheel)";
        let (start, start_window) = (list_row(&ctx, label), window_width(&ctx));
        let font = egui::TextStyle::Button.resolve(&ctx.global_style());
        let full = ctx.fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font, egui::Color32::PLACEHOLDER).size().x);
        assert!(full > start.width(), "{label:?} is cut off at first");
        let divider = || ctx.viewport(|v| v.prev_pass.widgets.get(egui::Id::new("part_list_divider")).map(|w| w.rect))
            .expect("a divider beside the list").center();
        let at = divider();
        drag(&ctx, &mut loaded, at, at + egui::vec2(60., 0.));
        settle(&ctx, &mut loaded);
        let widened = list_row(&ctx, label);
        assert!((widened.width() - start.width() - 60.).abs() < 1., "{start:?} to {widened:?}");
        assert_eq!(start.height(), widened.height(), "a name cut off doesn't wrap onto a second line");
        let at = divider();
        frame(&ctx, &mut loaded, click(at));
        frame(&ctx, &mut loaded, click(at));
        settle(&ctx, &mut loaded);
        assert_eq!(list_row(&ctx, label).width(), start.width());
        //egui keeps a window as wide as anything in it filled, so this needs nothing in it to fill the width given
        assert_eq!(window_width(&ctx), start_window, "the window narrows back with the list");
    }

    //The mouse's offset from the part wrapped onto a second line once its numbers had two digits, which moved the
    //fields under it as the mouse moved
    #[test]
    fn a_far_mouse_offset_stays_on_one_line() {
        let ctx = context();
        let mut loaded = Loaded::for_test(vec![bonder(0, 0)], vec![]);
        loaded.select(Some(PartId::Glyph(0)));
        loaded.hovered_hex = Some(Pos::new(-99, -99));
        settle(&ctx, &mut loaded);
        let readout = widget_rect(&ctx, |w| w.label.as_deref().is_some_and(|l| l.starts_with("From here:")))
            .expect("the offset is shown");
        let font = egui::TextStyle::Monospace.resolve(&ctx.global_style());
        let line = ctx.fonts_mut(|f| f.row_height(&font));
        assert!(readout.height() < 1.5 * line, "{readout:?} is more than one line of {line}");
    }

    //Taking a part out of the selection leaves it listed
    #[test]
    fn a_deselected_part_can_be_selected_again_from_the_list() {
        let ctx = context();
        let arm = Arm::new(Pos::new(0, 0), 0, 1, ArmType::PlainArm, Tape::new(0, vec![]));
        let mut loaded = listing_origin(&ctx, vec![glyph(0, 0, GlyphType::Bonding)], vec![arm]);
        loaded.set_selection(selected(&[PartId::Glyph(0), PartId::Arm(0)]));
        frame(&ctx, &mut loaded, vec![]);
        let shift_click = |ctx: &egui::Context, loaded: &mut Loaded| {
            let input = RawInput{events: click(row(ctx, "Arm 1 (Arm)").center()), modifiers: egui::Modifiers::SHIFT,
                ..Default::default()};
            let _ = ctx.run_ui(input, |ui| part_window(ui.ctx(), loaded));
        };
        shift_click(&ctx, &mut loaded);
        assert_eq!(loaded.selection, selected(&[PartId::Glyph(0)]));
        frame(&ctx, &mut loaded, vec![]);
        shift_click(&ctx, &mut loaded);
        assert_eq!(loaded.selection, selected(&[PartId::Glyph(0), PartId::Arm(0)]));
    }

    #[test]
    fn dragging_a_selected_row_moves_the_selected_parts_of_its_kind() {
        use GlyphType::*;
        let ctx = context();
        let arm = Arm::new(Pos::new(9, 9), 0, 1, ArmType::PlainArm, Tape::new(0, vec![]));
        let mut loaded = all_listed(&ctx, vec![glyph(0, 0, Bonding), glyph(3, 0, Calcification), glyph(6, 0, Duplication),
            glyph(9, 0, Disposal)], vec![arm]);
        loaded.set_selection(selected(&[PartId::Glyph(0), PartId::Glyph(2), PartId::Arm(0)]));
        frame(&ctx, &mut loaded, vec![]);
        //Below glyph 4, which puts them after it, in their order
        let to = bottom_half(row(&ctx, "Disposal (4)"));
        drag(&ctx, &mut loaded, row(&ctx, "Bonder (1)").center(), to);
        assert_eq!(glyph_types(&loaded), vec![Calcification, Disposal, Bonding, Duplication]);
        assert_eq!(loaded.selection, selected(&[PartId::Glyph(2), PartId::Glyph(3), PartId::Arm(0)]),
            "they stay selected");
    }

    //With only the selected parts listed, there was no row to drop them on
    #[test]
    fn a_selected_row_dropped_on_another_gathers_them_around_it() {
        use GlyphType::*;
        let ctx = context();
        let mut loaded = Loaded::for_test(vec![glyph(0, 0, Bonding), glyph(3, 0, Calcification), glyph(6, 0, Disposal),
            glyph(9, 0, Duplication)], vec![]);
        loaded.set_selection(selected(&[PartId::Glyph(0), PartId::Glyph(2)]));
        settle(&ctx, &mut loaded);
        let to = top_half(row(&ctx, "Disposal (3)"));
        drag(&ctx, &mut loaded, row(&ctx, "Bonder (1)").center(), to);
        assert_eq!(glyph_types(&loaded), vec![Calcification, Bonding, Disposal, Duplication]);
        assert_eq!(loaded.selection, selected(&[PartId::Glyph(1), PartId::Glyph(2)]), "they stay selected");
    }

    #[test]
    fn arm_rows_reorder_the_arms() {
        let ctx = context();
        let arms = (0..3).map(|i| Arm::new(Pos::new(i * 3, 0), 0, 1, ArmType::PlainArm,
            Tape::new(i as usize, vec![Instr::Grab]))).collect();
        let mut loaded = all_listed(&ctx, vec![], arms);
        loaded.select(None);
        frame(&ctx, &mut loaded, vec![]);
        let to = bottom_half(row(&ctx, "Arm 3 (Arm)"));
        drag(&ctx, &mut loaded, row(&ctx, "Arm 1 (Arm)").center(), to);
        let order: Vec<(i32, usize)> = loaded.timeline.base().arms.iter().map(|a| (a.pos.x, a.tape().first)).collect();
        assert_eq!(order, vec![(3, 1), (6, 2), (0, 0)], "it takes its tape along");
        assert_eq!(loaded.selection, selected(&[PartId::Arm(2)]), "a row that wasn't selected is once it's moved");
    }

    #[test]
    fn several_selected_parts_have_no_fields_but_can_be_deleted() {
        let ctx = context();
        let salt = vec![Atom::new(Pos::new(0, 0), AtomType::Salt)];
        let input = glyph(0, 0, GlyphType::Input(smallvec::smallvec![salt], 0));
        let mut loaded = all_listed(&ctx, vec![input, bonder(3, 0), bonder(6, 0)], vec![]);
        assert!(widget_rect(&ctx, |w| w.typ == WidgetType::DragValue).is_none(), "no fields");
        frame(&ctx, &mut loaded, click(row(&ctx, "Delete").center()));
        assert_eq!(positions(&loaded), vec![Pos::new(0, 0)], "the input stays");
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
        assert_eq!(loaded.single_selection(), None);
    }
}
