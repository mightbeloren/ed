use crate::input::{Key, KeyValue};

use super::{Command, InsertPlacement, Motion};

#[derive(Debug, Default)]
pub struct NormalParser {
    count: Option<u32>,
    pending_g: bool,
}

impl NormalParser {
    pub fn parse(&mut self, key: &Key) -> Option<Command> {
        if key.modifiers.control || key.modifiers.alt || key.modifiers.super_key {
            self.reset();
            return None;
        }

        match &key.value {
            KeyValue::Character(text) => {
                let mut characters = text.chars();
                let character = characters.next()?;
                if characters.next().is_some() {
                    self.reset();
                    return None;
                }
                self.parse_character(character)
            }
            KeyValue::Named(name) => self.parse_named(name),
        }
    }

    pub fn reset(&mut self) {
        self.count = None;
        self.pending_g = false;
    }

    fn parse_character(&mut self, character: char) -> Option<Command> {
        if self.pending_g {
            self.pending_g = false;
            if character == 'g' {
                let line = self.take_count().unwrap_or(1).saturating_sub(1) as usize;
                return Some(Command::GoToLine { line: Some(line) });
            }
            self.count = None;
            return None;
        }

        if character.is_ascii_digit() {
            if character == '0' && self.count.is_none() {
                return Some(self.motion(Motion::LineStart));
            }
            let digit = character.to_digit(10).expect("ASCII digit");
            let count = self
                .count
                .unwrap_or(0)
                .saturating_mul(10)
                .saturating_add(digit)
                .max(1);
            self.count = Some(count);
            return None;
        }

        match character {
            'h' => Some(self.motion(Motion::Left)),
            'j' => Some(self.motion(Motion::Down)),
            'k' => Some(self.motion(Motion::Up)),
            'l' => Some(self.motion(Motion::Right)),
            '^' => Some(self.motion(Motion::FirstNonBlank)),
            '$' => Some(self.motion(Motion::LineEnd)),
            'g' => {
                self.pending_g = true;
                None
            }
            'G' => {
                let line = self
                    .take_count()
                    .map(|count| count.saturating_sub(1) as usize);
                Some(Command::GoToLine { line })
            }
            'i' => Some(self.insert(InsertPlacement::BeforeCursor)),
            'a' => Some(self.insert(InsertPlacement::AfterCursor)),
            'I' => Some(self.insert(InsertPlacement::FirstNonBlank)),
            'A' => Some(self.insert(InsertPlacement::EndOfLine)),
            'o' => Some(self.insert(InsertPlacement::OpenBelow)),
            'O' => Some(self.insert(InsertPlacement::OpenAbove)),
            _ => {
                self.reset();
                None
            }
        }
    }

    fn parse_named(&mut self, name: &str) -> Option<Command> {
        let motion = match name {
            "left" => Motion::Left,
            "down" => Motion::Down,
            "up" => Motion::Up,
            "right" => Motion::Right,
            "home" => Motion::LineStart,
            "end" => Motion::LineEnd,
            "pageup" => Motion::PageUp,
            "pagedown" => Motion::PageDown,
            "escape" => {
                self.reset();
                return None;
            }
            _ => {
                self.reset();
                return None;
            }
        };
        Some(self.motion(motion))
    }

    fn motion(&mut self, motion: Motion) -> Command {
        Command::Move {
            motion,
            count: self.take_count().unwrap_or(1),
        }
    }

    fn insert(&mut self, placement: InsertPlacement) -> Command {
        self.reset();
        Command::EnterInsert(placement)
    }

    fn take_count(&mut self) -> Option<u32> {
        self.count.take()
    }
}

#[cfg(test)]
mod tests {
    use crate::input::{Key, KeyValue, Modifiers};

    use super::*;

    fn character(value: char) -> Key {
        Key {
            value: KeyValue::Character(value.to_string()),
            modifiers: Modifiers::default(),
        }
    }

    #[test]
    fn composes_counts_with_motions() {
        let mut parser = NormalParser::default();
        assert_eq!(parser.parse(&character('1')), None);
        assert_eq!(parser.parse(&character('2')), None);
        assert_eq!(
            parser.parse(&character('j')),
            Some(Command::Move {
                motion: Motion::Down,
                count: 12,
            })
        );
    }

    #[test]
    fn zero_is_a_motion_without_a_pending_count() {
        let mut parser = NormalParser::default();
        assert_eq!(
            parser.parse(&character('0')),
            Some(Command::Move {
                motion: Motion::LineStart,
                count: 1,
            })
        );
    }

    #[test]
    fn parses_absolute_line_jumps() {
        let mut parser = NormalParser::default();
        assert_eq!(parser.parse(&character('g')), None);
        assert_eq!(
            parser.parse(&character('g')),
            Some(Command::GoToLine { line: Some(0) })
        );

        assert_eq!(
            parser.parse(&character('G')),
            Some(Command::GoToLine { line: None })
        );
    }

    #[test]
    fn parses_insert_placements() {
        let mut parser = NormalParser::default();
        assert_eq!(
            parser.parse(&character('A')),
            Some(Command::EnterInsert(InsertPlacement::EndOfLine))
        );
    }
}
