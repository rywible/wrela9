use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use xxhash_rust::xxh3::Xxh3;

use crate::architecture_planning::{
    ArchitecturePlanningObservation, ArchitectureProfile, ContractFailureKind,
    VerifiedArchitecturePlanningContract,
};
use crate::image_planning::{
    PlanningFailure, PlanningFoundationObservation, VerifiedPlanningFoundation,
};
use crate::syntax;
use crate::typed_hir::AuthorityContext;
use crate::{
    distribution::CompilerDistribution,
    identity,
    identity::{IdentityCollision, IdentityFailure},
    semantic,
};

#[derive(Clone, Debug)]
pub struct CompilerInstallation {
    pub(crate) authenticated_modules: Arc<[ProjectFile]>,
}

impl Default for CompilerInstallation {
    fn default() -> Self {
        Self {
            authenticated_modules: Arc::from([]),
        }
    }
}

impl CompilerInstallation {
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_authenticated_modules(mut modules: Vec<ProjectFile>) -> Self {
        modules.push(layer1_build_module());
        modules.push(layer1_pool_module());
        Self {
            authenticated_modules: modules.into(),
        }
    }

    #[must_use]
    pub fn layer1() -> Self {
        Self::with_authenticated_modules(Vec::new())
    }
}

fn layer1_build_module() -> ProjectFile {
    ProjectFile::new(
        "src/core/build.wr",
        br#"pub struct Image:
    pub pure fn new() -> Image:
        panic "sealed Image constructor"

pub struct Test:
    pub pure fn new(cases: [TestApplication]) -> Test:
        panic "sealed Test constructor"
"#,
    )
}

