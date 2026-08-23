#![forbid(unsafe_code)]

use crate::{
    Cancellation, Diagnostic, ProjectFile, RecoveryAction, SourceRange, SyntaxElement,
    SyntaxElementKind,
};

const MAX_SOURCE_BYTES: usize = 1_048_576;
const MAX_NESTING: usize = 256;

pub(crate) struct ParsedSource {
    pub(crate) elements: Vec<SyntaxElement>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) imports: Vec<Import>,
    pub(crate) declarations: Vec<Declaration>,
    pub(crate) cancelled: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Declaration {
    pub(crate) kind: &'static str,
    pub(crate) name: String,
    pub(crate) public: bool,
    pub(crate) range: SourceRange,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct Import {
    pub(crate) target_path: String,
    pub(crate) alias: String,
    pub(crate) range: SourceRange,
}

pub(crate) fn parse(file: &ProjectFile, cancellation: &Cancellation) -> ParsedSource {
    let bytes = file.bytes();
    let path = file.path();
    if bytes.len() > MAX_SOURCE_BYTES {
        return ParsedSource {
            elements: vec![SyntaxElement::new(
                SyntaxElementKind::Invalid,
                "oversized_source",
                path,
                0,
                bytes.len(),
            )],
            diagnostics: vec![Diagnostic::new(
                "syntax.source_too_large",
                SourceRange::new(path, MAX_SOURCE_BYTES, bytes.len()),
                RecoveryAction::PreservedInvalidBytes,
            )],
            imports: Vec::new(),
            declarations: Vec::new(),
            cancelled: false,
        };
    }
    let mut elements = Vec::new();
    let mut diagnostics = Vec::new();
    let imports = scan_imports(file, &mut diagnostics);
    let declarations = scan_declarations(file);
    validate_top_level(file, &mut diagnostics);
    let mut offset = 0;

    while offset < bytes.len() {
        if offset % 256 == 0 && cancellation.is_cancelled() {
            return ParsedSource {
                elements,
                diagnostics,
                imports,
                declarations,
                cancelled: true,
            };
        }
        let start = offset;
        let (kind, name) = match bytes[offset] {
            b' ' => {
                offset += 1;
                while bytes.get(offset) == Some(&b' ') {
                    offset += 1;
                }
                (SyntaxElementKind::Trivia, "whitespace")
            }
            b'\t' => {
                offset += 1;
                diagnostics.push(Diagnostic::new(
                    "syntax.tab_outside_literal",
                    SourceRange::new(path, start, offset),
                    RecoveryAction::PreservedInvalidBytes,
                ));
                (SyntaxElementKind::Invalid, "invalid_tab")
            }
            b'\n' => {
                offset += 1;
                (SyntaxElementKind::Trivia, "line_ending")
            }
            b'\r' if bytes.get(offset + 1) == Some(&b'\n') => {
                offset += 2;
                (SyntaxElementKind::Trivia, "line_ending")
            }
            b'\r' => {
                offset += 1;
                diagnostics.push(Diagnostic::new(
                    "syntax.bare_carriage_return",
                    SourceRange::new(path, start, offset),
                    RecoveryAction::PreservedInvalidBytes,
                ));
                (SyntaxElementKind::Invalid, "invalid_line_ending")
            }
            b'#' => {
                offset += 1;
                while offset < bytes.len() && !matches!(bytes[offset], b'\r' | b'\n') {
                    offset += 1;
                }
                (SyntaxElementKind::Trivia, "comment")
            }
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                offset += 1;
                while bytes
                    .get(offset)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                {
                    offset += 1;
                }
                (SyntaxElementKind::Token, "word")
            }
            b'0'..=b'9' => {
                offset += 1;
                while bytes
                    .get(offset)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.'))
                {
                    offset += 1;
                }
                (SyntaxElementKind::Token, "number")
            }
            quote @ (b'\'' | b'"') => {
                offset += 1;
                let mut escaped = false;
                let mut closed = false;
                while offset < bytes.len() {
                    let byte = bytes[offset];
                    if matches!(byte, b'\r' | b'\n') {
                        break;
                    }
                    offset += 1;
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == quote {
                        closed = true;
                        break;
                    }
                }
                if closed && std::str::from_utf8(&bytes[start..offset]).is_ok() {
                    (SyntaxElementKind::Token, "literal")
                } else {
                    diagnostics.push(Diagnostic::new(
                        if closed {
                            "syntax.invalid_encoding"
                        } else {
                            "syntax.unclosed_literal"
                        },
                        SourceRange::new(path, start, offset),
                        RecoveryAction::PreservedInvalidBytes,
                    ));
                    (SyntaxElementKind::Invalid, "invalid_literal")
                }
            }
            byte if byte.is_ascii_punctuation() => {
                offset += 1;
                let previous = bytes.get(offset - 1);
                if (matches!(previous, Some(b'-' | b'=' | b'!' | b'<' | b'>'))
                    && bytes.get(offset) == Some(&b'='))
                    || (previous == Some(&b'-') && bytes.get(offset) == Some(&b'>'))
                {
                    offset += 1;
                }
                (SyntaxElementKind::Token, "punctuation")
            }
            byte => {
                let (extent, code) = if byte.is_ascii() {
                    (1, "syntax.invalid_character")
                } else {
                    match std::str::from_utf8(&bytes[offset..]) {
                        Ok(valid) => (
                            valid.chars().next().map_or(1, char::len_utf8),
                            "syntax.invalid_character",
                        ),
                        Err(error) if error.valid_up_to() > 0 => {
                            let valid =
                                std::str::from_utf8(&bytes[offset..offset + error.valid_up_to()])
                                    .expect("validated prefix");
                            (
                                valid.chars().next().map_or(1, char::len_utf8),
                                "syntax.invalid_character",
                            )
                        }
                        Err(error) => (
                            error.error_len().unwrap_or(bytes.len() - offset),
                            "syntax.invalid_encoding",
                        ),
                    }
                };
                offset += extent;
                diagnostics.push(Diagnostic::new(
                    code,
                    SourceRange::new(path, start, offset),
                    RecoveryAction::PreservedInvalidBytes,
                ));
                (SyntaxElementKind::Invalid, "invalid_byte")
            }
        };
        elements.push(SyntaxElement::new(kind, name, path, start, offset));
    }

    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        diagnostics.push(
            Diagnostic::new(
                "syntax.byte_order_mark",
                SourceRange::new(path, 0, 3),
                RecoveryAction::PreservedInvalidBytes,
            )
            .with_parameter("encoding", "utf-8"),
        );
    }

    scan_layout_and_delimiters(file, &mut elements, &mut diagnostics);
    elements.sort_by(|left, right| {
        left.range()
            .start()
            .cmp(&right.range().start())
            .then(left.range().end().cmp(&right.range().end()))
    });
    if diagnostics.len() > 64 {
        diagnostics.truncate(64);
        diagnostics.push(Diagnostic::new(
            "syntax.diagnostics_truncated",
            SourceRange::new(path, bytes.len(), bytes.len()),
            RecoveryAction::TruncatedDiagnostics,
        ));
    }

    ParsedSource {
        elements,
        diagnostics,
        imports,
        declarations,
        cancelled: false,
    }
}

