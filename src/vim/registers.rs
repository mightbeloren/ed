use super::{RegisterName, SelectionShape};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisterValue {
    pub text: String,
    pub shape: SelectionShape,
}

#[derive(Debug, Default)]
pub struct RegisterStore {
    unnamed: Option<RegisterValue>,
}

impl RegisterStore {
    pub fn write(&mut self, register: RegisterName, value: RegisterValue) {
        if register == RegisterName::Unnamed {
            self.unnamed = Some(value);
        }
    }

    pub fn unnamed(&self) -> Option<&RegisterValue> {
        self.unnamed.as_ref()
    }
}
