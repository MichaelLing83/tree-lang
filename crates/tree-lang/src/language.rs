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
}
