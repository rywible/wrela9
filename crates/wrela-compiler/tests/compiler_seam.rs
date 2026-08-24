use wrela_compiler::{
    Cancellation, CanonicalValue, CompilationOutcome, CompilationRequest, Compiler,
    CompilerInstallation, EvaluationOutcome, IdentityDomain, InspectSelection, OpenError,
    ProjectFile, ProjectSnapshot, ResolutionKind, Root, SyntaxElementKind, SyntaxNodeKind,
    SyntaxTokenKind,
};

#[test]
fn resolution_inspection_exposes_resolved_calls_and_references() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let source = br#"pure fn add(left: i64, right: i64) -> i64:
    return left + right

const ANSWER: i64 = add(40, 2)

@image
fn build() -> Image:
    return Image.new(answer=ANSWER)
"#;
    let request = CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
        Root::Image,
    )
    .with_inspection(InspectSelection::all());
    let CompilationOutcome::Accepted(accepted) = compiler.compile(request, &Cancellation::new())
    else {
        panic!("resolution inspection requires an accepted semantic revision");
    };

    let add = accepted
        .inspection()
        .identities()
        .iter()
        .find(|identity| {
            identity.domain() == IdentityDomain::Definition && identity.name() == "add"
        })
        .expect("add identity");
    let answer = accepted
        .inspection()
        .identities()
        .iter()
        .find(|identity| {
            identity.domain() == IdentityDomain::Definition && identity.name() == "ANSWER"
        })
        .expect("ANSWER identity");
    assert!(
        accepted
            .inspection()
            .resolutions()
            .iter()
            .any(|resolution| {
                resolution.kind() == ResolutionKind::Call
                    && resolution.target_domain() == IdentityDomain::Definition
                    && resolution.target_identity() == add.digest()
                    && &source
                        [resolution.range().start() as usize..resolution.range().end() as usize]
                        == b"add(40, 2)"
            })
    );
    assert!(
        accepted
            .inspection()
            .resolutions()
            .iter()
            .any(|resolution| {
                resolution.kind() == ResolutionKind::Reference
                    && resolution.target_domain() == IdentityDomain::Definition
                    && resolution.target_identity() == answer.digest()
                    && &source
                        [resolution.range().start() as usize..resolution.range().end() as usize]
                        == b"ANSWER"
            })
    );
}

#[test]
fn valid_image_is_accepted_without_losing_source_bytes() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let source = b"@image\nfn build() -> Image:\r\n    return Image.new()\n";
    let project = ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]);
    let request =
        CompilationRequest::new(project, Root::Image).with_inspection(InspectSelection::syntax());

    let CompilationOutcome::Accepted(accepted) = compiler.compile(request, &Cancellation::new())
    else {
        panic!("valid Image constructor must be accepted");
    };

    let syntax = accepted.inspection().syntax().expect("syntax requested");
    assert_eq!(syntax.len(), 1);
    assert_eq!(syntax[0].path(), "src/image.wr");
    assert_eq!(syntax[0].source_bytes(), source);
    assert_eq!(syntax[0].nodes()[0].kind(), SyntaxNodeKind::Source);
    assert!(
        syntax[0]
            .nodes()
            .iter()
            .any(|node| node.kind() == SyntaxNodeKind::Function && node.depth() == 1)
    );
    assert!(
        syntax[0]
            .nodes()
            .iter()
            .any(|node| node.kind() == SyntaxNodeKind::ReturnStatement && node.depth() >= 3)
    );
    assert!(
        syntax[0]
            .nodes()
            .iter()
            .any(|node| node.kind() == SyntaxNodeKind::CallExpression && node.depth() >= 4)
    );
    assert!(accepted.diagnostics().is_empty());
}

#[test]
fn an_empty_distribution_has_no_ambient_build_constructor_authority() {
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("empty distribution opens");
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new(
                "src/image.wr",
                b"@image\nfn build() -> Image:\n    return Image.new()\n",
            )]),
            Root::Image,
        ),
        &Cancellation::new(),
    );
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("an empty distribution cannot grant a compiler-owned constructor");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.unresolved_nominal_type")
    );
}

#[test]
fn layer_one_distribution_exposes_authenticated_build_constructors() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("Layer 1 opens");
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new(
                "src/image.wr",
                b"@image\nfn build() -> Image:\n    return Image.new()\n",
            )]),
            Root::Image,
        ),
        &Cancellation::new(),
    );
    assert!(matches!(outcome, CompilationOutcome::Accepted(_)));
}

#[test]
fn syntax_exposes_closed_exact_token_kinds_instead_of_broad_categories() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let source = b"@image\nfn build() -> Image:\n    return Image.new()\n";
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_inspection(InspectSelection::syntax()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("closed-token example must accept");
    };
    let kinds: Vec<_> = accepted.inspection().syntax().expect("syntax")[0]
        .elements()
        .iter()
        .map(|element| *element.kind())
        .collect();
    assert!(kinds.contains(&SyntaxElementKind::Token(SyntaxTokenKind::Fn)));
    assert!(kinds.contains(&SyntaxElementKind::Token(SyntaxTokenKind::Identifier)));
    assert!(kinds.contains(&SyntaxElementKind::Token(SyntaxTokenKind::LeftParen)));
    assert!(kinds.contains(&SyntaxElementKind::Token(SyntaxTokenKind::Arrow)));
    assert!(kinds.contains(&SyntaxElementKind::Token(SyntaxTokenKind::Return)));
}

