use super::*;

pub(super) fn classify_token_bytes(bytes: &[u8]) -> TokenKind {
    match bytes {
        b"fn" => TokenKind::Fn,
        b"pub" => TokenKind::Pub,
        b"pure" => TokenKind::Pure,
        b"async" => TokenKind::Async,
        b"any" => TokenKind::Any,
        b"return" => TokenKind::Return,
        b"break" => TokenKind::Break,
        b"case" => TokenKind::Case,
        b"continue" => TokenKind::Continue,
        b"defer" => TokenKind::Defer,
        b"for" => TokenKind::For,
        b"if" => TokenKind::If,
        b"elif" => TokenKind::Elif,
        b"else" => TokenKind::Else,
        b"const" => TokenKind::Const,
        b"struct" => TokenKind::Struct,
        b"resource" => TokenKind::Resource,
        b"enum" => TokenKind::Enum,
        b"interface" => TokenKind::Interface,
        b"type" => TokenKind::Type,
        b"pool" => TokenKind::Pool,
        b"suite" => TokenKind::Suite,
        b"test" => TokenKind::Test,
        b"from" => TokenKind::From,
        b"import" => TokenKind::Import,
        b"as" => TokenKind::As,
        b"comptime" => TokenKind::Comptime,
        b"assert" => TokenKind::Assert,
        b"in" => TokenKind::In,
        b"is" => TokenKind::Is,
        b"match" => TokenKind::Match,
        b"and" => TokenKind::And,
        b"or" => TokenKind::Or,
        b"not" => TokenKind::Not,
        b"await" => TokenKind::Await,
        b"own" => TokenKind::Own,
        b"panic" => TokenKind::Panic,
        b"pass" => TokenKind::Pass,
        b"take" => TokenKind::Take,
        b"read" => TokenKind::Read,
        b"mut" => TokenKind::Mut,
        b"self" => TokenKind::SelfValue,
        b"implements" => TokenKind::Implements,
        b"expect" => TokenKind::Expect,
        b"send" => TokenKind::Send,
        b"try_send" => TokenKind::TrySend,
        b"while" => TokenKind::While,
        b"with" => TokenKind::With,
        b"true" => TokenKind::True,
        b"false" => TokenKind::False,
        b"(" => TokenKind::LeftParen,
        b")" => TokenKind::RightParen,
        b"[" => TokenKind::LeftBracket,
        b"]" => TokenKind::RightBracket,
        b":" => TokenKind::Colon,
        b"," => TokenKind::Comma,
        b"." => TokenKind::Dot,
        b"@" => TokenKind::At,
        b"->" => TokenKind::Arrow,
        b"=" => TokenKind::Equal,
        b"==" => TokenKind::EqualEqual,
        b"!=" => TokenKind::BangEqual,
        b"<" => TokenKind::Less,
        b"<=" => TokenKind::LessEqual,
        b">" => TokenKind::Greater,
        b">=" => TokenKind::GreaterEqual,
        b"+" => TokenKind::Plus,
        b"-" => TokenKind::Minus,
        b"*" => TokenKind::Star,
        b"/" => TokenKind::Slash,
        b"%" => TokenKind::Percent,
        b"&" => TokenKind::Ampersand,
        b"|" => TokenKind::Pipe,
        b"^" => TokenKind::Caret,
        b"~" => TokenKind::Tilde,
        b"<<" => TokenKind::ShiftLeft,
        b">>" => TokenKind::ShiftRight,
        b".." => TokenKind::Range,
        b"..=" => TokenKind::RangeInclusive,
        b";" => TokenKind::Semicolon,
        b"?" => TokenKind::Question,
        [b'0'..=b'9', ..]
            if bytes.contains(&b'.') || bytes.contains(&b'e') || bytes.contains(&b'E') =>
        {
            TokenKind::FloatLiteral
        }
        [b'0'..=b'9', ..] => TokenKind::IntegerLiteral,
        [b'"' | b'\'', ..] => TokenKind::TextLiteral,
        [b'A'..=b'Z' | b'a'..=b'z' | b'_', ..] => TokenKind::Identifier,
        _ => TokenKind::Invalid,
    }
}

