//The arms' tapes as a grid, with a row per arm and a column per timestep. The caret's column is the
//timeline's target timestep, so picking a cell picks a time, and typing writes instructions there.
use super::*;
use core::ops::Range;
use egui::{pos2, vec2, Align2, Color32, Pos2, Rangef, Rect, Sense, Stroke, StrokeKind};

fn grid_id() -> egui::Id{
    egui::Id::new("tape_grid")
}
//Whether keys go to the tapes rather than to the part hotkeys
pub(super) fn has_focus(ctx: &egui::Context) -> bool{
    ctx.memory(|m| m.has_focus(grid_id()))
}

//The arms from `anchor_arm` to `arm` are selected, and the timesteps from `anchor_step` to the caret.
//With no anchor step, the caret sits before its timestep on every selected arm.
#[derive(Default)]
pub(super) struct TapeSelection{
    pub(super) arm: usize,
    pub(super) anchor_arm: usize,
    pub(super) anchor_step: Option<usize>,
    //The caret's timestep as of the last frame, to notice when something else moves it
    pub(super) caret: usize,
    //The cell a drag started on
    drag_from: Option<(usize, usize)>,
    //The arm whose number a drag started on, which dragging reorders, and the numbering_version then
    reorder_from: Option<(usize, u64)>,
    //The caret's arm and timestep when it was last scrolled into view
    shown: (usize, usize),
}
impl TapeSelection{
    fn arms(&self) -> Range<usize>{
        self.arm.min(self.anchor_arm)..self.arm.max(self.anchor_arm)+1
    }
    fn steps(&self) -> Range<usize>{
        let anchor = self.anchor_step.unwrap_or(self.caret);
        anchor.min(self.caret)..anchor.max(self.caret)
    }
    pub(super) fn collapse(&mut self){
        self.anchor_arm = self.arm;
        self.anchor_step = None;
    }
    //Keeps the selected arms among the first `arm_count`, which undo can make fewer at any time
    fn fit(&mut self, arm_count: usize){
        let last = arm_count.saturating_sub(1);
        self.arm = self.arm.min(last);
        self.anchor_arm = self.anchor_arm.min(last);
    }
}

pub(super) enum TapeOp{
    //Lines of instructions, written at the start of the selected timesteps in place of them.
    //A single line goes into every selected arm, and more go into an arm each.
    Write(Vec<Vec<Instr>>),
    //Deletes the selected timesteps, or else the one before or at the caret
    EraseBack, EraseForward,
}
//Where the longest of these arms' tapes ends, including what their Repeats and Resets run
pub(super) fn longest_tape(arms: &[Arm]) -> usize{
    arms.iter().map(|a| a.decoded().tape.end()).max().unwrap_or(0)
}
//Applies `op` to `tapes` (the arms it changes, in order) at the selected timesteps `steps`, writing over or
//blanking instructions instead of shifting them if `overwrite`. Returns where the caret goes, or None if there's
//nothing to do.
pub(super) fn apply_op(tapes: &mut [Tape], steps: Range<usize>, op: &TapeOp, overwrite: bool) -> Option<usize>{
    match op{
        TapeOp::Write(lines) => {
            let width = lines.iter().map(Vec::len).max().unwrap_or(0);
            if width == 0{
                return None;
            }
            for (i, tape) in tapes.iter_mut().enumerate(){
                let mut line = lines[if lines.len() == 1 {0} else {i}].clone();
                line.resize(width, Instr::Empty);
                tape.delete(steps.clone(), overwrite);
                //Blanks that end up past the tape's end are dropped by the tape
                tape.insert(steps.start, &line, overwrite);
            }
            Some(steps.start + width)
        }
        TapeOp::EraseBack | TapeOp::EraseForward => {
            let range = if !steps.is_empty(){
                steps
            } else if matches!(op, TapeOp::EraseBack){
                steps.start.checked_sub(1)?..steps.start
            } else {
                steps.start..steps.start+1
            };
            for tape in tapes{
                tape.delete(range.clone(), overwrite);
            }
            Some(range.start)
        }
    }
}
//The timesteps `steps` of these tapes as text, a line per tape
fn block_text<'a>(tapes: impl Iterator<Item = &'a Tape>, steps: Range<usize>) -> String{
    tapes.map(|t| steps.clone().map(|i| t.cell(i).to_char()).collect::<String>())
        .collect::<Vec<_>>().join("\n")
}
//Typed or pasted text as lines of instructions. Typing skips anything that isn't one, but in pasted text it's a
//blank, so the lines stay lined up.
fn text_lines(text: &str, pasted: bool) -> Vec<Vec<Instr>>{
    text.trim_end_matches(['\r', '\n']).split('\n')
        .map(|line| line.trim_end_matches('\r').chars()
            .filter_map(|c| Instr::from_char(c.to_ascii_lowercase()).or(pasted.then_some(Instr::Empty)))
            .collect())
        .collect()
}