#[test]
fn every_reserved_word_and_fixed_operator_has_a_closed_token_kind() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let source = b"and any as assert async await break case comptime const continue defer elif else enum expect false fn for from if implements import in interface is match mut not or own panic pass pool pub pure read resource return self send struct suite take test true try_send type while with & | ^ ~ << >> .. ..= ;";
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_inspection(InspectSelection::syntax()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("a reserved-word token corpus is intentionally not a declaration");
    };
    let tokens = rejected.inspection().syntax().expect("syntax")[0]
        .elements()
        .iter()
        .filter_map(|element| match element.kind() {
            SyntaxElementKind::Token(token) => Some(*token),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(tokens.len(), 59);
    assert!(!tokens.contains(&SyntaxTokenKind::Identifier));
    assert!(!tokens.contains(&SyntaxTokenKind::Invalid));
    assert_eq!(tokens[1], SyntaxTokenKind::Any);
    assert_eq!(tokens[6], SyntaxTokenKind::Break);
    assert_eq!(tokens[46], SyntaxTokenKind::TrySend);
    assert_eq!(tokens[50], SyntaxTokenKind::Ampersand);
    assert_eq!(tokens[57], SyntaxTokenKind::RangeInclusive);
    assert_eq!(tokens[58], SyntaxTokenKind::Semicolon);
}

#[test]
fn documented_future_forms_are_parsed_before_layer_one_semantic_admission() {
    let source = br#"interface Shape:
    fn area() -> i64

pool storage

fn typed(callback: fn(i64) -> i64, owned: own[storage] i64, erased: any Shape, fixed: [i64; 4]):
    pass

fn statements(value: i64):
    match value:
        case 0:
            pass
        case _:
            pass
    for item in [1]:
        continue
    while false:
        break
    defer value
    with value:
        pass
    take value
    send value
    try_send value

fn expressions(value: i64):
    range = 0..1
    identity = value is i64
    closure = |item| item
    repeated = [value; 4]
    moved = take value
    sent = send value
    tried = try_send value

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = Compiler::open(CompilerInstallation::layer1())
        .expect("Layer 1 opens")
        .compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
                Root::Image,
            )
            .with_inspection(InspectSelection::syntax()),
            &Cancellation::new(),
        );
    let (diagnostics, inspection) = match &outcome {
        CompilationOutcome::Accepted(accepted) => (accepted.diagnostics(), accepted.inspection()),
        CompilationOutcome::Rejected(rejected) => (rejected.diagnostics(), rejected.inspection()),
        other => panic!("documented syntax must remain contained: {other:#?}"),
    };
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code().starts_with("syntax.malformed")),
        "{diagnostics:#?}"
    );
    let kinds = inspection.syntax().expect("syntax requested")[0]
        .nodes()
        .iter()
        .map(|node| node.kind())
        .collect::<Vec<_>>();
    for expected in [
        SyntaxNodeKind::MatchStatement,
        SyntaxNodeKind::ForStatement,
        SyntaxNodeKind::WhileStatement,
        SyntaxNodeKind::DeferStatement,
        SyntaxNodeKind::WithStatement,
        SyntaxNodeKind::TakeStatement,
        SyntaxNodeKind::SendStatement,
        SyntaxNodeKind::TrySendStatement,
        SyntaxNodeKind::RangeExpression,
        SyntaxNodeKind::IsExpression,
        SyntaxNodeKind::ClosureExpression,
        SyntaxNodeKind::RepeatedArrayExpression,
        SyntaxNodeKind::TakeExpression,
        SyntaxNodeKind::SendExpression,
        SyntaxNodeKind::TrySendExpression,
    ] {
        assert!(
            kinds.contains(&expected),
            "missing syntax node {expected:?}"
        );
    }
}

#[test]
fn exponent_float_is_one_exact_literal_token() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let source =
        b"const SCALE: f64 = 1e+2f64\n\n@image\nfn build() -> Image:\n    return Image.new()\n";
    let CompilationOutcome::Accepted(accepted) = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_inspection(InspectSelection::syntax()),
        &Cancellation::new(),
    ) else {
        panic!("exponent literal must compile");
    };
    assert_eq!(
        accepted.inspection().syntax().expect("syntax")[0]
            .elements()
            .iter()
            .filter(|element| element.name() == "float_literal")
            .count(),
        1
    );
}

#[test]
fn malformed_edge_decimal_literals_remain_one_preserved_token() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    for (literal, expected_start) in [("1.", 17_u64), (".5", 17_u64)] {
        let source = format!(
            "const BAD: f64 = {literal}\n\n@image\nfn build() -> Image:\n    return Image.new()\n"
        );
        let outcome = compiler.compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source.as_bytes())]),
                Root::Image,
            )
            .with_inspection(InspectSelection::syntax()),
            &Cancellation::new(),
        );
        let CompilationOutcome::Rejected(rejected) = outcome else {
            panic!("{literal} is not a Wrela floating literal");
        };
        let invalid = rejected.inspection().syntax().expect("syntax")[0]
            .elements()
            .iter()
            .find(|element| {
                element.kind()
                    == &SyntaxElementKind::Invalid(wrela_compiler::SyntaxInvalidKind::Literal)
            })
            .expect("one malformed literal token");
        assert_eq!(invalid.range().start(), expected_start);
        assert_eq!(invalid.range().end() - invalid.range().start(), 2);
    }
}

#[test]
fn comment_marker_inside_text_is_not_reinterpreted_by_layout() {
    let source = b"@image\nfn build() -> Image:\n    return Image.new(label=\"x#y\")\n";
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        ),
        &Cancellation::new(),
    );
    assert!(
        matches!(outcome, CompilationOutcome::Accepted(_)),
        "a comment marker inside Text remains literal content: {outcome:#?}"
    );
}

#[test]
fn requesting_inspection_changes_only_the_projection() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let source = ProjectFile::new(
        "src/image.wr",
        b"@image\nfn build() -> Image:\n    return Image.new()\n",
    );
    let compile = |selection| {
        compiler.compile(
            CompilationRequest::new(ProjectSnapshot::new(vec![source.clone()]), Root::Image)
                .with_inspection(selection),
            &Cancellation::new(),
        )
    };
    let CompilationOutcome::Accepted(without) = compile(InspectSelection::none()) else {
        panic!("source accepts without inspection");
    };
    let CompilationOutcome::Accepted(with) = compile(InspectSelection::all()) else {
        panic!("source accepts with inspection");
    };
    assert_eq!(without.diagnostics(), with.diagnostics());
    assert!(without.inspection().syntax().is_none());
    assert!(with.inspection().syntax().is_some());
}

