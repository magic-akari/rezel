use crate::GeneratorError;
use crate::node::{
    CharClass, ConflictMarker, ConflictMarkerKind, ContextDeclaration, Expression, ExpressionKind,
    ExternalPropDeclaration, ExternalPropSourceDeclaration, ExternalSpecializeDeclaration,
    ExternalTokenDeclaration, GrammarDeclaration, Identifier, LiteralDeclaration,
    LiteralExpression, LocalTokenDeclaration, NameExpression, NamedNode, PrecDeclaration, PrecItem,
    PrecKind, Prop, PropPart, RepeatKind, RuleDeclaration, ScopedSkipDeclaration, SetExpression,
    SpecializeExpression, SpecializeKind, TokenConflictDeclaration, TokenDeclaration,
    TokenPrecDeclaration, TokenReference,
};

#[derive(Clone, Debug, Eq, PartialEq)]
enum TokenKind {
    Eof,
    String(String),
    At(String),
    Set { source: String, inverted: bool },
    Identifier(String),
    Punctuation(char),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Token {
    kind: TokenKind,
    start: usize,
    end: usize,
}

/// Parse one Lezer grammar source.
///
/// # Errors
///
/// Returns the first source error using a UTF-8 byte position.
pub fn parse_grammar(
    source: &str,
    file_name: Option<&str>,
) -> Result<GrammarDeclaration, GeneratorError> {
    Input::new(source, file_name)?.parse()
}

fn empty_grammar(start: usize) -> GrammarDeclaration {
    GrammarDeclaration {
        start,
        rules: Vec::new(),
        top_rules: Vec::new(),
        tokens: None,
        local_tokens: Vec::new(),
        context: None,
        external_tokens: Vec::new(),
        external_specializers: Vec::new(),
        external_prop_sources: Vec::new(),
        precedences: None,
        main_skip: None,
        scoped_skip: Vec::new(),
        dialects: Vec::new(),
        external_props: Vec::new(),
        auto_delimiters: false,
    }
}

struct Input<'a> {
    source: &'a str,
    file_name: Option<&'a str>,
    token: Token,
}

impl<'a> Input<'a> {
    fn new(source: &'a str, file_name: Option<&'a str>) -> Result<Self, GeneratorError> {
        let mut input = Self {
            source,
            file_name,
            token: Token {
                kind: TokenKind::Eof,
                start: 0,
                end: 0,
            },
        };
        input.next()?;
        Ok(input)
    }

    fn parse(mut self) -> Result<GrammarDeclaration, GeneratorError> {
        let start = self.token.start;
        let mut grammar = empty_grammar(start);
        let mut saw_top = false;
        while !self.at_eof() {
            self.parse_declaration(&mut grammar, &mut saw_top)?;
        }
        if !saw_top {
            return Err(self.error("Missing @top declaration", self.source.len()));
        }
        Ok(grammar)
    }

    fn parse_declaration(
        &mut self,
        grammar: &mut GrammarDeclaration,
        saw_top: &mut bool,
    ) -> Result<(), GeneratorError> {
        let start = self.token.start;
        if self.eat_at("top")? {
            if !matches!(self.token.kind, TokenKind::Identifier(_)) {
                return Err(self.error("Top rules must have a name", self.token.start));
            }
            let name = self.parse_identifier()?;
            grammar.top_rules.push(self.parse_rule(Some(name))?);
            *saw_top = true;
        } else if self.at_at("tokens") {
            if grammar.tokens.is_some() {
                return Err(self.error("Multiple @tokens declarations", self.token.start));
            }
            grammar.tokens = Some(self.parse_tokens()?);
        } else if self.eat_at("local")? {
            self.expect_identifier_value("tokens")?;
            grammar.local_tokens.push(self.parse_local_tokens(start)?);
        } else if self.eat_at("context")? {
            self.parse_context_declaration(grammar, start)?;
        } else if self.eat_at("external")? {
            self.parse_external_declaration(grammar, start)?;
        } else if self.eat_at("dialects")? {
            grammar.dialects = self.parse_dialects()?;
        } else if self.at_at("precedence") {
            if grammar.precedences.is_some() {
                return Err(self.error("Multiple precedence declarations", self.token.start));
            }
            grammar.precedences = Some(self.parse_precedence()?);
        } else if self.eat_at("detectDelim")? {
            grammar.auto_delimiters = true;
        } else if self.eat_at("skip")? {
            self.parse_skip_declaration(grammar, saw_top, start)?;
        } else {
            grammar.rules.push(self.parse_rule(None)?);
        }
        Ok(())
    }

    fn parse_context_declaration(
        &mut self,
        grammar: &mut GrammarDeclaration,
        start: usize,
    ) -> Result<(), GeneratorError> {
        if grammar.context.is_some() {
            return Err(self.error("Multiple @context declarations", start));
        }
        let id = self.parse_identifier()?;
        self.expect_identifier_value("from")?;
        let source = self.expect_string()?;
        grammar.context = Some(ContextDeclaration { start, id, source });
        Ok(())
    }

