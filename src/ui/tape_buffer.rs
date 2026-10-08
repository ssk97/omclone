//A tape as egui text: edits to the text become edits to the tape
use super::*;
use core::ops::Range;

pub(super) struct TapeBuffer<'a>{
    pub(super) tape_ref: &'a mut Tape,
    pub(super) earliest_edit: &'a mut Option<usize>,
    pub(super) tape_mode: bool,
    pub(super) held_str: String,
}
impl TapeBuffer<'_>{
    fn record_edit(&mut self, index: usize) {
        *self.earliest_edit = Some(self.earliest_edit.map_or(index, |e| e.min(index)));
    }
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
    fn insert_text(&mut self, text: &str, char_index: egui::text::CharIndex) -> usize {
        let char_index = char_index.0;
        let instr_mapped:Vec<Instr> = text.chars().filter_map(|x| Instr::from_char(x.to_ascii_lowercase())).collect();
        if instr_mapped.is_empty() {
            return 0;
        }
        self.record_edit(char_index);
        self.tape_ref.insert(char_index, &instr_mapped, self.tape_mode);
        self.held_str = self.tape_ref.to_string();
        instr_mapped.len()
    }
    fn delete_char_range(&mut self, char_range: Range<egui::text::CharIndex>) {
        let char_range = char_range.start.0..char_range.end.0;
        if char_range.is_empty() {
            return;
        }
        self.record_edit(char_range.start);
        self.tape_ref.delete(char_range, self.tape_mode);
        self.held_str = self.tape_ref.to_string();
    }

    fn clear(&mut self) {
        self.record_edit(0);
        self.tape_ref.clear();
        self.held_str = self.tape_ref.to_string();
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