fn validate_top_level(file: &ProjectFile, diagnostics: &mut Vec<Diagnostic>) {
    let mut offset = 0;
    for physical in file.bytes().split_inclusive(|byte| *byte == b'\n') {
        let mut line = physical.strip_suffix(b"\n").unwrap_or(physical);
        line = line.strip_suffix(b"\r").unwrap_or(line);
        let accepted = std::str::from_utf8(line).is_err()
            || line.is_empty()
            || line.starts_with(b" ")
            || line.starts_with(b"\t")
            || line.starts_with(b"#")
            || line.starts_with(b"@")
            || line.starts_with(b"from ")
            || line.starts_with(b"comptime assert ")
            || line.starts_with(b"comptime if ")
            || std::str::from_utf8(line)
                .ok()
                .and_then(declaration_header)
                .is_some();
        if !accepted {
            diagnostics.push(Diagnostic::new(
                "syntax.unexpected_top_level",
                SourceRange::new(file.path(), offset, offset + line.len()),
                RecoveryAction::SkippedToBoundary,
            ));
        }
        offset += physical.len();
    }
}

fn scan_declarations(file: &ProjectFile) -> Vec<Declaration> {
    let mut starts = Vec::new();
    let mut offset = 0;
    for physical in file.bytes().split_inclusive(|byte| *byte == b'\n') {
        let mut line = physical.strip_suffix(b"\n").unwrap_or(physical);
        line = line.strip_suffix(b"\r").unwrap_or(line);
        if !line.starts_with(b" ")
            && !line.starts_with(b"\t")
            && let Ok(text) = std::str::from_utf8(line)
            && let Some((kind, name, public)) = declaration_header(text)
        {
            starts.push((offset, offset + line.len(), kind, name, public));
        }
        offset += physical.len();
    }
    let source_len = file.bytes().len();
    starts
        .iter()
        .enumerate()
        .map(|(index, (start, header_end, kind, name, public))| {
            let end = starts.get(index + 1).map_or(source_len, |(next, ..)| *next);
            Declaration {
                kind,
                name: name.clone(),
                public: *public,
                range: SourceRange::new(file.path(), *start, *header_end),
                start: *start,
                end,
            }
        })
        .collect()
}

