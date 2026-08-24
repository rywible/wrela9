use std::sync::Arc;

use wrela_compiler::{
    Cancellation, CanonicalValue, CompilationOutcome, CompilationRequest, Compiler,
    CompilerInstallation, ConstructionKind, Diagnostic, DiagnosticValue, EvaluationOutcome,
    EvaluationPolicy, IdentityDomain, InspectSelection, ProjectFile, ProjectSnapshot, Root,
    SyntaxNodeKind,
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
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
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
fn comptime_if_selects_declarations_before_name_resolution_and_evaluation() {
    let source = br#"const DEBUG: bool = false

comptime if DEBUG:
    const VALUE: i64 = missing_in_unselected_branch
else:
    const VALUE: i64 = 42

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;

    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("only the selected compile-time declaration branch must enter semantics");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 42
            )
    }));
}

#[test]
fn comptime_elif_uses_the_bounded_evaluator_and_is_declaration_order_independent() {
    let source = br#"comptime if not enabled():
    const VALUE: i64 = 1
elif enabled():
    const VALUE: i64 = 2
else:
    const VALUE: i64 = 3

pure fn enabled() -> bool:
    return true

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;

    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("compile-time branches may call later pure declarations");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 2
            )
    }));
}

#[test]
fn accepted_layer_one_structural_types_resolve_through_the_compiler_seam() {
    let source = br#"pool storage

resource struct Ticket:
    value: i64

interface Shape:
    fn area() -> i64

fn typed(
    callback: fn(i64) -> i64,
    take owned: own[storage] Ticket,
    erased: any Shape,
    fixed: [i64; 4],
):
    pass

@image
fn build() -> Image:
    return Image.new()
"#;

    let outcome = compile(source);
    assert!(
        matches!(outcome, CompilationOutcome::Accepted(_)),
        "every accepted Layer 1 structural type must resolve: {outcome:#?}"
    );
}

#[test]
fn bounded_for_and_repeated_arrays_are_evaluated_as_initial_layer_one_wrela() {
    let source = br#"pure fn total() -> i64:
    mut sum = 0
    for value in [2; 4]:
        sum += value
    return sum

const VALUE: i64 = total()

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;

    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("a finite collection iteration must compile and evaluate");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 8
            )
    }));
}

#[test]
fn bounded_integer_ranges_are_evaluated_in_source_order() {
    let source = br#"pure fn total() -> i64:
    mut sum = 0
    for value in 1..=4:
        sum = sum + value
    return sum

const VALUE: i64 = total()

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("a finite integer range must compile and evaluate");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 10
            )
    }));
}

#[test]
fn nearest_loop_exits_apply_to_bounded_for_without_unrolling_control_storage() {
    let source = br#"pure fn total() -> i64:
    mut sum = 0
    for value in 1..=6:
        if value == 2:
            continue
        if value == 5:
            break
        sum += value
    return sum

const VALUE: i64 = total()

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("nearest-loop exits must work for bounded collection iteration: {outcome:#?}");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 8
            )
    }));
}

#[test]
fn source_ordered_exhaustive_match_is_verified_and_evaluated() {
    let source = br#"pure fn choose(value: i64) -> i64:
    match value:
        case 1:
            return 10
        case _:
            return 20

const VALUE: i64 = choose(1)

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("an exhaustive source-ordered match must compile and evaluate");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 10
            )
    }));
    let syntax = accepted.inspection().syntax().expect("syntax requested");
    assert_eq!(
        syntax[0]
            .nodes()
            .iter()
            .filter(|node| node.kind() == SyntaxNodeKind::MatchCase)
            .count(),
        2,
        "match patterns and their case headers remain structurally observable"
    );
}

#[test]
fn match_bindings_and_guards_are_scoped_and_evaluated_in_source_order() {
    let source = br#"pure fn choose(value: i64) -> i64:
    match value:
        case matched if matched > 10:
            return matched
        case matched:
            return matched + 1

const VALUE: i64 = choose(4)

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("a binding is visible in its guard and selected case body: {outcome:#?}");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 5
            )
    }));
}

#[test]
fn enum_payload_patterns_bind_nested_values_and_prove_exhaustiveness() {
    let source = br#"enum Lookup:
    Found(value: i64)
    Absent

pure fn choose(value: Lookup) -> i64:
    match value:
        case Lookup.Found(found):
            return found
        case Lookup.Absent:
            return 0

const VALUE: i64 = choose(Lookup.Found(7))

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("enum payload patterns must bind and evaluate: {outcome:#?}");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 7
            )
    }));
}

#[test]
fn tuple_fixed_array_and_or_patterns_destructure_closed_values() {
    let source = br#"enum Choice:
    Left(value: (i64, [i64; 2]))
    Right(value: (i64, [i64; 2]))

pure fn choose(value: Choice) -> i64:
    match value:
        case Choice.Left((head, [middle, tail])) or Choice.Right((head, [middle, tail])):
            return head + middle + tail

const VALUE: i64 = choose(Choice.Right((2, [3, 4])))

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("nested product and binding-consistent or patterns must evaluate: {outcome:#?}");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 9
            )
    }));
}

#[test]
fn struct_patterns_destructure_named_fields() {
    let source = br#"struct Point:
    x: i64
    y: i64

pure fn total(value: Point) -> i64:
    match value:
        case Point(x=left, y=right):
            return left + right

const VALUE: i64 = total(Point(x=5, y=8))

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("named struct patterns must bind their fields: {outcome:#?}");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 13
            )
    }));
}

#[test]
fn take_binding_patterns_make_resource_consumption_explicit() {
    let source = br#"resource struct Ticket:
    id: i64

fn consume(take ticket: Ticket) -> i64:
    return ticket.id

fn choose(take value: Ticket) -> i64:
    match value:
        case take ticket:
            return consume(take ticket)

fn compute() -> i64:
    ticket = Ticket(id=21)
    return choose(take ticket)

const VALUE: i64 = compute()

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("resource pattern moves must be explicitly authored: {outcome:#?}");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 21
            )
    }));
}

#[test]
fn take_binding_patterns_leave_the_matched_resource_unreadable() {
    let source = br#"resource struct Ticket:
    id: i64

fn consume(take ticket: Ticket) -> i64:
    return ticket.id

fn broken(take value: Ticket) -> i64:
    mut result: i64 = 0
    match value:
        case take ticket:
            result = consume(take ticket)
    return result + value.id

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(source);
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("a resource moved by a pattern must remain moved: {outcome:#?}");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.read_after_move")
    );
}

#[test]
fn mutable_field_and_index_places_update_the_nested_value() {
    let source = br#"struct State:
    mut values: [i64; 2]

pure fn compute() -> i64:
    mut state = State(values=[2, 3])
    state.values[1] += 4
    return state.values[0] + state.values[1]

const VALUE: i64 = compute()

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("mutable field/index places must update through the public seam: {outcome:#?}");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 9
            )
    }));
}

#[test]
fn for_patterns_destructure_every_bounded_element() {
    let source = br#"pure fn total(values: [(i64, i64); 2]) -> i64:
    mut result = 0
    for (left, right) in values:
        result += left + right
    return result

const VALUE: i64 = total([(1, 2), (3, 4)])

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("an irrefutable product pattern must bind each for element: {outcome:#?}");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 10
            )
    }));
}