    fn parse_external_declaration(
        &mut self,
        grammar: &mut GrammarDeclaration,
        start: usize,
    ) -> Result<(), GeneratorError> {
        if self.eat_identifier_value("tokens")? {
            grammar
                .external_tokens
                .push(self.parse_external_tokens(start)?);
        } else if self.eat_identifier_value("prop")? {
            grammar
                .external_props
                .push(self.parse_external_prop(start)?);
        } else if self.eat_identifier_value("extend")? {
            let declaration = self.parse_external_specializer(start, SpecializeKind::Extend)?;
            grammar.external_specializers.push(declaration);
        } else if self.eat_identifier_value("specialize")? {
            let declaration = self.parse_external_specializer(start, SpecializeKind::Specialize)?;
            grammar.external_specializers.push(declaration);
        } else if self.eat_identifier_value("propSource")? {
            let declaration = self.parse_external_prop_source(start)?;
            grammar.external_prop_sources.push(declaration);
        } else {
            return Err(self.unexpected());
        }
        Ok(())
    }

    fn parse_dialects(&mut self) -> Result<Vec<Identifier>, GeneratorError> {
        self.expect_punctuation('{')?;
        let mut dialects = Vec::new();
        let mut first = true;
        while !self.eat_punctuation('}')? {
            if !first {
                let _ = self.eat_punctuation(',')?;
            }
            first = false;
            dialects.push(self.parse_identifier()?);
        }
        Ok(dialects)
    }

    fn parse_skip_declaration(
        &mut self,
        grammar: &mut GrammarDeclaration,
        saw_top: &mut bool,
        start: usize,
    ) -> Result<(), GeneratorError> {
        let expression = self.parse_braced_expression()?;
        if !self.at_punctuation('{') {
            if grammar.main_skip.is_some() {
                return Err(self.error("Multiple top-level skip declarations", start));
            }
            grammar.main_skip = Some(expression);
            return Ok(());
        }
        self.next()?;
        let mut rules = Vec::new();
        let mut top_rules = Vec::new();
        while !self.eat_punctuation('}')? {
            if self.eat_at("top")? {
                let name = self.parse_identifier()?;
                top_rules.push(self.parse_rule(Some(name))?);
                *saw_top = true;
            } else {
                rules.push(self.parse_rule(None)?);
            }
        }
        grammar.scoped_skip.push(ScopedSkipDeclaration {
            expression,
            top_rules,
            rules,
        });
        Ok(())
    }

    fn parse_rule(&mut self, named: Option<Identifier>) -> Result<RuleDeclaration, GeneratorError> {
        let id = match named {
            Some(identifier) => identifier,
            None => self.parse_identifier()?,
        };
        let start = id.start;
        let props = self.parse_props()?;
        let mut params = Vec::new();
        if self.eat_punctuation('<')? {
            while !self.eat_punctuation('>')? {
                if !params.is_empty() {
                    self.expect_punctuation(',')?;
                }
                params.push(self.parse_identifier()?);
            }
        }
        let expression = self.parse_braced_expression()?;
        Ok(RuleDeclaration {
            start,
            id,
            props,
            params,
            expression,
        })
    }

    fn parse_props(&mut self) -> Result<Vec<Prop>, GeneratorError> {
        if !self.at_punctuation('[') {
            return Ok(Vec::new());
        }
        self.next()?;
        let mut props = Vec::new();
        while !self.eat_punctuation(']')? {
            if !props.is_empty() {
                self.expect_punctuation(',')?;
            }
            props.push(self.parse_prop()?);
        }
        Ok(props)
    }

    fn parse_prop(&mut self) -> Result<Prop, GeneratorError> {
        let start = self.token.start;
        let (at, name) = match &self.token.kind {
            TokenKind::At(name) => (true, name.clone()),
            TokenKind::Identifier(name) => (false, name.clone()),
            _ => return Err(self.unexpected()),
        };
        self.next()?;
        let mut value = Vec::new();
        if self.eat_punctuation('=')? {
            loop {
                match &self.token.kind {
                    TokenKind::String(text) | TokenKind::Identifier(text) => {
                        value.push(PropPart {
                            start: self.token.start,
                            value: Some(text.clone()),
                            name: None,
                        });
                        self.next()?;
                    }
                    TokenKind::Punctuation('.') => {
                        value.push(PropPart {
                            start: self.token.start,
                            value: Some(".".to_owned()),
                            name: None,
                        });
                        self.next()?;
                    }
                    TokenKind::Punctuation('{') => {
                        let part_start = self.token.start;
                        self.next()?;
                        let name = self.parse_identifier()?;
                        self.expect_punctuation('}')?;
                        value.push(PropPart {
                            start: part_start,
                            value: None,
                            name: Some(name.name),
                        });
                    }
                    _ => break,
                }
            }
        }
        Ok(Prop {
            start,
            at,
            name,
            value,
        })
    }