pub(super) fn punctuation_width(bytes: &[u8]) -> usize {
    if bytes.starts_with(b"..=") {
        3
    } else if matches!(
        bytes.get(..2),
        Some(b"->" | b"==" | b"!=" | b"<=" | b">=" | b"<<" | b">>" | b"..")
    ) {
        2
    } else {
        1
    }
}

pub(super) fn numeric_token_end(bytes: &[u8], start: usize, cancellation: &Cancellation) -> usize {
    let mut offset = start + 1;
    if bytes.get(start) == Some(&b'.') {
        while bytes
            .get(offset)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            if offset.is_multiple_of(256) && cancellation.is_cancelled() {
                return offset;
            }
            offset += 1;
        }
        return offset;
    }
    let based = bytes.get(start) == Some(&b'0')
        && matches!(
            bytes.get(offset),
            Some(b'x' | b'X' | b'b' | b'B' | b'o' | b'O')
        );
    if based {
        offset += 1;
        while bytes
            .get(offset)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            if offset.is_multiple_of(256) && cancellation.is_cancelled() {
                return offset;
            }
            offset += 1;
        }
        return offset;
    }
    while bytes
        .get(offset)
        .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_')
    {
        if offset.is_multiple_of(256) && cancellation.is_cancelled() {
            return offset;
        }
        offset += 1;
    }
    if bytes.get(offset) == Some(&b'.') && bytes.get(offset + 1) != Some(&b'.') {
        offset += 1;
        while bytes
            .get(offset)
            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_')
        {
            if offset.is_multiple_of(256) && cancellation.is_cancelled() {
                return offset;
            }
            offset += 1;
        }
    }
    if matches!(bytes.get(offset), Some(b'e' | b'E')) {
        offset += 1;
        if matches!(bytes.get(offset), Some(b'+' | b'-')) {
            offset += 1;
        }
        while bytes
            .get(offset)
            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_')
        {
            if offset.is_multiple_of(256) && cancellation.is_cancelled() {
                return offset;
            }
            offset += 1;
        }
    }
    while bytes.get(offset).is_some_and(u8::is_ascii_alphanumeric) {
        if offset.is_multiple_of(256) && cancellation.is_cancelled() {
            return offset;
        }
        offset += 1;
    }
    offset
}

pub(super) fn classify_numeric_literal(bytes: &[u8]) -> Option<TokenKind> {
    let source = std::str::from_utf8(bytes).ok()?;
    if let Some(body) = source.strip_prefix("0x") {
        return valid_based_integer(body, 16).then_some(TokenKind::IntegerLiteral);
    }
    if let Some(body) = source.strip_prefix("0b") {
        return valid_based_integer(body, 2).then_some(TokenKind::IntegerLiteral);
    }
    if let Some(body) = source.strip_prefix("0o") {
        return valid_based_integer(body, 8).then_some(TokenKind::IntegerLiteral);
    }
    if source.starts_with("0X") || source.starts_with("0B") || source.starts_with("0O") {
        return None;
    }

    let integer_suffixes = ["u16", "u32", "u64", "i16", "i32", "i64", "u8", "i8"];
    let float_suffixes = ["f16", "f32", "f64"];
    let integer_suffix = integer_suffixes
        .iter()
        .find_map(|suffix| source.strip_suffix(suffix));
    let float_suffix = float_suffixes
        .iter()
        .find_map(|suffix| source.strip_suffix(suffix));
    let (body, suffix_kind) = if let Some(body) = integer_suffix {
        (body, Some(TokenKind::IntegerLiteral))
    } else if let Some(body) = float_suffix {
        (body, Some(TokenKind::FloatLiteral))
    } else {
        (source, None)
    };

    let mut exponent_parts = body.split(['e', 'E']);
    let mantissa = exponent_parts.next()?;
    let exponent = exponent_parts.next();
    if exponent_parts.next().is_some() {
        return None;
    }
    if let Some(exponent) = exponent {
        let digits = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        if !valid_separated_digits(digits, 10) {
            return None;
        }
    }

    let mut decimal_parts = mantissa.split('.');
    let whole = decimal_parts.next()?;
    let fraction = decimal_parts.next();
    if decimal_parts.next().is_some()
        || !valid_separated_digits(whole, 10)
        || fraction.is_some_and(|digits| !valid_separated_digits(digits, 10))
    {
        return None;
    }
    let float_shape = fraction.is_some() || exponent.is_some();
    match suffix_kind {
        Some(TokenKind::IntegerLiteral) if float_shape => None,
        Some(kind) => Some(kind),
        None if float_shape => Some(TokenKind::FloatLiteral),
        None => Some(TokenKind::IntegerLiteral),
    }
}

