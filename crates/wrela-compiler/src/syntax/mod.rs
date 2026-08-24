#![forbid(unsafe_code)]

mod green;
mod layout;
mod lexer;
mod parser;

use green::*;
use layout::scan_layout_and_delimiters;
use lexer::*;
use parser::*;

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

pub(crate) type TokenKind = SyntaxTokenKind;

#[derive(Clone, Debug)]
struct Lexeme {
    kind: TokenKind,
    range: SourceRange,
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
    ConstU64(u64),
    Named(NameSyntax),
    Apply {
        base: NameSyntax,
        arguments: Vec<TypeSyntax>,
    },
    Array(Box<TypeSyntax>),
    Tuple(Vec<TypeSyntax>),
    FixedArray {
        element: Box<TypeSyntax>,
        length: FixedArrayLengthSyntax,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FixedArrayLengthSyntax {
    Literal(u64),
    Parameter(String),
}

#[derive(Clone, Debug)]
pub(crate) struct GenericParameterSyntax {
    pub(crate) name: String,
    pub(crate) kind: GenericParameterKindSyntax,
}

#[derive(Clone, Debug)]
pub(crate) enum GenericParameterKindSyntax {
    Type { interface_bound: Option<NameSyntax> },
    Const { type_syntax: TypeSyntax },
    Pool,
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
    pub(crate) generic_parameters: Vec<GenericParameterSyntax>,
    pub(crate) implements: Vec<NameSyntax>,
    pub(crate) fields: Vec<FieldSyntax>,
    pub(crate) functions: Vec<MemberFunctionSyntax>,
    pub(crate) constants: Vec<MemberConstantSyntax>,
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
pub(crate) struct MemberConstantSyntax {
    pub(crate) name: String,
    pub(crate) public: bool,
    pub(crate) type_syntax: TypeSyntax,
    pub(crate) value: Option<ExpressionSyntax>,
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
    pub(crate) constants: Vec<MemberConstantSyntax>,
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
    pub(crate) generic_parameters: Vec<GenericParameterSyntax>,
    pub(crate) variants: Vec<VariantSyntax>,
    pub(crate) functions: Vec<MemberFunctionSyntax>,
    pub(crate) constants: Vec<MemberConstantSyntax>,
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
    pub(crate) constants: Vec<MemberConstantSyntax>,
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
    pub(crate) generic_parameters: Vec<GenericParameterSyntax>,
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

fn token_text<'a>(file: &'a ProjectFile, token: &Lexeme) -> Option<&'a str> {
    std::str::from_utf8(checked_slice(
        file.bytes(),
        token.range.start(),
        token.range.end(),
    )?)
    .ok()
}

pub(crate) fn checked_slice(bytes: &[u8], start: u64, end: u64) -> Option<&[u8]> {
    if start > end {
        return None;
    }
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    bytes.get(start..end)
}