fn declaration_header(line: &str) -> Option<(&'static str, String, bool)> {
    let mut words = line.split_ascii_whitespace().peekable();
    let public = matches!(words.peek(), Some(&"pub"));
    if public {
        words.next();
    }
    if matches!(words.peek(), Some(&"pure" | &"async")) {
        words.next();
    }
    let first = words.next()?;
    let (kind, raw_name) = match first {
        "fn" => ("function", words.next()?),
        "const" => ("constant", words.next()?),
        "pool" => ("pool", words.next()?),
        "type" => ("type_alias", words.next()?),
        "struct" => ("struct", words.next()?),
        "enum" => ("enum", words.next()?),
        "interface" => ("interface", words.next()?),
        "suite" => ("suite", words.next()?),
        "resource" if words.next() == Some("struct") => ("resource_struct", words.next()?),
        _ => return None,
    };
    let name = raw_name
        .split(['(', '[', ':', '='])
        .next()
        .unwrap_or_default();
    valid_identifier(name).then(|| (kind, name.to_owned(), public))
}

fn scan_layout_and_delimiters(
    file: &ProjectFile,
    elements: &mut Vec<SyntaxElement>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let bytes = file.bytes();
    let path = file.path();
    let mut indent_stack = vec![0_usize];
    let mut delimiter_stack: Vec<(u8, usize)> = Vec::new();
    let mut expected_block = false;
    let mut offset = 0;

    for physical in bytes.split_inclusive(|byte| *byte == b'\n') {
        let mut content = physical.strip_suffix(b"\n").unwrap_or(physical);
        content = content.strip_suffix(b"\r").unwrap_or(content);
        let leading = content.iter().take_while(|byte| **byte == b' ').count();
        let significant = content[leading..]
            .iter()
            .take_while(|byte| **byte != b'#')
            .copied()
            .collect::<Vec<_>>();
        let significant = trim_ascii(&significant);
        let blank = significant.is_empty();

        if !blank && delimiter_stack.is_empty() {
            let current = *indent_stack.last().expect("indent stack has base");
            if leading % 4 != 0 {
                diagnostics.push(Diagnostic::new(
                    "syntax.invalid_indentation_width",
                    SourceRange::new(path, offset, offset + leading),
                    RecoveryAction::SkippedToBoundary,
                ));
            }
            if leading > current {
                if expected_block && leading == current + 4 {
                    indent_stack.push(leading);
                    elements.push(SyntaxElement::new(
                        SyntaxElementKind::Layout,
                        "indent",
                        path,
                        offset + leading,
                        offset + leading,
                    ));
                } else {
                    diagnostics.push(Diagnostic::new(
                        "syntax.unexpected_indentation",
                        SourceRange::new(path, offset, offset + leading),
                        RecoveryAction::SkippedToBoundary,
                    ));
                    indent_stack.push(leading);
                    elements.push(SyntaxElement::new(
                        SyntaxElementKind::Error,
                        "unexpected_indent_block",
                        path,
                        offset + leading,
                        offset + leading,
                    ));
                }
            } else {
                if expected_block && leading <= current {
                    diagnostics.push(Diagnostic::new(
                        "syntax.missing_block",
                        SourceRange::new(path, offset + leading, offset + leading),
                        RecoveryAction::InsertedMissing {
                            expected: "indented block".into(),
                        },
                    ));
                    elements.push(SyntaxElement::new(
                        SyntaxElementKind::Missing,
                        "missing_block",
                        path,
                        offset + leading,
                        offset + leading,
                    ));
                }
                if leading < current {
                    while indent_stack.last().is_some_and(|indent| *indent > leading) {
                        indent_stack.pop();
                        elements.push(SyntaxElement::new(
                            SyntaxElementKind::Layout,
                            "dedent",
                            path,
                            offset + leading,
                            offset + leading,
                        ));
                    }
                    if indent_stack.last().copied() != Some(leading) {
                        diagnostics.push(Diagnostic::new(
                            "syntax.inconsistent_dedent",
                            SourceRange::new(path, offset, offset + leading),
                            RecoveryAction::SkippedToBoundary,
                        ));
                    }
                }
            }
        }

        scan_line_delimiters(
            significant,
            offset + leading,
            path,
            &mut delimiter_stack,
            diagnostics,
        );
        if !blank && delimiter_stack.is_empty() {
            expected_block = significant.last() == Some(&b':');
        }
        offset += physical.len();
    }

    if expected_block && delimiter_stack.is_empty() {
        diagnostics.push(Diagnostic::new(
            "syntax.missing_block",
            SourceRange::new(path, bytes.len(), bytes.len()),
            RecoveryAction::InsertedMissing {
                expected: "indented block".into(),
            },
        ));
        elements.push(SyntaxElement::new(
            SyntaxElementKind::Missing,
            "missing_block",
            path,
            bytes.len(),
            bytes.len(),
        ));
    }
    while indent_stack.len() > 1 {
        indent_stack.pop();
        elements.push(SyntaxElement::new(
            SyntaxElementKind::Layout,
            "dedent",
            path,
            bytes.len(),
            bytes.len(),
        ));
    }
    for (opener, opener_offset) in delimiter_stack {
        diagnostics.push(
            Diagnostic::new(
                "syntax.missing_closer",
                SourceRange::new(path, bytes.len(), bytes.len()),
                RecoveryAction::InsertedMissing {
                    expected: match opener {
                        b'(' => ")".into(),
                        b'[' => "]".into(),
                        _ => "closer".into(),
                    },
                },
            )
            .with_parameter("opener_offset", opener_offset.to_string()),
        );
        elements.push(SyntaxElement::new(
            SyntaxElementKind::Missing,
            "missing_closer",
            path,
            bytes.len(),
            bytes.len(),
        ));
    }
}

