use wrela_compiler::{
    Cancellation, CanonicalValue, CompilationOutcome, CompilationRequest, Compiler,
    CompilerInstallation, DiagnosticValue, EvaluationOutcome, IdentityDomain, InspectSelection,
    ProjectFile, ProjectSnapshot, Root,
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
    assert!(accepted.inspection().identities().iter().any(|identity| {
        identity.domain() == IdentityDomain::Definition
            && identity.name() == "add"
            && identity.digest() == add.identity()
    }));
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
fn comptime_generic_demand_materializes_one_shared_specialization() {
    let source = br#"pure fn identity[T](value: T) -> T:
    return value

comptime assert identity(42) == 42

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("comptime generic demand must materialize before evaluation");
    };
    assert!(
        accepted
            .inspection()
            .specializations()
            .iter()
            .any(|specialization| {
                specialization.function() == "identity"
                    && specialization
                        .argument_types()
                        .iter()
                        .map(AsRef::as_ref)
                        .eq(["i64"])
            })
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
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("one nominal error must infer: {outcome:#?}");
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
fn creator_variants_must_be_declared_by_their_enum() {
    let source = br#"enum ReadError:
    Missing

fn broken() -> ReadError:
    return ReadError.NotDeclared

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("an undeclared creator variant must reject");
    };
    assert!(
        rejected.diagnostics().iter().any(|diagnostic| {
            diagnostic.code() == "semantic.invalid_typed_hir"
                && diagnostic.parameters().iter().any(|(name, value)| {
                    name.as_ref() == "kind" && value.as_ref() == "unresolved_call"
                })
        }),
        "{:#?}",
        rejected.diagnostics()
    );
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
fn verified_control_flow_preserves_branch_evaluation() {
    let source = br#"fn choose(flag: bool) -> i64:
    if flag:
        return 41
    else:
        return 42

const ANSWER: i64 = choose(false)

@image
fn build() -> Image:
    return Image.new(answer=ANSWER)
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("typed control flow must compile and evaluate");
    };
    assert!(
        accepted
            .inspection()
            .evaluations()
            .iter()
            .any(|evaluation| {
                evaluation.root() == "image.ANSWER"
                    && evaluation.outcome()
                        == &EvaluationOutcome::Completed(CanonicalValue::Integer {
                            type_name: "i64".into(),
                            value: 42,
                        })
            })
    );
}

#[test]
fn typed_hir_fingerprint_includes_literal_payloads_and_operations() {
    fn fingerprint(value: i32, operator: &str) -> u128 {
        let source = format!(
            "const ANSWER: i64 = {value} {operator} 1\n\n@image\nfn build() -> Image:\n    return Image.new()\n"
        );
        let CompilationOutcome::Accepted(accepted) = compile(source.as_bytes()) else {
            panic!("fingerprint fixture must compile");
        };
        accepted
            .inspection()
            .evaluations()
            .iter()
            .find(|evaluation| evaluation.root() == "image.ANSWER")
            .expect("constant evaluation")
            .receipt()
            .typed_hir_fingerprint()
    }

    assert_ne!(fingerprint(1, "+"), fingerprint(2, "+"));
    assert_ne!(fingerprint(1, "+"), fingerprint(1, "*"));

    fn signature_fingerprint(parameter: &str) -> u128 {
        let source = format!(
            "pure fn unused(value: {parameter}) -> {parameter}:\n    return value\n\n@image\nfn build() -> Image:\n    return Image.new()\n"
        );
        let CompilationOutcome::Accepted(accepted) = compile(source.as_bytes()) else {
            panic!("signature fingerprint fixture must compile");
        };
        accepted.inspection().evaluations()[0]
            .receipt()
            .typed_hir_fingerprint()
    }

    assert_ne!(signature_fingerprint("i32"), signature_fingerprint("i64"));
}

#[test]
fn recursive_facts_propagate_panic_and_evaluator_ineligibility() {
    let source = br#"fn dangerous() -> i64:
    panic "boom"

fn wrapper() -> i64:
    return dangerous()

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("unreached panic remains valid Wrela");
    };
    let wrapper = accepted
        .inspection()
        .function_facts()
        .iter()
        .find(|facts| facts.name() == "wrapper")
        .expect("wrapper facts");
    assert!(wrapper.may_panic());
}

#[test]
fn duplicate_declarations_are_rejected_in_one_module_namespace() {
    let source = br#"fn answer() -> i64:
    return 41

const answer: i64 = 42

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("duplicate declarations must reject");
    };
    let diagnostic = rejected
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.code() == "semantic.duplicate_declaration")
        .expect("structured duplicate diagnostic");
    assert_eq!(diagnostic.labels().len(), 1);
    assert_eq!(diagnostic.labels()[0].role(), "previous_declaration");
    assert!(diagnostic.typed_parameters().iter().any(|(name, value)| {
        name.as_ref() == "definition"
            && matches!(
                value,
                DiagnosticValue::Identity {
                    domain: IdentityDomain::Definition,
                    ..
                }
            )
    }));
}

#[test]
fn comments_and_text_cannot_fabricate_semantic_calls_or_effects() {
    let source = br#"fn harmless() -> Text:
    # panic "not real"; missing()?; Result.Err(Fake.Bad)
    return "Image.new( / missing()? )"

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("trivia and text are semantically inert");
    };
    let harmless = accepted
        .inspection()
        .function_facts()
        .iter()
        .find(|facts| facts.name() == "harmless")
        .expect("harmless facts");
    assert!(!harmless.may_panic());
    assert!(harmless.is_pure());
}

#[test]
fn symbolic_graph_sealing_rejects_multiple_image_roots() {
    let source = br#"fn child() -> Image:
    return Image.new()

@image
fn build() -> Image:
    return Image.new(child=child())
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("one evaluation may seal exactly one Image root");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "construction.invalid_graph")
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
    let identity = accepted
        .inspection()
        .specializations()
        .iter()
        .find(|specialization| specialization.function() == "identity")
        .expect("concrete identity body");
    assert_eq!(
        identity
            .argument_types()
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>(),
        ["i64"]
    );
    assert!(accepted.inspection().identities().iter().any(|identity| {
        identity.domain() == wrela_compiler::IdentityDomain::Specialization
            && identity.digest()
                == accepted
                    .inspection()
                    .specializations()
                    .iter()
                    .find(|specialization| specialization.function() == "identity")
                    .expect("identity specialization")
                    .identity()
    }));
    assert!(accepted.inspection().types().iter().any(|type_| {
        type_.name() == "value"
            && type_.role() == wrela_compiler::TypeRole::Parameter
            && type_.type_name() == "T"
    }));
    let facts = accepted
        .inspection()
        .function_facts()
        .iter()
        .find(|facts| facts.name() == "identity")
        .expect("identity facts");
    assert!(facts.is_bounded());
    assert!(facts.logical_cost() > 0);
}

#[test]
fn unbounded_recursion_is_rejected_before_evaluation() {
    let source = br#"fn recurse() -> i64:
    return recurse()

const VALUE: i64 = recurse()

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("unbounded recursion must reject before evaluator containment");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.unproven_recursive_bound")
    );
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