#[test]
fn inspection_selection_does_not_skip_unreachable_source_validation() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let files = vec![
        ProjectFile::new(
            "src/image.wr",
            b"@image\nfn build() -> Image:\n    return Image.new()\n",
        ),
        ProjectFile::new("src/unreachable.wr", b"fn broken(\n"),
    ];
    let compile = |selection| {
        compiler.compile(
            CompilationRequest::new(ProjectSnapshot::new(files.clone()), Root::Image)
                .with_inspection(selection),
            &Cancellation::new(),
        )
    };
    let CompilationOutcome::Rejected(without) = compile(InspectSelection::none()) else {
        panic!("unreachable malformed source must reject without inspection");
    };
    let CompilationOutcome::Rejected(with) = compile(InspectSelection::all()) else {
        panic!("unreachable malformed source must reject with inspection");
    };
    assert_eq!(without.diagnostics(), with.diagnostics());
}

#[test]
fn compilation_receipts_distinguish_project_revision_from_semantic_closure() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let root = ProjectFile::new(
        "src/image.wr",
        b"@image\nfn build() -> Image:\n    return Image.new()\n",
    );
    let compile = |revision: &str, unreachable: &[u8]| {
        compiler.compile(
            CompilationRequest::new(
                ProjectSnapshot::with_revision(
                    vec![
                        root.clone(),
                        ProjectFile::new("src/game/unreachable.wr", unreachable),
                    ],
                    revision,
                ),
                Root::Image,
            ),
            &Cancellation::new(),
        )
    };
    let CompilationOutcome::Accepted(before) = compile("editor-41", b"const VALUE: i64 = 1\n")
    else {
        panic!("receipt fixture must compile");
    };
    let CompilationOutcome::Accepted(after) = compile("editor-42", b"const VALUE: i64 = 2\n")
    else {
        panic!("receipt fixture must compile after an unreachable edit");
    };

    assert_eq!(before.inspection().project_revision(), "editor-41");
    assert_eq!(after.inspection().project_revision(), "editor-42");
    assert_ne!(
        before.inspection().snapshot_digest(),
        after.inspection().snapshot_digest()
    );
    assert_eq!(
        before.inspection().semantic_closure_digest(),
        after.inspection().semantic_closure_digest()
    );
}

#[test]
fn project_snapshot_verifies_a_host_supplied_digest() {
    let files = vec![ProjectFile::new(
        "src/image.wr",
        b"@image\nfn build() -> Image:\n    return Image.new()\n",
    )];
    let digest = ProjectSnapshot::new(files.clone()).digest();
    let verified = ProjectSnapshot::verified(files.clone(), "editor-7", digest)
        .expect("matching captured bytes verify");
    assert_eq!(verified.revision(), "editor-7");
    assert_eq!(verified.digest(), digest);

    let mismatch = ProjectSnapshot::verified(files, "editor-7", digest ^ 1)
        .expect_err("a stale adapter digest must not create a snapshot");
    assert_eq!(mismatch.expected(), digest ^ 1);
    assert_eq!(mismatch.actual(), digest);
}

#[test]
fn compiler_open_rejects_duplicate_and_invalid_authenticated_module_paths() {
    let duplicate = CompilerInstallation::with_authenticated_modules(vec![
        ProjectFile::new("src/std/core.wr", b""),
        ProjectFile::new("src/std/core.wr", b""),
    ]);
    assert!(matches!(
        Compiler::open(duplicate),
        Err(OpenError::DuplicateAuthenticatedModule { path })
            if path.as_ref() == "src/std/core.wr"
    ));

    let invalid = CompilerInstallation::with_authenticated_modules(vec![ProjectFile::new(
        "src/core.wr",
        b"",
    )]);
    assert!(matches!(
        Compiler::open(invalid),
        Err(OpenError::InvalidAuthenticatedModulePath { path })
            if path.as_ref() == "src/core.wr"
    ));
}

#[test]
fn compiler_open_rejects_authenticated_module_cycles() {
    let installation = CompilerInstallation::with_authenticated_modules(vec![
        ProjectFile::new(
            "src/std/first.wr",
            b"from std import second\n\npub const FIRST: i64 = 1\n",
        ),
        ProjectFile::new(
            "src/std/second.wr",
            b"from std import first\n\npub const SECOND: i64 = 2\n",
        ),
    ]);
    assert!(matches!(
        Compiler::open(installation),
        Err(OpenError::AuthenticatedImportCycle { path }) if path.as_ref() == "src/std/first.wr"
    ));
}

#[test]
fn compiler_open_seals_validated_versioned_distribution_content() {
    let malformed = CompilerInstallation::with_authenticated_modules(vec![ProjectFile::new(
        "src/core/broken.wr",
        b"pub fn broken(\n",
    )]);
    assert!(matches!(
        Compiler::open(malformed),
        Err(OpenError::MalformedAuthenticatedModule { path, code })
            if path.as_ref() == "src/core/broken.wr" && code.starts_with("syntax.")
    ));

    let first = Compiler::open(CompilerInstallation::with_authenticated_modules(vec![
        ProjectFile::new("src/core/value.wr", b"pub const VALUE: i64 = 1\n"),
    ]))
    .expect("valid distribution opens");
    let same = Compiler::open(CompilerInstallation::with_authenticated_modules(vec![
        ProjectFile::new("src/core/value.wr", b"pub const VALUE: i64 = 1\n"),
    ]))
    .expect("same distribution opens");
    let changed = Compiler::open(CompilerInstallation::with_authenticated_modules(vec![
        ProjectFile::new("src/core/value.wr", b"pub const VALUE: i64 = 2\n"),
    ]))
    .expect("changed distribution opens");
    let receipt = |compiler: &Compiler| {
        let CompilationOutcome::Rejected(rejected) = compiler.compile(
            CompilationRequest::new(ProjectSnapshot::new(Vec::new()), Root::Image),
            &Cancellation::new(),
        ) else {
            panic!("a missing root must reject with an inspection receipt");
        };
        rejected.inspection().clone()
    };
    let first = receipt(&first);
    let same = receipt(&same);
    let changed = receipt(&changed);
    assert_eq!(first.distribution_version(), "wrela9-layer1-v1");
    assert_eq!(first.distribution_digest(), same.distribution_digest());
    assert_ne!(first.distribution_digest(), changed.distribution_digest());
}

