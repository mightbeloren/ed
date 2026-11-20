use crate::{
    buffer::{Buffer, BufferPosition, CharOffset},
    vim::Direction,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Search {
    pub query: String,
    pub direction: Direction,
    pub whole_word: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchPrompt {
    pub direction: Direction,
    pub query: String,
}

impl SearchPrompt {
    pub fn display(&self) -> String {
        let prefix = match self.direction {
            Direction::Forward => '/',
            Direction::Backward => '?',
        };
        format!("{prefix}{}", self.query)
    }
}

pub fn find(
    buffer: &Buffer,
    origin: BufferPosition,
    search: &Search,
    count: u32,
) -> Option<BufferPosition> {
    if search.query.is_empty() {
        return None;
    }
    let haystack: Vec<char> = buffer
        .text(CharOffset(0), CharOffset(buffer.len_chars()))?
        .chars()
        .collect();
    let needle: Vec<char> = search.query.chars().collect();
    if needle.len() > haystack.len() {
        return None;
    }

    let mut offset = buffer.position_to_offset(origin).ok()?.0;
    for _ in 0..count.max(1) {
        offset = find_once(
            &haystack,
            &needle,
            offset,
            search.direction,
            search.whole_word,
        )?;
    }
    buffer.offset_to_position(CharOffset(offset)).ok()
}

pub fn word_under_cursor(buffer: &Buffer, position: BufferPosition) -> Option<String> {
    let line = buffer.line_text(position.line)?;
    let characters: Vec<char> = line.chars().collect();
    let current = *characters.get(position.character)?;
    if !is_keyword(current) {
        return None;
    }

    let mut start = position.character;
    while start > 0 && is_keyword(characters[start - 1]) {
        start -= 1;
    }
    let mut end = position.character + 1;
    while end < characters.len() && is_keyword(characters[end]) {
        end += 1;
    }
    Some(characters[start..end].iter().collect())
}

fn find_once(
    haystack: &[char],
    needle: &[char],
    origin: usize,
    direction: Direction,
    whole_word: bool,
) -> Option<usize> {
    let last_start = haystack.len().checked_sub(needle.len())?;
    let matches = |start: usize| {
        haystack.get(start..start + needle.len()) == Some(needle)
            && (!whole_word || has_word_boundaries(haystack, start, needle.len()))
    };

    match direction {
        Direction::Forward => ((origin + 1)..=last_start)
            .chain(0..=origin.min(last_start))
            .find(|&start| matches(start)),
        Direction::Backward => (0..origin.min(last_start + 1))
            .rev()
            .chain(((origin.saturating_add(1)).min(last_start + 1)..=last_start).rev())
            .find(|&start| matches(start)),
    }
}

fn has_word_boundaries(haystack: &[char], start: usize, length: usize) -> bool {
    let before = start.checked_sub(1).and_then(|index| haystack.get(index));
    let after = haystack.get(start + length);
    before.is_none_or(|character| !is_keyword(*character))
        && after.is_none_or(|character| !is_keyword(*character))
}

fn is_keyword(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer_with_text(text: &str) -> Buffer {
        let mut buffer = Buffer::scratch();
        buffer
            .insert(CharOffset(0), text)
            .expect("valid test insertion");
        buffer
    }

    #[test]
    fn searches_wrap_in_both_directions() {
        let buffer = buffer_with_text("one two one");
        let forward = Search {
            query: "one".to_owned(),
            direction: Direction::Forward,
            whole_word: false,
        };
        let backward = Search {
            direction: Direction::Backward,
            ..forward.clone()
        };

        assert_eq!(
            find(&buffer, BufferPosition::default(), &forward, 1),
            Some(BufferPosition {
                line: 0,
                character: 8
            })
        );
        assert_eq!(
            find(&buffer, BufferPosition::default(), &backward, 1),
            Some(BufferPosition {
                line: 0,
                character: 8
            })
        );
    }

    #[test]
    fn whole_word_search_does_not_match_inside_identifiers() {
        let buffer = buffer_with_text("item items item");
        let search = Search {
            query: "item".to_owned(),
            direction: Direction::Forward,
            whole_word: true,
        };

        assert_eq!(
            find(&buffer, BufferPosition::default(), &search, 1),
            Some(BufferPosition {
                line: 0,
                character: 11
            })
        );
    }
}
