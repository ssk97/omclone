//Driving the egui windows in tests: a context set up like the app's, and finding and clicking what it drew
//Debug builds only: the tests find widgets by type and label, which egui records only with debug assertions on.
use egui::{Event, Pos2};

pub fn context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.options_mut(|o| o.input_options.surrender_focus_on = egui::SurrenderFocusOn::Presses);
    //Records each widget's type and label, to find them by
    ctx.global_style_mut(|s| s.debug.show_interactive_widgets = true);
    ctx
}
//Where the first widget drawn last frame that matches is
pub fn widget_rect(ctx: &egui::Context, matches: impl Fn(&egui::WidgetInfo) -> bool) -> Option<egui::Rect> {
    ctx.viewport(|v| {
        let widgets = &v.prev_pass.widgets;
        widgets.layers().flat_map(|(_, rects)| rects)
            .find(|w| widgets.info(w.id).is_some_and(&matches)).map(|w| w.rect)
    })
}
//The center of the first widget drawn last frame that matches
pub fn widget(ctx: &egui::Context, matches: impl Fn(&egui::WidgetInfo) -> bool) -> Pos2 {
    widget_rect(ctx, matches).expect("no such widget drawn").center()
}
pub fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton{pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE}
}
//A press and release in one frame
pub fn click(pos: Pos2) -> Vec<Event> {
    vec![Event::PointerMoved(pos), button(pos, true), button(pos, false)]
}
