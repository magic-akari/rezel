use rezel_common::{SyntaxNode, TextSize, Tree};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RustSyntaxError {
    position: TextSize,
    message: &'static str,
}

impl RustSyntaxError {
    const fn new(position: TextSize, message: &'static str) -> Self {
        Self { position, message }
    }

    pub(crate) const fn position(self) -> TextSize {
        self.position
    }

    pub(crate) const fn message(self) -> &'static str {
        self.message
    }
}

pub(crate) fn validate_syntax(tree: &Tree, source: Option<&str>) -> Result<(), RustSyntaxError> {
    let root = tree.top_node();
    validate_node(&root, source)?;
    if let Some(source) = source {
        validate_source_tokens(&root, source)?;
    }
    Ok(())
}

fn validate_node(node: &SyntaxNode, source: Option<&str>) -> Result<(), RustSyntaxError> {
    match node.name().as_ref() {
        "BoundedType" => validate_precise_capture_bounds(node)?,
        "LetChain" => validate_let_chain(node)?,
        "UseBound" => validate_use_bound(node)?,
        _ => {}
    }
    if let Some(source) = source {
        match node.name().as_ref() {
            "String" => validate_string(node, source)?,
            "RawString" => validate_raw_string(node, source)?,
            "Char" => validate_character(node, source)?,
            "Integer" => validate_integer(node, source)?,
            "Float" => validate_float(node, source)?,
            _ => {}
        }
    }
    for child in node.children() {
        validate_node(&child, source)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LiteralFamily {
    Text,
    Byte,
    C,
}

fn validate_string(node: &SyntaxNode, source: &str) -> Result<(), RustSyntaxError> {
    let spelling = node_text(node, source)?;
    let (family, content) = if let Some(content) = spelling
        .strip_prefix('"')
        .and_then(|spelling| spelling.strip_suffix('"'))
    {
        (LiteralFamily::Text, content)
    } else if let Some(content) = spelling
        .strip_prefix("b\"")
        .and_then(|spelling| spelling.strip_suffix('"'))
    {
        (LiteralFamily::Byte, content)
    } else if let Some(content) = spelling
        .strip_prefix("c\"")
        .and_then(|spelling| spelling.strip_suffix('"'))
    {
        (LiteralFamily::C, content)
    } else {
        return Err(RustSyntaxError::new(
            node.from(),
            "a complete Rust string literal",
        ));
    };
    validate_cooked_content(node, content, family, true)
}

fn validate_raw_string(node: &SyntaxNode, source: &str) -> Result<(), RustSyntaxError> {
    let spelling = node_text(node, source)?;
    let (family, remainder) = if let Some(remainder) = spelling.strip_prefix("br") {
        (LiteralFamily::Byte, remainder)
    } else if let Some(remainder) = spelling.strip_prefix("cr") {
        (LiteralFamily::C, remainder)
    } else if let Some(remainder) = spelling.strip_prefix('r') {
        (LiteralFamily::Text, remainder)
    } else {
        return Err(RustSyntaxError::new(
            node.from(),
            "a Rust raw string prefix",
        ));
    };

    let hashes = remainder.bytes().take_while(|&byte| byte == b'#').count();
    if hashes > 255 {
        return Err(RustSyntaxError::new(
            node.from(),
            "at most 255 `#` delimiters in a Rust raw string",
        ));
    }
    let opening = hashes;
    if remainder.as_bytes().get(opening) != Some(&b'"') {
        return Err(RustSyntaxError::new(
            node.from(),
            "an opening quote in a Rust raw string",
        ));
    }
    let delimiter_length = hashes + 1;
    if remainder.len() < delimiter_length * 2 {
        return Err(RustSyntaxError::new(
            node.from(),
            "a complete Rust raw string literal",
        ));
    }
    let closing = remainder.len() - delimiter_length;
    if remainder.as_bytes().get(closing) != Some(&b'"')
        || !remainder[closing + 1..].bytes().all(|byte| byte == b'#')
    {
        return Err(RustSyntaxError::new(
            node.from(),
            "matching delimiters in a Rust raw string",
        ));
    }

    let content = &remainder[opening + 1..closing];
    validate_literal_characters(node, content, family)
}

fn validate_character(node: &SyntaxNode, source: &str) -> Result<(), RustSyntaxError> {
    let spelling = node_text(node, source)?;
    let (family, content) = if let Some(content) = spelling
        .strip_prefix('\'')
        .and_then(|spelling| spelling.strip_suffix('\''))
    {
        (LiteralFamily::Text, content)
    } else if let Some(content) = spelling
        .strip_prefix("b'")
        .and_then(|spelling| spelling.strip_suffix('\''))
    {
        (LiteralFamily::Byte, content)
    } else {
        return Err(RustSyntaxError::new(
            node.from(),
            "a complete Rust character literal",
        ));
    };

    if content.is_empty() {
        return Err(RustSyntaxError::new(
            node.from(),
            "one character in a Rust character literal",
        ));
    }
    if content.starts_with('\\') {
        let Some((end, _)) = parse_escape(content, 0, family, false) else {
            return Err(RustSyntaxError::new(
                node.from(),
                "a valid Rust character escape",
            ));
        };
        if end != content.len() {
            return Err(RustSyntaxError::new(
                node.from(),
                "one escape in a Rust character literal",
            ));
        }
        return Ok(());
    }

    let mut characters = content.chars();
    let character = characters.next().ok_or_else(|| {
        RustSyntaxError::new(node.from(), "one character in a Rust character literal")
    })?;
    if characters.next().is_some()
        || matches!(character, '\'' | '\\' | '\n' | '\r' | '\t')
        || (family == LiteralFamily::Byte && !character.is_ascii())
    {
        return Err(RustSyntaxError::new(
            node.from(),
            "one permitted character in a Rust character literal",
        ));
    }
    Ok(())
}

fn validate_integer(node: &SyntaxNode, source: &str) -> Result<(), RustSyntaxError> {
    let spelling = strip_integer_suffix(node_text(node, source)?);
    let valid = if let Some(digits) = spelling.strip_prefix("0b") {
        valid_radix_digits(digits, |byte| matches!(byte, b'0' | b'1'))
    } else if let Some(digits) = spelling.strip_prefix("0o") {
        valid_radix_digits(digits, |byte| matches!(byte, b'0'..=b'7'))
    } else if let Some(digits) = spelling.strip_prefix("0x") {
        valid_radix_digits(digits, |byte| byte.is_ascii_hexdigit())
    } else {
        valid_decimal_digits(spelling)
    };
    if valid {
        Ok(())
    } else {
        Err(RustSyntaxError::new(
            node.from(),
            "a valid Rust integer literal",
        ))
    }
}

fn validate_float(node: &SyntaxNode, source: &str) -> Result<(), RustSyntaxError> {
    let spelling = node_text(node, source)?;
    let (body, has_suffix) = if let Some(body) = spelling.strip_suffix("f32") {
        (body, true)
    } else if let Some(body) = spelling.strip_suffix("f64") {
        (body, true)
    } else {
        (spelling, false)
    };

    let exponent = body.bytes().position(|byte| matches!(byte, b'e' | b'E'));
    let valid = if let Some(exponent) = exponent {
        let mantissa = &body[..exponent];
        let exponent = &body[exponent + 1..];
        valid_float_mantissa(mantissa, false) && valid_float_exponent(exponent)
    } else if body.contains('.') {
        valid_float_mantissa(body, !has_suffix)
    } else {
        has_suffix && valid_decimal_digits(body)
    };
    if valid {
        Ok(())
    } else {
        Err(RustSyntaxError::new(
            node.from(),
            "a valid Rust floating-point literal",
        ))
    }
}

fn strip_integer_suffix(spelling: &str) -> &str {
    const SUFFIXES: [&str; 12] = [
        "usize", "isize", "u128", "i128", "u64", "i64", "u32", "i32", "u16", "i16", "u8", "i8",
    ];
    SUFFIXES
        .into_iter()
        .find_map(|suffix| spelling.strip_suffix(suffix))
        .unwrap_or(spelling)
}

fn valid_radix_digits(digits: &str, valid_digit: impl Fn(u8) -> bool) -> bool {
    digits.bytes().any(&valid_digit) && digits.bytes().all(|byte| byte == b'_' || valid_digit(byte))
}

fn valid_decimal_digits(digits: &str) -> bool {
    digits.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && digits
            .bytes()
            .all(|byte| byte == b'_' || byte.is_ascii_digit())
}

fn valid_float_mantissa(mantissa: &str, allow_empty_fraction: bool) -> bool {
    let Some((integer, fraction)) = mantissa.split_once('.') else {
        return valid_decimal_digits(mantissa);
    };
    if fraction.contains('.') || !valid_decimal_digits(integer) {
        return false;
    }
    (allow_empty_fraction && fraction.is_empty()) || valid_decimal_digits(fraction)
}

fn valid_float_exponent(exponent: &str) -> bool {
    let exponent = exponent
        .strip_prefix(['+', '-'])
        .unwrap_or(exponent)
        .trim_start_matches('_');
    valid_decimal_digits(exponent)
}

fn validate_cooked_content(
    node: &SyntaxNode,
    content: &str,
    family: LiteralFamily,
    allow_continuation: bool,
) -> Result<(), RustSyntaxError> {
    let mut offset = 0;
    while offset < content.len() {
        if content.as_bytes()[offset] == b'\\' {
            let Some((end, value)) = parse_escape(content, offset, family, allow_continuation)
            else {
                return Err(RustSyntaxError::new(
                    node.from(),
                    "a valid Rust string escape",
                ));
            };
            if family == LiteralFamily::C && value == Some(0) {
                return Err(RustSyntaxError::new(
                    node.from(),
                    "no interior NUL in a Rust C string",
                ));
            }
            offset = end;
            continue;
        }

        let character = content[offset..]
            .chars()
            .next()
            .expect("offset remains on a UTF-8 boundary");
        if character == '\r' {
            if content.as_bytes().get(offset + 1) == Some(&b'\n') {
                offset += 2;
                continue;
            }
            return Err(RustSyntaxError::new(
                node.from(),
                "no isolated carriage return in a Rust string",
            ));
        }
        if family == LiteralFamily::Byte && !character.is_ascii() {
            return Err(RustSyntaxError::new(
                node.from(),
                "only ASCII source characters in a Rust byte string",
            ));
        }
        if family == LiteralFamily::C && character == '\0' {
            return Err(RustSyntaxError::new(
                node.from(),
                "no interior NUL in a Rust C string",
            ));
        }
        offset += character.len_utf8();
    }
    Ok(())
}

fn validate_literal_characters(
    node: &SyntaxNode,
    content: &str,
    family: LiteralFamily,
) -> Result<(), RustSyntaxError> {
    let mut offset = 0;
    while offset < content.len() {
        let character = content[offset..]
            .chars()
            .next()
            .expect("offset remains on a UTF-8 boundary");
        if character == '\r' {
            if content.as_bytes().get(offset + 1) == Some(&b'\n') {
                offset += 2;
                continue;
            }
            return Err(RustSyntaxError::new(
                node.from(),
                "no isolated carriage return in a Rust raw string",
            ));
        }
        if family == LiteralFamily::Byte && !character.is_ascii() {
            return Err(RustSyntaxError::new(
                node.from(),
                "only ASCII source characters in a Rust raw byte string",
            ));
        }
        if family == LiteralFamily::C && character == '\0' {
            return Err(RustSyntaxError::new(
                node.from(),
                "no interior NUL in a Rust raw C string",
            ));
        }
        offset += character.len_utf8();
    }
    Ok(())
}

fn parse_escape(
    content: &str,
    offset: usize,
    family: LiteralFamily,
    allow_continuation: bool,
) -> Option<(usize, Option<u32>)> {
    let bytes = content.as_bytes();
    let kind = *bytes.get(offset + 1)?;
    match kind {
        b'n' => Some((offset + 2, Some(u32::from(b'\n')))),
        b'r' => Some((offset + 2, Some(u32::from(b'\r')))),
        b't' => Some((offset + 2, Some(u32::from(b'\t')))),
        b'\\' => Some((offset + 2, Some(u32::from(b'\\')))),
        b'\'' => Some((offset + 2, Some(u32::from(b'\'')))),
        b'"' => Some((offset + 2, Some(u32::from(b'"')))),
        b'0' => Some((offset + 2, Some(0))),
        b'x' => {
            let high = hex_value(*bytes.get(offset + 2)?)?;
            let low = hex_value(*bytes.get(offset + 3)?)?;
            let value = high * 16 + low;
            if family == LiteralFamily::Text && value > 0x7f {
                return None;
            }
            Some((offset + 4, Some(value)))
        }
        b'u' if family != LiteralFamily::Byte => parse_unicode_escape(content, offset),
        b'\n' if allow_continuation => Some((offset + 2, None)),
        b'\r' if allow_continuation && bytes.get(offset + 2) == Some(&b'\n') => {
            Some((offset + 3, None))
        }
        _ => None,
    }
}

fn parse_unicode_escape(content: &str, offset: usize) -> Option<(usize, Option<u32>)> {
    let bytes = content.as_bytes();
    if bytes.get(offset + 2) != Some(&b'{') {
        return None;
    }
    let mut cursor = offset + 3;
    let mut digits = 0;
    let mut value = 0_u32;
    loop {
        match *bytes.get(cursor)? {
            b'}' if digits != 0 => {
                char::from_u32(value)?;
                return Some((cursor + 1, Some(value)));
            }
            b'_' if digits != 0 => {}
            byte => {
                let digit = hex_value(byte)?;
                digits += 1;
                if digits > 6 {
                    return None;
                }
                value = value.checked_mul(16)?.checked_add(digit)?;
            }
        }
        cursor += 1;
    }
}

fn hex_value(byte: u8) -> Option<u32> {
    match byte {
        b'0'..=b'9' => Some(u32::from(byte - b'0')),
        b'a'..=b'f' => Some(u32::from(byte - b'a' + 10)),
        b'A'..=b'F' => Some(u32::from(byte - b'A' + 10)),
        _ => None,
    }
}

fn validate_source_tokens(root: &SyntaxNode, source: &str) -> Result<(), RustSyntaxError> {
    let mut excluded = Vec::new();
    collect_source_scan_exclusions(root, &mut excluded);
    excluded.sort_unstable();

    let bytes = source.as_bytes();
    let mut excluded_index = 0;
    let mut offset = 0;
    while offset < bytes.len() {
        while excluded
            .get(excluded_index)
            .is_some_and(|&(_, end)| end <= offset)
        {
            excluded_index += 1;
        }
        if let Some(&(start, end)) = excluded.get(excluded_index)
            && start <= offset
        {
            offset = end;
            continue;
        }
        if bytes[offset].is_ascii_digit() {
            offset = scan_source_number(source, offset)
                .map_err(|message| RustSyntaxError::new(text_position(offset), message))?;
            continue;
        }
        if bytes[offset] == b'#' {
            let next = bytes.get(offset + 1);
            if next == Some(&b'#') {
                return Err(RustSyntaxError::new(
                    text_position(offset),
                    "no Edition 2024 reserved `##` token",
                ));
            }
            if next == Some(&b'"') {
                return Err(RustSyntaxError::new(
                    text_position(offset),
                    "no Edition 2024 reserved guarded string",
                ));
            }
        }
        offset += 1;
    }
    Ok(())
}

fn scan_source_number(source: &str, start: usize) -> Result<usize, &'static str> {
    let bytes = source.as_bytes();
    if bytes.get(start..start + 2) == Some(b"0b") {
        return scan_radix_number(source, start, |byte| matches!(byte, b'0' | b'1'), true);
    }
    if bytes.get(start..start + 2) == Some(b"0o") {
        return scan_radix_number(source, start, |byte| matches!(byte, b'0'..=b'7'), true);
    }
    if bytes.get(start..start + 2) == Some(b"0x") {
        return scan_radix_number(source, start, |byte| byte.is_ascii_hexdigit(), false);
    }

    let mut cursor = scan_decimal_digits(bytes, start);
    if dot_starts_float(source, cursor) {
        cursor += 1;
        if bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor = scan_decimal_digits(bytes, cursor);
        }
        if bytes
            .get(cursor)
            .is_some_and(|byte| matches!(byte, b'e' | b'E'))
        {
            return scan_source_exponent(source, cursor + 1);
        }
        return Ok(scan_source_suffix(source, cursor));
    }
    if bytes
        .get(cursor)
        .is_some_and(|byte| matches!(byte, b'e' | b'E'))
    {
        return scan_source_exponent(source, cursor + 1);
    }
    Ok(scan_source_suffix(source, cursor))
}

