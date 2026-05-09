use std::sync::{Arc, OnceLock};

use rezel_common::{NodeFlags, NodeSet, NodeType, ParseErrorKind};
use rezel_lr::table::SequenceCode;
use rezel_lr::{LRParser, Language};

fn node_set() -> &'static Arc<NodeSet> {
    static NODE_SET: OnceLock<Arc<NodeSet>> = OnceLock::new();
    NODE_SET.get_or_init(|| Arc::new(NodeSet::new(vec![NodeType::new(0, "⚠", NodeFlags::ERROR)])))
}

static TRUNCATED_GOTO: Language = Language {
    states: &[0, 0, 0, 0, 0, 0],
    state_data: &[SequenceCode::End.raw(), SequenceCode::End.raw()],
    goto: &[1, 2, 0],
    token_data: &[],
    tokenizers: &[],
    top_rules: &[],
    max_term: 1,
    min_repeat_term: 1,
    token_precedence: 0,
    node_set,
    context: None,
    dialects: &[],
    dynamic_precedences: &[],
    specializers: &[],
    term_names: &[],
};

#[test]
fn malformed_static_tables_are_rejected_at_construction() {
    let error = LRParser::try_from_language(&TRUNCATED_GOTO).unwrap_err();
    assert_eq!(error.kind(), ParseErrorKind::Configuration);
    assert!(error.message().contains("goto"));
}
