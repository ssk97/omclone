//Undo and redo for layout edits: each step is the whole starting world from before an edit

//Starting worlds have no atoms made yet, so they're small enough to keep plenty
const MAX_UNDO: usize = 100;

pub(super) struct UndoHistory<T>{
    undo: Vec<T>,
    redo: Vec<T>,
}
impl<T> UndoHistory<T>{
    pub(super) fn new() -> Self{
        UndoHistory{undo: Vec::new(), redo: Vec::new()}
    }
    //Keeps the state from before an edit; anything undone before it can't be redone any more
    pub(super) fn record(&mut self, before: T){
        if self.undo.len() == MAX_UNDO{
            self.undo.remove(0);
        }
        self.undo.push(before);
        self.redo.clear();
    }
    //The state to go back to, given the current one
    pub(super) fn undo(&mut self, current: T) -> Option<T>{
        let previous = self.undo.pop()?;
        self.redo.push(current);
        Some(previous)
    }
    pub(super) fn redo(&mut self, current: T) -> Option<T>{
        let next = self.redo.pop()?;
        self.undo.push(current);
        Some(next)
    }
    pub(super) fn can_undo(&self) -> bool{
        !self.undo.is_empty()
    }
    pub(super) fn can_redo(&self) -> bool{
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
            history.record(state);
            state = next;
        }
        state = history.undo(state).unwrap();
        state = history.undo(state).unwrap();
        assert_eq!(state, 1);
        state = history.redo(state).unwrap();
        assert_eq!(state, 2);
        assert!(history.can_redo());
        //A new edit drops what was undone
        history.record(state);
        state = 10;
        assert!(!history.can_redo());
        assert_eq!(history.redo(state), None);
        assert_eq!(history.undo(state), Some(2));
    }

    #[test]
    fn oldest_steps_are_dropped() {
        let mut history = UndoHistory::new();
        for i in 0..MAX_UNDO+5 {
            history.record(i);
        }
        let mut state = usize::MAX;
        let mut count = 0;
        while let Some(previous) = history.undo(state) {
            state = previous;
            count += 1;
        }
        assert_eq!((count, state), (MAX_UNDO, 5));
    }
}
