#![forbid(unsafe_code)]

use rezel_common::SyntaxNode;
use rezel_lr::ParseLimits;

const SUITES: &[(&str, &str)] = &[
    ("class", include_str!("upstream/lezer-php/test/class.txt")),
    (
        "declarations",
        include_str!("upstream/lezer-php/test/declarations.txt"),
    ),
    (
        "expressions",
        include_str!("upstream/lezer-php/test/expressions.txt"),
    ),
    (
        "interpolation",
        include_str!("upstream/lezer-php/test/interpolation.txt"),
    ),
    (
        "literals",
        include_str!("upstream/lezer-php/test/literals.txt"),
    ),
    (
        "statements",
        include_str!("upstream/lezer-php/test/statements.txt"),
    ),
    ("string", include_str!("upstream/lezer-php/test/string.txt")),
    ("types", include_str!("upstream/lezer-php/test/types.txt")),
];

#[test]
fn strict_positive_csts_match_lezer_php_1_0_5() {
    let parser = rezel_lang_php::parser()
        .with_strict(true)
        .with_limits(ParseLimits {
            max_stacks: 2,
            max_recovery_actions: 0,
            ..ParseLimits::default()
        });
    let mut count = 0;
    let mut intentional_differences = 0;
    for &(suite, source) in SUITES {
        for case in parse_cases(source) {
            count += 1;
            let tree = parser.parse(case.source).unwrap_or_else(|error| {
                let recovered = rezel_lang_php::parser().parse(case.source).unwrap();
                panic!(
                    "{suite}/{}: positive fixture failed: {error}\n{recovered}",
                    case.name
                )
            });
            if suite == "interpolation"
                && matches!(case.name, "short open tag: On" | "short open tag: Off")
            {
                intentional_differences += 1;
                continue;
            }
            match_tree(&tree.top_node(), case.tree)
                .unwrap_or_else(|error| panic!("{suite}/{}: {error}", case.name));
        }
    }
    assert_eq!(count, 91, "the pinned upstream fixture count changed");
    assert_eq!(
        intentional_differences, 2,
        "short tags remain the only inherited CST difference"
    );
}

struct Case<'a> {
    name: &'a str,
    source: &'a str,
    tree: &'a str,
}

fn parse_cases(source: &str) -> Vec<Case<'_>> {
    let mut cases = Vec::new();
    let mut position = 0;
    while position < source.len() {
        while source[position..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
        {
            position += source[position..]
                .chars()
                .next()
                .expect("whitespace character exists")
                .len_utf8();
        }
        if position == source.len() {
            break;
        }
        let header = source[position..]
            .strip_prefix("# ")
            .expect("upstream fixture case starts with a heading");
        let header_length = header
            .find('\n')
            .expect("case heading is followed by a body");
        let name = header[..header_length].trim_end_matches('\r');
        let input_start = position + 2 + header_length + 1;
        let separator = source[input_start..]
            .find("\n==>\n")
            .map(|offset| input_start + offset)
            .expect("case contains one expected-tree separator");
        let expected_start = separator + "\n==>\n".len();
        let next_header = source[expected_start..]
            .find("\n# ")
            .map(|offset| expected_start + offset + 1);
        let expected_end = next_header.unwrap_or(source.len());
        cases.push(Case {
            name,
            source: source[input_start..separator].trim(),
            tree: source[expected_start..expected_end].trim(),
        });
        position = expected_end;
    }
    cases
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Token {
    Name(String),
    LeftParen,
    RightParen,
    Comma,
    Wildcard,
    Eof,
}

#[derive(Clone, Debug)]
struct TestSpec {
    name: String,
    children: Vec<Self>,
    wildcard: bool,
}

struct SpecParser<'a> {
    source: &'a str,
    position: usize,
    token: Token,
}

impl<'a> SpecParser<'a> {
    fn parse(source: &'a str) -> Result<Vec<TestSpec>, String> {
        let mut parser = Self {
            source,
            position: 0,
            token: Token::Eof,
        };
        parser.next()?;
        let result = parser.parse_sequence()?;
        if parser.token != Token::Eof {
            return Err(parser.error());
        }
        Ok(result)
    }

