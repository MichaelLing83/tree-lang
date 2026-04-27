/// Supported input languages for the MVP mappers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    C,
    Cpp,
    Java,
    Python,
    Rust,
}

impl Language {
    /// Detect language from file path extension.
    pub fn detect_from_path(path: &std::path::Path) -> Option<Self> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())?;
        match ext.as_str() {
            "c" | "h" => Some(Self::C),
            "cpp" | "cc" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "h++" => Some(Self::Cpp),
            "java" => Some(Self::Java),
            "py" => Some(Self::Python),
            "rs" => Some(Self::Rust),
            _ => None,
        }
    }

    /// Returns the tree-sitter [`tree_sitter::Language`] for this variant.
    pub fn tree_sitter_language(self) -> tree_sitter::Language {
        use Language::*;
        match self {
            C => tree_sitter_c::LANGUAGE.into(),
            Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Java => tree_sitter_java::LANGUAGE.into(),
            Python => tree_sitter_python::LANGUAGE.into(),
            Rust => tree_sitter_rust::LANGUAGE.into(),
        }
    }

    /// Stable spelling for CLI `-l` and `{language}` in `--print-format` (lowercase).
    pub fn as_cli_name(self) -> &'static str {
        match self {
            Language::C => "c",
            Language::Cpp => "cpp",
            Language::Java => "java",
            Language::Python => "python",
            Language::Rust => "rust",
        }
    }

    /// CLI / config spellings (case-insensitive).
    pub fn parse_cli_name(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "c" => Some(Self::C),
            "cpp" | "c++" | "cxx" => Some(Self::Cpp),
            "java" => Some(Self::Java),
            "python" | "py" => Some(Self::Python),
            "rust" | "rs" => Some(Self::Rust),
            _ => None,
        }
    }

    /// File extensions used when recursively collecting sources under a directory.
    pub fn source_extensions(self) -> &'static [&'static str] {
        use Language::*;
        match self {
            C => &[".c", ".h"],
            Cpp => &[
                ".cpp", ".cc", ".cxx", ".c++", ".hpp", ".hh", ".hxx", ".h++", ".h",
            ],
            Java => &[".java"],
            Python => &[".py"],
            Rust => &[".rs"],
        }
    }
}

impl std::str::FromStr for Language {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_cli_name(s).ok_or_else(|| {
            format!("unknown language {s:?}: expected one of c, cpp, java, python, rust")
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Language;

    #[test]
    fn detects_languages_from_supported_extensions() {
        let cases = [
            ("main.c", Language::C),
            ("include.H", Language::C),
            ("main.cpp", Language::Cpp),
            ("header.HXX", Language::Cpp),
            ("App.java", Language::Java),
            ("script.PY", Language::Python),
            ("lib.rs", Language::Rust),
        ];

        for (path, language) in cases {
            assert_eq!(Language::detect_from_path(Path::new(path)), Some(language));
        }
        assert_eq!(Language::detect_from_path(Path::new("README.md")), None);
        assert_eq!(Language::detect_from_path(Path::new("Makefile")), None);
    }

    #[test]
    fn parses_cli_names_and_reports_errors() {
        assert_eq!(Language::parse_cli_name(" c++ "), Some(Language::Cpp));
        assert_eq!(Language::parse_cli_name("PY"), Some(Language::Python));
        assert_eq!("rs".parse::<Language>().expect("parse rs"), Language::Rust);

        let err = "ruby".parse::<Language>().expect_err("unknown language");
        assert!(err.contains("unknown language"));
    }

    #[test]
    fn exposes_cli_names_and_source_extensions() {
        assert_eq!(Language::C.as_cli_name(), "c");
        assert_eq!(Language::Cpp.as_cli_name(), "cpp");
        assert_eq!(Language::Java.as_cli_name(), "java");
        assert_eq!(Language::Python.as_cli_name(), "python");
        assert_eq!(Language::Rust.as_cli_name(), "rust");

        assert!(Language::Cpp.source_extensions().contains(&".hpp"));
        assert_eq!(Language::Rust.source_extensions(), &[".rs"]);
    }

    #[test]
    fn returns_tree_sitter_languages_for_all_variants() {
        for language in [
            Language::C,
            Language::Cpp,
            Language::Java,
            Language::Python,
            Language::Rust,
        ] {
            let _ = language.tree_sitter_language();
        }
    }
}