fn scan_line_delimiters(
    line: &[u8],
    base: usize,
    path: &str,
    stack: &mut Vec<(u8, usize)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut quote = None;
    let mut escaped = false;
    for (index, byte) in line.iter().copied().enumerate() {
        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == active_quote {
                quote = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'(' | b'[' => {
                if stack.len() == MAX_NESTING {
                    diagnostics.push(Diagnostic::new(
                        "syntax.nesting_limit_exceeded",
                        SourceRange::new(path, base + index, base + index + 1),
                        RecoveryAction::PreservedInvalidBytes,
                    ));
                }
                stack.push((byte, base + index));
            }
            b')' | b']' => {
                let expected = if byte == b')' { b'(' } else { b'[' };
                if stack.last().is_some_and(|(opener, _)| *opener == expected) {
                    stack.pop();
                } else {
                    diagnostics.push(Diagnostic::new(
                        "syntax.unmatched_closer",
                        SourceRange::new(path, base + index, base + index + 1),
                        RecoveryAction::SkippedToBoundary,
                    ));
                }
            }
            _ => {}
        }
    }
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn scan_imports(file: &ProjectFile, diagnostics: &mut Vec<Diagnostic>) -> Vec<Import> {
    let mut imports = Vec::new();
    let mut offset = 0;
    let mut declarations_started = false;
    for line in file.bytes().split_inclusive(|byte| *byte == b'\n') {
        let content_len = line
            .strip_suffix(b"\n")
            .unwrap_or(line)
            .strip_suffix(b"\r")
            .unwrap_or_else(|| line.strip_suffix(b"\n").unwrap_or(line))
            .len();
        let content = &line[..content_len];
        let end = offset + content.len();
        if content.is_empty() || content.iter().all(|byte| byte.is_ascii_whitespace()) {
            offset += line.len();
            continue;
        }
        if content.first() == Some(&b'#') {
            offset += line.len();
            continue;
        }
        if content.starts_with(b"from ") {
            if declarations_started {
                diagnostics.push(Diagnostic::new(
                    "syntax.import_after_declaration",
                    SourceRange::new(file.path(), offset, end),
                    RecoveryAction::SkippedToBoundary,
                ));
            }
            match parse_import_line(content) {
                Some((target_path, alias)) => imports.push(Import {
                    target_path,
                    alias,
                    range: SourceRange::new(file.path(), offset, end),
                }),
                None => diagnostics.push(Diagnostic::new(
                    "syntax.malformed_import",
                    SourceRange::new(file.path(), offset, end),
                    RecoveryAction::SkippedToBoundary,
                )),
            }
        } else {
            declarations_started = true;
        }
        offset += line.len();
    }
    imports
}

fn parse_import_line(line: &[u8]) -> Option<(String, String)> {
    let text = std::str::from_utf8(line).ok()?;
    let words: Vec<_> = text.split_ascii_whitespace().collect();
    let (parent, leaf, alias) = match words.as_slice() {
        ["from", parent, "import", leaf] => (*parent, *leaf, *leaf),
        ["from", parent, "import", leaf, "as", alias] if valid_identifier(alias) => {
            (*parent, *leaf, *alias)
        }
        _ => return None,
    };
    if !parent.split('.').all(valid_path_segment) || !valid_path_segment(leaf) {
        return None;
    }
    Some((
        format!("src/{}/{}.wr", parent.replace('.', "/"), leaf),
        alias.to_owned(),
    ))
}

fn valid_path_segment(segment: &str) -> bool {
    let mut bytes = segment.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_identifier(identifier: &str) -> bool {
    let mut bytes = identifier.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}
