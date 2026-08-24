use wrela_compiler::{
    Cancellation, CanonicalValue, CompilationOutcome, CompilationRequest, Compiler,
    CompilerInstallation, ConstructionKind, Diagnostic, DiagnosticValue, EvaluationOutcome,
    EvaluationPolicy, IdentityDomain, InspectSelection, ProjectFile, ProjectSnapshot, Root,
};

fn has_text_parameter(diagnostic: &Diagnostic, name: &str, expected: &str) -> bool {
    diagnostic
        .typed_parameters()
        .iter()
        .any(|(parameter, value)| {
            parameter.as_ref() == name
                && matches!(value, DiagnosticValue::Text(value) if value.as_ref() == expected)
        })
}

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
fn image_constructor_cannot_fall_through_without_returning_an_image() {
    let source = br#"@image
fn build() -> Image:
    pass
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("a non-Unit Image Constructor must definitely return its Image root");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.missing_return")
    );
}

#[test]
fn image_constructor_must_return_image() {
    let source = br#"@image
fn build() -> i64:
    return 1
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("the Image Constructor return type must be Image");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.invalid_image_constructor_signature")
    );
}

#[test]
fn image_constructor_contract_rejects_parameters_generics_and_suspension() {
    for source in [
        b"@image\nfn build(value: i64) -> Image:\n    return Image.new()\n".as_slice(),
        b"@image\nfn build[T]() -> Image:\n    return Image.new()\n".as_slice(),
        b"@image\nasync fn build() -> Image:\n    return Image.new()\n".as_slice(),
    ] {
        let CompilationOutcome::Rejected(rejected) = compile(source) else {
            panic!("invalid Image Constructor contract must reject");
        };
        assert!(rejected.diagnostics().iter().any(|diagnostic| {
            diagnostic.code() == "semantic.invalid_image_constructor_signature"
        }));
    }
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
    assert_eq!(
        accepted.inspection().constructions()[0].kind(),
        ConstructionKind::Image
    );
    let add = accepted
        .inspection()
        .function_facts()
        .iter()
        .find(|facts| facts.name() == "add")
        .expect("function facts");
    assert!(add.is_pure());
    assert!(add.evaluator_eligible());
    assert!(add.may_panic());
    assert!(accepted.inspection().identities().iter().any(|identity| {
        identity.domain() == IdentityDomain::Specialization && identity.digest() == add.identity()
    }));
}

#[test]
fn evaluation_receipts_name_policy_root_transitive_dependencies_and_tariff() {
    let source = br#"const BASE: i64 = 40
const ANSWER: i64 = BASE + 2

@image
fn build() -> Image:
    return Image.new(answer=ANSWER)
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("receipt fixture must compile");
    };
    let answer = accepted
        .inspection()
        .evaluations()
        .iter()
        .find(|evaluation| evaluation.root() == "image.ANSWER")
        .expect("ANSWER evaluation");
    assert_eq!(answer.receipt().policy(), EvaluationPolicy::Constant);
    assert_eq!(
        answer.receipt().tariff_schema(),
        "wrela.evaluator.tariff.v2"
    );
    assert_ne!(answer.receipt().root_identity(), 0);
    assert!(answer.receipt().evaluator_eligible());
    assert_eq!(answer.receipt().dependency_roots().len(), 2);
    assert!(answer.receipt().fuel_used() > 0);
    assert!(answer.receipt().peak_memory() > 0);

    let image = accepted
        .inspection()
        .evaluations()
        .iter()
        .find(|evaluation| evaluation.root() == "image.build")
        .expect("Image Constructor evaluation");
    assert_eq!(image.receipt().policy(), EvaluationPolicy::ImageConstructor);
    assert_eq!(image.receipt().dependency_roots().len(), 2);
    assert_eq!(
        image.receipt().typed_hir_fingerprint(),
        answer.receipt().typed_hir_fingerprint()
    );
    assert_eq!(
        image.receipt().argument_fingerprint(),
        answer.receipt().argument_fingerprint()
    );
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

fn fetch() -> Result[i64]:
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
        .find(|error| error.function() == "fetch")
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
                && has_text_parameter(diagnostic, "kind", "unresolved_call")
        }),
        "{:#?}",
        rejected.diagnostics()
    );
}

