#![forbid(unsafe_code)]

use std::env;
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use rezel_generator::{
    BuildOptions, RustBindings, compile_grammar, emit_rust, emit_terms, emit_typed_syntax,
};

fn main() {
    let arguments = env::args_os().skip(1).collect::<Vec<_>>();
    if let Err(error) = run(&arguments) {
        eprintln!("rezel: {error}");
        std::process::exit(1);
    }
}

fn run(arguments: &[OsString]) -> Result<(), Box<dyn Error>> {
    let Some(command) = arguments.first().and_then(|value| value.to_str()) else {
        return Err(usage().into());
    };
    match command {
        "check" => check(&arguments[1..]),
        "generate" => generate(&arguments[1..]),
        "terms" => terms(&arguments[1..]),
        "-h" | "--help" | "help" => {
            println!("{}", usage());
            Ok(())
        }
        _ => Err(format!("unknown command {command:?}\n\n{}", usage()).into()),
    }
}

fn check(arguments: &[OsString]) -> Result<(), Box<dyn Error>> {
    let grammar_path = one_path_argument("check", arguments)?;
    let grammar = read_grammar(&grammar_path, true)?;
    print_warnings(&grammar);
    println!(
        "{}: {} states, {} terms, {} token words",
        grammar_path.display(),
        grammar.states.len() / rezel_lr::table::StateField::COUNT,
        grammar.max_term + 1,
        grammar.token_data.len(),
    );
    Ok(())
}

fn generate(arguments: &[OsString]) -> Result<(), Box<dyn Error>> {
    let mut grammar_path = None;
    let mut output = None;
    let mut terms_output = None;
    let mut include_names = false;
    let mut bindings = RustBindings::default();
    let mut typed_path = None;
    let mut typed_output = None;
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index]
            .to_str()
            .ok_or("arguments must be valid UTF-8")?;
        match argument {
            "-o" | "--output" => {
                index += 1;
                output = Some(path_value(arguments, index, argument)?);
            }
            "--terms" => {
                index += 1;
                terms_output = Some(path_value(arguments, index, argument)?);
            }
            "--include-names" => include_names = true,
            "--binding" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .and_then(|value| value.to_str())
                    .ok_or("--binding requires SOURCE:NAME=RUST_PATH")?;
                bindings = parse_binding(bindings, value)?;
            }
            "--typed" => {
                index += 1;
                typed_path = Some(path_value(arguments, index, argument)?);
            }
            "--typed-output" => {
                index += 1;
                typed_output = Some(path_value(arguments, index, argument)?);
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown generate option {value:?}").into());
            }
            _ if grammar_path.is_none() => {
                grammar_path = Some(PathBuf::from(&arguments[index]));
            }
            _ => return Err("generate accepts one grammar path".into()),
        }
        index += 1;
    }
    let grammar_path = grammar_path.ok_or("generate requires a grammar path")?;
    let output = output.ok_or("generate requires --output PATH")?;
    if typed_path.is_some() != typed_output.is_some() {
        return Err("generate requires --typed and --typed-output together".into());
    }
    let grammar = read_grammar_with_options(&grammar_path, include_names)?;
    print_warnings(&grammar);
    let generated = emit_rust(&grammar, &bindings)?;
    fs::write(&output, generated.parser)?;
    if let Some(terms_output) = terms_output {
        fs::write(terms_output, generated.terms)?;
    }
    if let Some((typed_path, typed_output)) = typed_path.zip(typed_output) {
        let schema = fs::read_to_string(typed_path)?;
        let typed = emit_typed_syntax(&grammar, &schema)?;
        fs::write(typed_output, typed)?;
    }
    Ok(())
}

fn terms(arguments: &[OsString]) -> Result<(), Box<dyn Error>> {
    let mut grammar_path = None;
    let mut output = None;
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index]
            .to_str()
            .ok_or("arguments must be valid UTF-8")?;
        match argument {
            "-o" | "--output" => {
                index += 1;
                output = Some(path_value(arguments, index, argument)?);
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown terms option {value:?}").into());
            }
            _ if grammar_path.is_none() => {
                grammar_path = Some(PathBuf::from(&arguments[index]));
            }
            _ => return Err("terms accepts one grammar path".into()),
        }
        index += 1;
    }
    let grammar_path = grammar_path.ok_or("terms requires a grammar path")?;
    let grammar = read_grammar(&grammar_path, false)?;
    print_warnings(&grammar);
    let source = emit_terms(&grammar)?;
    if let Some(output) = output {
        fs::write(output, source)?;
    } else {
        print!("{source}");
    }
    Ok(())
}

fn print_warnings(grammar: &rezel_generator::CompiledGrammar) {
    for warning in &grammar.warnings {
        eprintln!("rezel: warning: {warning}");
    }
}

fn one_path_argument(command: &str, arguments: &[OsString]) -> Result<PathBuf, Box<dyn Error>> {
    if arguments.len() != 1 {
        return Err(format!("{command} requires exactly one grammar path").into());
    }
    Ok(PathBuf::from(&arguments[0]))
}

fn path_value(
    arguments: &[OsString],
    index: usize,
    option: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    arguments
        .get(index)
        .map(PathBuf::from)
        .ok_or_else(|| format!("{option} requires a path").into())
}

fn read_grammar(
    path: &Path,
    include_names: bool,
) -> Result<rezel_generator::CompiledGrammar, Box<dyn Error>> {
    read_grammar_with_options(path, include_names)
}

fn read_grammar_with_options(
    path: &Path,
    include_names: bool,
) -> Result<rezel_generator::CompiledGrammar, Box<dyn Error>> {
    let source = fs::read_to_string(path)?;
    let file_name = path.to_string_lossy();
    let grammar = compile_grammar(&source, Some(&file_name), BuildOptions { include_names })?;
    Ok(grammar)
}

fn parse_binding(bindings: RustBindings, value: &str) -> Result<RustBindings, Box<dyn Error>> {
    let (external, rust_path) = value
        .split_once('=')
        .ok_or("--binding requires SOURCE:NAME=RUST_PATH")?;
    let (source, name) = external
        .rsplit_once(':')
        .ok_or("--binding requires SOURCE:NAME=RUST_PATH")?;
    Ok(bindings.with(source, name, rust_path))
}

fn usage() -> &'static str {
    "Usage:\n\
     \x20 rezel check GRAMMAR\n\
     \x20 rezel generate GRAMMAR --output PARSER.rs [--terms TERMS.rs]\n\
     \x20       [--include-names] [--binding SOURCE:NAME=RUST_PATH]...\n\
     \x20       [--typed SCHEMA.toml --typed-output TYPED.rs]\n\
     \x20 rezel terms GRAMMAR [--output TERMS.rs]"
}