#[test]
fn compiler_open_rejects_semantically_invalid_authenticated_modules() {
    let installation = CompilerInstallation::with_authenticated_modules(vec![ProjectFile::new(
        "src/core/broken.wr",
        b"pub fn broken(value: MissingType):\n    pass\n",
    )]);
    assert!(matches!(
        Compiler::open(installation),
        Err(OpenError::InvalidAuthenticatedModule { path, code })
            if path.as_ref() == "src/core/broken.wr"
                && code.as_ref() == "semantic.unresolved_type"
    ));
}

#[test]
fn many_specialization_demands_materialize_once_in_canonical_order() {
    let mut source = String::from("pure fn identity[T](value: T) -> T:\n    return value\n\n");
    for index in 0..128 {
        source.push_str(&format!("struct Value{index}:\n    value: i64\n\n"));
        source.push_str(&format!(
            "const RESULT{index}: Value{index} = identity(Value{index}(value={index}))\n\n"
        ));
    }
    source.push_str("@image\nfn build() -> Image:\n    return Image.new()\n");
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source.as_bytes())]),
            Root::Image,
        )
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("large deterministic specialization demand must compile: {outcome:#?}");
    };
    let identities = accepted
        .inspection()
        .specializations()
        .iter()
        .filter(|specialization| specialization.function() == "identity")
        .map(|specialization| specialization.identity())
        .collect::<Vec<_>>();
    assert_eq!(identities.len(), 128);
    assert!(identities.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn arbitrary_invalid_bytes_are_rejected_but_exactly_partitioned() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let source = [0, 0xff, b'\r', b'\t', b'\n'];
    let project = ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]);
    let request =
        CompilationRequest::new(project, Root::Image).with_inspection(InspectSelection::syntax());

    let CompilationOutcome::Rejected(rejected) = compiler.compile(request, &Cancellation::new())
    else {
        panic!("invalid bytes must be rejected");
    };
    let syntax = &rejected.inspection().syntax().expect("syntax requested")[0];
    assert_eq!(syntax.source_bytes(), source);
    let physical: Vec<_> = syntax
        .elements()
        .iter()
        .filter(|element| element.range().start() != element.range().end())
        .map(|element| (element.range().start(), element.range().end()))
        .collect();
    assert_eq!(physical, [(0, 1), (1, 2), (2, 3), (3, 4), (4, 5)]);
    assert_eq!(
        rejected
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [
            "syntax.invalid_character",
            "syntax.invalid_encoding",
            "syntax.bare_carriage_return",
            "syntax.tab_outside_literal",
        ]
    );
}

#[test]
fn every_individual_byte_is_losslessly_contained() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    for value in 0_u8..=u8::MAX {
        let source = [value];
        let outcome = compiler.compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
                Root::Image,
            )
            .with_inspection(InspectSelection::syntax()),
            &Cancellation::new(),
        );
        let syntax = match &outcome {
            CompilationOutcome::Accepted(accepted) => accepted.inspection().syntax(),
            CompilationOutcome::Rejected(rejected) => rejected.inspection().syntax(),
            other => panic!("byte {value:#04x} escaped creator containment: {other:#?}"),
        }
        .expect("syntax requested");
        assert_eq!(syntax[0].source_bytes(), source, "byte {value:#04x}");
        assert_eq!(
            syntax[0]
                .elements()
                .iter()
                .map(|element| element.range().end() - element.range().start())
                .sum::<u64>(),
            1,
            "byte {value:#04x} must have one physical owner"
        );
    }
}

#[test]
fn compact_deterministic_mutations_never_escape_creator_containment() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let seed = b"@image\nfn build() -> Image:\n    return Image.new()\n";
    let selected = (0..seed.len()).step_by(3).collect::<Vec<_>>();
    let mut mutations = Vec::new();
    for position in selected {
        let mut deleted = seed.to_vec();
        deleted.remove(position);
        mutations.push(deleted);

        let mut replaced = seed.to_vec();
        replaced[position] ^= 0x80;
        mutations.push(replaced);

        let mut inserted = seed.to_vec();
        inserted.insert(position, b']');
        mutations.push(inserted);
    }
    for source in mutations {
        let outcome = compiler.compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", &source)]),
                Root::Image,
            )
            .with_inspection(InspectSelection::syntax()),
            &Cancellation::new(),
        );
        let syntax = match &outcome {
            CompilationOutcome::Accepted(accepted) => accepted.inspection().syntax(),
            CompilationOutcome::Rejected(rejected) => rejected.inspection().syntax(),
            other => panic!("bounded source mutation escaped containment: {other:#?}"),
        }
        .expect("syntax requested");
        assert_eq!(syntax[0].source_bytes(), source);
    }
}

