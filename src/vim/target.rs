use crate::buffer::{Buffer, BufferPosition, CharOffset};

use super::{Direction, FindMotion, Motion, Target, TextObject, TextObjectKind, TextObjectScope};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionShape {
    CharacterWise,
    LineWise,
    BlockWise,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolvedTarget {
    pub start: CharOffset,
    pub end: CharOffset,
    pub shape: SelectionShape,
    pub cursor: BufferPosition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionOutcome {
    pub destination: BufferPosition,
    kind: MotionKind,
    pub find: Option<FindMotion>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MotionKind {
    Exclusive,
    Inclusive,
    LineWise,
}

pub fn resolve_motion(
    buffer: &Buffer,
    start: BufferPosition,
    desired_character: usize,
    motion: Motion,
    count: u32,
    last_find: Option<FindMotion>,
    page_rows: usize,
) -> Option<MotionOutcome> {
    let count = count.max(1) as usize;
    let last_line = buffer.len_lines().saturating_sub(1);
    let line_length = buffer.line_len_chars(start.line).unwrap_or(0);
    let line_max = line_length.saturating_sub(1);

    let (destination, kind, find) = match motion {
        Motion::Left => (
            BufferPosition {
                line: start.line,
                character: start.character.saturating_sub(count),
            },
            MotionKind::Exclusive,
            None,
        ),
        Motion::Right => {
            let character = start.character.saturating_add(count).min(line_max);
            (
                BufferPosition {
                    line: start.line,
                    character,
                },
                if character == start.character && line_length > 0 {
                    MotionKind::Inclusive
                } else {
                    MotionKind::Exclusive
                },
                None,
            )
        }
        Motion::Down | Motion::Up => {
            let distance = count.min(isize::MAX as usize) as isize;
            let delta = if motion == Motion::Down {
                distance
            } else {
                -distance
            };
            let line = start.line.saturating_add_signed(delta).min(last_line);
            (
                BufferPosition {
                    line,
                    character: desired_character
                        .min(buffer.line_len_chars(line).unwrap_or(0).saturating_sub(1)),
                },
                MotionKind::LineWise,
                None,
            )
        }
        Motion::PageDown | Motion::PageUp => {
            let distance = count.saturating_mul(page_rows).min(isize::MAX as usize) as isize;
            let delta = if motion == Motion::PageDown {
                distance
            } else {
                -distance
            };
            let line = start.line.saturating_add_signed(delta).min(last_line);
            (
                BufferPosition {
                    line,
                    character: desired_character
                        .min(buffer.line_len_chars(line).unwrap_or(0).saturating_sub(1)),
                },
                MotionKind::LineWise,
                None,
            )
        }
        Motion::LineStart => (
            BufferPosition {
                line: start.line,
                character: 0,
            },
            MotionKind::Exclusive,
            None,
        ),
        Motion::FirstNonBlank => {
            let character = buffer
                .line_text(start.line)
                .unwrap_or_default()
                .chars()
                .take_while(|character| character.is_whitespace())
                .count()
                .min(line_max);
            (
                BufferPosition {
                    line: start.line,
                    character,
                },
                MotionKind::Exclusive,
                None,
            )
        }
        Motion::LineEnd => {
            let line = start
                .line
                .saturating_add(count.saturating_sub(1))
                .min(last_line);
            (
                BufferPosition {
                    line,
                    character: buffer.line_len_chars(line).unwrap_or(0).saturating_sub(1),
                },
                MotionKind::Inclusive,
                None,
            )
        }
        Motion::GoToLine { line } => {
            let line = line.unwrap_or(last_line).min(last_line);
            (
                BufferPosition {
                    line,
                    character: desired_character
                        .min(buffer.line_len_chars(line).unwrap_or(0).saturating_sub(1)),
                },
                MotionKind::LineWise,
                None,
            )
        }
        Motion::WordForward { big_word } => {
            let start_offset = buffer.position_to_offset(start).ok()?;
            let (destination, reached_end) =
                repeat_word_forward(buffer, start_offset, big_word, count);
            let kind = if reached_end {
                MotionKind::Inclusive
            } else {
                MotionKind::Exclusive
            };
            (normal_position(buffer, destination)?, kind, None)
        }
        Motion::WordBackward { big_word } => {
            let start_offset = buffer.position_to_offset(start).ok()?;
            let destination = repeat_offset(count, start_offset, |offset| {
                word_backward(buffer, offset, big_word)
            });
            (
                normal_position(buffer, destination)?,
                MotionKind::Exclusive,
                None,
            )
        }
        Motion::WordEnd { big_word } => {
            let start_offset = buffer.position_to_offset(start).ok()?;
            let destination = repeat_offset(count, start_offset, |offset| {
                word_end(buffer, offset, big_word)
            });
            (
                normal_position(buffer, destination)?,
                MotionKind::Inclusive,
                None,
            )
        }
        Motion::FindCharacter(find) => {
            let destination = repeat_find(buffer, start, find, count)?;
            (destination, MotionKind::Inclusive, Some(find))
        }
        Motion::RepeatFind { reverse } => {
            let mut find = last_find?;
            if reverse {
                find.direction = opposite(find.direction);
            }
            let destination = repeat_find(buffer, start, find, count)?;
            (destination, MotionKind::Inclusive, None)
        }
        Motion::MatchingDelimiter => (
            matching_delimiter(buffer, start)?,
            MotionKind::Inclusive,
            None,
        ),
        Motion::Underscore | Motion::NextLineFirstNonBlank | Motion::PreviousLineFirstNonBlank => {
            let distance = match motion {
                Motion::Underscore => count.saturating_sub(1),
                _ => count,
            };
            let line = match motion {
                Motion::PreviousLineFirstNonBlank => start.line.saturating_sub(distance),
                _ => start.line.saturating_add(distance).min(last_line),
            };
            let character = buffer
                .line_text(line)
                .unwrap_or_default()
                .chars()
                .take_while(|character| character.is_whitespace())
                .count()
                .min(buffer.line_len_chars(line).unwrap_or(0).saturating_sub(1));
            (
                BufferPosition { line, character },
                MotionKind::LineWise,
                None,
            )
        }
    };

    Some(MotionOutcome {
        destination,
        kind,
        find,
    })
}

pub fn resolve_target(
    buffer: &Buffer,
    cursor: BufferPosition,
    desired_character: usize,
    target: Target,
    count: u32,
    last_find: Option<FindMotion>,
    page_rows: usize,
) -> Option<ResolvedTarget> {
    match target {
        Target::CurrentLine => linewise_target(buffer, cursor.line, cursor.line, count),
        Target::TextObject(object) => resolve_text_object(buffer, cursor, object, count),
        Target::Motion(motion) => {
            let outcome = resolve_motion(
                buffer,
                cursor,
                desired_character,
                motion,
                count,
                last_find,
                page_rows,
            )?;
            if outcome.kind == MotionKind::LineWise {
                return linewise_target(buffer, cursor.line, outcome.destination.line, 1);
            }

            let origin = buffer.position_to_offset(cursor).ok()?;
            let destination = buffer.position_to_offset(outcome.destination).ok()?;
            let (start, end) = if destination >= origin {
                let end = match outcome.kind {
                    MotionKind::Inclusive if destination.0 < buffer.len_chars() => {
                        CharOffset(destination.0 + 1)
                    }
                    _ => destination,
                };
                (origin, end)
            } else {
                let end = match outcome.kind {
                    MotionKind::Inclusive if origin.0 < buffer.len_chars() => {
                        CharOffset(origin.0 + 1)
                    }
                    _ => origin,
                };
                (destination, end)
            };

            (start < end).then_some(ResolvedTarget {
                start,
                end,
                shape: SelectionShape::CharacterWise,
                cursor: buffer.offset_to_position(start).ok()?,
            })
        }
    }
}

fn resolve_text_object(
    buffer: &Buffer,
    cursor: BufferPosition,
    object: TextObject,
    _count: u32,
) -> Option<ResolvedTarget> {
    let cursor_offset = buffer.position_to_offset(cursor).ok()?;
    let (start, end, shape) = match object.kind {
        TextObjectKind::Word { big_word } => {
            let class = word_class(buffer.char_at(cursor_offset)?, big_word);
            let mut start = cursor_offset.0;
            let mut end = cursor_offset.0 + 1;
            while start > 0 && word_class(buffer.char_at(CharOffset(start - 1))?, big_word) == class
            {
                start -= 1;
            }
            while end < buffer.len_chars()
                && word_class(buffer.char_at(CharOffset(end))?, big_word) == class
            {
                end += 1;
            }
            if object.scope == TextObjectScope::Around {
                while end < buffer.len_chars() && buffer.char_at(CharOffset(end))?.is_whitespace() {
                    end += 1;
                }
                if end == cursor_offset.0 + 1 {
                    while start > 0 && buffer.char_at(CharOffset(start - 1))?.is_whitespace() {
                        start -= 1;
                    }
                }
            }
            (
                CharOffset(start),
                CharOffset(end),
                SelectionShape::CharacterWise,
            )
        }
        TextObjectKind::Quotes(quote) => {
            let line = buffer.line_text(cursor.line)?;
            let chars: Vec<_> = line.chars().collect();
            let left = (0..=cursor.character.min(chars.len().saturating_sub(1)))
                .rev()
                .find(|&index| chars[index] == quote)?;
            let right = (cursor.character.saturating_add(1)..chars.len())
                .find(|&index| chars[index] == quote)?;
            let line_start = buffer.line_start_offset(cursor.line)?;
            let padding = usize::from(object.scope == TextObjectScope::Inner);
            (
                CharOffset(line_start.0 + left + padding),
                CharOffset(line_start.0 + right + 1 - padding),
                SelectionShape::CharacterWise,
            )
        }
        TextObjectKind::Delimited { open, close } => {
            let mut left = cursor_offset.0;
            while buffer.char_at(CharOffset(left))? != open {
                if left == 0 {
                    return None;
                }
                left -= 1;
            }
            let mut right = cursor_offset.0;
            while right < buffer.len_chars() && buffer.char_at(CharOffset(right))? != close {
                right += 1;
            }
            if right >= buffer.len_chars() {
                return None;
            }
            let padding = usize::from(object.scope == TextObjectScope::Inner);
            (
                CharOffset(left + padding),
                CharOffset(right + 1 - padding),
                SelectionShape::CharacterWise,
            )
        }
        TextObjectKind::Paragraph => {
            let mut first = cursor.line;
            let mut last = cursor.line;
            while first > 0 && !buffer.line_text(first - 1)?.trim().is_empty() {
                first -= 1;
            }
            while last + 1 < buffer.len_lines() && !buffer.line_text(last + 1)?.trim().is_empty() {
                last += 1;
            }
            if object.scope == TextObjectScope::Around && last + 1 < buffer.len_lines() {
                last += 1;
            }
            return linewise_target(buffer, first, last, 1);
        }
        TextObjectKind::Tag => {
            let text = buffer.text(CharOffset(0), CharOffset(buffer.len_chars()))?;
            let before = &text[..text
                .char_indices()
                .nth(cursor_offset.0)
                .map_or(text.len(), |(i, _)| i)];
            let open_byte = before.rfind('<')?;
            let open_end = text[open_byte..].find('>')? + open_byte;
            let name: String = text[open_byte + 1..open_end]
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '>')
                .collect();
            if name.is_empty() || name.starts_with('/') {
                return None;
            }
            let close_pattern = format!("</{name}>");
            let close_byte = text[open_end + 1..].find(&close_pattern)? + open_end + 1;
            let byte_to_char = |byte: usize| text[..byte].chars().count();
            let (start, end) = if object.scope == TextObjectScope::Inner {
                (byte_to_char(open_end + 1), byte_to_char(close_byte))
            } else {
                (
                    byte_to_char(open_byte),
                    byte_to_char(close_byte + close_pattern.len()),
                )
            };
            (
                CharOffset(start),
                CharOffset(end),
                SelectionShape::CharacterWise,
            )
        }
    };
    Some(ResolvedTarget {
        start,
        end,
        shape,
        cursor: buffer.offset_to_position(start).ok()?,
    })
}

fn linewise_target(
    buffer: &Buffer,
    first_line: usize,
    second_line: usize,
    count: u32,
) -> Option<ResolvedTarget> {
    let start_line = first_line.min(second_line);
    let mut end_line = first_line.max(second_line);
    end_line = end_line
        .saturating_add(count.max(1).saturating_sub(1) as usize)
        .min(buffer.len_lines().saturating_sub(1));
    let start = buffer.line_start_offset(start_line)?;
    let end = buffer
        .line_start_offset(end_line + 1)
        .unwrap_or(CharOffset(buffer.len_chars()));
    Some(ResolvedTarget {
        start,
        end,
        shape: SelectionShape::LineWise,
        cursor: BufferPosition {
            line: start_line,
            character: 0,
        },
    })
}

fn normal_position(buffer: &Buffer, offset: CharOffset) -> Option<BufferPosition> {
    let mut position = buffer.offset_to_position(offset).ok()?;
    position.character = position.character.min(
        buffer
            .line_len_chars(position.line)
            .unwrap_or(0)
            .saturating_sub(1),
    );
    Some(position)
}

fn repeat_offset(
    count: usize,
    mut offset: CharOffset,
    mut resolve: impl FnMut(CharOffset) -> CharOffset,
) -> CharOffset {
    for _ in 0..count {
        let next = resolve(offset);
        if next == offset {
            break;
        }
        offset = next;
    }
    offset
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum WordClass {
    Space,
    Keyword,
    Punctuation,
}

fn word_class(character: char, big_word: bool) -> WordClass {
    if character.is_whitespace() {
        WordClass::Space
    } else if big_word || character.is_alphanumeric() || character == '_' {
        WordClass::Keyword
    } else {
        WordClass::Punctuation
    }
}

fn repeat_word_forward(
    buffer: &Buffer,
    mut offset: CharOffset,
    big_word: bool,
    count: usize,
) -> (CharOffset, bool) {
    for _ in 0..count {
        let Some(next) = word_forward(buffer, offset, big_word) else {
            return (offset, true);
        };
        offset = next;
    }
    (offset, false)
}

fn word_forward(buffer: &Buffer, offset: CharOffset, big_word: bool) -> Option<CharOffset> {
    let length = buffer.len_chars();
    if length == 0 || offset.0 >= length.saturating_sub(1) {
        return None;
    }
    let mut index = offset.0;
    let class = word_class(buffer.char_at(CharOffset(index)).unwrap_or(' '), big_word);
    if class != WordClass::Space {
        while index < length
            && word_class(buffer.char_at(CharOffset(index)).unwrap_or(' '), big_word) == class
        {
            index += 1;
        }
    }
    while index < length
        && word_class(buffer.char_at(CharOffset(index)).unwrap_or(' '), big_word)
            == WordClass::Space
    {
        index += 1;
    }
    (index < length).then_some(CharOffset(index))
}

fn word_backward(buffer: &Buffer, offset: CharOffset, big_word: bool) -> CharOffset {
    if offset.0 == 0 {
        return offset;
    }
    let mut index = offset.0 - 1;
    while index > 0
        && word_class(buffer.char_at(CharOffset(index)).unwrap_or(' '), big_word)
            == WordClass::Space
    {
        index -= 1;
    }
    let class = word_class(buffer.char_at(CharOffset(index)).unwrap_or(' '), big_word);
    while index > 0
        && word_class(
            buffer.char_at(CharOffset(index - 1)).unwrap_or(' '),
            big_word,
        ) == class
    {
        index -= 1;
    }
    CharOffset(index)
}

fn word_end(buffer: &Buffer, offset: CharOffset, big_word: bool) -> CharOffset {
    let length = buffer.len_chars();
    if length == 0 {
        return CharOffset(0);
    }
    let mut index = (offset.0 + 1).min(length - 1);
    while index < length - 1
        && word_class(buffer.char_at(CharOffset(index)).unwrap_or(' '), big_word)
            == WordClass::Space
    {
        index += 1;
    }
    let class = word_class(buffer.char_at(CharOffset(index)).unwrap_or(' '), big_word);
    while index + 1 < length
        && word_class(
            buffer.char_at(CharOffset(index + 1)).unwrap_or(' '),
            big_word,
        ) == class
    {
        index += 1;
    }
    CharOffset(index)
}

fn repeat_find(
    buffer: &Buffer,
    start: BufferPosition,
    find: FindMotion,
    count: usize,
) -> Option<BufferPosition> {
    let text: Vec<char> = buffer.line_text(start.line)?.chars().collect();
    let mut origin = start.character;
    for _ in 0..count {
        origin = match find.direction {
            Direction::Forward => {
                ((origin + 1)..text.len()).find(|&index| text[index] == find.character)?
            }
            Direction::Backward => (0..origin)
                .rev()
                .find(|&index| text[index] == find.character)?,
        };
    }
    let character = if find.till {
        match find.direction {
            Direction::Forward => origin.saturating_sub(1),
            Direction::Backward => (origin + 1).min(text.len().saturating_sub(1)),
        }
    } else {
        origin
    };
    Some(BufferPosition {
        line: start.line,
        character,
    })
}

fn matching_delimiter(buffer: &Buffer, start: BufferPosition) -> Option<BufferPosition> {
    let offset = buffer.position_to_offset(start).ok()?;
    let character = buffer.char_at(offset)?;
    let (matching, direction) = match character {
        '(' => (')', Direction::Forward),
        '[' => (']', Direction::Forward),
        '{' => ('}', Direction::Forward),
        ')' => ('(', Direction::Backward),
        ']' => ('[', Direction::Backward),
        '}' => ('{', Direction::Backward),
        _ => return None,
    };
    let mut depth = 0usize;
    match direction {
        Direction::Forward => {
            for index in offset.0 + 1..buffer.len_chars() {
                match buffer.char_at(CharOffset(index)) {
                    Some(value) if value == character => depth += 1,
                    Some(value) if value == matching && depth == 0 => {
                        return normal_position(buffer, CharOffset(index));
                    }
                    Some(value) if value == matching => depth -= 1,
                    _ => {}
                }
            }
        }
        Direction::Backward => {
            for index in (0..offset.0).rev() {
                match buffer.char_at(CharOffset(index)) {
                    Some(value) if value == character => depth += 1,
                    Some(value) if value == matching && depth == 0 => {
                        return normal_position(buffer, CharOffset(index));
                    }
                    Some(value) if value == matching => depth -= 1,
                    _ => {}
                }
            }
        }
    }
    None
}

fn opposite(direction: Direction) -> Direction {
    match direction {
        Direction::Forward => Direction::Backward,
        Direction::Backward => Direction::Forward,
    }
}
