#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use crate::{
    Cancellation, Diagnostic, ProjectFile, RecoveryAction, SourceRange, SyntaxElement,
    SyntaxElementKind, SyntaxNodeObservation,
};

const MAX_SOURCE_BYTES: usize = 1_048_576;
const MAX_NESTING: usize = 256;

pub(crate) struct ParsedSource {
    pub(crate) elements: Vec<SyntaxElement>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) imports: Vec<Import>,
    pub(crate) declarations: Vec<Declaration>,
    pub(crate) comptime_assertions: Vec<ExpressionSyntax>,
    tree: GreenNode,
    pub(crate) cancelled: bool,
}

#[derive(Clone, Debug)]
struct GreenNode {
    kind: GreenKind,
    range: SourceRange,
    children: std::sync::Arc<[GreenChild]>,
}

#[derive(Clone, Debug)]
enum GreenChild {
    Node(GreenNode),
    Leaf(SyntaxElement),
}

#[derive(Clone, Copy, Debug)]
enum GreenKind {
    Source,
    Syntax(&'static str),
}

enum Event {
    Start(GreenKind, SourceRange),
    Token(usize),
    Missing(usize),
    Error(usize),
    Finish,
}

struct SyntaxRegion {
    kind: &'static str,
    range: SourceRange,
    children: Vec<SyntaxRegion>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenKind {
    Identifier,
    IntegerLiteral,
    FloatLiteral,
    TextLiteral,
    Fn,
    Pub,
    Pure,
    Async,
    Return,
    If,
    Else,
    Const,
    Struct,
    Resource,
    Enum,
    Interface,
    Type,
    Pool,
    Suite,
    Test,
    From,
    Import,
    As,
    Comptime,
    Assert,
    Await,
    Panic,
    Pass,
    Take,
    Expect,
    True,
    False,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Colon,
    Comma,
    Dot,
    At,
    Arrow,
    Equal,
    EqualEqual,
    BangEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Question,
    Invalid,
}

#[derive(Clone, Debug)]
struct Lexeme {
    kind: TokenKind,
    range: SourceRange,
}

impl TokenKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Identifier => "identifier",
            Self::IntegerLiteral => "integer_literal",
            Self::FloatLiteral => "float_literal",
            Self::TextLiteral => "text_literal",
            Self::Fn => "fn",
            Self::Pub => "pub",
            Self::Pure => "pure",
            Self::Async => "async",
            Self::Return => "return",
            Self::If => "if",
            Self::Else => "else",
            Self::Const => "const",
            Self::Struct => "struct",
            Self::Resource => "resource",
            Self::Enum => "enum",
            Self::Interface => "interface",
            Self::Type => "type",
            Self::Pool => "pool",
            Self::Suite => "suite",
            Self::Test => "test",
            Self::From => "from",
            Self::Import => "import",
            Self::As => "as",
            Self::Comptime => "comptime",
            Self::Assert => "assert",
            Self::Await => "await",
            Self::Panic => "panic",
            Self::Pass => "pass",
            Self::Take => "take",
            Self::Expect => "expect",
            Self::True => "true",
            Self::False => "false",
            Self::LeftParen => "left_paren",
            Self::RightParen => "right_paren",
            Self::LeftBracket => "left_bracket",
            Self::RightBracket => "right_bracket",
            Self::Colon => "colon",
            Self::Comma => "comma",
            Self::Dot => "dot",
            Self::At => "at",
            Self::Arrow => "arrow",
            Self::Equal => "equal",
            Self::EqualEqual => "equal_equal",
            Self::BangEqual => "bang_equal",
            Self::Less => "less",
            Self::LessEqual => "less_equal",
            Self::Greater => "greater",
            Self::GreaterEqual => "greater_equal",
            Self::Plus => "plus",
            Self::Minus => "minus",
            Self::Star => "star",
            Self::Slash => "slash",
            Self::Percent => "percent",
            Self::Question => "question",
            Self::Invalid => "invalid",
        }
    }
}