#[test]
fn source_authored_semantic_edge_cases_never_escape_the_outcome_firewall() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("Layer 1 opens");
    let corpus: &[&[u8]] = &[
        br#"struct Trouble:
    code: i64
fn swallow(value: Result[i64, Trouble]) -> i64:
    return 7
fn f() -> Result[i64]:
    return Result.Ok(swallow(Result.Err(Trouble(code=1))))
@image
fn build() -> Image:
    return Image.new()
"#,
        br#"enum First:
    Error
enum Second:
    Error
fn conflict(flag: bool) -> Result[i64]:
    if flag:
        return Result.Err(First.Error)
    return Result.Err(Second.Error)
@image
fn build() -> Image:
    return Image.new()
"#,
        br#"pure fn loop_forever(value: i64) -> i64:
    return loop_forever(value)
@image
fn build() -> Image:
    return Image.new()
"#,
        br#"resource struct Ticket:
    bytes: Bytes
struct Holder:
    ticket: Ticket
fn consume(take ticket: Ticket):
    pass
@image
fn build() -> Image:
    holder = Holder(ticket=Ticket(bytes=b\"x\"))
    consume(take holder.ticket)
    consume(take holder.ticket)
    return Image.new()
"#,
        b"fn broken(value: Missing):\n    return value\n\n@image\nfn build() -> Image:\n    return Image.new()\n",
    ];
    for source in corpus {
        let outcome = compiler.compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
                Root::Image,
            ),
            &Cancellation::new(),
        );
        assert!(
            matches!(
                outcome,
                CompilationOutcome::Accepted(_) | CompilationOutcome::Rejected(_)
            ),
            "Creator source escaped containment for {}: {outcome:#?}",
            String::from_utf8_lossy(source)
        );
    }
}

#[test]
fn generated_eligible_pure_programs_have_exact_wrela_outcomes() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    for left in -8_i64..8 {
        let right = left * 3 + 1;
        let expected = left + right;
        let source = format!(
            "const VALUE: i64 = {left} + {right}\n\n@image\nfn build() -> Image:\n    return Image.new(value=VALUE)\n"
        );
        let CompilationOutcome::Accepted(accepted) = compiler.compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source.as_bytes())]),
                Root::Image,
            )
            .with_inspection(InspectSelection::all()),
            &Cancellation::new(),
        ) else {
            panic!("generated pure program must compile");
        };
        let value = accepted
            .inspection()
            .evaluations()
            .iter()
            .find(|evaluation| evaluation.root() == "image.VALUE")
            .expect("generated constant is evaluated");
        assert_eq!(
            value.outcome(),
            &EvaluationOutcome::Completed(CanonicalValue::Integer {
                type_name: "i64".into(),
                value: i128::from(expected),
            })
        );
    }
}

#[test]
fn one_compiler_and_reopened_compiler_produce_identical_outcomes() {
    fn request() -> CompilationRequest {
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new(
                "src/image.wr",
                b"const VALUE: i64 = 6 * 7\n\n@image\nfn build() -> Image:\n    return Image.new(value=VALUE)\n",
            )]),
            Root::Image,
        )
        .with_inspection(InspectSelection::all())
    }

    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let first = compiler.compile(request(), &Cancellation::new());
    let repeated = compiler.compile(request(), &Cancellation::new());
    let reopened = Compiler::open(CompilerInstallation::layer1())
        .expect("distribution reopens")
        .compile(request(), &Cancellation::new());
    assert_eq!(first, repeated);
    assert_eq!(first, reopened);
}

#[test]
fn invalid_encoding_inside_a_literal_remains_one_invalid_literal() {
    let source =
        b"const BAD: Text = \"a\xffb\"\n\n@image\nfn build() -> Image:\n    return Image.new()\n";
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let CompilationOutcome::Rejected(rejected) = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_inspection(InspectSelection::syntax()),
        &Cancellation::new(),
    ) else {
        panic!("invalid literal encoding must reject");
    };
    assert_eq!(rejected.diagnostics()[0].code(), "syntax.invalid_encoding");
    let invalid = rejected.inspection().syntax().expect("syntax")[0]
        .elements()
        .iter()
        .find(|element| element.name() == "invalid_literal")
        .expect("invalid literal leaf");
    assert_eq!((invalid.range().start(), invalid.range().end()), (18, 23));
}

#[test]
fn unrecognized_top_level_text_is_not_silently_accepted() {
    let source =
        b"this is not a declaration\n\n@image\nfn build() -> Image:\n    return Image.new()\n";
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let CompilationOutcome::Rejected(rejected) = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        ),
        &Cancellation::new(),
    ) else {
        panic!("top-level executable text must reject");
    };
    assert_eq!(
        rejected.diagnostics()[0].code(),
        "syntax.unexpected_top_level"
    );
}

#[test]
fn declaration_introducer_without_a_name_is_explicitly_rejected() {
    let source = b"fn\n\n@image\nfn build() -> Image:\n    return Image.new()\n";
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let CompilationOutcome::Rejected(rejected) = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        ),
        &Cancellation::new(),
    ) else {
        panic!("a declaration introducer must own malformed-declaration evidence");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "syntax.malformed_declaration")
    );
}

#[test]
fn earlier_diagnostic_does_not_suppress_later_malformed_declaration() {
    let source = b"\t\nconst broken: = 1\n\n@image\nfn build() -> Image:\n    return Image.new()\n";
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let CompilationOutcome::Rejected(rejected) = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        ),
        &Cancellation::new(),
    ) else {
        panic!("each malformed declaration must retain local evidence");
    };
    let codes = rejected
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"syntax.tab_outside_literal"));
    assert!(codes.contains(&"syntax.malformed_declaration"));
}

#[test]
fn unreachable_modules_retain_lossless_syntax_without_entering_the_semantic_closure() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let project = ProjectSnapshot::new(vec![
        ProjectFile::new(
            "src/image.wr",
            b"from game import cards\n\n@image\nfn build() -> Image:\n    return cards.image()\n",
        ),
        ProjectFile::new(
            "src/game/cards.wr",
            b"pub fn image() -> Image:\n    return Image.new()\n",
        ),
        ProjectFile::new("src/game/broken.wr", [0xff, 0xfe]),
    ]);
    let request =
        CompilationRequest::new(project, Root::Image).with_inspection(InspectSelection::all());

    let outcome = compiler.compile(request, &Cancellation::new());
    let CompilationOutcome::Accepted(accepted) = &outcome else {
        panic!("unreachable malformed source must be inert: {outcome:#?}");
    };
    assert_eq!(
        accepted
            .inspection()
            .closure()
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>(),
        ["src/game/cards.wr", "src/image.wr"]
    );
    let syntax = accepted.inspection().syntax().expect("syntax");
    assert_eq!(syntax.len(), 3);
    assert_eq!(syntax[0].path(), "src/game/broken.wr");
    assert_eq!(syntax[0].source_bytes(), [0xff, 0xfe]);
    assert_eq!(syntax[1].path(), "src/game/cards.wr");
    assert_eq!(syntax[2].path(), "src/image.wr");
    assert!(accepted.diagnostics().is_empty());
}

