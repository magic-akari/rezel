use std::sync::{Arc, OnceLock};

use rezel_common::{NodeFlags, NodeSet, NodeType, ParseErrorKind};
use rezel_lr::table::SequenceCode;
use rezel_lr::{DynamicPrecedence, LRParser, Language, TokenTable, TopRule};

fn node_set() -> &'static Arc<NodeSet> {
    static NODE_SET: OnceLock<Arc<NodeSet>> = OnceLock::new();
    NODE_SET.get_or_init(|| Arc::new(NodeSet::new(vec![NodeType::new(0, "⚠", NodeFlags::ERROR)])))
}

static EMPTY_STATE_DATA: [u16; 2] = [SequenceCode::End.raw(), SequenceCode::Done.raw()];
static TOP_RULES: [TopRule; 1] = [TopRule {
    name: "T",
    state: 0,
    term: 0,
}];
static EMPTY_TOKEN_TABLE: TokenTable = TokenTable::new(&[], &[], &[], &[]);

const fn language_with_data(
    goto: &'static [u16],
    dynamic_precedences: &'static [DynamicPrecedence],
) -> Language {
    Language {
        states: &[0, 0, 0, 0, 0, 0],
        state_data: &EMPTY_STATE_DATA,
        goto,
        token_table: &EMPTY_TOKEN_TABLE,
        tokenizers: &[],
        top_rules: &TOP_RULES,
        max_term: 1,
        min_repeat_term: 1,
        token_precedence: 0,
        node_set,
        context: None,
        dialects: &[],
        dynamic_precedences,
        specializers: &[],
        term_names: &[],
    }
}

const fn language_with_tables(goto: &'static [u16]) -> Language {
    language_with_data(goto, &[])
}

static TRUNCATED_GOTO: Language = language_with_tables(&[1, 2, 0]);
static UNKNOWN_GOTO_TARGET: Language = language_with_tables(&[1, 2, 3, 1, 0]);
static UNKNOWN_GOTO_SOURCE: Language = language_with_tables(&[1, 2, 3, 0, 1]);
static UNKNOWN_DYNAMIC_PRECEDENCE: Language =
    language_with_data(&[1, 1], &[DynamicPrecedence::new(2, 1)]);
static DUPLICATE_DYNAMIC_PRECEDENCE: Language = language_with_data(
    &[1, 1],
    &[DynamicPrecedence::new(0, 1), DynamicPrecedence::new(0, 2)],
);

#[test]
fn malformed_static_tables_are_rejected_at_construction() {
    for (language, expected) in [
        (&TRUNCATED_GOTO, "goto"),
        (&UNKNOWN_GOTO_TARGET, "target"),
        (&UNKNOWN_GOTO_SOURCE, "source"),
        (&UNKNOWN_DYNAMIC_PRECEDENCE, "unknown term"),
        (&DUPLICATE_DYNAMIC_PRECEDENCE, "duplicate term"),
    ] {
        let error = LRParser::try_from_language(language).unwrap_err();
        assert_eq!(error.kind(), ParseErrorKind::Configuration);
        assert!(error.message().contains(expected), "{error}");
    }
}