    fn parse_braced_expression(&mut self) -> Result<Expression, GeneratorError> {
        self.expect_punctuation('{')?;
        let expression = self.parse_expression_choice()?;
        self.expect_punctuation('}')?;
        Ok(expression)
    }

    fn parse_expression_inner(&mut self) -> Result<Expression, GeneratorError> {
        let start = self.token.start;
        if self.eat_punctuation('(')? {
            return self.parse_parenthesized_expression(start);
        }
        if let TokenKind::String(value) = &self.token.kind {
            let value = value.clone();
            return self.parse_literal_expression(start, value);
        }
        if self.eat_identifier_value("_")? {
            return Ok(Expression::new(start, ExpressionKind::Any));
        }
        if let TokenKind::Set { source, inverted } = &self.token.kind {
            let source = source.clone();
            let inverted = *inverted;
            return self.parse_set_expression(start, &source, inverted);
        }
        if self.at_at("specialize") || self.at_at("extend") {
            return self.parse_specialize_expression(start);
        }
        if let TokenKind::At(name) = &self.token.kind
            && let Some(class) = CharClass::from_name(name)
        {
            self.next()?;
            return Ok(Expression::new(start, ExpressionKind::CharClass(class)));
        }
        if self.at_punctuation('[') {
            let id = Identifier {
                start,
                name: "_anon".to_owned(),
            };
            return self.parse_inline_rule_expression(start, id);
        }
        let id = self.parse_identifier()?;
        if self.at_punctuation('[') || self.at_punctuation('{') {
            return self.parse_inline_rule_expression(start, id);
        }
        if id.name == "std" && self.eat_punctuation('.')? {
            let class_name = match &self.token.kind {
                TokenKind::At(name) | TokenKind::Identifier(name) => name.clone(),
                _ => return Err(self.unexpected()),
            };
            let Some(class) = CharClass::from_name(&class_name) else {
                return Err(self.unexpected());
            };
            self.next()?;
            return Ok(Expression::new(start, ExpressionKind::CharClass(class)));
        }
        let arguments = self.parse_arguments()?;
        Ok(Expression::new(
            start,
            ExpressionKind::Name(NameExpression {
                start,
                id,
                arguments,
            }),
        ))
    }

    fn parse_parenthesized_expression(
        &mut self,
        start: usize,
    ) -> Result<Expression, GeneratorError> {
        if self.eat_punctuation(')')? {
            return Ok(empty_expression(start, true));
        }
        let expression = self.parse_expression_choice()?;
        self.expect_punctuation(')')?;
        Ok(expression)
    }

    fn parse_literal_expression(
        &mut self,
        start: usize,
        value: String,
    ) -> Result<Expression, GeneratorError> {
        self.next()?;
        if value.is_empty() {
            return Ok(empty_expression(start, true));
        }
        Ok(Expression::new(
            start,
            ExpressionKind::Literal(LiteralExpression { start, value }),
        ))
    }

    fn parse_set_expression(
        &mut self,
        start: usize,
        source: &str,
        inverted: bool,
    ) -> Result<Expression, GeneratorError> {
        let ranges = parse_set_ranges(source, start, self.file_name, self.source)?;
        self.next()?;
        Ok(Expression::new(
            start,
            ExpressionKind::Set(SetExpression {
                start,
                ranges,
                inverted,
            }),
        ))
    }

    fn parse_specialize_expression(&mut self, start: usize) -> Result<Expression, GeneratorError> {
        let kind = if self.at_at("extend") {
            SpecializeKind::Extend
        } else {
            SpecializeKind::Specialize
        };
        self.next()?;
        let props = self.parse_props()?;
        self.expect_punctuation('<')?;
        let token = self.parse_expression_choice()?;
        let content = self.parse_specialize_content(kind, &token, start)?;
        self.expect_punctuation('>')?;
        Ok(Expression::new(
            start,
            ExpressionKind::Specialize(SpecializeExpression {
                start,
                kind,
                props,
                token: Box::new(token),
                content: Box::new(content),
            }),
        ))
    }

    fn parse_specialize_content(
        &mut self,
        kind: SpecializeKind,
        token: &Expression,
        start: usize,
    ) -> Result<Expression, GeneratorError> {
        if self.eat_punctuation(',')? {
            return self.parse_expression_choice();
        }
        if let ExpressionKind::Literal(literal) = &token.kind {
            return Ok(Expression::new(
                literal.start,
                ExpressionKind::Literal(literal.clone()),
            ));
        }
        let directive = match kind {
            SpecializeKind::Extend => "extend",
            SpecializeKind::Specialize => "specialize",
        };
        Err(self.error(
            format!(
                "@{directive} requires two arguments when its first argument isn't a literal string"
            ),
            start,
        ))
    }

    fn parse_inline_rule_expression(
        &mut self,
        start: usize,
        id: Identifier,
    ) -> Result<Expression, GeneratorError> {
        let rule = self.parse_rule(Some(id))?;
        if !rule.params.is_empty() {
            return Err(self.error("Inline rules can't have parameters", rule.start));
        }
        Ok(Expression::new(
            start,
            ExpressionKind::InlineRule(Box::new(rule)),
        ))
    }

