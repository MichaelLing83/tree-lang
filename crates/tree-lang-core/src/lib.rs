//! Language-agnostic syntax kinds for unified analysis.

/// Byte offsets into the UTF-8 source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub start_byte: usize,
    pub end_byte: usize,
}

impl Span {
    #[inline]
    pub fn new(start_byte: usize, end_byte: usize) -> Self {
        Self {
            start_byte,
            end_byte,
        }
    }
}

/// Distinguishes common loop forms after lowering to a unified shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoopKind {
    For,
    ForEach,
    While,
    DoWhile,
    /// Rust `loop { ... }`
    Infinite,
}

/// Cross-language syntax matched in the MVP (functions, loops, conditionals).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnifiedKind {
    FunctionDefinition,
    Loop(LoopKind),
    If,
}

/// One tree-sitter subtree classified as a unified construct.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MappedNode {
    pub kind: UnifiedKind,
    pub span: Span,
}

impl MappedNode {
    #[inline]
    pub fn new(kind: UnifiedKind, span: Span) -> Self {
        Self { kind, span }
    }
}