#[test]
fn missing_selected_root_still_retains_every_captured_file_as_lossless_syntax() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let project = ProjectSnapshot::new(vec![
        ProjectFile::new("src/game/cards.wr", b"const VALUE: i64 = 1\n"),
        ProjectFile::new("src/game/broken.wr", [0xff, 0xfe]),
    ]);

    let CompilationOutcome::Rejected(rejected) = compiler.compile(
        CompilationRequest::new(project, Root::Image).with_inspection(InspectSelection::syntax()),
        &Cancellation::new(),
    ) else {
        panic!("a missing selected root must reject");
    };

    assert_eq!(rejected.diagnostics()[0].code(), "project.missing_root");
    let syntax = rejected.inspection().syntax().expect("syntax");
    assert_eq!(syntax.len(), 2);
    assert_eq!(syntax[0].path(), "src/game/broken.wr");
    assert_eq!(syntax[0].source_bytes(), [0xff, 0xfe]);
    assert_eq!(syntax[1].path(), "src/game/cards.wr");
    assert_eq!(syntax[1].source_bytes(), b"const VALUE: i64 = 1\n");
}

#[test]
fn missing_import_is_a_creator_rejection() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let project = ProjectSnapshot::new(vec![ProjectFile::new(
        "src/image.wr",
        b"from game import missing\n\n@image\nfn build() -> Image:\n    return Image.new()\n",
    )]);

    let CompilationOutcome::Rejected(rejected) = compiler.compile(
        CompilationRequest::new(project, Root::Image),
        &Cancellation::new(),
    ) else {
        panic!("missing import must reject");
    };
    assert_eq!(rejected.diagnostics()[0].code(), "project.missing_module");
}

#[test]
fn layout_recovery_keeps_later_declaration_visible() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let source = b"fn broken():\npass\n\npub fn later() -> i64:\n    return 7\n";
    let project = ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]);

    let CompilationOutcome::Rejected(rejected) = compiler.compile(
        CompilationRequest::new(project, Root::Image).with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    ) else {
        panic!("missing block must reject");
    };
    assert_eq!(rejected.diagnostics()[0].code(), "syntax.missing_block");
    let syntax = &rejected.inspection().syntax().expect("syntax")[0];
    assert!(syntax.elements().iter().any(|element| {
        element.name() == "missing_block" && element.range().start() == element.range().end()
    }));
    assert_eq!(syntax.source_bytes(), source);
    assert!(
        rejected
            .inspection()
            .function_facts()
            .iter()
            .any(|facts| facts.name() == "later")
    );
}

#[test]
fn delimiters_recover_with_structured_missing_and_unmatched_evidence() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let source = b"const A: i64 = (1 + 2\nconst B: i64 = 3]\n";
    let project = ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]);
    let CompilationOutcome::Rejected(rejected) = compiler.compile(
        CompilationRequest::new(project, Root::Image).with_inspection(InspectSelection::syntax()),
        &Cancellation::new(),
    ) else {
        panic!("delimiter errors must reject");
    };
    let codes: Vec<_> = rejected
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code())
        .collect();
    assert_eq!(codes, ["syntax.unmatched_closer", "syntax.missing_closer"]);
}

#[test]
fn syntax_diagnostics_are_capped_and_explicitly_truncated() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let source = vec![0xff; 100];
    let project = ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]);
    let CompilationOutcome::Rejected(rejected) = compiler.compile(
        CompilationRequest::new(project, Root::Image),
        &Cancellation::new(),
    ) else {
        panic!("invalid bytes must reject");
    };
    assert_eq!(rejected.diagnostics().len(), 65);
    assert_eq!(
        rejected.diagnostics().last().expect("truncation").code(),
        "syntax.diagnostics_truncated"
    );
}

#[test]
fn semantic_identity_survives_body_edits_while_fingerprint_changes() {
    fn compile(source: &[u8]) -> wrela_compiler::AcceptedCompilation {
        let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
        let project = ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]);
        let CompilationOutcome::Accepted(accepted) = compiler.compile(
            CompilationRequest::new(project, Root::Image).with_inspection(InspectSelection::all()),
            &Cancellation::new(),
        ) else {
            panic!("source must compile");
        };
        accepted
    }

    let before = compile(b"pub fn answer() -> i64:\n    return 41\n\n@image\nfn build() -> Image:\n    return Image.new()\n");
    let after = compile(b"pub fn answer() -> i64:\n    return 42\n\n@image\nfn build() -> Image:\n    return Image.new()\n");
    let before_answer = before
        .inspection()
        .identities()
        .iter()
        .find(|identity| identity.name() == "answer")
        .expect("answer identity");
    let after_answer = after
        .inspection()
        .identities()
        .iter()
        .find(|identity| identity.name() == "answer")
        .expect("answer identity");

    assert_eq!(before_answer.domain(), after_answer.domain());
    assert_eq!(before_answer.digest(), after_answer.digest());
    assert_eq!(before_answer.origin(), after_answer.origin());
    assert_ne!(before_answer.fingerprint(), after_answer.fingerprint());
}