    fn parse_arguments(&mut self) -> Result<Vec<Expression>, GeneratorError> {
        let mut arguments = Vec::new();
        if self.eat_punctuation('<')? {
            while !self.eat_punctuation('>')? {
                if !arguments.is_empty() {
                    self.expect_punctuation(',')?;
                }
                arguments.push(self.parse_expression_choice()?);
            }
        }
        Ok(arguments)
    }

    fn parse_expression_suffix(&mut self) -> Result<Expression, GeneratorError> {
        let start = self.token.start;
        let mut expression = self.parse_expression_inner()?;
        loop {
            let kind = if self.eat_punctuation('*')? {
                Some(RepeatKind::ZeroOrMore)
            } else if self.eat_punctuation('?')? {
                Some(RepeatKind::Optional)
            } else if self.eat_punctuation('+')? {
                Some(RepeatKind::OneOrMore)
            } else {
                None
            };
            let Some(kind) = kind else {
                return Ok(expression);
            };
            expression = Expression::new(
                start,
                ExpressionKind::Repeat {
                    expression: Box::new(expression),
                    kind,
                },
            );
        }
    }

    fn parse_expression_sequence(&mut self) -> Result<Expression, GeneratorError> {
        let start = self.token.start;
        let mut expressions = Vec::new();
        let mut markers = vec![Vec::new()];
        loop {
            loop {
                let marker_start = self.token.start;
                let kind = if self.eat_punctuation('~')? {
                    Some(ConflictMarkerKind::Ambiguity)
                } else if self.eat_punctuation('!')? {
                    Some(ConflictMarkerKind::Precedence)
                } else {
                    None
                };
                let Some(kind) = kind else {
                    break;
                };
                let id = self.parse_identifier()?;
                markers
                    .last_mut()
                    .expect("sequence always has a marker slot")
                    .push(ConflictMarker {
                        start: marker_start,
                        id,
                        kind,
                    });
            }
            if self.ends_sequence() {
                break;
            }
            expressions.push(self.parse_expression_suffix()?);
            markers.push(Vec::new());
        }
        if expressions.len() == 1 && markers.iter().all(Vec::is_empty) {
            return Ok(expressions.pop().expect("length was checked"));
        }
        let explicitly_empty = false;
        Ok(Expression::new(
            start,
            ExpressionKind::Sequence {
                expressions,
                markers,
                explicitly_empty,
            },
        ))
    }

    fn parse_expression_choice(&mut self) -> Result<Expression, GeneratorError> {
        let start = self.token.start;
        let left = self.parse_expression_sequence()?;
        if !self.eat_punctuation('|')? {
            return Ok(left);
        }
        let mut expressions = vec![left];
        loop {
            expressions.push(self.parse_expression_sequence()?);
            if !self.eat_punctuation('|')? {
                break;
            }
        }
        if let Some(empty) = expressions.iter().find(|expression| {
            matches!(
                expression.kind,
                ExpressionKind::Sequence {
                    ref expressions,
                    explicitly_empty: false,
                    ..
                } if expressions.is_empty()
            )
        }) {
            return Err(self.error(
                "Empty expression in choice operator. If this is intentional, use () to make it explicit.",
                empty.start,
            ));
        }
        Ok(Expression::new(start, ExpressionKind::Choice(expressions)))
    }

    fn parse_precedence(&mut self) -> Result<PrecDeclaration, GeneratorError> {
        let start = self.token.start;
        self.next()?;
        self.expect_punctuation('{')?;
        let mut items = Vec::new();
        while !self.eat_punctuation('}')? {
            if !items.is_empty() {
                let _ = self.eat_punctuation(',')?;
            }
            let id = self.parse_identifier()?;
            let kind = if self.eat_at("left")? {
                Some(PrecKind::Left)
            } else if self.eat_at("right")? {
                Some(PrecKind::Right)
            } else if self.eat_at("cut")? {
                Some(PrecKind::Cut)
            } else {
                None
            };
            items.push(PrecItem { id, kind });
        }
        Ok(PrecDeclaration { start, items })
    }

    fn parse_tokens(&mut self) -> Result<TokenDeclaration, GeneratorError> {
        let start = self.token.start;
        self.next()?;
        self.expect_punctuation('{')?;
        let mut rules = Vec::new();
        let mut literals = Vec::new();
        let mut precedences = Vec::new();
        let mut conflicts = Vec::new();
        while !self.eat_punctuation('}')? {
            if self.at_at("precedence") {
                precedences.push(self.parse_token_precedence()?);
            } else if self.at_at("conflict") {
                conflicts.push(self.parse_token_conflict()?);
            } else if let TokenKind::String(value) = &self.token.kind {
                let literal_start = self.token.start;
                let literal = value.clone();
                self.next()?;
                let props = self.parse_props()?;
                literals.push(LiteralDeclaration {
                    start: literal_start,
                    literal,
                    props,
                });
            } else {
                rules.push(self.parse_rule(None)?);
            }
        }
        Ok(TokenDeclaration {
            start,
            precedences,
            conflicts,
            rules,
            literals,
        })
    }

