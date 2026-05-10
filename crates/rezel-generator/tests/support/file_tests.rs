use rezel_common::{NodeType, SyntaxNode, Tree};
use rezel_lr::LRParser;
use serde::Deserialize;

use super::externals;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestConfig {
    pub top: Option<String>,
    pub dialect: Option<String>,
    pub strict: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct FileTest {
    pub name: String,
    pub text: String,
    pub expected: String,
    pub config_source: Option<String>,
    pub config: TestConfig,
    pub strict: bool,
}

impl FileTest {
    pub fn run(&self, parser: LRParser) -> Result<(), String> {
        let strict = self.config.strict.unwrap_or(self.strict);
        let mut parser = parser.with_strict(strict);
        if let Some(top) = &self.config.top {
            parser = parser
                .with_top(top)
                .map_err(|error| format!("failed to select top {top:?}: {error}"))?;
        }
        if let Some(dialect) = &self.config.dialect {
            parser = parser
                .with_dialect(dialect)
                .map_err(|error| format!("failed to select dialect {dialect:?}: {error}"))?;
        }
        let tree = parser
            .parse(&self.text)
            .map_err(|error| format!("parse failed: {error}"))?;
        test_tree(&tree, &self.expected)
    }
}

pub fn file_tests(source: &str, file_name: &str) -> Result<Vec<FileTest>, String> {
    let mut tests = Vec::new();
    let mut position = 0;
    while position < source.len() {
        position = skip_whitespace(source, position);
        if position == source.len() {
            break;
        }
        if source.as_bytes()[position] != b'#' {
            return Err(format_error(source, file_name, position));
        }

        let header_start = position + 1;
        let header_end = line_end(source, header_start);
        let header = source[header_start..header_end].trim_matches([' ', '\t', '\r']);
        position = after_line_end(source, header_end);

        let Some((separator, separator_end)) = find_separator(source, position) else {
            return Err(format_error(source, file_name, position));
        };
        let text = source[position..separator].trim().to_owned();
        position = separator_end;

        let next_header = find_next_header(source, position);
        let expected_end = next_header.unwrap_or(source.len());
        let expected = source[position..expected_end].trim().to_owned();
        position = expected_end;

        let (name, config_source, config) = parse_header(header)
            .map_err(|message| format!("{message} in {file_name} header {header:?}"))?;
        let strict = !expected.contains('⚠') && !expected.contains("...");
        tests.push(FileTest {
            name,
            text,
            expected,
            config_source,
            config,
            strict,
        });
    }
    if tests.is_empty() && !source.trim().is_empty() {
        return Err(format_error(source, file_name, 0));
    }
    Ok(tests)
}

fn skip_whitespace(source: &str, mut position: usize) -> usize {
    while let Some(character) = source[position..].chars().next() {
        if !character.is_whitespace() {
            break;
        }
        position += character.len_utf8();
        if position == source.len() {
            break;
        }
    }
    position
}

fn line_end(source: &str, position: usize) -> usize {
    source[position..]
        .find(['\r', '\n'])
        .map_or(source.len(), |offset| position + offset)
}

fn after_line_end(source: &str, mut position: usize) -> usize {
    if source[position..].starts_with("\r\n") {
        position += 2;
    } else if position < source.len() {
        position += 1;
    }
    position
}

fn find_next_header(source: &str, position: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut line_start = position;
    while line_start < source.len() {
        let end = line_end(source, line_start);
        if bytes.get(line_start) == Some(&b'#') {
            return Some(line_start);
        }
        if end == source.len() {
            return None;
        }
        line_start = after_line_end(source, end);
    }
    None
}

fn find_separator(source: &str, position: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut cursor = position;
    while cursor + 2 < bytes.len() {
        if bytes[cursor] != b'=' || bytes[cursor + 1] != b'=' {
            cursor += 1;
            continue;
        }
        let mut end = cursor + 2;
        while bytes.get(end) == Some(&b'=') {
            end += 1;
        }
        if bytes.get(end) == Some(&b'>') {
            return Some((cursor, end + 1));
        }
        cursor = end;
    }
    None
}

fn parse_header(header: &str) -> Result<(String, Option<String>, TestConfig), String> {
    let Some(config_start) = header.rfind('{') else {
        return Ok((header.to_owned(), None, TestConfig::default()));
    };
    if !header.ends_with('}') {
        return Ok((header.to_owned(), None, TestConfig::default()));
    }
    let config_source = &header[config_start..];
    let config = serde_json::from_str(config_source)
        .map_err(|error| format!("invalid test configuration: {error}"))?;
    Ok((
        header[..config_start].trim_end().to_owned(),
        Some(config_source.to_owned()),
        config,
    ))
}

fn format_error(source: &str, file_name: &str, position: usize) -> String {
    let context_end = source[position..]
        .find('\n')
        .map_or(source.len(), |offset| position + offset);
    let context = source[position..context_end]
        .lines()
        .map(|line| format!("  | {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("Unexpected file format in {file_name} around\n\n{context}")
}

#[derive(Clone, Debug)]
struct TestSpec {
    name: String,
    properties: Vec<ExpectedProperty>,
    children: Vec<Self>,
    wildcard: bool,
}

impl TestSpec {
    fn matches(&self, node_type: &NodeType) -> bool {
        self.name == node_type.name()
            && self
                .properties
                .iter()
                .all(|property| property.matches(node_type))
    }
}

#[derive(Clone, Debug)]
enum ExpectedProperty {
    ClosedBy(Vec<String>),
    OpenedBy(Vec<String>),
    Group(Vec<String>),
    Isolate(String),
    Tag(String),
}

impl ExpectedProperty {
    fn parse(name: &str, value: &str) -> Result<Self, String> {
        match name {
            "closedBy" => Ok(Self::ClosedBy(split_names(value))),
            "openedBy" => Ok(Self::OpenedBy(split_names(value))),
            "group" => Ok(Self::Group(split_names(value))),
            "isolate" if matches!(value, "" | "rtl" | "ltr" | "auto") => {
                Ok(Self::Isolate(if value.is_empty() {
                    "auto".to_owned()
                } else {
                    value.to_owned()
                }))
            }
            "tag" => Ok(Self::Tag(value.to_owned())),
            _ => Err(format!("unknown or invalid node property {name:?}")),
        }
    }

    fn matches(&self, node_type: &NodeType) -> bool {
        match self {
            Self::ClosedBy(expected) => {
                node_type.prop(rezel_common::closed_by_prop()) == Some(expected)
            }
            Self::OpenedBy(expected) => {
                node_type.prop(rezel_common::opened_by_prop()) == Some(expected)
            }
            Self::Group(expected) => node_type.prop(rezel_common::group_prop()) == Some(expected),
            Self::Isolate(expected) => {
                node_type.prop(rezel_common::isolate_prop()) == Some(expected)
            }
            Self::Tag(expected) => node_type.prop(externals::tag()) == Some(expected),
        }
    }
}

fn split_names(value: &str) -> Vec<String> {
    value.split_ascii_whitespace().map(str::to_owned).collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Token {
    Name(String),
    LeftBracket,
    RightBracket,
    LeftParen,
    RightParen,
    Comma,
    Equals,
    Wildcard,
    Eof,
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
        let mut properties = Vec::new();
        if self.token == Token::LeftBracket {
            self.next()?;
            while self.token != Token::RightBracket {
                let Token::Name(property_name) = &self.token else {
                    return Err(self.error());
                };
                let property_name = property_name.clone();
                self.next()?;
                let value = if self.token == Token::Equals {
                    self.next()?;
                    let Token::Name(property_value) = &self.token else {
                        return Err(self.error());
                    };
                    let property_value = property_value.clone();
                    self.next()?;
                    property_value
                } else {
                    String::new()
                };
                properties.push(ExpectedProperty::parse(&property_name, &value)?);
            }
            self.next()?;
        }

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
            properties,
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
            .expect("position is within source");
        self.position += character.len_utf8();
        self.token = match character {
            '[' => Token::LeftBracket,
            ']' => Token::RightBracket,
            '(' => Token::LeftParen,
            ')' => Token::RightParen,
            ',' => Token::Comma,
            '=' => Token::Equals,
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
        format!("Invalid test spec: {}", self.source)
    }
}

fn is_name_character(character: char) -> bool {
    !matches!(character, '(' | ')' | '[' | ']' | ',' | '=' | '"') && !character.is_whitespace()
}

pub fn test_tree(tree: &Tree, expected: &str) -> Result<(), String> {
    let specs = SpecParser::parse(expected)?;
    let root = tree.top_node();
    match_sequence(std::slice::from_ref(&root), &specs, tree, "tree")
}

fn match_sequence(
    actual: &[SyntaxNode],
    expected: &[TestSpec],
    tree: &Tree,
    parent: &str,
) -> Result<(), String> {
    let mut expected_index = 0;
    for node in actual {
        let next = expected.get(expected_index);
        if next.is_some_and(|spec| spec.matches(&node.node_type())) {
            let spec = &expected[expected_index];
            if !spec.wildcard {
                let children = named_children(node);
                match_sequence(&children, &spec.children, tree, &spec.name)?;
            }
            expected_index += 1;
        } else if !default_ignore(&node.node_type()) {
            let after = next.map_or_else(
                || format!("end of {parent}"),
                |spec| {
                    if parent == "tree" {
                        spec.name.clone()
                    } else {
                        format!("{} in {parent}", spec.name)
                    }
                },
            );
            return Err(format!(
                "Expected {after}, got {} at {}\n{tree}",
                node.name(),
                u32::from(node.to())
            ));
        }
    }
    if expected_index != expected.len() {
        let remaining = expected[expected_index..]
            .iter()
            .map(|spec| spec.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "Unexpected end of {parent}. Expected {remaining}\n{tree}"
        ));
    }
    Ok(())
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

fn default_ignore(node_type: &NodeType) -> bool {
    node_type
        .name()
        .chars()
        .any(|character| !(character.is_ascii_alphanumeric() || character == '_'))
}