#[test]
fn variants_enforce_their_declared_payload_arity() {
    let builtin = br#"const BAD: Option[i64] = Option.Some(1, 2)

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(builtin) else {
        panic!("a built-in variant must reject extra payload values");
    };
    assert!(rejected.diagnostics().iter().any(|diagnostic| {
        diagnostic.code() == "semantic.invalid_typed_hir"
            && has_text_parameter(diagnostic, "kind", "argument_count")
    }));

    let creator = br#"enum ReadError:
    Missing

fn broken() -> ReadError:
    return ReadError.Missing(1)

@image
fn build() -> Image:
    return Image.new()
"#;
    assert!(matches!(compile(creator), CompilationOutcome::Rejected(_)));
}

#[test]
fn callable_argument_labels_are_part_of_the_signature() {
    let source = br#"fn add(left: i64, right: i64) -> i64:
    return left + right

const BAD: i64 = add(lfet=1, right=2)

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("a misspelled argument label cannot bind positionally");
    };
    assert!(rejected.diagnostics().iter().any(|diagnostic| {
        diagnostic.code() == "semantic.invalid_typed_hir"
            && has_text_parameter(diagnostic, "kind", "argument_label_mismatch")
    }));
}

#[test]
fn every_callable_family_uses_the_same_arity_label_and_type_rules() {
    let cases: &[(&[u8], &str)] = &[
        (
            br#"fn add(left: i64, right: i64) -> i64:
    return left + right

const BAD: i64 = add(1)

@image
fn build() -> Image:
    return Image.new()
"#,
            "argument_count",
        ),
        (
            br#"struct Card:
    value: i64

const BAD: Card = Card(points=1)

@image
fn build() -> Image:
    return Image.new()
"#,
            "argument_label_mismatch",
        ),
        (
            br#"enum ReadError:
    Missing(path: Text)

fn broken() -> ReadError:
    return ReadError.Missing(path=1)

@image
fn build() -> Image:
    return Image.new()
"#,
            "argument_type_mismatch",
        ),
        (
            br#"const BAD: Option[i64] = Option.Some(item=1)

@image
fn build() -> Image:
    return Image.new()
"#,
            "argument_label_mismatch",
        ),
    ];
    for (source, expected) in cases {
        let CompilationOutcome::Rejected(rejected) = compile(source) else {
            panic!("invalid callable application must reject");
        };
        assert!(rejected.diagnostics().iter().any(|diagnostic| {
            diagnostic.code() == "semantic.invalid_typed_hir"
                && has_text_parameter(diagnostic, "kind", expected)
        }));
    }
}