#[test]
fn for_rejects_a_pattern_that_cannot_match_every_element() {
    let source = br#"enum Choice:
    Left(value: i64)
    Right(value: i64)

pure fn total(values: [Choice; 1]) -> i64:
    mut result = 0
    for Choice.Left(value) in values:
        result += value
    return result

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("a refutable for pattern must be rejected");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.invalid_match_pattern"),
        "{rejected:#?}"
    );
}

#[test]
fn mutable_resource_arguments_require_and_honor_an_explicit_call_site_marker() {
    let source = br#"resource struct Counter:
    mut value: i64

fn increment(mut counter: Counter):
    counter.value += 1

fn observe(read counter: Counter) -> i64:
    return counter.value

fn compute() -> i64:
    mut counter = Counter(value=4)
    increment(mut counter)
    return observe(counter)

const VALUE: i64 = compute()

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("an authored mut marker must grant call-scoped mutation: {outcome:#?}");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 5
            )
    }));
}

#[test]
fn mutable_and_consuming_parameters_reject_unmarked_arguments() {
    let source = br#"resource struct Ticket:
    id: i64

fn borrow_mut(mut ticket: Ticket):
    pass

fn consume(take ticket: Ticket):
    pass

fn broken(take ticket: Ticket):
    borrow_mut(ticket)
    consume(ticket)

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(source);
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("mut/take parameters must not infer authority from their signatures: {outcome:#?}");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == "semantic.argument_ownership_mismatch" })
    );
}

#[test]
fn explicit_take_tracks_sibling_resource_field_places_independently() {
    let source = br#"resource struct Ticket:
    id: i64

resource struct Envelope:
    left: Ticket
    right: Ticket

fn consume(take ticket: Ticket) -> i64:
    return ticket.id

fn total(take envelope: Envelope) -> i64:
    return consume(take envelope.left) + consume(take envelope.right)

fn compute() -> i64:
    envelope = Envelope(left=Ticket(id=6), right=Ticket(id=7))
    return total(take envelope)

const VALUE: i64 = compute()

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("moving one Resource field must leave its sibling place readable: {outcome:#?}");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 13
            )
    }));
}

#[test]
fn for_patterns_bind_nested_elements_for_each_iteration() {
    let source = br#"pure fn total(values: [(i64, i64); 2]) -> i64:
    mut result: i64 = 0
    for (left, right) in values:
        result += left + right
    return result

const VALUE: i64 = total([(1, 2), (3, 4)])

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("for must accept the same nested binding patterns as match: {outcome:#?}");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 10
            )
    }));
}

#[test]
fn compiler_bounded_while_and_nearest_loop_exits_evaluate_deterministically() {
    let source = br#"type Count = i64

pure fn total() -> i64:
    mut index: i64 = 0
    mut sum: Count = 0
    while index < 6:
        index = index + 1
        if index == 2:
            continue
        if index == 5:
            break
        sum = sum + index
    return sum

const VALUE: i64 = total()

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("a compiler-bounded while with nearest-loop exits must compile: {outcome:#?}");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 8
            )
    }));
}

#[test]
fn while_without_a_compiler_derived_bound_is_rejected() {
    let source = br#"pure fn spin(flag: bool):
    while flag:
        pass

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("an unbounded while must be rejected");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.unbounded_while")
    );
}

#[test]
fn while_bounds_do_not_reuse_branch_specific_integer_facts() {
    let source = br#"pure fn branch_bound(flag: bool):
    mut index = 0
    if flag:
        index = -100
    else:
        index = 0
    while index < 3:
        index += 1

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("a path-specific initial value must not prove a whole-loop bound");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.unbounded_while")
    );
}

#[test]
fn explicit_take_moves_resource_arguments() {
    let source = br#"resource struct Token:
    value: i64

pure fn consume(take token: Token) -> i64:
    return token.value

pure fn use_token() -> i64:
    token = Token(value=7)
    return consume(take token)

const VALUE: i64 = use_token()

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("an explicitly moved Resource argument must compile");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 7
            )
    }));
}

#[test]
fn named_function_values_keep_their_exact_callable_type_and_evaluate() {
    let source = br#"pure fn increment(value: i64) -> i64:
    return value + 1

pure fn apply(callback: fn(i64) -> i64, value: i64) -> i64:
    return callback(value)

const VALUE: i64 = apply(increment, 4)

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("a closed named function value must compile and evaluate");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 5
            )
    }));
}

#[test]
fn inline_closures_capture_bounded_data_and_evaluate_through_the_callable_seam() {
    let source = br#"pure fn calculate(offset: i64) -> i64:
    add = |value: i64| value + offset
    return add(4)

const VALUE: i64 = calculate(3)

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("a Data-capturing inline closure must compile and evaluate");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 7
            )
    }));
}

#[test]
fn closure_values_are_constant_callable_data_but_cannot_capture_resources() {
    let constant = br#"const ADD: fn(i64) -> i64 = |value| value + 2
const VALUE: i64 = ADD(5)

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let CompilationOutcome::Accepted(accepted) = compile(constant) else {
        panic!("a closed closure must be a constant callable value");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 7
            )
    }));

    let resource = br#"resource struct Token:
    value: i64

pure fn invalid(take token: Token) -> fn(i64) -> i64:
    return |value: i64| value + token.value

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(resource) else {
        panic!("runtime closures must not hide Resource captures");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == "semantic.closure_capture_requires_data" })
    );
}

#[test]
fn generic_nominal_construction_preserves_concrete_field_types() {
    let source = br#"struct Box[T]:
    value: T

pure fn unwrap(boxed: Box[i64]) -> i64:
    return boxed.value

const VALUE: i64 = unwrap(Box(value=9))

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!(
            "a generic nominal must infer and preserve its concrete type arguments: {outcome:#?}"
        );
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 9
            )
    }));
}

#[test]
fn closed_boolean_matches_prove_exhaustiveness_without_a_wildcard() {
    let source = br#"pure fn choose(value: bool) -> i64:
    match value:
        case true:
            return 1
        case false:
            return 2

const VALUE: i64 = choose(false)

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("all alternatives of a closed Bool match must prove exhaustiveness");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 2
            )
    }));

    let duplicate = br#"pure fn choose(value: bool) -> i64:
    match value:
        case true:
            return 1
        case true:
            return 2
        case false:
            return 3

@image
fn build() -> Image:
    return Image.new(value=choose(false))
"#;
    let CompilationOutcome::Rejected(rejected) = compile(duplicate) else {
        panic!("a duplicate literal case must be unreachable");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == "semantic.unreachable_match_case" })
    );
}

#[test]
fn closed_enum_matches_are_proven_exhaustive_and_evaluated() {
    let source = br#"enum Direction:
    North
    South

pure fn code(direction: Direction) -> i64:
    match direction:
        case Direction.North:
            return 10
        case Direction.South:
            return 20

const VALUE: i64 = code(Direction.South())

@image
fn build() -> Image:
    return Image.new(value=VALUE)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("all variants of a closed enum must prove match exhaustiveness: {outcome:#?}");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.VALUE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 20
            )
    }));
}

