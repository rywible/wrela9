use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::syntax;
use crate::{identity, identity::IdentityCollision, semantic, typed_hir::BuildAuthority};

#[derive(Clone, Debug)]
pub struct CompilerInstallation {
    authenticated_modules: Arc<[ProjectFile]>,
    build_authority: BuildAuthority,
}

impl Default for CompilerInstallation {
    fn default() -> Self {
        Self {
            authenticated_modules: Arc::from([]),
            build_authority: BuildAuthority::compiler_distribution(),
        }
    }
}

impl CompilerInstallation {
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_authenticated_modules(modules: Vec<ProjectFile>) -> Self {
        Self {
            authenticated_modules: modules.into(),
            build_authority: BuildAuthority::compiler_distribution(),
        }
    }
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

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectSnapshot {
    files: Arc<[ProjectFile]>,
}

impl ProjectSnapshot {
    #[must_use]
    pub fn new(files: Vec<ProjectFile>) -> Self {
        Self {
            files: files.into(),
        }
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
    pub const fn all() -> Self {
        Self {
            syntax: true,
            closure: true,
            identities: true,
            semantics: true,
            evaluation: true,
            construction: true,
            tests: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CompilationRequest {
    project: ProjectSnapshot,
    root: Root,
    inspection: InspectSelection,
}

impl CompilationRequest {
    #[must_use]
    pub fn new(project: ProjectSnapshot, root: Root) -> Self {
        Self {
            project,
            root,
            inspection: InspectSelection::none(),
        }
    }

    #[must_use]
    pub fn with_inspection(mut self, inspection: InspectSelection) -> Self {
        self.inspection = inspection;
        self
    }
}

#[derive(Clone, Debug, Default)]
pub struct Cancellation {
    cancelled: Arc<AtomicBool>,
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
        self.cancelled.load(Ordering::Acquire)
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

    #[must_use]
    pub fn path(&self) -> &str {
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
    role: Arc<str>,
}

impl DiagnosticLabel {
    #[must_use]
    pub fn range(&self) -> &SourceRange {
        &self.range
    }

    #[must_use]
    pub fn role(&self) -> &str {
        &self.role
    }
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
    parameters: Arc<[(Arc<str>, Arc<str>)]>,
    recovery: RecoveryAction,
}

impl Diagnostic {
    pub(crate) fn new(code: &'static str, primary: SourceRange, recovery: RecoveryAction) -> Self {
        Self {
            code: Arc::from(code),
            primary,
            labels: Arc::from([]),
            parameters: Arc::from([]),
            recovery,
        }
    }

    pub(crate) fn with_parameter(mut self, name: &'static str, value: impl Into<Arc<str>>) -> Self {
        self.parameters = Arc::from([(Arc::from(name), value.into())]);
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
    pub fn parameters(&self) -> &[(Arc<str>, Arc<str>)] {
        &self.parameters
    }

    #[must_use]
    pub fn recovery(&self) -> &RecoveryAction {
        &self.recovery
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyntaxElementKind {
    Token,
    Trivia,
    Invalid,
    Layout,
    Missing,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxElement {
    kind: SyntaxElementKind,
    name: Arc<str>,
    range: SourceRange,
}

impl SyntaxElement {
    pub(crate) fn new(
        kind: SyntaxElementKind,
        name: &'static str,
        path: &str,
        start: usize,
        end: usize,
    ) -> Self {
        Self {
            kind,
            name: Arc::from(name),
            range: SourceRange::new(path, start, end),
        }
    }

    #[must_use]
    pub fn kind(&self) -> &SyntaxElementKind {
        &self.kind
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn range(&self) -> &SourceRange {
        &self.range
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
    kind: Arc<str>,
    range: SourceRange,
    depth: u16,
}

impl SyntaxNodeObservation {
    pub(crate) fn new(kind: impl Into<Arc<str>>, range: SourceRange, depth: u16) -> Self {
        Self {
            kind: kind.into(),
            range,
            depth,
        }
    }

    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
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
    syntax: Arc<[SyntaxObservation]>,
    closure: Arc<[Arc<str>]>,
    identities: Arc<[IdentityObservation]>,
    function_facts: Arc<[FunctionFactsObservation]>,
    inferred_errors: Arc<[InferredErrorObservation]>,
    evaluations: Arc<[EvaluationObservation]>,
    constructions: Arc<[ConstructionObservation]>,
    test_plan: Arc<[TestApplicationObservation]>,
}

impl Inspection {
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
    pub fn function_facts(&self) -> &[FunctionFactsObservation] {
        &self.function_facts
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestApplicationObservation {
    suite: Arc<str>,
    test: Arc<str>,
    order: u32,
}

impl TestApplicationObservation {
    pub(crate) fn new(suite: impl Into<Arc<str>>, test: impl Into<Arc<str>>, order: u32) -> Self {
        Self {
            suite: suite.into(),
            test: test.into(),
            order,
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
    Tuple(Arc<[CanonicalValue]>),
    Array(Arc<[CanonicalValue]>),
    Variant {
        type_name: Arc<str>,
        variant: Arc<str>,
        payload: Arc<[CanonicalValue]>,
    },
    SymbolicHandle {
        kind: Arc<str>,
        identity: u128,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvaluationOutcome {
    Completed(CanonicalValue),
    CreatorRejected {
        kind: Arc<str>,
    },
    Panicked {
        kind: Arc<str>,
        site: SourceRange,
    },
    LimitExceeded {
        policy: Arc<str>,
        ceiling: u64,
        used: u64,
    },
    Cancelled,
    Defect {
        evidence: Arc<str>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvaluationReceipt {
    tariff_schema: Arc<str>,
    typed_hir_fingerprint: u128,
    fuel_used: u64,
    peak_memory: u64,
}

impl EvaluationReceipt {
    pub(crate) fn new(typed_hir_fingerprint: u128, fuel_used: u64, peak_memory: u64) -> Self {
        Self {
            tariff_schema: Arc::from("wrela.evaluator.tariff.v1"),
            typed_hir_fingerprint,
            fuel_used,
            peak_memory,
        }
    }

    #[must_use]
    pub fn tariff_schema(&self) -> &str {
        &self.tariff_schema
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
    name: Arc<str>,
    pure: bool,
    may_panic: bool,
    suspends: bool,
    evaluator_eligible: bool,
}

impl FunctionFactsObservation {
    pub(crate) fn new(
        name: impl Into<Arc<str>>,
        pure: bool,
        may_panic: bool,
        suspends: bool,
        evaluator_eligible: bool,
    ) -> Self {
        Self {
            name: name.into(),
            pure,
            may_panic,
            suspends,
            evaluator_eligible,
        }
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InferredErrorObservation {
    function: Arc<str>,
    error_type: Arc<str>,
}

impl InferredErrorObservation {
    pub(crate) fn new(function: impl Into<Arc<str>>, error_type: impl Into<Arc<str>>) -> Self {
        Self {
            function: function.into(),
            error_type: error_type.into(),
        }
    }

    #[must_use]
    pub fn function(&self) -> &str {
        &self.function
    }

    #[must_use]
    pub fn error_type(&self) -> &str {
        &self.error_type
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstructionObservation {
    identity: u128,
    kind: Arc<str>,
    site: SourceRange,
}

impl ConstructionObservation {
    pub(crate) fn new(identity: u128, kind: impl Into<Arc<str>>, site: SourceRange) -> Self {
        Self {
            identity,
            kind: kind.into(),
            site,
        }
    }

    #[must_use]
    pub const fn identity(&self) -> u128 {
        self.identity
    }

    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    #[must_use]
    pub const fn site(&self) -> &SourceRange {
        &self.site
    }
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
    canonical_key: Arc<[u8]>,
    digest: u128,
    fingerprint: u128,
}

impl IdentityObservation {
    pub(crate) fn new(
        domain: IdentityDomain,
        origin: IdentityOrigin,
        name: impl Into<Arc<str>>,
        canonical_key: Arc<[u8]>,
        digest: u128,
        fingerprint: u128,
    ) -> Self {
        Self {
            domain,
            origin,
            name: name.into(),
            canonical_key,
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

    pub(crate) fn canonical_key_bytes(&self) -> &[u8] {
        &self.canonical_key
    }

    #[must_use]
    pub const fn digest(&self) -> u128 {
        self.digest
    }

    #[must_use]
    pub const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedCompilation {
    diagnostics: Arc<[Diagnostic]>,
    inspection: Inspection,
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
    DuplicateAuthenticatedModule { path: Arc<str> },
    InvalidAuthenticatedModulePath { path: Arc<str> },
}

#[derive(Debug)]
pub struct Compiler {
    installation: Arc<CompilerInstallation>,
    poisoned: AtomicBool,
}

impl Compiler {
    pub fn open(installation: CompilerInstallation) -> Result<Self, OpenError> {
        let mut paths = BTreeSet::new();
        for module in &*installation.authenticated_modules {
            if !valid_module_path(module.path(), false) {
                return Err(OpenError::InvalidAuthenticatedModulePath {
                    path: Arc::clone(&module.path),
                });
            }
            if !paths.insert(Arc::clone(&module.path)) {
                return Err(OpenError::DuplicateAuthenticatedModule {
                    path: Arc::clone(&module.path),
                });
            }
        }
        Ok(Self {
            installation: Arc::new(installation),
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
        let _sealed_distribution = &self.installation;
        let mut files = BTreeMap::new();
        let mut diagnostics = Vec::new();
        for file in &*request.project.files {
            if cancellation.is_cancelled() {
                return CompilationOutcome::Cancelled;
            }
            if files.insert(file.path(), file).is_some() {
                diagnostics.push(Diagnostic::new(
                    "project.duplicate_path",
                    SourceRange::new(file.path(), 0, 0),
                    RecoveryAction::None,
                ));
            }
            if !valid_module_path(file.path(), true) {
                diagnostics.push(Diagnostic::new(
                    "project.invalid_module_path",
                    SourceRange::new(file.path(), 0, 0),
                    RecoveryAction::None,
                ));
            }
        }
        let mut authenticated_paths = BTreeSet::new();
        for module in &*self.installation.authenticated_modules {
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
            return CompilationOutcome::Rejected(RejectedCompilation {
                diagnostics: diagnostics.into(),
                inspection: Inspection::default(),
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
                if matches!(import.target_path.as_str(), "src/image.wr" | "src/test.wr") {
                    diagnostics.push(Diagnostic::new(
                        "project.root_not_importable",
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
        if let Some(range) = import_cycle(&parsed_sources) {
            diagnostics.push(Diagnostic::new(
                "project.import_cycle",
                range,
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

        let identities = match identity::catalog(&parsed_sources, &files, &authenticated_paths) {
            Ok(identities) => identities,
            Err(IdentityCollision {
                digest,
                first_key,
                second_key,
            }) => {
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
        let analysis = semantic::analyze(
            &parsed_sources,
            &files,
            request.root,
            cancellation,
            front_end_clean,
            &self.installation.build_authority,
        );
        if analysis.cancelled {
            return CompilationOutcome::Cancelled;
        }
        if let Some(defect) = analysis.defect.clone() {
            return CompilationOutcome::Defect(defect);
        }
        diagnostics.extend(analysis.diagnostics.iter().cloned());
        diagnostics.sort_by(|left, right| {
            left.primary
                .path
                .cmp(&right.primary.path)
                .then(left.primary.start.cmp(&right.primary.start))
                .then(left.code.cmp(&right.code))
        });

        let inspection = Inspection {
            syntax: if request.inspection.syntax {
                parsed_sources
                    .iter()
                    .map(|(path, parsed)| {
                        SyntaxObservation::new(
                            files[path.as_str()],
                            parsed.elements.clone(),
                            parsed.node_observations(),
                        )
                    })
                    .collect::<Vec<_>>()
                    .into()
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
                identities.into()
            } else {
                Arc::from([])
            },
            function_facts: if request.inspection.semantics {
                analysis.function_facts.into()
            } else {
                Arc::from([])
            },
            inferred_errors: if request.inspection.semantics {
                analysis.inferred_errors.into()
            } else {
                Arc::from([])
            },
            evaluations: if request.inspection.evaluation {
                analysis.evaluations.into()
            } else {
                Arc::from([])
            },
            constructions: if request.inspection.construction {
                analysis.constructions.into()
            } else {
                Arc::from([])
            },
            test_plan: if request.inspection.tests {
                analysis.test_plan.into()
            } else {
                Arc::from([])
            },
        };

        if diagnostics.is_empty() {
            CompilationOutcome::Accepted(AcceptedCompilation {
                diagnostics: Arc::from([]),
                inspection,
            })
        } else {
            CompilationOutcome::Rejected(RejectedCompilation {
                diagnostics: diagnostics.into(),
                inspection,
            })
        }
    }
}

fn import_cycle(parsed_sources: &BTreeMap<String, syntax::ParsedSource>) -> Option<SourceRange> {
    fn visit(
        path: &str,
        parsed_sources: &BTreeMap<String, syntax::ParsedSource>,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) -> Option<SourceRange> {
        if visited.contains(path) {
            return None;
        }
        visiting.insert(path.to_owned());
        let parsed = &parsed_sources[path];
        let mut imports = parsed.imports.iter().collect::<Vec<_>>();
        imports.sort_by(|left, right| left.target_path.cmp(&right.target_path));
        for import in imports {
            if !parsed_sources.contains_key(import.target_path.as_str()) {
                continue;
            }
            if visiting.contains(&import.target_path) {
                return Some(import.range.clone());
            }
            if let Some(range) = visit(&import.target_path, parsed_sources, visiting, visited) {
                return Some(range);
            }
        }
        visiting.remove(path);
        visited.insert(path.to_owned());
        None
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for path in parsed_sources.keys() {
        if let Some(range) = visit(path, parsed_sources, &mut visiting, &mut visited) {
            return Some(range);
        }
    }
    None
}

fn valid_module_path(path: &str, project: bool) -> bool {
    let Some(relative) = path.strip_prefix("src/") else {
        return false;
    };
    let Some(stem) = relative.strip_suffix(".wr") else {
        return false;
    };
    let mut segments = stem.split('/');
    let first = segments.next().unwrap_or_default();
    let rest: Vec<_> = segments.collect();
    if rest.is_empty() {
        if project {
            if !matches!(first, "image" | "test") {
                return false;
            }
        } else {
            return false;
        }
    }
    [first].into_iter().chain(rest).all(valid_path_segment)
}

fn valid_path_segment(segment: &str) -> bool {
    let mut bytes = segment.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unexpected_panics_poison_the_compiler_instance() {
        let compiler = Compiler::open(CompilerInstallation::empty()).expect("installation opens");
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