#[test]
fn creator_variant_payloads_are_parsed_and_type_checked() {
    let valid = br#"enum ReadError:
    Missing(path: Text)

fn error() -> ReadError:
    return ReadError.Missing("cards.wr")

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(valid);
    assert!(
        matches!(outcome, CompilationOutcome::Accepted(_)),
        "a declared named payload is real enum semantics: {outcome:#?}"
    );

    let invalid = br#"enum ReadError:
    Missing(path: Text)

fn error() -> ReadError:
    return ReadError.Missing(1)

@image
fn build() -> Image:
    return Image.new()
"#;
    assert!(matches!(compile(invalid), CompilationOutcome::Rejected(_)));
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
fn result_propagation_rewraps_success_and_preserves_the_exact_error() {
    let source = br#"enum ReadError:
    Missing

fn succeeds() -> Result[i64, ReadError]:
    return Result.Ok(7)

fn fails() -> Result[i64, ReadError]:
    return Result.Err(ReadError.Missing)

fn success_outer() -> Result[i64, ReadError]:
    return succeeds()?

fn error_outer() -> Result[i64, ReadError]:
    return fails()?

const SUCCESS: Result[i64, ReadError] = success_outer()
const ERROR: Result[i64, ReadError] = error_outer()

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("valid propagation must complete as an ordinary Result value: {outcome:#?}");
    };
    let evaluation = |name: &str| {
        accepted
            .inspection()
            .evaluations()
            .iter()
            .find(|evaluation| evaluation.root() == name)
            .expect("constant evaluation")
            .outcome()
    };
    assert_eq!(
        evaluation("image.SUCCESS"),
        &EvaluationOutcome::Completed(CanonicalValue::Variant {
            type_name: "Result".into(),
            variant: "Ok".into(),
            payload: vec![CanonicalValue::Integer {
                type_name: "i64".into(),
                value: 7,
            }]
            .into(),
        })
    );
    assert_eq!(
        evaluation("image.ERROR"),
        &EvaluationOutcome::Completed(CanonicalValue::Variant {
            type_name: "Result".into(),
            variant: "Err".into(),
            payload: vec![CanonicalValue::Variant {
                type_name: "ReadError".into(),
                variant: "Missing".into(),
                payload: Vec::new().into(),
            }]
            .into(),
        })
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

fn fetch() -> Result[i64, ReadError]:
    return Result.Err(ReadError.Missing)

fn load() -> Result[i64, AppError]:
    return fetch()?

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
fn numeric_formats_do_not_implicitly_convert_or_reinterpret_bits() {
    let mixed_float = br#"const BAD: f16 = 1.0f16 + 2.0f32

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(mixed_float) else {
        panic!("mixed float formats require an explicit conversion");
    };
    assert!(rejected.diagnostics().iter().any(|diagnostic| {
        diagnostic.code() == "semantic.invalid_typed_hir"
            && has_text_parameter(diagnostic, "kind", "binary_type_mismatch")
    }));

    let mixed_integer = br#"const BAD: u16 = 1u8 + 2u16

@image
fn build() -> Image:
    return Image.new()
"#;
    assert!(matches!(
        compile(mixed_integer),
        CompilationOutcome::Rejected(_)
    ));
}

#[test]
fn every_distinct_numeric_format_pair_requires_explicit_conversion() {
    let integer_formats = ["u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64"];
    for left in integer_formats {
        for right in integer_formats {
            if left == right {
                continue;
            }
            let source = format!(
                "const BAD: {left} = 1{left} + 1{right}\n\n@image\nfn build() -> Image:\n    return Image.new()\n"
            );
            assert!(
                matches!(compile(source.as_bytes()), CompilationOutcome::Rejected(_)),
                "{left} and {right} must not convert implicitly"
            );
        }
    }

    let float_formats = ["f16", "f32", "f64"];
    for left in float_formats {
        for right in float_formats {
            if left == right {
                continue;
            }
            let source = format!(
                "const BAD: {left} = 1.0{left} + 1.0{right}\n\n@image\nfn build() -> Image:\n    return Image.new()\n"
            );
            assert!(
                matches!(compile(source.as_bytes()), CompilationOutcome::Rejected(_)),
                "{left} and {right} must not convert implicitly"
            );
        }
    }
}

#[test]
fn unsigned_negation_rejects_and_signed_minimum_literals_are_valid() {
    let unsigned = br#"const BAD: u8 = -1u8

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(unsigned) else {
        panic!("an unsigned integer cannot be negated");
    };
    assert!(rejected.diagnostics().iter().any(|diagnostic| {
        diagnostic.code() == "semantic.invalid_typed_hir"
            && has_text_parameter(diagnostic, "kind", "invalid_unary_operand")
    }));

    let signed_minimum = br#"const MINIMUM: i8 = -128i8

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(signed_minimum);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("the minimum signed value is one valid unary expression: {outcome:#?}");
    };
    assert!(
        accepted
            .inspection()
            .evaluations()
            .iter()
            .any(|evaluation| {
                evaluation.root() == "image.MINIMUM"
                    && evaluation.outcome()
                        == &EvaluationOutcome::Completed(CanonicalValue::Integer {
                            type_name: "i8".into(),
                            value: -128,
                        })
            })
    );
}

#[test]
fn every_signed_minimum_is_a_valid_checked_unary_expression() {
    for (type_name, magnitude) in [
        ("i8", "128"),
        ("i16", "32768"),
        ("i32", "2147483648"),
        ("i64", "9223372036854775808"),
    ] {
        let source = format!(
            "const MINIMUM: {type_name} = -{magnitude}{type_name}\n\n@image\nfn build() -> Image:\n    return Image.new()\n"
        );
        assert!(
            matches!(compile(source.as_bytes()), CompilationOutcome::Accepted(_)),
            "the exact minimum {type_name} literal must compile"
        );
    }
}

#[test]
fn source_read_after_move_is_a_creator_rejection_not_a_compiler_defect() {
    let source = br#"fn consume(take value: i64):
    pass

fn broken(take value: i64) -> i64:
    consume(value)
    return value

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(source);
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("ordinary source ownership misuse must be rejected: {outcome:#?}");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.read_after_move")
    );
}

