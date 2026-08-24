use wrela_compiler::{
    Cancellation, CompilationOutcome, CompilationRequest, Compiler, CompilerInstallation,
    InspectSelection, ProjectFile, ProjectSnapshot, Root,
};

fn compile(source: &[u8]) -> CompilationOutcome {
    Compiler::open(CompilerInstallation::layer1())
        .expect("Layer 1 distribution opens")
        .compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
                Root::Image,
            )
            .with_inspection(InspectSelection::all()),
            &Cancellation::new(),
        )
}

#[test]
fn authenticated_pool_factory_opens_and_closes_a_scoped_pool() {
    let source = br#"from core import pool as pools

@image
fn build() -> Image:
    with pools.scoped(capacity=1) as scratch:
        pass
    return Image.new()
"#;

    let outcome = compile(source);
    assert!(
        matches!(outcome, CompilationOutcome::Accepted(_)),
        "the authenticated factory must open a bounded local Pool: {outcome:#?}"
    );
}

#[test]
fn project_scoped_lookalikes_have_no_pool_authority() {
    let source = br#"resource struct Scope:
    capacity: u64

pure fn scoped(capacity: u64) -> Scope:
    return Scope(capacity=capacity)

@image
fn build() -> Image:
    with scoped(capacity=1u64) as scratch:
        pass
    return Image.new()
"#;

    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("a Project-authored lookalike must not open a Pool scope");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == "semantic.unsupported_layer_one_syntax" }),
        "{rejected:#?}"
    );
}

#[test]
fn scoped_pool_binding_does_not_escape_its_with_block() {
    let source = br#"from core import pool as pools

@image
fn build() -> Image:
    with pools.scoped(capacity=1) as scratch:
        pass
    scratch
    return Image.new()
"#;

    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("the scoped handle must not escape its lexical block");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.unresolved_name"),
        "{rejected:#?}"
    );
}
