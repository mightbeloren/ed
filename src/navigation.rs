use crate::buffer::BufferPosition;

const HISTORY_LIMIT: usize = 100;

#[derive(Debug, Default)]
pub struct JumpList {
    entries: Vec<BufferPosition>,
    index: usize,
}

impl JumpList {
    pub fn record(&mut self, origin: BufferPosition, destination: BufferPosition) {
        if origin == destination {
            return;
        }
        if self.index < self.entries.len() {
            self.entries.truncate(self.index + 1);
        }
        if self.entries.last().copied() != Some(origin) {
            self.entries.push(origin);
        }
        self.entries.push(destination);
        self.trim();
        self.index = self.entries.len().saturating_sub(1);
    }

    pub fn older(&mut self, count: u32) -> Option<BufferPosition> {
        if self.entries.is_empty() || self.index == 0 {
            return None;
        }
        self.index = self.index.saturating_sub(count.max(1) as usize);
        self.entries.get(self.index).copied()
    }

    pub fn newer(&mut self, count: u32) -> Option<BufferPosition> {
        if self.entries.is_empty() || self.index + 1 >= self.entries.len() {
            return None;
        }
        self.index = self
            .index
            .saturating_add(count.max(1) as usize)
            .min(self.entries.len() - 1);
        self.entries.get(self.index).copied()
    }

    fn trim(&mut self) {
        if self.entries.len() > HISTORY_LIMIT {
            let excess = self.entries.len() - HISTORY_LIMIT;
            self.entries.drain(..excess);
            self.index = self.index.saturating_sub(excess);
        }
    }
}

#[derive(Debug, Default)]
pub struct ChangeList {
    entries: Vec<BufferPosition>,
    index: usize,
}

impl ChangeList {
    pub fn record(&mut self, position: BufferPosition) {
        if self.index < self.entries.len() {
            self.entries.truncate(self.index);
        }
        if self.entries.last().copied() != Some(position) {
            self.entries.push(position);
        }
        if self.entries.len() > HISTORY_LIMIT {
            let excess = self.entries.len() - HISTORY_LIMIT;
            self.entries.drain(..excess);
        }
        self.index = self.entries.len();
    }

    pub fn older(&mut self, count: u32) -> Option<BufferPosition> {
        if self.entries.is_empty() || self.index == 0 {
            return None;
        }
        self.index = self.index.saturating_sub(count.max(1) as usize);
        self.entries.get(self.index).copied()
    }

    pub fn newer(&mut self, count: u32) -> Option<BufferPosition> {
        if self.entries.is_empty() || self.index + 1 >= self.entries.len() {
            return None;
        }
        self.index = self
            .index
            .saturating_add(count.max(1) as usize)
            .min(self.entries.len() - 1);
        self.entries.get(self.index).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(line: usize) -> BufferPosition {
        BufferPosition { line, character: 0 }
    }

    #[test]
    fn jump_list_traverses_backward_and_forward() {
        let mut jumps = JumpList::default();
        jumps.record(position(0), position(4));
        jumps.record(position(4), position(8));

        assert_eq!(jumps.older(2), Some(position(0)));
        assert_eq!(jumps.newer(1), Some(position(4)));
        assert_eq!(jumps.newer(1), Some(position(8)));
    }

    #[test]
    fn change_list_starts_with_the_latest_change() {
        let mut changes = ChangeList::default();
        changes.record(position(1));
        changes.record(position(3));

        assert_eq!(changes.older(1), Some(position(3)));
        assert_eq!(changes.older(1), Some(position(1)));
        assert_eq!(changes.newer(1), Some(position(3)));
    }
}