fn classify_token_bytes(bytes: &[u8]) -> TokenKind {
    match bytes {
        b"fn" => TokenKind::Fn,
        b"pub" => TokenKind::Pub,
        b"pure" => TokenKind::Pure,
        b"async" => TokenKind::Async,
        b"return" => TokenKind::Return,
        b"if" => TokenKind::If,
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
        b"await" => TokenKind::Await,
        b"panic" => TokenKind::Panic,
        b"pass" => TokenKind::Pass,
        b"take" => TokenKind::Take,
        b"expect" => TokenKind::Expect,
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

#[derive(Clone, Debug)]
pub(crate) struct Declaration {
    pub(crate) kind: DeclarationKind,
    pub(crate) name: String,
    pub(crate) public: bool,
    pub(crate) attributes: Vec<AttributeSyntax>,
    pub(crate) syntax: Option<DeclarationSyntax>,
    pub(crate) range: SourceRange,
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) structurally_valid: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DeclarationKind {
    Function,
    Constant,
    Pool,
    TypeAlias,
    Struct,
    ResourceStruct,
    Enum,
    Interface,
    Suite,
}

impl DeclarationKind {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Constant => "constant",
            Self::Pool => "pool",
            Self::TypeAlias => "type_alias",
            Self::Struct => "struct",
            Self::ResourceStruct => "resource_struct",
            Self::Enum => "enum",
            Self::Interface => "interface",
            Self::Suite => "suite",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FunctionModifier {
    Ordinary,
    Pure,
    Async,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OwnershipSyntax {
    Value,
    Take,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AttributeSyntax {
    Image,
    Actor,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NameSyntax {
    pub(crate) segments: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TypeSyntax {
    Unit,
    Named(NameSyntax),
    Apply {
        base: NameSyntax,
        arguments: Vec<TypeSyntax>,
    },
    Array(Box<TypeSyntax>),
    Tuple(Vec<TypeSyntax>),
    Infer,
}

#[derive(Clone, Debug)]
pub(crate) struct ParameterSyntax {
    pub(crate) name: String,
    pub(crate) ownership: OwnershipSyntax,
    pub(crate) type_syntax: TypeSyntax,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) enum DeclarationSyntax {
    Function(FunctionSyntax),
    Constant(ConstantSyntax),
    Suite(SuiteSyntax),
    Enum(EnumSyntax),
    Named,
}

#[derive(Clone, Debug)]
pub(crate) struct EnumSyntax {
    pub(crate) variants: Vec<VariantSyntax>,
}

#[derive(Clone, Debug)]
pub(crate) struct VariantSyntax {
    pub(crate) name: String,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct FunctionSyntax {
    pub(crate) modifier: FunctionModifier,
    pub(crate) type_parameters: Vec<String>,
    pub(crate) parameters: Vec<ParameterSyntax>,
    pub(crate) return_type: TypeSyntax,
    pub(crate) body: Vec<StatementSyntax>,
}

#[derive(Clone, Debug)]
pub(crate) struct ConstantSyntax {
    pub(crate) type_syntax: TypeSyntax,
    pub(crate) value: ExpressionSyntax,
}

#[derive(Clone, Debug)]
pub(crate) struct SuiteSyntax {
    pub(crate) tests: Vec<TestSyntax>,
}

#[derive(Clone, Debug)]
pub(crate) struct TestSyntax {
    pub(crate) name: String,
    pub(crate) asynchronous: bool,
    pub(crate) parameters: Vec<ParameterSyntax>,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) enum StatementSyntax {
    Return {
        value: Option<ExpressionSyntax>,
        range: SourceRange,
    },
    Panic {
        value: ExpressionSyntax,
        range: SourceRange,
    },
    Assign {
        name: String,
        value: ExpressionSyntax,
        range: SourceRange,
    },
    Evaluate(ExpressionSyntax),
    If {
        condition: ExpressionSyntax,
        then_branch: Vec<StatementSyntax>,
        else_branch: Vec<StatementSyntax>,
        range: SourceRange,
    },
    Pass(SourceRange),
}

#[derive(Clone, Debug)]
pub(crate) struct ExpressionSyntax {
    pub(crate) kind: ExpressionSyntaxKind,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) enum ExpressionSyntaxKind {
    Integer(String),
    Float(String),
    Text(String),
    Bool(bool),
    Name(NameSyntax),
    Call {
        callee: NameSyntax,
        arguments: Vec<ArgumentSyntax>,
    },
    Array(Vec<ExpressionSyntax>),
    Unit,
    Negate(Box<ExpressionSyntax>),
    Await(Box<ExpressionSyntax>),
    Propagate(Box<ExpressionSyntax>),
    Binary {
        operator: BinaryOperatorSyntax,
        left: Box<ExpressionSyntax>,
        right: Box<ExpressionSyntax>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct ArgumentSyntax {
    pub(crate) label: Option<String>,
    pub(crate) value: ExpressionSyntax,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinaryOperatorSyntax {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Clone, Debug)]
pub(crate) struct Import {
    pub(crate) target_path: String,
    pub(crate) alias: String,
    pub(crate) range: SourceRange,
}

impl ParsedSource {
    pub(crate) fn declaration_bytes<'a>(
        &self,
        file: &'a ProjectFile,
        declaration: &Declaration,
    ) -> &'a [u8] {
        checked_slice(file.bytes(), declaration.start, declaration.end).unwrap_or_default()
    }

    pub(crate) fn node_observations(&self) -> Vec<SyntaxNodeObservation> {
        let mut result = Vec::new();
        self.tree.project(0, &mut result);
        result
    }
}

impl GreenNode {
    fn project(&self, depth: u16, output: &mut Vec<SyntaxNodeObservation>) {
        let kind = match self.kind {
            GreenKind::Source => "source",
            GreenKind::Syntax(kind) => kind,
        };
        output.push(SyntaxNodeObservation::new(kind, self.range.clone(), depth));
        for child in &*self.children {
            if let GreenChild::Node(node) = child {
                node.project(depth.saturating_add(1), output);
            }
        }
    }

    fn authored_bytes(&self) -> u64 {
        self.children
            .iter()
            .map(|child| match child {
                GreenChild::Node(node) => node.authored_bytes(),
                GreenChild::Leaf(leaf)
                    if matches!(
                        leaf.kind(),
                        SyntaxElementKind::Token
                            | SyntaxElementKind::Trivia
                            | SyntaxElementKind::Invalid
                    ) =>
                {
                    leaf.range().end().saturating_sub(leaf.range().start())
                }
                GreenChild::Leaf(_) => 0,
            })
            .sum()
    }
}

pub(crate) fn parse(file: &ProjectFile, cancellation: &Cancellation) -> ParsedSource {
    let bytes = file.bytes();
    let path = file.path();
    if bytes.len() > MAX_SOURCE_BYTES {
        let elements = vec![SyntaxElement::new(
            SyntaxElementKind::Invalid,
            "oversized_source",
            path,
            0,
            bytes.len(),
        )];
        return ParsedSource {
            tree: build_green_tree(file, &[], &elements, cancellation)
                .unwrap_or_else(|| cancelled_green(file)),
            elements,
            diagnostics: vec![Diagnostic::new(
                "syntax.source_too_large",
                SourceRange::new(path, MAX_SOURCE_BYTES, bytes.len()),
                RecoveryAction::PreservedInvalidBytes,
            )],
            imports: Vec::new(),
            declarations: Vec::new(),
            comptime_assertions: Vec::new(),
            cancelled: false,
        };
    }
    let mut elements = Vec::new();
    let mut diagnostics = Vec::new();
    let mut imports = Vec::new();
    let mut declarations = Vec::new();
    let mut lexemes = Vec::new();
    let mut offset = 0;

    while offset < bytes.len() {
        if offset.is_multiple_of(256) && cancellation.is_cancelled() {
            return cancelled_source(file, elements, diagnostics, imports, declarations);
        }
        let start = offset;
        let (kind, name, token_kind) = match bytes[offset] {
            b' ' => {
                offset += 1;
                while bytes.get(offset) == Some(&b' ') {
                    if offset.is_multiple_of(256) && cancellation.is_cancelled() {
                        break;
                    }
                    offset += 1;
                }
                (SyntaxElementKind::Trivia, "spaces", None)
            }
            b'\t' => {
                offset += 1;
                diagnostics.push(Diagnostic::new(
                    "syntax.tab_outside_literal",
                    SourceRange::new(path, start, offset),
                    RecoveryAction::PreservedInvalidBytes,
                ));
                (SyntaxElementKind::Invalid, "invalid_tab", None)
            }
            b'\n' => {
                offset += 1;
                (SyntaxElementKind::Trivia, "lf", None)
            }
            b'\r' if bytes.get(offset + 1) == Some(&b'\n') => {
                offset += 2;
                (SyntaxElementKind::Trivia, "crlf", None)
            }
            b'\r' => {
                offset += 1;
                diagnostics.push(Diagnostic::new(
                    "syntax.bare_carriage_return",
                    SourceRange::new(path, start, offset),
                    RecoveryAction::PreservedInvalidBytes,
                ));
                (SyntaxElementKind::Invalid, "invalid_line_ending", None)
            }
            b'#' => {
                offset += 1;
                while offset < bytes.len() && !matches!(bytes[offset], b'\r' | b'\n') {
                    if offset.is_multiple_of(256) && cancellation.is_cancelled() {
                        break;
                    }
                    offset += 1;
                }
                if bytes.get(start + 1) == Some(&b'#') {
                    (SyntaxElementKind::Trivia, "documentation_comment", None)
                } else {
                    (SyntaxElementKind::Trivia, "comment", None)
                }
            }
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => {
                offset += 1;
                while bytes
                    .get(offset)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                {
                    if offset.is_multiple_of(256) && cancellation.is_cancelled() {
                        break;
                    }
                    offset += 1;
                }
                let token = classify_token_bytes(&bytes[start..offset]);
                (SyntaxElementKind::Token, token.name(), Some(token))
            }
            b'0'..=b'9' => {
                offset = numeric_token_end(bytes, start, cancellation);
                let token = classify_token_bytes(&bytes[start..offset]);
                (SyntaxElementKind::Token, token.name(), Some(token))
            }
            quote @ (b'\'' | b'"') => {
                offset += 1;
                let mut escaped = false;
                let mut closed = false;
                while offset < bytes.len() {
                    if offset.is_multiple_of(256) && cancellation.is_cancelled() {
                        break;
                    }
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
                    (
                        SyntaxElementKind::Token,
                        "text_literal",
                        Some(TokenKind::TextLiteral),
                    )
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
                    (SyntaxElementKind::Invalid, "invalid_literal", None)
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
                let token = classify_token_bytes(&bytes[start..offset]);
                if token == TokenKind::Invalid {
                    diagnostics.push(Diagnostic::new(
                        "syntax.invalid_token",
                        SourceRange::new(path, start, offset),
                        RecoveryAction::PreservedInvalidBytes,
                    ));
                    (SyntaxElementKind::Invalid, "invalid_token", None)
                } else {
                    (SyntaxElementKind::Token, token.name(), Some(token))
                }
            }
            byte => {
                let (extent, code) = if byte.is_ascii() {
                    (1, "syntax.invalid_character")
                } else {
                    utf8_scalar_extent(bytes, offset)
                };
                offset += extent;
                diagnostics.push(Diagnostic::new(
                    code,
                    SourceRange::new(path, start, offset),
                    RecoveryAction::PreservedInvalidBytes,
                ));
                (SyntaxElementKind::Invalid, "invalid_byte", None)
            }
        };
        elements.push(SyntaxElement::new(kind, name, path, start, offset));
        if let Some(kind) = token_kind {
            lexemes.push(Lexeme {
                kind,
                range: SourceRange::new(path, start, offset),
            });
        }
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

    if scan_layout_and_delimiters(file, &mut elements, &mut diagnostics, cancellation) {
        return cancelled_source(file, elements, diagnostics, imports, declarations);
    }
    let Some(parsed_imports) = parse_imports(file, &lexemes, &mut diagnostics, cancellation) else {
        return cancelled_source(file, elements, diagnostics, imports, declarations);
    };
    imports = parsed_imports;
    let Some(parsed_declarations) = parse_declarations(file, &lexemes, cancellation) else {
        return cancelled_source(file, elements, diagnostics, imports, declarations);
    };
    declarations = parsed_declarations;
    if assign_attributes(file, &lexemes, &mut declarations, cancellation) {
        return cancelled_source(file, elements, diagnostics, imports, declarations);
    }
    for index in 0..declarations.len() {
        if cancellation.is_cancelled() {
            return cancelled_source(file, elements, diagnostics, imports, declarations);
        }
        let syntax = parse_declaration_syntax(
            file,
            &declarations[index],
            &lexemes,
            &mut diagnostics,
            cancellation,
        );
        declarations[index].syntax = syntax;
    }
    if cancellation.is_cancelled() {
        return cancelled_source(file, elements, diagnostics, imports, declarations);
    }
    let comptime_assertions =
        parse_comptime_assertions(file, &lexemes, &mut diagnostics, cancellation);
    if cancellation.is_cancelled() {
        return cancelled_source(file, elements, diagnostics, imports, declarations);
    }
    if validate_top_level(file, &lexemes, &mut diagnostics, cancellation) {
        return cancelled_source(file, elements, diagnostics, imports, declarations);
    }
    for declaration in &mut declarations {
        declaration.structurally_valid = !diagnostics.iter().any(|diagnostic| {
            let start = diagnostic.primary().start();
            let direct = start >= declaration.start
                && (start < declaration.end
                    || (start == declaration.end
                        && declaration.end == u64::try_from(bytes.len()).unwrap_or(u64::MAX)));
            let unmatched_opener = diagnostic.code() == "syntax.missing_closer"
                && diagnostic.parameters().iter().any(|(name, value)| {
                    name.as_ref() == "opener_offset"
                        && value.parse::<u64>().is_ok_and(|offset| {
                            offset >= declaration.start && offset < declaration.end
                        })
                });
            direct || unmatched_opener
        });
    }
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

    let Some(tree) = build_green_tree(file, &declarations, &elements, cancellation) else {
        return cancelled_source(file, elements, diagnostics, imports, declarations);
    };
    debug_assert_eq!(
        tree.authored_bytes(),
        u64::try_from(bytes.len()).unwrap_or(u64::MAX)
    );
    ParsedSource {
        elements,
        diagnostics,
        imports,
        declarations,
        comptime_assertions,
        tree,
        cancelled: false,
    }
}

fn cancelled_source(
    file: &ProjectFile,
    elements: Vec<SyntaxElement>,
    diagnostics: Vec<Diagnostic>,
    imports: Vec<Import>,
    declarations: Vec<Declaration>,
) -> ParsedSource {
    ParsedSource {
        tree: cancelled_green(file),
        elements,
        diagnostics,
        imports,
        declarations,
        comptime_assertions: Vec::new(),
        cancelled: true,
    }
}

fn cancelled_green(file: &ProjectFile) -> GreenNode {
    GreenNode {
        kind: GreenKind::Source,
        range: SourceRange::new(file.path(), 0, file.bytes().len()),
        children: std::sync::Arc::from([]),
    }
}

fn numeric_token_end(bytes: &[u8], start: usize, cancellation: &Cancellation) -> usize {
    let mut offset = start + 1;
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
    if bytes.get(offset) == Some(&b'.') && bytes.get(offset + 1).is_some_and(u8::is_ascii_digit) {
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

fn utf8_scalar_extent(bytes: &[u8], offset: usize) -> (usize, &'static str) {
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

fn build_green_tree(
    file: &ProjectFile,
    declarations: &[Declaration],
    elements: &[SyntaxElement],
    cancellation: &Cancellation,
) -> Option<GreenNode> {
    let mut events = vec![Event::Start(
        GreenKind::Source,
        SourceRange::new(file.path(), 0, file.bytes().len()),
    )];
    let mut element_index = 0;
    for declaration in declarations {
        if cancellation.is_cancelled()
            || emit_elements_before(
                declaration.start,
                elements,
                &mut element_index,
                &mut events,
                cancellation,
            )
            || emit_region(
                &declaration_region(file, declaration),
                elements,
                &mut element_index,
                &mut events,
                cancellation,
            )
        {
            return None;
        }
    }
    while element_index < elements.len() {
        if element_index.is_multiple_of(256) && cancellation.is_cancelled() {
            return None;
        }
        emit_element(elements, element_index, &mut events);
        element_index += 1;
    }
    events.push(Event::Finish);

    let mut stack: Vec<(GreenKind, SourceRange, Vec<GreenChild>)> = Vec::new();
    let mut root = None;
    for (event_index, event) in events.into_iter().enumerate() {
        if event_index.is_multiple_of(256) && cancellation.is_cancelled() {
            return None;
        }
        match event {
            Event::Start(kind, range) => stack.push((kind, range, Vec::new())),
            Event::Token(index) | Event::Missing(index) | Event::Error(index) => stack
                .last_mut()
                .expect("event parser has an open node")
                .2
                .push(GreenChild::Leaf(elements[index].clone())),
            Event::Finish => {
                let (kind, range, children) = stack.pop().expect("balanced parser events");
                let node = GreenNode {
                    kind,
                    range,
                    children: children.into(),
                };
                if let Some(parent) = stack.last_mut() {
                    parent.2.push(GreenChild::Node(node));
                } else {
                    root = Some(node);
                }
            }
        }
    }
    Some(root.expect("source event produces a root"))
}

fn declaration_region(file: &ProjectFile, declaration: &Declaration) -> SyntaxRegion {
    let mut children = Vec::new();
    if let Some(syntax) = &declaration.syntax {
        match syntax {
            DeclarationSyntax::Function(function) => {
                children.push(SyntaxRegion {
                    kind: "function_signature",
                    range: declaration.range.clone(),
                    children: function
                        .parameters
                        .iter()
                        .map(|parameter| SyntaxRegion {
                            kind: "parameter",
                            range: parameter.range.clone(),
                            children: Vec::new(),
                        })
                        .collect(),
                });
                let mut statements = Vec::new();
                collect_statement_regions(&function.body, &mut statements);
                children.push(SyntaxRegion {
                    kind: "block",
                    range: SourceRange::from_u64(
                        file.path(),
                        declaration.range.end(),
                        declaration.end,
                    ),
                    children: statements,
                });
            }
            DeclarationSyntax::Constant(constant) => children.push(SyntaxRegion {
                kind: "constant_value",
                range: declaration.range.clone(),
                children: vec![expression_region(&constant.value)],
            }),
            DeclarationSyntax::Suite(suite) => {
                children.push(SyntaxRegion {
                    kind: "suite_header",
                    range: declaration.range.clone(),
                    children: Vec::new(),
                });
                children.extend(suite.tests.iter().map(|test| {
                    SyntaxRegion {
                        kind: if test.asynchronous {
                            "async_test"
                        } else {
                            "test"
                        },
                        range: test.range.clone(),
                        children: test
                            .parameters
                            .iter()
                            .map(|parameter| SyntaxRegion {
                                kind: "parameter",
                                range: parameter.range.clone(),
                                children: Vec::new(),
                            })
                            .collect(),
                    }
                }));
            }
            DeclarationSyntax::Enum(enum_) => {
                children.extend(enum_.variants.iter().map(|variant| SyntaxRegion {
                    kind: "variant",
                    range: variant.range.clone(),
                    children: Vec::new(),
                }))
            }
            DeclarationSyntax::Named => {}
        }
    }
    children.sort_by(|left, right| {
        left.range
            .start()
            .cmp(&right.range.start())
            .then(left.range.end().cmp(&right.range.end()))
    });
    SyntaxRegion {
        kind: declaration.kind.name(),
        range: SourceRange::from_u64(file.path(), declaration.start, declaration.end),
        children,
    }
}

fn collect_statement_regions(statements: &[StatementSyntax], output: &mut Vec<SyntaxRegion>) {
    for statement in statements {
        let (kind, range, expressions) = match statement {
            StatementSyntax::Return { value, range } => (
                "return_statement",
                range,
                value.iter().map(expression_region).collect(),
            ),
            StatementSyntax::Panic { value, range } => {
                ("panic_statement", range, vec![expression_region(value)])
            }
            StatementSyntax::Assign { value, range, .. } => (
                "initialize_statement",
                range,
                vec![expression_region(value)],
            ),
            StatementSyntax::Evaluate(value) => (
                "expression_statement",
                &value.range,
                vec![expression_region(value)],
            ),
            StatementSyntax::If {
                condition, range, ..
            } => ("if_statement", range, vec![expression_region(condition)]),
            StatementSyntax::Pass(range) => ("pass_statement", range, Vec::new()),
        };
        output.push(SyntaxRegion {
            kind,
            range: range.clone(),
            children: expressions,
        });
        if let StatementSyntax::If {
            then_branch,
            else_branch,
            ..
        } = statement
        {
            collect_statement_regions(then_branch, output);
            collect_statement_regions(else_branch, output);
        }
    }
    output.sort_by_key(|region| region.range.start());
}

fn expression_region(expression: &ExpressionSyntax) -> SyntaxRegion {
    let (kind, children) = match &expression.kind {
        ExpressionSyntaxKind::Integer(_) => ("integer_expression", Vec::new()),
        ExpressionSyntaxKind::Float(_) => ("float_expression", Vec::new()),
        ExpressionSyntaxKind::Text(_) => ("text_expression", Vec::new()),
        ExpressionSyntaxKind::Bool(_) => ("bool_expression", Vec::new()),
        ExpressionSyntaxKind::Name(_) => ("name_expression", Vec::new()),
        ExpressionSyntaxKind::Call { arguments, .. } => (
            "call_expression",
            arguments
                .iter()
                .map(|argument| expression_region(&argument.value))
                .collect(),
        ),
        ExpressionSyntaxKind::Array(values) => (
            "array_expression",
            values.iter().map(expression_region).collect(),
        ),
        ExpressionSyntaxKind::Unit => ("unit_expression", Vec::new()),
        ExpressionSyntaxKind::Negate(value) => {
            ("negate_expression", vec![expression_region(value)])
        }
        ExpressionSyntaxKind::Await(value) => ("await_expression", vec![expression_region(value)]),
        ExpressionSyntaxKind::Propagate(value) => {
            ("propagate_expression", vec![expression_region(value)])
        }
        ExpressionSyntaxKind::Binary { left, right, .. } => (
            "binary_expression",
            vec![expression_region(left), expression_region(right)],
        ),
    };
    SyntaxRegion {
        kind,
        range: expression.range.clone(),
        children,
    }
}

fn emit_region(
    region: &SyntaxRegion,
    elements: &[SyntaxElement],
    element_index: &mut usize,
    events: &mut Vec<Event>,
    cancellation: &Cancellation,
) -> bool {
    if cancellation.is_cancelled() {
        return true;
    }
    events.push(Event::Start(
        GreenKind::Syntax(region.kind),
        region.range.clone(),
    ));
    for child in &region.children {
        if emit_elements_before(
            child.range.start(),
            elements,
            element_index,
            events,
            cancellation,
        ) || emit_region(child, elements, element_index, events, cancellation)
        {
            return true;
        }
    }
    if emit_elements_before(
        region.range.end(),
        elements,
        element_index,
        events,
        cancellation,
    ) {
        return true;
    }
    events.push(Event::Finish);
    false
}

fn emit_elements_before(
    end: u64,
    elements: &[SyntaxElement],
    element_index: &mut usize,
    events: &mut Vec<Event>,
    cancellation: &Cancellation,
) -> bool {
    while elements
        .get(*element_index)
        .is_some_and(|element| element.range().start() < end)
    {
        if element_index.is_multiple_of(256) && cancellation.is_cancelled() {
            return true;
        }
        emit_element(elements, *element_index, events);
        *element_index += 1;
    }
    false
}

fn emit_element(elements: &[SyntaxElement], index: usize, events: &mut Vec<Event>) {
    events.push(match elements[index].kind() {
        SyntaxElementKind::Missing => Event::Missing(index),
        SyntaxElementKind::Error | SyntaxElementKind::Invalid => Event::Error(index),
        SyntaxElementKind::Token | SyntaxElementKind::Trivia | SyntaxElementKind::Layout => {
            Event::Token(index)
        }
    });
}

fn validate_top_level(
    file: &ProjectFile,
    lexemes: &[Lexeme],
    diagnostics: &mut Vec<Diagnostic>,
    cancellation: &Cancellation,
) -> bool {
    let mut offset = 0;
    let mut token_index = 0;
    while offset < file.bytes().len() {
        if cancellation.is_cancelled() {
            return true;
        }
        let Some(physical_end) =
            physical_line_end(file.bytes(), offset, file.bytes().len(), cancellation)
        else {
            return true;
        };
        let content_end = physical_content_end(file.bytes(), offset, physical_end);
        let line = &file.bytes()[offset..content_end];
        let end = offset + line.len();
        while lexemes.get(token_index).is_some_and(|lexeme| {
            usize::try_from(lexeme.range.start()).is_ok_and(|start| start < offset)
        }) {
            token_index += 1;
        }
        let first = lexemes
            .get(token_index)
            .filter(|lexeme| usize::try_from(lexeme.range.start()).is_ok_and(|start| start < end));
        let accepted = first.is_none()
            || line.first().is_some_and(u8::is_ascii_whitespace)
            || first.is_some_and(|lexeme| {
                matches!(
                    lexeme.kind,
                    TokenKind::At
                        | TokenKind::From
                        | TokenKind::Comptime
                        | TokenKind::Pub
                        | TokenKind::Pure
                        | TokenKind::Async
                        | TokenKind::Fn
                        | TokenKind::Const
                        | TokenKind::Pool
                        | TokenKind::Type
                        | TokenKind::Struct
                        | TokenKind::Resource
                        | TokenKind::Enum
                        | TokenKind::Interface
                        | TokenKind::Suite
                )
            });
        if !accepted {
            diagnostics.push(Diagnostic::new(
                "syntax.unexpected_top_level",
                SourceRange::new(file.path(), offset, offset + line.len()),
                RecoveryAction::SkippedToBoundary,
            ));
        }
        offset = physical_end;
    }
    false
}

fn parse_declarations(
    file: &ProjectFile,
    lexemes: &[Lexeme],
    cancellation: &Cancellation,
) -> Option<Vec<Declaration>> {
    let mut starts = Vec::new();
    let mut index = 0;
    while index < lexemes.len() {
        if index.is_multiple_of(256) && cancellation.is_cancelled() {
            return None;
        }
        let lexeme = &lexemes[index];
        if !at_top_level(file.bytes(), lexeme.range.start()) {
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
            starts.push((
                usize::try_from(lexeme.range.start()).expect("admitted source offset"),
                usize::try_from(line_end).expect("admitted source offset"),
                kind,
                name.to_owned(),
                public,
            ));
        }
        index += 1;
    }
    let source_len = file.bytes().len();
    Some(
        starts
            .iter()
            .enumerate()
            .map(|(index, (start, header_end, kind, name, public))| {
                let end = starts.get(index + 1).map_or(source_len, |(next, ..)| *next);
                Declaration {
                    kind: *kind,
                    name: name.clone(),
                    public: *public,
                    attributes: Vec::new(),
                    syntax: None,
                    range: SourceRange::new(file.path(), *start, *header_end),
                    start: u64::try_from(*start).expect("admitted source offset fits u64"),
                    end: u64::try_from(end).expect("admitted source offset fits u64"),
                    structurally_valid: true,
                }
            })
            .collect(),
    )
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
                range: SourceRange::new(file.path(), offset, content_end),
            });
        }
        offset = physical_end;
    }
    Some(lines)
}

fn physical_line_end(
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

fn physical_content_end(bytes: &[u8], offset: usize, physical_end: usize) -> usize {
    let mut end = physical_end;
    if end > offset && bytes[end - 1] == b'\n' {
        end -= 1;
    }
    if end > offset && bytes[end - 1] == b'\r' {
        end -= 1;
    }
    end
}

fn leading_spaces(bytes: &[u8], cancellation: &Cancellation) -> Option<usize> {
    let mut count = 0;
    while bytes.get(count) == Some(&b' ') {
        if count.is_multiple_of(256) && cancellation.is_cancelled() {
            return None;
        }
        count += 1;
    }
    Some(count)
}

fn assign_attributes(
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
            .is_some_and(|declaration| declaration.start < first.range.start())
        {
            declaration_index += 1;
        }
        if let Some(declaration) = declarations.get_mut(declaration_index)
            && declaration.start == first.range.start()
        {
            declaration.attributes = std::mem::take(&mut pending);
            declaration_index += 1;
        } else {
            pending.clear();
        }
    }
    false
}

fn parse_declaration_syntax(
    file: &ProjectFile,
    declaration: &Declaration,
    lexemes: &[Lexeme],
    diagnostics: &mut Vec<Diagnostic>,
    cancellation: &Cancellation,
) -> Option<DeclarationSyntax> {
    let lines = token_lines(
        file,
        lexemes,
        declaration.start,
        declaration.end,
        cancellation,
    )?;
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
        DeclarationKind::Suite => {
            parse_suite_syntax(file, &mut cursor, &lines, diagnostics, cancellation)
        }
        DeclarationKind::Enum => {
            parse_enum_syntax(file, &mut cursor, &lines, diagnostics, cancellation)
        }
        DeclarationKind::ResourceStruct => {
            cursor.expect(TokenKind::Resource)?;
            cursor.expect(TokenKind::Struct)?;
            cursor.expect_identifier()?;
            Some(DeclarationSyntax::Named)
        }
        kind => {
            cursor.expect(match kind {
                DeclarationKind::Pool => TokenKind::Pool,
                DeclarationKind::TypeAlias => TokenKind::Type,
                DeclarationKind::Struct => TokenKind::Struct,
                DeclarationKind::Interface => TokenKind::Interface,
                _ => return None,
            })?;
            cursor.expect_identifier()?;
            Some(DeclarationSyntax::Named)
        }
    };
    if parsed.is_none() && diagnostics.is_empty() {
        diagnostics.push(Diagnostic::new(
            "syntax.malformed_declaration",
            declaration.range.clone(),
            RecoveryAction::SkippedToBoundary,
        ));
    }
    parsed
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
    let mut type_parameters = Vec::new();
    if cursor.consume(TokenKind::LeftBracket) {
        while !cursor.consume(TokenKind::RightBracket) {
            type_parameters.push(cursor.expect_identifier()?.to_owned());
            if !cursor.consume(TokenKind::Comma) {
                cursor.expect(TokenKind::RightBracket)?;
                break;
            }
        }
    }
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
    let body = parse_statement_block(file, lines, &mut line_index, 4, cancellation)?;
    Some(DeclarationSyntax::Function(FunctionSyntax {
        modifier,
        type_parameters,
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
    for line in lines.iter().skip(1).filter(|line| line.indent == 4) {
        let mut test = SyntaxCursor::new(file, &line.tokens, cancellation);
        let asynchronous = test.consume(TokenKind::Async);
        if !test.consume(TokenKind::Test) {
            continue;
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
        tests.push(TestSyntax {
            name,
            asynchronous,
            parameters,
            range: line.range.clone(),
        });
    }
    Some(DeclarationSyntax::Suite(SuiteSyntax { tests }))
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
    cursor.expect(TokenKind::Colon)?;
    if !cursor.at_end() {
        return None;
    }
    let mut variants = Vec::new();
    let mut names = BTreeSet::new();
    for line in lines.iter().skip(1).filter(|line| line.indent == 4) {
        if cancellation.is_cancelled() {
            return None;
        }
        let mut variant = SyntaxCursor::new(file, &line.tokens, cancellation);
        let name = variant.expect_identifier()?.to_owned();
        if !variant.at_end() {
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
            range: line.range.clone(),
        });
    }
    Some(DeclarationSyntax::Enum(EnumSyntax { variants }))
}

fn parse_parameters(cursor: &mut SyntaxCursor<'_, '_>) -> Option<Vec<ParameterSyntax>> {
    cursor.expect(TokenKind::LeftParen)?;
    let mut parameters = Vec::new();
    while !cursor.consume(TokenKind::RightParen) {
        let start = cursor.current_range()?.clone();
        let ownership = if cursor.consume(TokenKind::Take) {
            OwnershipSyntax::Take
        } else {
            OwnershipSyntax::Value
        };
        let name = cursor.expect_identifier()?.to_owned();
        cursor.expect(TokenKind::Colon)?;
        let type_syntax = cursor.parse_type()?;
        let end = cursor.previous_range()?.end();
        parameters.push(ParameterSyntax {
            name,
            ownership,
            type_syntax,
            range: SourceRange::from_u64(start.path(), start.start(), end),
        });
        if !cursor.consume(TokenKind::Comma) {
            cursor.expect(TokenKind::RightParen)?;
            break;
        }
    }
    Some(parameters)
}

fn parse_statement_block(
    file: &ProjectFile,
    lines: &[TokenLine<'_>],
    index: &mut usize,
    indent: usize,
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
            .is_some_and(|token| token.kind == TokenKind::Else)
        {
            break;
        }
        *index += 1;
        let mut cursor = SyntaxCursor::from_line(file, line, cancellation);
        let statement = match cursor.peek_kind()? {
            TokenKind::If => {
                cursor.advance();
                let condition = cursor.parse_expression(0)?;
                cursor.expect(TokenKind::Colon)?;
                if !cursor.at_end() {
                    return None;
                }
                let then_branch =
                    parse_statement_block(file, lines, index, indent + 4, cancellation)?;
                let mut else_branch = Vec::new();
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
                    else_branch =
                        parse_statement_block(file, lines, index, indent + 4, cancellation)?;
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
            TokenKind::Pass => {
                cursor.advance();
                if !cursor.at_end() {
                    return None;
                }
                StatementSyntax::Pass(line.range.clone())
            }
            TokenKind::At => continue,
            TokenKind::Identifier if cursor.peek_n_kind(1) == Some(TokenKind::Equal) => {
                let name = cursor.expect_identifier()?.to_owned();
                cursor.expect(TokenKind::Equal)?;
                StatementSyntax::Assign {
                    name,
                    value: cursor.parse_complete_expression()?,
                    range: line.range.clone(),
                }
            }
            _ => StatementSyntax::Evaluate(cursor.parse_complete_expression()?),
        };
        statements.push(statement);
    }
    Some(statements)
}

fn parse_comptime_assertions(
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

    fn parse_name(&mut self) -> Option<NameSyntax> {
        let mut segments = vec![self.expect_identifier()?.to_owned()];
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
        if self.consume(TokenKind::LeftParen) {
            if self.consume(TokenKind::RightParen) {
                return Some(TypeSyntax::Unit);
            }
            let mut members = vec![self.parse_type_at(depth + 1)?];
            while self.consume(TokenKind::Comma) {
                members.push(self.parse_type_at(depth + 1)?);
            }
            self.expect(TokenKind::RightParen)?;
            return Some(TypeSyntax::Tuple(members));
        }
        if self.consume(TokenKind::LeftBracket) {
            let element = self.parse_type_at(depth + 1)?;
            self.expect(TokenKind::RightBracket)?;
            return Some(TypeSyntax::Array(Box::new(element)));
        }
        let base = self.parse_name()?;
        if self.consume(TokenKind::LeftBracket) {
            let mut arguments = Vec::new();
            while !self.consume(TokenKind::RightBracket) {
                if self.peek_kind() == Some(TokenKind::Identifier)
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
        let expression = self.parse_expression(0)?;
        self.at_end().then_some(expression)
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
            let range = SourceRange::from_u64(
                left.range.path(),
                left.range.start(),
                self.previous_range()?.end(),
            );
            left = ExpressionSyntax {
                kind: ExpressionSyntaxKind::Propagate(Box::new(left)),
                range,
            };
        }
        while let Some((operator, precedence)) = self.binary_operator() {
            if precedence < minimum_precedence {
                break;
            }
            self.advance();
            let right = self.parse_expression_at(precedence + 1, depth + 1)?;
            let range =
                SourceRange::from_u64(left.range.path(), left.range.start(), right.range.end());
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
        let path = token.range.path();
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
                    kind: ExpressionSyntaxKind::Text(
                        text[1..text.len().saturating_sub(1)].to_owned(),
                    ),
                    range: token.range.clone(),
                }
            }
            TokenKind::True | TokenKind::False => ExpressionSyntax {
                kind: ExpressionSyntaxKind::Bool(token.kind == TokenKind::True),
                range: token.range.clone(),
            },
            TokenKind::Identifier => {
                self.index -= 1;
                let name = self.parse_name()?;
                let end = self.previous_range()?.end();
                if self.consume(TokenKind::LeftParen) {
                    let mut arguments = Vec::new();
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
                        range: SourceRange::from_u64(path, start, self.previous_range()?.end()),
                    }
                } else {
                    ExpressionSyntax {
                        kind: ExpressionSyntaxKind::Name(name),
                        range: SourceRange::from_u64(path, start, end),
                    }
                }
            }
            TokenKind::LeftParen => {
                if self.consume(TokenKind::RightParen) {
                    ExpressionSyntax {
                        kind: ExpressionSyntaxKind::Unit,
                        range: SourceRange::from_u64(path, start, self.previous_range()?.end()),
                    }
                } else {
                    let mut inner = self.parse_expression_at(0, depth + 1)?;
                    self.expect(TokenKind::RightParen)?;
                    inner.range = SourceRange::from_u64(path, start, self.previous_range()?.end());
                    inner
                }
            }
            TokenKind::LeftBracket => {
                let mut values = Vec::new();
                while !self.consume(TokenKind::RightBracket) {
                    values.push(self.parse_expression_at(0, depth + 1)?);
                    if !self.consume(TokenKind::Comma) {
                        self.expect(TokenKind::RightBracket)?;
                        break;
                    }
                }
                ExpressionSyntax {
                    kind: ExpressionSyntaxKind::Array(values),
                    range: SourceRange::from_u64(path, start, self.previous_range()?.end()),
                }
            }
            TokenKind::Minus => {
                let value = self.parse_expression_at(12, depth + 1)?;
                let end = value.range.end();
                ExpressionSyntax {
                    kind: ExpressionSyntaxKind::Negate(Box::new(value)),
                    range: SourceRange::from_u64(path, start, end),
                }
            }
            TokenKind::Await => {
                let value = self.parse_expression_at(12, depth + 1)?;
                let end = value.range.end();
                ExpressionSyntax {
                    kind: ExpressionSyntaxKind::Await(Box::new(value)),
                    range: SourceRange::from_u64(path, start, end),
                }
            }
            _ => return None,
        };
        if self.consume(TokenKind::Question) {
            expression = ExpressionSyntax {
                range: SourceRange::from_u64(path, start, self.previous_range()?.end()),
                kind: ExpressionSyntaxKind::Propagate(Box::new(expression)),
            };
        }
        Some(expression)
    }

    fn binary_operator(&self) -> Option<(BinaryOperatorSyntax, u8)> {
        Some(match self.peek_kind()? {
            TokenKind::EqualEqual => (BinaryOperatorSyntax::Equal, 5),
            TokenKind::BangEqual => (BinaryOperatorSyntax::NotEqual, 5),
            TokenKind::Less => (BinaryOperatorSyntax::Less, 5),
            TokenKind::LessEqual => (BinaryOperatorSyntax::LessEqual, 5),
            TokenKind::Greater => (BinaryOperatorSyntax::Greater, 5),
            TokenKind::GreaterEqual => (BinaryOperatorSyntax::GreaterEqual, 5),
            TokenKind::Plus => (BinaryOperatorSyntax::Add, 10),
            TokenKind::Minus => (BinaryOperatorSyntax::Subtract, 10),
            TokenKind::Star => (BinaryOperatorSyntax::Multiply, 11),
            TokenKind::Slash => (BinaryOperatorSyntax::Divide, 11),
            TokenKind::Percent => (BinaryOperatorSyntax::Remainder, 11),
            _ => return None,
        })
    }
}

fn at_top_level(bytes: &[u8], offset: u64) -> bool {
    let Ok(offset) = usize::try_from(offset) else {
        return false;
    };
    offset == 0 || bytes.get(offset - 1) == Some(&b'\n')
}

fn line_end(bytes: &[u8], offset: u64, cancellation: &Cancellation) -> Option<u64> {
    let offset = usize::try_from(offset).unwrap_or(bytes.len());
    let mut cursor = offset;
    while cursor < bytes.len() && !matches!(bytes[cursor], b'\r' | b'\n') {
        if (cursor - offset).is_multiple_of(256) && cancellation.is_cancelled() {
            return None;
        }
        cursor += 1;
    }
    Some(u64::try_from(cursor).unwrap_or(u64::MAX))
}

fn scan_layout_and_delimiters(
    file: &ProjectFile,
    elements: &mut Vec<SyntaxElement>,
    diagnostics: &mut Vec<Diagnostic>,
    cancellation: &Cancellation,
) -> bool {
    let bytes = file.bytes();
    let path = file.path();
    let mut indent_stack = vec![0_usize];
    let mut delimiter_stack: Vec<(u8, usize)> = Vec::new();
    let mut expected_block = false;
    let mut offset = 0;

    while offset < bytes.len() {
        if cancellation.is_cancelled() {
            return true;
        }
        let Some(physical_end) = physical_line_end(bytes, offset, bytes.len(), cancellation) else {
            return true;
        };
        let content_end = physical_content_end(bytes, offset, physical_end);
        let content = &bytes[offset..content_end];
        let Some(leading) = leading_spaces(content, cancellation) else {
            return true;
        };
        let mut significant_end = content.len();
        for (index, byte) in content[leading..].iter().enumerate() {
            if index.is_multiple_of(256) && cancellation.is_cancelled() {
                return true;
            }
            if *byte == b'#' {
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
    false
}

fn scan_line_delimiters(
    line: &[u8],
    base: usize,
    path: &str,
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
                        SourceRange::new(path, base + index, base + index + 1),
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
                        SourceRange::new(path, base + index, base + index + 1),
                        RecoveryAction::SkippedToBoundary,
                    ));
                }
            }
            _ => {}
        }
    }
    false
}

fn parse_imports(
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
                    SourceRange::new(file.path(), offset, end),
                    RecoveryAction::SkippedToBoundary,
                ));
            }
            match parse_import_tokens(file, &line_tokens) {
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
        (segment_token.kind == TokenKind::Identifier).then_some(())?;
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
    (leaf_token.kind == TokenKind::Identifier).then_some(())?;
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

fn token_text<'a>(file: &'a ProjectFile, token: &Lexeme) -> Option<&'a str> {
    std::str::from_utf8(checked_slice(
        file.bytes(),
        token.range.start(),
        token.range.end(),
    )?)
    .ok()
}

fn valid_path_segment(segment: &str) -> bool {
    let mut bytes = segment.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

pub(crate) fn checked_slice(bytes: &[u8], start: u64, end: u64) -> Option<&[u8]> {
    if start > end {
        return None;
    }
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    bytes.get(start..end)
}