    fn parse_local_tokens(
        &mut self,
        start: usize,
    ) -> Result<LocalTokenDeclaration, GeneratorError> {
        self.expect_punctuation('{')?;
        let mut rules = Vec::new();
        let mut precedences = Vec::new();
        let mut fallback = None;
        while !self.eat_punctuation('}')? {
            if self.at_at("precedence") {
                precedences.push(self.parse_token_precedence()?);
            } else if self.eat_at("else")? && fallback.is_none() {
                let id = self.parse_identifier()?;
                let props = self.parse_props()?;
                fallback = Some(NamedNode { id, props });
            } else {
                rules.push(self.parse_rule(None)?);
            }
        }
        Ok(LocalTokenDeclaration {
            start,
            precedences,
            rules,
            fallback,
        })
    }

    fn parse_token_precedence(&mut self) -> Result<TokenPrecDeclaration, GeneratorError> {
        let start = self.token.start;
        self.next()?;
        self.expect_punctuation('{')?;
        let mut items = Vec::new();
        while !self.eat_punctuation('}')? {
            if !items.is_empty() {
                let _ = self.eat_punctuation(',')?;
            }
            items.push(self.parse_token_reference("Invalid expression in token precedences")?);
        }
        Ok(TokenPrecDeclaration { start, items })
    }

    fn parse_token_conflict(&mut self) -> Result<TokenConflictDeclaration, GeneratorError> {
        let start = self.token.start;
        self.next()?;
        self.expect_punctuation('{')?;
        let left = self.parse_token_reference("Invalid expression in token conflict")?;
        let _ = self.eat_punctuation(',')?;
        let right = self.parse_token_reference("Invalid expression in token conflict")?;
        self.expect_punctuation('}')?;
        Ok(TokenConflictDeclaration { start, left, right })
    }

    fn parse_token_reference(&mut self, message: &str) -> Result<TokenReference, GeneratorError> {
        let expression = self.parse_expression_inner()?;
        match expression.kind {
            ExpressionKind::Literal(literal) => Ok(TokenReference::Literal(literal)),
            ExpressionKind::Name(name) => Ok(TokenReference::Name(name)),
            _ => Err(self.error(message, expression.start)),
        }
    }

    fn parse_external_token_set(
        &mut self,
        allow_conflicts: bool,
    ) -> Result<(Vec<NamedNode>, Vec<Identifier>), GeneratorError> {
        let mut tokens = Vec::new();
        let mut conflicts = Vec::new();
        self.expect_punctuation('{')?;
        let mut first = true;
        while !self.eat_punctuation('}')? {
            if !first {
                let _ = self.eat_punctuation(',')?;
            }
            first = false;
            if allow_conflicts && self.eat_at("conflict")? {
                self.expect_punctuation('{')?;
                let mut conflict_first = true;
                while !self.eat_punctuation('}')? {
                    if !conflict_first {
                        let _ = self.eat_punctuation(',')?;
                    }
                    conflict_first = false;
                    conflicts.push(self.parse_identifier()?);
                }
            } else {
                let id = self.parse_identifier()?;
                let props = self.parse_props()?;
                tokens.push(NamedNode { id, props });
            }
        }
        Ok((tokens, conflicts))
    }

    fn parse_external_tokens(
        &mut self,
        start: usize,
    ) -> Result<ExternalTokenDeclaration, GeneratorError> {
        let id = self.parse_identifier()?;
        self.expect_identifier_value("from")?;
        let source = self.expect_string()?;
        let (tokens, conflicts) = self.parse_external_token_set(true)?;
        Ok(ExternalTokenDeclaration {
            start,
            id,
            source,
            tokens,
            conflicts,
        })
    }

    fn parse_external_specializer(
        &mut self,
        start: usize,
        kind: SpecializeKind,
    ) -> Result<ExternalSpecializeDeclaration, GeneratorError> {
        let token = self.parse_braced_expression()?;
        let id = self.parse_identifier()?;
        self.expect_identifier_value("from")?;
        let source = self.expect_string()?;
        let (tokens, _) = self.parse_external_token_set(false)?;
        Ok(ExternalSpecializeDeclaration {
            start,
            kind,
            token,
            id,
            source,
            tokens,
        })
    }

    fn parse_external_prop_source(
        &mut self,
        start: usize,
    ) -> Result<ExternalPropSourceDeclaration, GeneratorError> {
        let id = self.parse_identifier()?;
        self.expect_identifier_value("from")?;
        let source = self.expect_string()?;
        Ok(ExternalPropSourceDeclaration { start, id, source })
    }

    fn parse_external_prop(
        &mut self,
        start: usize,
    ) -> Result<ExternalPropDeclaration, GeneratorError> {
        let external_id = self.parse_identifier()?;
        let id = if self.eat_identifier_value("as")? {
            self.parse_identifier()?
        } else {
            external_id.clone()
        };
        self.expect_identifier_value("from")?;
        let source = self.expect_string()?;
        Ok(ExternalPropDeclaration {
            start,
            id,
            external_id,
            source,
        })
    }