#[test]
fn a_move_in_a_branch_that_definitely_returns_does_not_poison_the_join() {
    let valid = br#"resource struct Ticket:
    id: i64

fn consume(take ticket: Ticket):
    pass

fn choose(flag: bool, take ticket: Ticket) -> Ticket:
    if flag:
        consume(ticket)
        return Ticket(id=0)
    return ticket

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(valid);
    assert!(
        matches!(outcome, CompilationOutcome::Accepted(_)),
        "{outcome:#?}"
    );

    let invalid = br#"resource struct Ticket:
    id: i64

fn consume(take ticket: Ticket):
    pass

fn broken(flag: bool, take ticket: Ticket) -> Ticket:
    if flag:
        consume(ticket)
    return ticket

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(invalid) else {
        panic!("a live branch move must reject the later read");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == "semantic.read_after_move" })
    );
}

#[test]
fn locals_are_immutable_unless_their_first_binding_is_mutable() {
    let immutable = br#"fn broken() -> i64:
    value = 1
    value = 2
    return value

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(immutable) else {
        panic!("plain assignment cannot silently shadow an immutable local");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.immutable_reassignment")
    );

    let mutable = br#"fn answer() -> i64:
    mut value = 1
    value = 2
    return value

const ANSWER: i64 = answer()

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(mutable);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("an explicitly mutable local may be reassigned: {outcome:#?}");
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
                            value: 2,
                        })
            })
    );
}

#[test]
fn suspension_requires_async_and_pure_is_an_enforced_ceiling() {
    let non_async = br#"async fn later() -> i64:
    return 1

fn broken() -> i64:
    return await later()

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(non_async) else {
        panic!("await is legal only in an async function");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.await_requires_async")
    );

    let impure = br#"async fn later() -> i64:
    return 1

pure fn broken() -> i64:
    return later()

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(impure) else {
        panic!("pure is a checked ceiling rather than documentation");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.pure_effect_violation")
    );
}

#[test]
fn transparent_type_aliases_normalize_to_their_target() {
    let source = br#"type Count = i64
type Score = Count

fn identity(value: Score) -> Count:
    return value

const ANSWER: i64 = identity(42i64)

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("transparent alias chains must retain the target type semantics: {outcome:#?}");
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

    let recursive = br#"type Left = Right
type Right = Left

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(recursive) else {
        panic!("a recursive transparent alias has no normal form");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.recursive_type_alias")
    );
}

#[test]
fn accepted_named_declarations_validate_their_complete_semantics() {
    let unresolved_field = br#"struct Card:
    value: MissingType

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(unresolved_field) else {
        panic!("a struct field cannot be accepted as inert text");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.unresolved_type")
    );

    let valid = br#"type CardId = u64

struct Card:
    pub id: CardId
    pub mut power: i32

resource struct PendingWrite:
    buffer: Bytes

interface Drawable:
    fn bounds() -> i64

pool Entities

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(valid);
    assert!(
        matches!(outcome, CompilationOutcome::Accepted(_)),
        "complete named declaration syntax and types must compile: {outcome:#?}"
    );
}

#[test]
fn associated_functions_and_direct_struct_construction_are_native_wrela_semantics() {
    let source = br#"struct Card:
    pub value: i64
    pure fn new(value: i64) -> Card:
        return Card(value=value)

const CARD: Card = Card.new(value=7)

@image
fn build() -> Image:
    return Image.new(card=CARD)
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("associated functions and direct construction must compile");
    };
    let card = accepted
        .inspection()
        .evaluations()
        .iter()
        .find(|evaluation| evaluation.root() == "image.CARD")
        .expect("Card constant is evaluated");
    assert_eq!(
        card.outcome(),
        &EvaluationOutcome::Completed(CanonicalValue::Struct {
            type_name: "Card".into(),
            fields: vec![(
                "value".into(),
                CanonicalValue::Integer {
                    type_name: "i64".into(),
                    value: 7,
                },
            )]
            .into(),
        })
    );
}