#[test]
fn attributes_belong_to_the_declaration_they_modify() {
    fn fingerprints(attribute: &str) -> (u128, u128) {
        let source = format!(
            "fn helper():\n    pass\n\n{attribute}\nfn build() -> Image:\n    return Image.new()\n"
        );
        let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
        let CompilationOutcome::Accepted(accepted) = compiler.compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
                Root::Image,
            )
            .with_inspection(InspectSelection::all()),
            &Cancellation::new(),
        ) else {
            panic!("attribute ownership fixture must compile");
        };
        let fingerprint = |name: &str| {
            accepted
                .inspection()
                .identities()
                .iter()
                .find(|identity| {
                    identity.domain() == IdentityDomain::Definition && identity.name() == name
                })
                .expect("definition identity")
                .fingerprint()
        };
        (fingerprint("helper"), fingerprint("build"))
    }

    let before = fingerprints("@image");
    let after = fingerprints("@image()");
    assert_eq!(before.0, after.0, "the preceding declaration is unchanged");
    assert_ne!(before.1, after.1, "the attributed declaration changed");
}

#[test]
fn documentation_belongs_to_the_declaration_it_documents() {
    fn fingerprints(documentation: &str) -> (u128, u128) {
        let source = format!(
            "fn helper():\n    pass\n\n## {documentation}\n@image\nfn build() -> Image:\n    return Image.new()\n"
        );
        let outcome = Compiler::open(CompilerInstallation::layer1())
            .expect("distribution opens")
            .compile(
                CompilationRequest::new(
                    ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
                    Root::Image,
                )
                .with_inspection(InspectSelection::all()),
                &Cancellation::new(),
            );
        let CompilationOutcome::Accepted(accepted) = outcome else {
            panic!("documentation ownership fixture must compile: {outcome:#?}");
        };
        let find = |name: &str| {
            accepted
                .inspection()
                .identities()
                .iter()
                .find(|identity| {
                    identity.domain() == IdentityDomain::Definition && identity.name() == name
                })
                .expect("definition")
                .fingerprint()
        };
        (find("helper"), find("build"))
    }
    let before = fingerprints("first");
    let after = fingerprints("second");
    assert_eq!(before.0, after.0);
    assert_ne!(before.1, after.1);
}

#[test]
fn identities_do_not_depend_on_snapshot_enumeration_order() {
    fn compile(files: Vec<ProjectFile>) -> Vec<(IdentityDomain, String, u128)> {
        let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
        let outcome = compiler.compile(
            CompilationRequest::new(ProjectSnapshot::new(files), Root::Image)
                .with_inspection(InspectSelection::all()),
            &Cancellation::new(),
        );
        let CompilationOutcome::Accepted(accepted) = &outcome else {
            panic!("source must compile: {outcome:#?}");
        };
        accepted
            .inspection()
            .identities()
            .iter()
            .map(|identity| {
                (
                    identity.domain(),
                    identity.name().to_owned(),
                    identity.digest(),
                )
            })
            .collect()
    }

    let root = ProjectFile::new(
        "src/image.wr",
        b"from game import cards\n\n@image\nfn build() -> Image:\n    return Image.new(answer=cards.answer())\n",
    );
    let cards = ProjectFile::new(
        "src/game/cards.wr",
        b"pub fn answer() -> i64:\n    return 42\n",
    );
    assert_eq!(
        compile(vec![root.clone(), cards.clone()]),
        compile(vec![cards, root])
    );
}

#[test]
fn identity_catalog_covers_nested_declarations_and_callable_parameters() {
    let source = br#"interface Measured:
    fn measure(read self) -> i64

struct Box[T]:
    value: T
    fn replace[U](self, next: U) -> U:
        return next

enum Choice:
    Item(value: i64)

fn choose(flag: bool) -> i64:
    return 1

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = Compiler::open(CompilerInstallation::layer1())
        .expect("distribution opens")
        .compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
                Root::Image,
            )
            .with_inspection(InspectSelection::all()),
            &Cancellation::new(),
        );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("nested identity fixture must compile: {outcome:#?}");
    };
    let names = accepted
        .inspection()
        .identities()
        .iter()
        .filter(|identity| identity.domain() == IdentityDomain::Definition)
        .map(|identity| identity.name())
        .collect::<std::collections::BTreeSet<_>>();
    for expected in [
        "Measured.measure",
        "Measured.measure.self",
        "Box.T",
        "Box.value",
        "Box.replace",
        "Box.replace.U",
        "Box.replace.self",
        "Box.replace.next",
        "Choice.Item.value",
        "choose.flag",
    ] {
        assert!(
            names.contains(expected),
            "missing nested DefId for {expected}"
        );
    }
}

#[test]
fn identity_observations_do_not_expose_private_canonical_keys() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new(
                "src/image.wr",
                b"@image\nfn build() -> Image:\n    return Image.new()\n",
            )]),
            Root::Image,
        )
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("identity fixture must compile");
    };
    let rendered = format!("{:#?}", accepted.inspection().identities());
    assert!(!rendered.contains("canonical_key"), "{rendered}");
}