fn scan_radix_number(
    source: &str,
    start: usize,
    valid_digit: impl Fn(u8) -> bool,
    rejects_decimal_digits_and_exponent: bool,
) -> Result<usize, &'static str> {
    let bytes = source.as_bytes();
    let mut cursor = start + 2;
    while bytes.get(cursor) == Some(&b'_') {
        cursor += 1;
    }
    if !bytes.get(cursor).copied().is_some_and(&valid_digit) {
        return Err("at least one digit after a Rust radix prefix");
    }
    while bytes
        .get(cursor)
        .copied()
        .is_some_and(|byte| byte == b'_' || valid_digit(byte))
    {
        cursor += 1;
    }
    if rejects_decimal_digits_and_exponent
        && bytes
            .get(cursor)
            .is_some_and(|byte| byte.is_ascii_digit() || matches!(byte, b'e' | b'E'))
    {
        return Err("no out-of-radix digit or exponent after a Rust radix literal");
    }
    if dot_starts_float(source, cursor) {
        return Err("no floating-point period after a Rust radix literal");
    }
    Ok(scan_source_suffix(source, cursor))
}

fn scan_source_exponent(source: &str, mut cursor: usize) -> Result<usize, &'static str> {
    let bytes = source.as_bytes();
    if bytes
        .get(cursor)
        .is_some_and(|byte| matches!(byte, b'+' | b'-'))
    {
        cursor += 1;
    }
    while bytes.get(cursor) == Some(&b'_') {
        cursor += 1;
    }
    if !bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        return Err("at least one digit in a Rust floating-point exponent");
    }
    cursor = scan_decimal_digits(bytes, cursor);
    Ok(scan_source_suffix(source, cursor))
}

