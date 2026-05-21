const MAX_EXPRESSION_SIZE: usize = 1_000;

#[derive(Debug)]
enum Expr<'source> {
    Tag(&'source str),
    Not(Box<Self>),
    And(Box<Self>, Box<Self>),
    Or(Box<Self>, Box<Self>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Token<'source> {
    Tag(&'source str),
    Not,
    And,
    Or,
    LeftParen,
    RightParen,
    End,
}

struct Lexer<'source> {
    source: &'source str,
    index: usize,
}

impl<'source> Lexer<'source> {
    fn next(&mut self) -> Result<Token<'source>, ()> {
        while self
            .source
            .as_bytes()
            .get(self.index)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
        {
            self.index += 1;
        }
        let Some(byte) = self.source.as_bytes().get(self.index).copied() else {
            return Ok(Token::End);
        };
        let single = match byte {
            b'!' => Some(Token::Not),
            b'(' => Some(Token::LeftParen),
            b')' => Some(Token::RightParen),
            _ => None,
        };
        if let Some(token) = single {
            self.index += 1;
            return Ok(token);
        }
        if matches!(byte, b'&' | b'|') {
            if self.source.as_bytes().get(self.index + 1) != Some(&byte) {
                return Err(());
            }
            self.index += 2;
            return Ok(if byte == b'&' { Token::And } else { Token::Or });
        }

        let start = self.index;
        for (offset, character) in self.source[start..].char_indices() {
            if !(character.is_alphanumeric() || matches!(character, '_' | '.')) {
                break;
            }
            self.index = start + offset + character.len_utf8();
        }
        if self.index == start {
            return Err(());
        }
        Ok(Token::Tag(&self.source[start..self.index]))
    }
}

struct Parser<'source> {
    lexer: Lexer<'source>,
    current: Token<'source>,
    size: usize,
}

impl<'source> Parser<'source> {
    fn parse(source: &'source str) -> Result<Expr<'source>, ()> {
        let mut parser = Self {
            lexer: Lexer { source, index: 0 },
            current: Token::End,
            size: 0,
        };
        parser.bump()?;
        let expression = parser.parse_or()?;
        (parser.current == Token::End)
            .then_some(expression)
            .ok_or(())
    }

    fn bump(&mut self) -> Result<(), ()> {
        self.current = self.lexer.next()?;
        Ok(())
    }

    fn parse_or(&mut self) -> Result<Expr<'source>, ()> {
        let mut expression = self.parse_and()?;
        while self.current == Token::Or {
            self.bump()?;
            let right = self.parse_and()?;
            expression = Expr::Or(Box::new(expression), Box::new(right));
        }
        Ok(expression)
    }

    fn parse_and(&mut self) -> Result<Expr<'source>, ()> {
        let mut expression = self.parse_not()?;
        while self.current == Token::And {
            self.bump()?;
            let right = self.parse_not()?;
            expression = Expr::And(Box::new(expression), Box::new(right));
        }
        Ok(expression)
    }

    fn parse_not(&mut self) -> Result<Expr<'source>, ()> {
        self.size += 1;
        if self.size > MAX_EXPRESSION_SIZE {
            return Err(());
        }
        if self.current != Token::Not {
            return self.parse_atom();
        }
        self.bump()?;
        if self.current == Token::Not {
            return Err(());
        }
        Ok(Expr::Not(Box::new(self.parse_atom()?)))
    }

    fn parse_atom(&mut self) -> Result<Expr<'source>, ()> {
        match self.current {
            Token::Tag(tag) => {
                self.bump()?;
                Ok(Expr::Tag(tag))
            }
            Token::LeftParen => {
                self.bump()?;
                let expression = self.parse_or()?;
                if self.current != Token::RightParen {
                    return Err(());
                }
                self.bump()?;
                Ok(expression)
            }
            _ => Err(()),
        }
    }
}

pub(super) fn go_version(comment: &str) -> Option<String> {
    let line = comment.trim();
    let suffix = line.strip_prefix("//go:build")?;
    let expression = suffix.trim();
    if (!suffix.is_empty() && expression.len() == suffix.len()) || expression.is_empty() {
        return None;
    }
    let expression = Parser::parse(expression).ok()?;
    let version = minimum_version(&expression, 1);
    match version {
        ..=-1 => Some(String::new()),
        0 => Some("go1".to_owned()),
        version => Some(format!("go1.{version}")),
    }
}

fn minimum_version(expression: &Expr<'_>, sign: i8) -> i32 {
    match expression {
        Expr::Tag(tag) if sign < 0 => -1,
        Expr::Tag(tag) if *tag == "go1" => 0,
        Expr::Tag(tag) => tag
            .strip_prefix("go1.")
            .and_then(|minor| minor.parse().ok())
            .unwrap_or(-1),
        Expr::Not(expression) => minimum_version(expression, -sign),
        Expr::And(left, right) if sign > 0 => {
            minimum_version(left, sign).max(minimum_version(right, sign))
        }
        Expr::And(left, right) => minimum_version(left, sign).min(minimum_version(right, sign)),
        Expr::Or(left, right) if sign > 0 => {
            minimum_version(left, sign).min(minimum_version(right, sign))
        }
        Expr::Or(left, right) => minimum_version(left, sign).max(minimum_version(right, sign)),
    }
}

#[cfg(test)]
mod tests {
    use super::go_version;

    #[test]
    fn matches_the_go_constraint_version_examples() {
        assert_eq!(go_version("//go:build windows"), Some(String::new()));
        assert_eq!(
            go_version("//go:build linux && go1.2"),
            Some("go1.2".to_owned())
        );
        assert_eq!(
            go_version("//go:build linux && go1.2 || windows"),
            Some(String::new())
        );
        assert_eq!(
            go_version("//go:build (linux && go1.2) || (windows && go1.1)"),
            Some("go1.1".to_owned())
        );
        assert_eq!(
            go_version("//go:build linux && go1.2 && go1.4"),
            Some("go1.4".to_owned())
        );
        assert_eq!(go_version("//go:build go1"), Some("go1".to_owned()));
    }

    #[test]
    fn rejects_non_constraints_and_invalid_expressions() {
        assert_eq!(go_version("//go:buildtag"), None);
        assert_eq!(go_version("//go:build"), None);
        assert_eq!(go_version("//go:build ! !go1.2"), None);
        assert_eq!(go_version("// +build go1.2"), None);
    }
}
