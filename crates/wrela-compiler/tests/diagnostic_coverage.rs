use wrela_compiler::{
    Cancellation, CompilationOutcome, CompilationRequest, Compiler, CompilerInstallation,
    ProjectFile, ProjectSnapshot, Root,
};

fn compile(files: Vec<ProjectFile>, root: Root) -> CompilationOutcome {
    Compiler::open(CompilerInstallation::layer1())
        .expect("distribution opens")
        .compile(
            CompilationRequest::new(ProjectSnapshot::new(files), root),
            &Cancellation::new(),
        )
}

fn image(source: &[u8]) -> CompilationOutcome {
    compile(vec![ProjectFile::new("src/image.wr", source)], Root::Image)
}

fn test_image(source: &[u8]) -> CompilationOutcome {
    compile(vec![ProjectFile::new("src/test.wr", source)], Root::Test)
}

fn assert_code(outcome: CompilationOutcome, expected: &str) {
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("{expected} fixture must reject: {outcome:#?}");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == expected),
        "missing {expected}: {:#?}",
        rejected.diagnostics()
    );
}

#[test]
fn project_admission_diagnostics_are_directly_observable() {
    let valid = b"@image\nfn build() -> Image:\n    return Image.new()\n";
    for (outcome, code) in [
        (
            compile(
                vec![
                    ProjectFile::new("src/image.wr", valid),
                    ProjectFile::new("src/image.wr", valid),
                ],
                Root::Image,
            ),
            "project.duplicate_path",
        ),
        (
            compile(
                vec![
                    ProjectFile::new("src/image.wr", valid),
                    ProjectFile::new("src/utility.wr", b""),
                ],
                Root::Image,
            ),
            "project.invalid_module_path",
        ),
        (compile(Vec::new(), Root::Image), "project.missing_root"),
    ] {
        assert_code(outcome, code);
    }
}

#[test]
fn lexical_and_layout_diagnostics_are_directly_observable() {
    let cases: &[(&[u8], &str)] = &[
        (
            b"\xef\xbb\xbf@image\nfn build() -> Image:\n    return Image.new()\n",
            "syntax.byte_order_mark",
        ),
        (
            b"@image\nfn build() -> Image:\n    return Image.new()\n\nfrom game import cards\n",
            "syntax.import_after_declaration",
        ),
        (
            b"fn helper():\n    if true:\n        pass\n      pass\n",
            "syntax.inconsistent_dedent",
        ),
        (
            b"fn helper():\n   pass\n",
            "syntax.invalid_indentation_width",
        ),
        (b"$\n", "syntax.invalid_token"),
        (b"comptime assert\n", "syntax.malformed_comptime_assertion"),
        (b"from game cards\n", "syntax.malformed_import"),
        (b"const TEXT: Text = \"open\n", "syntax.unclosed_literal"),
        (
            b"    const VALUE: i64 = 1\n",
            "syntax.unexpected_indentation",
        ),
    ];
    for (source, code) in cases {
        assert_code(image(source), code);
    }
}