#[test]
fn nested_and_specialized_fingerprints_own_their_complete_meaning() {
    fn accepted(source: &[u8], root: Root) -> wrela_compiler::AcceptedCompilation {
        let outcome = Compiler::open(CompilerInstallation::layer1())
            .expect("distribution opens")
            .compile(
                CompilationRequest::new(
                    ProjectSnapshot::new(vec![ProjectFile::new(root_path(root), source)]),
                    root,
                )
                .with_inspection(InspectSelection::all()),
                &Cancellation::new(),
            );
        let CompilationOutcome::Accepted(accepted) = outcome else {
            panic!("fingerprint fixture must compile: {outcome:#?}");
        };
        accepted
    }
    fn root_path(root: Root) -> &'static str {
        match root {
            Root::Image => "src/image.wr",
            Root::Test => "src/test.wr",
        }
    }
    fn fingerprint(
        accepted: &wrela_compiler::AcceptedCompilation,
        domain: IdentityDomain,
        name: &str,
    ) -> u128 {
        accepted
            .inspection()
            .identities()
            .iter()
            .find(|identity| identity.domain() == domain && identity.name() == name)
            .expect("identity observation")
            .fingerprint()
    }

    let test_before = accepted(
        br#"pub suite behavior:
    test works():
        expect true

@image
fn build() -> Image:
    return Image.new(tests=Test.new(cases=[behavior.works()]))
"#,
        Root::Test,
    );
    let test_after = accepted(
        br#"pub suite behavior:
    test works():
        expect 1 == 1

@image
fn build() -> Image:
    return Image.new(tests=Test.new(cases=[behavior.works()]))
"#,
        Root::Test,
    );
    assert_ne!(
        fingerprint(&test_before, IdentityDomain::Test, "behavior.works"),
        fingerprint(&test_after, IdentityDomain::Test, "behavior.works")
    );

    let variant_before = accepted(
        b"enum Choice:\n    Item(value: i64)\n\n@image\nfn build() -> Image:\n    return Image.new()\n",
        Root::Image,
    );
    let variant_after = accepted(
        b"enum Choice:\n    Item(value: bool)\n\n@image\nfn build() -> Image:\n    return Image.new()\n",
        Root::Image,
    );
    assert_ne!(
        fingerprint(&variant_before, IdentityDomain::Generated, "Choice.Item"),
        fingerprint(&variant_after, IdentityDomain::Generated, "Choice.Item")
    );

    let specialization_before = accepted(
        b"pure fn answer() -> i64:\n    return 41\n\nconst VALUE: i64 = answer()\n\n@image\nfn build() -> Image:\n    return Image.new(value=VALUE)\n",
        Root::Image,
    );
    let specialization_after = accepted(
        b"pure fn answer() -> i64:\n    return 42\n\nconst VALUE: i64 = answer()\n\n@image\nfn build() -> Image:\n    return Image.new(value=VALUE)\n",
        Root::Image,
    );
    let specialization = specialization_before
        .inspection()
        .specializations()
        .iter()
        .find(|specialization| specialization.function() == "answer")
        .expect("answer specialization");
    let before = specialization_before
        .inspection()
        .identities()
        .iter()
        .find(|identity| identity.digest() == specialization.identity())
        .expect("specialization identity");
    let after = specialization_after
        .inspection()
        .identities()
        .iter()
        .find(|identity| identity.digest() == specialization.identity())
        .expect("stable specialization identity");
    assert_eq!(before.digest(), after.digest());
    assert_ne!(before.fingerprint(), after.fingerprint());
}

#[test]
fn nominal_type_identity_is_domain_separated_from_definition_identity() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let source =
        b"struct Card:\n    value: i64\n\n@image\nfn build() -> Image:\n    return Image.new()\n";
    let CompilationOutcome::Accepted(accepted) = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    ) else {
        panic!("nominal identity fixture must compile");
    };
    let definition = accepted
        .inspection()
        .identities()
        .iter()
        .find(|identity| {
            identity.domain() == IdentityDomain::Definition && identity.name() == "Card"
        })
        .expect("Card DefinitionId");
    let type_ = accepted
        .inspection()
        .identities()
        .iter()
        .find(|identity| identity.domain() == IdentityDomain::Type && identity.name() == "Card")
        .expect("Card TypeId");
    assert_ne!(definition.digest(), type_.digest());
}

#[test]
fn source_size_limit_has_an_exact_lossless_boundary() {
    const LIMIT: usize = 1_048_576;
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let prefix = b"@image\nfn build() -> Image:\n    return Image.new()\n#";
    let mut at_limit = prefix.to_vec();
    at_limit.resize(LIMIT, b'x');
    let at_limit_outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", &at_limit)]),
            Root::Image,
        ),
        &Cancellation::new(),
    );
    assert!(matches!(at_limit_outcome, CompilationOutcome::Accepted(_)));

    at_limit.push(b'x');
    let CompilationOutcome::Rejected(rejected) = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", &at_limit)]),
            Root::Image,
        )
        .with_inspection(InspectSelection::syntax()),
        &Cancellation::new(),
    ) else {
        panic!("one byte beyond limit must reject");
    };
    assert_eq!(rejected.diagnostics()[0].code(), "syntax.source_too_large");
    assert_eq!(
        rejected.inspection().syntax().expect("syntax")[0].source_bytes(),
        at_limit
    );
}

#[test]
fn nesting_limit_is_diagnosed_without_losing_bytes() {
    let mut source = b"const DEEP: i64 = ".to_vec();
    source.extend(std::iter::repeat_n(b'(', 257));
    source.push(b'1');
    source.extend(std::iter::repeat_n(b')', 257));
    source.extend_from_slice(b"\n\n@image\nfn build() -> Image:\n    return Image.new()\n");
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let CompilationOutcome::Rejected(rejected) = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", &source)]),
            Root::Image,
        )
        .with_inspection(InspectSelection::syntax()),
        &Cancellation::new(),
    ) else {
        panic!("excess nesting must reject");
    };
    assert_eq!(
        rejected.diagnostics()[0].code(),
        "syntax.nesting_limit_exceeded"
    );
    assert_eq!(
        rejected.inspection().syntax().expect("syntax")[0].source_bytes(),
        source
    );
}

#[test]
fn expect_is_rejected_outside_a_wrela_test() {
    let source =
        b"fn helper():\n    expect false\n\n@image\nfn build() -> Image:\n    return Image.new()\n";
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        ),
        &Cancellation::new(),
    );
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("expect is Test-only syntax: {outcome:#?}");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "syntax.malformed_declaration")
    );
}

#[test]
fn an_attribute_statement_is_rejected_instead_of_discarded() {
    let source = b"fn helper():\n    @image\n    pass\n\n@image\nfn build() -> Image:\n    return Image.new()\n";
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        ),
        &Cancellation::new(),
    );
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("attributes cannot be silently discarded as statements: {outcome:#?}");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "syntax.malformed_declaration")
    );
}