    fn parse_identifier(&mut self) -> Result<Identifier, GeneratorError> {
        let TokenKind::Identifier(name) = &self.token.kind else {
            return Err(self.unexpected());
        };
        let identifier = Identifier {
            start: self.token.start,
            name: name.clone(),
        };
        self.next()?;
        Ok(identifier)
    }

    fn expect_identifier_value(&mut self, expected: &str) -> Result<(), GeneratorError> {
        if self.eat_identifier_value(expected)? {
            Ok(())
        } else {
            Err(self.unexpected())
        }
    }

    fn eat_identifier_value(&mut self, expected: &str) -> Result<bool, GeneratorError> {
        let matches = matches!(&self.token.kind, TokenKind::Identifier(name) if name == expected);
        if matches {
            self.next()?;
        }
        Ok(matches)
    }

    fn expect_string(&mut self) -> Result<String, GeneratorError> {
        let TokenKind::String(value) = &self.token.kind else {
            return Err(self.unexpected());
        };
        let value = value.clone();
        self.next()?;
        Ok(value)
    }

    fn ends_sequence(&self) -> bool {
        matches!(
            self.token.kind,
            TokenKind::Eof | TokenKind::Punctuation('}' | ')' | '|' | '{' | ',' | '>')
        )
    }

    fn at_at(&self, expected: &str) -> bool {
        matches!(&self.token.kind, TokenKind::At(name) if name == expected)
    }

    fn eat_at(&mut self, expected: &str) -> Result<bool, GeneratorError> {
        let matches = self.at_at(expected);
        if matches {
            self.next()?;
        }
        Ok(matches)
    }

    fn at_punctuation(&self, expected: char) -> bool {
        self.token.kind == TokenKind::Punctuation(expected)
    }

    fn eat_punctuation(&mut self, expected: char) -> Result<bool, GeneratorError> {
        let matches = self.at_punctuation(expected);
        if matches {
            self.next()?;
        }
        Ok(matches)
    }

    fn expect_punctuation(&mut self, expected: char) -> Result<(), GeneratorError> {
        if self.eat_punctuation(expected)? {
            Ok(())
        } else {
            Err(self.unexpected())
        }
    }

    fn at_eof(&self) -> bool {
        self.token.kind == TokenKind::Eof
    }

    fn next(&mut self) -> Result<(), GeneratorError> {
        let mut start = self.token.end;
        start = self.skip_trivia(start)?;
        if start == self.source.len() {
            self.token = Token {
                kind: TokenKind::Eof,
                start,
                end: start,
            };
            return Ok(());
        }
        let character = self.source[start..]
            .chars()
            .next()
            .expect("start precedes source end");
        if character == '"' || character == '\'' {
            self.token = self.lex_string(start, character)?;
            return Ok(());
        }
        if character == '@' {
            self.token = self.lex_at(start)?;
            return Ok(());
        }
        if (character == '$' || character == '!')
            && self.source[start + character.len_utf8()..].starts_with('[')
        {
            self.token = self.lex_set(start, character == '!')?;
            return Ok(());
        }
        if is_punctuation(character) {
            self.token = Token {
                kind: TokenKind::Punctuation(character),
                start,
                end: start + character.len_utf8(),
            };
            return Ok(());
        }
        if is_word(character) {
            let end = scan_while(self.source, start, is_word);
            self.token = Token {
                kind: TokenKind::Identifier(self.source[start..end].to_owned()),
                start,
                end,
            };
            return Ok(());
        }
        Err(self.error(format!("Unexpected character {character:?}"), start))
    }

    fn skip_trivia(&self, mut cursor: usize) -> Result<usize, GeneratorError> {
        loop {
            let whitespace_end = scan_while(self.source, cursor, is_ecmascript_whitespace);
            cursor = whitespace_end;
            let rest = &self.source[cursor..];
            if rest.starts_with("//") {
                cursor += 2;
                cursor = scan_while(self.source, cursor, |character| {
                    !is_line_terminator(character)
                });
                continue;
            }
            if rest.starts_with("/*") {
                let body_start = cursor + 2;
                let Some(relative_end) = self.source[body_start..].find("*/") else {
                    return Err(self.error(
                        format!(
                            "Unexpected character {:?}",
                            self.source[cursor..].chars().next()
                        ),
                        cursor,
                    ));
                };
                cursor = body_start + relative_end + 2;
                continue;
            }
            return Ok(cursor);
        }
    }