    fn parse_sequence(&mut self) -> Result<Vec<TestSpec>, String> {
        let mut sequence = Vec::new();
        while !matches!(self.token, Token::Eof | Token::RightParen) {
            sequence.push(self.parse_spec()?);
            if self.token == Token::Comma {
                self.next()?;
            }
        }
        Ok(sequence)
    }

    fn parse_spec(&mut self) -> Result<TestSpec, String> {
        let Token::Name(name) = &self.token else {
            return Err(self.error());
        };
        let name = name.clone();
        self.next()?;
        let mut children = Vec::new();
        let mut wildcard = false;
        if self.token == Token::LeftParen {
            self.next()?;
            children = self.parse_sequence()?;
            if self.token != Token::RightParen {
                return Err(self.error());
            }
            self.next()?;
        } else if self.token == Token::Wildcard {
            wildcard = true;
            self.next()?;
        }
        Ok(TestSpec {
            name,
            children,
            wildcard,
        })
    }

    fn next(&mut self) -> Result<(), String> {
        while let Some(character) = self.source[self.position..].chars().next() {
            if !character.is_whitespace() {
                break;
            }
            self.position += character.len_utf8();
        }
        if self.position == self.source.len() {
            self.token = Token::Eof;
            return Ok(());
        }
        if self.source[self.position..].starts_with("(...)") {
            self.position += "(...)".len();
            self.token = Token::Wildcard;
            return Ok(());
        }

        let character = self.source[self.position..]
            .chars()
            .next()
            .expect("position remains within the expected tree");
        self.position += character.len_utf8();
        self.token = match character {
            '(' => Token::LeftParen,
            ')' => Token::RightParen,
            ',' => Token::Comma,
            '"' => Token::Name(self.read_quoted_name()?),
            value if is_name_character(value) => {
                let start = self.position - value.len_utf8();
                while let Some(next) = self.source[self.position..].chars().next() {
                    if !is_name_character(next) {
                        break;
                    }
                    self.position += next.len_utf8();
                }
                Token::Name(self.source[start..self.position].to_owned())
            }
            _ => return Err(self.error()),
        };
        Ok(())
    }

    fn read_quoted_name(&mut self) -> Result<String, String> {
        let start = self.position - 1;
        let mut escaped = false;
        while let Some(character) = self.source[self.position..].chars().next() {
            self.position += character.len_utf8();
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                return serde_json::from_str(&self.source[start..self.position])
                    .map_err(|_| self.error());
            }
        }
        Err(self.error())
    }

    fn error(&self) -> String {
        format!("invalid expected tree: {}", self.source)
    }
}

fn is_name_character(character: char) -> bool {
    !matches!(character, '(' | ')' | ',' | '"') && !character.is_whitespace()
}

fn match_tree(root: &SyntaxNode, expected: &str) -> Result<(), String> {
    let specs = SpecParser::parse(expected)?;
    match_sequence(std::slice::from_ref(root), &specs, "tree")
}

fn match_sequence(
    actual: &[SyntaxNode],
    expected: &[TestSpec],
    parent: &str,
) -> Result<(), String> {
    let mut expected_index = 0;
    for node in actual {
        let next = expected.get(expected_index);
        if next.is_some_and(|spec| spec.name == node.name().as_ref()) {
            let spec = &expected[expected_index];
            if !spec.wildcard {
                match_sequence(&named_children(node), &spec.children, &spec.name)?;
            }
            expected_index += 1;
        } else if !default_ignore(node) {
            let after = next.map_or("end of sequence", |spec| spec.name.as_str());
            return Err(format!("expected {after} in {parent}, got {}", node.name()));
        }
    }
    if expected_index == expected.len() {
        return Ok(());
    }
    let remaining = expected[expected_index..]
        .iter()
        .map(|spec| spec.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!("unexpected end of {parent}; expected {remaining}"))
}

fn named_children(node: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut result = Vec::new();
    for child in node.children() {
        if child.name().is_empty() {
            result.extend(named_children(&child));
        } else {
            result.push(child);
        }
    }
    result
}

fn default_ignore(node: &SyntaxNode) -> bool {
    node.name()
        .chars()
        .any(|character| !(character.is_ascii_alphanumeric() || character == '_'))
}
