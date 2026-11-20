use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Address {
    Current,
    Last,
    Line(usize),
    Mark(char),
    VisualStart,
    VisualEnd,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RangeSeparator {
    Comma,
    Semicolon,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum RangeSpec {
    #[default]
    Default,
    Whole,
    Addresses {
        start: Address,
        end: Option<Address>,
        separator: Option<RangeSeparator>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Substitute {
    pub pattern: String,
    pub replacement: String,
    pub global: bool,
    pub case_insensitive: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GlobalAction {
    Count,
    Delete,
    Substitute(Substitute),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExCommand {
    GoTo(Address),
    Write {
        range: RangeSpec,
        path: Option<String>,
        force: bool,
        quit: bool,
    },
    Quit {
        force: bool,
    },
    Edit {
        path: Option<String>,
        force: bool,
    },
    Substitute {
        range: RangeSpec,
        substitution: Substitute,
    },
    Global {
        range: RangeSpec,
        pattern: String,
        case_insensitive: bool,
        invert: bool,
        action: GlobalAction,
    },
    Delete {
        range: RangeSpec,
    },
    Shell {
        command: String,
        open_immediately: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExError(pub String);

impl fmt::Display for ExError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for ExError {}

pub fn parse(input: &str) -> Result<ExCommand, ExError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ExError("Empty Ex command".to_owned()));
    }
    let (range, rest) = parse_range(input)?;
    let rest = rest.trim_start();
    if rest.is_empty() {
        return match &range {
            RangeSpec::Addresses {
                start, end: None, ..
            } => Ok(ExCommand::GoTo(*start)),
            _ => Err(ExError("Missing Ex command".to_owned())),
        };
    }
    if let Some(command) = rest.strip_prefix('!') {
        require_default_range(&range)?;
        let command = command.trim_start();
        if command.is_empty() {
            return Err(ExError("Missing shell command".to_owned()));
        }
        return Ok(ExCommand::Shell {
            command: command.to_owned(),
            open_immediately: false,
        });
    }

    let name_end = rest
        .char_indices()
        .take_while(|(_, character)| character.is_ascii_alphabetic())
        .map(|(index, character)| index + character.len_utf8())
        .last()
        .unwrap_or(0);
    let name = &rest[..name_end];
    let mut arguments = &rest[name_end..];
    let force = arguments.starts_with('!');
    if force {
        arguments = &arguments[1..];
    }
    arguments = arguments.trim_start();

    match name {
        "w" | "write" => Ok(ExCommand::Write {
            range,
            path: nonempty(arguments),
            force,
            quit: false,
        }),
        "wq" => Ok(ExCommand::Write {
            range,
            path: nonempty(arguments),
            force,
            quit: true,
        }),
        "q" | "quit" => {
            require_default_range(&range)?;
            no_arguments(arguments, ExCommand::Quit { force })
        }
        "e" | "edit" => {
            require_default_range(&range)?;
            Ok(ExCommand::Edit {
                path: nonempty(arguments),
                force,
            })
        }
        "s" | "substitute" => Ok(ExCommand::Substitute {
            range,
            substitution: parse_substitution(arguments)?,
        }),
        "g" | "global" => parse_global(range, arguments, false),
        "v" | "vglobal" => parse_global(range, arguments, true),
        "d" | "delete" => no_arguments(arguments, ExCommand::Delete { range }),
        "run" => {
            require_default_range(&range)?;
            if arguments.is_empty() {
                Err(ExError("Missing shell command".to_owned()))
            } else {
                Ok(ExCommand::Shell {
                    command: arguments.to_owned(),
                    open_immediately: true,
                })
            }
        }
        _ => Err(ExError(format!("Not an editor command: {name}"))),
    }
}

fn parse_range(input: &str) -> Result<(RangeSpec, &str), ExError> {
    if let Some(rest) = input.strip_prefix('%') {
        return Ok((RangeSpec::Whole, rest));
    }
    let Some((start, rest)) = parse_address(input)? else {
        return Ok((RangeSpec::Default, input));
    };
    let rest = rest.trim_start();
    if matches!(rest.chars().next(), Some(',' | ';')) {
        let separator = if rest.starts_with(';') {
            RangeSeparator::Semicolon
        } else {
            RangeSeparator::Comma
        };
        let rest = rest[1..].trim_start();
        let Some((end, rest)) = parse_address(rest)? else {
            return Err(ExError("Missing range end".to_owned()));
        };
        Ok((
            RangeSpec::Addresses {
                start,
                end: Some(end),
                separator: Some(separator),
            },
            rest,
        ))
    } else {
        Ok((
            RangeSpec::Addresses {
                start,
                end: None,
                separator: None,
            },
            rest,
        ))
    }
}

fn parse_address(input: &str) -> Result<Option<(Address, &str)>, ExError> {
    let Some(first) = input.chars().next() else {
        return Ok(None);
    };
    match first {
        '.' => Ok(Some((Address::Current, &input[1..]))),
        '$' => Ok(Some((Address::Last, &input[1..]))),
        '\'' => {
            let mut characters = input.char_indices();
            characters.next();
            let Some((index, mark)) = characters.next() else {
                return Err(ExError("Missing mark name".to_owned()));
            };
            let address = match mark {
                '<' => Address::VisualStart,
                '>' => Address::VisualEnd,
                'a'..='z' => Address::Mark(mark),
                _ => return Err(ExError(format!("Invalid mark: {mark}"))),
            };
            Ok(Some((address, &input[index + mark.len_utf8()..])))
        }
        '0'..='9' => {
            let end = input
                .char_indices()
                .take_while(|(_, character)| character.is_ascii_digit())
                .map(|(index, character)| index + character.len_utf8())
                .last()
                .unwrap_or(0);
            let line = input[..end]
                .parse::<usize>()
                .map_err(|_| ExError("Invalid line address".to_owned()))?;
            if line == 0 {
                return Err(ExError("Line addresses are 1-based".to_owned()));
            }
            Ok(Some((Address::Line(line), &input[end..])))
        }
        _ => Ok(None),
    }
}

fn parse_substitution(arguments: &str) -> Result<Substitute, ExError> {
    let (delimiter, rest) = take_delimiter(arguments)?;
    let (pattern, rest) = take_delimited(rest, delimiter)?;
    let (replacement, flags) = take_delimited(rest, delimiter)?;
    let flags = flags.trim();
    if flags.chars().any(|flag| !matches!(flag, 'g' | 'i')) {
        return Err(ExError(format!("Unsupported substitute flags: {flags}")));
    }
    Ok(Substitute {
        pattern,
        replacement,
        global: flags.contains('g'),
        case_insensitive: flags.contains('i'),
    })
}

fn parse_global(range: RangeSpec, arguments: &str, invert: bool) -> Result<ExCommand, ExError> {
    let (delimiter, rest) = take_delimiter(arguments)?;
    let (pattern, action) = take_delimited(rest, delimiter)?;
    let action = action.trim();
    let action = if action.is_empty() || matches!(action, "p" | "print") {
        GlobalAction::Count
    } else if matches!(action, "d" | "delete") {
        GlobalAction::Delete
    } else if let Some(arguments) = action.strip_prefix('s') {
        GlobalAction::Substitute(parse_substitution(arguments)?)
    } else {
        return Err(ExError(format!("Unsupported global command: {action}")));
    };
    Ok(ExCommand::Global {
        range,
        pattern,
        case_insensitive: false,
        invert,
        action,
    })
}

fn take_delimiter(input: &str) -> Result<(char, &str), ExError> {
    let input = input.trim_start();
    let Some(delimiter) = input.chars().next() else {
        return Err(ExError("Missing command delimiter".to_owned()));
    };
    if delimiter.is_alphanumeric() || delimiter.is_whitespace() || delimiter == '\\' {
        return Err(ExError("Invalid command delimiter".to_owned()));
    }
    Ok((delimiter, &input[delimiter.len_utf8()..]))
}

fn take_delimited(input: &str, delimiter: char) -> Result<(String, &str), ExError> {
    let mut value = String::new();
    let mut characters = input.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        if character == delimiter {
            return Ok((value, &input[index + character.len_utf8()..]));
        }
        if character == '\\'
            && let Some((_, next)) = characters.peek().copied()
            && next == delimiter
        {
            characters.next();
            value.push(delimiter);
            continue;
        }
        value.push(character);
    }
    Err(ExError(format!("Missing closing delimiter: {delimiter}")))
}

fn require_default_range(range: &RangeSpec) -> Result<(), ExError> {
    if matches!(range, RangeSpec::Default) {
        Ok(())
    } else {
        Err(ExError("No range allowed".to_owned()))
    }
}

fn nonempty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

fn no_arguments(arguments: &str, command: ExCommand) -> Result<ExCommand, ExError> {
    if arguments.is_empty() {
        Ok(command)
    } else {
        Err(ExError("Trailing characters".to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_addresses_and_ranges() {
        assert_eq!(parse("42"), Ok(ExCommand::GoTo(Address::Line(42))));
        assert_eq!(
            parse(".,$d"),
            Ok(ExCommand::Delete {
                range: RangeSpec::Addresses {
                    start: Address::Current,
                    end: Some(Address::Last),
                    separator: Some(RangeSeparator::Comma)
                }
            })
        );
        assert_eq!(
            parse("'<,'>s/old/new/g"),
            Ok(ExCommand::Substitute {
                range: RangeSpec::Addresses {
                    start: Address::VisualStart,
                    end: Some(Address::VisualEnd),
                    separator: Some(RangeSeparator::Comma)
                },
                substitution: Substitute {
                    pattern: "old".to_owned(),
                    replacement: "new".to_owned(),
                    global: true,
                    case_insensitive: false
                }
            })
        );
    }

    #[test]
    fn rejects_ranges_for_lifecycle_commands() {
        assert_eq!(parse("%q!"), Err(ExError("No range allowed".to_owned())));
        assert_eq!(
            parse("1e! other.txt"),
            Err(ExError("No range allowed".to_owned()))
        );
    }

    #[test]
    fn parses_shell_commands() {
        assert_eq!(
            parse("!printf hello"),
            Ok(ExCommand::Shell {
                command: "printf hello".to_owned(),
                open_immediately: false
            })
        );
        assert_eq!(
            parse("run cargo test"),
            Ok(ExCommand::Shell {
                command: "cargo test".to_owned(),
                open_immediately: true
            })
        );
        assert_eq!(parse("!"), Err(ExError("Missing shell command".to_owned())));
        assert_eq!(
            parse("run"),
            Err(ExError("Missing shell command".to_owned()))
        );
    }

    #[test]
    fn parses_file_and_global_commands() {
        assert_eq!(
            parse("wq! output.txt"),
            Ok(ExCommand::Write {
                range: RangeSpec::Default,
                path: Some("output.txt".to_owned()),
                force: true,
                quit: true
            })
        );
        assert_eq!(
            parse("g/error/d"),
            Ok(ExCommand::Global {
                range: RangeSpec::Default,
                pattern: "error".to_owned(),
                case_insensitive: false,
                invert: false,
                action: GlobalAction::Delete
            })
        );
    }
}
