use wrela_compiler::{
    Cancellation, CompilationOutcome, CompilationRequest, Compiler, CompilerInstallation,
    InspectSelection, ProjectFile, ProjectSnapshot, Root,
};

fn compile(files: Vec<ProjectFile>, root: Root) -> CompilationOutcome {
    Compiler::open(CompilerInstallation::empty())
        .expect("distribution opens")
        .compile(
            CompilationRequest::new(ProjectSnapshot::new(files), root)
                .with_inspection(InspectSelection::all()),
            &Cancellation::new(),
        )
}

#[test]
fn import_cycles_are_rejected_with_canonical_evidence() {
    let files = vec![
        ProjectFile::new(
            "src/image.wr",
            b"from game import first\n\n@image\nfn build() -> Image:\n    return Image.new()\n",
        ),
        ProjectFile::new("src/game/first.wr", b"from game import second\n"),
        ProjectFile::new("src/game/second.wr", b"from game import first\n"),
    ];
    let CompilationOutcome::Rejected(rejected) = compile(files, Root::Image) else {
        panic!("cycle must reject");
    };
    assert_eq!(
        rejected
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        ["project.import_cycle"]
    );
}

#[test]
fn test_image_plans_every_reachable_test_once_in_source_order() {
    let source = br#"pub suite arithmetic:
    test adds():
        expect 2 + 2 == 4

    test subtracts():
        expect 7 - 2 == 5

@image
fn build() -> Image:
    tests = Test.new(cases=[arithmetic.adds(), arithmetic.subtracts()])
    return Image.new(tests=tests)
"#;
    let outcome = compile(vec![ProjectFile::new("src/test.wr", source)], Root::Test);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("complete Test Image must compile: {outcome:#?}");
    };
    assert_eq!(
        accepted
            .inspection()
            .test_plan()
            .iter()
            .map(|application| (application.suite(), application.test(), application.order()))
            .collect::<Vec<_>>(),
        [("arithmetic", "adds", 0), ("arithmetic", "subtracts", 1)]
    );
}

#[test]
fn missing_and_duplicate_test_applications_are_rejected() {
    let source = br#"pub suite arithmetic:
    test adds():
        expect 2 + 2 == 4

    test subtracts():
        expect 7 - 2 == 5

@image
fn build() -> Image:
    tests = Test.new(cases=[arithmetic.adds(), arithmetic.adds()])
    return Image.new(tests=tests)
"#;
    let CompilationOutcome::Rejected(rejected) =
        compile(vec![ProjectFile::new("src/test.wr", source)], Root::Test)
    else {
        panic!("invalid Test plan must reject");
    };
    assert_eq!(
        rejected
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        ["test.duplicate_application", "test.missing_application",]
    );
}

#[test]
fn cancelled_request_publishes_no_partial_result() {
    let cancellation = Cancellation::new();
    cancellation.cancel();
    let outcome = Compiler::open(CompilerInstallation::empty())
        .expect("distribution opens")
        .compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new(
                    "src/image.wr",
                    b"@image\nfn build() -> Image:\n    return Image.new()\n",
                )]),
                Root::Image,
            ),
            &cancellation,
        );
    assert_eq!(outcome, CompilationOutcome::Cancelled);
}

#[test]
fn imported_private_declarations_do_not_enter_the_module_namespace() {
    let files = vec![
        ProjectFile::new(
            "src/image.wr",
            b"from game import cards\n\n@image\nfn build() -> Image:\n    return Image.new(answer=cards.secret())\n",
        ),
        ProjectFile::new(
            "src/game/cards.wr",
            b"fn secret() -> i64:\n    return 42\n",
        ),
    ];
    let CompilationOutcome::Rejected(rejected) = compile(files, Root::Image) else {
        panic!("private imported member must not resolve");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.invalid_typed_hir")
    );
}

#[test]
fn imports_never_leak_bare_members_into_local_scope() {
    let files = vec![
        ProjectFile::new(
            "src/image.wr",
            b"from game import cards\n\n@image\nfn build() -> Image:\n    return Image.new(answer=answer())\n",
        ),
        ProjectFile::new(
            "src/game/cards.wr",
            b"pub fn answer() -> i64:\n    return 42\n",
        ),
    ];
    let CompilationOutcome::Rejected(rejected) = compile(files, Root::Image) else {
        panic!("an imported public member requires its Module alias");
    };
    assert!(rejected.diagnostics().iter().any(|diagnostic| {
        diagnostic.code() == "semantic.invalid_typed_hir"
            && diagnostic
                .parameters()
                .iter()
                .any(|(name, value)| name.as_ref() == "kind" && value.as_ref() == "unresolved_call")
    }));
}

#[test]
fn test_parameters_establish_exclusive_binding_with_take() {
    let source = br#"pub suite counters:
    test increments(counter: Counter):
        expect true

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) =
        compile(vec![ProjectFile::new("src/test.wr", source)], Root::Test)
    else {
        panic!("shared test parameter must reject");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "test.parameter_requires_take")
    );
}

#[test]
fn authenticated_modules_resolve_from_the_sealed_distribution_and_cannot_be_shadowed() {
    let installation = CompilerInstallation::with_authenticated_modules(vec![ProjectFile::new(
        "src/core/images.wr",
        b"pub fn blank() -> Image:\n    return Image.new()\n",
    )]);
    let compiler = Compiler::open(installation).expect("distribution opens");
    let root = ProjectFile::new(
        "src/image.wr",
        b"from core import images\n\n@image\nfn build() -> Image:\n    return images.blank()\n",
    );
    let CompilationOutcome::Accepted(accepted) = compiler.compile(
        CompilationRequest::new(ProjectSnapshot::new(vec![root.clone()]), Root::Image)
            .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    ) else {
        panic!("authenticated Module must resolve");
    };
    assert!(accepted.inspection().identities().iter().any(|identity| {
        identity.origin() == wrela_compiler::IdentityOrigin::Authenticated
            && identity.name() == "core.images"
    }));

    let CompilationOutcome::Rejected(rejected) = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![
                root,
                ProjectFile::new(
                    "src/core/images.wr",
                    b"pub fn blank() -> Image:\n    return Image.new()\n",
                ),
            ]),
            Root::Image,
        )
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    ) else {
        panic!("Project cannot forge authenticated origin");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "project.authenticated_module_shadow")
    );
    assert!(rejected.inspection().identities().iter().all(|identity| {
        identity.name() != "core.images"
            || identity.origin() == wrela_compiler::IdentityOrigin::Project
    }));
}
