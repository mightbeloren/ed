use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use tree_sitter::{Parser, QueryCursor, StreamingIterator};

use crate::plugin::PluginRegistry;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HighlightSpan {
    pub line: usize,
    pub start_character: usize,
    pub end_character: usize,
    pub color: u32,
}

#[derive(Clone, Debug)]
pub struct Theme {
    captures: HashMap<String, u32>,
}

#[derive(Deserialize)]
struct ThemeFile {
    captures: HashMap<String, String>,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            captures: HashMap::from([
                ("attribute".to_owned(), 0x9cdcfe),
                ("comment".to_owned(), 0x6a9955),
                ("constant".to_owned(), 0x4fc1ff),
                ("constructor".to_owned(), 0x4ec9b0),
                ("function".to_owned(), 0xdcdcaa),
                ("keyword".to_owned(), 0xc586c0),
                ("label".to_owned(), 0xc8c8c8),
                ("number".to_owned(), 0xb5cea8),
                ("operator".to_owned(), 0xd4d4d4),
                ("property".to_owned(), 0x9cdcfe),
                ("punctuation".to_owned(), 0x808080),
                ("string".to_owned(), 0xce9178),
                ("tag".to_owned(), 0x569cd6),
                ("type".to_owned(), 0x4ec9b0),
                ("variable".to_owned(), 0x9cdcfe),
            ]),
        }
    }
}

impl Theme {
    pub fn load(config_root: &Path) -> (Self, Option<String>) {
        let path = config_root.join("theme.toml");
        if !path.exists() {
            return (Self::default(), None);
        }
        match fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|text| toml::from_str::<ThemeFile>(&text).map_err(|error| error.to_string()))
            .and_then(|file| Self::from_file(file).map_err(|error| error.to_string()))
        {
            Ok(theme) => (theme, None),
            Err(error) => (
                Self::default(),
                Some(format!("could not load {}: {error}", path.display())),
            ),
        }
    }

    fn from_file(file: ThemeFile) -> Result<Self, String> {
        let mut captures = HashMap::new();
        for (capture, color) in file.captures {
            captures.insert(
                capture.trim_start_matches('@').to_owned(),
                parse_color(&color)?,
            );
        }
        Ok(Self { captures })
    }

    pub fn color(&self, capture: &str) -> Option<u32> {
        let mut name = capture.trim_start_matches('@');
        loop {
            if let Some(color) = self.captures.get(name) {
                return Some(*color);
            }
            let (parent, _) = name.rsplit_once('.')?;
            name = parent;
        }
    }
}

pub struct SyntaxManager {
    registry: PluginRegistry,
    theme: Theme,
    parser: Parser,
    active_language: Option<usize>,
    spans: Vec<HighlightSpan>,
    errors: Vec<String>,
}

impl SyntaxManager {
    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            registry: PluginRegistry::default(),
            theme: Theme::default(),
            parser: Parser::new(),
            active_language: None,
            spans: Vec::new(),
            errors: Vec::new(),
        }
    }

    #[cfg(not(test))]
    pub fn load_default() -> Self {
        Self::load(&default_config_root())
    }

    #[cfg(not(test))]
    pub fn load(config_root: &Path) -> Self {
        let registry = PluginRegistry::load(config_root);
        let mut errors = registry.errors().to_vec();
        let (theme, theme_error) = Theme::load(config_root);
        if let Some(error) = theme_error {
            errors.push(error);
        }
        Self {
            registry,
            theme,
            parser: Parser::new(),
            active_language: None,
            spans: Vec::new(),
            errors,
        }
    }

    #[cfg(test)]
    pub fn from_parts(registry: PluginRegistry, theme: Theme) -> Self {
        Self {
            registry,
            theme,
            parser: Parser::new(),
            active_language: None,
            spans: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn refresh(&mut self, path: Option<&Path>, source: &str) {
        self.spans.clear();
        let Some(path) = path else {
            self.active_language = None;
            return;
        };
        let Some((index, plugin)) = self.registry.language_for_path(path) else {
            self.active_language = None;
            return;
        };
        if self.active_language != Some(index) {
            if let Err(error) = self.parser.set_language(plugin.language()) {
                self.errors
                    .push(format!("could not activate {}: {error}", plugin.name()));
                self.active_language = None;
                return;
            }
            self.active_language = Some(index);
        }
        let Some(tree) = self.parser.parse(source, None) else {
            self.errors
                .push(format!("parser for {} returned no tree", plugin.name()));
            return;
        };
        let capture_names = plugin.query().capture_names();
        let line_ranges = line_byte_ranges(source);
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(plugin.query(), tree.root_node(), source.as_bytes());
        while let Some(query_match) = matches.next() {
            for capture in query_match.captures {
                let Some(color) = capture_names
                    .get(capture.index as usize)
                    .and_then(|capture| self.theme.color(capture))
                else {
                    continue;
                };
                push_node_spans(
                    &mut self.spans,
                    source,
                    &line_ranges,
                    capture.node.start_byte(),
                    capture.node.end_byte(),
                    color,
                );
            }
        }
        self.spans
            .sort_by_key(|span| (span.line, span.start_character, span.end_character));
    }

    pub fn spans_for_line(&self, line: usize) -> impl Iterator<Item = HighlightSpan> + '_ {
        self.spans
            .iter()
            .copied()
            .filter(move |span| span.line == line)
    }

    pub fn active_language_name(&self) -> Option<&str> {
        self.active_language
            .and_then(|index| self.registry.language(index))
            .map(|language| language.name())
    }

    pub fn errors(&self) -> &[String] {
        &self.errors
    }
}

