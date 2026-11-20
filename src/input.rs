use std::fmt;

use gpui::Keystroke;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Key {
    pub value: KeyValue,
    pub modifiers: Modifiers,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KeyValue {
    Character(String),
    Named(String),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Modifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
    pub function: bool,
}

impl From<&Keystroke> for Key {
    fn from(keystroke: &Keystroke) -> Self {
        let modifiers = Modifiers {
            control: keystroke.modifiers.control,
            alt: keystroke.modifiers.alt,
            shift: keystroke.modifiers.shift,
            super_key: keystroke.modifiers.platform,
            function: keystroke.modifiers.function,
        };

        let printable = (!modifiers.control && !modifiers.alt && !modifiers.super_key)
            .then(|| {
                keystroke.key_char.clone().or_else(|| {
                    (keystroke.key.chars().count() == 1).then(|| {
                        if modifiers.shift {
                            keystroke.key.to_uppercase()
                        } else {
                            keystroke.key.clone()
                        }
                    })
                })
            })
            .flatten();

        let value = printable
            .map(KeyValue::Character)
            .unwrap_or_else(|| KeyValue::Named(keystroke.key.clone()));

        Self { value, modifiers }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.control {
            write!(formatter, "Ctrl+")?;
        }
        if self.modifiers.alt {
            write!(formatter, "Alt+")?;
        }
        if self.modifiers.shift {
            write!(formatter, "Shift+")?;
        }
        if self.modifiers.super_key {
            write!(formatter, "Super+")?;
        }
        if self.modifiers.function {
            write!(formatter, "Fn+")?;
        }

        match &self.value {
            KeyValue::Character(character) => write!(formatter, "{character:?}"),
            KeyValue::Named(name) => write!(formatter, "{name}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::Keystroke;

    use super::{Key, KeyValue};

    #[test]
    fn normalizes_printable_input() {
        let keystroke = Keystroke::parse("a").expect("valid keystroke");
        let key = Key::from(&keystroke);

        assert_eq!(key.value, KeyValue::Character("a".to_owned()));
        assert_eq!(key.to_string(), "\"a\"");
    }

    #[test]
    fn normalizes_shifted_printable_input() {
        let keystroke = Keystroke::parse("G").expect("valid keystroke");
        let key = Key::from(&keystroke);

        assert_eq!(key.value, KeyValue::Character("G".to_owned()));
    }

    #[test]
    fn preserves_control_chords_as_named_keys() {
        let keystroke = Keystroke::parse("ctrl-r").expect("valid keystroke");
        let key = Key::from(&keystroke);

        assert_eq!(key.value, KeyValue::Named("r".to_owned()));
        assert!(key.modifiers.control);
        assert_eq!(key.to_string(), "Ctrl+r");
    }
}