fn scan_decimal_digits(bytes: &[u8], mut cursor: usize) -> usize {
    while bytes
        .get(cursor)
        .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_')
    {
        cursor += 1;
    }
    cursor
}

fn dot_starts_float(source: &str, cursor: usize) -> bool {
    let bytes = source.as_bytes();
    if bytes.get(cursor) != Some(&b'.') {
        return false;
    }
    let after = cursor + 1;
    bytes.get(after) != Some(&b'.')
        && bytes.get(after) != Some(&b'_')
        && !source[after..]
            .chars()
            .next()
            .is_some_and(unicode_ident::is_xid_start)
}

fn scan_source_suffix(source: &str, mut cursor: usize) -> usize {
    let Some(first) = source[cursor..].chars().next() else {
        return cursor;
    };
    if first != '_' && !unicode_ident::is_xid_start(first) {
        return cursor;
    }
    cursor += first.len_utf8();
    while let Some(character) = source[cursor..].chars().next()
        && unicode_ident::is_xid_continue(character)
    {
        cursor += character.len_utf8();
    }
    cursor
}

fn collect_source_scan_exclusions(node: &SyntaxNode, ranges: &mut Vec<(usize, usize)>) {
    let name = node.name();
    if matches!(
        name.as_ref(),
        "String"
            | "RawString"
            | "Char"
            | "LineComment"
            | "BlockComment"
            | "Lifetime"
            | "LoopLabel"
            | "Metavariable"
    ) || name.ends_with("Identifier")
    {
        ranges.push((usize::from(node.from()), usize::from(node.to())));
        return;
    }
    for child in node.children() {
        collect_source_scan_exclusions(&child, ranges);
    }
}

