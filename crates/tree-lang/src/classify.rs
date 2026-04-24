use tree_lang_core::{BranchKind, LoopKind, MappedNode, Span, UnifiedKind};

use crate::Language;

/// Maps a tree-sitter node `kind` string to a unified [`MappedNode`], if this MVP covers it.
pub(crate) fn classify(language: Language, kind: &str, span: Span) -> Option<MappedNode> {
    use Language::*;
    use BranchKind::*;
    use LoopKind::*;
    use UnifiedKind::*;

    let u = match language {
        Python => match kind {
            "function_definition" => FunctionDefinition,
            "for_statement" => Loop(For),
            "while_statement" => Loop(While),
            "if_statement" => Branch(If),
            "match_statement" => Branch(Match),
            _ => return None,
        },
        Java => match kind {
            "method_declaration" => FunctionDefinition,
            "for_statement" => Loop(For),
            "enhanced_for_statement" => Loop(ForEach),
            "while_statement" => Loop(While),
            "do_statement" => Loop(DoWhile),
            "if_statement" => Branch(If),
            "switch_statement" => Branch(Switch),
            _ => return None,
        },
        Rust => match kind {
            "function_item" => FunctionDefinition,
            "for_expression" => Loop(For),
            "while_expression" => Loop(While),
            "loop_expression" => Loop(Infinite),
            "if_expression" => Branch(If),
            "match_expression" => Branch(Match),
            _ => return None,
        },
        C => match kind {
            "function_definition" => FunctionDefinition,
            "for_statement" => Loop(For),
            "while_statement" => Loop(While),
            "do_statement" => Loop(DoWhile),
            "if_statement" => Branch(If),
            "switch_statement" => Branch(Switch),
            _ => return None,
        },
        Cpp => match kind {
            "function_definition" => FunctionDefinition,
            "for_statement" => Loop(For),
            "for_range_loop" => Loop(ForEach),
            "while_statement" => Loop(While),
            "do_statement" => Loop(DoWhile),
            "if_statement" => Branch(If),
            "switch_statement" => Branch(Switch),
            _ => return None,
        },
    };

    Some(MappedNode::new(u, span))
}
