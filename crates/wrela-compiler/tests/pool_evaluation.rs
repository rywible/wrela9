use wrela_compiler::{
    ArchitectureProfile, Cancellation, CompilationOutcome, CompilationRequest, Compiler,
    CompilerInstallation, CoreCustodyOperation, InspectSelection, PoolOperation, ProjectFile,
    ProjectSnapshot, RequirementBounds, RequirementCategory, Root,
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

fn compile_planned(source: &[u8]) -> CompilationOutcome {
    Compiler::open(CompilerInstallation::layer1())
        .expect("Layer 1 distribution opens")
        .compile(
            CompilationRequest::new(
                ProjectSnapshot::new(vec![ProjectFile::new("src/image.wr", source)]),
                Root::Image,
            )
            .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
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

#[test]
fn sequential_required_allocations_use_one_path_sensitive_slot() {
    let source = br#"from core import pool as pools

resource struct Token:
    value: i64

@image
fn build() -> Image:
    token = Token(value=7)
    with pools.scoped(capacity=1) as scratch:
        first = scratch.allocate(value=take token)
        token = scratch.reclaim(allocation=take first)
        second = scratch.allocate(value=take token)
        token = scratch.reclaim(allocation=take second)
    return Image.new(token=take token)
"#;

    let outcome = compile(source);
    assert!(
        matches!(outcome, CompilationOutcome::Accepted(_)),
        "two sequential allocation sites have a path-sensitive peak commitment of one: {outcome:#?}"
    );
}

#[test]
fn full_try_allocate_returns_the_original_resource_before_commit() {
    let source = br#"from core import pool as pools

resource struct Token:
    value: i64

@image
fn build() -> Image:
    first = Token(value=1)
    second = Token(value=2)
    with pools.scoped(capacity=1) as scratch:
        held = scratch.allocate(value=take first)
        attempted = scratch.try_allocate(value=take second)
        match attempted:
            case Result.Err(take full):
                second = take full.value
            case _:
                panic "a full Pool accepted another live allocation"
        first = scratch.reclaim(allocation=take held)
    return Image.new(first=take first, second=take second)
"#;

    let outcome = compile(source);
    assert!(
        matches!(outcome, CompilationOutcome::Accepted(_)),
        "PoolFull must retain the exact pre-commit resource value: {outcome:#?}"
    );
}

#[test]
fn permit_converts_one_reservation_to_live_without_growing_commitment() {
    let source = br#"from core import pool as pools

resource struct Token:
    value: i64

@image
fn build() -> Image:
    token = Token(value=7)
    with pools.scoped(capacity=1) as scratch:
        permit: pools.Permit[Token] = scratch.reserve()
        allocation = scratch.consume(permit=take permit, value=take token)
        token = scratch.reclaim(allocation=take allocation)
    return Image.new(token=take token)
"#;

    let outcome = compile(source);
    assert!(
        matches!(outcome, CompilationOutcome::Accepted(_)),
        "consuming a Permit must convert, rather than add, commitment: {outcome:#?}"
    );
}

#[test]
fn held_permit_is_compiler_reclaimed_when_the_pool_scope_closes() {
    let source = br#"from core import pool as pools

@image
fn build() -> Image:
    with pools.scoped(capacity=1) as scratch:
        permit: pools.Permit[i64] = scratch.reserve()
    return Image.new()
"#;

    let outcome = compile(source);
    assert!(
        matches!(outcome, CompilationOutcome::Accepted(_)),
        "legal scope cleanup must release an outstanding Permit: {outcome:#?}"
    );
}

#[test]
fn copied_key_misses_after_reclaim_advances_the_slot_generation() {
    let source = br#"from core import pool as pools

@image
fn build() -> Image:
    mut value = 0
    with pools.scoped(capacity=1) as scratch:
        allocation = scratch.allocate(value=7)
        key = allocation.key
        value = scratch.reclaim(allocation=take allocation)
        stale = scratch.lookup(key=key)
        match stale:
            case Option.None:
                pass
            case Option.Some(_):
                panic "a stale Key found a reclaimed allocation"
    return Image.new(value=value)
"#;

    let outcome = compile(source);
    assert!(
        matches!(outcome, CompilationOutcome::Accepted(_)),
        "a copied Key must become a miss after reclaim: {outcome:#?}"
    );
}

#[test]
fn key_from_another_pool_identity_is_always_a_miss() {
    let source = br#"from core import pool as pools

@image
fn build() -> Image:
    mut value = 0
    with pools.scoped(capacity=1) as first_pool:
        allocation = first_pool.allocate(value=7)
        key = allocation.key
        with pools.scoped(capacity=1) as second_pool:
            foreign = second_pool.lookup(key=key)
            match foreign:
                case Option.None:
                    pass
                case Option.Some(_):
                    panic "a foreign Pool accepted another Pool's Key"
        value = first_pool.reclaim(allocation=take allocation)
    return Image.new(value=value)
"#;

    let outcome = compile(source);
    assert!(
        matches!(outcome, CompilationOutcome::Accepted(_)),
        "Key identity includes the exact Pool: {outcome:#?}"
    );
}

#[test]
fn whole_image_planning_emits_exact_evidence_for_required_pool_operations() {
    let source = br#"from core import pool as pools

resource struct Token:
    value: i64

@image
fn build() -> Image:
    token = Token(value=7)
    with pools.scoped(capacity=1) as scratch:
        allocation = scratch.allocate(value=take token)
        token = scratch.reclaim(allocation=take allocation)
        permit: pools.Permit[Token] = scratch.reserve()
        scratch.release(permit=take permit)
    return Image.new(token=take token)
"#;

    let outcome = compile_planned(source);
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("a discharged allocation and Permit fit one usable slot: {outcome:#?}");
    };
    let planning = accepted
        .inspection()
        .planning_foundation()
        .expect("planning inspection");
    assert_eq!(planning.pools().len(), 1);
    assert_eq!(planning.pools()[0].declared_capacity(), 1);
    assert_eq!(planning.pools()[0].peak_commitment(), 1);
    assert_eq!(planning.pool_admission_evidence().len(), 2);
    assert!(
        planning
            .pool_admission_evidence()
            .iter()
            .any(|evidence| evidence.operation() == PoolOperation::Allocate)
    );
    assert!(
        planning
            .pool_admission_evidence()
            .iter()
            .any(|evidence| evidence.operation() == PoolOperation::Reserve)
    );
    assert!(planning.requirements().iter().any(|requirement| {
        requirement.category() == RequirementCategory::CapacityPressure
            && matches!(
                requirement.bounds(),
                RequirementBounds::PoolCapacity {
                    declared: 1,
                    usable: 1,
                    peak_committed: 1,
                    ..
                }
            )
    }));
    let model = planning.pool_model();
    assert!(model.agrees());
    assert!(model.covers_accepted());
    assert!(model.covers_full());
    assert!(model.covers_released());
    assert!(model.covers_reserved());
    assert!(model.covers_stale());
    assert!(model.covers_retired());
    let core = accepted
        .inspection()
        .core_program()
        .expect("Core inspection");
    let proof_effects = core
        .executables()
        .iter()
        .flat_map(|executable| executable.custody_effects())
        .filter(|effect| effect.operation() == CoreCustodyOperation::ProofCondition)
        .collect::<Vec<_>>();
    assert_eq!(proof_effects.len(), 2);
    let evidence = planning
        .pool_admission_evidence()
        .iter()
        .map(|evidence| {
            (
                evidence.requirement_identity(),
                evidence.requirement_current_meaning(),
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        proof_effects
            .iter()
            .map(|effect| {
                (
                    effect.requirement_identity().unwrap(),
                    effect.requirement_current_meaning().unwrap(),
                )
            })
            .collect::<std::collections::BTreeSet<_>>(),
        evidence
    );
    assert!(
        proof_effects
            .iter()
            .all(|effect| effect.retains_fallible_source_type())
    );
}

#[test]
fn whole_image_admission_rejects_simultaneous_required_commitment_over_capacity() {
    let source = br#"from core import pool as pools

@image
fn build() -> Image:
    mut one = 0
    mut two = 0
    with pools.scoped(capacity=1) as scratch:
        first = scratch.allocate(value=1)
        second = scratch.allocate(value=2)
        one = scratch.reclaim(allocation=take first)
        two = scratch.reclaim(allocation=take second)
    return Image.new(one=one, two=two)
"#;

    let CompilationOutcome::Rejected(rejected) = compile_planned(source) else {
        panic!("whole-Image admission must reject commitment above declared capacity");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "admission.pool_capacity"),
        "{rejected:#?}"
    );
    assert!(rejected.inspection().planning_foundation().is_none());
    assert!(rejected.inspection().core_program().is_none());
}

#[test]
fn compiler_reclaimed_permit_keeps_the_slot_committed_until_scope_cleanup() {
    let source = br#"from core import pool as pools

@image
fn build() -> Image:
    mut value = 0
    with pools.scoped(capacity=1) as scratch:
        permit: pools.Permit[i64] = scratch.reserve()
        allocation = scratch.allocate(value=7)
        value = scratch.reclaim(allocation=take allocation)
    return Image.new(value=value)
"#;

    let CompilationOutcome::Rejected(rejected) = compile_planned(source) else {
        panic!(
            "compiler reclamation runs at cleanup, so the intervening allocation needs a second slot"
        );
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "admission.pool_capacity"),
        "{rejected:#?}"
    );
    assert!(rejected.inspection().planning_foundation().is_none());
    assert!(rejected.inspection().core_program().is_none());
}

#[test]
fn outer_pool_commitment_is_counted_inside_a_nested_pool_scope() {
    let source = br#"from core import pool as pools

@image
fn build() -> Image:
    mut one = 0
    mut two = 0
    with pools.scoped(capacity=1) as outer:
        first = outer.allocate(value=1)
        with pools.scoped(capacity=1) as inner:
            second = outer.allocate(value=2)
            one = outer.reclaim(allocation=take first)
            two = outer.reclaim(allocation=take second)
    return Image.new(one=one, two=two)
"#;

    let CompilationOutcome::Rejected(rejected) = compile_planned(source) else {
        panic!("nesting another Pool scope cannot hide commitment on the outer Pool");
    };
    assert!(
        rejected
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code() == "admission.pool_capacity"),
        "{rejected:#?}"
    );
    assert!(rejected.inspection().planning_foundation().is_none());
    assert!(rejected.inspection().core_program().is_none());
}

#[test]
fn successful_try_allocate_commits_exactly_one_live_slot() {
    let source = br#"from core import pool as pools

resource struct Token:
    value: i64

@image
fn build() -> Image:
    token = Token(value=7)
    with pools.scoped(capacity=1) as scratch:
        attempted = scratch.try_allocate(value=take token)
        match attempted:
            case Result.Ok(take allocation):
                token = scratch.reclaim(allocation=take allocation)
            case _:
                panic "an empty Pool reported full"
    return Image.new(token=take token)
"#;

    let outcome = compile(source);
    assert!(
        matches!(outcome, CompilationOutcome::Accepted(_)),
        "successful try_allocate commits one live slot and reclaim releases it: {outcome:#?}"
    );
}
