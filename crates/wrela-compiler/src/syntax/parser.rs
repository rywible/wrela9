use super::*;

pub(super) fn parse_imports(
    file: &ProjectFile,
    lexemes: &[Lexeme],
    diagnostics: &mut Vec<Diagnostic>,
    cancellation: &Cancellation,
) -> Option<Vec<Import>> {
    let mut imports = Vec::new();
    let mut offset = 0;
    let mut token_index = 0;
    let mut declarations_started = false;
    while offset < file.bytes().len() {
        if cancellation.is_cancelled() {
            return None;
        }
        let physical_end =
            physical_line_end(file.bytes(), offset, file.bytes().len(), cancellation)?;
        let content_end = physical_content_end(file.bytes(), offset, physical_end);
        let content = &file.bytes()[offset..content_end];
        let end = offset + content.len();
        while lexemes.get(token_index).is_some_and(|lexeme| {
            usize::try_from(lexeme.range.start()).is_ok_and(|start| start < offset)
        }) {
            token_index += 1;
        }
        let line_start = token_index;
        while lexemes.get(token_index).is_some_and(|lexeme| {
            usize::try_from(lexeme.range.start()).is_ok_and(|start| start < end)
        }) {
            token_index += 1;
        }
        let line_tokens = lexemes[line_start..token_index].iter().collect::<Vec<_>>();
        if line_tokens.is_empty() || content.first().is_some_and(u8::is_ascii_whitespace) {
            offset = physical_end;
            continue;
        }
        if line_tokens
            .first()
            .is_some_and(|token| token.kind == TokenKind::From)
        {
            if declarations_started {
                diagnostics.push(Diagnostic::new(
                    "syntax.import_after_declaration",
                    SourceRange::new_shared(file.path_arc(), offset, end),
                    RecoveryAction::SkippedToBoundary,
                ));
            }
            match parse_import_tokens(file, &line_tokens) {
                Some((target_path, alias)) => imports.push(Import {
                    target_path,
                    alias,
                    range: SourceRange::new_shared(file.path_arc(), offset, end),
                }),
                None => diagnostics.push(Diagnostic::new(
                    "syntax.malformed_import",
                    SourceRange::new_shared(file.path_arc(), offset, end),
                    RecoveryAction::SkippedToBoundary,
                )),
            }
        } else {
            declarations_started = true;
        }
        offset = physical_end;
    }
    Some(imports)
}

fn parse_import_tokens(file: &ProjectFile, tokens: &[&Lexeme]) -> Option<(String, String)> {
    let mut cursor = 0;
    (tokens.get(cursor)?.kind == TokenKind::From).then_some(())?;
    cursor += 1;
    let mut parent = Vec::new();
    loop {
        let segment_token = tokens.get(cursor)?;
        matches!(segment_token.kind, TokenKind::Identifier | TokenKind::Pool).then_some(())?;
        let segment = token_text(file, segment_token)?;
        valid_path_segment(segment).then_some(())?;
        parent.push(segment);
        cursor += 1;
        if tokens
            .get(cursor)
            .is_some_and(|token| token.kind == TokenKind::Dot)
        {
            cursor += 1;
        } else {
            break;
        }
    }
    (tokens.get(cursor)?.kind == TokenKind::Import).then_some(())?;
    cursor += 1;
    let leaf_token = tokens.get(cursor)?;
    matches!(leaf_token.kind, TokenKind::Identifier | TokenKind::Pool).then_some(())?;
    let leaf = token_text(file, leaf_token)?;
    valid_path_segment(leaf).then_some(())?;
    cursor += 1;
    let alias = if tokens
        .get(cursor)
        .is_some_and(|token| token.kind == TokenKind::As)
    {
        cursor += 1;
        let alias_token = tokens.get(cursor)?;
        (alias_token.kind == TokenKind::Identifier).then_some(())?;
        cursor += 1;
        token_text(file, alias_token)?
    } else {
        leaf
    };
    (cursor == tokens.len()).then_some(())?;
    Some((
        format!("src/{}/{}.wr", parent.join("/"), leaf),
        alias.to_owned(),
    ))
}

fn valid_path_segment(segment: &str) -> bool {
    let mut bytes = segment.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

pub(super) fn parse_declarations(
    file: &ProjectFile,
    lexemes: &[Lexeme],
    diagnostics: &mut Vec<Diagnostic>,
    cancellation: &Cancellation,
) -> Option<Vec<Declaration>> {
    let mut starts = Vec::new();
    let mut index = 0;
    let mut pending_attribute_start = None;
    while index < lexemes.len() {
        if index.is_multiple_of(256) && cancellation.is_cancelled() {
            return None;
        }
        let lexeme = &lexemes[index];
        if !at_top_level(file.bytes(), lexeme.range.start()) {
            index += 1;
            continue;
        }
        if lexeme.kind == TokenKind::At {
            pending_attribute_start.get_or_insert(lexeme.range.start());
            index += 1;
            continue;
        }
        let line_end = line_end(file.bytes(), lexeme.range.start(), cancellation)?;
        let mut cursor = index;
        let public = lexemes
            .get(cursor)
            .is_some_and(|token| token.kind == TokenKind::Pub);
        if public {
            cursor += 1;
        }
        if lexemes
            .get(cursor)
            .is_some_and(|token| matches!(token.kind, TokenKind::Pure | TokenKind::Async))
        {
            cursor += 1;
        }
        let Some(keyword) = lexemes.get(cursor) else {
            break;
        };
        let (kind, name_index) = match keyword.kind {
            TokenKind::Fn => (DeclarationKind::Function, cursor + 1),
            TokenKind::Const => (DeclarationKind::Constant, cursor + 1),
            TokenKind::Pool => (DeclarationKind::Pool, cursor + 1),
            TokenKind::Type => (DeclarationKind::TypeAlias, cursor + 1),
            TokenKind::Struct => (DeclarationKind::Struct, cursor + 1),
            TokenKind::Enum => (DeclarationKind::Enum, cursor + 1),
            TokenKind::Interface => (DeclarationKind::Interface, cursor + 1),
            TokenKind::Suite => (DeclarationKind::Suite, cursor + 1),
            TokenKind::Resource
                if lexemes
                    .get(cursor + 1)
                    .is_some_and(|token| token.kind == TokenKind::Struct) =>
            {
                (DeclarationKind::ResourceStruct, cursor + 2)
            }
            _ => {
                index += 1;
                continue;
            }
        };
        if let Some(name) = lexemes.get(name_index)
            && name.kind == TokenKind::Identifier
            && name.range.start() < line_end
            && let Some(bytes) = checked_slice(file.bytes(), name.range.start(), name.range.end())
            && let Ok(name) = std::str::from_utf8(bytes)
        {
            let authored_start = usize::try_from(
                pending_attribute_start
                    .take()
                    .unwrap_or(lexeme.range.start()),
            )
            .expect("admitted source offset");
            starts.push((
                declaration_prefix_start(file.bytes(), authored_start),
                usize::try_from(lexeme.range.start()).expect("admitted source offset"),
                usize::try_from(line_end).expect("admitted source offset"),
                kind,
                name.to_owned(),
                public,
            ));
        } else {
            diagnostics.push(Diagnostic::new(
                "syntax.malformed_declaration",
                SourceRange::from_u64_shared(file.path_arc(), lexeme.range.start(), line_end),
                RecoveryAction::SkippedToBoundary,
            ));
        }
        index += 1;
    }
    let source_len = file.bytes().len();
    Some(
        starts
            .iter()
            .enumerate()
            .map(
                |(index, (start, header_start, header_end, kind, name, public))| {
                    let end = starts.get(index + 1).map_or(source_len, |(next, ..)| *next);
                    Declaration {
                        kind: *kind,
                        name: name.clone(),
                        public: *public,
                        attributes: Vec::new(),
                        syntax: None,
                        range: SourceRange::new_shared(file.path_arc(), *start, *header_end),
                        start: u64::try_from(*start).expect("admitted source offset fits u64"),
                        header_start: u64::try_from(*header_start)
                            .expect("admitted source offset fits u64"),
                        end: u64::try_from(end).expect("admitted source offset fits u64"),
                        structurally_valid: true,
                    }
                },
            )
            .collect(),
    )
}

fn declaration_prefix_start(bytes: &[u8], mut start: usize) -> usize {
    while start > 0 {
        let before_newline = if bytes.get(start.wrapping_sub(1)) == Some(&b'\n') {
            start - 1
        } else {
            start
        };
        if before_newline == 0 {
            break;
        }
        let line_start = bytes[..before_newline]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        let line = &bytes[line_start..before_newline];
        let content = line
            .strip_suffix(b"\r")
            .unwrap_or(line)
            .iter()
            .skip_while(|byte| **byte == b' ')
            .copied()
            .collect::<Vec<_>>();
        if !content.starts_with(b"##") {
            break;
        }
        start = line_start;
    }
    start
}

#[derive(Clone)]
struct TokenLine<'a> {
    indent: usize,
    tokens: Vec<&'a Lexeme>,
    range: SourceRange,
}

fn token_lines<'a>(
    file: &ProjectFile,
    lexemes: &'a [Lexeme],
    start: u64,
    end: u64,
    cancellation: &Cancellation,
) -> Option<Vec<TokenLine<'a>>> {
    let mut lines = Vec::new();
    let mut offset = usize::try_from(start).unwrap_or(file.bytes().len());
    let end = usize::try_from(end)
        .unwrap_or(file.bytes().len())
        .min(file.bytes().len());
    let mut token_index = lexemes.partition_point(|token| token.range.end() <= start);
    while offset < end {
        if cancellation.is_cancelled() {
            return None;
        }
        let physical_end = physical_line_end(file.bytes(), offset, end, cancellation)?;
        let content_end = physical_content_end(file.bytes(), offset, physical_end);
        let indent = leading_spaces(&file.bytes()[offset..content_end], cancellation)?;
        while lexemes.get(token_index).is_some_and(|token| {
            usize::try_from(token.range.start()).is_ok_and(|position| position < offset)
        }) {
            token_index += 1;
        }
        let line_start = token_index;
        while lexemes.get(token_index).is_some_and(|token| {
            usize::try_from(token.range.start()).is_ok_and(|position| position < content_end)
        }) {
            token_index += 1;
        }
        let tokens = lexemes[line_start..token_index].iter().collect::<Vec<_>>();
        if !tokens.is_empty() {
            lines.push(TokenLine {
                indent,
                tokens,
                range: SourceRange::new_shared(file.path_arc(), offset, content_end),
            });
        }
        offset = physical_end;
    }
    Some(lines)
}

pub(super) fn physical_line_end(
    bytes: &[u8],
    offset: usize,
    end: usize,
    cancellation: &Cancellation,
) -> Option<usize> {
    let mut cursor = offset;
    while cursor < end {
        if (cursor - offset).is_multiple_of(256) && cancellation.is_cancelled() {
            return None;
        }
        cursor += 1;
        if bytes[cursor - 1] == b'\n' {
            break;
        }
    }
    Some(cursor)
}

pub(super) fn physical_content_end(bytes: &[u8], offset: usize, physical_end: usize) -> usize {
    let mut end = physical_end;
    if end > offset && bytes[end - 1] == b'\n' {
        end -= 1;
    }
    if end > offset && bytes[end - 1] == b'\r' {
        end -= 1;
    }
    end
}

pub(super) fn leading_spaces(bytes: &[u8], cancellation: &Cancellation) -> Option<usize> {
    let mut count = 0;
    while bytes.get(count) == Some(&b' ') {
        if count.is_multiple_of(256) && cancellation.is_cancelled() {
            return None;
        }
        count += 1;
    }
    Some(count)
}

