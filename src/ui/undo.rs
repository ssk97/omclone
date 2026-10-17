//Undo and redo for edits: each step is the whole starting world from before an edit,
//along with the kind of edit that followed it, which undoing or redoing that step needs to know

use std::collections::VecDeque;

//Starting worlds have no atoms made yet, so they're small enough to keep plenty
const MAX_UNDO: usize = 100;

pub struct UndoHistory<T, K>{
    //The oldest step is dropped from the front once there are too many
    undo: VecDeque<(T, K)>,
    redo: Vec<(T, K)>,
}
impl<T, K: Copy> UndoHistory<T, K>{
    pub fn new() -> Self{
        UndoHistory{undo: VecDeque::new(), redo: Vec::new()}
    }
    //Keeps the state from before an edit; anything undone before it can't be redone any more
    pub fn record(&mut self, before: T, kind: K){
        if self.undo.len() == MAX_UNDO{
            self.undo.pop_front();
        }
        self.undo.push_back((before, kind));
        self.redo.clear();
    }
    //The state to go back to, given the current one, and the kind of edit that's undone
    pub fn undo(&mut self, current: T) -> Option<(T, K)>{
        let (previous, kind) = self.undo.pop_back()?;
        self.redo.push((current, kind));
        Some((previous, kind))
    }
    pub fn redo(&mut self, current: T) -> Option<(T, K)>{
        let (next, kind) = self.redo.pop()?;
        self.undo.push_back((current, kind));
        Some((next, kind))
    }
    pub fn can_undo(&self) -> bool{
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool{
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_and_redo_walk_the_edits() {
        let mut history = UndoHistory::new();
        let mut state = 0;
        for next in 1..=3 {
            history.record(state, ());
            state = next;
        }
        state = history.undo(state).unwrap().0;
        state = history.undo(state).unwrap().0;
        assert_eq!(state, 1);
        state = history.redo(state).unwrap().0;
        assert_eq!(state, 2);
        assert!(history.can_redo());
        //A new edit drops what was undone
        history.record(state, ());
        state = 10;
        assert!(!history.can_redo());
        assert_eq!(history.redo(state), None);
        assert_eq!(history.undo(state), Some((2, ())));
    }

    #[test]
    fn kinds_stay_with_their_edits() {
        let mut history = UndoHistory::new();
        history.record(0, 'a');
        history.record(1, 'b');
        let mut state = 2;
        let mut kinds = Vec::new();
        while let Some((previous, kind)) = history.undo(state) {
            state = previous;
            kinds.push(kind);
        }
        while let Some((next, kind)) = history.redo(state) {
            state = next;
            kinds.push(kind);
        }
        assert_eq!((state, kinds), (2, vec!['b', 'a', 'a', 'b']));
    }

    #[test]
    fn oldest_steps_are_dropped() {
        let mut history = UndoHistory::new();
        for i in 0..MAX_UNDO+5 {
            history.record(i, ());
        }
        let mut state = usize::MAX;
        let mut count = 0;
        while let Some((previous, _)) = history.undo(state) {
            state = previous;
            count += 1;
        }
        assert_eq!((count, state), (MAX_UNDO, 5));
    }
}
