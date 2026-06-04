use rezel_generator::{BuildOptions, compile_grammar};

fn compile(source: &str) -> rezel_generator::CompiledGrammar {
    compile_grammar(source, Some("code_points.grammar"), BuildOptions::default())
        .expect("code-point grammar compiles")
}

#[test]
fn noncharacters_and_eof_have_distinct_transitions() {
    let grammar = compile(
        r#"
@top T { (Ffff | Maximum | End)+ }

@tokens {
  Ffff { "\u{ffff}" }
  Maximum { "\u{10ffff}" }
  End { @eof }
}
"#,
    );
    let table = &grammar.token_table;
    let start = table.states[0];
    let edge_end = usize::from(start.edge_start) + usize::from(start.edge_count);
    let edges = &table.edges[usize::from(start.edge_start)..edge_end];

    assert!(table.eof.iter().any(|transition| transition.state == 0));
    assert!(
        edges
            .iter()
            .any(|edge| edge.from == 0xffff && edge.to == 0x1_0000)
    );
    assert!(
        edges
            .iter()
            .any(|edge| edge.from == 0x10_ffff && edge.to == 0x11_0000)
    );
}

#[test]
fn any_and_inverted_sets_cover_the_code_point_domain() {
    let any = compile(
        r"
@top T { Any }

@tokens {
  Any { _ }
}
",
    );
    let any_start = any.token_table.states[0];
    let any_edge_end = usize::from(any_start.edge_start) + usize::from(any_start.edge_count);
    let any_edges = &any.token_table.edges[usize::from(any_start.edge_start)..any_edge_end];
    assert_eq!(any_edges.len(), 1);
    assert_eq!((any_edges[0].from, any_edges[0].to), (0, 0x11_0000));

    let inverted = compile(
        r"
@top T { NotA }

@tokens {
  NotA { ![a] }
}
",
    );
    let inverted_start = inverted.token_table.states[0];
    let inverted_edge_end =
        usize::from(inverted_start.edge_start) + usize::from(inverted_start.edge_count);
    let inverted_edges =
        &inverted.token_table.edges[usize::from(inverted_start.edge_start)..inverted_edge_end];
    assert_eq!(
        inverted_edges
            .iter()
            .map(|edge| (edge.from, edge.to))
            .collect::<Vec<_>>(),
        vec![(0, u32::from(b'a')), (u32::from(b'a') + 1, 0x11_0000)]
    );
    assert!(
        inverted_edges
            .iter()
            .any(|edge| edge.from <= 0xd800 && edge.to > 0xdfff)
    );
}