#[test]
fn is_tests_a_single_literal_or_closed_variant_without_reflection() {
    let source = br#"enum Direction:
    North
    South

pure fn is_south(direction: Direction) -> bool:
    return direction is Direction.South

const ENUM_VALUE: bool = is_south(Direction.South())
const LITERAL_VALUE: bool = 4 is 4 and true

@image
fn build() -> Image:
    return Image.new(enum_value=ENUM_VALUE, literal_value=LITERAL_VALUE)
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("single-case is patterns must compile and evaluate");
    };
    for root in ["image.ENUM_VALUE", "image.LITERAL_VALUE"] {
        assert!(
            accepted
                .inspection()
                .evaluations()
                .iter()
                .any(|evaluation| {
                    evaluation.root() == root
                        && matches!(
                            evaluation.outcome(),
                            EvaluationOutcome::Completed(CanonicalValue::Bool(true))
                        )
                })
        );
    }
}

#[test]
fn is_patterns_bind_payloads_only_in_the_successful_branch() {
    let source = br#"enum Lookup:
    Found(value: i64)
    Absent

pure fn choose(value: Lookup) -> i64:
    if value is Lookup.Found(found):
        return found
    return 0

const FOUND: i64 = choose(Lookup.Found(7))
const ABSENT: i64 = choose(Lookup.Absent)

@image
fn build() -> Image:
    return Image.new(found=FOUND, absent=ABSENT)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("a successful is pattern must expose its payload binding: {outcome:#?}");
    };
    for (root, expected) in [("image.FOUND", 7), ("image.ABSENT", 0)] {
        assert!(
            accepted
                .inspection()
                .evaluations()
                .iter()
                .any(|evaluation| {
                    evaluation.root() == root
                        && matches!(
                            evaluation.outcome(),
                            EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. })
                                if *value == expected
                        )
                })
        );
    }
}

#[test]
fn is_pattern_bindings_cannot_escape_or_be_introduced_through_negation() {
    let escaped = br#"enum Lookup:
    Found(value: i64)
    Absent

pure fn choose(value: Lookup) -> i64:
    if value is Lookup.Found(found):
        pass
    return found

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(escaped) else {
        panic!("an is-pattern binding must not escape its successful branch");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.unresolved_name")
    );

    let negated = br#"enum Lookup:
    Found(value: i64)
    Absent

pure fn choose(value: Lookup) -> i64:
    if not (value is Lookup.Found(found)):
        return 0
    return found

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(negated) else {
        panic!("a binding is-pattern must be the direct condition of its branch");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.invalid_match_pattern")
    );
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
fn image_constructor_failures_never_reach_graph_sealing() {
    for (source, expected_code) in [
        (
            b"@image\nfn build() -> Image:\n    panic \"boom\"\n".as_slice(),
            "evaluation.panicked",
        ),
        (
            b"@image\nfn build() -> Image:\n    value = 127i8 + 1i8\n    return Image.new(value=value)\n"
                .as_slice(),
            "evaluation.panicked",
        ),
    ] {
        let CompilationOutcome::Rejected(rejected) = compile(source) else {
            panic!("Image evaluation failure must remain a Creator rejection");
        };
        assert!(
            rejected
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code() == expected_code)
        );
    }

    let mut source = String::new();
    for index in 0..32 {
        if index == 31 {
            source.push_str("pure fn helper31() -> i64:\n    return 1\n\n");
        } else {
            source.push_str(&format!(
                "pure fn helper{index}() -> i64:\n    return helper{}()\n\n",
                index + 1
            ));
        }
    }
    source.push_str("@image\nfn build() -> Image:\n    return Image.new(value=helper0())\n");
    let CompilationOutcome::Rejected(rejected) = compile(source.as_bytes()) else {
        panic!("Image evaluator containment must be a structured Creator rejection");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "evaluation.limit_exceeded")
    );
}

#[test]
fn nested_comptime_statements_select_before_function_semantics() {
    let source = br#"const ENABLED: bool = false

pure fn answer() -> i64:
    comptime if ENABLED:
        return missing()
    elif not ENABLED:
        return 42
    else:
        return also_missing()

@image
fn build() -> Image:
    return Image.new(value=answer())
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("only the selected nested statement branch is semantic: {outcome:#?}");
    };
    assert!(matches!(
        accepted.inspection().evaluations()[1].outcome(),
        EvaluationOutcome::Completed(CanonicalValue::SymbolicHandle {
            kind: ConstructionKind::Image,
            ..
        })
    ));
    let syntax = accepted.inspection().syntax().expect("syntax");
    assert_eq!(syntax[0].source_bytes(), source);
}

#[test]
fn nested_comptime_member_blocks_select_struct_resource_and_enum_members() {
    let source = br#"const ENABLED: bool = false

struct Card:
    comptime if ENABLED:
        broken: Missing
        pure fn score(self) -> i64:
            return missing()
    else:
        value: i64
        pure fn score(self) -> i64:
            return self.value

resource struct Ticket:
    comptime if ENABLED:
        broken: Missing
    else:
        bytes: Bytes

enum State:
    comptime if ENABLED:
        Broken(value: Missing)
    else:
        Ready

@image
fn build() -> Image:
    card = Card(value=42)
    ticket = Ticket(bytes=b"ok")
    return Image.new(score=card.score(), ticket=ticket, state=State.Ready)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("only selected container members are semantic: {outcome:#?}");
    };
    let syntax = &accepted.inspection().syntax().expect("syntax")[0];
    assert_eq!(syntax.source_bytes(), source);
    let count = |kind| {
        syntax
            .nodes()
            .iter()
            .filter(|node| node.kind() == kind)
            .count()
    };
    assert_eq!(count(SyntaxNodeKind::ComptimeSelection), 3);
    assert_eq!(count(SyntaxNodeKind::ComptimeBranch), 6);
    assert_eq!(count(SyntaxNodeKind::Field), 4);
    assert_eq!(count(SyntaxNodeKind::MemberFunction), 2);
    assert_eq!(count(SyntaxNodeKind::Variant), 2);
}

#[test]
fn unselected_nested_comptime_branches_still_require_valid_syntax() {
    let source = br#"const ENABLED: bool = false

pure fn answer() -> i64:
    comptime if ENABLED:
        if:
            pass
    else:
        return 42

@image
fn build() -> Image:
    return Image.new(value=answer())
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("compile-time selection never hides malformed syntax");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "syntax.malformed_declaration")
    );
    assert_eq!(
        rejected.inspection().syntax().expect("syntax")[0].source_bytes(),
        source
    );
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
fn text_escapes_decode_and_unknown_escapes_are_rejected() {
    let source = br#"const VALUE: Text = "line\n\u{41}"

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("valid Text escapes must compile");
    };
    assert!(
        accepted
            .inspection()
            .evaluations()
            .iter()
            .any(|evaluation| {
                evaluation.root() == "image.VALUE"
                    && evaluation.outcome()
                        == &EvaluationOutcome::Completed(CanonicalValue::Text("line\nA".into()))
            })
    );

    let malformed = br#"const VALUE: Text = "bad\q"

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(malformed) else {
        panic!("an unknown Text escape must be preserved and rejected");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "syntax.invalid_literal")
    );
}

#[test]
fn numeric_separators_and_prefixes_follow_the_source_contract() {
    for literal in ["1__2", "1_", "0b_1", "0Xff", "0B10", "0O7"] {
        let source = format!(
            "const VALUE: i64 = {literal}\n\n@image\nfn build() -> Image:\n    return Image.new()\n"
        );
        let CompilationOutcome::Rejected(rejected) = compile(source.as_bytes()) else {
            panic!("malformed numeric literal {literal} must reject");
        };
        assert!(
            rejected
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code() == "syntax.invalid_literal"),
            "{literal} must be rejected at the lossless syntax seam"
        );
    }

    let valid = br#"const VALUE: i64 = 1_024

