use std::{
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
};

use libloading::Library;
use serde::Deserialize;
use tree_sitter::{Language, Query};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LanguageManifest {
    pub name: String,
    pub extensions: Vec<String>,
    pub grammar: PathBuf,
    pub highlights: PathBuf,
    pub symbol: Option<String>,
}

pub struct LanguagePlugin {
    name: String,
    extensions: Vec<String>,
    query: Query,
    language: Language,
    _library: Option<Library>,
}

impl LanguagePlugin {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn language(&self) -> &Language {
        &self.language
    }

    pub fn query(&self) -> &Query {
        &self.query
    }

    pub fn matches_path(&self, path: &Path) -> bool {
        path.extension()
            .and_then(OsStr::to_str)
            .is_some_and(|extension| {
                self.extensions
                    .iter()
                    .any(|candidate| candidate == extension)
            })
    }

    #[cfg(test)]
    pub fn for_test(
        name: impl Into<String>,
        extensions: Vec<String>,
        language: Language,
        highlights: &str,
    ) -> Self {
        let query = Query::new(&language, highlights).expect("valid test highlight query");
        Self {
            name: name.into(),
            extensions,
            query,
            language,
            _library: None,
        }
    }
}

#[derive(Default)]
pub struct PluginRegistry {
    languages: Vec<LanguagePlugin>,
    errors: Vec<String>,
}

impl PluginRegistry {
    pub fn load(config_root: &Path) -> Self {
        let languages_root = config_root.join("languages");
        let Ok(entries) = fs::read_dir(&languages_root) else {
            return Self::default();
        };
        let mut folders = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        folders.sort();

        let mut registry = Self::default();
        for folder in folders {
            match load_language(&folder) {
                Ok(language) => registry.languages.push(language),
                Err(error) => registry
                    .errors
                    .push(format!("{}: {error}", folder.display())),
            }
        }
        registry
    }

    pub fn language_for_path(&self, path: &Path) -> Option<(usize, &LanguagePlugin)> {
        self.languages
            .iter()
            .enumerate()
            .find(|(_, language)| language.matches_path(path))
    }

    pub fn language(&self, index: usize) -> Option<&LanguagePlugin> {
        self.languages.get(index)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.languages.len()
    }

    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    #[cfg(test)]
    pub fn from_languages(languages: Vec<LanguagePlugin>) -> Self {
        Self {
            languages,
            errors: Vec::new(),
        }
    }
}

fn load_language(folder: &Path) -> Result<LanguagePlugin, String> {
    let manifest_path = folder.join("language.toml");
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("could not read {}: {error}", manifest_path.display()))?;
    let manifest: LanguageManifest = toml::from_str(&manifest_text)
        .map_err(|error| format!("invalid {}: {error}", manifest_path.display()))?;
    validate_manifest(&manifest)?;

    let grammar_path = plugin_path(folder, &manifest.grammar)?;
    let highlights_path = plugin_path(folder, &manifest.highlights)?;
    let highlights = fs::read_to_string(&highlights_path)
        .map_err(|error| format!("could not read {}: {error}", highlights_path.display()))?;
    let symbol = manifest
        .symbol
        .clone()
        .unwrap_or_else(|| format!("tree_sitter_{}", manifest.name.replace(['-', '.'], "_")));

    type GrammarFunction = unsafe extern "C" fn() -> *const tree_sitter::ffi::TSLanguage;
    // SAFETY: Runtime language plugins are trusted native code. The library remains owned by the
    // returned plugin, the symbol has Tree-sitter's generated C signature, and the pointer is
    // checked before constructing Language. ABI compatibility is validated immediately below.
    let (library, language) = unsafe {
        let library = Library::new(&grammar_path)
            .map_err(|error| format!("could not load {}: {error}", grammar_path.display()))?;
        let grammar = library
            .get::<GrammarFunction>(symbol.as_bytes())
            .map_err(|error| format!("missing symbol {symbol}: {error}"))?;
        let pointer = grammar();
        if pointer.is_null() {
            return Err(format!("symbol {symbol} returned a null language"));
        }
        (library, Language::from_raw(pointer))
    };
    let abi = language.abi_version();
    if !(tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION..=tree_sitter::LANGUAGE_VERSION)
        .contains(&abi)
    {
        return Err(format!(
            "unsupported Tree-sitter ABI {abi}; expected {}..={}",
            tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION,
            tree_sitter::LANGUAGE_VERSION
        ));
    }
    let query = Query::new(&language, &highlights)
        .map_err(|error| format!("invalid {}: {error}", highlights_path.display()))?;

    Ok(LanguagePlugin {
        name: manifest.name,
        extensions: manifest.extensions,
        query,
        language,
        _library: Some(library),
    })
}

fn validate_manifest(manifest: &LanguageManifest) -> Result<(), String> {
    if manifest.name.trim().is_empty() {
        return Err("language name must not be empty".to_owned());
    }
    if manifest.extensions.is_empty()
        || manifest
            .extensions
            .iter()
            .any(|extension| extension.is_empty() || extension.starts_with('.'))
    {
        return Err("extensions must contain names without a leading dot".to_owned());
    }
    Ok(())
}

fn plugin_path(folder: &Path, relative: &Path) -> Result<PathBuf, String> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        return Err(format!(
            "plugin paths must stay inside the language folder: {}",
            relative.display()
        ));
    }
    Ok(folder.join(relative))
}

#[cfg(test)]
mod tests {
    use std::{process, time::SystemTime};

    use super::*;

    fn temporary_directory(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ed-plugin-{name}-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after Unix epoch")
                .as_nanos()
        ))
    }

    #[test]
    fn parses_manifests_and_rejects_paths_outside_the_plugin() {
        let manifest: LanguageManifest = toml::from_str(
            r#"
name = "rust"
extensions = ["rs"]
grammar = "rust.so"
highlights = "highlights.scm"
symbol = "tree_sitter_rust"
"#,
        )
        .expect("parse manifest");

        assert_eq!(manifest.name, "rust");
        assert_eq!(manifest.extensions, ["rs"]);
        assert!(validate_manifest(&manifest).is_ok());
        assert!(plugin_path(Path::new("plugin"), Path::new("../grammar.so")).is_err());
    }

    #[test]
    fn registry_reports_broken_plugins_without_aborting_other_startup_work() {
        let root = temporary_directory("broken");
        let language = root.join("languages/rust");
        fs::create_dir_all(&language).expect("create language directory");
        fs::write(
            language.join("language.toml"),
            r#"
name = "rust"
extensions = ["rs"]
grammar = "missing.so"
highlights = "highlights.scm"
"#,
        )
        .expect("write manifest");
        fs::write(language.join("highlights.scm"), "(identifier) @variable")
            .expect("write highlights query");

        let registry = PluginRegistry::load(&root);

        assert_eq!(registry.len(), 0);
        assert_eq!(registry.errors().len(), 1);
        assert!(registry.errors()[0].contains("missing.so"));

        fs::remove_dir_all(root).expect("remove fixture directory");
    }
}
