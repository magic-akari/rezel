#![forbid(unsafe_code)]

use gungraun::prelude::*;
use gungraun::{Callgrind, EventKind};
use rezel_parser_benchmark::{
    backends::{rezel, tree_sitter},
    datasets::{Language, Tier},
};

#[library_benchmark]
#[bench::go_10kb(rezel::setup(Language::Go, Tier::Kb10))]
#[bench::go_50kb(rezel::setup(Language::Go, Tier::Kb50))]
#[bench::go_100kb(rezel::setup(Language::Go, Tier::Kb100))]
#[bench::go_500kb(rezel::setup(Language::Go, Tier::Kb500))]
#[bench::java_10kb(rezel::setup(Language::Java, Tier::Kb10))]
#[bench::java_50kb(rezel::setup(Language::Java, Tier::Kb50))]
#[bench::java_100kb(rezel::setup(Language::Java, Tier::Kb100))]
#[bench::java_500kb(rezel::setup(Language::Java, Tier::Kb500))]
#[bench::json_10kb(rezel::setup(Language::Json, Tier::Kb10))]
#[bench::json_50kb(rezel::setup(Language::Json, Tier::Kb50))]
#[bench::json_100kb(rezel::setup(Language::Json, Tier::Kb100))]
#[bench::json_500kb(rezel::setup(Language::Json, Tier::Kb500))]
#[bench::kotlin_10kb(rezel::setup(Language::Kotlin, Tier::Kb10))]
#[bench::kotlin_50kb(rezel::setup(Language::Kotlin, Tier::Kb50))]
#[bench::kotlin_100kb(rezel::setup(Language::Kotlin, Tier::Kb100))]
#[bench::kotlin_500kb(rezel::setup(Language::Kotlin, Tier::Kb500))]
#[bench::python_10kb(rezel::setup(Language::Python, Tier::Kb10))]
#[bench::python_50kb(rezel::setup(Language::Python, Tier::Kb50))]
#[bench::python_100kb(rezel::setup(Language::Python, Tier::Kb100))]
#[bench::python_500kb(rezel::setup(Language::Python, Tier::Kb500))]
#[bench::rust_10kb(rezel::setup(Language::Rust, Tier::Kb10))]
#[bench::rust_50kb(rezel::setup(Language::Rust, Tier::Kb50))]
#[bench::rust_100kb(rezel::setup(Language::Rust, Tier::Kb100))]
#[bench::rust_500kb(rezel::setup(Language::Rust, Tier::Kb500))]
#[bench::swift_10kb(rezel::setup(Language::Swift, Tier::Kb10))]
#[bench::swift_50kb(rezel::setup(Language::Swift, Tier::Kb50))]
#[bench::swift_100kb(rezel::setup(Language::Swift, Tier::Kb100))]
#[bench::swift_500kb(rezel::setup(Language::Swift, Tier::Kb500))]
fn parse_rezel(case: rezel::ParseCase) {
    rezel::parse(case);
}

#[library_benchmark]
#[bench::go_10kb(tree_sitter::setup(Language::Go, Tier::Kb10))]
#[bench::go_50kb(tree_sitter::setup(Language::Go, Tier::Kb50))]
#[bench::go_100kb(tree_sitter::setup(Language::Go, Tier::Kb100))]
#[bench::go_500kb(tree_sitter::setup(Language::Go, Tier::Kb500))]
#[bench::java_10kb(tree_sitter::setup(Language::Java, Tier::Kb10))]
#[bench::java_50kb(tree_sitter::setup(Language::Java, Tier::Kb50))]
#[bench::java_100kb(tree_sitter::setup(Language::Java, Tier::Kb100))]
#[bench::java_500kb(tree_sitter::setup(Language::Java, Tier::Kb500))]
#[bench::json_10kb(tree_sitter::setup(Language::Json, Tier::Kb10))]
#[bench::json_50kb(tree_sitter::setup(Language::Json, Tier::Kb50))]
#[bench::json_100kb(tree_sitter::setup(Language::Json, Tier::Kb100))]
#[bench::json_500kb(tree_sitter::setup(Language::Json, Tier::Kb500))]
#[bench::kotlin_10kb(tree_sitter::setup(Language::Kotlin, Tier::Kb10))]
#[bench::kotlin_50kb(tree_sitter::setup(Language::Kotlin, Tier::Kb50))]
#[bench::kotlin_100kb(tree_sitter::setup(Language::Kotlin, Tier::Kb100))]
#[bench::kotlin_500kb(tree_sitter::setup(Language::Kotlin, Tier::Kb500))]
#[bench::python_10kb(tree_sitter::setup(Language::Python, Tier::Kb10))]
#[bench::python_50kb(tree_sitter::setup(Language::Python, Tier::Kb50))]
#[bench::python_100kb(tree_sitter::setup(Language::Python, Tier::Kb100))]
#[bench::python_500kb(tree_sitter::setup(Language::Python, Tier::Kb500))]
#[bench::rust_10kb(tree_sitter::setup(Language::Rust, Tier::Kb10))]
#[bench::rust_50kb(tree_sitter::setup(Language::Rust, Tier::Kb50))]
#[bench::rust_100kb(tree_sitter::setup(Language::Rust, Tier::Kb100))]
#[bench::rust_500kb(tree_sitter::setup(Language::Rust, Tier::Kb500))]
#[bench::swift_10kb(tree_sitter::setup(Language::Swift, Tier::Kb10))]
#[bench::swift_50kb(tree_sitter::setup(Language::Swift, Tier::Kb50))]
#[bench::swift_100kb(tree_sitter::setup(Language::Swift, Tier::Kb100))]
#[bench::swift_500kb(tree_sitter::setup(Language::Swift, Tier::Kb500))]
fn parse_tree_sitter(case: tree_sitter::ParseCase) {
    tree_sitter::parse(case);
}

library_benchmark_group!(name = parser, benchmarks = [parse_rezel, parse_tree_sitter]);

main!(
    config = LibraryBenchmarkConfig::default()
        .tool(Callgrind::with_args(["--branch-sim=no", "--cache-sim=no"]).format([EventKind::Ir]),),
    library_benchmark_groups = [parser]
);