#[test]
fn declaration_and_type_diagnostics_are_directly_observable() {
    let cases: &[(&[u8], &str)] = &[
        (
            br#"struct Card:
    value: i64
    value: i64

@image
fn build() -> Image:
    return Image.new()
"#,
            "semantic.duplicate_field",
        ),
        (
            br#"interface Drawable:
    fn draw()
    fn draw()

@image
fn build() -> Image:
    return Image.new()
"#,
            "semantic.duplicate_interface_requirement",
        ),
        (
            br#"fn helper():
    mut value = 1
    mut value = 2

@image
fn build() -> Image:
    return Image.new()
"#,
            "semantic.duplicate_local",
        ),
        (
            br#"struct Card:
    fn value():
        pass
    fn value():
        pass

@image
fn build() -> Image:
    return Image.new()
"#,
            "semantic.duplicate_member",
        ),
        (
            br#"struct Card:
    fn convert[T, T]():
        pass

@image
fn build() -> Image:
    return Image.new()
"#,
            "semantic.duplicate_type_parameter",
        ),
        (
            br#"enum Choice:
    Item
    Item

@image
fn build() -> Image:
    return Image.new()
"#,
            "semantic.duplicate_variant",
        ),
        (
            br#"enum Choice:
    Item(read value: i64)

@image
fn build() -> Image:
    return Image.new()
"#,
            "semantic.enum_payload_requires_value_mode",
        ),
        (
            br#"struct Marker:
    value: i64

struct Card implements Marker:
    value: i64

@image
fn build() -> Image:
    return Image.new()
"#,
            "semantic.implements_requires_interface",
        ),
        (
            br#"interface Drawable:
    fn draw(read self, value: i64)

struct Card implements Drawable:
    fn draw(read self, value: bool):
        pass

@image
fn build() -> Image:
    return Image.new()
"#,
            "semantic.interface_signature_mismatch",
        ),
        (
            br#"comptime assert unknown_name

@image
fn build() -> Image:
    return Image.new()
"#,
            "semantic.invalid_comptime_expression",
        ),
        (
            br#"fn inspect(read value: i64):
    pass

@image
fn build() -> Image:
    return Image.new()
"#,
            "semantic.ownership_mode_requires_resource",
        ),
        (
            br#"enum Failure:
    Broken

pub fn work() -> Result[i64]:
    return Result.Err(Failure.Broken)

@image
fn build() -> Image:
    return Image.new()
"#,
            "semantic.public_result_requires_error_type",
        ),
        (
            br#"resource struct Ticket:
    bytes: Bytes

struct Holder:
    ticket: Ticket

@image
fn build() -> Image:
    return Image.new()
"#,
            "semantic.resource_field_requires_resource_struct",
        ),
        (
            br#"resource struct Ticket:
    bytes: Bytes

fn inspect(ticket: Ticket):
    pass

@image
fn build() -> Image:
    return Image.new()
"#,
            "semantic.resource_parameter_requires_mode",
        ),
        (
            br#"fn work() -> Result[i64]:
    return Result.Ok(1)

@image
fn build() -> Image:
    return Image.new()
"#,
            "semantic.unconstrained_inferred_error",
        ),
        (
            br#"@unknown
fn helper():
    pass

@image
fn build() -> Image:
    return Image.new()
"#,
            "semantic.unknown_attribute",
        ),
        (
            br#"struct Card implements Missing:
    value: i64

@image
fn build() -> Image:
    return Image.new()
"#,
            "semantic.unresolved_interface",
        ),
    ];
    for (source, code) in cases {
        assert_code(image(source), code);
    }
}

#[test]
fn namespace_and_image_constructor_diagnostics_are_directly_observable() {
    assert_code(
        compile(
            vec![
                ProjectFile::new(
                    "src/image.wr",
                    b"from game import cards\n\nconst cards: i64 = 1\n",
                ),
                ProjectFile::new("src/game/cards.wr", b""),
            ],
            Root::Image,
        ),
        "semantic.import_alias_conflict",
    );
    assert_code(
        image(b"fn helper():\n    pass\n"),
        "semantic.missing_image_constructor",
    );
    assert_code(
        image(
            br#"@image
fn first() -> Image:
    return Image.new()

@image
fn second() -> Image:
    return Image.new()
"#,
        ),
        "semantic.multiple_image_constructors",
    );
    assert_code(
        compile(
            vec![
                ProjectFile::new("src/image.wr", b"from game import boot\n"),
                ProjectFile::new(
                    "src/game/boot.wr",
                    b"@image\nfn build() -> Image:\n    return Image.new()\n",
                ),
            ],
            Root::Image,
        ),
        "semantic.image_constructor_outside_root",
    );
}

#[test]
fn test_declaration_diagnostics_are_directly_observable() {
    assert_code(
        test_image(
            br#"pub suite behavior:
    test works():
        expect true
    test works():
        expect true

@image
fn build() -> Image:
    return Image.new()
"#,
        ),
        "test.duplicate_declaration",
    );
    assert_code(
        test_image(
            br#"suite behavior:
    test works():
        expect true

@image
fn build() -> Image:
    return Image.new()
"#,
        ),
        "test.suite_must_be_public",
    );
}