fn valid_based_integer(body: &str, radix: u32) -> bool {
    let integer_suffixes = ["u16", "u32", "u64", "i16", "i32", "i64", "u8", "i8"];
    let digits = integer_suffixes
        .iter()
        .find_map(|suffix| body.strip_suffix(suffix))
        .unwrap_or(body);
    valid_separated_digits(digits, radix)
}

fn valid_separated_digits(source: &str, radix: u32) -> bool {
    let characters = source.as_bytes();
    !characters.is_empty()
        && characters.iter().enumerate().all(|(index, byte)| {
            if *byte == b'_' {
                index > 0
                    && index + 1 < characters.len()
                    && (characters[index - 1] as char).is_digit(radix)
                    && (characters[index + 1] as char).is_digit(radix)
            } else {
                (*byte as char).is_digit(radix)
            }
        })
}

pub(super) fn utf8_scalar_extent(bytes: &[u8], offset: usize) -> (usize, &'static str) {
    let first = bytes[offset];
    let expected = match first {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return (1, "syntax.invalid_encoding"),
    };
    let end = offset.saturating_add(expected).min(bytes.len());
    if end - offset == expected && std::str::from_utf8(&bytes[offset..end]).is_ok() {
        (expected, "syntax.invalid_character")
    } else {
        (1, "syntax.invalid_encoding")
    }
}

pub(super) fn decode_text_literal(bytes: &[u8]) -> Option<String> {
    let interior = bytes.strip_prefix(b"\"")?.strip_suffix(b"\"")?;
    decode_text_content(interior)
}

pub(super) fn decode_scalar_literal(bytes: &[u8]) -> Option<char> {
    let interior = bytes.strip_prefix(b"'")?.strip_suffix(b"'")?;
    let decoded = decode_text_content(interior)?;
    let mut scalars = decoded.chars();
    let scalar = scalars.next()?;
    scalars.next().is_none().then_some(scalar)
}

fn decode_text_content(interior: &[u8]) -> Option<String> {
    let source = std::str::from_utf8(interior).ok()?;
    let mut characters = source.chars();
    let mut decoded = String::new();
    while let Some(character) = characters.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        match characters.next()? {
            '\\' => decoded.push('\\'),
            '"' => decoded.push('"'),
            '\'' => decoded.push('\''),
            'n' => decoded.push('\n'),
            'r' => decoded.push('\r'),
            't' => decoded.push('\t'),
            '0' => decoded.push('\0'),
            'u' => {
                if characters.next()? != '{' {
                    return None;
                }
                let mut digits = String::new();
                loop {
                    let digit = characters.next()?;
                    if digit == '}' {
                        break;
                    }
                    if !digit.is_ascii_hexdigit() || digits.len() == 6 {
                        return None;
                    }
                    digits.push(digit);
                }
                if digits.is_empty() {
                    return None;
                }
                let scalar = u32::from_str_radix(&digits, 16).ok()?;
                decoded.push(char::from_u32(scalar)?);
            }
            _ => return None,
        }
    }
    Some(decoded)
}

