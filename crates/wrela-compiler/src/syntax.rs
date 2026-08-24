#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::{
    Cancellation, Diagnostic, DiagnosticValue, ProjectFile, RecoveryAction, SourceRange,
    SyntaxElement, SyntaxElementKind, SyntaxErrorKind, SyntaxInvalidKind, SyntaxLayoutKind,
    SyntaxMissingKind, SyntaxNodeKind, SyntaxNodeObservation, SyntaxTokenKind, SyntaxTriviaKind,
};

const MAX_SOURCE_BYTES: usize = 1_048_576;
const MAX_NESTING: usize = 256;

#[derive(Clone)]
pub(crate) struct ParsedSource {
    pub(crate) elements: Vec<SyntaxElement>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) imports: Vec<Import>,
    pub(crate) declarations: Vec<Declaration>,
    pub(crate) comptime_assertions: Vec<ExpressionSyntax>,
    pub(crate) comptime_selections: Vec<ComptimeSelection>,
    tree: GreenNode,
    pub(crate) cancelled: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ComptimeSelection {
    pub(crate) branches: Vec<ComptimeBranch>,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct ComptimeBranch {
    pub(crate) condition: Option<ExpressionSyntax>,
    pub(crate) declarations: Vec<Declaration>,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
struct GreenNode {
    kind: SyntaxNodeKind,
    range: SourceRange,
    children: std::sync::Arc<[GreenChild]>,
}

#[derive(Clone, Debug)]
enum GreenChild {
    Node(GreenNode),
    Leaf(SyntaxElement),
}

enum Event {
    Start(SyntaxNodeKind, SourceRange),
    Token(usize),
    Missing(usize),
    Error(usize),
    Finish,
}

struct SyntaxRegion {
    kind: SyntaxNodeKind,
    range: SourceRange,
    children: Vec<SyntaxRegion>,
}

pub(crate) type TokenKind = SyntaxTokenKind;

#[derive(Clone, Debug)]
struct Lexeme {
    kind: TokenKind,
    range: SourceRange,
}

fn classify_token_bytes(bytes: &[u8]) -> TokenKind {
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

fn punctuation_width(bytes: &[u8]) -> usize {
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

#[derive(Clone, Debug)]
pub(crate) struct Declaration {
    pub(crate) kind: DeclarationKind,
    pub(crate) name: String,
    pub(crate) public: bool,
    pub(crate) attributes: Vec<AttributeSyntax>,
    pub(crate) syntax: Option<DeclarationSyntax>,
    pub(crate) range: SourceRange,
    pub(crate) start: u64,
    pub(crate) header_start: u64,
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

    const fn node_kind(self) -> SyntaxNodeKind {
        match self {
            Self::Function => SyntaxNodeKind::Function,
            Self::Constant => SyntaxNodeKind::Constant,
            Self::Pool => SyntaxNodeKind::Pool,
            Self::TypeAlias => SyntaxNodeKind::TypeAlias,
            Self::Struct => SyntaxNodeKind::Struct,
            Self::ResourceStruct => SyntaxNodeKind::ResourceStruct,
            Self::Enum => SyntaxNodeKind::Enum,
            Self::Interface => SyntaxNodeKind::Interface,
            Self::Suite => SyntaxNodeKind::Suite,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FunctionModifier {
    Ordinary,
    Pure,
    Async,
}

impl FunctionModifier {
    pub(crate) const fn canonical_tag(self) -> u8 {
        match self {
            Self::Ordinary => 0x01,
            Self::Pure => 0x02,
            Self::Async => 0x03,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OwnershipSyntax {
    Value,
    Read,
    Mut,
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
    FixedArray {
        element: Box<TypeSyntax>,
        length: u64,
    },
    Function {
        parameters: Vec<TypeSyntax>,
        return_type: Box<TypeSyntax>,
    },
    Own {
        pool: NameSyntax,
        value: Box<TypeSyntax>,
    },
    Any(NameSyntax),
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
    TypeAlias(TypeSyntax),
    Suite(SuiteSyntax),
    Enum(EnumSyntax),
    Struct(StructSyntax),
    ResourceStruct(StructSyntax),
    Interface(InterfaceSyntax),
    Pool,
}

#[derive(Clone, Debug)]
pub(crate) struct StructSyntax {
    pub(crate) type_parameters: Vec<String>,
    pub(crate) implements: Vec<NameSyntax>,
    pub(crate) fields: Vec<FieldSyntax>,
    pub(crate) functions: Vec<MemberFunctionSyntax>,
    pub(crate) comptime_selections: Vec<ComptimeMemberSelection>,
}

#[derive(Clone, Debug)]
pub(crate) struct MemberFunctionSyntax {
    pub(crate) name: String,
    pub(crate) public: bool,
    pub(crate) function: FunctionSyntax,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct FieldSyntax {
    pub(crate) name: String,
    pub(crate) public: bool,
    pub(crate) mutable: bool,
    pub(crate) type_syntax: TypeSyntax,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct InterfaceSyntax {
    pub(crate) requirements: Vec<FunctionRequirementSyntax>,
}

#[derive(Clone, Debug)]
pub(crate) struct FunctionRequirementSyntax {
    pub(crate) name: String,
    pub(crate) modifier: FunctionModifier,
    pub(crate) parameters: Vec<ParameterSyntax>,
    pub(crate) return_type: TypeSyntax,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct EnumSyntax {
    pub(crate) type_parameters: Vec<String>,
    pub(crate) variants: Vec<VariantSyntax>,
    pub(crate) functions: Vec<MemberFunctionSyntax>,
    pub(crate) comptime_selections: Vec<ComptimeMemberSelection>,
}

#[derive(Clone, Debug)]
pub(crate) struct ComptimeMemberSelection {
    pub(crate) branches: Vec<ComptimeMemberBranch>,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct ComptimeMemberBranch {
    pub(crate) condition: Option<ExpressionSyntax>,
    pub(crate) fields: Vec<FieldSyntax>,
    pub(crate) variants: Vec<VariantSyntax>,
    pub(crate) functions: Vec<MemberFunctionSyntax>,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct VariantSyntax {
    pub(crate) name: String,
    pub(crate) parameters: Vec<ParameterSyntax>,
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
    pub(crate) body: Vec<StatementSyntax>,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct ComptimeStatementBranch {
    pub(crate) condition: Option<ExpressionSyntax>,
    pub(crate) statements: Vec<StatementSyntax>,
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
    Assert {
        condition: ExpressionSyntax,
        range: SourceRange,
    },
    Expect {
        condition: ExpressionSyntax,
        range: SourceRange,
    },
    Assign {
        place: PlaceSyntax,
        mutable_binding: bool,
        declared_type: Option<TypeSyntax>,
        operator: Option<BinaryOperatorSyntax>,
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
    Comptime {
        branches: Vec<ComptimeStatementBranch>,
        range: SourceRange,
    },
    For {
        pattern: PatternSyntax,
        iterable: ExpressionSyntax,
        body: Vec<StatementSyntax>,
        range: SourceRange,
    },
    While {
        condition: ExpressionSyntax,
        body: Vec<StatementSyntax>,
        range: SourceRange,
    },
    Break(SourceRange),
    Continue(SourceRange),
    Match {
        value: ExpressionSyntax,
        cases: Vec<MatchCaseSyntax>,
        range: SourceRange,
    },
    Defer {
        expression: ExpressionSyntax,
        range: SourceRange,
    },
    With {
        scope: ExpressionSyntax,
        binding: Option<String>,
        body: Vec<StatementSyntax>,
        range: SourceRange,
    },
    Unsupported {
        kind: UnsupportedStatementKind,
        range: SourceRange,
    },
    Pass(SourceRange),
}

#[derive(Clone, Debug)]
pub(crate) struct PlaceSyntax {
    pub(crate) root: String,
    pub(crate) projections: Vec<PlaceProjectionSyntax>,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) enum PlaceProjectionSyntax {
    Field { name: String, range: SourceRange },
    Index(ExpressionSyntax),
}

#[derive(Clone, Debug)]
pub(crate) struct MatchCaseSyntax {
    pub(crate) pattern: PatternSyntax,
    pub(crate) guard: Option<ExpressionSyntax>,
    pub(crate) body: Vec<StatementSyntax>,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct PatternSyntax {
    pub(crate) kind: PatternSyntaxKind,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) enum PatternSyntaxKind {
    Wildcard,
    Literal(ExpressionSyntax),
    Binding(String),
    Take(Box<PatternSyntax>),
    Constructor {
        name: NameSyntax,
        arguments: Vec<PatternArgumentSyntax>,
    },
    Tuple(Vec<PatternSyntax>),
    FixedArray(Vec<PatternSyntax>),
    Or(Vec<PatternSyntax>),
}

#[derive(Clone, Debug)]
pub(crate) struct PatternArgumentSyntax {
    pub(crate) label: Option<String>,
    pub(crate) pattern: PatternSyntax,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum UnsupportedStatementKind {
    Take,
    Send,
    TrySend,
}

#[derive(Clone, Debug)]
pub(crate) struct ExpressionSyntax {
    pub(crate) kind: ExpressionSyntaxKind,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct ClosureParameterSyntax {
    pub(crate) name: String,
    pub(crate) type_: Option<TypeSyntax>,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) enum ExpressionSyntaxKind {
    Integer(String),
    Float(String),
    Text(String),
    Scalar(char),
    Bytes(Vec<u8>),
    Bool(bool),
    Name(NameSyntax),
    Call {
        callee: NameSyntax,
        arguments: Vec<ArgumentSyntax>,
    },
    Array(Vec<ExpressionSyntax>),
    RepeatedArray {
        value: Box<ExpressionSyntax>,
        length: u64,
    },
    Tuple(Vec<ExpressionSyntax>),
    Index {
        value: Box<ExpressionSyntax>,
        index: Box<ExpressionSyntax>,
    },
    Unit,
    Positive(Box<ExpressionSyntax>),
    Negate(Box<ExpressionSyntax>),
    BitNot(Box<ExpressionSyntax>),
    Not(Box<ExpressionSyntax>),
    Await(Box<ExpressionSyntax>),
    Mut(Box<ExpressionSyntax>),
    Take(Box<ExpressionSyntax>),
    Closure {
        parameters: Vec<ClosureParameterSyntax>,
        body: Box<ExpressionSyntax>,
    },
    Propagate(Box<ExpressionSyntax>),
    Is {
        value: Box<ExpressionSyntax>,
        pattern: Box<PatternSyntax>,
    },
    Binary {
        operator: BinaryOperatorSyntax,
        left: Box<ExpressionSyntax>,
        right: Box<ExpressionSyntax>,
    },
    Unsupported(UnsupportedExpressionKind),
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum UnsupportedExpressionKind {
    Send,
    TrySend,
}

#[derive(Clone, Debug)]
pub(crate) struct ArgumentSyntax {
    pub(crate) label: Option<String>,
    pub(crate) value: ExpressionSyntax,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinaryOperatorSyntax {
    Range,
    RangeInclusive,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
    And,
    Or,
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
        output.push(SyntaxNodeObservation::new(
            self.kind,
            self.range.clone(),
            depth,
        ));
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
                        SyntaxElementKind::Token(_)
                            | SyntaxElementKind::Trivia(_)
                            | SyntaxElementKind::Invalid(_)
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
    let path = file.path_arc();
    if bytes.len() > MAX_SOURCE_BYTES {
        let elements = vec![SyntaxElement::new(
            SyntaxElementKind::Invalid(SyntaxInvalidKind::OversizedSource),
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
                SourceRange::new_shared(path, MAX_SOURCE_BYTES, bytes.len()),
                RecoveryAction::PreservedInvalidBytes,
            )],
            imports: Vec::new(),
            declarations: Vec::new(),
            comptime_assertions: Vec::new(),
            comptime_selections: Vec::new(),
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
        let (kind, token_kind) = match bytes[offset] {
            b' ' => {
                offset += 1;
                while bytes.get(offset) == Some(&b' ') {
                    if offset.is_multiple_of(256) && cancellation.is_cancelled() {
                        break;
                    }
                    offset += 1;
                }
                (SyntaxElementKind::Trivia(SyntaxTriviaKind::Spaces), None)
            }
            b'\t' => {
                offset += 1;
                diagnostics.push(Diagnostic::new(
                    "syntax.tab_outside_literal",
                    SourceRange::new_shared(path, start, offset),
                    RecoveryAction::PreservedInvalidBytes,
                ));
                (SyntaxElementKind::Invalid(SyntaxInvalidKind::Tab), None)
            }
            b'\n' => {
                offset += 1;
                (SyntaxElementKind::Trivia(SyntaxTriviaKind::Lf), None)
            }
            b'\r' if bytes.get(offset + 1) == Some(&b'\n') => {
                offset += 2;
                (SyntaxElementKind::Trivia(SyntaxTriviaKind::Crlf), None)
            }
            b'\r' => {
                offset += 1;
                diagnostics.push(Diagnostic::new(
                    "syntax.bare_carriage_return",
                    SourceRange::new_shared(path, start, offset),
                    RecoveryAction::PreservedInvalidBytes,
                ));
                (
                    SyntaxElementKind::Invalid(SyntaxInvalidKind::LineEnding),
                    None,
                )
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
                    (
                        SyntaxElementKind::Trivia(SyntaxTriviaKind::DocumentationComment),
                        None,
                    )
                } else {
                    (SyntaxElementKind::Trivia(SyntaxTriviaKind::Comment), None)
                }
            }
            b'b' if bytes.get(offset + 1) == Some(&b'"') => {
                offset += 2;
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
                    } else if byte == b'"' {
                        closed = true;
                        break;
                    }
                }
                let literal = &bytes[start..offset];
                if closed && decode_bytes_literal(literal).is_some() {
                    (
                        SyntaxElementKind::Token(TokenKind::BytesLiteral),
                        Some(TokenKind::BytesLiteral),
                    )
                } else {
                    diagnostics.push(Diagnostic::new(
                        if closed {
                            "syntax.invalid_literal"
                        } else {
                            "syntax.unclosed_literal"
                        },
                        SourceRange::new_shared(path, start, offset),
                        RecoveryAction::PreservedInvalidBytes,
                    ));
                    (SyntaxElementKind::Invalid(SyntaxInvalidKind::Literal), None)
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
                (SyntaxElementKind::Token(token), Some(token))
            }
            b'0'..=b'9' => {
                offset = numeric_token_end(bytes, start, cancellation);
                let literal = &bytes[start..offset];
                if let Some(token) = classify_numeric_literal(literal) {
                    (SyntaxElementKind::Token(token), Some(token))
                } else {
                    diagnostics.push(Diagnostic::new(
                        "syntax.invalid_literal",
                        SourceRange::new_shared(path, start, offset),
                        RecoveryAction::PreservedInvalidBytes,
                    ));
                    (SyntaxElementKind::Invalid(SyntaxInvalidKind::Literal), None)
                }
            }
            b'.' if bytes.get(offset + 1).is_some_and(u8::is_ascii_digit) => {
                offset = numeric_token_end(bytes, start, cancellation);
                diagnostics.push(Diagnostic::new(
                    "syntax.invalid_literal",
                    SourceRange::new_shared(path, start, offset),
                    RecoveryAction::PreservedInvalidBytes,
                ));
                (SyntaxElementKind::Invalid(SyntaxInvalidKind::Literal), None)
            }
            b'"' if bytes.get(offset..offset + 3) == Some(b"\"\"\"") => {
                let (end, closed) = multiline_literal_end(bytes, start, cancellation);
                offset = end;
                let literal = &bytes[start..offset];
                if closed && decode_multiline_text_literal(literal).is_some() {
                    (
                        SyntaxElementKind::Token(TokenKind::TextLiteral),
                        Some(TokenKind::TextLiteral),
                    )
                } else {
                    diagnostics.push(Diagnostic::new(
                        if closed {
                            "syntax.invalid_literal"
                        } else {
                            "syntax.unclosed_literal"
                        },
                        SourceRange::new_shared(path, start, offset),
                        RecoveryAction::PreservedInvalidBytes,
                    ));
                    (SyntaxElementKind::Invalid(SyntaxInvalidKind::Literal), None)
                }
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
                let literal = &bytes[start..offset];
                let valid_encoding = std::str::from_utf8(literal).is_ok();
                let valid_literal = if quote == b'"' {
                    decode_text_literal(literal).is_some()
                } else {
                    decode_scalar_literal(literal).is_some()
                };
                if closed && valid_encoding && valid_literal {
                    let token = if quote == b'"' {
                        TokenKind::TextLiteral
                    } else {
                        TokenKind::ScalarLiteral
                    };
                    (SyntaxElementKind::Token(token), Some(token))
                } else {
                    diagnostics.push(Diagnostic::new(
                        if closed && !valid_encoding {
                            "syntax.invalid_encoding"
                        } else if closed {
                            "syntax.invalid_literal"
                        } else {
                            "syntax.unclosed_literal"
                        },
                        SourceRange::new_shared(path, start, offset),
                        RecoveryAction::PreservedInvalidBytes,
                    ));
                    (SyntaxElementKind::Invalid(SyntaxInvalidKind::Literal), None)
                }
            }
            byte if byte.is_ascii_punctuation() => {
                offset += punctuation_width(&bytes[start..]);
                let token = classify_token_bytes(&bytes[start..offset]);
                if token == TokenKind::Invalid {
                    diagnostics.push(Diagnostic::new(
                        "syntax.invalid_token",
                        SourceRange::new_shared(path, start, offset),
                        RecoveryAction::PreservedInvalidBytes,
                    ));
                    (SyntaxElementKind::Invalid(SyntaxInvalidKind::Token), None)
                } else {
                    (SyntaxElementKind::Token(token), Some(token))
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
                    SourceRange::new_shared(path, start, offset),
                    RecoveryAction::PreservedInvalidBytes,
                ));
                (SyntaxElementKind::Invalid(SyntaxInvalidKind::Byte), None)
            }
        };
        elements.push(SyntaxElement::new(kind, path, start, offset));
        if let Some(kind) = token_kind {
            lexemes.push(Lexeme {
                kind,
                range: SourceRange::new_shared(path, start, offset),
            });
        }
    }

    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        diagnostics.push(
            Diagnostic::new(
                "syntax.byte_order_mark",
                SourceRange::new_shared(path, 0, 3),
                RecoveryAction::PreservedInvalidBytes,
            )
            .with_parameter("encoding", "utf-8"),
        );
    }

    let mut layout_elements = Vec::new();
    if scan_layout_and_delimiters(
        file,
        &lexemes,
        &mut layout_elements,
        &mut diagnostics,
        cancellation,
    ) {
        return cancelled_source(file, elements, diagnostics, imports, declarations);
    }
    elements = merge_syntax_elements(elements, layout_elements);
    let Some(parsed_imports) = parse_imports(file, &lexemes, &mut diagnostics, cancellation) else {
        return cancelled_source(file, elements, diagnostics, imports, declarations);
    };
    imports = parsed_imports;
    let Some(parsed_declarations) =
        parse_declarations(file, &lexemes, &mut diagnostics, cancellation)
    else {
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
        let exceeds_nesting_limit = diagnostics.iter().any(|diagnostic| {
            diagnostic.code() == "syntax.nesting_limit_exceeded"
                && diagnostic.primary().start() >= declarations[index].start
                && diagnostic.primary().start() < declarations[index].end
        });
        if exceeds_nesting_limit {
            declarations[index].structurally_valid = false;
            continue;
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
    let comptime_selections =
        parse_comptime_selections(file, &lexemes, &mut diagnostics, cancellation);
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
                && diagnostic.typed_parameters().iter().any(|(name, value)| {
                    name.as_ref() == "opener_offset"
                        && matches!(value, DiagnosticValue::Unsigned(offset) if
                            u64::try_from(*offset).is_ok_and(|offset| {
                            offset >= declaration.start && offset < declaration.end
                        }))
                });
            direct || unmatched_opener
        });
    }
    if diagnostics.len() > 64 {
        diagnostics.truncate(64);
        diagnostics.push(Diagnostic::new(
            "syntax.diagnostics_truncated",
            SourceRange::new_shared(path, bytes.len(), bytes.len()),
            RecoveryAction::TruncatedDiagnostics,
        ));
    }

    let mut tree_declarations = declarations.clone();
    tree_declarations.extend(
        comptime_selections
            .iter()
            .flat_map(|selection| &selection.branches)
            .flat_map(|branch| branch.declarations.iter().cloned()),
    );
    tree_declarations.sort_by_key(|declaration| declaration.start);
    let Some(tree) = build_green_tree(file, &tree_declarations, &elements, cancellation) else {
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
        comptime_selections,
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
        comptime_selections: Vec::new(),
        cancelled: true,
    }
}

fn cancelled_green(file: &ProjectFile) -> GreenNode {
    GreenNode {
        kind: SyntaxNodeKind::Source,
        range: SourceRange::new_shared(file.path_arc(), 0, file.bytes().len()),
        children: std::sync::Arc::from([]),
    }
}

fn numeric_token_end(bytes: &[u8], start: usize, cancellation: &Cancellation) -> usize {
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

fn classify_numeric_literal(bytes: &[u8]) -> Option<TokenKind> {
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
        SyntaxNodeKind::Source,
        SourceRange::new_shared(file.path_arc(), 0, file.bytes().len()),
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

    let mut stack: Vec<(SyntaxNodeKind, SourceRange, Vec<GreenChild>)> = Vec::new();
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

fn merge_syntax_elements(
    physical: Vec<SyntaxElement>,
    layout: Vec<SyntaxElement>,
) -> Vec<SyntaxElement> {
    let mut physical = physical.into_iter().peekable();
    let mut layout = layout.into_iter().peekable();
    let mut merged = Vec::with_capacity(physical.len() + layout.len());
    loop {
        let take_layout = match (physical.peek(), layout.peek()) {
            (Some(physical), Some(layout)) => {
                (layout.range().start(), layout.range().end())
                    < (physical.range().start(), physical.range().end())
            }
            (None, Some(_)) => true,
            (Some(_), None) => false,
            (None, None) => break,
        };
        if take_layout {
            merged.push(layout.next().expect("peeked layout element"));
        } else {
            merged.push(physical.next().expect("peeked physical element"));
        }
    }
    merged
}

fn declaration_region(file: &ProjectFile, declaration: &Declaration) -> SyntaxRegion {
    let mut children = Vec::new();
    if let Some(syntax) = &declaration.syntax {
        match syntax {
            DeclarationSyntax::Function(function) => {
                children.push(SyntaxRegion {
                    kind: SyntaxNodeKind::FunctionSignature,
                    range: declaration.range.clone(),
                    children: function
                        .parameters
                        .iter()
                        .map(|parameter| SyntaxRegion {
                            kind: SyntaxNodeKind::Parameter,
                            range: parameter.range.clone(),
                            children: Vec::new(),
                        })
                        .collect(),
                });
                let mut statements = Vec::new();
                collect_statement_regions(&function.body, &mut statements);
                children.push(SyntaxRegion {
                    kind: SyntaxNodeKind::Block,
                    range: SourceRange::from_u64_shared(
                        file.path_arc(),
                        declaration.range.end(),
                        declaration.end,
                    ),
                    children: statements,
                });
            }
            DeclarationSyntax::Constant(constant) => children.push(SyntaxRegion {
                kind: SyntaxNodeKind::ConstantValue,
                range: declaration.range.clone(),
                children: vec![expression_region(&constant.value)],
            }),
            DeclarationSyntax::TypeAlias(_) => {}
            DeclarationSyntax::Suite(suite) => {
                children.push(SyntaxRegion {
                    kind: SyntaxNodeKind::SuiteHeader,
                    range: declaration.range.clone(),
                    children: Vec::new(),
                });
                children.extend(suite.tests.iter().map(|test| {
                    let mut test_children = test
                        .parameters
                        .iter()
                        .map(|parameter| SyntaxRegion {
                            kind: SyntaxNodeKind::Parameter,
                            range: parameter.range.clone(),
                            children: Vec::new(),
                        })
                        .collect::<Vec<_>>();
                    collect_statement_regions(&test.body, &mut test_children);
                    SyntaxRegion {
                        kind: if test.asynchronous {
                            SyntaxNodeKind::AsyncTest
                        } else {
                            SyntaxNodeKind::Test
                        },
                        range: test.range.clone(),
                        children: test_children,
                    }
                }));
            }
            DeclarationSyntax::Enum(enum_) => {
                let variants = enum_
                    .variants
                    .iter()
                    .map(|variant| SyntaxRegion {
                        kind: SyntaxNodeKind::Variant,
                        range: variant.range.clone(),
                        children: variant
                            .parameters
                            .iter()
                            .map(|parameter| SyntaxRegion {
                                kind: SyntaxNodeKind::Parameter,
                                range: parameter.range.clone(),
                                children: Vec::new(),
                            })
                            .collect(),
                    })
                    .collect();
                let functions = enum_.functions.iter().map(member_function_region).collect();
                let mut regions = merge_regions(variants, functions);
                regions.extend(
                    enum_
                        .comptime_selections
                        .iter()
                        .map(member_selection_region),
                );
                regions.sort_by_key(|region| region.range.start());
                children.extend(regions);
            }
            DeclarationSyntax::Struct(struct_) | DeclarationSyntax::ResourceStruct(struct_) => {
                let fields = struct_
                    .fields
                    .iter()
                    .map(|field| SyntaxRegion {
                        kind: SyntaxNodeKind::Field,
                        range: field.range.clone(),
                        children: Vec::new(),
                    })
                    .collect();
                let functions = struct_
                    .functions
                    .iter()
                    .map(member_function_region)
                    .collect();
                let mut regions = merge_regions(fields, functions);
                regions.extend(
                    struct_
                        .comptime_selections
                        .iter()
                        .map(member_selection_region),
                );
                regions.sort_by_key(|region| region.range.start());
                children.extend(regions);
            }
            DeclarationSyntax::Interface(interface) => {
                children.extend(interface.requirements.iter().map(|requirement| {
                    SyntaxRegion {
                        kind: SyntaxNodeKind::FunctionRequirement,
                        range: requirement.range.clone(),
                        children: requirement
                            .parameters
                            .iter()
                            .map(|parameter| SyntaxRegion {
                                kind: SyntaxNodeKind::Parameter,
                                range: parameter.range.clone(),
                                children: Vec::new(),
                            })
                            .collect(),
                    }
                }));
            }
            DeclarationSyntax::Pool => {}
        }
    }
    SyntaxRegion {
        kind: declaration.kind.node_kind(),
        range: SourceRange::from_u64_shared(file.path_arc(), declaration.start, declaration.end),
        children,
    }
}

fn member_selection_region(selection: &ComptimeMemberSelection) -> SyntaxRegion {
    SyntaxRegion {
        kind: SyntaxNodeKind::ComptimeSelection,
        range: selection.range.clone(),
        children: selection
            .branches
            .iter()
            .map(|branch| {
                let mut children = branch
                    .condition
                    .iter()
                    .map(expression_region)
                    .collect::<Vec<_>>();
                children.extend(branch.fields.iter().map(|field| SyntaxRegion {
                    kind: SyntaxNodeKind::Field,
                    range: field.range.clone(),
                    children: Vec::new(),
                }));
                children.extend(branch.variants.iter().map(|variant| {
                    SyntaxRegion {
                        kind: SyntaxNodeKind::Variant,
                        range: variant.range.clone(),
                        children: variant
                            .parameters
                            .iter()
                            .map(|parameter| SyntaxRegion {
                                kind: SyntaxNodeKind::Parameter,
                                range: parameter.range.clone(),
                                children: Vec::new(),
                            })
                            .collect(),
                    }
                }));
                children.extend(branch.functions.iter().map(member_function_region));
                children.sort_by_key(|region| region.range.start());
                SyntaxRegion {
                    kind: SyntaxNodeKind::ComptimeBranch,
                    range: branch.range.clone(),
                    children,
                }
            })
            .collect(),
    }
}

fn collect_statement_regions(statements: &[StatementSyntax], output: &mut Vec<SyntaxRegion>) {
    let mut pending = statements.iter().rev().collect::<Vec<_>>();
    while let Some(statement) = pending.pop() {
        if let StatementSyntax::Comptime { branches, range } = statement {
            output.push(SyntaxRegion {
                kind: SyntaxNodeKind::ComptimeSelection,
                range: range.clone(),
                children: branches
                    .iter()
                    .map(|branch| {
                        let mut children = branch
                            .condition
                            .iter()
                            .map(expression_region)
                            .collect::<Vec<_>>();
                        collect_statement_regions(&branch.statements, &mut children);
                        SyntaxRegion {
                            kind: SyntaxNodeKind::ComptimeBranch,
                            range: branch.range.clone(),
                            children,
                        }
                    })
                    .collect(),
            });
            continue;
        }
        let (kind, range, expressions) = match statement {
            StatementSyntax::Return { value, range } => (
                SyntaxNodeKind::ReturnStatement,
                range,
                value.iter().map(expression_region).collect(),
            ),
            StatementSyntax::Panic { value, range } => (
                SyntaxNodeKind::PanicStatement,
                range,
                vec![expression_region(value)],
            ),
            StatementSyntax::Assert { condition, range } => (
                SyntaxNodeKind::AssertStatement,
                range,
                vec![expression_region(condition)],
            ),
            StatementSyntax::Expect { condition, range } => (
                SyntaxNodeKind::ExpectStatement,
                range,
                vec![expression_region(condition)],
            ),
            StatementSyntax::Assign { value, range, .. } => (
                SyntaxNodeKind::InitializeStatement,
                range,
                vec![expression_region(value)],
            ),
            StatementSyntax::Evaluate(value) => (
                SyntaxNodeKind::ExpressionStatement,
                &value.range,
                vec![expression_region(value)],
            ),
            StatementSyntax::If {
                condition, range, ..
            } => (
                SyntaxNodeKind::IfStatement,
                range,
                vec![expression_region(condition)],
            ),
            StatementSyntax::Comptime { .. } => unreachable!("handled before generic statement"),
            StatementSyntax::For {
                pattern,
                iterable,
                range,
                ..
            } => (
                SyntaxNodeKind::ForStatement,
                range,
                vec![pattern_region(pattern), expression_region(iterable)],
            ),
            StatementSyntax::While {
                condition, range, ..
            } => (
                SyntaxNodeKind::WhileStatement,
                range,
                vec![expression_region(condition)],
            ),
            StatementSyntax::Break(range) => (SyntaxNodeKind::BreakStatement, range, Vec::new()),
            StatementSyntax::Continue(range) => {
                (SyntaxNodeKind::ContinueStatement, range, Vec::new())
            }
            StatementSyntax::Match { value, range, .. } => (
                SyntaxNodeKind::MatchStatement,
                range,
                vec![expression_region(value)],
            ),
            StatementSyntax::Defer { expression, range } => (
                SyntaxNodeKind::DeferStatement,
                range,
                vec![expression_region(expression)],
            ),
            StatementSyntax::With { scope, range, .. } => (
                SyntaxNodeKind::WithStatement,
                range,
                vec![expression_region(scope)],
            ),
            StatementSyntax::Unsupported { kind, range } => {
                (unsupported_statement_node(*kind), range, Vec::new())
            }
            StatementSyntax::Pass(range) => (SyntaxNodeKind::PassStatement, range, Vec::new()),
        };
        output.push(SyntaxRegion {
            kind,
            range: range.clone(),
            children: expressions,
        });
        match statement {
            StatementSyntax::If {
                then_branch,
                else_branch,
                ..
            } => {
                pending.extend(else_branch.iter().rev());
                pending.extend(then_branch.iter().rev());
            }
            StatementSyntax::Comptime { .. } => unreachable!("handled before generic statement"),
            StatementSyntax::For { body, .. } => pending.extend(body.iter().rev()),
            StatementSyntax::While { body, .. } => pending.extend(body.iter().rev()),
            StatementSyntax::With { body, .. } => pending.extend(body.iter().rev()),
            StatementSyntax::Match { cases, .. } => {
                for case in cases {
                    output.push(SyntaxRegion {
                        kind: SyntaxNodeKind::MatchCase,
                        range: case.range.clone(),
                        children: std::iter::once(pattern_region(&case.pattern))
                            .chain(case.guard.iter().map(expression_region))
                            .collect(),
                    });
                    collect_statement_regions(&case.body, output);
                }
            }
            _ => {}
        }
    }
}

fn pattern_region(pattern: &PatternSyntax) -> SyntaxRegion {
    let (kind, children) = match &pattern.kind {
        PatternSyntaxKind::Wildcard | PatternSyntaxKind::Binding(_) => {
            (SyntaxNodeKind::NameExpression, Vec::new())
        }
        PatternSyntaxKind::Literal(expression) => return expression_region(expression),
        PatternSyntaxKind::Take(pattern) => (
            SyntaxNodeKind::TakeExpression,
            vec![pattern_region(pattern)],
        ),
        PatternSyntaxKind::Constructor { arguments, .. } => (
            SyntaxNodeKind::CallExpression,
            arguments
                .iter()
                .map(|argument| pattern_region(&argument.pattern))
                .collect(),
        ),
        PatternSyntaxKind::Tuple(patterns) => (
            SyntaxNodeKind::TupleExpression,
            patterns.iter().map(pattern_region).collect(),
        ),
        PatternSyntaxKind::FixedArray(patterns) => (
            SyntaxNodeKind::ArrayExpression,
            patterns.iter().map(pattern_region).collect(),
        ),
        PatternSyntaxKind::Or(patterns) => (
            SyntaxNodeKind::BinaryExpression,
            patterns.iter().map(pattern_region).collect(),
        ),
    };
    SyntaxRegion {
        kind,
        range: pattern.range.clone(),
        children,
    }
}

fn member_function_region(function: &MemberFunctionSyntax) -> SyntaxRegion {
    let mut children = function
        .function
        .parameters
        .iter()
        .map(|parameter| SyntaxRegion {
            kind: SyntaxNodeKind::Parameter,
            range: parameter.range.clone(),
            children: Vec::new(),
        })
        .collect::<Vec<_>>();
    collect_statement_regions(&function.function.body, &mut children);
    SyntaxRegion {
        kind: SyntaxNodeKind::MemberFunction,
        range: function.range.clone(),
        children,
    }
}

fn merge_regions(left: Vec<SyntaxRegion>, right: Vec<SyntaxRegion>) -> Vec<SyntaxRegion> {
    let mut left = left.into_iter().peekable();
    let mut right = right.into_iter().peekable();
    let mut merged = Vec::with_capacity(left.len() + right.len());
    loop {
        let take_right = match (left.peek(), right.peek()) {
            (Some(left), Some(right)) => right.range.start() < left.range.start(),
            (None, Some(_)) => true,
            (Some(_), None) => false,
            (None, None) => break,
        };
        if take_right {
            merged.push(right.next().expect("peeked right region"));
        } else {
            merged.push(left.next().expect("peeked left region"));
        }
    }
    merged
}

fn expression_region(expression: &ExpressionSyntax) -> SyntaxRegion {
    let (kind, children) = match &expression.kind {
        ExpressionSyntaxKind::Integer(_) => (SyntaxNodeKind::IntegerExpression, Vec::new()),
        ExpressionSyntaxKind::Float(_) => (SyntaxNodeKind::FloatExpression, Vec::new()),
        ExpressionSyntaxKind::Text(_) => (SyntaxNodeKind::TextExpression, Vec::new()),
        ExpressionSyntaxKind::Scalar(_) => (SyntaxNodeKind::ScalarExpression, Vec::new()),
        ExpressionSyntaxKind::Bytes(_) => (SyntaxNodeKind::BytesExpression, Vec::new()),
        ExpressionSyntaxKind::Bool(_) => (SyntaxNodeKind::BoolExpression, Vec::new()),
        ExpressionSyntaxKind::Name(_) => (SyntaxNodeKind::NameExpression, Vec::new()),
        ExpressionSyntaxKind::Call { arguments, .. } => (
            SyntaxNodeKind::CallExpression,
            arguments
                .iter()
                .map(|argument| expression_region(&argument.value))
                .collect(),
        ),
        ExpressionSyntaxKind::Array(values) => (
            SyntaxNodeKind::ArrayExpression,
            values.iter().map(expression_region).collect(),
        ),
        ExpressionSyntaxKind::RepeatedArray { value, .. } => (
            SyntaxNodeKind::RepeatedArrayExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::Tuple(values) => (
            SyntaxNodeKind::TupleExpression,
            values.iter().map(expression_region).collect(),
        ),
        ExpressionSyntaxKind::Index { value, index } => (
            SyntaxNodeKind::IndexExpression,
            vec![expression_region(value), expression_region(index)],
        ),
        ExpressionSyntaxKind::Unit => (SyntaxNodeKind::UnitExpression, Vec::new()),
        ExpressionSyntaxKind::Positive(value) => (
            SyntaxNodeKind::PositiveExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::Negate(value) => (
            SyntaxNodeKind::NegateExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::BitNot(value) => (
            SyntaxNodeKind::BitNotExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::Not(value) => (
            SyntaxNodeKind::NotExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::Await(value) => (
            SyntaxNodeKind::AwaitExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::Mut(value) => (
            SyntaxNodeKind::MutExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::Take(value) => (
            SyntaxNodeKind::TakeExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::Closure { body, .. } => (
            SyntaxNodeKind::ClosureExpression,
            vec![expression_region(body)],
        ),
        ExpressionSyntaxKind::Propagate(value) => (
            SyntaxNodeKind::PropagateExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::Is { value, pattern } => (
            SyntaxNodeKind::IsExpression,
            vec![expression_region(value), pattern_region(pattern)],
        ),
        ExpressionSyntaxKind::Binary {
            operator,
            left,
            right,
        } => (
            if matches!(
                operator,
                BinaryOperatorSyntax::Range | BinaryOperatorSyntax::RangeInclusive
            ) {
                SyntaxNodeKind::RangeExpression
            } else {
                SyntaxNodeKind::BinaryExpression
            },
            vec![expression_region(left), expression_region(right)],
        ),
        ExpressionSyntaxKind::Unsupported(kind) => (
            match kind {
                UnsupportedExpressionKind::Send => SyntaxNodeKind::SendExpression,
                UnsupportedExpressionKind::TrySend => SyntaxNodeKind::TrySendExpression,
            },
            Vec::new(),
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
    let mut stack = vec![(region, 0_usize, false)];
    while let Some((current, child_index, started)) = stack.last_mut() {
        if cancellation.is_cancelled() {
            return true;
        }
        if !*started {
            events.push(Event::Start(current.kind, current.range.clone()));
            *started = true;
        }
        if let Some(child) = current.children.get(*child_index) {
            *child_index += 1;
            if emit_elements_before(
                child.range.start(),
                elements,
                element_index,
                events,
                cancellation,
            ) {
                return true;
            }
            stack.push((child, 0, false));
            continue;
        }
        if emit_elements_before(
            current.range.end(),
            elements,
            element_index,
            events,
            cancellation,
        ) {
            return true;
        }
        events.push(Event::Finish);
        stack.pop();
    }
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
        SyntaxElementKind::Missing(_) => Event::Missing(index),
        SyntaxElementKind::Error(_) | SyntaxElementKind::Invalid(_) => Event::Error(index),
        SyntaxElementKind::Token(_)
        | SyntaxElementKind::Trivia(_)
        | SyntaxElementKind::Layout(_) => Event::Token(index),
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
    let mut delimiter_depth = 0_u32;
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
            match lexemes[token_index].kind {
                TokenKind::LeftParen | TokenKind::LeftBracket => {
                    delimiter_depth = delimiter_depth.saturating_add(1);
                }
                TokenKind::RightParen | TokenKind::RightBracket => {
                    delimiter_depth = delimiter_depth.saturating_sub(1);
                }
                _ => {}
            }
            token_index += 1;
        }
        let first = lexemes
            .get(token_index)
            .filter(|lexeme| usize::try_from(lexeme.range.start()).is_ok_and(|start| start < end));
        let accepted = first.is_none()
            || delimiter_depth > 0
            || line.first().is_some_and(u8::is_ascii_whitespace)
            || first.is_some_and(|lexeme| {
                matches!(
                    lexeme.kind,
                    TokenKind::At
                        | TokenKind::From
                        | TokenKind::Comptime
                        | TokenKind::Elif
                        | TokenKind::Else
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
                SourceRange::new_shared(file.path_arc(), offset, offset + line.len()),
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
            if member.consume(TokenKind::Fn) {
                let name = member.expect_identifier()?.to_owned();
                let type_parameters = parse_type_parameter_names(&mut member)?;
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
    let type_parameters = parse_type_parameter_names(cursor)?;
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
        if member.consume(TokenKind::Fn) {
            let name = member.expect_identifier()?.to_owned();
            let type_parameters = parse_type_parameter_names(&mut member)?;
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
        implements,
        fields,
        functions,
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
    for line in lines.iter().skip(1) {
        if cancellation.is_cancelled() || line.indent != 4 {
            return None;
        }
        let mut requirement = SyntaxCursor::new(file, &line.tokens, cancellation);
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
    cursor.expect(TokenKind::Colon)?;
    if !cursor.at_end() {
        return None;
    }
    let mut variants = Vec::new();
    let mut functions = Vec::new();
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
        if member.consume(TokenKind::Fn) {
            let name = member.expect_identifier()?.to_owned();
            let member_type_parameters = parse_type_parameter_names(&mut member)?;
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
        variants,
        functions,
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

fn parse_type_parameter_names(cursor: &mut SyntaxCursor<'_, '_>) -> Option<Vec<String>> {
    let mut parameters = Vec::new();
    if cursor.consume(TokenKind::LeftBracket) {
        while !cursor.consume(TokenKind::RightBracket) {
            parameters.push(cursor.expect_identifier()?.to_owned());
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

fn unsupported_statement_node(kind: UnsupportedStatementKind) -> SyntaxNodeKind {
    match kind {
        UnsupportedStatementKind::Take => SyntaxNodeKind::TakeStatement,
        UnsupportedStatementKind::Send => SyntaxNodeKind::SendStatement,
        UnsupportedStatementKind::TrySend => SyntaxNodeKind::TrySendStatement,
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
            kind @ (TokenKind::Take | TokenKind::Send | TokenKind::TrySend) => {
                StatementSyntax::Unsupported {
                    kind: match kind {
                        TokenKind::Take => UnsupportedStatementKind::Take,
                        TokenKind::Send => UnsupportedStatementKind::Send,
                        TokenKind::TrySend => UnsupportedStatementKind::TrySend,
                        _ => unreachable!("guarded opaque statement"),
                    },
                    range: line.range.clone(),
                }
            }
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

fn parse_comptime_selections(
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
                let length = self.expect_integer_literal()?.parse().ok()?;
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
            .any(|token| token.kind == TokenKind::TrySend)
        {
            UnsupportedExpressionKind::TrySend
        } else if self
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

fn token_text<'a>(file: &'a ProjectFile, token: &Lexeme) -> Option<&'a str> {
    std::str::from_utf8(checked_slice(
        file.bytes(),
        token.range.start(),
        token.range.end(),
    )?)
    .ok()
}

fn decode_text_literal(bytes: &[u8]) -> Option<String> {
    let interior = bytes.strip_prefix(b"\"")?.strip_suffix(b"\"")?;
    decode_text_content(interior)
}

fn decode_scalar_literal(bytes: &[u8]) -> Option<char> {
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

fn decode_bytes_literal(bytes: &[u8]) -> Option<Vec<u8>> {
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

fn multiline_literal_end(bytes: &[u8], start: usize, cancellation: &Cancellation) -> (usize, bool) {
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

fn decode_multiline_text_literal(bytes: &[u8]) -> Option<String> {
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