@image
fn build() -> Image:
    return Image.new()
"#;
    assert!(matches!(compile(valid), CompilationOutcome::Accepted(_)));
}

#[test]
fn ordinary_assert_is_checked_by_the_verified_evaluator() {
    let passing = br#"pure fn checked() -> i64:
    assert true
    return 7

const VALUE: i64 = checked()

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Accepted(accepted) = compile(passing) else {
        panic!("a passing ordinary assertion must evaluate");
    };
    assert!(
        accepted
            .inspection()
            .evaluations()
            .iter()
            .any(|evaluation| {
                evaluation.root() == "image.VALUE"
                    && evaluation.outcome()
                        == &EvaluationOutcome::Completed(CanonicalValue::Integer {
                            type_name: "i64".into(),
                            value: 7,
                        })
            })
    );

    let failing = br#"pure fn checked() -> i64:
    assert false
    return 7

const VALUE: i64 = checked()

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(failing) else {
        panic!("a failed ordinary assertion must reject evaluation");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "evaluation.panicked")
    );
}

#[test]
fn boolean_operators_short_circuit_in_source_order() {
    let source = br#"pure fn unsafe_value() -> bool:
    return 1i64 / 0i64 == 0i64

const AND_VALUE: bool = false and unsafe_value()
const OR_VALUE: bool = true or unsafe_value()
const NOT_VALUE: bool = not false

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("Boolean operators must type-check and short-circuit");
    };
    for (root, value) in [
        ("image.AND_VALUE", false),
        ("image.OR_VALUE", true),
        ("image.NOT_VALUE", true),
    ] {
        assert!(
            accepted
                .inspection()
                .evaluations()
                .iter()
                .any(|evaluation| {
                    evaluation.root() == root
                        && evaluation.outcome()
                            == &EvaluationOutcome::Completed(CanonicalValue::Bool(value))
                })
        );
    }
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
fn comptime_callable_selection_is_owned_by_each_concrete_specialization() {
    let source = br#"pure fn selected[const N: u64](values: [i64; N]) -> i64:
    comptime if N == 1u64:
        return values[0]
    else:
        return values[1]

const ONE: i64 = selected([7])
const TWO: i64 = selected([7, 9])

@image
fn build() -> Image:
    return Image.new(one=ONE, two=TWO)
"#;

    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("each Specialization must evaluate its own compile-time branch: {outcome:#?}");
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.ONE"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 7
            )
    }));
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.TWO"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 9
            )
    }));
}

#[test]
fn comptime_member_selection_is_owned_by_each_applied_nominal_type() {
    let source = br#"struct Bucket[const N: u64]:
    values: [i64; N]
    pure fn selected(read self) -> i64:
        comptime if N == 1u64:
            return self.values[0]
        else:
            return self.values[1]

pure fn one() -> i64:
    bucket = Bucket(values=[7])
    return bucket.selected()

pure fn two() -> i64:
    bucket = Bucket(values=[7, 9])
    return bucket.selected()

const ONE: i64 = one()
const TWO: i64 = two()

@image
fn build() -> Image:
    return Image.new(one=ONE, two=TWO)
"#;

    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("each applied nominal Type must select its member body independently: {outcome:#?}");
    };
    for (root, expected) in [("image.ONE", 7), ("image.TWO", 9)] {
        assert!(
            accepted
                .inspection()
                .evaluations()
                .iter()
                .any(|evaluation| {
                    evaluation.root() == root
                        && matches!(
                            evaluation.outcome(),
                            EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. })
                                if *value == expected
                        )
                })
        );
    }
}

#[test]
fn comptime_member_declarations_are_selected_per_applied_nominal_type() {
    let source = br#"struct Bucket[const N: u64]:
    comptime if N == 1u64:
        value: i64
    else:
        other: i64

const ONE: Bucket[1] = Bucket(value=7)
const TWO: Bucket[2] = Bucket(other=9)

pure fn one(bucket: Bucket[1]) -> i64:
    return bucket.value

pure fn two(bucket: Bucket[2]) -> i64:
    return bucket.other

const ONE_VALUE: i64 = one(ONE)
const TWO_VALUE: i64 = two(TWO)

@image
fn build() -> Image:
    return Image.new(one=ONE_VALUE, two=TWO_VALUE)
"#;

    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("each applied TypeId must own its selected member declarations: {outcome:#?}");
    };
    for (root, expected) in [("image.ONE_VALUE", 7), ("image.TWO_VALUE", 9)] {
        assert!(
            accepted
                .inspection()
                .evaluations()
                .iter()
                .any(|evaluation| {
                    evaluation.root() == root
                        && matches!(
                            evaluation.outcome(),
                            EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. })
                                if *value == expected
                        )
                })
        );
    }
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
fn inferred_result_signatures_are_concrete_before_callers_are_checked() {
    let source = br#"enum ReadError:
    Missing

fn fetch() -> Result[i64]:
    return Result.Err(ReadError.Missing)

const FETCHED: Result[i64, ReadError] = fetch()

@image
fn build() -> Image:
    return Image.new(value=FETCHED)
"#;
    let outcome = compile(source);
    assert!(
        matches!(outcome, CompilationOutcome::Accepted(_)),
        "an inferred private signature must be concrete at its caller: {outcome:#?}"
    );
}

#[test]
fn expected_types_concretize_nested_result_variants() {
    let source = br#"struct Trouble:
    code: i64

fn swallow(value: Result[i64, Trouble]) -> i64:
    return 7

fn f() -> Result[i64]:
    return Result.Ok(swallow(Result.Err(Trouble(code=1))))

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(source);
    assert!(
        matches!(outcome, CompilationOutcome::Accepted(_)),
        "context must eliminate every Result placeholder before HIR sealing: {outcome:#?}"
    );
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
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.unresolved_call"),
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
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.argument_count")
    );

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
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.argument_label_mismatch")
    );
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
        let expected = format!("semantic.{expected}");
        assert!(
            rejected
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code() == expected)
        );
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
    let diagnostic = rejected
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.code() == "semantic.conflicting_inferred_errors")
        .expect("conflicting inference diagnostic");
    let contribution_ranges = diagnostic
        .labels()
        .iter()
        .filter(|label| label.role() == "propagation_source")
        .map(|label| &source[label.range().start() as usize..label.range().end() as usize])
        .collect::<Vec<_>>();
    assert!(contribution_ranges.iter().any(|range| {
        range
            .windows(b"ReadError.Missing".len())
            .any(|window| window == b"ReadError.Missing")
    }));
    assert!(contribution_ranges.iter().any(|range| {
        range
            .windows(b"ParseError.Invalid".len())
            .any(|window| window == b"ParseError.Invalid")
    }));
    assert!(has_text_parameter(
        diagnostic,
        "repair",
        "explicit_error_annotation"
    ));
    assert!(has_text_parameter(diagnostic, "conversion", "map_error"));
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
fn private_error_inference_follows_a_directly_returned_result() {
    let source = br#"enum ReadError:
    Missing

fn outer() -> Result[i64]:
    return inner()

fn inner() -> Result[i64]:
    return Result.Err(ReadError.Missing)

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("a directly returned Result carries its Nominal Error");
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
fn private_error_inference_follows_a_result_returned_through_a_local() {
    let source = br#"enum ReadError:
    Missing

fn outer() -> Result[i64]:
    result = inner()
    return result

fn inner() -> Result[i64, ReadError]:
    return Result.Err(ReadError.Missing)

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("a local cannot erase the returned Result's Nominal Error: {outcome:#?}");
    };
    let inferred = accepted
        .inspection()
        .inferred_errors()
        .iter()
        .find(|error| error.function() == "outer")
        .expect("outer inferred error");
    assert_eq!(inferred.error_type(), "ReadError");
}

