use wrela_compiler::{
    Cancellation, CompilationOutcome, CompilationRequest, Compiler, CompilerInstallation,
    InspectSelection, ProjectFile, ProjectSnapshot, Root,
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
    assert!(accepted.diagnostics().is_empty());
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

    let CompilationOutcome::Accepted(accepted) = compiler.compile(request, &Cancellation::new())
    else {
        panic!("unreachable malformed source must be inert");
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
        CompilationRequest::new(project, Root::Image).with_inspection(InspectSelection::syntax()),
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
    assert_eq!(before_answer.canonical_key(), after_answer.canonical_key());
    assert_ne!(before_answer.fingerprint(), after_answer.fingerprint());
}

#[test]
fn identities_do_not_depend_on_snapshot_enumeration_order() {
    fn compile(files: Vec<ProjectFile>) -> Vec<(String, u128)> {
        let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
        let CompilationOutcome::Accepted(accepted) = compiler.compile(
            CompilationRequest::new(ProjectSnapshot::new(files), Root::Image)
                .with_inspection(InspectSelection::all()),
            &Cancellation::new(),
        ) else {
            panic!("source must compile");
        };
        accepted
            .inspection()
            .identities()
            .iter()
            .map(|identity| (identity.canonical_key().to_owned(), identity.digest()))
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