fn node_text<'source>(
    node: &SyntaxNode,
    source: &'source str,
) -> Result<&'source str, RustSyntaxError> {
    source
        .get(usize::from(node.from())..usize::from(node.to()))
        .ok_or_else(|| RustSyntaxError::new(node.from(), "a Rust CST range on UTF-8 boundaries"))
}

fn text_position(offset: usize) -> TextSize {
    let offset = u32::try_from(offset).expect("Rezel source offsets fit in u32");
    TextSize::from(offset)
}

fn validate_precise_capture_bounds(bounds: &SyntaxNode) -> Result<(), RustSyntaxError> {
    let mut found = false;
    if let Some(position) = duplicate_use_bound(bounds, &mut found) {
        return Err(RustSyntaxError::new(
            position,
            "at most one precise capturing `use<...>` bound",
        ));
    }
    Ok(())
}

fn duplicate_use_bound(node: &SyntaxNode, found: &mut bool) -> Option<TextSize> {
    for child in node.children() {
        match child.name().as_ref() {
            "BoundedType" => {
                if let Some(position) = duplicate_use_bound(&child, found) {
                    return Some(position);
                }
            }
            "UseBound" if *found => return Some(child.from()),
            "UseBound" => *found = true,
            _ => {}
        }
    }
    None
}