#[test]
fn private_error_inference_follows_an_inferred_result_through_a_local() {
    let source = br#"enum ReadError:
    Missing

fn outer() -> Result[i64]:
    result = inner()
    return result

fn inner() -> Result[i64]:
    return Result.Err(ReadError.Missing)

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("local Result flow must participate in the private error fixpoint: {outcome:#?}");
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
fn private_error_inference_follows_a_propagated_local() {
    let source = br#"enum ReadError:
    Missing

fn outer(input: Result[i64, ReadError]) -> Result[i64]:
    result = input
    return result?

@image
fn build() -> Image:
    return Image.new(value=outer(Result.Err(ReadError.Missing)))
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("a propagated local must contribute its exact error: {outcome:#?}");
    };
    let inferred = accepted
        .inspection()
        .inferred_errors()
        .iter()
        .find(|error| error.function() == "outer")
        .expect("outer inferred error");
    assert_eq!(inferred.error_type(), "ReadError");
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
fn option_propagation_returns_the_compatible_absent_alternative() {
    let source = br#"fn succeeds() -> Option[i64]:
    return Option.Some(7)

fn fails() -> Option[i64]:
    return Option.None

fn success_outer() -> Option[i64]:
    value = succeeds()?
    return Option.Some(value)

fn none_outer() -> Option[i64]:
    value = fails()?
    return Option.Some(value)

const SOME: Option[i64] = success_outer()
const NONE: Option[i64] = none_outer()

@image
fn build() -> Image:
    return Image.new(some=SOME, none=NONE)
"#;

    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("Option propagation must return the compatible absent alternative: {outcome:#?}");
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
        evaluation("image.SOME"),
        &EvaluationOutcome::Completed(CanonicalValue::Variant {
            type_name: "Option".into(),
            variant: "Some".into(),
            payload: vec![CanonicalValue::Integer {
                type_name: "i64".into(),
                value: 7,
            }]
            .into(),
        })
    );
    assert_eq!(
        evaluation("image.NONE"),
        &EvaluationOutcome::Completed(CanonicalValue::Variant {
            type_name: "Option".into(),
            variant: "None".into(),
            payload: Vec::new().into(),
        })
    );
}

#[test]
fn option_propagation_cannot_escape_a_result_returning_function() {
    let source = br#"enum ReadError:
    Missing

fn inner() -> Option[i64]:
    return Option.None

fn outer() -> Result[i64, ReadError]:
    return Result.Ok(inner()?)

@image
fn build() -> Image:
    return Image.new()
"#;

    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("Option.None cannot become a Result early return");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.propagation_return_mismatch")
    );
}

#[test]
fn result_propagation_cannot_escape_an_option_returning_function() {
    let source = br#"enum ReadError:
    Missing

fn inner() -> Result[i64, ReadError]:
    return Result.Err(ReadError.Missing)

fn outer() -> Option[i64]:
    return Option.Some(inner()?)

@image
fn build() -> Image:
    return Image.new()
"#;

    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("Result.Err cannot become an Option early return");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.propagation_return_mismatch")
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
fn deferred_expressions_run_in_reverse_order_on_normal_scope_exit() {
    let source = br#"fn fail(which: i64):
    if which == 1:
        panic "first deferred expression"
    else:
        panic "second deferred expression"

fn run():
    defer fail(1)
    defer fail(2)

const VALUE: () = run()

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("the last registered deferred expression must Panic first");
    };
    let diagnostic = rejected
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.code() == "evaluation.panicked")
        .expect("deferred Panic diagnostic");
    let expected = source
        .windows(b"        panic \"second deferred expression\"".len())
        .position(|window| window == b"        panic \"second deferred expression\"")
        .expect("second Panic site");
    assert_eq!(diagnostic.primary().start(), expected as u64);
}

#[test]
fn deferred_expression_runs_on_return() {
    let source = br#"fn fail():
    panic "deferred return cleanup"

fn run() -> i64:
    defer fail()
    return 7

const VALUE: i64 = run()

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("return must run deferred expressions");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "evaluation.panicked")
    );
}

#[test]
fn deferred_expression_runs_on_propagation() {
    let source = br#"enum Failure:
    Failed

fn fail() -> Result[i64, Failure]:
    return Result.Err(Failure.Failed)

fn cleanup():
    panic "deferred propagation cleanup"

fn run() -> Result[i64, Failure]:
    defer cleanup()
    return fail()?

const VALUE: Result[i64, Failure] = run()

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("propagation must run deferred expressions");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "evaluation.panicked")
    );
}

#[test]
fn deferred_expression_runs_on_break() {
    let source = br#"fn cleanup():
    panic "deferred break cleanup"

fn run():
    for value in [1]:
        defer cleanup()
        break

const VALUE: () = run()

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("break must run deferred expressions in the loop body scope");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "evaluation.panicked")
    );
}

#[test]
fn deferred_expression_runs_on_continue() {
    let source = br#"fn cleanup():
    panic "deferred continue cleanup"

fn run():
    for value in [1]:
        defer cleanup()
        continue

const VALUE: () = run()

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("continue must run deferred expressions in the loop body scope");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "evaluation.panicked")
    );
}

#[test]
fn panic_does_not_run_deferred_cleanup() {
    let source = br#"fn cleanup():
    panic "deferred cleanup must not run"

fn run():
    defer cleanup()
    panic "body panic"

const VALUE: () = run()

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("the body Panic must reject evaluation");
    };
    let diagnostic = rejected
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.code() == "evaluation.panicked")
        .expect("body Panic diagnostic");
    let expected = source
        .windows(b"    panic \"body panic\"".len())
        .position(|window| window == b"    panic \"body panic\"")
        .expect("body Panic site");
    assert_eq!(diagnostic.primary().start(), expected as u64);
}

#[test]
fn deferred_expression_cannot_return_a_recoverable_error() {
    let source = br#"enum Failure:
    Failed

fn recoverable() -> Result[i64, Failure]:
    return Result.Err(Failure.Failed)

fn run():
    defer recoverable()

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("a deferred Result must be resolved before scope exit");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.defer_returns_recoverable_error")
    );
}

#[test]
fn deferred_expression_cannot_propagate_a_recoverable_error() {
    let source = br#"enum Failure:
    Failed

fn recoverable() -> Result[i64, Failure]:
    return Result.Err(Failure.Failed)

fn run() -> Result[i64, Failure]:
    defer recoverable()?
    return Result.Ok(1)

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("deferred propagation cannot replace or hide the original exit");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.defer_returns_recoverable_error")
    );
}

