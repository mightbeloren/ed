use crate::input::{Key, KeyValue};

use super::{
    Command, Direction, FindMotion, InsertPlacement, Motion, Operator, OperatorCommand,
    RegisterName, Target,
};

#[derive(Clone, Copy, Debug)]
struct PendingOperator {
    operator: Operator,
    count: u32,
    repeat: char,
    register: RegisterName,
}

#[derive(Clone, Copy, Debug)]
struct PendingFind {
    direction: Direction,
    till: bool,
}

#[derive(Debug, Default)]
pub struct NormalParser {
    count: Option<u32>,
    motion_count: Option<u32>,
    pending_g: bool,
    pending_operator_g: bool,
    pending_register: bool,
    register: RegisterName,
    operator: Option<PendingOperator>,
    pending_find: Option<PendingFind>,
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
        self.motion_count = None;
        self.pending_g = false;
        self.pending_operator_g = false;
        self.pending_register = false;
        self.register = RegisterName::Unnamed;
        self.operator = None;
        self.pending_find = None;
    }

    pub fn is_operator_pending(&self) -> bool {
        self.operator.is_some()
    }

    fn parse_character(&mut self, character: char) -> Option<Command> {
        if self.pending_register {
            self.pending_register = false;
            self.register = match character {
                '_' => RegisterName::BlackHole,
                '"' => RegisterName::Unnamed,
                _ => {
                    self.reset();
                    return None;
                }
            };
            return None;
        }

        if let Some(find) = self.pending_find.take() {
            return Some(self.complete_motion(Motion::FindCharacter(FindMotion {
                character,
                direction: find.direction,
                till: find.till,
            })));
        }

        if self.operator.is_some() {
            return self.parse_operator_character(character);
        }

        if self.pending_g {
            self.pending_g = false;
            return match character {
                'g' => {
                    let line = self.take_count().unwrap_or(1).saturating_sub(1) as usize;
                    Some(self.motion(Motion::GoToLine { line: Some(line) }))
                }
                '~' => {
                    self.begin_operator(Operator::SwapCase, '~');
                    None
                }
                'u' => {
                    self.begin_operator(Operator::Lowercase, 'u');
                    None
                }
                'U' => {
                    self.begin_operator(Operator::Uppercase, 'U');
                    None
                }
                _ => {
                    self.reset();
                    None
                }
            };
        }

        if character == '"' {
            self.pending_register = true;
            return None;
        }

        if character.is_ascii_digit() {
            if character == '0' && self.count.is_none() {
                return Some(self.motion(Motion::LineStart));
            }
            self.count = Some(append_digit(self.count, character));
            return None;
        }

        match character {
            'h' => Some(self.motion(Motion::Left)),
            'j' => Some(self.motion(Motion::Down)),
            'k' => Some(self.motion(Motion::Up)),
            'l' => Some(self.motion(Motion::Right)),
            'w' => Some(self.motion(Motion::WordForward { big_word: false })),
            'W' => Some(self.motion(Motion::WordForward { big_word: true })),
            'b' => Some(self.motion(Motion::WordBackward { big_word: false })),
            'B' => Some(self.motion(Motion::WordBackward { big_word: true })),
            'e' => Some(self.motion(Motion::WordEnd { big_word: false })),
            'E' => Some(self.motion(Motion::WordEnd { big_word: true })),
            '^' => Some(self.motion(Motion::FirstNonBlank)),
            '$' => Some(self.motion(Motion::LineEnd)),
            '%' => Some(self.motion(Motion::MatchingDelimiter)),
            ';' => Some(self.motion(Motion::RepeatFind { reverse: false })),
            ',' => Some(self.motion(Motion::RepeatFind { reverse: true })),
            'f' => self.wait_for_find(Direction::Forward, false),
            'F' => self.wait_for_find(Direction::Backward, false),
            't' => self.wait_for_find(Direction::Forward, true),
            'T' => self.wait_for_find(Direction::Backward, true),
            'g' => {
                self.pending_g = true;
                None
            }
            'G' => {
                let line = self
                    .take_count()
                    .map(|count| count.saturating_sub(1) as usize);
                Some(self.motion(Motion::GoToLine { line }))
            }
            'd' => self.wait_for_operator(Operator::Delete, 'd'),
            'c' => self.wait_for_operator(Operator::Change, 'c'),
            'y' => self.wait_for_operator(Operator::Yank, 'y'),
            '>' => self.wait_for_operator(Operator::Indent, '>'),
            '<' => self.wait_for_operator(Operator::Outdent, '<'),
            '=' => self.wait_for_operator(Operator::Reindent, '='),
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

    fn parse_operator_character(&mut self, character: char) -> Option<Command> {
        if self.pending_operator_g {
            self.pending_operator_g = false;
            if character == 'g' {
                let line = self.motion_count.take().unwrap_or(1).saturating_sub(1) as usize;
                return Some(self.complete_motion(Motion::GoToLine { line: Some(line) }));
            }
            self.reset();
            return None;
        }

        if character.is_ascii_digit() {
            if character == '0' && self.motion_count.is_none() {
                return Some(self.complete_motion(Motion::LineStart));
            }
            self.motion_count = Some(append_digit(self.motion_count, character));
            return None;
        }

        let pending = self.operator.expect("operator checked above");
        if character == pending.repeat {
            let count = multiplied_count(pending.count, self.motion_count.take().unwrap_or(1));
            let command = Command::Operate(OperatorCommand {
                operator: pending.operator,
                target: Target::CurrentLine,
                count,
                register: pending.register,
            });
            self.reset();
            return Some(command);
        }

        let motion = match character {
            'h' => Motion::Left,
            'j' => Motion::Down,
            'k' => Motion::Up,
            'l' => Motion::Right,
            'w' => Motion::WordForward { big_word: false },
            'W' => Motion::WordForward { big_word: true },
            'b' => Motion::WordBackward { big_word: false },
            'B' => Motion::WordBackward { big_word: true },
            'e' => Motion::WordEnd { big_word: false },
            'E' => Motion::WordEnd { big_word: true },
            '^' => Motion::FirstNonBlank,
            '$' => Motion::LineEnd,
            '%' => Motion::MatchingDelimiter,
            ';' => Motion::RepeatFind { reverse: false },
            ',' => Motion::RepeatFind { reverse: true },
            'f' => return self.wait_for_find(Direction::Forward, false),
            'F' => return self.wait_for_find(Direction::Backward, false),
            't' => return self.wait_for_find(Direction::Forward, true),
            'T' => return self.wait_for_find(Direction::Backward, true),
            'g' => {
                self.pending_operator_g = true;
                return None;
            }
            'G' => {
                let line = self
                    .motion_count
                    .take()
                    .map(|count| count.saturating_sub(1) as usize);
                Motion::GoToLine { line }
            }
            _ => {
                self.reset();
                return None;
            }
        };
        Some(self.complete_motion(motion))
    }

    fn parse_named(&mut self, name: &str) -> Option<Command> {
        if name == "escape" {
            self.reset();
            return None;
        }
        let motion = match name {
            "left" => Motion::Left,
            "down" => Motion::Down,
            "up" => Motion::Up,
            "right" => Motion::Right,
            "home" => Motion::LineStart,
            "end" => Motion::LineEnd,
            "pageup" => Motion::PageUp,
            "pagedown" => Motion::PageDown,
            _ => {
                self.reset();
                return None;
            }
        };
        Some(self.complete_motion(motion))
    }

    fn wait_for_operator(&mut self, operator: Operator, repeat: char) -> Option<Command> {
        self.begin_operator(operator, repeat);
        None
    }

    fn begin_operator(&mut self, operator: Operator, repeat: char) {
        self.operator = Some(PendingOperator {
            operator,
            count: self.take_count().unwrap_or(1),
            repeat,
            register: self.register,
        });
    }

    fn wait_for_find(&mut self, direction: Direction, till: bool) -> Option<Command> {
        self.pending_find = Some(PendingFind { direction, till });
        None
    }

    fn complete_motion(&mut self, motion: Motion) -> Command {
        if let Some(pending) = self.operator {
            let count = multiplied_count(pending.count, self.motion_count.take().unwrap_or(1));
            let command = Command::Operate(OperatorCommand {
                operator: pending.operator,
                target: Target::Motion(motion),
                count,
                register: pending.register,
            });
            self.reset();
            command
        } else {
            self.motion(motion)
        }
    }

    fn motion(&mut self, motion: Motion) -> Command {
        let command = Command::Move {
            motion,
            count: self.take_count().unwrap_or(1),
        };
        self.register = RegisterName::Unnamed;
        command
    }

    fn insert(&mut self, placement: InsertPlacement) -> Command {
        self.reset();
        Command::EnterInsert(placement)
    }

    fn take_count(&mut self) -> Option<u32> {
        self.count.take()
    }
}

fn append_digit(count: Option<u32>, character: char) -> u32 {
    let digit = character.to_digit(10).expect("ASCII digit");
    count
        .unwrap_or(0)
        .saturating_mul(10)
        .saturating_add(digit)
        .max(1)
}

fn multiplied_count(left: u32, right: u32) -> u32 {
    left.saturating_mul(right).max(1)
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

    fn parse_keys(keys: &str) -> Option<Command> {
        let mut parser = NormalParser::default();
        keys.chars()
            .filter_map(|value| parser.parse(&character(value)))
            .last()
    }

    #[test]
    fn composes_counts_with_motions() {
        assert_eq!(
            parse_keys("12j"),
            Some(Command::Move {
                motion: Motion::Down,
                count: 12,
            })
        );
    }

    #[test]
    fn composes_operator_and_motion_counts() {
        assert_eq!(
            parse_keys("2d3w"),
            Some(Command::Operate(OperatorCommand {
                operator: Operator::Delete,
                target: Target::Motion(Motion::WordForward { big_word: false }),
                count: 6,
                register: RegisterName::Unnamed,
            }))
        );
    }

    #[test]
    fn doubled_operator_targets_current_lines() {
        assert_eq!(
            parse_keys("3dd"),
            Some(Command::Operate(OperatorCommand {
                operator: Operator::Delete,
                target: Target::CurrentLine,
                count: 3,
                register: RegisterName::Unnamed,
            }))
        );
    }

    #[test]
    fn parses_g_operators_and_black_hole_register() {
        assert_eq!(
            parse_keys("\"_gUw"),
            Some(Command::Operate(OperatorCommand {
                operator: Operator::Uppercase,
                target: Target::Motion(Motion::WordForward { big_word: false }),
                count: 1,
                register: RegisterName::BlackHole,
            }))
        );
    }

    #[test]
    fn parses_find_motion_after_operator() {
        assert_eq!(
            parse_keys("df)"),
            Some(Command::Operate(OperatorCommand {
                operator: Operator::Delete,
                target: Target::Motion(Motion::FindCharacter(FindMotion {
                    character: ')',
                    direction: Direction::Forward,
                    till: false,
                })),
                count: 1,
                register: RegisterName::Unnamed,
            }))
        );
    }
}