pub(super) fn assign_attributes(
    file: &ProjectFile,
    lexemes: &[Lexeme],
    declarations: &mut [Declaration],
    cancellation: &Cancellation,
) -> bool {
    let Some(lines) = token_lines(
        file,
        lexemes,
        0,
        u64::try_from(file.bytes().len()).unwrap_or(u64::MAX),
        cancellation,
    ) else {
        return true;
    };
    let mut pending = Vec::new();
    let mut declaration_index = 0;
    for line in lines {
        if cancellation.is_cancelled() {
            return true;
        }
        if line.indent != 0 {
            continue;
        }
        let Some(first) = line.tokens.first() else {
            continue;
        };
        if first.kind == TokenKind::At {
            pending.push(
                line.tokens
                    .get(1)
                    .and_then(|token| token_text(file, token))
                    .map_or(AttributeSyntax::Unknown, |name| match name {
                        "image" => AttributeSyntax::Image,
                        "actor" => AttributeSyntax::Actor,
                        _ => AttributeSyntax::Unknown,
                    }),
            );
            continue;
        }
        while declarations
            .get(declaration_index)
            .is_some_and(|declaration| declaration.header_start < first.range.start())
        {
            declaration_index += 1;
        }
        if let Some(declaration) = declarations.get_mut(declaration_index)
            && declaration.header_start == first.range.start()
        {
            declaration.attributes = std::mem::take(&mut pending);
            declaration_index += 1;
        } else {
            pending.clear();
        }
    }
    false
}

pub(super) fn parse_declaration_syntax(
    file: &ProjectFile,
    declaration: &Declaration,
    lexemes: &[Lexeme],
    diagnostics: &mut Vec<Diagnostic>,
    cancellation: &Cancellation,
) -> Option<DeclarationSyntax> {
    let lines = token_lines(
        file,
        lexemes,
        declaration.header_start,
        declaration.end,
        cancellation,
    )?;
    let lines = normalize_declaration_lines(lines, declaration.kind)?;
    let header = lines.first()?;
    let mut cursor = SyntaxCursor::new(file, &header.tokens, cancellation);
    cursor.consume(TokenKind::Pub);
    let modifier = if cursor.consume(TokenKind::Pure) {
        FunctionModifier::Pure
    } else if cursor.consume(TokenKind::Async) {
        FunctionModifier::Async
    } else {
        FunctionModifier::Ordinary
    };
    let parsed = match declaration.kind {
        DeclarationKind::Function => {
            parse_function_syntax(file, &mut cursor, modifier, &lines, cancellation)
        }
        DeclarationKind::Constant => parse_constant_syntax(&mut cursor),
        DeclarationKind::TypeAlias => {
            cursor.expect(TokenKind::Type)?;
            cursor.expect_identifier()?;
            cursor.expect(TokenKind::Equal)?;
            let target = cursor.parse_type()?;
            cursor
                .at_end()
                .then_some(DeclarationSyntax::TypeAlias(target))
        }
        DeclarationKind::Suite => {
            parse_suite_syntax(file, &mut cursor, &lines, diagnostics, cancellation)
        }
        DeclarationKind::Enum => {
            parse_enum_syntax(file, &mut cursor, &lines, diagnostics, cancellation)
        }
        DeclarationKind::Struct => {
            parse_struct_syntax(file, &mut cursor, &lines, false, cancellation)
        }
        DeclarationKind::ResourceStruct => {
            parse_struct_syntax(file, &mut cursor, &lines, true, cancellation)
        }
        DeclarationKind::Interface => {
            parse_interface_syntax(file, &mut cursor, &lines, cancellation)
        }
        DeclarationKind::Pool => {
            cursor.expect(TokenKind::Pool)?;
            cursor.expect_identifier()?;
            cursor.at_end().then_some(DeclarationSyntax::Pool)
        }
    };
    let has_local_syntax_evidence = diagnostics.iter().any(|diagnostic| {
        let primary = diagnostic.primary();
        let direct = primary.start() >= declaration.start && primary.start() <= declaration.end;
        let unmatched_opener = diagnostic.code() == "syntax.missing_closer"
            && diagnostic.typed_parameters().iter().any(|(name, value)| {
                name.as_ref() == "opener_offset"
                    && matches!(value, DiagnosticValue::Unsigned(offset) if
                        u64::try_from(*offset).is_ok_and(|offset| {
                        offset >= declaration.start && offset < declaration.end
                    }))
            });
        diagnostic.code().starts_with("syntax.")
            && primary.path() == declaration.range.path()
            && (direct || unmatched_opener)
    });
    if parsed.is_none() && !has_local_syntax_evidence {
        diagnostics.push(Diagnostic::new(
            "syntax.malformed_declaration",
            declaration.range.clone(),
            RecoveryAction::SkippedToBoundary,
        ));
    }
    parsed
}

fn normalize_declaration_lines<'a>(
    mut lines: Vec<TokenLine<'a>>,
    kind: DeclarationKind,
) -> Option<Vec<TokenLine<'a>>> {
    let base_indent = lines.first()?.indent;
    for line in &mut lines {
        line.indent = line.indent.checked_sub(base_indent)?;
    }
    let mut parens = 0_u32;
    let mut brackets = 0_u32;
    let block = matches!(
        kind,
        DeclarationKind::Function
            | DeclarationKind::Struct
            | DeclarationKind::ResourceStruct
            | DeclarationKind::Enum
            | DeclarationKind::Interface
            | DeclarationKind::Suite
    );
    let mut header_end = None;
    for (line_index, line) in lines.iter().enumerate() {
        for token in &line.tokens {
            match token.kind {
                TokenKind::LeftParen => parens = parens.checked_add(1)?,
                TokenKind::RightParen => parens = parens.checked_sub(1)?,
                TokenKind::LeftBracket => brackets = brackets.checked_add(1)?,
                TokenKind::RightBracket => brackets = brackets.checked_sub(1)?,
                _ => {}
            }
        }
        let balanced = parens == 0 && brackets == 0;
        let complete = balanced
            && if block {
                line.tokens
                    .last()
                    .is_some_and(|token| token.kind == TokenKind::Colon)
            } else {
                true
            };
        if complete {
            header_end = Some(line_index);
            break;
        }
    }
    let header_end = header_end?;
    if header_end == 0 {
        return Some(lines);
    }
    let first = lines.first()?;
    let last = &lines[header_end];
    let mut header_tokens = Vec::new();
    for line in &lines[..=header_end] {
        header_tokens.extend(line.tokens.iter().copied());
    }
    let mut normalized = Vec::with_capacity(lines.len() - header_end);
    normalized.push(TokenLine {
        indent: first.indent,
        tokens: header_tokens,
        range: SourceRange::from_u64_shared(
            first.range.path_arc(),
            first.range.start(),
            last.range.end(),
        ),
    });
    normalized.extend(lines.into_iter().skip(header_end + 1));
    Some(normalized)
}