#[test]
fn defer_registration_is_lexical_and_execution_dependent() {
    let source = |condition: &str| {
        format!(
            "fn cleanup():\n    panic \"lexical cleanup\"\n\nfn run() -> i64:\n    if {condition}:\n        defer cleanup()\n    return 7\n\nconst VALUE: i64 = run()\n\n@image\nfn build() -> Image:\n    return Image.new()\n"
        )
    };

    let CompilationOutcome::Accepted(accepted) = compile(source("false").as_bytes()) else {
        panic!("an unexecuted defer statement must not register cleanup");
    };
    assert!(
        accepted
            .inspection()
            .evaluations()
            .iter()
            .any(|evaluation| {
                evaluation.root() == "image.VALUE"
                    && matches!(
                        evaluation.outcome(),
                        EvaluationOutcome::Completed(CanonicalValue::Integer { value: 7, .. })
                    )
            })
    );

    let CompilationOutcome::Rejected(rejected) = compile(source("true").as_bytes()) else {
        panic!("a nested lexical scope must run its registered cleanup when it exits");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "evaluation.panicked")
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
fn evaluation_failures_expose_provenance_identity_call_chain_and_contributors() {
    let source = br#"pure fn inner() -> i64:
    panic "boom"

pure fn outer() -> i64:
    return inner()

const BAD: i64 = outer()

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("the evaluated Panic fixture must reject");
    };
    let evaluation = rejected
        .inspection()
        .evaluations()
        .iter()
        .find(|evaluation| evaluation.root() == "image.BAD")
        .expect("failed constant evaluation remains inspectable");
    assert!(matches!(
        evaluation.outcome(),
        EvaluationOutcome::Panicked { .. }
    ));
    let receipt = evaluation.receipt();
    assert!(receipt.provenance().is_some());
    assert!(receipt.relevant_identity().is_some());
    assert!(
        receipt
            .call_chain()
            .iter()
            .any(|frame| frame.callable() == "inner")
    );
    assert!(!receipt.contributors().is_empty());
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
fn exact_propagation_rejects_local_mediated_error_conversion() {
    let source = br#"enum ReadError:
    Missing

enum AppError:
    Failed

fn fetch() -> Result[i64, ReadError]:
    return Result.Err(ReadError.Missing)

fn load() -> Result[i64, AppError]:
    result = fetch()
    return Result.Ok(result?)

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("a local cannot hide an implicit error conversion");
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
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.binary_type_mismatch")
    );

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
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.invalid_unary_operand")
    );

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
fn negating_an_evaluated_signed_minimum_panics_on_fixed_width_overflow() {
    let source = br#"const MINIMUM: i8 = -128i8
const NEGATED: i8 = -MINIMUM

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("negating an evaluated i8 minimum must overflow");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "evaluation.panicked")
    );
}

#[test]
fn source_read_after_move_is_a_creator_rejection_not_a_compiler_defect() {
    let source = br#"resource struct Ticket:
    id: i64

fn consume(take value: Ticket):
    pass

fn broken(take value: Ticket) -> i64:
    consume(take value)
    return value.id

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
fn moving_a_resource_field_moves_its_containing_place() {
    let source = br#"resource struct Ticket:
    id: i64

resource struct Envelope:
    ticket: Ticket

fn consume(take ticket: Ticket):
    pass

fn broken(take envelope: Envelope):
    consume(take envelope.ticket)
    consume(take envelope.ticket)

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(source);
    let CompilationOutcome::Rejected(rejected) = outcome else {
        panic!("a Resource field cannot be moved twice: {outcome:#?}");
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
        consume(take ticket)
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
        consume(take ticket)
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
fn expected_types_concretize_zero_payload_generic_nominals() {
    let source = br#"enum Maybe[T]:
    Nothing
    Some(value: T)

const NOTHING: Maybe[i64] = Maybe.Nothing()

@image
fn build() -> Image:
    return Image.new(nothing=NOTHING)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!(
            "an expected generic nominal type must supply otherwise-unconstrained arguments: {outcome:#?}"
        );
    };
    assert!(
        accepted
            .inspection()
            .evaluations()
            .iter()
            .any(|evaluation| {
                evaluation.root() == "image.NOTHING"
                    && matches!(
                        evaluation.outcome(),
                        EvaluationOutcome::Completed(CanonicalValue::Variant { variant, .. })
                            if variant.as_ref() == "Nothing"
                    )
            })
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
fn generic_kinds_bounds_existentials_and_associated_constants_are_operational() {
    let source = br#"interface Measured:
    pure fn measure(read self) -> i64
    const ZERO: i64

struct Sample implements Measured:
    value: i64
    const ZERO: i64 = 0
    pub const WIDTH: i64 = 3
    pure fn measure(read self) -> i64:
        return self.value

struct Buffer[const N: u64]:
    values: [i64; N]
    pure fn first(read self) -> i64:
        return self.values[0]

pool storage

resource struct OwnedSample[P: Pool]:
    value: own[P] Sample

fn release(take value: OwnedSample[storage]):
    pass

pure fn first[const N: u64](values: [i64; N]) -> i64:
    return values[0]

pure fn measured[T: Measured](value: T) -> i64:
    return value.measure()

pure fn erased(value: any Measured) -> i64:
    return value.measure()

pure fn buffer_first() -> i64:
    buffer = Buffer(values=[5, 8])
    return buffer.first()

const BUFFER: Buffer[2] = Buffer(values=[5, 8])
const TOTAL: i64 = first([5, 8]) + buffer_first() + measured(Sample(value=13)) + erased(Sample(value=16)) + Sample.WIDTH

@image
fn build() -> Image:
    return Image.new(total=TOTAL)
"#;

    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!(
            "Layer 1 generic kinds, interface bounds, existentials, and associated constants must compile: {outcome:#?}"
        );
    };
    assert!(accepted.inspection().evaluations().iter().any(|evaluation| {
        evaluation.root() == "image.TOTAL"
            && matches!(
                evaluation.outcome(),
                EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) if *value == 42
            )
    }));
}

#[test]
fn resources_require_explicit_transfer_and_results_are_must_use() {
    let implicit_transfer = br#"resource struct Ticket:
    value: i64

resource struct Envelope:
    ticket: Ticket

fn wrap(take ticket: Ticket) -> Envelope:
    return Envelope(ticket=ticket)

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(implicit_transfer) else {
        panic!("copying a Resource into another Resource must reject");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.resource_requires_take")
    );

    let discarded_result = br#"struct Failure:
    code: i64

fn broken():
    Result.Ok(1)

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(discarded_result) else {
        panic!("discarding a Result must reject");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.must_use_value")
    );
}

#[test]
fn plain_generic_data_cannot_silently_promote_to_resource() {
    let source = br#"resource struct Ticket:
    value: i64

struct Box[T]:
    value: T

fn invalid(take value: Box[Ticket]):
    pass

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(source) else {
        panic!("a plain generic Data aggregate cannot hide a Resource");
    };
    assert!(rejected.diagnostics().iter().any(|diagnostic| {
        diagnostic.code() == "semantic.resource_argument_requires_resource_struct"
    }));
}

#[test]
fn public_nameability_and_result_error_contract_cover_every_signature_form() {
    let private_exposure = br#"struct Hidden:
    value: i64

pub const EXPOSED: Hidden = Hidden(value=1)
pub type PublicAlias = Hidden

pub enum PublicChoice:
    Item(value: Hidden)

pub interface PublicInterface:
    fn reveal() -> Hidden

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(private_exposure) else {
        panic!("all public signature forms must enforce nameability");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.code() == "semantic.private_type_in_public_signature")
            .count()
            >= 4
    );

    let invalid_results = br#"interface NotAnErrorValue:
    fn marker()

fn primitive_error() -> Result[i64, Text]:
    return Result.Err("bad")

fn interface_error() -> Result[i64, NotAnErrorValue]:
    return Result.Ok(1)

pub fn omitted_error() -> Result[i64]:
    return Result.Ok(1)

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(invalid_results) else {
        panic!("Result errors must be concrete named Data or Resource types");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.code() == "semantic.invalid_result_error_type")
            .count()
            >= 2
    );
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.public_result_requires_error_type")
    );
}