fn validate_use_bound(bound: &SyntaxNode) -> Result<(), RustSyntaxError> {
    let mut found_identifier = false;
    for child in bound.children() {
        match child.name().as_ref() {
            "Identifier" => found_identifier = true,
            "Lifetime" if found_identifier => {
                return Err(RustSyntaxError::new(
                    child.from(),
                    "lifetimes before type and const parameters in a precise capturing bound",
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_let_chain(chain: &SyntaxNode) -> Result<(), RustSyntaxError> {
    for child in chain.children() {
        if child.name().as_ref() == "LetCondition" {
            let scrutinee = child.last_child().ok_or_else(|| {
                RustSyntaxError::new(child.to(), "an expression after `=` in a let condition")
            })?;
            validate_let_chain_operand(&scrutinee)?;
        } else if child.node_type().is_name("Expression") {
            validate_let_chain_operand(&child)?;
        }
    }
    Ok(())
}

fn validate_let_chain_operand(operand: &SyntaxNode) -> Result<(), RustSyntaxError> {
    let excluded = match operand.name().as_ref() {
        "AssignmentExpression" | "RangeExpression" | "StructExpression" => true,
        "BinaryExpression" => operand.child_by_name("LogicOp").is_some(),
        _ => false,
    };
    if excluded {
        return Err(RustSyntaxError::new(
            operand.from(),
            "a let-chain operand without a top-level lazy boolean, range, assignment, or struct expression",
        ));
    }
    Ok(())
}