fn parse_function_syntax(
    file: &ProjectFile,
    cursor: &mut SyntaxCursor<'_, '_>,
    modifier: FunctionModifier,
    lines: &[TokenLine<'_>],
    cancellation: &Cancellation,
) -> Option<DeclarationSyntax> {
    cursor.expect(TokenKind::Fn)?;
    cursor.expect_identifier()?;
    let generic_parameters = parse_generic_parameters(cursor)?;
    let type_parameters = generic_parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .collect();
    let parameters = parse_parameters(cursor)?;
    let return_type = if cursor.consume(TokenKind::Arrow) {
        cursor.parse_type()?
    } else {
        TypeSyntax::Unit
    };
    cursor.expect(TokenKind::Colon)?;
    if !cursor.at_end() {
        return None;
    }
    let mut line_index = 1;
    let body = parse_statement_block(
        file,
        lines,
        &mut line_index,
        4,
        StatementContext::Ordinary,
        cancellation,
    )?;
    Some(DeclarationSyntax::Function(FunctionSyntax {
        modifier,
        type_parameters,
        generic_parameters,
        parameters,
        return_type,
        body,
    }))
}

fn parse_constant_syntax(cursor: &mut SyntaxCursor<'_, '_>) -> Option<DeclarationSyntax> {
    cursor.expect(TokenKind::Const)?;
    cursor.expect_identifier()?;
    cursor.expect(TokenKind::Colon)?;
    let type_syntax = cursor.parse_type()?;
    cursor.expect(TokenKind::Equal)?;
    let value = cursor.parse_complete_expression()?;
    Some(DeclarationSyntax::Constant(ConstantSyntax {
        type_syntax,
        value,
    }))
}

fn parse_struct_member_selection(
    file: &ProjectFile,
    lines: &[TokenLine<'_>],
    index: &mut usize,
    cancellation: &Cancellation,
) -> Option<ComptimeMemberSelection> {
    parse_member_selection(file, lines, index, false, cancellation)
}

fn parse_enum_member_selection(
    file: &ProjectFile,
    lines: &[TokenLine<'_>],
    index: &mut usize,
    cancellation: &Cancellation,
) -> Option<ComptimeMemberSelection> {
    parse_member_selection(file, lines, index, true, cancellation)
}

fn parse_member_selection(
    file: &ProjectFile,
    lines: &[TokenLine<'_>],
    index: &mut usize,
    enum_members: bool,
    cancellation: &Cancellation,
) -> Option<ComptimeMemberSelection> {
    let selection_start = lines.get(*index)?.range.start();
    let mut branches = Vec::new();
    loop {
        let header = lines.get(*index)?;
        let mut cursor = SyntaxCursor::from_line(file, header, cancellation);
        let condition = if cursor.consume(TokenKind::Comptime) {
            cursor.expect(TokenKind::If)?;
            Some(cursor.parse_expression(0)?)
        } else if cursor.consume(TokenKind::Elif) {
            Some(cursor.parse_expression(0)?)
        } else if cursor.consume(TokenKind::Else) {
            None
        } else {
            return None;
        };
        cursor.expect(TokenKind::Colon)?;
        if !cursor.at_end() {
            return None;
        }
        *index += 1;
        let mut fields = Vec::new();
        let mut variants = Vec::new();
        let mut functions = Vec::new();
        let mut constants = Vec::new();
        while let Some(line) = lines.get(*index) {
            if cancellation.is_cancelled() {
                return None;
            }
            if line.indent <= 4 {
                break;
            }
            if line.indent != 8 {
                return None;
            }
            let mut member = SyntaxCursor::new(file, &line.tokens, cancellation);
            let public = member.consume(TokenKind::Pub);
            let modifier = if member.consume(TokenKind::Pure) {
                FunctionModifier::Pure
            } else if member.consume(TokenKind::Async) {
                FunctionModifier::Async
            } else {
                FunctionModifier::Ordinary
            };
            if modifier == FunctionModifier::Ordinary && member.consume(TokenKind::Const) {
                let name = member.expect_identifier()?.to_owned();
                member.expect(TokenKind::Colon)?;
                let type_syntax = member.parse_type()?;
                member.expect(TokenKind::Equal)?;
                let value = member.parse_complete_expression()?;
                constants.push(MemberConstantSyntax {
                    name,
                    public,
                    type_syntax,
                    value: Some(value),
                    range: line.range.clone(),
                });
                *index += 1;
                continue;
            }
            if member.consume(TokenKind::Fn) {
                let name = member.expect_identifier()?.to_owned();
                let generic_parameters = parse_generic_parameters(&mut member)?;
                let type_parameters = generic_parameters
                    .iter()
                    .map(|parameter| parameter.name.clone())
                    .collect();
                let parameters = parse_parameters(&mut member)?;
                let return_type = if member.consume(TokenKind::Arrow) {
                    member.parse_type()?
                } else {
                    TypeSyntax::Unit
                };
                member.expect(TokenKind::Colon)?;
                if !member.at_end() {
                    return None;
                }
                *index += 1;
                let body = parse_statement_block(
                    file,
                    lines,
                    index,
                    12,
                    StatementContext::Ordinary,
                    cancellation,
                )?;
                if body.is_empty() {
                    return None;
                }
                functions.push(MemberFunctionSyntax {
                    name,
                    public,
                    function: FunctionSyntax {
                        modifier,
                        type_parameters,
                        generic_parameters,
                        parameters,
                        return_type,
                        body: body.clone(),
                    },
                    range: SourceRange::from_u64_shared(
                        line.range.path_arc(),
                        line.range.start(),
                        statement_range(body.last()?).end(),
                    ),
                });
                continue;
            }
            if enum_members {
                if public || modifier != FunctionModifier::Ordinary {
                    return None;
                }
                let name = member.expect_identifier()?.to_owned();
                let parameters = if member.peek_kind() == Some(TokenKind::LeftParen) {
                    parse_parameters(&mut member)?
                } else {
                    Vec::new()
                };
                if !member.at_end() {
                    return None;
                }
                variants.push(VariantSyntax {
                    name,
                    parameters,
                    range: line.range.clone(),
                });
            } else {
                if modifier != FunctionModifier::Ordinary {
                    return None;
                }
                let mutable = member.consume(TokenKind::Mut);
                let name = member.expect_identifier()?.to_owned();
                member.expect(TokenKind::Colon)?;
                let type_syntax = member.parse_type()?;
                if !member.at_end() {
                    return None;
                }
                fields.push(FieldSyntax {
                    name,
                    public,
                    mutable,
                    type_syntax,
                    range: line.range.clone(),
                });
            }
            *index += 1;
        }
        if fields.is_empty() && variants.is_empty() && functions.is_empty() {
            return None;
        }
        let branch_end = functions
            .last()
            .map(|function| function.range.end())
            .into_iter()
            .chain(fields.last().map(|field| field.range.end()))
            .chain(variants.last().map(|variant| variant.range.end()))
            .max()?;
        branches.push(ComptimeMemberBranch {
            condition,
            fields,
            variants,
            functions,
            constants,
            range: SourceRange::from_u64_shared(
                header.range.path_arc(),
                header.range.start(),
                branch_end,
            ),
        });
        let continues = lines.get(*index).is_some_and(|line| {
            line.indent == 4
                && line
                    .tokens
                    .first()
                    .is_some_and(|token| matches!(token.kind, TokenKind::Elif | TokenKind::Else))
        });
        if !continues {
            break;
        }
    }
    let selection_end = branches.last()?.range.end();
    Some(ComptimeMemberSelection {
        branches,
        range: SourceRange::from_u64_shared(file.path_arc(), selection_start, selection_end),
    })
}

fn parse_struct_syntax(
    file: &ProjectFile,
    cursor: &mut SyntaxCursor<'_, '_>,
    lines: &[TokenLine<'_>],
    resource: bool,
    cancellation: &Cancellation,
) -> Option<DeclarationSyntax> {
    if resource {
        cursor.expect(TokenKind::Resource)?;
    }
    cursor.expect(TokenKind::Struct)?;
    cursor.expect_identifier()?;
    let generic_parameters = parse_generic_parameters(cursor)?;
    let type_parameters = generic_parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .collect();
    let mut implements = Vec::new();
    if cursor.consume(TokenKind::Implements) {
        loop {
            implements.push(cursor.parse_name()?);
            if !cursor.consume(TokenKind::Comma) {
                break;
            }
        }
    }
    cursor.expect(TokenKind::Colon)?;
    if !cursor.at_end() {
        return None;
    }
    let mut fields = Vec::new();
    let mut functions = Vec::new();
    let mut constants = Vec::new();
    let mut comptime_selections = Vec::new();
    let mut line_index = 1;
    while let Some(line) = lines.get(line_index) {
        if cancellation.is_cancelled() || line.indent != 4 {
            return None;
        }
        if line
            .tokens
            .first()
            .is_some_and(|token| token.kind == TokenKind::Comptime)
        {
            comptime_selections.push(parse_struct_member_selection(
                file,
                lines,
                &mut line_index,
                cancellation,
            )?);
            continue;
        }
        let mut member = SyntaxCursor::new(file, &line.tokens, cancellation);
        let public = member.consume(TokenKind::Pub);
        let modifier = if member.consume(TokenKind::Pure) {
            FunctionModifier::Pure
        } else if member.consume(TokenKind::Async) {
            FunctionModifier::Async
        } else {
            FunctionModifier::Ordinary
        };
        if modifier == FunctionModifier::Ordinary && member.consume(TokenKind::Const) {
            let name = member.expect_identifier()?.to_owned();
            member.expect(TokenKind::Colon)?;
            let type_syntax = member.parse_type()?;
            member.expect(TokenKind::Equal)?;
            let value = member.parse_complete_expression()?;
            constants.push(MemberConstantSyntax {
                name,
                public,
                type_syntax,
                value: Some(value),
                range: line.range.clone(),
            });
            line_index += 1;
            continue;
        }
        if member.consume(TokenKind::Fn) {
            let name = member.expect_identifier()?.to_owned();
            let generic_parameters = parse_generic_parameters(&mut member)?;
            let type_parameters = generic_parameters
                .iter()
                .map(|parameter| parameter.name.clone())
                .collect();
            let parameters = parse_parameters(&mut member)?;
            let return_type = if member.consume(TokenKind::Arrow) {
                member.parse_type()?
            } else {
                TypeSyntax::Unit
            };
            member.expect(TokenKind::Colon)?;
            if !member.at_end() {
                return None;
            }
            line_index += 1;
            let body = parse_statement_block(
                file,
                lines,
                &mut line_index,
                8,
                StatementContext::Ordinary,
                cancellation,
            )?;
            if body.is_empty() {
                return None;
            }
            let range = SourceRange::from_u64_shared(
                line.range.path_arc(),
                line.range.start(),
                statement_range(body.last()?).end(),
            );
            functions.push(MemberFunctionSyntax {
                name,
                public,
                function: FunctionSyntax {
                    modifier,
                    type_parameters,
                    generic_parameters,
                    parameters,
                    return_type,
                    body,
                },
                range,
            });
            continue;
        }
        if modifier != FunctionModifier::Ordinary {
            return None;
        }
        let mutable = member.consume(TokenKind::Mut);
        let name = member.expect_identifier()?.to_owned();
        member.expect(TokenKind::Colon)?;
        let type_syntax = member.parse_type()?;
        if !member.at_end() {
            return None;
        }
        fields.push(FieldSyntax {
            name,
            public,
            mutable,
            type_syntax,
            range: line.range.clone(),
        });
        line_index += 1;
    }
    let syntax = StructSyntax {
        type_parameters,
        generic_parameters,
        implements,
        fields,
        functions,
        constants,
        comptime_selections,
    };
    Some(if resource {
        DeclarationSyntax::ResourceStruct(syntax)
    } else {
        DeclarationSyntax::Struct(syntax)
    })
}

fn parse_interface_syntax(
    file: &ProjectFile,
    cursor: &mut SyntaxCursor<'_, '_>,
    lines: &[TokenLine<'_>],
    cancellation: &Cancellation,
) -> Option<DeclarationSyntax> {
    cursor.expect(TokenKind::Interface)?;
    cursor.expect_identifier()?;
    cursor.expect(TokenKind::Colon)?;
    if !cursor.at_end() {
        return None;
    }
    let mut requirements = Vec::new();
    let mut constants = Vec::new();
    for line in lines.iter().skip(1) {
        if cancellation.is_cancelled() || line.indent != 4 {
            return None;
        }
        let mut requirement = SyntaxCursor::new(file, &line.tokens, cancellation);
        let public = requirement.consume(TokenKind::Pub);
        if requirement.consume(TokenKind::Const) {
            let name = requirement.expect_identifier()?.to_owned();
            requirement.expect(TokenKind::Colon)?;
            let type_syntax = requirement.parse_type()?;
            if !requirement.at_end() {
                return None;
            }
            constants.push(MemberConstantSyntax {
                name,
                public,
                type_syntax,
                value: None,
                range: line.range.clone(),
            });
            continue;
        }
        if public {
            return None;
        }
        let modifier = if requirement.consume(TokenKind::Pure) {
            FunctionModifier::Pure
        } else if requirement.consume(TokenKind::Async) {
            FunctionModifier::Async
        } else {
            FunctionModifier::Ordinary
        };
        requirement.expect(TokenKind::Fn)?;
        let name = requirement.expect_identifier()?.to_owned();
        let parameters = parse_parameters(&mut requirement)?;
        let return_type = if requirement.consume(TokenKind::Arrow) {
            requirement.parse_type()?
        } else {
            TypeSyntax::Unit
        };
        if !requirement.at_end() {
            return None;
        }
        requirements.push(FunctionRequirementSyntax {
            name,
            modifier,
            parameters,
            return_type,
            range: line.range.clone(),
        });
    }
    Some(DeclarationSyntax::Interface(InterfaceSyntax {
        requirements,
        constants,
    }))
}

fn parse_suite_syntax(
    file: &ProjectFile,
    cursor: &mut SyntaxCursor<'_, '_>,
    lines: &[TokenLine<'_>],
    diagnostics: &mut Vec<Diagnostic>,
    cancellation: &Cancellation,
) -> Option<DeclarationSyntax> {
    cursor.expect(TokenKind::Suite)?;
    cursor.expect_identifier()?;
    cursor.expect(TokenKind::Colon)?;
    if !cursor.at_end() {
        return None;
    }
    let mut tests = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    let mut line_index = 1;
    while let Some(line) = lines.get(line_index) {
        if line.indent < 4 {
            break;
        }
        if line.indent > 4 {
            return None;
        }
        let mut test = SyntaxCursor::new(file, &line.tokens, cancellation);
        let asynchronous = test.consume(TokenKind::Async);
        if !test.consume(TokenKind::Test) {
            return None;
        }
        let name = test.expect_identifier()?.to_owned();
        let parameters = parse_parameters(&mut test)?;
        test.expect(TokenKind::Colon)?;
        if !test.at_end() {
            return None;
        }
        if !names.insert(name.clone()) {
            diagnostics.push(
                Diagnostic::new(
                    "test.duplicate_declaration",
                    line.range.clone(),
                    RecoveryAction::None,
                )
                .with_parameter("name", name.clone()),
            );
        }
        line_index += 1;
        let body = parse_statement_block(
            file,
            lines,
            &mut line_index,
            8,
            StatementContext::Test,
            cancellation,
        )?;
        if body.is_empty() {
            return None;
        }
        let range = SourceRange::from_u64_shared(
            line.range.path_arc(),
            line.range.start(),
            statement_range(body.last().expect("non-empty Test body")).end(),
        );
        tests.push(TestSyntax {
            name,
            asynchronous,
            parameters,
            body,
            range,
        });
    }
    Some(DeclarationSyntax::Suite(SuiteSyntax { tests }))
}

fn statement_range(statement: &StatementSyntax) -> &SourceRange {
    match statement {
        StatementSyntax::Return { range, .. }
        | StatementSyntax::Panic { range, .. }
        | StatementSyntax::Assert { range, .. }
        | StatementSyntax::Expect { range, .. }
        | StatementSyntax::Assign { range, .. }
        | StatementSyntax::If { range, .. }
        | StatementSyntax::Comptime { range, .. }
        | StatementSyntax::For { range, .. }
        | StatementSyntax::While { range, .. }
        | StatementSyntax::Break(range)
        | StatementSyntax::Continue(range)
        | StatementSyntax::Match { range, .. }
        | StatementSyntax::Defer { range, .. }
        | StatementSyntax::With { range, .. }
        | StatementSyntax::Unsupported { range, .. }
        | StatementSyntax::Pass(range) => range,
        StatementSyntax::Evaluate(expression) => &expression.range,
    }
}

fn parse_enum_syntax(
    file: &ProjectFile,
    cursor: &mut SyntaxCursor<'_, '_>,
    lines: &[TokenLine<'_>],
    diagnostics: &mut Vec<Diagnostic>,
    cancellation: &Cancellation,
) -> Option<DeclarationSyntax> {
    cursor.expect(TokenKind::Enum)?;
    cursor.expect_identifier()?;
    let generic_parameters = parse_generic_parameters(cursor)?;
    let type_parameters = generic_parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .collect();
    cursor.expect(TokenKind::Colon)?;
    if !cursor.at_end() {
        return None;
    }
    let mut variants = Vec::new();
    let mut functions = Vec::new();
    let mut constants = Vec::new();
    let mut comptime_selections = Vec::new();
    let mut names = BTreeSet::new();
    let mut line_index = 1;
    while let Some(line) = lines.get(line_index) {
        if cancellation.is_cancelled() || line.indent != 4 {
            return None;
        }
        if line
            .tokens
            .first()
            .is_some_and(|token| token.kind == TokenKind::Comptime)
        {
            comptime_selections.push(parse_enum_member_selection(
                file,
                lines,
                &mut line_index,
                cancellation,
            )?);
            continue;
        }
        let mut member = SyntaxCursor::new(file, &line.tokens, cancellation);
        let public = member.consume(TokenKind::Pub);
        let modifier = if member.consume(TokenKind::Pure) {
            FunctionModifier::Pure
        } else if member.consume(TokenKind::Async) {
            FunctionModifier::Async
        } else {
            FunctionModifier::Ordinary
        };
        if modifier == FunctionModifier::Ordinary && member.consume(TokenKind::Const) {
            let name = member.expect_identifier()?.to_owned();
            member.expect(TokenKind::Colon)?;
            let type_syntax = member.parse_type()?;
            member.expect(TokenKind::Equal)?;
            let value = member.parse_complete_expression()?;
            constants.push(MemberConstantSyntax {
                name,
                public,
                type_syntax,
                value: Some(value),
                range: line.range.clone(),
            });
            line_index += 1;
            continue;
        }
        if member.consume(TokenKind::Fn) {
            let name = member.expect_identifier()?.to_owned();
            let member_generic_parameters = parse_generic_parameters(&mut member)?;
            let member_type_parameters = member_generic_parameters
                .iter()
                .map(|parameter| parameter.name.clone())
                .collect();
            let parameters = parse_parameters(&mut member)?;
            let return_type = if member.consume(TokenKind::Arrow) {
                member.parse_type()?
            } else {
                TypeSyntax::Unit
            };
            member.expect(TokenKind::Colon)?;
            if !member.at_end() {
                return None;
            }
            line_index += 1;
            let body = parse_statement_block(
                file,
                lines,
                &mut line_index,
                8,
                StatementContext::Ordinary,
                cancellation,
            )?;
            if body.is_empty() {
                return None;
            }
            functions.push(MemberFunctionSyntax {
                name,
                public,
                function: FunctionSyntax {
                    modifier,
                    type_parameters: member_type_parameters,
                    generic_parameters: member_generic_parameters,
                    parameters,
                    return_type,
                    body: body.clone(),
                },
                range: SourceRange::from_u64_shared(
                    line.range.path_arc(),
                    line.range.start(),
                    statement_range(body.last()?).end(),
                ),
            });
            continue;
        }
        if public || modifier != FunctionModifier::Ordinary {
            return None;
        }
        let name = member.expect_identifier()?.to_owned();
        let parameters = if member.peek_kind() == Some(TokenKind::LeftParen) {
            parse_parameters(&mut member)?
        } else {
            Vec::new()
        };
        if !member.at_end() {
            return None;
        }
        if !names.insert(name.clone()) {
            diagnostics.push(
                Diagnostic::new(
                    "semantic.duplicate_variant",
                    line.range.clone(),
                    RecoveryAction::None,
                )
                .with_parameter("name", name.clone()),
            );
        }
        variants.push(VariantSyntax {
            name,
            parameters,
            range: line.range.clone(),
        });
        line_index += 1;
    }
    Some(DeclarationSyntax::Enum(EnumSyntax {
        type_parameters,
        generic_parameters,
        variants,
        functions,
        constants,
        comptime_selections,
    }))
}

fn parse_parameters(cursor: &mut SyntaxCursor<'_, '_>) -> Option<Vec<ParameterSyntax>> {
    cursor.expect(TokenKind::LeftParen)?;
    let mut parameters = Vec::new();
    while !cursor.consume(TokenKind::RightParen) {
        let start = cursor.current_range()?.clone();
        let ownership = if cursor.consume(TokenKind::Read) {
            OwnershipSyntax::Read
        } else if cursor.consume(TokenKind::Mut) {
            OwnershipSyntax::Mut
        } else if cursor.consume(TokenKind::Take) {
            OwnershipSyntax::Take
        } else {
            OwnershipSyntax::Value
        };
        let is_self = cursor.consume(TokenKind::SelfValue);
        let (name, type_syntax) = if is_self {
            (
                "self".to_owned(),
                TypeSyntax::Named(NameSyntax {
                    segments: vec!["Self".to_owned()],
                }),
            )
        } else {
            let name = cursor.expect_identifier()?.to_owned();
            cursor.expect(TokenKind::Colon)?;
            (name, cursor.parse_type()?)
        };
        let ownership = if is_self && ownership == OwnershipSyntax::Value {
            OwnershipSyntax::Read
        } else {
            ownership
        };
        let end = cursor.previous_range()?.end();
        parameters.push(ParameterSyntax {
            name,
            ownership,
            type_syntax,
            range: SourceRange::from_u64_shared(start.path_arc(), start.start(), end),
        });
        if !cursor.consume(TokenKind::Comma) {
            cursor.expect(TokenKind::RightParen)?;
            break;
        }
    }
    Some(parameters)
}

fn parse_generic_parameters(
    cursor: &mut SyntaxCursor<'_, '_>,
) -> Option<Vec<GenericParameterSyntax>> {
    let mut parameters = Vec::new();
    if cursor.consume(TokenKind::LeftBracket) {
        while !cursor.consume(TokenKind::RightBracket) {
            let parameter = if cursor.consume(TokenKind::Const) {
                let name = cursor.expect_identifier()?.to_owned();
                cursor.expect(TokenKind::Colon)?;
                GenericParameterSyntax {
                    name,
                    kind: GenericParameterKindSyntax::Const {
                        type_syntax: cursor.parse_type()?,
                    },
                }
            } else {
                let name = cursor.expect_identifier()?.to_owned();
                let kind = if cursor.consume(TokenKind::Colon) {
                    let pool_bound = cursor.consume(TokenKind::Pool)
                        || (cursor.peek_kind() == Some(TokenKind::Identifier)
                            && cursor
                                .tokens
                                .get(cursor.index)
                                .and_then(|token| token_text(cursor.file, token))
                                == Some("Pool")
                            && {
                                cursor.advance();
                                true
                            });
                    if pool_bound {
                        GenericParameterKindSyntax::Pool
                    } else {
                        GenericParameterKindSyntax::Type {
                            interface_bound: Some(cursor.parse_name()?),
                        }
                    }
                } else {
                    GenericParameterKindSyntax::Type {
                        interface_bound: None,
                    }
                };
                GenericParameterSyntax { name, kind }
            };
            parameters.push(parameter);
            if !cursor.consume(TokenKind::Comma) {
                cursor.expect(TokenKind::RightBracket)?;
                break;
            }
        }
    }
    Some(parameters)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatementContext {
    Ordinary,
    Test,
}

pub(super) fn unsupported_statement_node(kind: UnsupportedStatementKind) -> SyntaxNodeKind {
    match kind {
        UnsupportedStatementKind::Take => SyntaxNodeKind::TakeStatement,
    }
}

fn parse_statement_block(
    file: &ProjectFile,
    lines: &[TokenLine<'_>],
    index: &mut usize,
    indent: usize,
    context: StatementContext,
    cancellation: &Cancellation,
) -> Option<Vec<StatementSyntax>> {
    let mut statements = Vec::new();
    while let Some(line) = lines.get(*index) {
        if cancellation.is_cancelled() {
            return None;
        }
        if line.indent < indent {
            break;
        }
        if line.indent > indent {
            return None;
        }
        if line
            .tokens
            .first()
            .is_some_and(|token| matches!(token.kind, TokenKind::Elif | TokenKind::Else))
        {
            break;
        }
        *index += 1;
        let mut cursor = SyntaxCursor::from_line(file, line, cancellation);
        let statement = match cursor.peek_kind()? {
            TokenKind::Comptime => {
                cursor.advance();
                cursor.expect(TokenKind::If)?;
                let condition = cursor.parse_expression(0)?;
                cursor.expect(TokenKind::Colon)?;
                if !cursor.at_end() {
                    return None;
                }
                let then_statements =
                    parse_statement_block(file, lines, index, indent + 4, context, cancellation)?;
                if then_statements.is_empty() {
                    return None;
                }
                let mut branches = vec![ComptimeStatementBranch {
                    condition: Some(condition),
                    range: SourceRange::from_u64_shared(
                        line.range.path_arc(),
                        line.range.start(),
                        statement_range(then_statements.last()?).end(),
                    ),
                    statements: then_statements,
                }];
                while let Some(next) = lines.get(*index)
                    && next.indent == indent
                    && next
                        .tokens
                        .first()
                        .is_some_and(|token| token.kind == TokenKind::Elif)
                {
                    let mut branch = SyntaxCursor::from_line(file, next, cancellation);
                    branch.expect(TokenKind::Elif)?;
                    let condition = branch.parse_expression(0)?;
                    branch.expect(TokenKind::Colon)?;
                    if !branch.at_end() {
                        return None;
                    }
                    *index += 1;
                    let statements = parse_statement_block(
                        file,
                        lines,
                        index,
                        indent + 4,
                        context,
                        cancellation,
                    )?;
                    if statements.is_empty() {
                        return None;
                    }
                    branches.push(ComptimeStatementBranch {
                        condition: Some(condition),
                        range: SourceRange::from_u64_shared(
                            next.range.path_arc(),
                            next.range.start(),
                            statement_range(statements.last()?).end(),
                        ),
                        statements,
                    });
                }
                if let Some(next) = lines.get(*index)
                    && next.indent == indent
                    && next
                        .tokens
                        .first()
                        .is_some_and(|token| token.kind == TokenKind::Else)
                {
                    let mut branch = SyntaxCursor::from_line(file, next, cancellation);
                    branch.expect(TokenKind::Else)?;
                    branch.expect(TokenKind::Colon)?;
                    if !branch.at_end() {
                        return None;
                    }
                    *index += 1;
                    let statements = parse_statement_block(
                        file,
                        lines,
                        index,
                        indent + 4,
                        context,
                        cancellation,
                    )?;
                    if statements.is_empty() {
                        return None;
                    }
                    branches.push(ComptimeStatementBranch {
                        condition: None,
                        range: SourceRange::from_u64_shared(
                            next.range.path_arc(),
                            next.range.start(),
                            statement_range(statements.last()?).end(),
                        ),
                        statements,
                    });
                }
                let end = branches.last()?.range.end();
                StatementSyntax::Comptime {
                    branches,
                    range: SourceRange::from_u64_shared(
                        line.range.path_arc(),
                        line.range.start(),
                        end,
                    ),
                }
            }
            TokenKind::For => {
                cursor.advance();
                let pattern = cursor.parse_pattern()?;
                cursor.expect(TokenKind::In)?;
                let iterable = cursor.parse_expression(0)?;
                cursor.expect(TokenKind::Colon)?;
                if !cursor.at_end() {
                    return None;
                }
                let body =
                    parse_statement_block(file, lines, index, indent + 4, context, cancellation)?;
                StatementSyntax::For {
                    pattern,
                    iterable,
                    body,
                    range: line.range.clone(),
                }
            }
            TokenKind::Match => {
                cursor.advance();
                let value = cursor.parse_expression(0)?;
                cursor.expect(TokenKind::Colon)?;
                if !cursor.at_end() {
                    return None;
                }
                let mut cases = Vec::new();
                while let Some(case_line) = lines.get(*index)
                    && case_line.indent == indent + 4
                    && case_line
                        .tokens
                        .first()
                        .is_some_and(|token| token.kind == TokenKind::Case)
                {
                    let mut case_cursor = SyntaxCursor::from_line(file, case_line, cancellation);
                    case_cursor.expect(TokenKind::Case)?;
                    let pattern = case_cursor.parse_pattern()?;
                    let guard = if case_cursor.consume(TokenKind::If) {
                        Some(case_cursor.parse_expression(0)?)
                    } else {
                        None
                    };
                    case_cursor.expect(TokenKind::Colon)?;
                    if !case_cursor.at_end() {
                        return None;
                    }
                    let case_range = case_line.range.clone();
                    *index += 1;
                    let body = parse_statement_block(
                        file,
                        lines,
                        index,
                        indent + 8,
                        context,
                        cancellation,
                    )?;
                    cases.push(MatchCaseSyntax {
                        pattern,
                        guard,
                        body,
                        range: case_range,
                    });
                }
                if cases.is_empty() {
                    return None;
                }
                StatementSyntax::Match {
                    value,
                    cases,
                    range: line.range.clone(),
                }
            }
            TokenKind::While => {
                cursor.advance();
                let condition = cursor.parse_expression(0)?;
                cursor.expect(TokenKind::Colon)?;
                if !cursor.at_end() {
                    return None;
                }
                let body =
                    parse_statement_block(file, lines, index, indent + 4, context, cancellation)?;
                StatementSyntax::While {
                    condition,
                    body,
                    range: line.range.clone(),
                }
            }
            TokenKind::With => {
                cursor.advance();
                let scope = cursor.parse_expression(0)?;
                let binding = if cursor.consume(TokenKind::As) {
                    Some(cursor.expect_identifier()?.to_owned())
                } else {
                    None
                };
                cursor.expect(TokenKind::Colon)?;
                if !cursor.at_end() {
                    return None;
                }
                let body =
                    parse_statement_block(file, lines, index, indent + 4, context, cancellation)?;
                StatementSyntax::With {
                    scope,
                    binding,
                    body,
                    range: line.range.clone(),
                }
            }
            TokenKind::Break => {
                cursor.advance();
                if !cursor.at_end() {
                    return None;
                }
                StatementSyntax::Break(line.range.clone())
            }
            TokenKind::Continue => {
                cursor.advance();
                if !cursor.at_end() {
                    return None;
                }
                StatementSyntax::Continue(line.range.clone())
            }
            TokenKind::Defer => {
                cursor.advance();
                StatementSyntax::Defer {
                    expression: cursor.parse_complete_expression()?,
                    range: line.range.clone(),
                }
            }
            TokenKind::Take => StatementSyntax::Unsupported {
                kind: UnsupportedStatementKind::Take,
                range: line.range.clone(),
            },
            TokenKind::If => {
                cursor.advance();
                let condition = cursor.parse_expression(0)?;
                cursor.expect(TokenKind::Colon)?;
                if !cursor.at_end() {
                    return None;
                }
                let then_branch =
                    parse_statement_block(file, lines, index, indent + 4, context, cancellation)?;
                let mut elif_branches = Vec::new();
                let mut else_branch = Vec::new();
                while let Some(next) = lines.get(*index)
                    && next.indent == indent
                    && next
                        .tokens
                        .first()
                        .is_some_and(|token| token.kind == TokenKind::Elif)
                {
                    let mut elif_cursor = SyntaxCursor::from_line(file, next, cancellation);
                    elif_cursor.expect(TokenKind::Elif)?;
                    let elif_condition = elif_cursor.parse_expression(0)?;
                    elif_cursor.expect(TokenKind::Colon)?;
                    if !elif_cursor.at_end() {
                        return None;
                    }
                    let elif_range = next.range.clone();
                    *index += 1;
                    let elif_body = parse_statement_block(
                        file,
                        lines,
                        index,
                        indent + 4,
                        context,
                        cancellation,
                    )?;
                    elif_branches.push((elif_condition, elif_body, elif_range));
                }
                if let Some(next) = lines.get(*index)
                    && next.indent == indent
                    && next
                        .tokens
                        .first()
                        .is_some_and(|token| token.kind == TokenKind::Else)
                {
                    let mut else_cursor = SyntaxCursor::from_line(file, next, cancellation);
                    else_cursor.expect(TokenKind::Else)?;
                    else_cursor.expect(TokenKind::Colon)?;
                    if !else_cursor.at_end() {
                        return None;
                    }
                    *index += 1;
                    else_branch = parse_statement_block(
                        file,
                        lines,
                        index,
                        indent + 4,
                        context,
                        cancellation,
                    )?;
                }
                for (condition, then_branch, range) in elif_branches.into_iter().rev() {
                    else_branch = vec![StatementSyntax::If {
                        condition,
                        then_branch,
                        else_branch,
                        range,
                    }];
                }
                StatementSyntax::If {
                    condition,
                    then_branch,
                    else_branch,
                    range: line.range.clone(),
                }
            }
            TokenKind::Return => {
                cursor.advance();
                let value = if cursor.at_end() {
                    None
                } else {
                    Some(cursor.parse_complete_expression()?)
                };
                StatementSyntax::Return {
                    value,
                    range: line.range.clone(),
                }
            }
            TokenKind::Panic => {
                cursor.advance();
                StatementSyntax::Panic {
                    value: cursor.parse_complete_expression()?,
                    range: line.range.clone(),
                }
            }
            TokenKind::Assert => {
                cursor.advance();
                StatementSyntax::Assert {
                    condition: cursor.parse_complete_expression()?,
                    range: line.range.clone(),
                }
            }
            TokenKind::Expect => {
                if context != StatementContext::Test {
                    return None;
                }
                cursor.advance();
                StatementSyntax::Expect {
                    condition: cursor.parse_complete_expression()?,
                    range: line.range.clone(),
                }
            }
            TokenKind::Pass => {
                cursor.advance();
                if !cursor.at_end() {
                    return None;
                }
                StatementSyntax::Pass(line.range.clone())
            }
            TokenKind::At => return None,
            TokenKind::Mut => {
                cursor.advance();
                let token = *cursor.tokens.get(cursor.index)?;
                let root = cursor.expect_identifier()?.to_owned();
                let declared_type = if cursor.consume(TokenKind::Colon) {
                    Some(cursor.parse_type()?)
                } else {
                    None
                };
                cursor.expect(TokenKind::Equal)?;
                StatementSyntax::Assign {
                    place: PlaceSyntax {
                        root,
                        projections: Vec::new(),
                        range: token.range.clone(),
                    },
                    mutable_binding: true,
                    declared_type,
                    operator: None,
                    value: cursor.parse_complete_expression()?,
                    range: line.range.clone(),
                }
            }
            TokenKind::Identifier => {
                let checkpoint = cursor.index;
                let place = cursor.parse_place()?;
                let declared_type = if cursor.consume(TokenKind::Colon) {
                    if !place.projections.is_empty() {
                        return None;
                    }
                    Some(cursor.parse_type()?)
                } else {
                    None
                };
                let operator = cursor.peek_kind().and_then(compound_assignment_operator);
                if operator.is_some() {
                    cursor.advance();
                }
                if !cursor.consume(TokenKind::Equal) {
                    cursor.index = checkpoint;
                    StatementSyntax::Evaluate(cursor.parse_complete_expression()?)
                } else {
                    StatementSyntax::Assign {
                        place,
                        mutable_binding: false,
                        declared_type,
                        operator,
                        value: cursor.parse_complete_expression()?,
                        range: line.range.clone(),
                    }
                }
            }
            _ => StatementSyntax::Evaluate(cursor.parse_complete_expression()?),
        };
        statements.push(statement);
    }
    Some(statements)
}

fn compound_assignment_operator(kind: TokenKind) -> Option<BinaryOperatorSyntax> {
    Some(match kind {
        TokenKind::Plus => BinaryOperatorSyntax::Add,
        TokenKind::Minus => BinaryOperatorSyntax::Subtract,
        TokenKind::Star => BinaryOperatorSyntax::Multiply,
        TokenKind::Slash => BinaryOperatorSyntax::Divide,
        TokenKind::Percent => BinaryOperatorSyntax::Remainder,
        TokenKind::Ampersand => BinaryOperatorSyntax::BitAnd,
        TokenKind::Pipe => BinaryOperatorSyntax::BitOr,
        TokenKind::Caret => BinaryOperatorSyntax::BitXor,
        TokenKind::ShiftLeft => BinaryOperatorSyntax::ShiftLeft,
        TokenKind::ShiftRight => BinaryOperatorSyntax::ShiftRight,
        _ => return None,
    })
}

pub(super) fn parse_comptime_assertions(
    file: &ProjectFile,
    lexemes: &[Lexeme],
    diagnostics: &mut Vec<Diagnostic>,
    cancellation: &Cancellation,
) -> Vec<ExpressionSyntax> {
    let mut assertions = Vec::new();
    let Some(lines) = token_lines(
        file,
        lexemes,
        0,
        u64::try_from(file.bytes().len()).unwrap_or(u64::MAX),
        cancellation,
    ) else {
        return assertions;
    };
    for line in lines {
        if cancellation.is_cancelled() {
            break;
        }
        if line.indent != 0
            || line
                .tokens
                .first()
                .is_none_or(|token| token.kind != TokenKind::Comptime)
        {
            continue;
        }
        let mut cursor = SyntaxCursor::new(file, &line.tokens, cancellation);
        cursor.advance();
        if !cursor.consume(TokenKind::Assert) {
            continue;
        }
        if let Some(expression) = cursor.parse_complete_expression() {
            assertions.push(expression);
        } else {
            diagnostics.push(Diagnostic::new(
                "syntax.malformed_comptime_assertion",
                line.range,
                RecoveryAction::SkippedToBoundary,
            ));
        }
    }
    assertions
}

pub(super) fn parse_comptime_selections(
    file: &ProjectFile,
    lexemes: &[Lexeme],
    diagnostics: &mut Vec<Diagnostic>,
    cancellation: &Cancellation,
) -> Vec<ComptimeSelection> {
    let Some(lines) = token_lines(
        file,
        lexemes,
        0,
        u64::try_from(file.bytes().len()).unwrap_or(u64::MAX),
        cancellation,
    ) else {
        return Vec::new();
    };
    let mut selections = Vec::new();
    let mut line_index = 0;
    while let Some(line) = lines.get(line_index) {
        if cancellation.is_cancelled() {
            break;
        }
        let begins_selection = line.indent == 0
            && line
                .tokens
                .first()
                .is_some_and(|token| token.kind == TokenKind::Comptime)
            && line
                .tokens
                .get(1)
                .is_some_and(|token| token.kind == TokenKind::If);
        if !begins_selection {
            line_index += 1;
            continue;
        }
        let selection_start = line.range.start();
        let mut branches = Vec::new();
        while let Some(header) = lines.get(line_index) {
            let (condition_start, is_else) = if header
                .tokens
                .first()
                .is_some_and(|token| token.kind == TokenKind::Comptime)
                && header
                    .tokens
                    .get(1)
                    .is_some_and(|token| token.kind == TokenKind::If)
            {
                (2, false)
            } else if header
                .tokens
                .first()
                .is_some_and(|token| token.kind == TokenKind::Elif)
            {
                (1, false)
            } else if header
                .tokens
                .first()
                .is_some_and(|token| token.kind == TokenKind::Else)
            {
                (1, true)
            } else {
                break;
            };
            let well_formed_header = header.indent == 0
                && header
                    .tokens
                    .last()
                    .is_some_and(|token| token.kind == TokenKind::Colon)
                && if is_else {
                    header.tokens.len() == 2
                } else {
                    header.tokens.len() > condition_start + 1
                };
            if !well_formed_header {
                diagnostics.push(Diagnostic::new(
                    "syntax.malformed_comptime_selection",
                    header.range.clone(),
                    RecoveryAction::SkippedToBoundary,
                ));
                line_index += 1;
                break;
            }
            let condition = if is_else {
                None
            } else {
                let condition_tokens =
                    &header.tokens[condition_start..header.tokens.len().saturating_sub(1)];
                let mut cursor = SyntaxCursor::new(file, condition_tokens, cancellation);
                match cursor.parse_complete_expression() {
                    Some(condition) => Some(condition),
                    None => {
                        diagnostics.push(Diagnostic::new(
                            "syntax.malformed_comptime_selection",
                            header.range.clone(),
                            RecoveryAction::SkippedToBoundary,
                        ));
                        line_index += 1;
                        break;
                    }
                }
            };
            line_index += 1;
            let body_start = line_index;
            while lines
                .get(line_index)
                .is_some_and(|candidate| candidate.indent > 0)
            {
                line_index += 1;
            }
            let branch_end = lines.get(line_index).map_or_else(
                || u64::try_from(file.bytes().len()).unwrap_or(u64::MAX),
                |candidate| candidate.range.start(),
            );
            let declarations = parse_comptime_branch_declarations(
                file,
                lexemes,
                &lines[body_start..line_index],
                branch_end,
                diagnostics,
                cancellation,
            );
            if declarations.is_empty() {
                diagnostics.push(Diagnostic::new(
                    "syntax.malformed_comptime_selection",
                    header.range.clone(),
                    RecoveryAction::SkippedToBoundary,
                ));
            }
            branches.push(ComptimeBranch {
                condition,
                declarations,
                range: SourceRange::from_u64_shared(
                    header.range.path_arc(),
                    header.range.start(),
                    branch_end,
                ),
            });
            let continues = lines.get(line_index).is_some_and(|candidate| {
                candidate.indent == 0
                    && candidate.tokens.first().is_some_and(|token| {
                        matches!(token.kind, TokenKind::Elif | TokenKind::Else)
                    })
            });
            if !continues {
                break;
            }
        }
        if !branches.is_empty() {
            let selection_end = branches
                .last()
                .map_or(selection_start, |branch| branch.range.end());
            selections.push(ComptimeSelection {
                branches,
                range: SourceRange::from_u64_shared(
                    file.path_arc(),
                    selection_start,
                    selection_end,
                ),
            });
        }
    }
    selections
}

fn parse_comptime_branch_declarations(
    file: &ProjectFile,
    lexemes: &[Lexeme],
    lines: &[TokenLine<'_>],
    branch_end: u64,
    diagnostics: &mut Vec<Diagnostic>,
    cancellation: &Cancellation,
) -> Vec<Declaration> {
    let mut starts = Vec::new();
    let mut pending_attributes = Vec::new();
    let mut pending_start = None;
    for line in lines {
        if cancellation.is_cancelled() || line.indent != 4 {
            continue;
        }
        let Some(first) = line.tokens.first() else {
            continue;
        };
        if first.kind == TokenKind::At {
            pending_start.get_or_insert(line.range.start());
            pending_attributes.push(
                line.tokens
                    .get(1)
                    .and_then(|token| token_text(file, token))
                    .map_or(AttributeSyntax::Unknown, |name| match name {
                        "image" => AttributeSyntax::Image,
                        "actor" => AttributeSyntax::Actor,
                        _ => AttributeSyntax::Unknown,
                    }),
            );
            continue;
        }
        let mut cursor = 0;
        let public = line
            .tokens
            .get(cursor)
            .is_some_and(|token| token.kind == TokenKind::Pub);
        if public {
            cursor += 1;
        }
        if line
            .tokens
            .get(cursor)
            .is_some_and(|token| matches!(token.kind, TokenKind::Pure | TokenKind::Async))
        {
            cursor += 1;
        }
        let Some(keyword) = line.tokens.get(cursor) else {
            pending_attributes.clear();
            pending_start = None;
            continue;
        };
        let (kind, name_index) = match keyword.kind {
            TokenKind::Fn => (DeclarationKind::Function, cursor + 1),
            TokenKind::Const => (DeclarationKind::Constant, cursor + 1),
            TokenKind::Pool => (DeclarationKind::Pool, cursor + 1),
            TokenKind::Type => (DeclarationKind::TypeAlias, cursor + 1),
            TokenKind::Struct => (DeclarationKind::Struct, cursor + 1),
            TokenKind::Enum => (DeclarationKind::Enum, cursor + 1),
            TokenKind::Interface => (DeclarationKind::Interface, cursor + 1),
            TokenKind::Suite => (DeclarationKind::Suite, cursor + 1),
            TokenKind::Resource
                if line
                    .tokens
                    .get(cursor + 1)
                    .is_some_and(|token| token.kind == TokenKind::Struct) =>
            {
                (DeclarationKind::ResourceStruct, cursor + 2)
            }
            _ => {
                pending_attributes.clear();
                pending_start = None;
                continue;
            }
        };
        let Some(name_token) = line.tokens.get(name_index) else {
            diagnostics.push(Diagnostic::new(
                "syntax.malformed_declaration",
                line.range.clone(),
                RecoveryAction::SkippedToBoundary,
            ));
            continue;
        };
        let Some(name) = token_text(file, name_token) else {
            continue;
        };
        starts.push((
            pending_start.take().unwrap_or(line.range.start()),
            line.range.start(),
            line.range.end(),
            kind,
            name.to_owned(),
            public,
            std::mem::take(&mut pending_attributes),
        ));
    }
    let mut declarations = starts
        .iter()
        .enumerate()
        .map(
            |(index, (start, header_start, header_end, kind, name, public, attributes))| {
                let end = starts
                    .get(index + 1)
                    .map_or(branch_end, |(next_start, ..)| *next_start);
                Declaration {
                    kind: *kind,
                    name: name.clone(),
                    public: *public,
                    attributes: attributes.clone(),
                    syntax: None,
                    range: SourceRange::from_u64_shared(file.path_arc(), *start, *header_end),
                    start: *start,
                    header_start: *header_start,
                    end,
                    structurally_valid: true,
                }
            },
        )
        .collect::<Vec<_>>();
    for declaration in &mut declarations {
        declaration.syntax =
            parse_declaration_syntax(file, declaration, lexemes, diagnostics, cancellation);
        declaration.structurally_valid = declaration.syntax.is_some();
    }
    declarations
}

struct SyntaxCursor<'a, 'tokens> {
    file: &'a ProjectFile,
    tokens: &'tokens [&'tokens Lexeme],
    index: usize,
    cancellation: &'a Cancellation,
}

impl<'a, 'tokens> SyntaxCursor<'a, 'tokens> {
    fn new(
        file: &'a ProjectFile,
        tokens: &'tokens [&'tokens Lexeme],
        cancellation: &'a Cancellation,
    ) -> Self {
        Self {
            file,
            tokens,
            index: 0,
            cancellation,
        }
    }

    fn from_line(
        file: &'a ProjectFile,
        line: &'tokens TokenLine<'a>,
        cancellation: &'a Cancellation,
    ) -> Self {
        Self::new(file, &line.tokens, cancellation)
    }

    fn at_end(&self) -> bool {
        self.index == self.tokens.len()
    }
    fn peek_kind(&self) -> Option<TokenKind> {
        self.tokens.get(self.index).map(|token| token.kind)
    }
    fn peek_n_kind(&self, offset: usize) -> Option<TokenKind> {
        self.tokens.get(self.index + offset).map(|token| token.kind)
    }
    fn advance(&mut self) {
        if self.cancellation.is_cancelled() {
            self.index = self.tokens.len();
            return;
        }
        self.index = self.index.saturating_add(1);
    }
    fn consume(&mut self, kind: TokenKind) -> bool {
        if self.peek_kind() == Some(kind) {
            self.advance();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, kind: TokenKind) -> Option<()> {
        self.consume(kind).then_some(())
    }
    fn current_range(&self) -> Option<&SourceRange> {
        self.tokens.get(self.index).map(|token| &token.range)
    }
    fn previous_range(&self) -> Option<&SourceRange> {
        self.index
            .checked_sub(1)
            .and_then(|index| self.tokens.get(index))
            .map(|token| &token.range)
    }
    fn expect_identifier(&mut self) -> Option<&'a str> {
        if self.cancellation.is_cancelled() {
            return None;
        }
        let token = *self.tokens.get(self.index)?;
        (token.kind == TokenKind::Identifier).then_some(())?;
        self.index += 1;
        token_text(self.file, token)
    }

    fn expect_integer_literal(&mut self) -> Option<&'a str> {
        if self.cancellation.is_cancelled() {
            return None;
        }
        let token = *self.tokens.get(self.index)?;
        (token.kind == TokenKind::IntegerLiteral).then_some(())?;
        self.index += 1;
        token_text(self.file, token)
    }

    fn parse_name(&mut self) -> Option<NameSyntax> {
        let first = if self.consume(TokenKind::SelfValue) {
            "self".to_owned()
        } else {
            self.expect_identifier()?.to_owned()
        };
        let mut segments = vec![first];
        while self.consume(TokenKind::Dot) {
            segments.push(self.expect_identifier()?.to_owned());
        }
        Some(NameSyntax { segments })
    }

    fn parse_type(&mut self) -> Option<TypeSyntax> {
        self.parse_type_at(0)
    }

    fn parse_type_at(&mut self, depth: usize) -> Option<TypeSyntax> {
        if depth >= MAX_NESTING || self.cancellation.is_cancelled() {
            return None;
        }
        if self.consume(TokenKind::Fn) {
            self.expect(TokenKind::LeftParen)?;
            let mut parameters = Vec::new();
            while !self.consume(TokenKind::RightParen) {
                parameters.push(self.parse_type_at(depth + 1)?);
                if !self.consume(TokenKind::Comma) {
                    self.expect(TokenKind::RightParen)?;
                    break;
                }
            }
            self.expect(TokenKind::Arrow)?;
            return Some(TypeSyntax::Function {
                parameters,
                return_type: Box::new(self.parse_type_at(depth + 1)?),
            });
        }
        if self.consume(TokenKind::Own) {
            self.expect(TokenKind::LeftBracket)?;
            let pool = self.parse_name()?;
            self.expect(TokenKind::RightBracket)?;
            return Some(TypeSyntax::Own {
                pool,
                value: Box::new(self.parse_type_at(depth + 1)?),
            });
        }
        if self.consume(TokenKind::Any) {
            return Some(TypeSyntax::Any(self.parse_name()?));
        }
        if self.consume(TokenKind::LeftParen) {
            if self.consume(TokenKind::RightParen) {
                return Some(TypeSyntax::Unit);
            }
            let first = self.parse_type_at(depth + 1)?;
            if !self.consume(TokenKind::Comma) {
                self.expect(TokenKind::RightParen)?;
                return Some(first);
            }
            let mut members = vec![first];
            while !self.consume(TokenKind::RightParen) {
                members.push(self.parse_type_at(depth + 1)?);
                if !self.consume(TokenKind::Comma) {
                    self.expect(TokenKind::RightParen)?;
                    break;
                }
            }
            return Some(TypeSyntax::Tuple(members));
        }
        if self.consume(TokenKind::LeftBracket) {
            let element = self.parse_type_at(depth + 1)?;
            if self.consume(TokenKind::Semicolon) {
                let length = if self.peek_kind() == Some(TokenKind::IntegerLiteral) {
                    FixedArrayLengthSyntax::Literal(self.expect_integer_literal()?.parse().ok()?)
                } else {
                    FixedArrayLengthSyntax::Parameter(self.expect_identifier()?.to_owned())
                };
                self.expect(TokenKind::RightBracket)?;
                return Some(TypeSyntax::FixedArray {
                    element: Box::new(element),
                    length,
                });
            }
            self.expect(TokenKind::RightBracket)?;
            return Some(TypeSyntax::Array(Box::new(element)));
        }
        let base = self.parse_name()?;
        if self.consume(TokenKind::LeftBracket) {
            let mut arguments = Vec::new();
            while !self.consume(TokenKind::RightBracket) {
                if self.peek_kind() == Some(TokenKind::IntegerLiteral) {
                    arguments.push(TypeSyntax::ConstU64(
                        self.expect_integer_literal()?.parse().ok()?,
                    ));
                } else if self.peek_kind() == Some(TokenKind::Identifier)
                    && self
                        .tokens
                        .get(self.index)
                        .and_then(|token| token_text(self.file, token))
                        == Some("_")
                {
                    self.advance();
                    arguments.push(TypeSyntax::Infer);
                } else {
                    arguments.push(self.parse_type_at(depth + 1)?);
                }
                if !self.consume(TokenKind::Comma) {
                    self.expect(TokenKind::RightBracket)?;
                    break;
                }
            }
            Some(TypeSyntax::Apply { base, arguments })
        } else {
            Some(TypeSyntax::Named(base))
        }
    }

    fn parse_complete_expression(&mut self) -> Option<ExpressionSyntax> {
        let checkpoint = self.index;
        if let Some(expression) = self.parse_expression(0)
            && self.at_end()
        {
            return Some(expression);
        }
        self.index = checkpoint;
        let kind = if self
            .tokens
            .iter()
            .any(|token| token.kind == TokenKind::Send)
        {
            UnsupportedExpressionKind::Send
        } else {
            return None;
        };
        let first = self.tokens.get(checkpoint)?.range.clone();
        let last = self.tokens.last()?.range.clone();
        self.index = self.tokens.len();
        Some(ExpressionSyntax {
            kind: ExpressionSyntaxKind::Unsupported(kind),
            range: SourceRange::from_u64_shared(first.path_arc(), first.start(), last.end()),
        })
    }

    fn parse_place(&mut self) -> Option<PlaceSyntax> {
        let first = *self.tokens.get(self.index)?;
        let root = self.expect_identifier()?.to_owned();
        let mut projections = Vec::new();
        while !self.cancellation.is_cancelled() {
            if self.consume(TokenKind::Dot) {
                let token = *self.tokens.get(self.index)?;
                let name = self.expect_identifier()?.to_owned();
                projections.push(PlaceProjectionSyntax::Field {
                    name,
                    range: token.range.clone(),
                });
            } else if self.consume(TokenKind::LeftBracket) {
                let index = self.parse_expression(0)?;
                self.expect(TokenKind::RightBracket)?;
                projections.push(PlaceProjectionSyntax::Index(index));
            } else {
                break;
            }
        }
        Some(PlaceSyntax {
            root,
            projections,
            range: SourceRange::from_u64_shared(
                first.range.path_arc(),
                first.range.start(),
                self.previous_range()?.end(),
            ),
        })
    }

    fn parse_pattern(&mut self) -> Option<PatternSyntax> {
        let first = self.parse_pattern_primary(0)?;
        if !self.consume(TokenKind::Or) {
            return Some(first);
        }
        let start = first.range.start();
        let path = first.range.path_arc().clone();
        let mut alternatives = vec![first];
        loop {
            alternatives.push(self.parse_pattern_primary(0)?);
            if !self.consume(TokenKind::Or) {
                break;
            }
        }
        let end = alternatives.last()?.range.end();
        Some(PatternSyntax {
            kind: PatternSyntaxKind::Or(alternatives),
            range: SourceRange::from_u64_shared(&path, start, end),
        })
    }

    fn parse_pattern_primary(&mut self, depth: usize) -> Option<PatternSyntax> {
        if depth >= MAX_NESTING || self.cancellation.is_cancelled() {
            return None;
        }
        let token = *self.tokens.get(self.index)?;
        if token.kind == TokenKind::Take {
            self.advance();
            let pattern = self.parse_pattern_primary(depth + 1)?;
            let end = pattern.range.end();
            return Some(PatternSyntax {
                kind: PatternSyntaxKind::Take(Box::new(pattern)),
                range: SourceRange::from_u64_shared(
                    token.range.path_arc(),
                    token.range.start(),
                    end,
                ),
            });
        }
        if token.kind == TokenKind::Identifier && token_text(self.file, token) == Some("_") {
            self.advance();
            return Some(PatternSyntax {
                kind: PatternSyntaxKind::Wildcard,
                range: token.range.clone(),
            });
        }
        if token.kind == TokenKind::LeftParen {
            self.advance();
            if self.consume(TokenKind::RightParen) {
                let expression = ExpressionSyntax {
                    kind: ExpressionSyntaxKind::Unit,
                    range: SourceRange::from_u64_shared(
                        token.range.path_arc(),
                        token.range.start(),
                        self.previous_range()?.end(),
                    ),
                };
                return Some(PatternSyntax {
                    range: expression.range.clone(),
                    kind: PatternSyntaxKind::Literal(expression),
                });
            }
            let first = self.parse_pattern()?;
            if !self.consume(TokenKind::Comma) {
                self.expect(TokenKind::RightParen)?;
                return Some(first);
            }
            let mut members = vec![first];
            while !self.consume(TokenKind::RightParen) {
                members.push(self.parse_pattern()?);
                if !self.consume(TokenKind::Comma) {
                    self.expect(TokenKind::RightParen)?;
                    break;
                }
            }
            return Some(PatternSyntax {
                kind: PatternSyntaxKind::Tuple(members),
                range: SourceRange::from_u64_shared(
                    token.range.path_arc(),
                    token.range.start(),
                    self.previous_range()?.end(),
                ),
            });
        }
        if token.kind == TokenKind::LeftBracket {
            self.advance();
            let mut members = Vec::new();
            while !self.consume(TokenKind::RightBracket) {
                members.push(self.parse_pattern()?);
                if !self.consume(TokenKind::Comma) {
                    self.expect(TokenKind::RightBracket)?;
                    break;
                }
            }
            return Some(PatternSyntax {
                kind: PatternSyntaxKind::FixedArray(members),
                range: SourceRange::from_u64_shared(
                    token.range.path_arc(),
                    token.range.start(),
                    self.previous_range()?.end(),
                ),
            });
        }
        if matches!(token.kind, TokenKind::Identifier | TokenKind::SelfValue) {
            let name = self.parse_name()?;
            let end = self.previous_range()?.end();
            if name.segments.len() == 1 && self.peek_kind() != Some(TokenKind::LeftParen) {
                return Some(PatternSyntax {
                    kind: PatternSyntaxKind::Binding(name.segments[0].clone()),
                    range: SourceRange::from_u64_shared(
                        token.range.path_arc(),
                        token.range.start(),
                        end,
                    ),
                });
            }
            let mut arguments = Vec::new();
            if self.consume(TokenKind::LeftParen) {
                while !self.consume(TokenKind::RightParen) {
                    let label = if self.peek_kind() == Some(TokenKind::Identifier)
                        && self.peek_n_kind(1) == Some(TokenKind::Equal)
                    {
                        let label = self.expect_identifier()?.to_owned();
                        self.expect(TokenKind::Equal)?;
                        Some(label)
                    } else {
                        None
                    };
                    arguments.push(PatternArgumentSyntax {
                        label,
                        pattern: self.parse_pattern()?,
                    });
                    if !self.consume(TokenKind::Comma) {
                        self.expect(TokenKind::RightParen)?;
                        break;
                    }
                }
            }
            return Some(PatternSyntax {
                kind: PatternSyntaxKind::Constructor { name, arguments },
                range: SourceRange::from_u64_shared(
                    token.range.path_arc(),
                    token.range.start(),
                    self.previous_range()?.end(),
                ),
            });
        }
        if matches!(
            token.kind,
            TokenKind::IntegerLiteral
                | TokenKind::FloatLiteral
                | TokenKind::TextLiteral
                | TokenKind::ScalarLiteral
                | TokenKind::BytesLiteral
                | TokenKind::True
                | TokenKind::False
                | TokenKind::Minus
                | TokenKind::Plus
        ) {
            let expression = self.parse_expression_at(12, depth + 1)?;
            return Some(PatternSyntax {
                range: expression.range.clone(),
                kind: PatternSyntaxKind::Literal(expression),
            });
        }
        None
    }

    fn parse_expression(&mut self, minimum_precedence: u8) -> Option<ExpressionSyntax> {
        self.parse_expression_at(minimum_precedence, 0)
    }

    fn parse_expression_at(
        &mut self,
        minimum_precedence: u8,
        depth: usize,
    ) -> Option<ExpressionSyntax> {
        if depth >= MAX_NESTING || self.cancellation.is_cancelled() {
            return None;
        }
        let mut left = self.parse_primary(depth)?;
        if self.consume(TokenKind::Question) {
            let range = SourceRange::from_u64_shared(
                left.range.path_arc(),
                left.range.start(),
                self.previous_range()?.end(),
            );
            left = ExpressionSyntax {
                kind: ExpressionSyntaxKind::Propagate(Box::new(left)),
                range,
            };
        }
        loop {
            if minimum_precedence <= 5 && self.consume(TokenKind::Is) {
                let pattern = self.parse_pattern()?;
                let range = SourceRange::from_u64_shared(
                    left.range.path_arc(),
                    left.range.start(),
                    pattern.range.end(),
                );
                left = ExpressionSyntax {
                    kind: ExpressionSyntaxKind::Is {
                        value: Box::new(left),
                        pattern: Box::new(pattern),
                    },
                    range,
                };
                continue;
            }
            let Some((operator, precedence)) = self.binary_operator() else {
                break;
            };
            if precedence < minimum_precedence {
                break;
            }
            self.advance();
            let right = self.parse_expression_at(precedence + 1, depth + 1)?;
            let range = SourceRange::from_u64_shared(
                left.range.path_arc(),
                left.range.start(),
                right.range.end(),
            );
            left = ExpressionSyntax {
                kind: ExpressionSyntaxKind::Binary {
                    operator,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                range,
            };
        }
        Some(left)
    }

    fn parse_primary(&mut self, depth: usize) -> Option<ExpressionSyntax> {
        let token = *self.tokens.get(self.index)?;
        self.index += 1;
        let start = token.range.start();
        let path = token.range.path_arc();
        let mut expression = match token.kind {
            TokenKind::IntegerLiteral => ExpressionSyntax {
                kind: ExpressionSyntaxKind::Integer(token_text(self.file, token)?.to_owned()),
                range: token.range.clone(),
            },
            TokenKind::FloatLiteral => ExpressionSyntax {
                kind: ExpressionSyntaxKind::Float(token_text(self.file, token)?.to_owned()),
                range: token.range.clone(),
            },
            TokenKind::TextLiteral => {
                let text = token_text(self.file, token)?;
                ExpressionSyntax {
                    kind: ExpressionSyntaxKind::Text(if text.starts_with("\"\"\"") {
                        decode_multiline_text_literal(text.as_bytes())?
                    } else {
                        decode_text_literal(text.as_bytes())?
                    }),
                    range: token.range.clone(),
                }
            }
            TokenKind::ScalarLiteral => ExpressionSyntax {
                kind: ExpressionSyntaxKind::Scalar(decode_scalar_literal(
                    token_text(self.file, token)?.as_bytes(),
                )?),
                range: token.range.clone(),
            },
            TokenKind::BytesLiteral => ExpressionSyntax {
                kind: ExpressionSyntaxKind::Bytes(decode_bytes_literal(
                    token_text(self.file, token)?.as_bytes(),
                )?),
                range: token.range.clone(),
            },
            TokenKind::True | TokenKind::False => ExpressionSyntax {
                kind: ExpressionSyntaxKind::Bool(token.kind == TokenKind::True),
                range: token.range.clone(),
            },
            TokenKind::Identifier | TokenKind::SelfValue => {
                self.index -= 1;
                let name = self.parse_name()?;
                let end = self.previous_range()?.end();
                if self.consume(TokenKind::LeftParen) {
                    let mut arguments = Vec::new();
                    let mut saw_named = false;
                    while !self.consume(TokenKind::RightParen) {
                        let label = if self.peek_kind() == Some(TokenKind::Identifier)
                            && self.peek_n_kind(1) == Some(TokenKind::Equal)
                        {
                            let label = self.expect_identifier()?.to_owned();
                            self.expect(TokenKind::Equal)?;
                            saw_named = true;
                            Some(label)
                        } else {
                            if saw_named {
                                return None;
                            }
                            None
                        };
                        arguments.push(ArgumentSyntax {
                            label,
                            value: self.parse_expression_at(0, depth + 1)?,
                        });
                        if !self.consume(TokenKind::Comma) {
                            self.expect(TokenKind::RightParen)?;
                            break;
                        }
                    }
                    ExpressionSyntax {
                        kind: ExpressionSyntaxKind::Call {
                            callee: name,
                            arguments,
                        },
                        range: SourceRange::from_u64_shared(
                            path,
                            start,
                            self.previous_range()?.end(),
                        ),
                    }
                } else {
                    ExpressionSyntax {
                        kind: ExpressionSyntaxKind::Name(name),
                        range: SourceRange::from_u64_shared(path, start, end),
                    }
                }
            }
            TokenKind::LeftParen => {
                if self.consume(TokenKind::RightParen) {
                    ExpressionSyntax {
                        kind: ExpressionSyntaxKind::Unit,
                        range: SourceRange::from_u64_shared(
                            path,
                            start,
                            self.previous_range()?.end(),
                        ),
                    }
                } else {
                    let first = self.parse_expression_at(0, depth + 1)?;
                    if self.consume(TokenKind::Comma) {
                        let mut values = vec![first];
                        while !self.consume(TokenKind::RightParen) {
                            values.push(self.parse_expression_at(0, depth + 1)?);
                            if !self.consume(TokenKind::Comma) {
                                self.expect(TokenKind::RightParen)?;
                                break;
                            }
                        }
                        ExpressionSyntax {
                            kind: ExpressionSyntaxKind::Tuple(values),
                            range: SourceRange::from_u64_shared(
                                path,
                                start,
                                self.previous_range()?.end(),
                            ),
                        }
                    } else {
                        let mut inner = first;
                        self.expect(TokenKind::RightParen)?;
                        inner.range =
                            SourceRange::from_u64_shared(path, start, self.previous_range()?.end());
                        inner
                    }
                }
            }
            TokenKind::LeftBracket => {
                let mut values = Vec::new();
                while !self.consume(TokenKind::RightBracket) {
                    let value = self.parse_expression_at(0, depth + 1)?;
                    if values.is_empty() && self.consume(TokenKind::Semicolon) {
                        let length = self.expect_integer_literal()?.parse().ok()?;
                        self.expect(TokenKind::RightBracket)?;
                        return Some(ExpressionSyntax {
                            kind: ExpressionSyntaxKind::RepeatedArray {
                                value: Box::new(value),
                                length,
                            },
                            range: SourceRange::from_u64_shared(
                                path,
                                start,
                                self.previous_range()?.end(),
                            ),
                        });
                    }
                    values.push(value);
                    if !self.consume(TokenKind::Comma) {
                        self.expect(TokenKind::RightBracket)?;
                        break;
                    }
                }
                ExpressionSyntax {
                    kind: ExpressionSyntaxKind::Array(values),
                    range: SourceRange::from_u64_shared(path, start, self.previous_range()?.end()),
                }
            }
            TokenKind::Plus => {
                let value = self.parse_expression_at(12, depth + 1)?;
                let end = value.range.end();
                ExpressionSyntax {
                    kind: ExpressionSyntaxKind::Positive(Box::new(value)),
                    range: SourceRange::from_u64_shared(path, start, end),
                }
            }
            TokenKind::Minus => {
                let value = self.parse_expression_at(12, depth + 1)?;
                let end = value.range.end();
                ExpressionSyntax {
                    kind: ExpressionSyntaxKind::Negate(Box::new(value)),
                    range: SourceRange::from_u64_shared(path, start, end),
                }
            }
            TokenKind::Tilde => {
                let value = self.parse_expression_at(12, depth + 1)?;
                let end = value.range.end();
                ExpressionSyntax {
                    kind: ExpressionSyntaxKind::BitNot(Box::new(value)),
                    range: SourceRange::from_u64_shared(path, start, end),
                }
            }
            TokenKind::Not => {
                let value = self.parse_expression_at(4, depth + 1)?;
                let end = value.range.end();
                ExpressionSyntax {
                    kind: ExpressionSyntaxKind::Not(Box::new(value)),
                    range: SourceRange::from_u64_shared(path, start, end),
                }
            }
            TokenKind::Await => {
                let value = self.parse_expression_at(12, depth + 1)?;
                let end = value.range.end();
                ExpressionSyntax {
                    kind: ExpressionSyntaxKind::Await(Box::new(value)),
                    range: SourceRange::from_u64_shared(path, start, end),
                }
            }
            TokenKind::Send => {
                let value = self.parse_expression_at(12, depth + 1)?;
                let end = value.range.end();
                ExpressionSyntax {
                    kind: ExpressionSyntaxKind::Send(Box::new(value)),
                    range: SourceRange::from_u64_shared(path, start, end),
                }
            }
            TokenKind::TrySend => {
                let value = self.parse_expression_at(12, depth + 1)?;
                let end = value.range.end();
                ExpressionSyntax {
                    kind: ExpressionSyntaxKind::TrySend(Box::new(value)),
                    range: SourceRange::from_u64_shared(path, start, end),
                }
            }
            TokenKind::Mut => {
                let value = self.parse_expression_at(12, depth + 1)?;
                let end = value.range.end();
                ExpressionSyntax {
                    kind: ExpressionSyntaxKind::Mut(Box::new(value)),
                    range: SourceRange::from_u64_shared(path, start, end),
                }
            }
            TokenKind::Take => {
                let value = self.parse_expression_at(12, depth + 1)?;
                let end = value.range.end();
                ExpressionSyntax {
                    kind: ExpressionSyntaxKind::Take(Box::new(value)),
                    range: SourceRange::from_u64_shared(path, start, end),
                }
            }
            TokenKind::Pipe => {
                let mut parameters = Vec::new();
                while !self.consume(TokenKind::Pipe) {
                    let parameter_start = self.tokens.get(self.index)?.range.clone();
                    let name = self.expect_identifier()?.to_owned();
                    let type_ = if self.consume(TokenKind::Colon) {
                        Some(self.parse_type_at(depth + 1)?)
                    } else {
                        None
                    };
                    let end = self.previous_range()?.end();
                    parameters.push(ClosureParameterSyntax {
                        name,
                        type_,
                        range: SourceRange::from_u64_shared(
                            parameter_start.path_arc(),
                            parameter_start.start(),
                            end,
                        ),
                    });
                    if !self.consume(TokenKind::Comma) {
                        self.expect(TokenKind::Pipe)?;
                        break;
                    }
                }
                let body = self.parse_expression_at(0, depth + 1)?;
                let end = body.range.end();
                ExpressionSyntax {
                    kind: ExpressionSyntaxKind::Closure {
                        parameters,
                        body: Box::new(body),
                    },
                    range: SourceRange::from_u64_shared(path, start, end),
                }
            }
            _ => return None,
        };
        loop {
            if self.consume(TokenKind::LeftBracket) {
                let index = self.parse_expression_at(0, depth + 1)?;
                self.expect(TokenKind::RightBracket)?;
                expression = ExpressionSyntax {
                    range: SourceRange::from_u64_shared(path, start, self.previous_range()?.end()),
                    kind: ExpressionSyntaxKind::Index {
                        value: Box::new(expression),
                        index: Box::new(index),
                    },
                };
            } else if self.consume(TokenKind::Question) {
                expression = ExpressionSyntax {
                    range: SourceRange::from_u64_shared(path, start, self.previous_range()?.end()),
                    kind: ExpressionSyntaxKind::Propagate(Box::new(expression)),
                };
            } else {
                break;
            }
        }
        Some(expression)
    }

    fn binary_operator(&self) -> Option<(BinaryOperatorSyntax, u8)> {
        Some(match self.peek_kind()? {
            TokenKind::Range => (BinaryOperatorSyntax::Range, 1),
            TokenKind::RangeInclusive => (BinaryOperatorSyntax::RangeInclusive, 1),
            TokenKind::EqualEqual => (BinaryOperatorSyntax::Equal, 5),
            TokenKind::BangEqual => (BinaryOperatorSyntax::NotEqual, 5),
            TokenKind::Less => (BinaryOperatorSyntax::Less, 5),
            TokenKind::LessEqual => (BinaryOperatorSyntax::LessEqual, 5),
            TokenKind::Greater => (BinaryOperatorSyntax::Greater, 5),
            TokenKind::GreaterEqual => (BinaryOperatorSyntax::GreaterEqual, 5),
            TokenKind::Plus => (BinaryOperatorSyntax::Add, 10),
            TokenKind::Minus => (BinaryOperatorSyntax::Subtract, 10),
            TokenKind::Pipe => (BinaryOperatorSyntax::BitOr, 6),
            TokenKind::Caret => (BinaryOperatorSyntax::BitXor, 7),
            TokenKind::Ampersand => (BinaryOperatorSyntax::BitAnd, 8),
            TokenKind::ShiftLeft => (BinaryOperatorSyntax::ShiftLeft, 9),
            TokenKind::ShiftRight => (BinaryOperatorSyntax::ShiftRight, 9),
            TokenKind::Star => (BinaryOperatorSyntax::Multiply, 11),
            TokenKind::Slash => (BinaryOperatorSyntax::Divide, 11),
            TokenKind::Percent => (BinaryOperatorSyntax::Remainder, 11),
            TokenKind::And => (BinaryOperatorSyntax::And, 3),
            TokenKind::Or => (BinaryOperatorSyntax::Or, 2),
            _ => return None,
        })
    }
}