    fn lex_string(&self, start: usize, quote: char) -> Result<Token, GeneratorError> {
        let mut cursor = start + quote.len_utf8();
        let content_start = cursor;
        while cursor < self.source.len() {
            let character = self.source[cursor..]
                .chars()
                .next()
                .expect("cursor precedes source end");
            if character == quote {
                let raw = &self.source[content_start..cursor];
                let value = decode_string(raw);
                let end = cursor + quote.len_utf8();
                return Ok(Token {
                    kind: TokenKind::String(value),
                    start,
                    end,
                });
            }
            if character == '\\' {
                cursor += 1;
                if cursor >= self.source.len() {
                    break;
                }
                let escaped = self.source[cursor..]
                    .chars()
                    .next()
                    .expect("cursor precedes source end");
                if is_line_terminator(escaped) {
                    break;
                }
                cursor += escaped.len_utf8();
            } else {
                cursor += character.len_utf8();
            }
        }
        Err(self.error("Unterminated string literal", start))
    }

    fn lex_at(&self, start: usize) -> Result<Token, GeneratorError> {
        let name_start = start + 1;
        let end = scan_while(self.source, name_start, is_word);
        if end == name_start {
            return Err(self.error("@ without a name", start));
        }
        Ok(Token {
            kind: TokenKind::At(self.source[name_start..end].to_owned()),
            start,
            end,
        })
    }

    fn lex_set(&self, start: usize, inverted: bool) -> Result<Token, GeneratorError> {
        let mut cursor = start + 2;
        let content_start = cursor;
        while cursor < self.source.len() {
            let character = self.source[cursor..]
                .chars()
                .next()
                .expect("cursor precedes source end");
            if character == ']' {
                let source = self.source[content_start..cursor].to_owned();
                return Ok(Token {
                    kind: TokenKind::Set { source, inverted },
                    start,
                    end: cursor + 1,
                });
            }
            if character == '\\' {
                cursor += 1;
                if cursor >= self.source.len() {
                    break;
                }
                let escaped = self.source[cursor..]
                    .chars()
                    .next()
                    .expect("cursor precedes source end");
                cursor += escaped.len_utf8();
            } else {
                cursor += character.len_utf8();
            }
        }
        Err(self.error("Unterminated character set", start))
    }

    fn unexpected(&self) -> GeneratorError {
        let found = &self.source[self.token.start..self.token.end];
        self.error(format!("Unexpected token '{found}'"), self.token.start)
    }

    fn error(&self, message: impl Into<String>, position: usize) -> GeneratorError {
        let message = message.into();
        let (line, column) = line_info(self.source, position);
        let location = match self.file_name {
            Some(file_name) => format!("{file_name} {line}:{column}"),
            None => format!("{line}:{column}"),
        };
        GeneratorError::new(format!("{message} ({location})"), Some(position))
    }
}

fn empty_expression(start: usize, explicitly_empty: bool) -> Expression {
    Expression::new(
        start,
        ExpressionKind::Sequence {
            expressions: Vec::new(),
            markers: vec![Vec::new(), Vec::new()],
            explicitly_empty,
        },
    )
}

fn is_punctuation(character: char) -> bool {
    matches!(
        character,
        '[' | ']'
            | '('
            | ')'
            | '!'
            | '~'
            | '+'
            | '*'
            | '?'
            | '{'
            | '}'
            | '<'
            | '>'
            | '.'
            | ','
            | '|'
            | ':'
            | '$'
            | '='
    )
}

fn is_word(character: char) -> bool {
    character == '_' || character == '-' || character.is_alphabetic() || character.is_ascii_digit()
}

fn is_ecmascript_whitespace(character: char) -> bool {
    (character.is_whitespace() && character != '\u{0085}') || character == '\u{feff}'
}