fn layer1_pool_module() -> ProjectFile {
    ProjectFile::new(
        "src/core/pool.wr",
        br#"pub resource struct Scope:
    capacity: u64

pub pure fn scoped(capacity: u64) -> Scope:
    return Scope(capacity=capacity)
"#,
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectFile {
    path: Arc<str>,
    bytes: Arc<[u8]>,
}

impl ProjectFile {
    #[must_use]
    pub fn new(path: impl Into<Arc<str>>, bytes: impl AsRef<[u8]>) -> Self {
        Self {
            path: path.into(),
            bytes: Arc::from(bytes.as_ref()),
        }
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    pub(crate) const fn path_arc(&self) -> &Arc<str> {
        &self.path
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectSnapshot {
    files: Arc<[ProjectFile]>,
    revision: Arc<str>,
    digest: u128,
}

impl ProjectSnapshot {
    #[must_use]
    pub fn new(files: Vec<ProjectFile>) -> Self {
        let digest = project_snapshot_digest(&files);
        Self {
            files: files.into(),
            revision: format!("snapshot-{digest:032x}").into(),
            digest,
        }
    }

    #[must_use]
    pub fn with_revision(files: Vec<ProjectFile>, revision: impl Into<Arc<str>>) -> Self {
        let digest = project_snapshot_digest(&files);
        Self {
            files: files.into(),
            revision: revision.into(),
            digest,
        }
    }

    pub fn verified(
        files: Vec<ProjectFile>,
        revision: impl Into<Arc<str>>,
        expected_digest: u128,
    ) -> Result<Self, SnapshotDigestMismatch> {
        let snapshot = Self::with_revision(files, revision);
        if snapshot.digest == expected_digest {
            Ok(snapshot)
        } else {
            Err(SnapshotDigestMismatch {
                expected: expected_digest,
                actual: snapshot.digest,
            })
        }
    }

    #[must_use]
    pub fn revision(&self) -> &str {
        &self.revision
    }

    #[must_use]
    pub const fn digest(&self) -> u128 {
        self.digest
    }
}

impl Default for ProjectSnapshot {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotDigestMismatch {
    expected: u128,
    actual: u128,
}

impl SnapshotDigestMismatch {
    #[must_use]
    pub const fn expected(&self) -> u128 {
        self.expected
    }

    #[must_use]
    pub const fn actual(&self) -> u128 {
        self.actual
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Root {
    Image,
    Test,
}

impl Root {
    fn path(self) -> &'static str {
        match self {
            Self::Image => "src/image.wr",
            Self::Test => "src/test.wr",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InspectSelection {
    syntax: bool,
    closure: bool,
    identities: bool,
    semantics: bool,
    evaluation: bool,
    construction: bool,
    tests: bool,
    architecture_planning: bool,
    planning: bool,
}

impl InspectSelection {
    #[must_use]
    pub const fn none() -> Self {
        Self {
            syntax: false,
            closure: false,
            identities: false,
            semantics: false,
            evaluation: false,
            construction: false,
            tests: false,
            architecture_planning: false,
            planning: false,
        }
    }

    #[must_use]
    pub const fn syntax() -> Self {
        Self {
            syntax: true,
            ..Self::none()
        }
    }

    #[must_use]
    pub const fn planning() -> Self {
        Self {
            architecture_planning: true,
            planning: true,
            ..Self::none()
        }
    }

    #[must_use]
    pub const fn all() -> Self {
        Self {
            syntax: true,
            closure: true,
            identities: true,
            semantics: true,
            evaluation: true,
            construction: true,
            tests: true,
            architecture_planning: true,
            planning: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CompilationRequest {
    project: ProjectSnapshot,
    root: Root,
    inspection: InspectSelection,
    architecture_profile: Option<ArchitectureProfile>,
}

impl CompilationRequest {
    #[must_use]
    pub fn new(project: ProjectSnapshot, root: Root) -> Self {
        Self {
            project,
            root,
            inspection: InspectSelection::none(),
            architecture_profile: None,
        }
    }

    #[must_use]
    pub fn with_inspection(mut self, inspection: InspectSelection) -> Self {
        self.inspection = inspection;
        self
    }

    #[must_use]
    pub fn with_architecture_profile(mut self, profile: ArchitectureProfile) -> Self {
        self.architecture_profile = Some(profile);
        self
    }
}

#[derive(Clone, Debug, Default)]
pub struct Cancellation {
    cancelled: Arc<AtomicBool>,
    #[cfg(test)]
    polls_before_cancel: Arc<std::sync::atomic::AtomicUsize>,
}

impl Cancellation {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        #[cfg(test)]
        {
            let remaining = self.polls_before_cancel.load(Ordering::Acquire);
            if remaining > 0 && self.polls_before_cancel.fetch_sub(1, Ordering::AcqRel) == 1 {
                self.cancel();
            }
        }
        self.cancelled.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn cancel_after_private_polls(&self, polls: usize) {
        self.polls_before_cancel.store(polls, Ordering::Release);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceRange {
    path: Arc<str>,
    start: u64,
    end: u64,
}

impl SourceRange {
    pub(crate) fn new(path: &str, start: usize, end: usize) -> Self {
        Self {
            path: Arc::from(path),
            start: u64::try_from(start).expect("usize always fits u64 on supported hosts"),
            end: u64::try_from(end).expect("usize always fits u64 on supported hosts"),
        }
    }

    pub(crate) fn from_u64(path: &str, start: u64, end: u64) -> Self {
        debug_assert!(start <= end);
        Self {
            path: Arc::from(path),
            start,
            end,
        }
    }

    pub(crate) fn from_u64_shared(path: &Arc<str>, start: u64, end: u64) -> Self {
        debug_assert!(start <= end);
        Self {
            path: Arc::clone(path),
            start,
            end,
        }
    }

    pub(crate) fn new_shared(path: &Arc<str>, start: usize, end: usize) -> Self {
        Self {
            path: Arc::clone(path),
            start: u64::try_from(start).expect("usize always fits u64 on supported hosts"),
            end: u64::try_from(end).expect("usize always fits u64 on supported hosts"),
        }
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn path_arc(&self) -> &Arc<str> {
        &self.path
    }

    #[must_use]
    pub const fn start(&self) -> u64 {
        self.start
    }

    #[must_use]
    pub const fn end(&self) -> u64 {
        self.end
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticLabel {
    range: SourceRange,
    role: DiagnosticLabelRole,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticLabelRole {
    PreviousDeclaration,
    PropagationSource,
    Related,
}

impl DiagnosticLabel {
    #[must_use]
    pub fn range(&self) -> &SourceRange {
        &self.range
    }

    #[must_use]
    pub fn role(&self) -> &str {
        match self.role {
            DiagnosticLabelRole::PreviousDeclaration => "previous_declaration",
            DiagnosticLabelRole::PropagationSource => "propagation_source",
            DiagnosticLabelRole::Related => "related",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiagnosticValue {
    Text(Arc<str>),
    Unsigned(u128),
    Signed(i128),
    Identity {
        domain: IdentityDomain,
        digest: u128,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryAction {
    None,
    PreservedInvalidBytes,
    InsertedMissing { expected: Arc<str> },
    SkippedToBoundary,
    TruncatedDiagnostics,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    code: Arc<str>,
    primary: SourceRange,
    labels: Arc<[DiagnosticLabel]>,
    typed_parameters: Arc<[(Arc<str>, DiagnosticValue)]>,
    recovery: RecoveryAction,
}

impl Diagnostic {
    pub(crate) fn new(code: &'static str, primary: SourceRange, recovery: RecoveryAction) -> Self {
        Self {
            code: Arc::from(code),
            primary,
            labels: Arc::from([]),
            typed_parameters: Arc::from([]),
            recovery,
        }
    }

    pub(crate) fn with_parameter(mut self, name: &'static str, value: impl Into<Arc<str>>) -> Self {
        let value = value.into();
        let mut typed = self.typed_parameters.to_vec();
        typed.push((Arc::from(name), DiagnosticValue::Text(value)));
        self.typed_parameters = typed.into();
        self
    }

    pub(crate) fn with_unsigned_parameter(mut self, name: &'static str, value: u128) -> Self {
        let mut typed = self.typed_parameters.to_vec();
        typed.push((Arc::from(name), DiagnosticValue::Unsigned(value)));
        self.typed_parameters = typed.into();
        self
    }

    pub(crate) fn with_identity_parameter(
        mut self,
        name: &'static str,
        domain: IdentityDomain,
        digest: u128,
    ) -> Self {
        let mut typed = self.typed_parameters.to_vec();
        typed.push((
            Arc::from(name),
            DiagnosticValue::Identity { domain, digest },
        ));
        self.typed_parameters = typed.into();
        self
    }

    pub(crate) fn with_label(mut self, range: SourceRange, role: DiagnosticLabelRole) -> Self {
        let mut labels = self.labels.to_vec();
        labels.push(DiagnosticLabel { range, role });
        self.labels = labels.into();
        self
    }

    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    #[must_use]
    pub fn primary(&self) -> &SourceRange {
        &self.primary
    }

    #[must_use]
    pub fn labels(&self) -> &[DiagnosticLabel] {
        &self.labels
    }

    #[must_use]
    pub fn typed_parameters(&self) -> &[(Arc<str>, DiagnosticValue)] {
        &self.typed_parameters
    }

    #[must_use]
    pub fn recovery(&self) -> &RecoveryAction {
        &self.recovery
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxTokenKind {
    Identifier,
    IntegerLiteral,
    FloatLiteral,
    TextLiteral,
    ScalarLiteral,
    BytesLiteral,
    Fn,
    Pub,
    Pure,
    Async,
    Any,
    Return,
    Break,
    Case,
    Continue,
    Defer,
    For,
    If,
    Elif,
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
    In,
    Is,
    Match,
    And,
    Or,
    Not,
    Await,
    Own,
    Panic,
    Pass,
    Take,
    Read,
    Mut,
    SelfValue,
    Implements,
    Expect,
    Send,
    TrySend,
    While,
    With,
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
    Ampersand,
    Pipe,
    Caret,
    Tilde,
    ShiftLeft,
    ShiftRight,
    Range,
    RangeInclusive,
    Semicolon,
    Question,
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxTriviaKind {
    Spaces,
    Lf,
    Crlf,
    Comment,
    DocumentationComment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxInvalidKind {
    OversizedSource,
    Tab,
    LineEnding,
    Literal,
    Token,
    Byte,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxLayoutKind {
    Indent,
    Dedent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxMissingKind {
    Block,
    Closer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxErrorKind {
    UnexpectedIndentBlock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxElementKind {
    Token(SyntaxTokenKind),
    Trivia(SyntaxTriviaKind),
    Invalid(SyntaxInvalidKind),
    Layout(SyntaxLayoutKind),
    Missing(SyntaxMissingKind),
    Error(SyntaxErrorKind),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxElement {
    kind: SyntaxElementKind,
    range: SourceRange,
}

impl SyntaxElement {
    pub(crate) fn new(kind: SyntaxElementKind, path: &Arc<str>, start: usize, end: usize) -> Self {
        Self {
            kind,
            range: SourceRange::new_shared(path, start, end),
        }
    }

    #[must_use]
    pub fn kind(&self) -> &SyntaxElementKind {
        &self.kind
    }

    #[must_use]
    pub fn name(&self) -> &str {
        self.kind.name()
    }

    #[must_use]
    pub fn range(&self) -> &SourceRange {
        &self.range
    }
}

impl SyntaxElementKind {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Token(kind) => kind.name(),
            Self::Trivia(SyntaxTriviaKind::Spaces) => "spaces",
            Self::Trivia(SyntaxTriviaKind::Lf) => "lf",
            Self::Trivia(SyntaxTriviaKind::Crlf) => "crlf",
            Self::Trivia(SyntaxTriviaKind::Comment) => "comment",
            Self::Trivia(SyntaxTriviaKind::DocumentationComment) => "documentation_comment",
            Self::Invalid(SyntaxInvalidKind::OversizedSource) => "oversized_source",
            Self::Invalid(SyntaxInvalidKind::Tab) => "invalid_tab",
            Self::Invalid(SyntaxInvalidKind::LineEnding) => "invalid_line_ending",
            Self::Invalid(SyntaxInvalidKind::Literal) => "invalid_literal",
            Self::Invalid(SyntaxInvalidKind::Token) => "invalid_token",
            Self::Invalid(SyntaxInvalidKind::Byte) => "invalid_byte",
            Self::Layout(SyntaxLayoutKind::Indent) => "indent",
            Self::Layout(SyntaxLayoutKind::Dedent) => "dedent",
            Self::Missing(SyntaxMissingKind::Block) => "missing_block",
            Self::Missing(SyntaxMissingKind::Closer) => "missing_closer",
            Self::Error(SyntaxErrorKind::UnexpectedIndentBlock) => "unexpected_indent_block",
        }
    }
}

impl SyntaxTokenKind {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Identifier => "identifier",
            Self::IntegerLiteral => "integer_literal",
            Self::FloatLiteral => "float_literal",
            Self::TextLiteral => "text_literal",
            Self::ScalarLiteral => "scalar_literal",
            Self::BytesLiteral => "bytes_literal",
            Self::Fn => "fn",
            Self::Pub => "pub",
            Self::Pure => "pure",
            Self::Async => "async",
            Self::Any => "any",
            Self::Return => "return",
            Self::Break => "break",
            Self::Case => "case",
            Self::Continue => "continue",
            Self::Defer => "defer",
            Self::For => "for",
            Self::If => "if",
            Self::Elif => "elif",
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
            Self::In => "in",
            Self::Is => "is",
            Self::Match => "match",
            Self::And => "and",
            Self::Or => "or",
            Self::Not => "not",
            Self::Await => "await",
            Self::Own => "own",
            Self::Panic => "panic",
            Self::Pass => "pass",
            Self::Take => "take",
            Self::Read => "read",
            Self::Mut => "mut",
            Self::SelfValue => "self",
            Self::Implements => "implements",
            Self::Expect => "expect",
            Self::Send => "send",
            Self::TrySend => "try_send",
            Self::While => "while",
            Self::With => "with",
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
            Self::Ampersand => "ampersand",
            Self::Pipe => "pipe",
            Self::Caret => "caret",
            Self::Tilde => "tilde",
            Self::ShiftLeft => "shift_left",
            Self::ShiftRight => "shift_right",
            Self::Range => "range",
            Self::RangeInclusive => "range_inclusive",
            Self::Semicolon => "semicolon",
            Self::Question => "question",
            Self::Invalid => "invalid",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxObservation {
    path: Arc<str>,
    bytes: Arc<[u8]>,
    elements: Arc<[SyntaxElement]>,
    nodes: Arc<[SyntaxNodeObservation]>,
}

impl SyntaxObservation {
    pub(crate) fn new(
        file: &ProjectFile,
        elements: Vec<SyntaxElement>,
        nodes: Vec<SyntaxNodeObservation>,
    ) -> Self {
        Self {
            path: Arc::clone(&file.path),
            bytes: Arc::clone(&file.bytes),
            elements: elements.into(),
            nodes: nodes.into(),
        }
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    #[must_use]
    pub fn source_bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn elements(&self) -> &[SyntaxElement] {
        &self.elements
    }

    #[must_use]
    pub fn nodes(&self) -> &[SyntaxNodeObservation] {
        &self.nodes
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxNodeObservation {
    kind: SyntaxNodeKind,
    range: SourceRange,
    depth: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxNodeKind {
    Source,
    Function,
    Constant,
    Pool,
    TypeAlias,
    Struct,
    ResourceStruct,
    Enum,
    Interface,
    Suite,
    FunctionSignature,
    Parameter,
    Block,
    ConstantValue,
    SuiteHeader,
    Test,
    AsyncTest,
    Variant,
    Field,
    MemberFunction,
    FunctionRequirement,
    ReturnStatement,
    PanicStatement,
    AssertStatement,
    ExpectStatement,
    InitializeStatement,
    ExpressionStatement,
    IfStatement,
    ComptimeSelection,
    ComptimeBranch,
    MatchStatement,
    MatchCase,
    ForStatement,
    WhileStatement,
    BreakStatement,
    ContinueStatement,
    DeferStatement,
    WithStatement,
    TakeStatement,
    SendStatement,
    TrySendStatement,
    PassStatement,
    IntegerExpression,
    FloatExpression,
    TextExpression,
    ScalarExpression,
    BytesExpression,
    BoolExpression,
    NameExpression,
    CallExpression,
    ArrayExpression,
    TupleExpression,
    IndexExpression,
    UnitExpression,
    PositiveExpression,
    NegateExpression,
    BitNotExpression,
    NotExpression,
    AwaitExpression,
    MutExpression,
    PropagateExpression,
    BinaryExpression,
    RangeExpression,
    IsExpression,
    ClosureExpression,
    RepeatedArrayExpression,
    TakeExpression,
    SendExpression,
    TrySendExpression,
}

impl SyntaxNodeObservation {
    pub(crate) const fn new(kind: SyntaxNodeKind, range: SourceRange, depth: u16) -> Self {
        Self { kind, range, depth }
    }

    #[must_use]
    pub const fn kind(&self) -> SyntaxNodeKind {
        self.kind
    }

    #[must_use]
    pub const fn range(&self) -> &SourceRange {
        &self.range
    }

    #[must_use]
    pub const fn depth(&self) -> u16 {
        self.depth
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inspection {
    distribution_version: Arc<str>,
    distribution_digest: u128,
    project_revision: Arc<str>,
    snapshot_digest: u128,
    semantic_closure_digest: Option<u128>,
    syntax: Arc<[SyntaxObservation]>,
    closure: Arc<[Arc<str>]>,
    identities: Arc<[IdentityObservation]>,
    resolutions: Arc<[ResolutionObservation]>,
    function_facts: Arc<[FunctionFactsObservation]>,
    types: Arc<[TypeObservation]>,
    ownership: Arc<[OwnershipObservation]>,
    specializations: Arc<[SpecializationObservation]>,
    inferred_errors: Arc<[InferredErrorObservation]>,
    evaluations: Arc<[EvaluationObservation]>,
    constructions: Arc<[ConstructionObservation]>,
    test_plan: Arc<[TestApplicationObservation]>,
    architecture_planning_contract: Option<ArchitecturePlanningObservation>,
    completed_semantic_program: Option<CompletedSemanticProgramObservation>,
    planning_foundation: Option<PlanningFoundationObservation>,
}

impl Inspection {
    #[must_use]
    pub fn distribution_version(&self) -> &str {
        &self.distribution_version
    }

    #[must_use]
    pub const fn distribution_digest(&self) -> u128 {
        self.distribution_digest
    }

    #[must_use]
    pub fn project_revision(&self) -> &str {
        &self.project_revision
    }

    #[must_use]
    pub const fn snapshot_digest(&self) -> u128 {
        self.snapshot_digest
    }

    #[must_use]
    pub const fn semantic_closure_digest(&self) -> Option<u128> {
        self.semantic_closure_digest
    }

    #[must_use]
    pub fn syntax(&self) -> Option<&[SyntaxObservation]> {
        (!self.syntax.is_empty()).then_some(&self.syntax)
    }

    #[must_use]
    pub fn closure(&self) -> &[Arc<str>] {
        &self.closure
    }

    #[must_use]
    pub fn identities(&self) -> &[IdentityObservation] {
        &self.identities
    }

    #[must_use]
    pub fn resolutions(&self) -> &[ResolutionObservation] {
        &self.resolutions
    }

    #[must_use]
    pub fn function_facts(&self) -> &[FunctionFactsObservation] {
        &self.function_facts
    }

    #[must_use]
    pub fn types(&self) -> &[TypeObservation] {
        &self.types
    }

    #[must_use]
    pub fn ownership(&self) -> &[OwnershipObservation] {
        &self.ownership
    }

    #[must_use]
    pub fn specializations(&self) -> &[SpecializationObservation] {
        &self.specializations
    }

    #[must_use]
    pub fn inferred_errors(&self) -> &[InferredErrorObservation] {
        &self.inferred_errors
    }

    #[must_use]
    pub fn evaluations(&self) -> &[EvaluationObservation] {
        &self.evaluations
    }

    #[must_use]
    pub fn constructions(&self) -> &[ConstructionObservation] {
        &self.constructions
    }

    #[must_use]
    pub fn test_plan(&self) -> &[TestApplicationObservation] {
        &self.test_plan
    }

    #[must_use]
    pub const fn architecture_planning_contract(&self) -> Option<&ArchitecturePlanningObservation> {
        self.architecture_planning_contract.as_ref()
    }

    #[must_use]
    pub const fn completed_semantic_program(&self) -> Option<&CompletedSemanticProgramObservation> {
        self.completed_semantic_program.as_ref()
    }

    #[must_use]
    pub const fn planning_foundation(&self) -> Option<&PlanningFoundationObservation> {
        self.planning_foundation.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletedSemanticProgramObservation {
    fingerprint: u128,
    context_identity: u128,
    typed_program_fingerprint: u128,
    identity_catalog_revision: u128,
    custody_fingerprint: u128,
    construction_graph_fingerprint: u128,
    executable_demand_fingerprint: u128,
    solved_specialization_count: usize,
    evaluation_count: usize,
    construction_count: usize,
    executable_count: usize,
}

impl CompletedSemanticProgramObservation {
    pub(crate) const fn new(values: CompletedSemanticProgramValues) -> Self {
        Self {
            fingerprint: values.fingerprint,
            context_identity: values.context_identity,
            typed_program_fingerprint: values.typed_program_fingerprint,
            identity_catalog_revision: values.identity_catalog_revision,
            custody_fingerprint: values.custody_fingerprint,
            construction_graph_fingerprint: values.construction_graph_fingerprint,
            executable_demand_fingerprint: values.executable_demand_fingerprint,
            solved_specialization_count: values.solved_specialization_count,
            evaluation_count: values.evaluation_count,
            construction_count: values.construction_count,
            executable_count: values.executable_count,
        }
    }

    #[must_use]
    pub const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }
    #[must_use]
    pub const fn context_identity(&self) -> u128 {
        self.context_identity
    }
    #[must_use]
    pub const fn typed_program_fingerprint(&self) -> u128 {
        self.typed_program_fingerprint
    }
    #[must_use]
    pub const fn identity_catalog_revision(&self) -> u128 {
        self.identity_catalog_revision
    }
    #[must_use]
    pub const fn custody_fingerprint(&self) -> u128 {
        self.custody_fingerprint
    }
    #[must_use]
    pub const fn construction_graph_fingerprint(&self) -> u128 {
        self.construction_graph_fingerprint
    }
    #[must_use]
    pub const fn executable_demand_fingerprint(&self) -> u128 {
        self.executable_demand_fingerprint
    }
    #[must_use]
    pub const fn solved_specialization_count(&self) -> usize {
        self.solved_specialization_count
    }
    #[must_use]
    pub const fn evaluation_count(&self) -> usize {
        self.evaluation_count
    }
    #[must_use]
    pub const fn construction_count(&self) -> usize {
        self.construction_count
    }
    #[must_use]
    pub const fn executable_count(&self) -> usize {
        self.executable_count
    }
    #[must_use]
    pub const fn phase_schema(&self) -> &'static str {
        crate::completed_semantic::PHASE_SCHEMA
    }
}

pub(crate) struct CompletedSemanticProgramValues {
    pub(crate) fingerprint: u128,
    pub(crate) context_identity: u128,
    pub(crate) typed_program_fingerprint: u128,
    pub(crate) identity_catalog_revision: u128,
    pub(crate) custody_fingerprint: u128,
    pub(crate) construction_graph_fingerprint: u128,
    pub(crate) executable_demand_fingerprint: u128,
    pub(crate) solved_specialization_count: usize,
    pub(crate) evaluation_count: usize,
    pub(crate) construction_count: usize,
    pub(crate) executable_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResolutionKind {
    Reference,
    Call,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResolutionObservation {
    kind: ResolutionKind,
    range: SourceRange,
    target_domain: IdentityDomain,
    target_identity: u128,
}

impl ResolutionObservation {
    pub(crate) const fn new(
        kind: ResolutionKind,
        range: SourceRange,
        target_domain: IdentityDomain,
        target_identity: u128,
    ) -> Self {
        Self {
            kind,
            range,
            target_domain,
            target_identity,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> ResolutionKind {
        self.kind
    }

    #[must_use]
    pub const fn range(&self) -> &SourceRange {
        &self.range
    }

    #[must_use]
    pub const fn target_domain(&self) -> IdentityDomain {
        self.target_domain
    }

    #[must_use]
    pub const fn target_identity(&self) -> u128 {
        self.target_identity
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestApplicationObservation {
    suite: Arc<str>,
    test: Arc<str>,
    order: u32,
    asynchronous: bool,
    bindings: Arc<[TestBindingObservation]>,
}

impl TestApplicationObservation {
    pub(crate) fn new(
        suite: impl Into<Arc<str>>,
        test: impl Into<Arc<str>>,
        order: u32,
        asynchronous: bool,
        bindings: Vec<TestBindingObservation>,
    ) -> Self {
        Self {
            suite: suite.into(),
            test: test.into(),
            order,
            asynchronous,
            bindings: bindings.into(),
        }
    }

    #[must_use]
    pub fn suite(&self) -> &str {
        &self.suite
    }

    #[must_use]
    pub fn test(&self) -> &str {
        &self.test
    }

    #[must_use]
    pub const fn order(&self) -> u32 {
        self.order
    }

    #[must_use]
    pub const fn is_async(&self) -> bool {
        self.asynchronous
    }

    #[must_use]
    pub fn bindings(&self) -> &[TestBindingObservation] {
        &self.bindings
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestBindingObservation {
    name: Arc<str>,
    type_name: Arc<str>,
    ownership: OwnershipMode,
    value: CanonicalValue,
}

impl TestBindingObservation {
    pub(crate) fn new(
        name: impl Into<Arc<str>>,
        type_name: impl Into<Arc<str>>,
        ownership: OwnershipMode,
        value: CanonicalValue,
    ) -> Self {
        Self {
            name: name.into(),
            type_name: type_name.into(),
            ownership,
            value,
        }
    }
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    #[must_use]
    pub fn type_name(&self) -> &str {
        &self.type_name
    }
    #[must_use]
    pub const fn ownership(&self) -> OwnershipMode {
        self.ownership
    }
    #[must_use]
    pub const fn value(&self) -> &CanonicalValue {
        &self.value
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalValue {
    Unit,
    Bool(bool),
    Integer {
        type_name: Arc<str>,
        value: i128,
    },
    Float {
        type_name: Arc<str>,
        bits: u64,
    },
    Text(Arc<str>),
    Scalar(char),
    Bytes(Arc<[u8]>),
    Function {
        identity: u128,
    },
    Closure {
        identity: u128,
        captures: Arc<[CanonicalValue]>,
    },
    Tuple(Arc<[CanonicalValue]>),
    Array(Arc<[CanonicalValue]>),
    Variant {
        type_name: Arc<str>,
        variant: Arc<str>,
        payload: Arc<[CanonicalValue]>,
    },
    Struct {
        type_name: Arc<str>,
        fields: Arc<[(Arc<str>, CanonicalValue)]>,
    },
    SymbolicHandle {
        kind: ConstructionKind,
        identity: u128,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvaluationOutcome {
    Completed(CanonicalValue),
    CreatorRejected {
        kind: EvaluationRejectionKind,
    },
    Panicked {
        kind: EvaluationPanicKind,
        site: SourceRange,
    },
    LimitExceeded {
        policy: EvaluationLimitPolicy,
        ceiling: u64,
        used: u64,
    },
    Cancelled,
    Defect {
        evidence: Arc<str>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvaluationRejectionKind {
    ConstantDependencyCycle,
    UnresolvedConstant,
    UnresolvedCall,
    ArgumentCount,
    ArgumentTypeMismatch,
    ReturnTypeMismatch,
    MissingLocal,
    InvalidUnaryOperand,
    PropagationRequiresResult,
    ResultOkMissingPayload,
    InvalidBooleanOperator,
    BinaryTypeMismatch,
    AwaitNotEvaluatorEligible,
}

impl EvaluationRejectionKind {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ConstantDependencyCycle => "constant_dependency_cycle",
            Self::UnresolvedConstant => "unresolved_constant",
            Self::UnresolvedCall => "unresolved_call",
            Self::ArgumentCount => "argument_count",
            Self::ArgumentTypeMismatch => "argument_type_mismatch",
            Self::ReturnTypeMismatch => "return_type_mismatch",
            Self::MissingLocal => "missing_local",
            Self::InvalidUnaryOperand => "invalid_unary_operand",
            Self::PropagationRequiresResult => "propagation_requires_result",
            Self::ResultOkMissingPayload => "result_ok_missing_payload",
            Self::InvalidBooleanOperator => "invalid_boolean_operator",
            Self::BinaryTypeMismatch => "binary_type_mismatch",
            Self::AwaitNotEvaluatorEligible => "await_not_evaluator_eligible",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvaluationPanicKind {
    Explicit,
    AssertionFailed,
    IntegerOverflow,
    DivisionByZero,
    IndexOutOfBounds,
}

impl EvaluationPanicKind {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::AssertionFailed => "assertion_failed",
            Self::IntegerOverflow => "integer_overflow",
            Self::DivisionByZero => "division_by_zero",
            Self::IndexOutOfBounds => "index_out_of_bounds",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvaluationLimitPolicy {
    RootFuel,
    RootMemory,
    CallDepth,
    CompilationFuel,
    CompilationMemory,
}

impl EvaluationLimitPolicy {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::RootFuel => "root_fuel",
            Self::RootMemory => "root_memory",
            Self::CallDepth => "call_depth",
            Self::CompilationFuel => "compilation_fuel",
            Self::CompilationMemory => "compilation_memory",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvaluationReceipt {
    tariff_schema: Arc<str>,
    policy: EvaluationPolicy,
    root_identity: u128,
    argument_fingerprint: u128,
    evaluator_eligible: bool,
    dependency_roots: Arc<[u128]>,
    typed_hir_fingerprint: u128,
    fuel_used: u64,
    peak_memory: u64,
    provenance: Option<SourceRange>,
    relevant_identity: Option<u128>,
    call_chain: Arc<[EvaluationFrameObservation]>,
    contributors: Arc<[EvaluationContributorObservation]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvaluationFrameObservation {
    identity: u128,
    callable: Arc<str>,
    call_site: SourceRange,
}

impl EvaluationFrameObservation {
    pub(crate) fn new(
        identity: u128,
        callable: impl Into<Arc<str>>,
        call_site: SourceRange,
    ) -> Self {
        Self {
            identity,
            callable: callable.into(),
            call_site,
        }
    }
    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }
    #[must_use]
    pub fn callable(&self) -> &str {
        &self.callable
    }
    #[must_use]
    pub const fn call_site(&self) -> &SourceRange {
        &self.call_site
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvaluationContributorObservation {
    site: SourceRange,
    fuel: u64,
}

impl EvaluationContributorObservation {
    pub(crate) const fn new(site: SourceRange, fuel: u64) -> Self {
        Self { site, fuel }
    }
    #[must_use]
    pub const fn site(&self) -> &SourceRange {
        &self.site
    }
    #[must_use]
    pub const fn fuel(&self) -> u64 {
        self.fuel
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvaluationPolicy {
    Constant,
    ComptimeAssertion,
    ImageConstructor,
}

impl EvaluationReceipt {
    pub(crate) fn new(
        policy: EvaluationPolicy,
        root_identity: u128,
        dependency_roots: Vec<u128>,
        typed_hir_fingerprint: u128,
        fuel_used: u64,
        peak_memory: u64,
    ) -> Self {
        Self {
            tariff_schema: Arc::from("wrela.evaluator.tariff.v2"),
            policy,
            root_identity,
            argument_fingerprint: xxhash_rust::xxh3::xxh3_128(b"wrela.evaluation-arguments\0\x01"),
            evaluator_eligible: true,
            dependency_roots: dependency_roots.into(),
            typed_hir_fingerprint,
            fuel_used,
            peak_memory,
            provenance: None,
            relevant_identity: None,
            call_chain: Arc::from([]),
            contributors: Arc::from([]),
        }
    }

    pub(crate) fn with_failure_evidence(
        mut self,
        provenance: Option<SourceRange>,
        relevant_identity: Option<u128>,
        call_chain: Vec<EvaluationFrameObservation>,
        contributors: Vec<EvaluationContributorObservation>,
    ) -> Self {
        self.provenance = provenance;
        self.relevant_identity = relevant_identity;
        self.call_chain = call_chain.into();
        self.contributors = contributors.into();
        self
    }

    #[cfg(test)]
    pub(crate) fn with_test_tariff_schema(mut self, schema: &'static str) -> Self {
        self.tariff_schema = Arc::from(schema);
        self
    }

    #[must_use]
    pub fn tariff_schema(&self) -> &str {
        &self.tariff_schema
    }

    #[must_use]
    pub const fn policy(&self) -> EvaluationPolicy {
        self.policy
    }

    #[must_use]
    pub const fn root_identity(&self) -> u128 {
        self.root_identity
    }

    #[must_use]
    pub const fn argument_fingerprint(&self) -> u128 {
        self.argument_fingerprint
    }

    #[must_use]
    pub const fn evaluator_eligible(&self) -> bool {
        self.evaluator_eligible
    }

    #[must_use]
    pub fn dependency_roots(&self) -> &[u128] {
        &self.dependency_roots
    }

    #[must_use]
    pub const fn typed_hir_fingerprint(&self) -> u128 {
        self.typed_hir_fingerprint
    }

    #[must_use]
    pub const fn fuel_used(&self) -> u64 {
        self.fuel_used
    }

    #[must_use]
    pub const fn peak_memory(&self) -> u64 {
        self.peak_memory
    }
    #[must_use]
    pub const fn provenance(&self) -> Option<&SourceRange> {
        self.provenance.as_ref()
    }
    #[must_use]
    pub const fn relevant_identity(&self) -> Option<u128> {
        self.relevant_identity
    }
    #[must_use]
    pub fn call_chain(&self) -> &[EvaluationFrameObservation] {
        &self.call_chain
    }
    #[must_use]
    pub fn contributors(&self) -> &[EvaluationContributorObservation] {
        &self.contributors
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvaluationObservation {
    root: Arc<str>,
    outcome: EvaluationOutcome,
    receipt: EvaluationReceipt,
}

impl EvaluationObservation {
    pub(crate) fn new(
        root: impl Into<Arc<str>>,
        outcome: EvaluationOutcome,
        receipt: EvaluationReceipt,
    ) -> Self {
        Self {
            root: root.into(),
            outcome,
            receipt,
        }
    }

    #[must_use]
    pub fn root(&self) -> &str {
        &self.root
    }

    #[must_use]
    pub const fn outcome(&self) -> &EvaluationOutcome {
        &self.outcome
    }

    #[must_use]
    pub const fn receipt(&self) -> &EvaluationReceipt {
        &self.receipt
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionFactsObservation {
    identity: u128,
    name: Arc<str>,
    pure: bool,
    may_panic: bool,
    suspends: bool,
    evaluator_eligible: bool,
    ownership_transfer: bool,
    bounded: bool,
    logical_cost: u64,
}

impl FunctionFactsObservation {
    pub(crate) fn new(
        identity: u128,
        name: impl Into<Arc<str>>,
        facts: FunctionFactsValues,
    ) -> Self {
        Self {
            identity,
            name: name.into(),
            pure: facts.pure,
            may_panic: facts.may_panic,
            suspends: facts.suspends,
            evaluator_eligible: facts.evaluator_eligible,
            ownership_transfer: facts.ownership_transfer,
            bounded: facts.bounded,
            logical_cost: facts.logical_cost,
        }
    }

    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn is_pure(&self) -> bool {
        self.pure
    }

    #[must_use]
    pub const fn may_panic(&self) -> bool {
        self.may_panic
    }

    #[must_use]
    pub const fn suspends(&self) -> bool {
        self.suspends
    }

    #[must_use]
    pub const fn evaluator_eligible(&self) -> bool {
        self.evaluator_eligible
    }

    #[must_use]
    pub const fn transfers_ownership(&self) -> bool {
        self.ownership_transfer
    }

    #[must_use]
    pub const fn is_bounded(&self) -> bool {
        self.bounded
    }

    #[must_use]
    pub const fn logical_cost(&self) -> u64 {
        self.logical_cost
    }
}

pub(crate) struct FunctionFactsValues {
    pub(crate) pure: bool,
    pub(crate) may_panic: bool,
    pub(crate) suspends: bool,
    pub(crate) evaluator_eligible: bool,
    pub(crate) ownership_transfer: bool,
    pub(crate) bounded: bool,
    pub(crate) logical_cost: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeRole {
    Parameter,
    Return,
    Constant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeObservation {
    owner_identity: u128,
    name: Arc<str>,
    role: TypeRole,
    type_name: Arc<str>,
}

impl TypeObservation {
    pub(crate) fn new(
        owner_identity: u128,
        name: impl Into<Arc<str>>,
        role: TypeRole,
        type_name: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            owner_identity,
            name: name.into(),
            role,
            type_name: type_name.into(),
        }
    }
    #[must_use]
    pub const fn owner_identity(&self) -> u128 {
        self.owner_identity
    }
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    #[must_use]
    pub const fn role(&self) -> TypeRole {
        self.role
    }
    #[must_use]
    pub fn type_name(&self) -> &str {
        &self.type_name
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnershipMode {
    Value,
    Read,
    Mut,
    Take,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnershipObservation {
    owner_identity: u128,
    name: Arc<str>,
    mode: OwnershipMode,
}

impl OwnershipObservation {
    pub(crate) fn new(
        owner_identity: u128,
        name: impl Into<Arc<str>>,
        mode: OwnershipMode,
    ) -> Self {
        Self {
            owner_identity,
            name: name.into(),
            mode,
        }
    }
    #[must_use]
    pub const fn owner_identity(&self) -> u128 {
        self.owner_identity
    }
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    #[must_use]
    pub const fn mode(&self) -> OwnershipMode {
        self.mode
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecializationObservation {
    identity: u128,
    function_identity: u128,
    function: Arc<str>,
    argument_types: Arc<[Arc<str>]>,
}

impl SpecializationObservation {
    pub(crate) fn new(
        identity: u128,
        function_identity: u128,
        function: impl Into<Arc<str>>,
        argument_types: Vec<Arc<str>>,
    ) -> Self {
        Self {
            identity,
            function_identity,
            function: function.into(),
            argument_types: argument_types.into(),
        }
    }
    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }
    #[must_use]
    pub const fn function_identity(&self) -> u128 {
        self.function_identity
    }
    #[must_use]
    pub fn function(&self) -> &str {
        &self.function
    }
    #[must_use]
    pub fn argument_types(&self) -> &[Arc<str>] {
        &self.argument_types
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InferredErrorObservation {
    specialization_identity: u128,
    function: Arc<str>,
    error_type: Arc<str>,
    provenance: SourceRange,
}

impl InferredErrorObservation {
    pub(crate) fn new(
        specialization_identity: u128,
        function: impl Into<Arc<str>>,
        error_type: impl Into<Arc<str>>,
        provenance: SourceRange,
    ) -> Self {
        Self {
            specialization_identity,
            function: function.into(),
            error_type: error_type.into(),
            provenance,
        }
    }

    #[must_use]
    pub const fn specialization_identity(&self) -> u128 {
        self.specialization_identity
    }

    #[must_use]
    pub fn function(&self) -> &str {
        &self.function
    }

    #[must_use]
    pub fn error_type(&self) -> &str {
        &self.error_type
    }

    pub(crate) const fn provenance(&self) -> &SourceRange {
        &self.provenance
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstructionObservation {
    identity: u128,
    kind: ConstructionKind,
    site: SourceRange,
    edges: Arc<[u128]>,
    operands: Arc<[ConstructionOperandObservation]>,
}

impl ConstructionObservation {
    pub(crate) fn new(
        identity: u128,
        kind: ConstructionKind,
        site: SourceRange,
        edges: Vec<u128>,
        operands: Vec<ConstructionOperandObservation>,
    ) -> Self {
        Self {
            identity,
            kind,
            site,
            edges: edges.into(),
            operands: operands.into(),
        }
    }

    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }

    #[must_use]
    pub const fn kind(&self) -> ConstructionKind {
        self.kind
    }

    #[must_use]
    pub const fn site(&self) -> &SourceRange {
        &self.site
    }
    #[must_use]
    pub fn edges(&self) -> &[u128] {
        &self.edges
    }
    #[must_use]
    pub fn operands(&self) -> &[ConstructionOperandObservation] {
        &self.operands
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstructionOperandObservation {
    label: Arc<str>,
    value: CanonicalValue,
}

impl ConstructionOperandObservation {
    pub(crate) fn new(label: impl Into<Arc<str>>, value: CanonicalValue) -> Self {
        Self {
            label: label.into(),
            value,
        }
    }

    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    #[must_use]
    pub const fn value(&self) -> &CanonicalValue {
        &self.value
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstructionKind {
    Image,
    Test,
    Node { type_identity: u128 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum IdentityDomain {
    Module,
    Definition,
    Type,
    Pool,
    Specialization,
    Generated,
    SourceSite,
    Test,
    Construction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum IdentityOrigin {
    Project,
    Authenticated,
    Generated,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityObservation {
    domain: IdentityDomain,
    origin: IdentityOrigin,
    name: Arc<str>,
    digest: u128,
    fingerprint: u128,
}

impl IdentityObservation {
    pub(crate) fn new(
        domain: IdentityDomain,
        origin: IdentityOrigin,
        name: impl Into<Arc<str>>,
        digest: u128,
        fingerprint: u128,
    ) -> Self {
        Self {
            domain,
            origin,
            name: name.into(),
            digest,
            fingerprint,
        }
    }

    #[must_use]
    pub const fn domain(&self) -> IdentityDomain {
        self.domain
    }

    #[must_use]
    pub const fn origin(&self) -> IdentityOrigin {
        self.origin
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn digest(&self) -> u128 {
        self.digest
    }

    #[must_use]
    pub const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }

    pub(crate) fn replace_fingerprint(&mut self, fingerprint: u128) {
        self.fingerprint = fingerprint;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedCompilation {
    diagnostics: Arc<[Diagnostic]>,
    inspection: Inspection,
    completed_semantic_program: Arc<crate::completed_semantic::CompletedSemanticProgram>,
    planning_foundation: Option<Arc<VerifiedPlanningFoundation>>,
}

impl AcceptedCompilation {
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn inspection(&self) -> &Inspection {
        &self.inspection
    }

    #[must_use]
    pub fn semantic_program_fingerprint(&self) -> u128 {
        self.completed_semantic_program.fingerprint()
    }

    #[must_use]
    pub fn planning_foundation_fingerprint(&self) -> Option<u128> {
        self.planning_foundation
            .as_ref()
            .map(|planning| planning.fingerprint())
    }

    #[allow(dead_code)]
    pub(crate) fn completed_semantic_program(
        &self,
    ) -> &crate::completed_semantic::CompletedSemanticProgram {
        &self.completed_semantic_program
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedCompilation {
    diagnostics: Arc<[Diagnostic]>,
    inspection: Inspection,
}

impl RejectedCompilation {
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn inspection(&self) -> &Inspection {
        &self.inspection
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostFailure {
    operation: Arc<str>,
}

impl HostFailure {
    #[must_use]
    pub fn operation(&self) -> &str {
        &self.operation
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Defect {
    phase: Arc<str>,
    evidence: Arc<str>,
}

impl Defect {
    pub(crate) fn new(phase: impl Into<Arc<str>>, evidence: impl Into<Arc<str>>) -> Self {
        Self {
            phase: phase.into(),
            evidence: evidence.into(),
        }
    }

    #[must_use]
    pub fn phase(&self) -> &str {
        &self.phase
    }

    #[must_use]
    pub fn evidence(&self) -> &str {
        &self.evidence
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompilationOutcome {
    Accepted(AcceptedCompilation),
    Rejected(RejectedCompilation),
    Cancelled,
    HostFailed(HostFailure),
    Defect(Defect),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenError {
    DuplicateAuthenticatedModule {
        path: Arc<str>,
    },
    InvalidAuthenticatedModulePath {
        path: Arc<str>,
    },
    MalformedAuthenticatedModule {
        path: Arc<str>,
        code: Arc<str>,
    },
    InvalidAuthenticatedModule {
        path: Arc<str>,
        code: Arc<str>,
    },
    AuthenticatedIdentityCollision {
        digest: u128,
    },
    AuthenticatedModuleDefect {
        phase: Arc<str>,
        evidence: Arc<str>,
    },
    MissingAuthenticatedDependency {
        path: Arc<str>,
        dependency: Arc<str>,
    },
    AuthenticatedImportCycle {
        path: Arc<str>,
    },
}

#[derive(Debug)]
pub struct Compiler {
    distribution: Arc<CompilerDistribution>,
    poisoned: AtomicBool,
}

impl Compiler {
    pub fn open(installation: CompilerInstallation) -> Result<Self, OpenError> {
        let distribution = CompilerDistribution::seal(installation)?;
        Ok(Self {
            distribution: Arc::new(distribution),
            poisoned: AtomicBool::new(false),
        })
    }

    #[must_use]
    pub fn compile(
        &self,
        request: CompilationRequest,
        cancellation: &Cancellation,
    ) -> CompilationOutcome {
        if self.poisoned.load(Ordering::Acquire) {
            return CompilationOutcome::Defect(Defect {
                phase: Arc::from("compiler"),
                evidence: Arc::from("compiler instance is poisoned"),
            });
        }
        if cancellation.is_cancelled() {
            return CompilationOutcome::Cancelled;
        }
        self.contain(|| self.compile_inner(&request, cancellation))
    }

    fn contain(&self, operation: impl FnOnce() -> CompilationOutcome) -> CompilationOutcome {
        match catch_unwind(AssertUnwindSafe(operation)) {
            Ok(outcome) => outcome,
            Err(_) => {
                self.poisoned.store(true, Ordering::Release);
                CompilationOutcome::Defect(Defect {
                    phase: Arc::from("host containment"),
                    evidence: Arc::from("unexpected panic; compiler instance poisoned"),
                })
            }
        }
    }

    fn compile_inner(
        &self,
        request: &CompilationRequest,
        cancellation: &Cancellation,
    ) -> CompilationOutcome {
        let mut files = BTreeMap::new();
        let mut diagnostics = Vec::new();
        let mut project_files = request.project.files.iter().collect::<Vec<_>>();
        project_files.sort_by(|left, right| {
            left.path()
                .cmp(right.path())
                .then(left.bytes().cmp(right.bytes()))
        });
        for file in project_files {
            if cancellation.is_cancelled() {
                return CompilationOutcome::Cancelled;
            }
            if files.contains_key(file.path()) {
                diagnostics.push(Diagnostic::new(
                    "project.duplicate_path",
                    SourceRange::new_shared(file.path_arc(), 0, 0),
                    RecoveryAction::None,
                ));
            } else {
                files.insert(file.path(), file);
            }
            if !crate::project_closure::valid_module_path(
                file.path(),
                crate::project_closure::ModuleOrigin::Project,
            ) {
                diagnostics.push(Diagnostic::new(
                    "project.invalid_module_path",
                    SourceRange::new_shared(file.path_arc(), 0, 0),
                    RecoveryAction::None,
                ));
            }
        }
        let mut authenticated_paths = BTreeSet::new();
        for module in self.distribution.modules() {
            if files.contains_key(module.path()) {
                diagnostics.push(Diagnostic::new(
                    "project.authenticated_module_shadow",
                    SourceRange::new(module.path(), 0, 0),
                    RecoveryAction::None,
                ));
            } else {
                files.insert(module.path(), module);
                authenticated_paths.insert(module.path());
            }
        }

        let Some(_root) = files.get(request.root.path()).copied() else {
            diagnostics.push(Diagnostic::new(
                "project.missing_root",
                SourceRange::new(request.root.path(), 0, 0),
                RecoveryAction::None,
            ));
            // Inspection is a projection of completed compiler work.  Always perform the
            // same parse work so changing the projection cannot change cancellation or
            // diagnostics.
            let Some(unreachable_syntax) = parse_unreachable_project_syntax(
                &request.project,
                &BTreeMap::new(),
                &files,
                cancellation,
            ) else {
                return CompilationOutcome::Cancelled;
            };
            let syntax = if request.inspection.syntax {
                unreachable_syntax.into()
            } else {
                Arc::from([])
            };
            return CompilationOutcome::Rejected(RejectedCompilation {
                diagnostics: diagnostics.into(),
                inspection: Inspection {
                    distribution_version: Arc::from(self.distribution.version()),
                    distribution_digest: self.distribution.digest(),
                    project_revision: Arc::clone(&request.project.revision),
                    snapshot_digest: request.project.digest,
                    syntax,
                    ..Inspection::default()
                },
            });
        };

        let mut pending = BTreeSet::from([request.root.path().to_owned()]);
        let mut parsed_sources = BTreeMap::new();
        while let Some(path) = pending.pop_first() {
            if parsed_sources.contains_key(path.as_str()) {
                continue;
            }
            let Some(file) = files.get(path.as_str()).copied() else {
                continue;
            };
            let parsed = syntax::parse(file, cancellation);
            if parsed.cancelled {
                return CompilationOutcome::Cancelled;
            }
            for import in &parsed.imports {
                if authenticated_paths.contains(path.as_str())
                    && !authenticated_paths.contains(import.target_path.as_str())
                    && files.contains_key(import.target_path.as_str())
                {
                    diagnostics.push(Diagnostic::new(
                        "project.authenticated_imports_project_module",
                        import.range.clone(),
                        RecoveryAction::None,
                    ));
                } else if files.contains_key(import.target_path.as_str()) {
                    pending.insert(import.target_path.clone());
                } else {
                    diagnostics.push(
                        Diagnostic::new(
                            "project.missing_module",
                            import.range.clone(),
                            RecoveryAction::None,
                        )
                        .with_parameter("module_path", import.target_path.clone()),
                    );
                }
            }
            diagnostics.extend(parsed.diagnostics.iter().cloned());
            parsed_sources.insert(path, parsed);
        }
        if let Some(cycle) = crate::project_closure::first_import_cycle(&parsed_sources) {
            diagnostics.push(Diagnostic::new(
                "project.import_cycle",
                cycle.range,
                RecoveryAction::None,
            ));
        }
        diagnostics.sort_by(|left, right| {
            left.primary
                .path
                .cmp(&right.primary.path)
                .then(left.primary.start.cmp(&right.primary.start))
                .then(left.code.cmp(&right.code))
        });
        if diagnostics.is_empty() {
            match semantic::select_comptime_declarations(
                &mut parsed_sources,
                &files,
                &authenticated_paths,
                request.root,
                cancellation,
                self.distribution.build_authority(),
                self.distribution.pool_authority(),
            ) {
                Ok(()) => {}
                Err(semantic::SelectionFailure::Diagnostic(diagnostic)) => {
                    diagnostics.push(diagnostic);
                }
                Err(semantic::SelectionFailure::Defect(defect)) => {
                    return CompilationOutcome::Defect(defect);
                }
                Err(semantic::SelectionFailure::Cancelled) => {
                    return CompilationOutcome::Cancelled;
                }
            }
        }
        let semantic_closure_digest = crate::project_closure::digest(
            &parsed_sources,
            &files,
            &authenticated_paths,
            self.distribution.digest(),
        );

        let mut identity_catalog = match identity::catalog(
            &parsed_sources,
            &files,
            &authenticated_paths,
            cancellation,
        ) {
            Ok(identities) => identities,
            Err(IdentityFailure::Cancelled) => return CompilationOutcome::Cancelled,
            Err(IdentityFailure::Collision(IdentityCollision {
                digest,
                first_key,
                second_key,
            })) => {
                return CompilationOutcome::Defect(Defect {
                    phase: Arc::from("identity catalog"),
                    evidence: format!(
                        "XXH3-128 collision {digest:032x} between {first_key:02x?} and {second_key:02x?}"
                    )
                    .into(),
                });
            }
        };
        let front_end_clean = diagnostics.is_empty();
        let revision = semantic::analyze(
            &parsed_sources,
            &files,
            &mut identity_catalog,
            request.root,
            cancellation,
            front_end_clean,
            semantic::AnalysisContext::new(
                AuthorityContext::new(
                    self.distribution.build_authority(),
                    self.distribution.pool_authority(),
                ),
                Some(crate::completed_semantic::ContextInput {
                    distribution_digest: self.distribution.digest(),
                    semantic_closure_digest,
                    root: request.root,
                }),
            ),
        );
        let finalized = match revision.finalize(
            request.inspection.semantics,
            request.inspection.evaluation,
            request.inspection.construction,
            request.inspection.tests,
        ) {
            Ok(revision) => revision,
            Err(semantic::SemanticFailure::Cancelled) => return CompilationOutcome::Cancelled,
            Err(semantic::SemanticFailure::Defect(defect)) => {
                return CompilationOutcome::Defect(defect);
            }
        };
        diagnostics.extend(finalized.diagnostics);
        let semantic_projection = finalized.projection;
        let completed_semantic_program = finalized.completed;
        let architecture_contract = if diagnostics.is_empty() {
            match request.architecture_profile {
                None => None,
                Some(profile) => match self
                    .distribution
                    .architecture_planning()
                    .authenticate(profile, cancellation)
                {
                    Ok(contract) => Some(Arc::new(contract)),
                    Err(failure) if failure.kind() == ContractFailureKind::Cancelled => {
                        return CompilationOutcome::Cancelled;
                    }
                    Err(failure) if failure.kind() == ContractFailureKind::UnsupportedProfile => {
                        diagnostics
                            .push(unsupported_architecture_diagnostic(request.root, profile));
                        None
                    }
                    Err(failure) => {
                        return CompilationOutcome::Defect(Defect::new(
                            "architecture planning contract",
                            failure.bounded_evidence(),
                        ));
                    }
                },
            }
        } else {
            None
        };
        diagnostics.sort_by(|left, right| {
            left.primary
                .path
                .cmp(&right.primary.path)
                .then(left.primary.start.cmp(&right.primary.start))
                .then(left.code.cmp(&right.code))
        });

        let planning_foundation = if diagnostics.is_empty() {
            match (
                completed_semantic_program.as_ref(),
                architecture_contract.as_ref(),
            ) {
                (Some(semantic_program), Some(architecture_contract)) => {
                    match self.distribution.image_planning().plan(
                        Arc::clone(semantic_program),
                        Arc::clone(architecture_contract),
                        cancellation,
                    ) {
                        Ok(planning) => Some(Arc::new(planning)),
                        Err(PlanningFailure::Cancelled) => return CompilationOutcome::Cancelled,
                        Err(PlanningFailure::Defect(evidence)) => {
                            return CompilationOutcome::Defect(Defect::new(
                                "Image Planning Foundation",
                                evidence,
                            ));
                        }
                    }
                }
                _ => None,
            }
        } else {
            None
        };

        let Some(unreachable_project_syntax) = parse_unreachable_project_syntax(
            &request.project,
            &parsed_sources,
            &files,
            cancellation,
        ) else {
            return CompilationOutcome::Cancelled;
        };

        let inspection = Inspection {
            distribution_version: Arc::from(self.distribution.version()),
            distribution_digest: self.distribution.digest(),
            project_revision: Arc::clone(&request.project.revision),
            snapshot_digest: request.project.digest,
            semantic_closure_digest: Some(semantic_closure_digest),
            syntax: if request.inspection.syntax {
                let mut syntax = parsed_sources
                    .iter()
                    .map(|(path, parsed)| {
                        SyntaxObservation::new(
                            files[path.as_str()],
                            parsed.elements.clone(),
                            parsed.node_observations(),
                        )
                    })
                    .chain(unreachable_project_syntax)
                    .collect::<Vec<_>>();
                syntax.sort_by(|left, right| {
                    left.path()
                        .cmp(right.path())
                        .then(left.source_bytes().cmp(right.source_bytes()))
                });
                syntax.into()
            } else {
                Arc::from([])
            },
            closure: if request.inspection.closure {
                parsed_sources
                    .keys()
                    .map(|path| Arc::<str>::from(path.as_str()))
                    .collect::<Vec<_>>()
                    .into()
            } else {
                Arc::from([])
            },
            identities: if request.inspection.identities {
                identity_catalog.project_observations().into()
            } else {
                Arc::from([])
            },
            resolutions: semantic_projection.resolutions,
            function_facts: semantic_projection.function_facts,
            types: semantic_projection.types,
            ownership: semantic_projection.ownership,
            specializations: semantic_projection.specializations,
            inferred_errors: semantic_projection.inferred_errors,
            evaluations: semantic_projection.evaluations,
            constructions: semantic_projection.constructions,
            test_plan: semantic_projection.test_plan,
            architecture_planning_contract: architecture_observation(
                architecture_contract.as_deref(),
                request.inspection,
            ),
            completed_semantic_program: if diagnostics.is_empty() && request.inspection.semantics {
                completed_semantic_program
                    .as_ref()
                    .map(|completed| completed.observation())
            } else {
                None
            },
            planning_foundation: if request.inspection.planning {
                planning_foundation
                    .as_ref()
                    .map(|planning| planning.observation())
            } else {
                None
            },
        };

        if diagnostics.is_empty() {
            let Some(completed_semantic_program) = completed_semantic_program else {
                return CompilationOutcome::Defect(Defect::new(
                    "semantic completion",
                    "accepted compilation omitted Completed Semantic Program",
                ));
            };
            CompilationOutcome::Accepted(AcceptedCompilation {
                diagnostics: Arc::from([]),
                inspection,
                completed_semantic_program,
                planning_foundation,
            })
        } else {
            CompilationOutcome::Rejected(RejectedCompilation {
                diagnostics: diagnostics.into(),
                inspection,
            })
        }
    }
}

fn architecture_observation(
    contract: Option<&VerifiedArchitecturePlanningContract>,
    inspection: InspectSelection,
) -> Option<ArchitecturePlanningObservation> {
    inspection
        .architecture_planning
        .then(|| contract.map(VerifiedArchitecturePlanningContract::observation))
        .flatten()
}

fn unsupported_architecture_diagnostic(root: Root, profile: ArchitectureProfile) -> Diagnostic {
    Diagnostic::new(
        "architecture.unsupported_profile",
        SourceRange::new(root.path(), 0, 0),
        RecoveryAction::None,
    )
    .with_parameter("profile", profile.canonical_name())
}

fn parse_unreachable_project_syntax(
    project: &ProjectSnapshot,
    parsed_sources: &BTreeMap<String, syntax::ParsedSource>,
    selected_files: &BTreeMap<&str, &ProjectFile>,
    cancellation: &Cancellation,
) -> Option<Vec<SyntaxObservation>> {
    let mut project_files = project.files.iter().collect::<Vec<_>>();
    project_files.sort_by(|left, right| {
        left.path()
            .cmp(right.path())
            .then(left.bytes().cmp(right.bytes()))
    });

    let mut observations = Vec::new();
    for file in project_files {
        let is_reachable_file = parsed_sources.contains_key(file.path())
            && selected_files
                .get(file.path())
                .is_some_and(|selected| std::ptr::eq(*selected, file));
        if is_reachable_file {
            continue;
        }
        let parsed = syntax::parse(file, cancellation);
        if parsed.cancelled {
            return None;
        }
        let nodes = parsed.node_observations();
        observations.push(SyntaxObservation::new(file, parsed.elements, nodes));
    }
    Some(observations)
}

fn project_snapshot_digest(files: &[ProjectFile]) -> u128 {
    let mut files = files.iter().collect::<Vec<_>>();
    files.sort_by(|left, right| {
        left.path()
            .cmp(right.path())
            .then(left.bytes().cmp(right.bytes()))
    });
    let mut hasher = Xxh3::new();
    hasher.update(b"wrela.project-snapshot\0\x01");
    for file in files {
        hash_digest_part(&mut hasher, file.path().as_bytes());
        hash_digest_part(&mut hasher, file.bytes());
    }
    hasher.digest128()
}

fn hash_digest_part(hasher: &mut Xxh3, bytes: &[u8]) {
    hasher.update(&u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unexpected_panics_poison_the_compiler_instance() {
        let compiler = Compiler::open(CompilerInstallation::layer1()).expect("installation opens");
        assert!(matches!(
            compiler.contain(|| panic!("injected private invariant failure")),
            CompilationOutcome::Defect(_)
        ));
        assert!(matches!(
            compiler.compile(
                CompilationRequest::new(ProjectSnapshot::default(), Root::Image),
                &Cancellation::new(),
            ),
            CompilationOutcome::Defect(defect)
                if defect.evidence() == "compiler instance is poisoned"
        ));
    }
}
