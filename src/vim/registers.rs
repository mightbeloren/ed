use std::collections::HashMap;

use super::{RegisterName, SelectionShape};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisterValue {
    pub text: String,
    pub shape: SelectionShape,
}

#[derive(Debug, Default)]
pub struct RegisterStore {
    unnamed: Option<RegisterValue>,
    named: HashMap<char, RegisterValue>,
    numbered: Vec<Option<RegisterValue>>,
    yank: Option<RegisterValue>,
    small_delete: Option<RegisterValue>,
    clipboard: Option<RegisterValue>,
    clipboard_dirty: bool,
}

impl RegisterStore {
    pub fn write(&mut self, register: RegisterName, value: RegisterValue) {
        if register == RegisterName::BlackHole {
            return;
        }
        self.unnamed = Some(value.clone());
        self.write_selected(register, value);
    }

    pub fn record_yank(&mut self, register: RegisterName, value: RegisterValue) {
        self.yank = Some(value.clone());
        self.write(register, value);
    }

    pub fn record_delete(&mut self, register: RegisterName, value: RegisterValue) {
        if register == RegisterName::BlackHole {
            return;
        }
        if value.shape == SelectionShape::LineWise || value.text.contains('\n') {
            if self.numbered.len() < 10 {
                self.numbered.resize(10, None);
            }
            for index in (2..=9).rev() {
                self.numbered[index] = self.numbered[index - 1].clone();
            }
            self.numbered[1] = Some(value.clone());
        } else {
            self.small_delete = Some(value.clone());
        }
        self.write(register, value);
    }

    pub fn read(&self, register: RegisterName) -> Option<&RegisterValue> {
        match register {
            RegisterName::Unnamed => self.unnamed.as_ref(),
            RegisterName::BlackHole => None,
            RegisterName::Named { name, .. } => self.named.get(&name),
            RegisterName::Numbered(number) => self.numbered.get(number as usize)?.as_ref(),
            RegisterName::Yank => self.yank.as_ref(),
            RegisterName::SmallDelete => self.small_delete.as_ref(),
            RegisterName::Clipboard | RegisterName::Primary => self.clipboard.as_ref(),
        }
    }

    pub fn unnamed(&self) -> Option<&RegisterValue> {
        self.unnamed.as_ref()
    }

    pub fn set_clipboard_text(&mut self, text: String) {
        self.clipboard = Some(RegisterValue {
            text,
            shape: SelectionShape::CharacterWise,
        });
        self.clipboard_dirty = false;
    }

    pub fn take_clipboard_write(&mut self) -> Option<String> {
        if !self.clipboard_dirty {
            return None;
        }
        self.clipboard_dirty = false;
        self.clipboard.as_ref().map(|value| value.text.clone())
    }

    fn write_selected(&mut self, register: RegisterName, value: RegisterValue) {
        match register {
            RegisterName::Named { name, append } => {
                if append {
                    self.named
                        .entry(name)
                        .and_modify(|existing| existing.text.push_str(&value.text))
                        .or_insert(value);
                } else {
                    self.named.insert(name, value);
                }
            }
            RegisterName::Numbered(number) => {
                if self.numbered.len() < 10 {
                    self.numbered.resize(10, None);
                }
                if number < 10 {
                    self.numbered[number as usize] = Some(value);
                }
            }
            RegisterName::Yank => self.yank = Some(value),
            RegisterName::SmallDelete => self.small_delete = Some(value),
            RegisterName::Clipboard | RegisterName::Primary => {
                self.clipboard = Some(value);
                self.clipboard_dirty = true;
            }
            RegisterName::Unnamed | RegisterName::BlackHole => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str, shape: SelectionShape) -> RegisterValue {
        RegisterValue {
            text: text.to_owned(),
            shape,
        }
    }

    #[test]
    fn named_append_registers_concatenate() {
        let mut registers = RegisterStore::default();
        registers.record_yank(
            RegisterName::Named {
                name: 'a',
                append: false,
            },
            value("one", SelectionShape::CharacterWise),
        );
        registers.record_yank(
            RegisterName::Named {
                name: 'a',
                append: true,
            },
            value("two", SelectionShape::CharacterWise),
        );
        assert_eq!(
            registers
                .read(RegisterName::Named {
                    name: 'a',
                    append: false
                })
                .unwrap()
                .text,
            "onetwo"
        );
    }

    #[test]
    fn delete_histories_distinguish_line_and_small_deletes() {
        let mut registers = RegisterStore::default();
        registers.record_delete(
            RegisterName::Unnamed,
            value("line\n", SelectionShape::LineWise),
        );
        registers.record_delete(
            RegisterName::Unnamed,
            value("x", SelectionShape::CharacterWise),
        );
        assert_eq!(
            registers.read(RegisterName::Numbered(1)).unwrap().text,
            "line\n"
        );
        assert_eq!(registers.read(RegisterName::SmallDelete).unwrap().text, "x");
    }
}
