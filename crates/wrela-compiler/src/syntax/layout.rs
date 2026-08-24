use super::*;

pub(super) fn scan_layout_and_delimiters(
    file: &ProjectFile,
    lexemes: &[Lexeme],
    elements: &mut Vec<SyntaxElement>,
    diagnostics: &mut Vec<Diagnostic>,
    cancellation: &Cancellation,
) -> bool {
    let bytes = file.bytes();
    let path = file.path_arc();
    let mut indent_stack = vec![0_usize];
    let mut delimiter_stack: Vec<(u8, usize)> = Vec::new();
    let mut expected_block = false;
    let mut offset = 0;
    let text_ranges = lexemes
        .iter()
        .filter(|lexeme| lexeme.kind == TokenKind::TextLiteral)
        .map(|lexeme| (lexeme.range.start(), lexeme.range.end()))
        .collect::<Vec<_>>();
    let mut text_index = 0;

    while offset < bytes.len() {
        if cancellation.is_cancelled() {
            return true;
        }
        let Some(physical_end) = physical_line_end(bytes, offset, bytes.len(), cancellation) else {
            return true;
        };
        let content_end = physical_content_end(bytes, offset, physical_end);
        while text_ranges
            .get(text_index)
            .is_some_and(|(_, end)| *end <= offset as u64)
        {
            text_index += 1;
        }
        if text_ranges
            .get(text_index)
            .is_some_and(|(start, end)| *start < offset as u64 && offset as u64 <= *end)
        {
            offset = physical_end;
            continue;
        }
        let content = &bytes[offset..content_end];
        let Some(leading) = leading_spaces(content, cancellation) else {
            return true;
        };
        let mut significant_end = content.len();
        for (index, byte) in content[leading..].iter().enumerate() {
            if index.is_multiple_of(256) && cancellation.is_cancelled() {
                return true;
            }
            let absolute = u64::try_from(offset + leading + index).unwrap_or(u64::MAX);
            while text_ranges
                .get(text_index)
                .is_some_and(|(_, end)| *end <= absolute)
            {
                text_index += 1;
            }
            let inside_text = text_ranges
                .get(text_index)
                .is_some_and(|(start, end)| *start <= absolute && absolute < *end);
            if *byte == b'#' && !inside_text {
                significant_end = leading + index;
                break;
            }
        }
        while significant_end > leading && content[significant_end - 1].is_ascii_whitespace() {
            if (significant_end - leading).is_multiple_of(256) && cancellation.is_cancelled() {
                return true;
            }
            significant_end -= 1;
        }
        let significant = &content[leading..significant_end];
        let blank = significant.is_empty();

        if !blank && delimiter_stack.is_empty() {
            let current = *indent_stack.last().expect("indent stack has base");
            if leading % 4 != 0 {
                diagnostics.push(Diagnostic::new(
                    "syntax.invalid_indentation_width",
                    SourceRange::new_shared(path, offset, offset + leading),
                    RecoveryAction::SkippedToBoundary,
                ));
            }
            if leading > current {
                if expected_block && leading == current + 4 {
                    indent_stack.push(leading);
                    elements.push(SyntaxElement::new(
                        SyntaxElementKind::Layout(SyntaxLayoutKind::Indent),
                        path,
                        offset + leading,
                        offset + leading,
                    ));
                } else {
                    diagnostics.push(Diagnostic::new(
                        "syntax.unexpected_indentation",
                        SourceRange::new_shared(path, offset, offset + leading),
                        RecoveryAction::SkippedToBoundary,
                    ));
                    indent_stack.push(leading);
                    elements.push(SyntaxElement::new(
                        SyntaxElementKind::Error(SyntaxErrorKind::UnexpectedIndentBlock),
                        path,
                        offset + leading,
                        offset + leading,
                    ));
                }
            } else {
                if expected_block && leading <= current {
                    diagnostics.push(Diagnostic::new(
                        "syntax.missing_block",
                        SourceRange::new_shared(path, offset + leading, offset + leading),
                        RecoveryAction::InsertedMissing {
                            expected: "indented block".into(),
                        },
                    ));
                    elements.push(SyntaxElement::new(
                        SyntaxElementKind::Missing(SyntaxMissingKind::Block),
                        path,
                        offset + leading,
                        offset + leading,
                    ));
                }
                if leading < current {
                    while indent_stack.last().is_some_and(|indent| *indent > leading) {
                        indent_stack.pop();
                        elements.push(SyntaxElement::new(
                            SyntaxElementKind::Layout(SyntaxLayoutKind::Dedent),
                            path,
                            offset + leading,
                            offset + leading,
                        ));
                    }
                    if indent_stack.last().copied() != Some(leading) {
                        diagnostics.push(Diagnostic::new(
                            "syntax.inconsistent_dedent",
                            SourceRange::new_shared(path, offset, offset + leading),
                            RecoveryAction::SkippedToBoundary,
                        ));
                    }
                }
            }
        }

        if scan_line_delimiters(
            significant,
            offset + leading,
            path,
            &mut delimiter_stack,
            diagnostics,
            cancellation,
        ) {
            return true;
        }
        if !blank && delimiter_stack.is_empty() {
            expected_block = significant.last() == Some(&b':');
        }
        offset = physical_end;
    }

    if expected_block && delimiter_stack.is_empty() {
        diagnostics.push(Diagnostic::new(
            "syntax.missing_block",
            SourceRange::new_shared(path, bytes.len(), bytes.len()),
            RecoveryAction::InsertedMissing {
                expected: "indented block".into(),
            },
        ));
        elements.push(SyntaxElement::new(
            SyntaxElementKind::Missing(SyntaxMissingKind::Block),
            path,
            bytes.len(),
            bytes.len(),
        ));
    }
    while indent_stack.len() > 1 {
        indent_stack.pop();
        elements.push(SyntaxElement::new(
            SyntaxElementKind::Layout(SyntaxLayoutKind::Dedent),
            path,
            bytes.len(),
            bytes.len(),
        ));
    }
    for (opener, opener_offset) in delimiter_stack {
        diagnostics.push(
            Diagnostic::new(
                "syntax.missing_closer",
                SourceRange::new_shared(path, bytes.len(), bytes.len()),
                RecoveryAction::InsertedMissing {
                    expected: match opener {
                        b'(' => ")".into(),
                        b'[' => "]".into(),
                        _ => "closer".into(),
                    },
                },
            )
            .with_unsigned_parameter("opener_offset", opener_offset as u128),
        );
        elements.push(SyntaxElement::new(
            SyntaxElementKind::Missing(SyntaxMissingKind::Closer),
            path,
            bytes.len(),
            bytes.len(),
        ));
    }
    false
}

fn scan_line_delimiters(
    line: &[u8],
    base: usize,
    path: &Arc<str>,
    stack: &mut Vec<(u8, usize)>,
    diagnostics: &mut Vec<Diagnostic>,
    cancellation: &Cancellation,
) -> bool {
    let mut quote = None;
    let mut escaped = false;
    for (index, byte) in line.iter().copied().enumerate() {
        if index.is_multiple_of(256) && cancellation.is_cancelled() {
            return true;
        }
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
                        SourceRange::new_shared(path, base + index, base + index + 1),
                        RecoveryAction::PreservedInvalidBytes,
                    ));
                } else {
                    stack.push((byte, base + index));
                }
            }
            b')' | b']' => {
                let expected = if byte == b')' { b'(' } else { b'[' };
                if stack.last().is_some_and(|(opener, _)| *opener == expected) {
                    stack.pop();
                } else {
                    diagnostics.push(Diagnostic::new(
                        "syntax.unmatched_closer",
                        SourceRange::new_shared(path, base + index, base + index + 1),
                        RecoveryAction::SkippedToBoundary,
                    ));
                }
            }
            _ => {}
        }
    }
    false
}
