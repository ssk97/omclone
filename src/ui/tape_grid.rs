//The arms' tapes as a grid, with a row per arm and a column per timestep. The caret's column is the
//timeline's target timestep, so picking a cell picks a time, and typing writes instructions there.
use super::*;
use core::ops::Range;
use egui::{pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind};

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
    fn collapse(&mut self){
        self.anchor_arm = self.arm;
        self.anchor_step = None;
    }
}

pub(super) enum TapeOp{
    //Lines of instructions, written at the start of the selected timesteps in place of them.
    //A single line goes into every selected arm, and more go into an arm each.
    Write(Vec<Vec<Instr>>),
    //Deletes the selected timesteps, or else the one before or at the caret
    EraseBack, EraseForward,
}
//The instruction at an absolute timestep, before the tape loops
fn cell(tape: &Tape, step: usize) -> Instr{
    step.checked_sub(tape.first).and_then(|i| tape.instructions.get(i)).copied().unwrap_or(Instr::Empty)
}
//Where the longest of these arms' tapes ends
pub(super) fn longest_tape(arms: &[Arm]) -> usize{
    arms.iter().map(|a| a.instruction_tape.end()).max().unwrap_or(0)
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
                //Blanks past the tape's end don't change what runs, but they'd make the loop longer
                let end = tape.end();
                let keep = if overwrite {end.saturating_sub(steps.start)} else if steps.start < end {width} else {0};
                while line.len() > keep && line.last() == Some(&Instr::Empty){
                    line.pop();
                }
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
//The first timestep at which two versions of a tape can act differently, if they differ, given the loop length
pub(super) fn first_difference(a: &Tape, b: &Tape, loop_len: usize) -> Option<usize>{
    if a.first == b.first && a.instructions == b.instructions{
        return None;
    }
    let (end_a, end_b) = (a.end(), b.end());
    let start = a.first.min(b.first);
    //Tapes that only differ in blanks at the end act the same, so that's where they differ
    let mut step = (start..end_a.max(end_b)).find(|&i| cell(a, i) != cell(b, i)).unwrap_or(end_a.min(end_b));
    //A tape that halts runs on past the loop, instead of starting over
    if a.halts() != b.halts(){
        step = step.min(start + loop_len);
    }
    Some(step)
}
//The selected block as text, a line per arm
fn block_text(world: &World, arms: Range<usize>, steps: Range<usize>) -> String{
    world.arms[arms].iter()
        .map(|a| steps.clone().map(|i| cell(&a.instruction_tape, i).to_char()).collect::<String>())
        .collect::<Vec<_>>().join("\n")
}
//Typed or pasted text as lines of instructions, dropping anything that isn't one
fn text_lines(text: &str) -> Vec<Vec<Instr>>{
    text.trim_end_matches(|c| c == '\r' || c == '\n').split('\n')
        .map(|line| line.chars().filter_map(|c| Instr::from_char(c.to_ascii_lowercase())).collect())
        .collect()
}

//Moves the caret, which moves the timeline there
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
//Single typed instructions and erases can join the undo step of the edit before.
fn edit(loaded: &mut Loaded, op: TapeOp, groupable: bool){
    if !EDITOR_ENABLED{
        return;
    }
    let sel = &loaded.tape_sel;
    let (steps, from, selected) = (sel.steps(), sel.caret, sel.arms());
    let groupable = groupable && steps.is_empty();
    let arms = match &op{
        TapeOp::Write(lines) if lines.len() > 1 => {
            selected.start..(selected.start + lines.len()).min(loaded.timeline.base().arms.len())
        }
        _ => selected.clone(),
    };
    let base = loaded.timeline.base();
    let mut tapes: Vec<Tape> = base.arms[arms.clone()].iter().map(|a| a.instruction_tape.clone()).collect();
    let Some(caret) = apply_op(&mut tapes, steps, &op, loaded.tape_mode) else {return};
    let earliest = arms.clone().zip(&tapes)
        .filter_map(|(i, tape)| first_difference(&base.arms[i].instruction_tape, tape, base.repeat_length))
        .min();
    match earliest{
        Some(earliest) => {
            loaded.record_tape_edit(arms.clone(), from, caret, groupable);
            for (i, tape) in arms.clone().zip(tapes){
                loaded.timeline.base_mut().arms[i].instruction_tape = tape;
            }
            loaded.finish_tape_edit(earliest);
        }
        //Nothing changed (like a blank typed past the end), but the caret still moves on
        None => if groupable && loaded.tape_group == Some((arms.clone(), from)){
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
    let arm_count = loaded.timeline.base().arms.len();
    match event{
        //egui-miniquad pastes as Text, but other backends send Paste
        Event::Text(text) | Event::Paste(text) =>
            edit(loaded, TapeOp::Write(text_lines(text)), matches!(event, Event::Text(_)) && text.chars().count() == 1),
        Event::Copy | Event::Cut => {
            let sel = &loaded.tape_sel;
            if sel.steps().is_empty(){
                return;
            }
            ui.ctx().copy_text(block_text(loaded.timeline.base(), sel.arms(), sel.steps()));
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
    gutter: f32,
    ruler: f32,
    ruler_below: f32,
}
#[derive(Clone, Copy)]
enum Hit{
    Cell(usize, usize), Ruler(usize), Gutter(usize)
}
impl Layout{
    fn x(&self, step: usize) -> f32{
        self.origin.x + self.gutter + step as f32 * self.cell.x
    }
    fn y(&self, arm: usize) -> f32{
        self.origin.y + self.ruler + arm as f32 * self.cell.y
    }
    fn cell_rect(&self, arm: usize, step: usize) -> Rect{
        Rect::from_min_size(pos2(self.x(step), self.y(arm)), self.cell)
    }
    //The cell at `pos`, clamped to the grid
    fn cell_at(&self, pos: Pos2, arms: usize, steps: usize) -> (usize, usize){
        let index = |offset: f32, size: f32, count: usize| ((offset / size).max(0.) as usize).min(count-1);
        (index(pos.y - self.origin.y - self.ruler, self.cell.y, arms), index(pos.x - self.origin.x - self.gutter, self.cell.x, steps))
    }
    fn hit(&self, pos: Pos2, arms: usize, steps: usize) -> Hit{
        let (arm, step) = self.cell_at(pos, arms, steps);
        if pos.y < self.view.min.y + self.ruler || pos.y >= self.view.max.y - self.ruler_below{
            Hit::Ruler(step)
        } else if pos.x < self.view.min.x + self.gutter{
            Hit::Gutter(arm)
        } else {
            Hit::Cell(arm, step)
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
            Hit::Gutter(_) => sel.collapse(),
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
        _ => (128, 128, 128),
    };
    Color32::from_rgba_unmultiplied(r, g, b, 90)
}

//Shows the grid, returning the height it needs to show every arm
pub(super) fn tape_grid(ui: &mut egui::Ui, loaded: &mut Loaded) -> f32{
    let arm_count = loaded.timeline.base().arms.len();
    if arm_count == 0{
        return ui.weak("No arms").rect.height();
    }
    //Something else, like the timeline slider or running, can move the caret
    let caret = loaded.cursor_step();
    let sel = &mut loaded.tape_sel;
    if caret != sel.caret{
        sel.caret = caret;
        sel.anchor_step = None;
    }
    sel.arm = sel.arm.min(arm_count-1);
    sel.anchor_arm = sel.anchor_arm.min(arm_count-1);

    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let (char_width, row_height, small_height) = ui.ctx().fonts_mut(|f|
        (f.glyph_width(&font, '0'), f.row_height(&font), f.row_height(&small)));
    let cell = vec2((char_width + 1.).round(), (row_height + 4.).round());
    let digits = arm_count.to_string().len().max(2);
    let gutter = digits as f32 * char_width + 10.;
    let ruler = cell.y;
    let ruler_below = ruler.max(small_height + 2. + ui.spacing().scroll.bar_width);
    let step_count = loaded.max_timestep.max(longest_tape(&loaded.timeline.base().arms)).max(caret) + 16;

    let size = vec2(gutter + step_count as f32 * cell.x, ruler + arm_count as f32 * cell.y + ruler_below);
    //A wheel notch scrolls one arm, or sideways one ruler tick, smoothly. Partial notches from trackpads and
    //free-spinning wheels scroll part of the way. egui scrolls `line_scroll_speed` per line, and wheels give a line per
    //notch on Windows (see mouse_wheel_event) and Linux, but miniquad multiplies macOS wheel mice by 10, and browsers
    //report around 100 pixels a notch.
    let lines_per_notch = if cfg!(target_os = "macos") {10.} else if cfg!(target_arch = "wasm32") {100.} else {1.};
    let line_speed = ui.ctx().options(|o| o.input_options.line_scroll_speed);
    let per_line = vec2(5. * cell.x, cell.y) / (line_speed * lines_per_notch);
    egui::ScrollArea::both().auto_shrink([false, true]).wheel_scroll_multiplier(per_line).show_viewport(ui, |ui, viewport| {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let response = ui.interact(rect, grid_id(), Sense::click_and_drag());
        //The viewport can be larger than the grid (it's sized before the scroll area shrinks to fit), and the
        //rulers and arm numbers have to stay on the grid
        let view = viewport.translate(rect.min.to_vec2()).intersect(rect);
        let layout = Layout{origin: rect.min, view, cell, gutter, ruler, ruler_below};

        let (pointer, pressed, down, shift, pressed_anywhere) = ui.input(|i|
            (i.pointer.interact_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers.shift, i.pointer.any_pressed()));
        if let Some(pos) = pointer{
            if pressed && response.is_pointer_button_down_on(){
                response.request_focus();
                press(loaded, layout.hit(pos, arm_count, step_count), shift);
            } else if down && response.is_pointer_button_down_on(){
                let (arm, step) = layout.cell_at(pos, arm_count, step_count);
                drag(loaded, arm, step);
            }
        }
        if !down{
            loaded.tape_sel.drag_from = None;
        }
        if response.has_focus(){
            //Like a text box, it keeps the arrow keys and gives up focus on a click elsewhere
            ui.memory_mut(|m| m.set_focus_lock_filter(grid_id(), egui::EventFilter{
                horizontal_arrows: true, vertical_arrows: true, escape: true, tab: false}));
            if pressed_anywhere && !response.is_pointer_button_down_on(){
                response.surrender_focus();
            } else {
                for event in ui.input(|i| i.events.clone()){
                    handle_event(ui, loaded, &event);
                }
            }
        }

        let sel = &mut loaded.tape_sel;
        if (sel.arm, sel.caret) != sel.shown{
            sel.shown = (sel.arm, sel.caret);
            //Clear of the arm numbers and the timestep rulers, which cover the grid's edges
            ui.scroll_to_rect(Rect::from_min_max(pos2(layout.x(sel.caret) - gutter, layout.y(sel.arm) - ruler),
                pos2(layout.x(sel.caret + 1), layout.y(sel.arm + 1) + ruler_below)), None);
        }
        paint(ui, loaded, &layout, step_count, &font, &small, response.has_focus());
    });
    size.y
}
fn paint(ui: &egui::Ui, loaded: &Loaded, layout: &Layout, step_count: usize, font: &egui::FontId, small: &egui::FontId,
    focused: bool)
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
        let tape = &base.arms[arm].instruction_tape;
        if !tape.halts() && !tape.instructions.is_empty(){
            painter.rect_filled(row(tape.first, tape.first + base.repeat_length), 0., Color32::from_rgba_unmultiplied(128, 128, 128, 45));
        }
    }
    painter.rect_filled(column(sel.caret, view.min.y), 0., visuals.selection.bg_fill.gamma_multiply(0.35));
    if let RunState::Crashed(step) = loaded.run_state{
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
        let tape = &base.arms[arm].instruction_tape;
        for step in steps.start.max(tape.first)..steps.end.min(tape.end()){
            let instr = tape.instructions[step - tape.first];
            if instr != Instr::Empty{
                let rect = layout.cell_rect(arm, step);
                //A gap on the right keeps neighboring instructions apart
                painter.rect_filled(Rect::from_min_max(rect.min + vec2(0., 1.), rect.max - vec2(1., 1.)), 1., tint(instr));
                painter.text(rect.center(), Align2::CENTER_CENTER, instr.to_char(), font.clone(), text_color);
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
    let digits = arm_count.to_string().len().max(2);
    let selected_arms = sel.arms();
    for arm in arms{
        let color = if selected_arms.contains(&arm) {text_color} else {visuals.weak_text_color()};
        painter.text(pos2(gutter.max.x - 5., layout.cell_rect(arm, 0).center().y), Align2::RIGHT_CENTER,
            format!("{:0digits$}", arm + 1), font.clone(), color);
    }
    painter.vline(gutter.max.x, gutter.y_range(), border);
    painter.rect_filled(Rect::from_min_size(view.min, vec2(layout.gutter, layout.ruler)), 0., fill);
    painter.rect_filled(Rect::from_min_max(pos2(view.min.x, view.max.y - layout.ruler_below), pos2(gutter.max.x, view.max.y)), 0., fill);
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
        let lines = text_lines("d \r\n  f\n");
        assert_eq!(lines, vec![vec![RotateClockwise, Empty], vec![Empty, Empty, Grab]]);
        let mut tapes = [tape(0, vec![Extend]), tape(0, vec![Extend])];
        assert_eq!(apply_op(&mut tapes, 0..0, &TapeOp::Write(lines), false), Some(3));
        assert_tape(&tapes[0], 0, &[RotateClockwise, Empty, Empty, Extend]);
        assert_tape(&tapes[1], 2, &[Grab, Extend]);

        let mut world_tapes = vec![tape(1, vec![RotateClockwise, Grab]), tape(0, vec![Extend])];
        let text = world_tapes.iter().map(|t| (0..4).map(|i| cell(t, i).to_char()).collect::<String>()).collect::<Vec<_>>().join("\n");
        assert_eq!(text, " df \nw   ");
        assert_eq!(apply_op(&mut world_tapes, 0..0, &TapeOp::Write(text_lines(&text)), true), Some(4));
        assert_tape(&world_tapes[0], 1, &[RotateClockwise, Grab]);
    }

    #[test]
    fn differences_start_at_the_first_changed_step() {
        let a = tape(2, vec![RotateClockwise, Grab, Drop]);
        assert_eq!(first_difference(&a, &a.clone(), 10), None);
        assert_eq!(first_difference(&a, &tape(2, vec![RotateClockwise, Extend, Drop]), 10), Some(3));
        assert_eq!(first_difference(&a, &tape(1, vec![Extend, RotateClockwise, Grab, Drop]), 10), Some(1));
        //Only blanks at the end differ
        assert_eq!(first_difference(&a, &tape(2, vec![RotateClockwise, Grab, Drop, Empty]), 10), Some(5));
        //Halting changes what happens once the loop would start over
        assert_eq!(first_difference(&a, &tape(2, vec![RotateClockwise, Grab, Drop, Empty, Empty, Halt]), 4), Some(6));
    }
}