pub(super) fn decode_bytes_literal(bytes: &[u8]) -> Option<Vec<u8>> {
    let interior = bytes.strip_prefix(b"b\"")?.strip_suffix(b"\"")?;
    let mut decoded = Vec::new();
    let mut index = 0;
    while index < interior.len() {
        let byte = interior[index];
        if !byte.is_ascii() {
            return None;
        }
        index += 1;
        if byte != b'\\' {
            decoded.push(byte);
            continue;
        }
        let escape = *interior.get(index)?;
        index += 1;
        match escape {
            b'\\' => decoded.push(b'\\'),
            b'"' => decoded.push(b'"'),
            b'n' => decoded.push(b'\n'),
            b'r' => decoded.push(b'\r'),
            b't' => decoded.push(b'\t'),
            b'0' => decoded.push(0),
            b'x' => {
                let high = (*interior.get(index)? as char).to_digit(16)?;
                let low = (*interior.get(index + 1)? as char).to_digit(16)?;
                decoded.push(u8::try_from(high * 16 + low).ok()?);
                index += 2;
            }
            _ => return None,
        }
    }
    Some(decoded)
}

pub(super) fn multiline_literal_end(
    bytes: &[u8],
    start: usize,
    cancellation: &Cancellation,
) -> (usize, bool) {
    let mut cursor = start.saturating_add(3);
    while cursor.saturating_add(3) <= bytes.len() {
        if cursor.is_multiple_of(256) && cancellation.is_cancelled() {
            return (cursor, false);
        }
        if bytes.get(cursor..cursor + 3) == Some(b"\"\"\"") {
            let line_start = bytes[..cursor]
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map_or(start + 3, |index| index + 1);
            let before_is_indent = bytes[line_start..cursor].iter().all(|byte| *byte == b' ');
            let line_end = bytes[cursor + 3..]
                .iter()
                .position(|byte| matches!(byte, b'\r' | b'\n'))
                .map_or(bytes.len(), |length| cursor + 3 + length);
            let after = &bytes[cursor + 3..line_end];
            let after = after.strip_prefix(b" ").unwrap_or(after);
            let after_is_valid = after.iter().all(|byte| *byte == b' ')
                || after
                    .iter()
                    .position(|byte| *byte == b'#')
                    .is_some_and(|comment| after[..comment].iter().all(|byte| *byte == b' '));
            if before_is_indent && after_is_valid {
                return (cursor + 3, true);
            }
        }
        cursor += 1;
    }
    (bytes.len(), false)
}

pub(super) fn decode_multiline_text_literal(bytes: &[u8]) -> Option<String> {
    let interior = bytes.strip_prefix(b"\"\"\"")?.strip_suffix(b"\"\"\"")?;
    let closer_line_start = interior
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let indent = &interior[closer_line_start..];
    if !indent.iter().all(|byte| *byte == b' ') {
        return None;
    }
    let mut content = &interior[..closer_line_start];
    content = content
        .strip_prefix(b"\r\n")
        .or_else(|| content.strip_prefix(b"\n"))
        .unwrap_or(content);
    let mut normalized = Vec::new();
    for line in content.split_inclusive(|byte| *byte == b'\n') {
        let (body, ending) = line.strip_suffix(b"\n").map_or((line, &b""[..]), |body| {
            let body = body.strip_suffix(b"\r").unwrap_or(body);
            (body, &b"\n"[..])
        });
        if body.iter().any(|byte| !byte.is_ascii_whitespace()) {
            normalized.extend_from_slice(body.strip_prefix(indent)?);
        }
        normalized.extend_from_slice(ending);
    }
    decode_text_content(&normalized)
}