#[test]
fn explicit_interfaces_require_matching_native_wrela_methods() {
    let valid = br#"interface Drawable:
    pure fn bounds(read self) -> i64

struct Card implements Drawable:
    value: i64
    pure fn bounds(read self) -> i64:
        return 1

pure fn card_bounds() -> i64:
    card = Card(value=7)
    return card.bounds()

const BOUNDS: i64 = card_bounds()

@image
fn build() -> Image:
    return Image.new(bounds=BOUNDS)
"#;
    assert!(matches!(compile(valid), CompilationOutcome::Accepted(_)));

    let missing = br#"interface Drawable:
    fn bounds(read self) -> i64

struct Card implements Drawable:
    value: i64

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(missing) else {
        panic!("explicit conformance requires every declared method");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == "semantic.missing_interface_requirement" })
    );
}

#[test]
fn named_arguments_may_reorder_without_changing_source_evaluation_order() {
    let source = br#"pure fn subtract(left: i64, right: i64) -> i64:
    return left - right

const RESULT: i64 = subtract(right=2, left=9)

@image
fn build() -> Image:
    return Image.new(result=RESULT)
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("named call arguments bind by label");
    };
    assert!(
        accepted
            .inspection()
            .evaluations()
            .iter()
            .any(|evaluation| {
                evaluation.root() == "image.RESULT"
                    && evaluation.outcome()
                        == &EvaluationOutcome::Completed(CanonicalValue::Integer {
                            type_name: "i64".into(),
                            value: 7,
                        })
            })
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
            && has_text_parameter(diagnostic, "kind", "constant_dependency_cycle")
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
fn nested_generic_parameters_bind_through_option_array_and_result() {
    let source = br#"struct Failure:
    code: i64

pure fn keep_option[T](value: Option[T]) -> Option[T]:
    return value

pure fn keep_array[T](value: [T]) -> [T]:
    return value

pure fn keep_result[T](value: Result[T, Failure]) -> Result[T, Failure]:
    return value

const OPTION: Option[i64] = keep_option(Option.Some(1))
const ARRAY: [i64] = keep_array([1, 2])
const RESULT: Result[i64, Failure] = keep_result(Result.Ok(3))

@image
fn build() -> Image:
    return Image.new(option=OPTION, array=ARRAY, result=RESULT)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("nested generic binding must specialize: {outcome:#?}");
    };
    assert_eq!(
        accepted
            .inspection()
            .specializations()
            .iter()
            .filter(|specialization| specialization.function().starts_with("keep_"))
            .count(),
        3
    );
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

#[test]
fn logical_cost_counts_repeated_calls_and_is_reported_per_specialization() {
    let source = br#"pure fn leaf[T](value: T) -> T:
    return value

pure fn once() -> i64:
    return leaf(1)

pure fn twice() -> i64:
    first = leaf(1)
    return first + leaf(2)

const A: i64 = once()
const B: i64 = twice()
const C: bool = leaf(true)

@image
fn build() -> Image:
    return Image.new(a=A, b=B, c=C)
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("weighted specialization facts must compile");
    };
    let facts = accepted.inspection().function_facts();
    let once = facts
        .iter()
        .find(|facts| facts.name() == "once")
        .expect("once facts");
    let twice = facts
        .iter()
        .find(|facts| facts.name() == "twice")
        .expect("twice facts");
    assert!(twice.logical_cost() > once.logical_cost());
    let leaf = facts
        .iter()
        .filter(|facts| facts.name() == "leaf")
        .collect::<Vec<_>>();
    assert_eq!(leaf.len(), 2, "i64 and bool have distinct concrete facts");
    assert_ne!(leaf[0].identity(), leaf[1].identity());
    assert!(leaf.iter().all(|facts| facts.logical_cost() > 0));
}
