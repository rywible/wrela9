use wrela_compiler::{
    Cancellation, CanonicalValue, CompilationOutcome, CompilationRequest, Compiler,
    CompilerInstallation, EvaluationOutcome, IdentityDomain, InspectSelection, ProjectFile,
    ProjectSnapshot, Root, SyntaxElementKind, SyntaxNodeKind, SyntaxTokenKind,
};

#[test]
fn valid_image_is_accepted_without_losing_source_bytes() {
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
fn syntax_exposes_closed_exact_token_kinds_instead_of_broad_categories() {
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
fn exponent_float_is_one_exact_literal_token() {
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
fn comment_marker_inside_text_is_not_reinterpreted_by_layout() {
    let source = b"@image\nfn build() -> Image:\n    return Image.new(label=\"x#y\")\n";
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
fn arbitrary_invalid_bytes_are_rejected_but_exactly_partitioned() {
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
fn generated_eligible_pure_programs_have_exact_wrela_outcomes() {
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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

    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
    let first = compiler.compile(request(), &Cancellation::new());
    let repeated = compiler.compile(request(), &Cancellation::new());
    let reopened = Compiler::open(CompilerInstallation::empty())
        .expect("distribution reopens")
        .compile(request(), &Cancellation::new());
    assert_eq!(first, repeated);
    assert_eq!(first, reopened);
}

#[test]
fn invalid_encoding_inside_a_literal_remains_one_invalid_literal() {
    let source =
        b"const BAD: Text = \"a\xffb\"\n\n@image\nfn build() -> Image:\n    return Image.new()\n";
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
fn only_reachable_modules_can_reject_the_selected_root() {
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
    assert_eq!(accepted.inspection().syntax().expect("syntax").len(), 2);
}

#[test]
fn missing_import_is_a_creator_rejection() {
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
        let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
        let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
        let outcome = Compiler::open(CompilerInstallation::empty())
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
        let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
fn nested_and_specialized_fingerprints_own_their_complete_meaning() {
    fn accepted(source: &[u8], root: Root) -> wrela_compiler::AcceptedCompilation {
        let outcome = Compiler::open(CompilerInstallation::empty())
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
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
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
