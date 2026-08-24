use wrela_compiler::{
    Cancellation, CanonicalValue, CompilationOutcome, CompilationRequest, Compiler,
    CompilerInstallation, InspectSelection, OpenError, ProjectFile, ProjectSnapshot, Root,
};

fn compile(files: Vec<ProjectFile>, root: Root) -> CompilationOutcome {
    Compiler::open(CompilerInstallation::layer1())
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
fn nested_comptime_statements_select_test_bodies_before_test_semantics() {
    let source = br#"const ENABLED: bool = false

pub suite behavior:
    test selected():
        comptime if ENABLED:
            expect missing()
        else:
            expect true

@image
fn build() -> Image:
    tests = Test.new(cases=[behavior.selected()])
    return Image.new(tests=tests)
"#;
    let outcome = compile(vec![ProjectFile::new("src/test.wr", source)], Root::Test);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("only the selected nested Test branch is semantic: {outcome:#?}");
    };
    assert_eq!(accepted.inspection().test_plan().len(), 1);
}

#[test]
fn suite_rejects_a_non_test_member_without_stalling() {
    let source = br#"pub suite behavior:
    fn helper():
        pass

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(vec![ProjectFile::new("src/test.wr", source)], Root::Test);
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("a Suite owns only nested Tests: {outcome:#?}");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "syntax.malformed_declaration")
    );
}

#[test]
fn test_bodies_are_resolved_and_type_checked_in_layer_one() {
    let source = br#"pub suite behavior:
    test invalid_expectation():
        expect 1

@image
fn build() -> Image:
    tests = Test.new(cases=[behavior.invalid_expectation()])
    return Image.new(tests=tests)
"#;
    let CompilationOutcome::Rejected(rejected) =
        compile(vec![ProjectFile::new("src/test.wr", source)], Root::Test)
    else {
        panic!("a Test body is semantic source even before native Test execution exists");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.expect_requires_bool")
    );
}

#[test]
fn generic_calls_demanded_only_by_test_bodies_are_materialized() {
    let source = br#"pure fn identity[T](value: T) -> T:
    return value

pub suite behavior:
    test specializes():
        value = identity(1)
        expect value == 1

@image
fn build() -> Image:
    tests = Test.new(cases=[behavior.specializes()])
    return Image.new(tests=tests)
"#;
    let outcome = compile(vec![ProjectFile::new("src/test.wr", source)], Root::Test);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("test-only generic demand must materialize: {outcome:#?}");
    };
    assert_eq!(
        accepted
            .inspection()
            .specializations()
            .iter()
            .filter(|specialization| specialization.function() == "identity")
            .count(),
        1
    );
}

#[test]
fn test_plan_preserves_application_order_in_the_image_constructor() {
    let source = br#"pub suite arithmetic:
    test adds():
        expect true

    test subtracts():
        expect true

@image
fn build() -> Image:
    tests = Test.new(cases=[arithmetic.subtracts(), arithmetic.adds()])
    return Image.new(tests=tests)
"#;
    let outcome = compile(vec![ProjectFile::new("src/test.wr", source)], Root::Test);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("reordered complete Test Image must compile: {outcome:#?}");
    };
    assert_eq!(
        accepted
            .inspection()
            .test_plan()
            .iter()
            .map(|application| (application.test(), application.order()))
            .collect::<Vec<_>>(),
        [("subtracts", 0), ("adds", 1)]
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
fn unrelated_test_calls_are_rejected_outside_the_test_facility_cases_list() {
    let source = br#"pub suite arithmetic:
    test adds():
        expect 2 + 2 == 4

@image
fn build() -> Image:
    ignored = arithmetic.adds()
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) =
        compile(vec![ProjectFile::new("src/test.wr", source)], Root::Test)
    else {
        panic!("a Test Application belongs inside Test.new cases");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == "semantic.test_application_outside_cases" })
    );
}

#[test]
fn test_applications_exist_only_inside_the_test_facility_cases_operand() {
    let source = br#"pub suite behavior:
    test works():
        expect true

@image
fn build() -> Image:
    tests = Test.new(cases=[behavior.works()])
    return Image.new(tests=tests, leaked=behavior.works())
"#;
    let CompilationOutcome::Rejected(rejected) =
        compile(vec![ProjectFile::new("src/test.wr", source)], Root::Test)
    else {
        panic!("a Test Application outside cases must reject");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == "semantic.test_application_outside_cases" })
    );
}

#[test]
fn tests_and_the_test_build_constructor_share_callable_validation() {
    for source in [
        br#"pub suite behavior:
    test consumes(take value: i64):
        expect value == 1

@image
fn build() -> Image:
    tests = Test.new(cases=[behavior.consumes()])
    return Image.new(tests=tests)
"#
        .as_slice(),
        br#"pub suite behavior:
    test works():
        expect true

@image
fn build() -> Image:
    tests = Test.new(items=[behavior.works()])
    return Image.new(tests=tests)
"#
        .as_slice(),
    ] {
        let CompilationOutcome::Rejected(rejected) =
            compile(vec![ProjectFile::new("src/test.wr", source)], Root::Test)
        else {
            panic!("invalid Test callable application must reject");
        };
        assert!(rejected.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic.code(),
            "semantic.argument_count" | "semantic.argument_label_mismatch"
        )));
    }
}