#[cfg(not(test))]
pub fn default_config_root() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("ed")
}

fn parse_color(color: &str) -> Result<u32, String> {
    let color = color
        .strip_prefix('#')
        .ok_or_else(|| format!("color must use #RRGGBB: {color}"))?;
    if color.len() != 6 {
        return Err(format!("color must use #RRGGBB: #{color}"));
    }
    u32::from_str_radix(color, 16).map_err(|_| format!("invalid color: #{color}"))
}

#[derive(Clone, Copy)]
struct LineByteRange {
    start: usize,
    end: usize,
}

fn line_byte_ranges(source: &str) -> Vec<LineByteRange> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for line in source.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        ranges.push(LineByteRange {
            start,
            end: start + content.len(),
        });
        start += line.len();
    }
    if source.is_empty() || source.ends_with('\n') {
        ranges.push(LineByteRange { start, end: start });
    }
    ranges
}

fn push_node_spans(
    spans: &mut Vec<HighlightSpan>,
    source: &str,
    lines: &[LineByteRange],
    start_byte: usize,
    end_byte: usize,
    color: u32,
) {
    for (line, range) in lines.iter().enumerate() {
        let start = start_byte.max(range.start);
        let end = end_byte.min(range.end);
        if start >= end {
            continue;
        }
        let start_character = source[range.start..start].chars().count();
        let end_character = start_character + source[start..end].chars().count();
        spans.push(HighlightSpan {
            line,
            start_character,
            end_character,
            color,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::{process, time::SystemTime};

    use tree_sitter::Language;

    use crate::plugin::LanguagePlugin;

    use super::*;

    fn temporary_directory(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ed-syntax-{name}-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after Unix epoch")
                .as_nanos()
        ))
    }

    #[test]
    fn loads_capture_colors_and_falls_back_to_parent_capture_names() {
        let root = temporary_directory("theme");
        fs::create_dir(&root).expect("create theme directory");
        fs::write(
            root.join("theme.toml"),
            r##"
[captures]
keyword = "#112233"
"function" = "#abcdef"
"##,
        )
        .expect("write theme");

        let (theme, error) = Theme::load(&root);

        assert_eq!(error, None);
        assert_eq!(theme.color("@keyword"), Some(0x112233));
        assert_eq!(theme.color("function.call"), Some(0xabcdef));
        assert_eq!(theme.color("missing"), None);

        fs::remove_dir_all(root).expect("remove theme directory");
    }

    #[test]
    fn highlights_runtime_selected_language_with_unicode_character_offsets() {
        let language = Language::from(tree_sitter_json::LANGUAGE);
        let plugin = LanguagePlugin::for_test(
            "json",
            vec!["json".to_owned()],
            language,
            "(string) @string\n(number) @number\n(true) @constant.builtin",
        );
        let registry = PluginRegistry::from_languages(vec![plugin]);
        let mut manager = SyntaxManager::from_parts(registry, Theme::default());
        let source = "{\"🦀\": 12, \"ok\": true}";

        manager.refresh(Some(Path::new("data.json")), source);

        assert_eq!(manager.active_language_name(), Some("json"));
        let spans = manager.spans_for_line(0).collect::<Vec<_>>();
        assert!(spans.iter().any(|span| {
            span.start_character == 1 && span.end_character == 4 && span.color == 0xce9178
        }));
        assert!(spans.iter().any(|span| span.color == 0xb5cea8));
        assert!(spans.iter().any(|span| span.color == 0x4fc1ff));
    }

    #[test]
    fn invalid_theme_falls_back_without_panicking() {
        let root = temporary_directory("invalid-theme");
        fs::create_dir(&root).expect("create theme directory");
        fs::write(root.join("theme.toml"), "[captures]\nkeyword = \"purple\"")
            .expect("write theme");

        let (theme, error) = Theme::load(&root);

        assert!(error.is_some());
        assert_eq!(theme.color("keyword"), Theme::default().color("keyword"));

        fs::remove_dir_all(root).expect("remove theme directory");
    }
}