#[test]
fn structural_comparison_requires_synthesized_capability() {
    let comparable = br#"struct Point:
    x: i64
    y: i64

enum Direction:
    North
    South

const ORDERED: bool = Point(x=1, y=9) < Point(x=2, y=0) and Direction.North < Direction.South

@image
fn build() -> Image:
    return Image.new(ordered=ORDERED)
"#;
    let CompilationOutcome::Accepted(accepted) = compile(comparable) else {
        panic!("Data aggregates must receive structural Eq/Order semantics");
    };
    assert!(
        accepted
            .inspection()
            .evaluations()
            .iter()
            .any(|evaluation| {
                evaluation.root() == "image.ORDERED"
                    && evaluation.outcome()
                        == &EvaluationOutcome::Completed(CanonicalValue::Bool(true))
            })
    );

    let resource = br#"resource struct Ticket:
    value: i64

fn invalid(read left: Ticket, read right: Ticket) -> bool:
    return left == right

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(resource) else {
        panic!("Resources must not acquire synthesized comparison");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.binary_type_mismatch")
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
fn one_element_tuple_types_keep_their_required_trailing_comma() {
    let source = br#"const ONE: (i64,) = (1,)

@image
fn build() -> Image:
    return Image.new(value=ONE)
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("a one-element tuple type and value must compile");
    };
    assert!(
        accepted
            .inspection()
            .types()
            .iter()
            .any(|type_| { type_.name() == "ONE" && type_.type_name() == "(i64,)" })
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
fn visibly_decreasing_bounded_integer_recursion_is_admitted() {
    let source = br#"pure fn countdown(remaining: u8) -> u8:
    if remaining == 0u8:
        return 0u8
    return countdown(remaining - 1u8)

const ZERO: u8 = countdown(4u8)

@image
fn build() -> Image:
    return Image.new(value=ZERO)
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("a visibly decreasing fixed-width measure is finite: {outcome:#?}");
    };
    assert!(
        accepted
            .inspection()
            .function_facts()
            .iter()
            .find(|facts| facts.name() == "countdown")
            .is_some_and(|facts| facts.is_bounded())
    );
}

#[test]
fn every_edge_in_a_decreasing_recursive_group_uses_the_group_measure() {
    let source = br#"pure fn even(remaining: u8) -> bool:
    if remaining == 0u8:
        return true
    return odd(remaining - 1u8)

pure fn odd(remaining: u8) -> bool:
    if remaining == 0u8:
        return false
    return even(remaining - 1u8)

const ANSWER: bool = even(4u8)

@image
fn build() -> Image:
    return Image.new(value=ANSWER)
"#;
    let outcome = compile(source);
    assert!(
        matches!(outcome, CompilationOutcome::Accepted(_)),
        "a mutually recursive SCC is finite when every internal edge decreases: {outcome:#?}"
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
fn authenticated_constructor_signatures_create_typed_non_root_symbolic_nodes() {
    let compiler = Compiler::open(CompilerInstallation::with_authenticated_modules(vec![
        ProjectFile::new(
            "src/runtime/topology.wr",
            br#"pub struct Node:
    pub pure fn new(children: [Node]) -> Node:
        panic "sealed Node constructor"
"#,
        ),
    ]))
    .expect("authenticated topology declaration seals");
    let source = br#"from runtime import topology

@image
fn build() -> Image:
    leaf = topology.Node.new(children=[])
    root = topology.Node.new(children=[leaf])
    return Image.new(node=root)
"#;
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("an authenticated signature must expose a sealed constructor: {outcome:#?}");
    };
    assert_eq!(accepted.inspection().constructions().len(), 3);
    let node_types = accepted
        .inspection()
        .identities()
        .iter()
        .filter(|identity| identity.domain() == IdentityDomain::Type && identity.name() == "Node")
        .map(|identity| identity.digest())
        .collect::<std::collections::BTreeSet<_>>();
    let constructed_node_types = accepted
        .inspection()
        .constructions()
        .iter()
        .filter_map(|construction| match construction.kind() {
            ConstructionKind::Node { type_identity } => Some(type_identity),
            ConstructionKind::Image | ConstructionKind::Test => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(constructed_node_types, node_types);
    assert_eq!(
        accepted
            .inspection()
            .constructions()
            .iter()
            .map(|construction| construction.edges().len())
            .sum::<usize>(),
        2,
        "the sealed graph retains both node-to-node and Image-to-node topology",
    );
    assert!(
        accepted
            .inspection()
            .constructions()
            .iter()
            .flat_map(|construction| construction.operands())
            .any(|operand| operand.label() == "children"
                && matches!(operand.value(), CanonicalValue::Array(values) if !values.is_empty()))
    );
    assert_eq!(
        accepted
            .inspection()
            .constructions()
            .iter()
            .filter(|construction| matches!(construction.kind(), ConstructionKind::Node { .. }))
            .count(),
        2
    );
    assert_eq!(
        accepted
            .inspection()
            .constructions()
            .iter()
            .filter(|construction| construction.kind() == ConstructionKind::Image)
            .count(),
        1
    );
}

#[test]
fn repeated_construction_at_one_loop_site_receives_stable_distinct_coordinates() {
    let compiler = Compiler::open(CompilerInstallation::with_authenticated_modules(vec![
        ProjectFile::new(
            "src/runtime/topology.wr",
            br#"pub struct Node:
    pub pure fn new(children: [Node]) -> Node:
        panic "sealed Node constructor"
"#,
        ),
    ]))
    .expect("authenticated topology declaration seals");
    let source = br#"from runtime import topology

@image
fn build() -> Image:
    mut root = topology.Node.new(children=[])
    for index in 0..2:
        root = topology.Node.new(children=[root])
    return Image.new(node=root)
"#;
    let outcome = compiler.compile(
        CompilationRequest::new(
            ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
            Root::Image,
        )
        .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("repeated construction at one static loop site must compile: {outcome:#?}");
    };
    assert_eq!(accepted.inspection().constructions().len(), 4);
    assert_eq!(
        accepted
            .inspection()
            .constructions()
            .iter()
            .map(|construction| construction.identity())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        4,
    );
    assert_eq!(
        accepted
            .inspection()
            .constructions()
            .iter()
            .map(|construction| construction.edges().len())
            .sum::<usize>(),
        3,
    );
}

#[test]
fn sealed_constructor_registry_requires_both_an_authenticated_signature_and_namespace_access() {
    let installation = || {
        CompilerInstallation::with_authenticated_modules(vec![ProjectFile::new(
            "src/runtime/topology.wr",
            br#"pub struct Node:
    pub pure fn new(children: [Node]) -> Node:
        panic "sealed Node constructor"
"#,
        )])
    };
    let compile_with = |source: &[u8]| {
        Compiler::open(installation())
            .expect("authenticated topology declaration seals")
            .compile(
                CompilationRequest::new(
                    ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
                    Root::Image,
                ),
                &Cancellation::new(),
            )
    };

    let wrong_type = compile_with(
        br#"from runtime import topology

@image
fn build() -> Image:
    node = topology.Node.new(children=1)
    return Image.new(node=node)
"#,
    );
    let CompilationOutcome::Rejected(wrong_type) = wrong_type else {
        panic!("the authenticated constructor signature must check operands");
    };
    assert!(
        wrong_type
            .diagnostics()
            .iter()
            .any(|diagnostic| { diagnostic.code() == "semantic.argument_type_mismatch" })
    );

    let ambient = compile_with(
        br#"@image
fn build() -> Image:
    node = Node.new(children=[])
    return Image.new(node=node)
"#,
    );
    let CompilationOutcome::Rejected(ambient) = ambient else {
        panic!("an unimported authenticated constructor must not become ambient authority");
    };
    assert!(
        ambient
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.unresolved_nominal_type"),
        "{ambient:#?}"
    );
}

#[test]
fn project_authored_constructor_signatures_remain_ordinary_wrela() {
    let source = br#"struct Node:
    pure fn new(label: Text) -> Node:
        return Node()

@image
fn build() -> Image:
    node = Node.new(label="ordinary")
    return Image.new(node=node)
"#;
    let CompilationOutcome::Accepted(accepted) = compile(source) else {
        panic!("a project-associated function remains ordinary Wrela");
    };
    assert_eq!(accepted.inspection().constructions().len(), 1);
    assert_eq!(
        accepted.inspection().constructions()[0].kind(),
        ConstructionKind::Image
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

#[test]
fn tuples_field_reads_and_elif_cross_the_complete_compiler_seam() {
    let source = br#"struct Pair:
    left: i64
    right: i64

pure fn choose(value: i64) -> i64:
    if value < 0:
        return 1
    elif value == 0:
        return 2
    else:
        return 3

pure fn read_left() -> i64:
    pair = Pair(left=7, right=9)
    return pair.left

const TUPLE: (i64, bool) = (choose(0), true)
const FIELD: i64 = read_left()

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("accepted Layer 1 expression and branch forms must compile: {outcome:#?}");
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
        evaluation("image.TUPLE"),
        &EvaluationOutcome::Completed(CanonicalValue::Tuple(
            vec![
                CanonicalValue::Integer {
                    type_name: "i64".into(),
                    value: 2,
                },
                CanonicalValue::Bool(true),
            ]
            .into()
        ))
    );
    assert_eq!(
        evaluation("image.FIELD"),
        &EvaluationOutcome::Completed(CanonicalValue::Integer {
            type_name: "i64".into(),
            value: 7,
        })
    );
}

#[test]
fn scalar_bytes_and_multiline_text_literals_are_canonical_values() {
    let source = br#"const SCALAR: Scalar = '\u{1f642}'
const BYTES_VALUE: Bytes = b"A\x00\n"
const MULTILINE: Text = """
    first
    second
    """

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("accepted literal forms must compile: {outcome:#?}");
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
        evaluation("image.SCALAR"),
        &EvaluationOutcome::Completed(CanonicalValue::Scalar('🙂'))
    );
    assert_eq!(
        evaluation("image.BYTES_VALUE"),
        &EvaluationOutcome::Completed(CanonicalValue::Bytes(Arc::from(*b"A\0\n")))
    );
    assert_eq!(
        evaluation("image.MULTILINE"),
        &EvaluationOutcome::Completed(CanonicalValue::Text("first\nsecond\n".into()))
    );

    for malformed in [
        b"const VALUE: Scalar = 'ab'\n".as_slice(),
        b"const VALUE: Bytes = b\"non-ascii \xff\"\n".as_slice(),
        b"const VALUE: Bytes = b\"\\u{41}\"\n".as_slice(),
    ] {
        let CompilationOutcome::Rejected(rejected) = compile(malformed) else {
            panic!("malformed literal must reject");
        };
        assert!(
            rejected
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code() == "syntax.invalid_literal")
        );
    }
}

#[test]
fn fixed_operators_and_checked_indexing_cross_verified_hir() {
    let source = br#"const BITS: u8 = ((0b1100u8 & 0b1010u8) | 0b0001u8) ^ 0b0010u8
const SHIFTED: u16 = 1u16 << 8u16
const INVERTED: u8 = ~0u8
const INDEXED: i64 = [4, 5, 6][1]
const POSITIVE: i64 = +2

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("fixed operator vocabulary must compile: {outcome:#?}");
    };
    let integer = |name: &str| {
        let EvaluationOutcome::Completed(CanonicalValue::Integer { value, .. }) = accepted
            .inspection()
            .evaluations()
            .iter()
            .find(|evaluation| evaluation.root() == name)
            .expect("constant evaluation")
            .outcome()
        else {
            panic!("integer constant outcome");
        };
        *value
    };
    assert_eq!(integer("image.BITS"), 11);
    assert_eq!(integer("image.SHIFTED"), 256);
    assert_eq!(integer("image.INVERTED"), 255);
    assert_eq!(integer("image.INDEXED"), 5);
    assert_eq!(integer("image.POSITIVE"), 2);

    let out_of_bounds = br#"const BAD: i64 = [1][2]

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(out_of_bounds) else {
        panic!("checked indexing must panic during constant evaluation");
    };
    assert!(rejected.diagnostics().iter().any(|diagnostic| {
        diagnostic.code() == "evaluation.panicked"
            && has_text_parameter(diagnostic, "kind", "index_out_of_bounds")
    }));
}

#[test]
fn data_comparisons_have_one_typed_hir_and_evaluator_meaning() {
    let source = br#"struct Point:
    x: i64

enum Choice:
    None
    Some(value: i64)

const TEXT_EQUAL: bool = "same" == "same"
const SCALAR_ORDER: bool = 'a' < 'b'
const BYTES_ORDER: bool = b"ab" < b"ac"
const ARRAY_EQUAL: bool = [1, 2] == [1, 2]
const TUPLE_DIFFERENT: bool = (1, false) != (1, true)
const STRUCT_EQUAL: bool = Point(x=1) == Point(x=1)
const ENUM_DIFFERENT: bool = Choice.Some(value=1) != Choice.None

@image
fn build() -> Image:
    return Image.new()
"#;
    let outcome = compile(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("comparable Data must evaluate after Typed HIR accepts it: {outcome:#?}");
    };
    for name in [
        "image.TEXT_EQUAL",
        "image.SCALAR_ORDER",
        "image.BYTES_ORDER",
        "image.ARRAY_EQUAL",
        "image.TUPLE_DIFFERENT",
        "image.STRUCT_EQUAL",
        "image.ENUM_DIFFERENT",
    ] {
        assert_eq!(
            accepted
                .inspection()
                .evaluations()
                .iter()
                .find(|evaluation| evaluation.root() == name)
                .expect("constant evaluation")
                .outcome(),
            &EvaluationOutcome::Completed(CanonicalValue::Bool(true)),
            "{name}"
        );
    }

    let invalid = br#"const BAD: bool = false < true

@image
fn build() -> Image:
    return Image.new()
"#;
    let CompilationOutcome::Rejected(rejected) = compile(invalid) else {
        panic!("Boolean values are equatable but not ordered");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "semantic.binary_type_mismatch")
    );
}