#[test]
fn cancelled_request_publishes_no_partial_result() {
    let cancellation = Cancellation::new();
    cancellation.cancel();
    let outcome = Compiler::open(CompilerInstallation::layer1())
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
fn a_cancelled_request_does_not_change_the_next_request() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let project = || {
        ProjectSnapshot::new(vec![ProjectFile::new(
            "src/image.wr",
            b"@image\nfn build() -> Image:\n    return Image.new()\n",
        )])
    };
    let cancellation = Cancellation::new();
    cancellation.cancel();
    assert_eq!(
        compiler.compile(
            CompilationRequest::new(project(), Root::Image),
            &cancellation,
        ),
        CompilationOutcome::Cancelled
    );
    assert!(matches!(
        compiler.compile(
            CompilationRequest::new(project(), Root::Image),
            &Cancellation::new(),
        ),
        CompilationOutcome::Accepted(_)
    ));
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
            .any(|diagnostic| diagnostic.code() == "semantic.unresolved_call")
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
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.unresolved_call")
    );
}

#[test]
fn duplicate_module_aliases_are_rejected_before_member_lookup() {
    let files = vec![
        ProjectFile::new(
            "src/image.wr",
            b"from game import cards as data\nfrom game import rules as data\n\n@image\nfn build() -> Image:\n    return Image.new()\n",
        ),
        ProjectFile::new("src/game/cards.wr", b"pub const COUNT: i64 = 1\n"),
        ProjectFile::new("src/game/rules.wr", b"pub const COUNT: i64 = 2\n"),
    ];
    let CompilationOutcome::Rejected(rejected) = compile(files, Root::Image) else {
        panic!("one imported Module alias cannot bind two namespaces");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.duplicate_import_alias")
    );
}

#[test]
fn imported_public_suites_expose_their_nested_tests_through_the_module_namespace() {
    let files = vec![
        ProjectFile::new(
            "src/test.wr",
            br#"from game import arithmetic

@image
fn build() -> Image:
    tests = Test.new(cases=[arithmetic.behavior.adds()])
    return Image.new(tests=tests)
"#,
        ),
        ProjectFile::new(
            "src/game/arithmetic.wr",
            br#"pub suite behavior:
    test adds():
        expect 2 + 2 == 4
"#,
        ),
    ];
    let outcome = compile(files, Root::Test);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("a reachable imported Test must be statically applicable: {outcome:#?}");
    };
    assert_eq!(accepted.inspection().test_plan().len(), 1);
    assert_eq!(accepted.inspection().test_plan()[0].suite(), "behavior");
    assert_eq!(accepted.inspection().test_plan()[0].test(), "adds");
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
fn unresolved_test_parameter_types_are_rejected_without_changing_arity() {
    let source = br#"pub suite values:
    test consumes(take value: MissingType):
        expect true

@image
fn build() -> Image:
    tests = Test.new(cases=[values.consumes(1)])
    return Image.new(tests=tests)
"#;
    let outcome = compile(vec![ProjectFile::new("src/test.wr", source)], Root::Test);
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!(
            "an unresolved Test parameter type cannot disappear from its signature: {outcome:#?}"
        );
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.unresolved_type")
    );
}

#[test]
fn test_plan_exposes_typed_exclusive_bindings() {
    let source = br#"pub suite values:
    test consumes(take value: i64):
        expect true

@image
fn build() -> Image:
    tests = Test.new(cases=[values.consumes(1)])
    return Image.new(tests=tests)
"#;
    let CompilationOutcome::Accepted(accepted) =
        compile(vec![ProjectFile::new("src/test.wr", source)], Root::Test)
    else {
        panic!("typed Test binding must compile");
    };
    let binding = &accepted.inspection().test_plan()[0].bindings()[0];
    assert_eq!(binding.name(), "value");
    assert_eq!(binding.type_name(), "i64");
    assert_eq!(binding.ownership(), wrela_compiler::OwnershipMode::Take);
    assert!(matches!(
        binding.value(),
        CanonicalValue::Integer { value, .. } if *value == 1
    ));
    assert!(accepted.inspection().ownership().iter().any(|ownership| {
        ownership.name() == "value" && ownership.mode() == wrela_compiler::OwnershipMode::Take
    }));
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

#[test]
fn authenticated_modules_cannot_import_project_modules() {
    let outcome = Compiler::open(CompilerInstallation::with_authenticated_modules(vec![
        ProjectFile::new(
            "src/core/images.wr",
            b"from game import helper\n\npub fn blank() -> Image:\n    return helper.make()\n",
        ),
    ]));
    assert!(matches!(
        outcome,
        Err(OpenError::MissingAuthenticatedDependency { path, dependency })
            if path.as_ref() == "src/core/images.wr"
                && dependency.as_ref() == "src/game/helper.wr"
    ));
}
