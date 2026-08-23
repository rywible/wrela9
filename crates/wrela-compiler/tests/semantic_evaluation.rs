use wrela_compiler::{
    Cancellation, CanonicalValue, CompilationOutcome, CompilationRequest, Compiler,
    CompilerInstallation, EvaluationOutcome, InspectSelection, ProjectFile, ProjectSnapshot, Root,
};

fn compile(source: &[u8]) -> CompilationOutcome {
    let compiler = Compiler::open(CompilerInstallation::empty()).expect("distribution opens");
    compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    )
}

#[test]
fn pure_constants_and_image_construction_share_the_verified_evaluator() {
    let source = br#"const ANSWER: i64 = add(40, 2)

pure fn add(left: i64, right: i64) -> i64:
    return left + right

@image
fn build() -> Image:
    return Image.new(answer=ANSWER)
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("valid pure program must compile");
    };
    let answer = accepted
        .inspection()
        .evaluations()
        .iter()
        .find(|evaluation| evaluation.root() == "image.ANSWER")
        .expect("constant evaluation");
    assert_eq!(
        answer.outcome(),
        &EvaluationOutcome::Completed(CanonicalValue::Integer {
            type_name: "i64".into(),
            value: 42,
        })
    );
    assert!(answer.receipt().fuel_used() > 0);
    assert_eq!(accepted.inspection().constructions().len(), 1);
    assert_eq!(accepted.inspection().constructions()[0].kind(), "Image");
    let add = accepted
        .inspection()
        .function_facts()
        .iter()
        .find(|facts| facts.name() == "add")
        .expect("function facts");
    assert!(add.is_pure());
    assert!(add.evaluator_eligible());
    assert!(!add.may_panic());
}

#[test]
fn failed_compile_time_assertion_is_a_creator_rejection() {
    let source = br#"comptime assert 2 + 2 == 5

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("failed assertion must reject");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "evaluation.assertion_failed")
    );
}

#[test]
fn public_signature_cannot_expose_a_private_nominal_type() {
    let source = br#"struct Secret:
    value: i64

pub fn reveal() -> Secret:
    return Secret(value=1)

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("private type exposure must reject");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.private_type_in_public_signature")
    );
}

#[test]
fn private_result_infers_one_nominal_error() {
    let source = br#"enum ReadError:
    Missing

fn read() -> Result[i64]:
    return Result.Err(ReadError.Missing)

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("one nominal error must infer");
    };
    let inferred = accepted
        .inspection()
        .inferred_errors()
        .iter()
        .find(|error| error.function() == "read")
        .expect("inferred error");
    assert_eq!(inferred.error_type(), "ReadError");
}

#[test]
fn private_result_rejects_conflicting_nominal_errors() {
    let source = br#"enum ReadError:
    Missing

enum ParseError:
    Invalid

fn broken(flag: bool) -> Result[i64]:
    if flag:
        return Result.Err(ReadError.Missing)
    return Result.Err(ParseError.Invalid)

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("conflicting errors must reject");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.conflicting_inferred_errors")
    );
}

#[test]
fn private_error_inference_solves_recursive_call_facts_independent_of_order() {
    let source = br#"enum ReadError:
    Missing

fn outer() -> Result[i64]:
    return inner()?

fn inner() -> Result[i64]:
    return Result.Err(ReadError.Missing)

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("propagated nominal error must infer");
    };
    assert_eq!(
        accepted
            .inspection()
            .inferred_errors()
            .iter()
            .map(|error| (error.function(), error.error_type()))
            .collect::<Vec<_>>(),
        [("inner", "ReadError"), ("outer", "ReadError")]
    );
}

#[test]
fn exact_propagation_rejects_implicit_error_conversion() {
    let source = br#"enum ReadError:
    Missing

enum AppError:
    Failed

fn read() -> Result[i64, ReadError]:
    return Result.Err(ReadError.Missing)

fn load() -> Result[i64, AppError]:
    return read()?

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("implicit conversion must reject");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.propagation_error_mismatch")
    );
}

#[test]
fn fixed_width_numeric_literals_include_f16_and_checked_integer_overflow() {
    let valid = br#"const BYTE: u8 = 255u8
const HALF: f16 = 1.5f16

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Accepted(accepted) = compile(valid) else {
        panic!("fixed-width literals must compile");
    };
    assert!(
        accepted
            .inspection()
            .evaluations()
            .iter()
            .any(|evaluation| {
                matches!(
                    evaluation.outcome(),
                    EvaluationOutcome::Completed(CanonicalValue::Float { type_name, bits })
                        if type_name.as_ref() == "f16" && *bits == 0x3e00
                )
            })
    );

    let overflow = br#"const BAD: u8 = 255u8 + 1u8

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(overflow) else {
        panic!("checked overflow must reject constant evaluation");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "evaluation.panicked")
    );
}

#[test]
fn constant_dependency_cycles_are_structured_creator_rejections() {
    let source = br#"const FIRST: i64 = SECOND
const SECOND: i64 = FIRST

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("constant cycle must reject");
    };
    assert!(rejected.diagnostics().iter().any(|diagnostic| {
        diagnostic.code() == "evaluation.rejected"
            && diagnostic.parameters().iter().any(|(name, value)| {
                name.as_ref() == "kind" && value.as_ref() == "constant_dependency_cycle"
            })
    }));
}

#[test]
fn demanded_generic_specialization_uses_concrete_wrela_types() {
    let source = br#"pure fn identity[T](value: T) -> T:
    return value

const ANSWER: i64 = identity(42)

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("demanded generic must specialize");
    };
    let answer = accepted
        .inspection()
        .evaluations()
        .iter()
        .find(|evaluation| evaluation.root() == "image.ANSWER")
        .expect("answer");
    assert_eq!(
        answer.outcome(),
        &EvaluationOutcome::Completed(CanonicalValue::Integer {
            type_name: "i64".into(),
            value: 42,
        })
    );
}

#[test]
fn unbounded_recursion_is_contained_by_a_logical_limit() {
    let source = br#"fn recurse() -> i64:
    return recurse()

const VALUE: i64 = recurse()

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("unbounded recursion must reject without a host overflow");
    };
    assert!(rejected.diagnostics().iter().any(|diagnostic| {
        diagnostic.code() == "evaluation.limit_exceeded"
            && diagnostic
                .parameters()
                .iter()
                .any(|(name, value)| name.as_ref() == "policy" && value.as_ref() == "call_depth")
    }));
}

#[test]
fn build_constructors_have_no_authority_outside_the_image_call_chain() {
    let source = br#"const FORGED: Image = Image.new()

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("constant evaluation cannot construct graph nodes");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.build_constructor_outside_image")
    );
}