fn is_line_terminator(character: char) -> bool {
    matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn scan_while(source: &str, mut cursor: usize, predicate: impl Fn(char) -> bool) -> usize {
    while cursor < source.len() {
        let character = source[cursor..]
            .chars()
            .next()
            .expect("cursor precedes source end");
        if !predicate(character) {
            break;
        }
        cursor += character.len_utf8();
    }
    cursor
}

fn line_info(source: &str, position: usize) -> (usize, usize) {
    let prefix = &source[..position.min(source.len())];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let line_start = prefix.rfind('\n').map_or(0, |offset| offset + 1);
    (line, position.saturating_sub(line_start))
}

fn decode_string(source: &str) -> String {
    let mut result = String::new();
    let mut cursor = 0;
    while cursor < source.len() {
        let character = source[cursor..]
            .chars()
            .next()
            .expect("cursor precedes source end");
        if character != '\\' {
            result.push(character);
            cursor += character.len_utf8();
            continue;
        }
        cursor += 1;
        if cursor >= source.len() {
            break;
        }
        let escaped = source[cursor..]
            .chars()
            .next()
            .expect("cursor precedes source end");
        cursor += escaped.len_utf8();
        match escaped {
            'u' | 'U' => {
                if source[cursor..].starts_with('{')
                    && let Some(close) = source[cursor + 1..].find('}')
                {
                    let end = cursor + 1 + close;
                    let digits = &source[cursor + 1..end];
                    if let Some(value) = decode_hex_scalar(digits) {
                        result.push(value);
                        cursor = end + 1;
                        continue;
                    }
                }
                if let Some((value, end)) = decode_fixed_hex(source, cursor, 4) {
                    result.push(value);
                    cursor = end;
                } else {
                    result.push(escaped);
                }
            }
            'x' | 'X' => {
                if let Some((value, end)) = decode_fixed_hex(source, cursor, 2) {
                    result.push(value);
                    cursor = end;
                } else {
                    result.push(escaped);
                }
            }
            'n' => result.push('\n'),
            't' => result.push('\t'),
            'b' => result.push('\u{0008}'),
            'r' => result.push('\r'),
            'f' => result.push('\u{000c}'),
            '0' => result.push('\0'),
            unknown => result.push(unknown),
        }
    }
    result
}

fn decode_fixed_hex(source: &str, cursor: usize, digits: usize) -> Option<(char, usize)> {
    let end = cursor.checked_add(digits)?;
    let text = source.get(cursor..end)?;
    if !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(text, 16).ok()?;
    let character = char::from_u32(value)?;
    Some((character, end))
}

fn decode_hex_scalar(source: &str) -> Option<char> {
    if source.is_empty() || !source.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(source, 16).ok()?;
    char::from_u32(value)
}

fn parse_set_ranges(
    source: &str,
    position: usize,
    file_name: Option<&str>,
    grammar_source: &str,
) -> Result<Vec<(u32, u32)>, GeneratorError> {
    let atoms = set_atoms(source);
    let mut ranges = Vec::new();
    let mut cursor = 0;
    while cursor < atoms.len() {
        let (start, escaped) = atoms[cursor];
        if start == u32::from('-') && !escaped {
            push_range(
                &mut ranges,
                start,
                start + 1,
                position,
                file_name,
                grammar_source,
            )?;
            cursor += 1;
            continue;
        }
        if cursor + 2 < atoms.len() {
            let (separator, separator_escaped) = atoms[cursor + 1];
            if separator == u32::from('-') && !separator_escaped {
                let end = atoms[cursor + 2].0;
                if end < start {
                    return Err(source_error(
                        "Invalid character range",
                        position,
                        file_name,
                        grammar_source,
                    ));
                }
                push_range(
                    &mut ranges,
                    start,
                    end + 1,
                    position,
                    file_name,
                    grammar_source,
                )?;
                cursor += 3;
                continue;
            }
        }
        push_range(
            &mut ranges,
            start,
            start + 1,
            position,
            file_name,
            grammar_source,
        )?;
        cursor += 1;
    }
    ranges.sort_unstable();
    Ok(ranges)
}

fn set_atoms(source: &str) -> Vec<(u32, bool)> {
    let mut result = Vec::new();
    let mut cursor = 0;
    while cursor < source.len() {
        let character = source[cursor..]
            .chars()
            .next()
            .expect("cursor precedes source end");
        if character != '\\' {
            result.push((u32::from(character), false));
            cursor += character.len_utf8();
            continue;
        }
        let escape_start = cursor;
        cursor += 1;
        if cursor >= source.len() {
            break;
        }
        let escaped = source[cursor..]
            .chars()
            .next()
            .expect("cursor precedes source end");
        cursor += escaped.len_utf8();
        let decoded = match escaped {
            'n' => '\n',
            't' => '\t',
            'b' => '\u{0008}',
            'r' => '\r',
            'f' => '\u{000c}',
            '0' => '\0',
            'u' | 'U' => {
                if source[cursor..].starts_with('{')
                    && let Some(close) = source[cursor + 1..].find('}')
                {
                    let end = cursor + 1 + close;
                    let digits = &source[cursor + 1..end];
                    if let Some(value) = decode_hex_scalar(digits) {
                        cursor = end + 1;
                        value
                    } else {
                        escaped
                    }
                } else if let Some((value, end)) = decode_fixed_hex(source, cursor, 4) {
                    cursor = end;
                    value
                } else {
                    escaped
                }
            }
            'x' | 'X' => {
                if let Some((value, end)) = decode_fixed_hex(source, cursor, 2) {
                    cursor = end;
                    value
                } else {
                    escaped
                }
            }
            unknown => unknown,
        };
        let _ = escape_start;
        result.push((u32::from(decoded), true));
    }
    result
}

fn push_range(
    ranges: &mut Vec<(u32, u32)>,
    start: u32,
    end: u32,
    position: usize,
    file_name: Option<&str>,
    grammar_source: &str,
) -> Result<(), GeneratorError> {
    if ranges
        .iter()
        .any(|&(other_start, other_end)| other_end > start && other_start < end)
    {
        return Err(source_error(
            "Overlapping character range",
            position,
            file_name,
            grammar_source,
        ));
    }
    ranges.push((start, end));
    Ok(())
}

fn source_error(
    message: &str,
    position: usize,
    file_name: Option<&str>,
    source: &str,
) -> GeneratorError {
    let (line, column) = line_info(source, position);
    let location = match file_name {
        Some(file_name) => format!("{file_name} {line}:{column}"),
        None => format!("{line}:{column}"),
    };
    GeneratorError::new(format!("{message} ({location})"), Some(position))
}
