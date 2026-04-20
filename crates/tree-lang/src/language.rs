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
            Cpp => &[".cpp", ".cc", ".cxx", ".c++", ".hpp", ".hh", ".hxx", ".h++", ".h"],
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
            format!(
                "unknown language {s:?}: expected one of c, cpp, java, python, rust"
            )
        })
    }
}