//Where the caret goes after a Repeat or Reset was typed at `step` into `tapes` (those of `arms`): past the longest of
//what it runs in them. In insert mode, blanks go in after it, so that the instructions after it move past what it
//runs, by the same amount in every arm.
fn skip_what_it_runs(base: &World, arms: Range<usize>, tapes: &mut [Tape], step: usize, insert: bool) -> usize{
    let len = arms.zip(tapes.iter())
        .filter_map(|(i, tape)| base.arms[i].decode(tape, &base.track_maps).span_at(step)
            .filter(|span| span.start == step).map(|span| span.len()))
        .max().unwrap_or(1);
    if insert{
        for tape in tapes.iter_mut(){
            tape.insert(step + 1, &vec![Instr::Empty; len - 1], false);
        }
    }
    step + len
}
//Moves the caret, which moves the timeline there (unless it crashed before there)
fn move_caret(loaded: &mut Loaded, step: usize){
    loaded.tape_sel.caret = step;
    loaded.try_set_target_time(step);
}
//Moves the caret to `step`, selecting the timesteps it passes over if `extend`
fn select_step(loaded: &mut Loaded, step: usize, extend: bool){
    let sel = &mut loaded.tape_sel;
    if extend{
        sel.anchor_step.get_or_insert(sel.caret);
    } else {
        sel.anchor_step = None;
    }
    move_caret(loaded, step);
}
//Applies an edit to the selected tapes, keeping it for undo, then moves the caret past it.
//When `typing` (a typed instruction or an erase), more typing can join its undo step, and it joins the step
//before unless it replaces a selection.
fn edit(loaded: &mut Loaded, op: TapeOp, typing: bool){
    if !EDITOR_ENABLED{
        return;
    }
    let sel = &loaded.tape_sel;
    let (steps, from, selected) = (sel.steps(), sel.caret, sel.arms());
    let joins = typing && steps.is_empty();
    let arms = match &op{
        TapeOp::Write(lines) if lines.len() > 1 => {
            selected.start..(selected.start + lines.len()).min(loaded.timeline.base().arms.len())
        }
        _ => selected.clone(),
    };
    let base = loaded.timeline.base();
    let mut tapes: Vec<Tape> = base.arms[arms.clone()].iter().map(|a| a.tape().clone()).collect();
    let Some(mut caret) = apply_op(&mut tapes, steps.clone(), &op, loaded.tape_mode) else {return};
    let typed_repeat_or_reset = typing && matches!(&op, TapeOp::Write(lines)
        if lines.len() == 1 && matches!(lines[0][..], [Instr::Repeat | Instr::Reset]));
    if typed_repeat_or_reset{
        caret = skip_what_it_runs(base, arms.clone(), &mut tapes, steps.start, !loaded.tape_mode);
    }
    let earliest = arms.clone().zip(&tapes).filter_map(|(i, tape)| base.tape_change(i, tape)).min();
    match earliest{
        Some(earliest) => {
            loaded.record_tape_edit(arms.clone(), from, caret, joins, typing);
            for (i, tape) in arms.clone().zip(tapes){
                loaded.timeline.base_mut().set_tape(i, tape);
            }
            loaded.finish_tape_edit(earliest);
        }
        //Nothing changed (like a blank typed past the end), but the caret still moves on
        None => if joins && loaded.tape_group == Some((arms.clone(), from)){
            loaded.tape_group = Some((arms.clone(), caret));
        }
    }
    let sel = &mut loaded.tape_sel;
    if arms != selected{
        (sel.anchor_arm, sel.arm) = (arms.start, arms.end-1);
    }
    sel.anchor_step = None;
    move_caret(loaded, caret);
}
fn handle_event(ui: &egui::Ui, loaded: &mut Loaded, event: &egui::Event){
    use egui::{Event, Key};
    //An undo among the frame's earlier events may have taken arms away
    let arm_count = loaded.timeline.base().arms.len();
    if arm_count == 0{
        return;
    }
    loaded.tape_sel.fit(arm_count);
    match event{
        //Typing gives a character at a time. egui-miniquad pastes as Text too (other backends send Paste), so a
        //one-character paste counts as typing.
        Event::Text(text) | Event::Paste(text) => {
            let typed = text.chars().count() == 1;
            edit(loaded, TapeOp::Write(text_lines(text, !typed)), typed);
        }
        Event::Copy | Event::Cut => {
            let sel = &loaded.tape_sel;
            if sel.steps().is_empty(){
                return;
            }
            let tapes = loaded.timeline.base().arms[sel.arms()].iter().map(|a| a.tape());
            ui.ctx().copy_text(block_text(tapes, sel.steps()));
            if matches!(event, Event::Cut){
                edit(loaded, TapeOp::EraseForward, false);
            }
        }
        &Event::Key{key, pressed: true, modifiers, ..} => {
            let sel = &mut loaded.tape_sel;
            let (shift, steps) = (modifiers.shift, sel.steps());
            match key{
                Key::ArrowLeft => {
                    let step = if !shift && !steps.is_empty() {steps.start} else {sel.caret.saturating_sub(1)};
                    select_step(loaded, step, shift);
                }
                Key::ArrowRight => {
                    let step = if !shift && !steps.is_empty() {steps.end} else {sel.caret + 1};
                    select_step(loaded, step, shift);
                }
                Key::ArrowUp | Key::ArrowDown => {
                    sel.arm = if key == Key::ArrowUp {sel.arm.saturating_sub(1)} else {(sel.arm + 1).min(arm_count-1)};
                    if !shift{
                        sel.collapse();
                    }
                }
                Key::Home => select_step(loaded, 0, shift),
                Key::End => {
                    let end = longest_tape(&loaded.timeline.base().arms[loaded.tape_sel.arms()]);
                    select_step(loaded, end, shift);
                }
                Key::A if modifiers.command => {
                    let end = longest_tape(&loaded.timeline.base().arms);
                    (sel.anchor_arm, sel.arm, sel.anchor_step) = (0, arm_count-1, Some(0));
                    move_caret(loaded, end);
                }
                //Undo comes here, rather than to the part hotkeys, to happen after the keys typed before it
                Key::Z if modifiers.command => if modifiers.shift {loaded.redo()} else {loaded.undo()},
                Key::Y if modifiers.command => loaded.redo(),
                Key::Backspace => edit(loaded, TapeOp::EraseBack, true),
                Key::Delete => edit(loaded, TapeOp::EraseForward, true),
                Key::Insert => loaded.tape_mode = !loaded.tape_mode,
                Key::Escape => {
                    if sel.anchor_step.is_some() || sel.anchor_arm != sel.arm{
                        sel.collapse();
                    } else {
                        ui.memory_mut(|m| m.surrender_focus(grid_id()));
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

//Where the grid is drawn: `origin` is its top left corner and `view` the part that's visible, both on screen.
//The arm numbers are on the left, and timestep rulers are above and below (the one below leaves room for the scroll bar).
struct Layout{
    origin: Pos2,
    view: Rect,
    cell: egui::Vec2,
    //The digits in the arm numbers, which the gutter fits
    digits: usize,
    gutter: f32,
    ruler: f32,
    ruler_below: f32,
}
#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit{
    Cell(usize, usize), Ruler(usize), Gutter(usize)
}
impl Layout{
    //The sizes for `arm_count` arms in `ui`'s style. Where it's drawn (`origin` and `view`) is filled in once known.
    fn new(ui: &egui::Ui, arm_count: usize) -> Self{
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let small = egui::TextStyle::Small.resolve(ui.style());
        let (char_width, row_height, small_height) = ui.ctx().fonts_mut(|f|
            (f.glyph_width(&font, '0'), f.row_height(&font), f.row_height(&small)));
        let cell = vec2((char_width + 1.).round(), (row_height + 4.).round());
        let digits = arm_count.to_string().len().max(2);
        let ruler = cell.y;
        Layout{origin: Pos2::ZERO, view: Rect::NOTHING, cell, digits, gutter: digits as f32 * char_width + 10., ruler,
            ruler_below: ruler.max(small_height + 2. + ui.spacing().scroll.bar_width)}
    }
    fn x(&self, step: usize) -> f32{
        self.origin.x + self.gutter + step as f32 * self.cell.x
    }
    fn y(&self, arm: usize) -> f32{
        self.origin.y + self.ruler + arm as f32 * self.cell.y
    }
    fn cell_rect(&self, arm: usize, step: usize) -> Rect{
        Rect::from_min_size(pos2(self.x(step), self.y(arm)), self.cell)
    }
    //The visible part of the arms' rows, between the rulers (including the arm numbers)
    fn rows_view(&self) -> Rect{
        Rect::from_x_y_ranges(self.view.x_range(), self.view.min.y + self.ruler..=self.view.max.y - self.ruler_below)
    }
    //Where dropping the arm numbered `from` (as an index) at `pos` would put it: the arm it goes beside, whether after
    //it, and its new index. Only a drop on the rows counts.
    fn reorder_target(&self, pos: Pos2, from: usize, arms: usize, steps: usize) -> Option<(usize, bool, usize)>{
        if !self.rows_view().contains(pos){
            return None;
        }
        let (anchor, _) = self.cell_at(pos, arms, steps);
        //Lower rows have higher numbers, so the bottom half puts it after this arm
        let after = pos.y >= self.cell_rect(anchor, 0).center().y;
        Some((anchor, after, reorder_beside(from, anchor, after)?))
    }
    //The cell at `pos`, clamped to the grid
    fn cell_at(&self, pos: Pos2, arms: usize, steps: usize) -> (usize, usize){
        let index = |offset: f32, size: f32, count: usize| ((offset / size).max(0.) as usize).min(count-1);
        (index(pos.y - self.origin.y - self.ruler, self.cell.y, arms), index(pos.x - self.origin.x - self.gutter, self.cell.x, steps))
    }
    //What's at `pos`. The corners where the rulers meet the arm numbers have nothing.
    fn hit(&self, pos: Pos2, arms: usize, steps: usize) -> Option<Hit>{
        let (arm, step) = self.cell_at(pos, arms, steps);
        let on_ruler = pos.y < self.view.min.y + self.ruler || pos.y >= self.view.max.y - self.ruler_below;
        let on_gutter = pos.x < self.view.min.x + self.gutter;
        match (on_ruler, on_gutter){
            (true, true) => None,
            (true, false) => Some(Hit::Ruler(step)),
            (false, true) => Some(Hit::Gutter(arm)),
            (false, false) => Some(Hit::Cell(arm, step)),
        }
    }
    //Which of `count` rows or columns of `size`, the first starting at `start`, are within the view's span `view`
    fn visible(start: f32, size: f32, view: (f32, f32), count: usize) -> Range<usize>{
        let first = ((view.0 - start) / size).floor().max(0.) as usize;
        let last = ((view.1 - start) / size).ceil().max(0.) as usize;
        first.min(count)..last.min(count)
    }
}
fn press(loaded: &mut Loaded, hit: Hit, shift: bool){
    let can_reorder = loaded.edit_lock().is_none();
    let numbering = loaded.numbering_version;
    let sel = &mut loaded.tape_sel;
    let step = match hit{
        Hit::Cell(arm, step) => {
            sel.arm = arm;
            Some(step)
        }
        Hit::Ruler(step) => Some(step),
        Hit::Gutter(arm) => {
            sel.arm = arm;
            None
        }
    };
    if !shift{
        match hit{
            //Picking a time on the ruler keeps the selected arms
            Hit::Ruler(_) => sel.anchor_step = None,
            Hit::Cell(arm, step) => {
                sel.collapse();
                sel.drag_from = Some((arm, step));
            }
            Hit::Gutter(arm) => {
                sel.collapse();
                //Clicking only selects its row; dragging reorders it
                sel.reorder_from = can_reorder.then_some((arm, numbering));
            }
        }
    }
    let Some(step) = step else {return};
    if shift{
        //Selecting timesteps by clicking covers the clicked one
        let anchor = *sel.anchor_step.get_or_insert(sel.caret);
        move_caret(loaded, if step >= anchor {step + 1} else {step});
    } else {
        move_caret(loaded, step);
    }
}
//Dragging across timesteps selects the cells passed over, and dragging straight down or up puts the caret on each arm
fn drag(loaded: &mut Loaded, arm: usize, step: usize){
    let sel = &mut loaded.tape_sel;
    let Some((from_arm, from_step)) = sel.drag_from else {return};
    (sel.anchor_arm, sel.arm) = (from_arm, arm);
    let (anchor, caret) = match step.cmp(&from_step){
        std::cmp::Ordering::Equal => (None, from_step),
        std::cmp::Ordering::Greater => (Some(from_step), step + 1),
        std::cmp::Ordering::Less => (Some(from_step + 1), step),
    };
    sel.anchor_step = anchor;
    if caret != sel.caret{
        move_caret(loaded, caret);
    }
}
//The cell color for an instruction's kind
fn tint(instr: Instr) -> Color32{
    use Instr::*;
    let (r, g, b) = match instr{
        RotateClockwise | RotateCounterClockwise => (70, 130, 230),
        PivotClockwise | PivotCounterClockwise => (160, 90, 220),
        Extend | Retract => (60, 170, 90),
        Grab | Drop => (230, 140, 40),
        Forward | Back => (40, 170, 180),
        Repeat | Reset => (220, 90, 160),
        Clash => (220, 50, 50),
        _ => (128, 128, 128),
    };
    Color32::from_rgba_unmultiplied(r, g, b, 90)
}
//An instruction's cell, drawn over the grid's background
fn paint_instr(painter: &egui::Painter, rect: Rect, instr: Instr, font: &egui::FontId, text_color: Color32){
    //A gap on the right keeps neighboring instructions apart
    painter.rect_filled(Rect::from_min_max(rect.min + vec2(0., 1.), rect.max - vec2(1., 1.)), 1., tint(instr));
    painter.text(rect.center(), Align2::CENTER_CENTER, instr.to_char(), font.clone(), text_color);
}
//A cell that a Repeat or Reset covers: what's written there (if anything) in the top half, and what runs there in a
//box in the bottom half, empty if nothing does. An instruction written where a Repeat or Reset runs (other than in
//its own cell) clashes with it, so it's outlined, and its box shows the Clash.
fn paint_covered(painter: &egui::Painter, rect: Rect, written: Instr, runs: Instr,
    fonts: (&egui::FontId, &egui::FontId), colors: (Color32, Color32))
{
    let split = rect.min.y + (rect.height() / 2.).round();
    if written != Instr::Empty{
        paint_instr(painter, Rect::from_x_y_ranges(rect.x_range(), rect.min.y..=split), written, fonts.0, colors.0);
    }
    let fill = if runs == Instr::Empty {Color32::from_rgba_unmultiplied(128, 128, 128, 40)} else {tint(runs).gamma_multiply(0.6)};
    let run_box = Rect::from_min_max(pos2(rect.min.x, split), rect.max - vec2(1., 1.));
    painter.rect_filled(run_box, 1., fill);
    if runs != Instr::Empty{
        painter.text(run_box.center(), Align2::CENTER_CENTER, runs.to_char(), fonts.1.clone(), colors.1);
    }
    if runs == Instr::Clash{
        painter.rect_stroke(rect, 1., Stroke::new(1.5, Color32::from_rgb(220, 50, 50)), StrokeKind::Inside);
    }
}
//The instructions' letters, drawn as on the grid, and what they mean
pub(super) fn tape_letters(ui: &mut egui::Ui){
    use Instr::*;
    if EDITOR_ENABLED{
        ui.label("Type these at the caret. Space leaves a blank.");
    }
    let cell = Layout::new(ui, 1).cell;
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    egui::Grid::new("tape_letters").show(ui, |ui| {
        for (instrs, meaning) in [
            (&[Extend, Retract][..], "extend / retract"),
            (&[RotateCounterClockwise, RotateClockwise], "rotate counterclockwise / clockwise"),
            (&[PivotCounterClockwise, PivotClockwise], "pivot counterclockwise / clockwise"),
            (&[Grab, Drop], "grab / drop"),
            (&[Forward, Back], "forward / back along a track"),
            (&[Repeat, Reset], "repeat / reset (boxed letters show what they run, ! a clash that crashes)"),
            (&[Noop], "noop"),
            (&[Halt], "halt"),
        ]{
            let (rect, _) = ui.allocate_exact_size(vec2(2. * cell.x, cell.y), Sense::hover());
            let visuals = ui.visuals();
            let used = Rect::from_min_size(rect.min, vec2(instrs.len() as f32 * cell.x, cell.y));
            ui.painter().rect_filled(used, 0., visuals.extreme_bg_color);
            for (i, &instr) in instrs.iter().enumerate(){
                let cell_rect = Rect::from_min_size(rect.min + vec2(i as f32 * cell.x, 0.), cell);
                paint_instr(ui.painter(), cell_rect, instr, &font, visuals.strong_text_color());
            }
            ui.label(meaning);
            ui.end_row();
        }
    });
}

//Shows the grid, returning the height it needs to show every arm
pub(super) fn tape_grid(ui: &mut egui::Ui, loaded: &mut Loaded) -> f32{
    let arm_count = loaded.timeline.base().arms.len();
    if arm_count == 0{
        return ui.weak("No arms").rect.height();
    }
    //Something else, like the timeline slider or running, can move the caret. A crash only stops the timeline
    //short of the caret, which stays where it is, so typing past the crash carries on in order.
    let time = loaded.cursor_step();
    let crashed = matches!(loaded.run_state, RunState::Crashed{..});
    let sel = &mut loaded.tape_sel;
    if time != sel.caret && !(crashed && sel.caret > time){
        sel.caret = time;
        sel.anchor_step = None;
    }
    let caret = sel.caret;
    sel.fit(arm_count);

    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let mut layout = Layout::new(ui, arm_count);
    let Layout{cell, gutter, ruler, ruler_below, ..} = layout;
    let step_count = loaded.max_timestep.max(longest_tape(&loaded.timeline.base().arms)).max(caret) + 16;

    let size = vec2(gutter + step_count as f32 * cell.x, ruler + arm_count as f32 * cell.y + ruler_below);
    //A wheel notch, which is a line (see mouse_wheel_event), scrolls one arm, or sideways one ruler tick, smoothly.
    //Partial notches from trackpads and free-spinning wheels scroll part of the way. egui scrolls
    //`line_scroll_speed` per line, so this undoes that.
    let line_speed = ui.ctx().options(|o| o.input_options.line_scroll_speed);
    let per_line = vec2(5. * cell.x, cell.y) / line_speed;
    egui::ScrollArea::both().auto_shrink([false, true]).wheel_scroll_multiplier(per_line).show_viewport(ui, |ui, viewport| {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let response = ui.interact(rect, grid_id(), Sense::click_and_drag());
        //The viewport can be larger than the grid (it's sized before the scroll area shrinks to fit), and the
        //rulers and arm numbers have to stay on the grid
        let view = viewport.translate(rect.min.to_vec2()).intersect(rect);
        (layout.origin, layout.view) = (rect.min, view);

        let (press_pos, pointer, down, shift, pressed_anywhere) = ui.input(|i| (
            //Where the button went down this frame. By the end of a slow frame, the pointer may have moved on.
            i.events.iter().rev().find_map(|e| match *e{
                egui::Event::PointerButton{pos, button: egui::PointerButton::Primary, pressed: true, ..} => Some(pos),
                _ => None,
            }),
            i.pointer.interact_pos(), i.pointer.primary_down(), i.modifiers.shift, i.pointer.any_pressed()));
        //A press can be released in the same frame (a click during a slow frame, or a touchpad tap). egui reports that
        //as a click, since the button was never down on the grid at the end of a frame.
        let quick_click = press_pos.is_some() && !down && response.clicked();
        let pressed_on_grid = response.is_pointer_button_down_on() || quick_click;
        if let Some(pos) = press_pos.filter(|_| pressed_on_grid){
            response.request_focus();
            if let Some(hit) = layout.hit(pos, arm_count, step_count){
                press(loaded, hit, shift);
            }
        } else if let Some(pos) = pointer.filter(|_| down && response.is_pointer_button_down_on()){
            let (arm, step) = layout.cell_at(pos, arm_count, step_count);
            drag(loaded, arm, step);
        }
        //Where an arm number being dragged would go. A press that egui doesn't take as a drag only selected its row,
        //and a drag the arms were renumbered during (by an undo, say) would take the wrong arm, so it does nothing.
        let reorder_from = loaded.tape_sel.reorder_from.filter(|&(_, numbering)| numbering == loaded.numbering_version)
            .map(|(from, _)| from);
        let reordering = reorder_from.is_some() && (response.dragged() || response.drag_stopped());
        let reorder = reorder_from.filter(|_| reordering).zip(pointer)
            .and_then(|(from, pos)| layout.reorder_target(pos, from, arm_count, step_count));
        let over_numbers = matches!(response.hover_pos().and_then(|p| layout.hit(p, arm_count, step_count)),
            Some(Hit::Gutter(_)));
        if reordering || (over_numbers && loaded.edit_lock().is_none()){
            //miniquad has no grab hand, and this shows which way they go
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        if reordering{
            windows::drag_edge_scroll(ui, layout.rows_view());
        }
        let drop_beside = reorder.filter(|_| down).map(|(anchor, after, _)| (anchor, after));
        if !down{
            loaded.tape_sel.drag_from = None;
            loaded.tape_sel.reorder_from = None;
            if let (Some(from), Some((_, _, to))) = (reorder_from, reorder){
                if let Err(err) = loaded.apply_edit(&LayoutEdit::ReorderArm{from, to}){
                    loaded.message = Some(err);
                }
            }
        }
        if response.has_focus(){
            //Like a text box, it keeps the arrow keys and gives up focus on a press elsewhere
            ui.memory_mut(|m| m.set_focus_lock_filter(grid_id(), egui::EventFilter{
                horizontal_arrows: true, vertical_arrows: true, escape: true, tab: false}));
            if pressed_anywhere && !pressed_on_grid{
                response.surrender_focus();
            } else {
                for event in ui.input(|i| i.events.clone()){
                    handle_event(ui, loaded, &event);
                }
            }
        }

        //Only along the way the caret moved, so running doesn't pull the view back to the caret's arm
        let sel = &mut loaded.tape_sel;
        let (arm_moved, step_moved) = (sel.arm != sel.shown.0, sel.caret != sel.shown.1);
        if arm_moved || step_moved{
            sel.shown = (sel.arm, sel.caret);
            //Clear of the arm numbers and the timestep rulers, which cover the grid's edges
            let x = if step_moved {Rangef::new(layout.x(sel.caret) - gutter, layout.x(sel.caret + 1))} else {view.x_range()};
            let y = if arm_moved {Rangef::new(layout.y(sel.arm) - ruler, layout.y(sel.arm + 1) + ruler_below)} else {view.y_range()};
            ui.scroll_to_rect(Rect::from_x_y_ranges(x, y), None);
        }
        paint(ui, loaded, &layout, step_count, &font, &small, response.has_focus(), drop_beside);
    });
    size.y
}
//`drop_beside` is the arm a dragged arm number would go beside, and whether after it
fn paint(ui: &egui::Ui, loaded: &Loaded, layout: &Layout, step_count: usize, font: &egui::FontId, small: &egui::FontId,
    focused: bool, drop_beside: Option<(usize, bool)>)
{
    let painter = ui.painter();
    let visuals = ui.visuals();
    let base = loaded.timeline.base();
    let arm_count = base.arms.len();
    let sel = &loaded.tape_sel;
    let view = layout.view;
    let arms = Layout::visible(layout.y(0), layout.cell.y, (view.min.y, view.max.y), arm_count);
    let steps = Layout::visible(layout.x(0), layout.cell.x, (view.min.x, view.max.x), step_count);
    let column = |step: usize, top: f32| Rect::from_min_max(pos2(layout.x(step), top), pos2(layout.x(step + 1), layout.y(arm_count)));

    painter.rect_filled(Rect::from_min_max(pos2(layout.x(0), layout.y(0)), pos2(layout.x(step_count), layout.y(arm_count))),
        0., visuals.extreme_bg_color);
    for arm in arms.clone(){
        let row = |from: usize, to: usize| Rect::from_min_max(pos2(layout.x(from), layout.y(arm)), pos2(layout.x(to), layout.y(arm + 1)));
        if arm % 2 == 1{
            painter.rect_filled(row(0, step_count), 0., visuals.faint_bg_color);
        }
        //What a looping tape repeats: from its first instruction to where it starts over
        let tape = &base.arms[arm].decoded().tape;
        if !tape.runs_once() && !tape.instructions.is_empty(){
            painter.rect_filled(row(tape.first, tape.first + base.repeat_length), 0., Color32::from_rgba_unmultiplied(128, 128, 128, 45));
        }
    }
    //The arm row a dragged arm number would go beside is lit up, here and in the arm numbers
    let drop_row = drop_beside.map(|(anchor, _)| Rect::from_x_y_ranges(view.x_range(), layout.y(anchor)..=layout.y(anchor + 1)));
    let drop_fill = visuals.selection.bg_fill.gamma_multiply(0.4);
    if let Some(row) = drop_row{
        painter.rect_filled(row, 0., drop_fill);
    }
    painter.rect_filled(column(sel.caret, view.min.y), 0., visuals.selection.bg_fill.gamma_multiply(0.35));
    //Where the run crashed, or else the first clash, which it will crash at if nothing stops it sooner
    let crash = match loaded.run_state{
        RunState::Crashed{step, ..} => Some(step),
        _ => base.first_clash(),
    };
    if let Some(step) = crash{
        painter.rect_filled(column(step, layout.y(0)), 0., Color32::from_rgba_unmultiplied(220, 50, 50, 70));
    }
    let selected = sel.steps();
    if !selected.is_empty(){
        let arms = sel.arms();
        painter.rect_filled(Rect::from_min_max(pos2(layout.x(selected.start), layout.y(arms.start)),
            pos2(layout.x(selected.end), layout.y(arms.end))), 0., visuals.selection.bg_fill.gamma_multiply(0.6));
    }
    let text_color = visuals.strong_text_color();
    for arm in arms.clone(){
        let (tape, decoded) = (base.arms[arm].tape(), base.arms[arm].decoded());
        //The decoded tape starts with the written one, and runs on past it when a Repeat or Reset does
        for step in steps.start.max(tape.first)..steps.end.min(decoded.tape.end()){
            let rect = layout.cell_rect(arm, step);
            let written = tape.cell(step);
            match decoded.span_at(step){
                Some(_) => paint_covered(painter, rect, written, decoded.tape.cell(step), (font, small),
                    (text_color, visuals.text_color())),
                None => if written != Instr::Empty{
                    paint_instr(painter, rect, written, font, text_color);
                }
            }
        }
    }
    let caret_color = if focused {text_color} else {visuals.weak_text_color()};
    for arm in sel.arms(){
        if loaded.tape_mode{
            painter.rect_stroke(layout.cell_rect(arm, sel.caret), 1., Stroke::new(1.5, caret_color), StrokeKind::Inside);
        } else {
            let x = layout.x(sel.caret);
            painter.vline(x, layout.y(arm)..=layout.y(arm + 1), Stroke::new(2., caret_color));
        }
    }

    //The timestep rulers and arm numbers stay at the edges as the grid scrolls
    let fill = visuals.window_fill;
    let border = Stroke::new(1., visuals.widgets.noninteractive.bg_stroke.color);
    let paint_ruler = |rect: Rect, above: bool| {
        painter.rect_filled(rect, 0., fill);
        painter.rect_filled(Rect::from_x_y_ranges(layout.x(sel.caret)..=layout.x(sel.caret + 1), rect.y_range()),
            0., visuals.selection.bg_fill.gamma_multiply(0.6));
        //Ticks reach from the tapes. The numbers go above the ticks at the top, and beside them at the bottom,
        //leaving the rest of the bottom ruler to the scroll bar.
        let edge = if above {rect.max.y} else {rect.min.y};
        for step in steps.clone().filter(|s| s % 5 == 0){
            let x = layout.x(step);
            let tick = layout.ruler * if step % 10 == 0 {0.5} else {0.25};
            painter.vline(x, if above {edge - tick..=edge} else {edge..=edge + tick}, border);
            if step % 10 == 0{
                painter.text(pos2(x + 2., rect.min.y + 1.), Align2::LEFT_TOP, step, small.clone(), visuals.text_color());
            }
        }
        painter.hline(rect.x_range(), edge, border);
    };
    paint_ruler(Rect::from_min_size(view.min, vec2(view.width(), layout.ruler)), true);
    paint_ruler(Rect::from_min_max(pos2(view.min.x, view.max.y - layout.ruler_below), view.max), false);
    let gutter = Rect::from_min_size(view.min, vec2(layout.gutter, view.height()));
    painter.rect_filled(gutter, 0., fill);
    if let Some(row) = drop_row{
        painter.rect_filled(row.intersect(gutter), 0., drop_fill);
    }
    let selected_arms = sel.arms();
    for arm in arms{
        let color = if selected_arms.contains(&arm) {text_color} else {visuals.weak_text_color()};
        painter.text(pos2(gutter.max.x - 5., layout.cell_rect(arm, 0).center().y), Align2::RIGHT_CENTER,
            format!("{:0width$}", arm + 1, width = layout.digits), font.clone(), color);
    }
    painter.vline(gutter.max.x, gutter.y_range(), border);
    painter.rect_filled(Rect::from_min_size(view.min, vec2(layout.gutter, layout.ruler)), 0., fill);
    painter.rect_filled(Rect::from_min_max(pos2(view.min.x, view.max.y - layout.ruler_below), pos2(gutter.max.x, view.max.y)), 0., fill);
    //Where the dragged arm number would go: the edge of the lit-up row, unless that's under a ruler
    if let Some((anchor, after)) = drop_beside{
        let edge = layout.y(if after {anchor + 1} else {anchor});
        if layout.rows_view().y_range().contains(edge){
            painter.hline(view.x_range(), edge, Stroke::new(2., visuals.selection.stroke.color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Instr::*;

    fn tape(first: usize, instructions: Vec<Instr>) -> Tape {
        Tape::new(first, instructions)
    }
    fn assert_tape(t: &Tape, first: usize, instructions: &[Instr]) {
        assert_eq!((t.first, t.instructions.as_slice()), (first, instructions));
    }
    fn write(line: &[Instr]) -> TapeOp {
        TapeOp::Write(vec![line.to_vec()])
    }

    #[test]
    fn one_line_goes_into_every_arm() {
        let mut tapes = [tape(0, vec![RotateClockwise, Grab]), tape(1, vec![Extend])];
        assert_eq!(apply_op(&mut tapes, 1..1, &write(&[Drop]), false), Some(2));
        assert_tape(&tapes[0], 0, &[RotateClockwise, Drop, Grab]);
        assert_tape(&tapes[1], 1, &[Drop, Extend]);

        let mut tapes = [tape(0, vec![RotateClockwise, Grab]), tape(1, vec![Extend])];
        apply_op(&mut tapes, 1..1, &write(&[Drop]), true);
        assert_tape(&tapes[0], 0, &[RotateClockwise, Drop]);
        assert_tape(&tapes[1], 1, &[Drop]);
    }

    #[test]
    fn blanks_past_the_end_are_dropped() {
        //A wait inserted into every arm only shifts the arms with instructions after it
        let mut tapes = [tape(0, vec![RotateClockwise]), tape(0, vec![Grab, Drop, Extend]), tape(0, vec![])];
        assert_eq!(apply_op(&mut tapes, 2..2, &write(&[Empty]), false), Some(3));
        assert_tape(&tapes[0], 0, &[RotateClockwise]);
        assert_tape(&tapes[1], 0, &[Grab, Drop, Empty, Extend]);
        assert_tape(&tapes[2], 0, &[]);

        //Writing past the end pads up to the new instruction, but no further
        let mut tapes = [tape(0, vec![RotateClockwise])];
        assert_eq!(apply_op(&mut tapes, 3..3, &write(&[Grab, Empty]), false), Some(5));
        assert_tape(&tapes[0], 0, &[RotateClockwise, Empty, Empty, Grab]);
    }

    #[test]
    fn erasing_at_the_caret() {
        let mut tapes = [tape(0, vec![RotateClockwise, Grab])];
        assert_eq!(apply_op(&mut tapes, 0..0, &TapeOp::EraseBack, false), None);
        assert_eq!(apply_op(&mut tapes, 1..1, &TapeOp::EraseBack, false), Some(0));
        assert_tape(&tapes[0], 0, &[Grab]);

        let mut tapes = [tape(0, vec![RotateClockwise, Grab, Drop])];
        assert_eq!(apply_op(&mut tapes, 1..1, &TapeOp::EraseForward, true), Some(1));
        assert_tape(&tapes[0], 0, &[RotateClockwise, Empty, Drop]);
    }

    #[test]
    fn erasing_a_block_across_the_first_instruction() {
        //Deleting timesteps 1..4 of "  dfr" and "d fwg" leaves " r" and "dg"
        let mut tapes = [tape(2, vec![RotateClockwise, Grab, Drop]),
            tape(0, vec![RotateClockwise, Empty, Grab, Extend, Forward])];
        assert_eq!(apply_op(&mut tapes, 1..4, &TapeOp::EraseBack, false), Some(1));
        assert_tape(&tapes[0], 1, &[Drop]);
        assert_tape(&tapes[1], 0, &[RotateClockwise, Forward]);
    }

    #[test]
    fn pasted_lines_stay_aligned() {
        let lines = text_lines("d \r\n  f\n", true);
        assert_eq!(lines, vec![vec![RotateClockwise, Empty], vec![Empty, Empty, Grab]]);
        let mut tapes = [tape(0, vec![Extend]), tape(0, vec![Extend])];
        assert_eq!(apply_op(&mut tapes, 0..0, &TapeOp::Write(lines), false), Some(3));
        assert_tape(&tapes[0], 0, &[RotateClockwise, Empty, Empty, Extend]);
        assert_tape(&tapes[1], 2, &[Grab, Extend]);

        let mut world_tapes = vec![tape(1, vec![RotateClockwise, Grab]), tape(0, vec![Extend])];
        let text = block_text(world_tapes.iter(), 0..4);
        assert_eq!(text, " df \nw   ");
        assert_eq!(apply_op(&mut world_tapes, 0..0, &TapeOp::Write(text_lines(&text, true)), true), Some(4));
        assert_tape(&world_tapes[0], 1, &[RotateClockwise, Grab]);
    }

    #[test]
    fn pasted_unknown_characters_are_blanks() {
        //So the lines after them stay lined up
        assert_eq!(text_lines("d.f\nBz", true), vec![vec![RotateClockwise, Empty, Grab], vec![Halt, Empty]]);
        //Typing one isn't a blank
        assert_eq!(text_lines("z", false), vec![Vec::<Instr>::new()]);
        assert_eq!(text_lines("D", false), vec![vec![RotateClockwise]]);
    }

    #[test]
    fn ruler_and_gutter_corners_hit_nothing() {
        let layout = Layout{origin: pos2(0., 0.), view: Rect::from_min_max(pos2(0., 0.), pos2(200., 100.)),
            cell: vec2(10., 20.), digits: 2, gutter: 30., ruler: 20., ruler_below: 25.};
        let hit = |x, y| layout.hit(pos2(x, y), 10, 50);
        assert_eq!(hit(5., 5.), None);
        assert_eq!(hit(5., 90.), None);
        assert_eq!(hit(55., 5.), Some(Hit::Ruler(2)));
        assert_eq!(hit(55., 90.), Some(Hit::Ruler(2)));
        assert_eq!(hit(5., 50.), Some(Hit::Gutter(1)));
        assert_eq!(hit(55., 50.), Some(Hit::Cell(1, 2)));
    }
}

//These edit the tapes, which needs the editor
#[cfg(all(test, feature = "editor_ui"))]
mod editing_tests {
    use super::*;
    use egui::{Event, Key, Modifiers};

    fn key(key: Key, modifiers: Modifiers) -> Event {
        Event::Key{key, physical_key: None, pressed: true, repeat: false, modifiers}
    }
    //Handles keys that arrived together, as the grid does in one frame
    fn handle_frame(loaded: &mut Loaded, events: &[Event]) {
        let _ = egui::Context::default().run_ui(Default::default(), |ui| {
            for event in events {
                handle_event(ui, loaded, event);
            }
        });
    }
    fn add_arm_and_select_it(loaded: &mut Loaded, x: i32) {
        let edit = LayoutEdit::Add{part: NewPart::Arm(ArmType::PlainArm), pos: Pos::new(x, 0), rot: 0, len: 1};
        loaded.apply_edit(&edit).unwrap();
        let arm = loaded.timeline.base().arms.len() - 1;
        (loaded.tape_sel.arm, loaded.tape_sel.anchor_arm) = (arm, arm);
    }

    //The selection was only fitted to the arms once a frame, so typing after an undo that removed the selected
    //arm indexed past the arms
    #[test]
    fn typing_after_undoing_the_selected_arm() {
        let arm = Arm::new(Pos::new(0, 0), 0, 1, ArmType::PlainArm, Tape::new(0, vec![]));
        let mut loaded = Loaded::for_test(vec![], vec![arm]);
        add_arm_and_select_it(&mut loaded, 3);
        handle_frame(&mut loaded, &[key(Key::Z, Modifiers::COMMAND), Event::Text("f".into())]);
        let arms = &loaded.timeline.base().arms;
        assert_eq!(arms.len(), 1);
        assert_eq!(arms[0].tape().instructions, vec![Instr::Grab]);
    }

    fn arm_with(x: i32, instructions: Vec<Instr>) -> Arm {
        Arm::new(Pos::new(x, 0), 0, 1, ArmType::PlainArm, Tape::new(0, instructions))
    }
    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton{pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE}
    }
    //Draws the grid for one frame with these events, returning its layout as drawn
    fn grid_frame(ctx: &egui::Context, loaded: &mut Loaded, events: Vec<Event>) -> Layout {
        let mut layout = None;
        let _ = ctx.run_ui(egui::RawInput{events, ..Default::default()}, |ui| {
            tape_grid(ui, loaded);
            layout = Some(Layout::new(ui, loaded.timeline.base().arms.len()));
        });
        let mut layout = layout.unwrap();
        layout.origin = ctx.read_response(grid_id()).expect("the grid was drawn").rect.min;
        layout
    }
    //A press and its release in the same frame (as in a click during a slow frame, or a touchpad tap)
    fn quick_click(pos: Pos2) -> Vec<Event> {
        vec![Event::PointerMoved(pos), button(pos, true), button(pos, false)]
    }

    //A press released in the same frame didn't count, and when the grid had focus it gave it up, so the keys after
    //went to the part hotkeys (where Backspace deletes the selected part)
    #[test]
    fn quick_clicks_move_the_caret_and_keep_focus() {
        let ctx = egui::Context::default();
        let mut loaded = Loaded::for_test(vec![], vec![arm_with(0, vec![]), arm_with(3, vec![])]);
        let layout = grid_frame(&ctx, &mut loaded, vec![]);
        grid_frame(&ctx, &mut loaded, quick_click(layout.cell_rect(0, 2).center()));
        assert_eq!((loaded.tape_sel.arm, loaded.cursor_step()), (0, 2));
        assert!(has_focus(&ctx));
        grid_frame(&ctx, &mut loaded, quick_click(layout.cell_rect(1, 5).center()));
        assert_eq!((loaded.tape_sel.arm, loaded.cursor_step()), (1, 5));
        assert!(has_focus(&ctx), "clicking the grid again keeps its focus");
        grid_frame(&ctx, &mut loaded, vec![Event::Text("f".into())]);
        assert_eq!(loaded.timeline.base().arms[1].tape().cell(5), Instr::Grab);
    }

    //When a frame took long enough for the pointer to move on after the press, the press landed where the pointer
    //had got to
    #[test]
    fn drags_select_from_where_the_button_went_down() {
        let ctx = egui::Context::default();
        let mut loaded = Loaded::for_test(vec![], vec![arm_with(0, vec![])]);
        let layout = grid_frame(&ctx, &mut loaded, vec![]);
        let (from, to) = (layout.cell_rect(0, 3).center(), layout.cell_rect(0, 8).center());
        grid_frame(&ctx, &mut loaded, vec![Event::PointerMoved(from), button(from, true), Event::PointerMoved(to)]);
        grid_frame(&ctx, &mut loaded, vec![]);
        grid_frame(&ctx, &mut loaded, vec![button(to, false)]);
        assert_eq!(loaded.tape_sel.steps(), 3..9);
    }

    //Right away rather than on release, so keys go to the board while a part is dragged there
    #[test]
    fn pressing_elsewhere_gives_up_focus() {
        let ctx = egui::Context::default();
        let mut loaded = Loaded::for_test(vec![], vec![arm_with(0, vec![])]);
        let layout = grid_frame(&ctx, &mut loaded, vec![]);
        let cell = layout.cell_rect(0, 2).center();
        grid_frame(&ctx, &mut loaded, vec![Event::PointerMoved(cell), button(cell, true)]);
        grid_frame(&ctx, &mut loaded, vec![button(cell, false)]);
        assert!(has_focus(&ctx));
        let below = cell + vec2(0., 1000.);
        grid_frame(&ctx, &mut loaded, vec![Event::PointerMoved(below), button(below, true)]);
        assert!(!has_focus(&ctx));
    }

    //Arms whose tapes start at their original index, to tell them apart
    fn numbered_arms(count: i32) -> Vec<Arm> {
        (0..count).map(|i| Arm::new(Pos::new(i * 3, 0), 0, 1, ArmType::PlainArm, Tape::new(i as usize, vec![Instr::Grab]))).collect()
    }
    fn arm_order(loaded: &Loaded) -> Vec<usize> {
        loaded.timeline.base().arms.iter().map(|a| a.tape().first).collect()
    }
    //An arm's number, in the gutter
    fn number(layout: &Layout, arm: usize) -> Pos2 {
        pos2(layout.origin.x + layout.gutter / 2., layout.cell_rect(arm, 0).center().y)
    }
    //Presses at `from` and moves to `to` over a few frames, with `before_release` sent there before releasing
    fn drag_grid(ctx: &egui::Context, loaded: &mut Loaded, from: Pos2, to: Pos2, before_release: Vec<Event>) {
        grid_frame(ctx, loaded, vec![Event::PointerMoved(from), button(from, true)]);
        for i in 1..=4 {
            grid_frame(ctx, loaded, vec![Event::PointerMoved(from + (to - from) * (i as f32 / 4.))]);
        }
        grid_frame(ctx, loaded, before_release);
        grid_frame(ctx, loaded, vec![button(to, false)]);
    }

    #[test]
    fn dragging_an_arms_number_reorders_it() {
        let ctx = egui::Context::default();
        let mut loaded = Loaded::for_test(vec![], numbered_arms(4));
        let layout = grid_frame(&ctx, &mut loaded, vec![]);
        //The bottom half of arm 3's row puts it after arm 3
        let to = number(&layout, 2) + vec2(0., layout.cell.y / 4.);
        drag_grid(&ctx, &mut loaded, number(&layout, 0), to, vec![]);
        assert_eq!(arm_order(&loaded), vec![1, 2, 0, 3]);
        assert_eq!(loaded.tape_sel.arms(), 2..3, "the caret is on its new row");
    }

    #[test]
    fn arm_number_drags_that_dont_reorder() {
        let to = |layout: &Layout| number(layout, 2) + vec2(0., layout.cell.y / 4.);
        let escape = key(Key::Escape, Modifiers::NONE);
        let ctx = egui::Context::default();
        let mut loaded = Loaded::for_test(vec![], numbered_arms(4));
        let layout = grid_frame(&ctx, &mut loaded, vec![]);
        drag_grid(&ctx, &mut loaded, number(&layout, 0), to(&layout), vec![escape]);
        assert_eq!(arm_order(&loaded), vec![0, 1, 2, 3], "Esc drops it");

        loaded.run_state = RunState::FreeRun;
        drag_grid(&ctx, &mut loaded, number(&layout, 0), to(&layout), vec![]);
        assert_eq!((arm_order(&loaded), &loaded.message), (vec![0, 1, 2, 3], &None), "not while running");
        loaded.run_state = RunState::Manual(0);

        grid_frame(&ctx, &mut loaded, quick_click(number(&layout, 1)));
        assert_eq!(arm_order(&loaded), vec![0, 1, 2, 3]);
        assert_eq!(loaded.tape_sel.arms(), 1..2, "a click only selects the row");
    }

    #[test]
    fn an_arm_number_drag_that_undo_renumbers_the_arms_during_does_nothing() {
        let ctx = egui::Context::default();
        let mut loaded = Loaded::for_test(vec![], numbered_arms(3));
        add_arm_and_select_it(&mut loaded, 20);
        let layout = grid_frame(&ctx, &mut loaded, vec![]);
        //The undo takes away the arm being dragged
        drag_grid(&ctx, &mut loaded, number(&layout, 3), number(&layout, 0), vec![key(Key::Z, Modifiers::COMMAND)]);
        assert_eq!(arm_order(&loaded), vec![0, 1, 2]);
    }

    //Shown at timestep 2, with an arm that swings back and forth, so any edit at the caret changes what it does
    fn swinging_at_step_2() -> Loaded {
        use Instr::{RotateClockwise as Cw, RotateCounterClockwise as Ccw};
        let arm = Arm::new(Pos::new(0, 0), 0, 1, ArmType::PlainArm, Tape::new(0, vec![Cw, Ccw, Cw, Ccw]));
        let mut loaded = Loaded::for_test(vec![], vec![arm]);
        let right = key(Key::ArrowRight, Modifiers::NONE);
        handle_frame(&mut loaded, &[right.clone(), right]);
        loaded.update();
        assert_eq!(loaded.timeline.time(), 2.);
        loaded
    }

    //A tape edit moves the timeline back to the start to re-simulate, and when the caret then landed one step past
    //there, the timeline replayed that step instead of going straight to it
    #[test]
    fn erasing_goes_straight_to_the_caret() {
        let mut loaded = swinging_at_step_2();
        handle_frame(&mut loaded, &[key(Key::Backspace, Modifiers::NONE)]);
        loaded.update();
        assert_eq!(loaded.timeline.time(), 1.);
    }

    //Like moving right, it plays the timestep from where the timeline was shown
    #[test]
    fn typing_plays_the_typed_step() {
        let mut loaded = swinging_at_step_2();
        handle_frame(&mut loaded, &[Event::Text("a".into())]);
        loaded.update();
        assert_eq!(loaded.timeline.time(), 2. + loaded.run_speed);
    }

    fn typed(text: &str) -> Vec<Event> {
        text.chars().map(|c| Event::Text(c.to_string())).collect()
    }

    #[test]
    fn typing_a_reset_moves_on_past_what_it_runs() {
        use Instr::*;
        //Typed before the Drop, the Reset runs two turns back, so the Drop moves past both
        let mut loaded = Loaded::for_test(vec![], vec![arm_with(0, vec![Drop])]);
        handle_frame(&mut loaded, &typed("ddxf"));
        let arm = &loaded.timeline.base().arms[0];
        assert_eq!(arm.tape().instructions, [RotateClockwise, RotateClockwise, Reset, Empty, Grab, Drop]);
        assert_eq!(arm.first_clash(), None);
        //The typing is one undo step, which puts back what runs too
        handle_frame(&mut loaded, &[key(Key::Z, Modifiers::COMMAND)]);
        let arm = &loaded.timeline.base().arms[0];
        assert_eq!((&arm.tape().instructions[..], &arm.decoded().tape.instructions[..]), (&[Drop][..], &[Drop][..]));
    }

    #[test]
    fn a_reset_typed_in_overwrite_mode_runs_over_what_follows() {
        use Instr::*;
        //Nothing moves, so the Drop is left where the Reset runs, but the caret still goes past it
        let mut loaded = Loaded::for_test(vec![], vec![arm_with(0, vec![Empty, Empty, Empty, Drop])]);
        loaded.tape_mode = true;
        handle_frame(&mut loaded, &typed("ddx"));
        let arm = &loaded.timeline.base().arms[0];
        assert_eq!(arm.tape().instructions, [RotateClockwise, RotateClockwise, Reset, Drop]);
        assert_eq!(arm.first_clash(), Some((3, Reset)));
        assert_eq!(loaded.tape_sel.caret, 4);
    }

    #[test]
    fn a_reset_typed_into_several_arms_keeps_them_lined_up() {
        use Instr::*;
        //The first arm's Reset runs two turns and the second's one, so both make room for two
        let arms = vec![arm_with(0, vec![RotateClockwise, RotateClockwise, Drop]), arm_with(3, vec![RotateClockwise, Empty, Drop])];
        let mut loaded = Loaded::for_test(vec![], arms);
        let right = key(Key::ArrowRight, Modifiers::NONE);
        handle_frame(&mut loaded, &[right.clone(), right]);
        loaded.tape_sel.anchor_arm = 1;
        handle_frame(&mut loaded, &typed("x"));
        let tapes: Vec<Vec<Instr>> = loaded.timeline.base().arms.iter().map(|a| a.tape().instructions.clone()).collect();
        assert_eq!(tapes, [vec![RotateClockwise, RotateClockwise, Reset, Empty, Drop], vec![RotateClockwise, Empty, Reset, Empty, Drop]]);
        assert_eq!(loaded.tape_sel.caret, 4);
    }

    #[test]
    fn keys_after_undoing_the_only_arm() {
        let mut loaded = Loaded::for_test(vec![], vec![]);
        add_arm_and_select_it(&mut loaded, 0);
        handle_frame(&mut loaded, &[key(Key::Z, Modifiers::COMMAND), key(Key::ArrowDown, Modifiers::NONE),
            key(Key::End, Modifiers::NONE), Event::Text("f".into())]);
        assert!(loaded.timeline.base().arms.is_empty());
    }
}
