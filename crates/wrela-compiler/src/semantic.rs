use std::collections::{BTreeMap, BTreeSet};

use crate::compiler::{
    Cancellation, ConstructionObservation, Defect, Diagnostic, EvaluationObservation,
    EvaluationOutcome, FunctionFactsObservation, InferredErrorObservation, ProjectFile,
    RecoveryAction, Root, SourceRange, TestApplicationObservation,
};
use crate::evaluator::{Constant, Construction, Engine, Function};
use crate::syntax::ParsedSource;
use crate::typed_hir::{self, BuildAuthority};

pub(crate) struct Analysis {
    pub(crate) diagnostics: Vec<Diagnostic>,
    pub(crate) function_facts: Vec<FunctionFactsObservation>,
    pub(crate) inferred_errors: Vec<InferredErrorObservation>,
    pub(crate) evaluations: Vec<EvaluationObservation>,
    pub(crate) constructions: Vec<ConstructionObservation>,
    pub(crate) test_plan: Vec<TestApplicationObservation>,
    pub(crate) defect: Option<Defect>,
    pub(crate) cancelled: bool,
}

pub(crate) fn analyze<'a>(
    parsed_sources: &BTreeMap<String, ParsedSource>,
    files: &BTreeMap<&'a str, &'a ProjectFile>,
    root: Root,
    cancellation: &Cancellation,
    executable_allowed: bool,
    build_authority: &BuildAuthority,
) -> Analysis {
    let mut diagnostics = Vec::new();
    let mut functions = BTreeMap::new();
    let mut constants = BTreeMap::new();
    let mut facts = BTreeMap::new();
    let mut private_types = BTreeSet::new();
    let mut public_function_headers = Vec::new();
    let mut nominal_types = BTreeSet::new();
    let mut image_functions = Vec::new();
    let mut suites = Vec::new();
    let mut declared_names = BTreeMap::new();

    for (path, parsed) in parsed_sources {
        if cancellation.is_cancelled() {
            return cancelled();
        }
        let file = files[path.as_str()];
        validate_attributes(file, &mut diagnostics);
        for declaration in &parsed.declarations {
            if !declaration.structurally_valid {
                continue;
            }
            let declaration_key = format!("{}.{}", module_name(path), declaration.name);
            if declared_names
                .insert(declaration_key, declaration.range.clone())
                .is_some()
            {
                diagnostics.push(
                    Diagnostic::new(
                        "semantic.duplicate_declaration",
                        declaration.range.clone(),
                        RecoveryAction::None,
                    )
                    .with_parameter("name", declaration.name.clone()),
                );
                continue;
            }
            let bytes = parsed.declaration_bytes(file, declaration);
            let text = String::from_utf8_lossy(bytes);
            if matches!(
                declaration.kind,
                "struct" | "resource_struct" | "enum" | "type_alias"
            ) {
                nominal_types.insert(declaration.name.clone());
                if !declaration.public {
                    private_types.insert(declaration.name.clone());
                }
            }
            if declaration.kind == "function" {
                if let Some(function) =
                    parse_function(file, declaration.start, declaration.end, declaration.public)
                {
                    let qualified = qualify(path, &function.name);
                    let body_text = function_body(&function);
                    let suspends = text.trim_start().starts_with("async ")
                        || text.trim_start().starts_with("pub async ")
                        || body_text.contains("await ");
                    let forbidden = body_text.contains("send ")
                        || body_text.contains("try_send ")
                        || body_text.contains("Facility.");
                    let eligible = !suspends && !forbidden;
                    let pure = eligible
                        && !body_text.contains("Image.new(")
                        && !body_text.contains("Test.new(");
                    let may_panic = body_text.contains("panic ")
                        || body_text.contains(" / ")
                        || body_text.contains(" % ");
                    facts.insert(
                        qualified.clone(),
                        FunctionFacts {
                            name: function.name.clone(),
                            pure,
                            may_panic,
                            suspends,
                            evaluator_eligible: eligible,
                        },
                    );
                    if declaration.public {
                        public_function_headers
                            .push((declaration.range.clone(), first_line(&text).to_owned()));
                    }
                    if has_image_attribute(file.bytes(), declaration.start) {
                        image_functions.push((
                            path.clone(),
                            function.name.clone(),
                            declaration.range.clone(),
                        ));
                    }
                    functions.insert(qualified, function);
                }
            } else if declaration.kind == "constant"
                && let Some(constant) = parse_constant(file, declaration.start)
            {
                constants.insert(qualify(path, &constant.name), constant);
            } else if declaration.kind == "suite" {
                suites.extend(parse_suite(file, declaration.start, declaration.end));
                validate_suite(file, declaration, &mut diagnostics);
            }
        }
    }

    let qualified_functions = functions.clone();
    for (importer_path, parsed) in parsed_sources {
        let importer_module = module_name(importer_path);
        for import in &parsed.imports {
            if declared_names.contains_key(&format!("{importer_module}.{}", import.alias)) {
                diagnostics.push(
                    Diagnostic::new(
                        "semantic.import_alias_conflict",
                        import.range.clone(),
                        RecoveryAction::None,
                    )
                    .with_parameter("alias", import.alias.clone()),
                );
                continue;
            }
            let target_module = module_name(&import.target_path);
            for (qualified, function) in &qualified_functions {
                if function.public
                    && let Some(member) = qualified.strip_prefix(&format!("{target_module}."))
                {
                    functions.insert(
                        format!("{importer_module}|{}.{}", import.alias, member),
                        function.clone(),
                    );
                }
            }
        }
    }

    solve_function_facts(&mut facts, &functions);
    let mut function_facts = facts
        .into_values()
        .map(|facts| {
            FunctionFactsObservation::new(
                facts.name,
                facts.pure,
                facts.may_panic,
                facts.suspends,
                facts.evaluator_eligible,
            )
        })
        .collect::<Vec<_>>();

    for (range, header) in public_function_headers {
        if let Some(private) = private_types
            .iter()
            .find(|private| contains_type_name(&header, private))
        {
            diagnostics.push(
                Diagnostic::new(
                    "semantic.private_type_in_public_signature",
                    range.clone(),
                    RecoveryAction::None,
                )
                .with_parameter("private_type", private.clone()),
            );
        }
        if partial_result_return(&header).is_some() {
            diagnostics.push(Diagnostic::new(
                "semantic.public_result_requires_error_type",
                range,
                RecoveryAction::None,
            ));
        }
    }

    for constant in constants.values() {
        if contains_build_constructor(&semantic_code_line(&constant.expression)) {
            diagnostics.push(Diagnostic::new(
                "semantic.build_constructor_outside_image",
                constant.source.clone(),
                RecoveryAction::None,
            ));
        }
    }
    let image_reachable = image_functions
        .first()
        .map_or_else(BTreeSet::new, |(path, name, _)| {
            reachable_functions(&qualify(path, name), &functions)
        });
    let mut checked_sources = BTreeSet::new();
    for function in functions.values() {
        if checked_sources.insert(function.source.clone())
            && contains_build_constructor(&function_body(function))
            && !image_reachable.contains(&function.source)
        {
            diagnostics.push(Diagnostic::new(
                "semantic.build_constructor_outside_image",
                function.source.clone(),
                RecoveryAction::None,
            ));
        }
    }

    let (inferred_errors, error_diagnostics) = infer_private_errors(&functions);
    diagnostics.extend(error_diagnostics);
    for range in unproven_recursive_cycles(&functions) {
        diagnostics.push(Diagnostic::new(
            "semantic.unproven_recursive_bound",
            range,
            RecoveryAction::None,
        ));
    }

    let expected_root_path = match root {
        Root::Image => "src/image.wr",
        Root::Test => "src/test.wr",
    };
    if executable_allowed && image_functions.is_empty() {
        diagnostics.push(Diagnostic::new(
            "semantic.missing_image_constructor",
            SourceRange::new(expected_root_path, 0, 0),
            RecoveryAction::None,
        ));
    } else if executable_allowed && image_functions.len() > 1 {
        for (_, _, range) in image_functions.iter().skip(1) {
            diagnostics.push(Diagnostic::new(
                "semantic.multiple_image_constructors",
                range.clone(),
                RecoveryAction::None,
            ));
        }
    } else if executable_allowed && image_functions[0].0 != expected_root_path {
        diagnostics.push(Diagnostic::new(
            "semantic.image_constructor_outside_root",
            image_functions[0].2.clone(),
            RecoveryAction::None,
        ));
    }

    let mut evaluations = Vec::new();
    let mut constructions = Vec::new();
    let mut test_plan = Vec::new();
    let mut verification_defect = None;
    let allowed_tests = suites
        .iter()
        .map(|test| format!("{}.{}", test.suite, test.test))
        .collect::<BTreeSet<_>>();
    let program = match typed_hir::verify(
        functions,
        constants.clone(),
        &allowed_tests,
        build_authority,
        &nominal_types,
        cancellation,
    ) {
        Ok(program) => Some(program),
        Err(typed_hir::VerificationFailure::Defect { evidence }) => {
            verification_defect = Some(Defect::new("typed HIR verification", evidence));
            None
        }
        Err(typed_hir::VerificationFailure::Creator { kind, site }) => {
            diagnostics.push(
                Diagnostic::new("semantic.invalid_typed_hir", site, RecoveryAction::None)
                    .with_parameter("kind", kind),
            );
            None
        }
        Err(typed_hir::VerificationFailure::Cancelled) => return cancelled(),
    };
    if executable_allowed && diagnostics.is_empty() {
        let program = program.as_ref().expect("verified when no diagnostics");
        let constant_names = constants.keys().cloned().collect::<Vec<_>>();
        for name in constant_names {
            let mut engine = Engine::new(program, cancellation);
            let run = engine.evaluate_constant(&name);
            if run.outcome == EvaluationOutcome::Cancelled {
                return cancelled();
            }
            map_evaluation_failure(&run.outcome, &constants[&name].source, &mut diagnostics);
            evaluations.push(EvaluationObservation::new(name, run.outcome, run.receipt));
        }
        for (path, expression, range) in compile_time_assertions(parsed_sources, files) {
            let module = module_name(&path);
            let Ok(expression) = program.verify_expression(
                &module,
                &expression,
                &range,
                &allowed_tests,
                build_authority,
                &nominal_types,
                cancellation,
            ) else {
                diagnostics.push(Diagnostic::new(
                    "semantic.invalid_comptime_expression",
                    range,
                    RecoveryAction::None,
                ));
                continue;
            };
            let mut engine = Engine::new(program, cancellation);
            let run = engine.evaluate_expression(&expression, &range);
            if run.outcome == EvaluationOutcome::Cancelled {
                return cancelled();
            }
            if run.outcome != EvaluationOutcome::Completed(crate::CanonicalValue::Bool(true)) {
                diagnostics.push(Diagnostic::new(
                    "evaluation.assertion_failed",
                    range,
                    RecoveryAction::None,
                ));
            }
            evaluations.push(EvaluationObservation::new(
                format!("{}.comptime_assert", module_name(&path)),
                run.outcome,
                run.receipt,
            ));
        }
        if diagnostics.is_empty()
            && let Some((path, image_name, image_range)) = image_functions.first()
        {
            let mut engine = Engine::new(program, cancellation);
            let run = engine.evaluate_function(&qualify(path, image_name));
            if run.outcome == EvaluationOutcome::Cancelled {
                return cancelled();
            }
            map_evaluation_failure(&run.outcome, image_range, &mut diagnostics);
            if root == Root::Test {
                test_plan = plan_test_applications(
                    &suites,
                    &run.test_applications,
                    image_range,
                    &mut diagnostics,
                );
            }
            if let Err(kind) = seal_construction_graph(&run.outcome, &run.constructions) {
                diagnostics.push(
                    Diagnostic::new(
                        "construction.invalid_graph",
                        image_range.clone(),
                        RecoveryAction::None,
                    )
                    .with_parameter("kind", kind),
                );
            }
            constructions.extend(run.constructions.into_iter().map(|construction| {
                ConstructionObservation::new(
                    construction.identity,
                    construction.kind,
                    construction.site,
                )
            }));
            evaluations.push(EvaluationObservation::new(
                format!("{}.{}", module_name(path), image_name),
                run.outcome,
                run.receipt,
            ));
        }
    }

    function_facts.sort_by(|left, right| left.name().cmp(right.name()));
    evaluations.sort_by(|left, right| left.root().cmp(right.root()));
    constructions.sort_by_key(ConstructionObservation::identity);
    Analysis {
        diagnostics,
        function_facts,
        inferred_errors,
        evaluations,
        constructions,
        test_plan,
        defect: verification_defect,
        cancelled: false,
    }
}

fn seal_construction_graph(
    outcome: &EvaluationOutcome,
    constructions: &[Construction],
) -> Result<(), &'static str> {
    let EvaluationOutcome::Completed(crate::CanonicalValue::SymbolicHandle {
        kind,
        identity: root,
    }) = outcome
    else {
        return Ok(());
    };
    if kind.as_ref() != "Image" {
        return Err("returned_root_is_not_image");
    }
    if constructions
        .iter()
        .filter(|construction| construction.kind == "Image")
        .count()
        != 1
    {
        return Err("image_root_count");
    }
    let by_identity = constructions
        .iter()
        .map(|construction| (construction.identity, construction))
        .collect::<BTreeMap<_, _>>();
    if by_identity.len() != constructions.len() {
        return Err("duplicate_construction_identity");
    }
    let mut reachable = BTreeSet::new();
    let mut pending = vec![*root];
    while let Some(identity) = pending.pop() {
        if !reachable.insert(identity) {
            continue;
        }
        let Some(node) = by_identity.get(&identity) else {
            return Err("cross_root_handle");
        };
        pending.extend(node.edges.iter().copied());
    }
    if reachable.len() != constructions.len() {
        return Err("unreachable_construction");
    }
    Ok(())
}

fn plan_test_applications(
    suites: &[SuiteTest],
    applications: &[String],
    image_range: &SourceRange,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<TestApplicationObservation> {
    let expected = suites
        .iter()
        .map(|test| format!("{}.{}", test.suite, test.test))
        .collect::<BTreeSet<_>>();
    for test in &expected {
        let count = applications
            .iter()
            .filter(|application| *application == test)
            .count();
        if count == 0 {
            diagnostics.push(
                Diagnostic::new(
                    "test.missing_application",
                    image_range.clone(),
                    RecoveryAction::None,
                )
                .with_parameter("test", test.clone()),
            );
        } else if count > 1 {
            diagnostics.push(
                Diagnostic::new(
                    "test.duplicate_application",
                    image_range.clone(),
                    RecoveryAction::None,
                )
                .with_parameter("test", test.clone()),
            );
        }
    }
    applications
        .iter()
        .enumerate()
        .filter_map(|(order, application)| {
            let (suite, test) = application.rsplit_once('.')?;
            Some(TestApplicationObservation::new(
                suite,
                test,
                u32::try_from(order).unwrap_or(u32::MAX),
            ))
        })
        .collect()
}

#[derive(Clone, Debug)]
struct SuiteTest {
    suite: String,
    test: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FunctionFacts {
    name: String,
    pure: bool,
    may_panic: bool,
    suspends: bool,
    evaluator_eligible: bool,
}

fn solve_function_facts(
    facts: &mut BTreeMap<String, FunctionFacts>,
    functions: &BTreeMap<String, Function>,
) {
    for _ in 0..=facts.len() {
        let previous = facts.clone();
        for (name, current) in &mut *facts {
            let Some(function) = functions.get(name) else {
                continue;
            };
            for called in called_functions(&function_body(function)) {
                let imported = format!("{}|{called}", function.module);
                let local = format!("{}.{}", function.module, called);
                let resolved = if functions.contains_key(&imported) {
                    imported
                } else {
                    local
                };
                let Some(callee) = functions.get(&resolved) else {
                    continue;
                };
                let canonical = format!("{}.{}", callee.module, callee.name);
                let Some(callee_facts) = previous.get(&canonical) else {
                    continue;
                };
                current.pure &= callee_facts.pure;
                current.may_panic |= callee_facts.may_panic;
                current.suspends |= callee_facts.suspends;
                current.evaluator_eligible &= callee_facts.evaluator_eligible;
            }
        }
        if *facts == previous {
            break;
        }
    }
}

fn parse_suite(file: &ProjectFile, start: u64, end: u64) -> Vec<SuiteTest> {
    let Some(bytes) = crate::syntax::checked_slice(file.bytes(), start, end) else {
        return Vec::new();
    };
    let Ok(source) = std::str::from_utf8(bytes) else {
        return Vec::new();
    };
    let Some(header) = source.lines().next() else {
        return Vec::new();
    };
    let Some(suite) = header
        .trim()
        .strip_prefix("pub suite ")
        .or_else(|| header.trim().strip_prefix("suite "))
        .and_then(|rest| rest.split(':').next())
    else {
        return Vec::new();
    };
    source
        .lines()
        .skip(1)
        .filter_map(|line| {
            let rest = line.strip_prefix("    ")?;
            if rest.starts_with(' ') {
                return None;
            }
            let rest = rest
                .strip_prefix("async test ")
                .or_else(|| rest.strip_prefix("test "))?;
            let test = rest.split('(').next()?.trim();
            (!test.is_empty()).then(|| SuiteTest {
                suite: suite.to_owned(),
                test: test.to_owned(),
            })
        })
        .collect()
}

fn validate_suite(
    file: &ProjectFile,
    declaration: &crate::syntax::Declaration,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !declaration.public {
        diagnostics.push(Diagnostic::new(
            "test.suite_must_be_public",
            declaration.range.clone(),
            RecoveryAction::None,
        ));
    }
    let Some(bytes) =
        crate::syntax::checked_slice(file.bytes(), declaration.start, declaration.end)
    else {
        return;
    };
    let Ok(source) = std::str::from_utf8(bytes) else {
        return;
    };
    let mut offset = declaration.start;
    for line in source.lines() {
        let trimmed = line.trim_start();
        if (trimmed.starts_with("test ") || trimmed.starts_with("async test "))
            && let Some(parameters) = trimmed
                .split_once('(')
                .and_then(|(_, rest)| rest.split_once(')'))
                .map(|(parameters, _)| parameters)
        {
            for parameter in split_commas(parameters) {
                if !parameter.is_empty() && !parameter.starts_with("take ") {
                    diagnostics.push(Diagnostic::new(
                        "test.parameter_requires_take",
                        SourceRange::from_u64(
                            file.path(),
                            offset,
                            offset.saturating_add(u64::try_from(line.len()).unwrap_or(u64::MAX)),
                        ),
                        RecoveryAction::None,
                    ));
                }
            }
        }
        offset = offset.saturating_add(u64::try_from(line.len() + 1).unwrap_or(u64::MAX));
    }
}

fn parse_function(file: &ProjectFile, start: u64, end: u64, public: bool) -> Option<Function> {
    let source =
        std::str::from_utf8(crate::syntax::checked_slice(file.bytes(), start, end)?).ok()?;
    let mut lines = source.lines();
    let header = lines.next()?.trim();
    let fn_offset = header.find("fn ")? + 3;
    let after_fn = &header[fn_offset..];
    let open = after_fn.find('(')?;
    let name = after_fn[..open].split('[').next()?.trim().to_owned();
    let close = after_fn.rfind(')')?;
    let parameters = split_commas(&after_fn[open + 1..close])
        .into_iter()
        .filter(|parameter| !parameter.is_empty())
        .filter_map(|parameter| {
            let (name, type_name) = parameter.split_once(':')?;
            let name = name
                .split_ascii_whitespace()
                .last()
                .unwrap_or(name)
                .to_owned();
            Some((name, type_name.trim().to_owned()))
        })
        .collect();
    let return_type = after_fn[close + 1..]
        .trim()
        .strip_prefix("->")
        .map_or_else(String::new, |value| {
            value.trim().trim_end_matches(':').trim().to_owned()
        });
    let mut line_offset = start
        .saturating_add(u64::try_from(header.len()).ok()?)
        .saturating_add(1);
    let body = lines
        .map(|line| {
            let result = (line_offset, line.to_owned());
            line_offset = line_offset
                .saturating_add(u64::try_from(line.len()).unwrap_or(u64::MAX))
                .saturating_add(1);
            result
        })
        .collect();
    Some(Function {
        name,
        module: module_name(file.path()),
        public,
        parameters,
        return_type,
        body,
        source: SourceRange::from_u64(
            file.path(),
            start,
            start.saturating_add(u64::try_from(header.len()).ok()?),
        ),
    })
}

fn parse_constant(file: &ProjectFile, start: u64) -> Option<Constant> {
    let end = u64::try_from(file.bytes().len()).ok()?;
    let line = std::str::from_utf8(crate::syntax::checked_slice(file.bytes(), start, end)?)
        .ok()?
        .lines()
        .next()?;
    let rest = line.trim().strip_prefix("const ")?;
    let (name, rest) = rest.split_once(':')?;
    let (type_name, expression) = rest.split_once('=')?;
    Some(Constant {
        name: name.trim().to_owned(),
        module: module_name(file.path()),
        type_name: type_name.trim().to_owned(),
        expression: expression.trim().to_owned(),
        source: SourceRange::from_u64(
            file.path(),
            start,
            start.saturating_add(u64::try_from(line.len()).ok()?),
        ),
    })
}

fn infer_private_errors(
    functions: &BTreeMap<String, Function>,
) -> (Vec<InferredErrorObservation>, Vec<Diagnostic>) {
    let unique = functions
        .iter()
        .filter(|(name, function)| *name == &format!("{}.{}", function.module, function.name))
        .map(|(name, function)| (name.clone(), function.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut sets = unique
        .iter()
        .filter(|(_, function)| partial_result_return(&function.return_type).is_some())
        .map(|(name, function)| {
            let body = function_body(function);
            (
                name.clone(),
                error_contributors(&body)
                    .into_iter()
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for _ in 0..=unique.len() {
        let previous = sets.clone();
        for (name, errors) in &mut sets {
            let function = &unique[name];
            for callee in propagated_calls(&function_body(function)) {
                let Some(callee) = resolve_function_key(function, &callee, functions) else {
                    continue;
                };
                if let Some(explicit) =
                    explicit_result_error(unique.get(&callee).map_or("", |f| &f.return_type))
                {
                    errors.insert(explicit.to_owned());
                } else if let Some(inferred) = previous.get(&callee) {
                    errors.extend(inferred.iter().cloned());
                }
            }
        }
        if sets == previous {
            break;
        }
    }

    let mut observations = Vec::new();
    let mut diagnostics = Vec::new();
    for (name, errors) in &sets {
        let function = &unique[name];
        match errors.iter().collect::<Vec<_>>().as_slice() {
            [error_type] => observations.push(InferredErrorObservation::new(
                function.name.clone(),
                (*error_type).clone(),
            )),
            [] => diagnostics.push(Diagnostic::new(
                "semantic.unconstrained_inferred_error",
                function.source.clone(),
                RecoveryAction::None,
            )),
            _ => diagnostics.push(Diagnostic::new(
                "semantic.conflicting_inferred_errors",
                function.source.clone(),
                RecoveryAction::None,
            )),
        }
    }

    for function in unique.values() {
        let Some(caller_error) = explicit_result_error(&function.return_type) else {
            continue;
        };
        for callee in propagated_calls(&function_body(function)) {
            let Some(callee) = resolve_function_key(function, &callee, functions) else {
                continue;
            };
            let callee_error =
                explicit_result_error(unique.get(&callee).map_or("", |f| &f.return_type))
                    .map(str::to_owned)
                    .or_else(|| {
                        sets.get(&callee).and_then(|errors| {
                            (errors.len() == 1).then(|| errors.first().expect("one error").clone())
                        })
                    });
            if callee_error
                .as_deref()
                .is_some_and(|error| error != caller_error)
            {
                diagnostics.push(Diagnostic::new(
                    "semantic.propagation_error_mismatch",
                    function.source.clone(),
                    RecoveryAction::None,
                ));
            }
        }
    }
    observations.sort_by(|left, right| left.function().cmp(right.function()));
    (observations, diagnostics)
}

fn resolve_function_key(
    caller: &Function,
    called: &str,
    functions: &BTreeMap<String, Function>,
) -> Option<String> {
    let imported = format!("{}|{called}", caller.module);
    let local = format!("{}.{}", caller.module, called);
    let function = functions.get(&imported).or_else(|| functions.get(&local))?;
    Some(format!("{}.{}", function.module, function.name))
}

fn function_body(function: &Function) -> String {
    function
        .body
        .iter()
        .map(|(_, line)| semantic_code_line(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn semantic_code_line(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut quote = None;
    let mut escaped = false;
    for character in line.chars() {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active {
                quote = None;
                result.push(character);
            } else {
                result.push(' ');
            }
        } else if character == '#' {
            break;
        } else if matches!(character, '\'' | '"') {
            quote = Some(character);
            result.push(character);
        } else {
            result.push(character);
        }
    }
    result
}

fn explicit_result_error(return_type: &str) -> Option<&str> {
    let inner = return_type.strip_prefix("Result[")?.strip_suffix(']')?;
    let arguments = split_commas(inner);
    (arguments.len() == 2).then(|| arguments[1].trim())
}

fn propagated_calls(body: &str) -> Vec<String> {
    let mut calls = Vec::new();
    for (question, _) in body.match_indices('?') {
        let before = &body[..question];
        let Some(open) = before.rfind('(') else {
            continue;
        };
        let name = before[..open]
            .trim_end()
            .rsplit(|character: char| {
                !(character.is_ascii_alphanumeric() || matches!(character, '_' | '.'))
            })
            .next()
            .unwrap_or_default()
            .rsplit('.')
            .next()
            .unwrap_or_default();
        if !name.is_empty() {
            calls.push(name.to_owned());
        }
    }
    calls
}

fn error_contributors(body: &str) -> Vec<String> {
    let mut contributors = BTreeSet::new();
    let mut remaining = body;
    let needle = "Result.Err(";
    while let Some(index) = remaining.find(needle) {
        let after = &remaining[index + needle.len()..];
        let candidate = after
            .trim_start()
            .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .next()
            .unwrap_or_default();
        if !candidate.is_empty() {
            contributors.insert(candidate.to_owned());
        }
        remaining = after;
    }
    contributors.into_iter().collect()
}

fn compile_time_assertions<'a>(
    parsed_sources: &BTreeMap<String, ParsedSource>,
    files: &BTreeMap<&'a str, &'a ProjectFile>,
) -> Vec<(String, String, SourceRange)> {
    let mut assertions = Vec::new();
    for path in parsed_sources.keys() {
        let mut offset = 0;
        for physical in files[path.as_str()]
            .bytes()
            .split_inclusive(|byte| *byte == b'\n')
        {
            let line = physical.strip_suffix(b"\n").unwrap_or(physical);
            if let Ok(text) = std::str::from_utf8(line)
                && let Some(expression) = text.strip_prefix("comptime assert ")
            {
                assertions.push((
                    path.clone(),
                    expression.trim().to_owned(),
                    SourceRange::new(path, offset, offset + line.len()),
                ));
            }
            offset += physical.len();
        }
    }
    assertions
}

fn map_evaluation_failure(
    outcome: &EvaluationOutcome,
    range: &SourceRange,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match outcome {
        EvaluationOutcome::Completed(_) => {}
        EvaluationOutcome::Panicked { kind, .. } => diagnostics.push(
            Diagnostic::new("evaluation.panicked", range.clone(), RecoveryAction::None)
                .with_parameter("kind", kind.clone()),
        ),
        EvaluationOutcome::LimitExceeded { policy, .. } => diagnostics.push(
            Diagnostic::new(
                "evaluation.limit_exceeded",
                range.clone(),
                RecoveryAction::None,
            )
            .with_parameter("policy", policy.clone()),
        ),
        EvaluationOutcome::CreatorRejected { kind } => diagnostics.push(
            Diagnostic::new("evaluation.rejected", range.clone(), RecoveryAction::None)
                .with_parameter("kind", kind.clone()),
        ),
        EvaluationOutcome::Cancelled | EvaluationOutcome::Defect { .. } => {}
    }
}

fn has_image_attribute(bytes: &[u8], declaration_start: u64) -> bool {
    let Some(before) = crate::syntax::checked_slice(bytes, 0, declaration_start) else {
        return false;
    };
    before
        .split(|byte| *byte == b'\n')
        .rev()
        .map(|line| String::from_utf8_lossy(line).trim().to_owned())
        .find(|line| !line.is_empty())
        .is_some_and(|line| line == "@image")
}

fn contains_build_constructor(source: &str) -> bool {
    source.contains("Image.new(") || source.contains("Test.new(")
}

fn validate_attributes(file: &ProjectFile, diagnostics: &mut Vec<Diagnostic>) {
    let mut offset = 0;
    for physical in file.bytes().split_inclusive(|byte| *byte == b'\n') {
        let line = physical.strip_suffix(b"\n").unwrap_or(physical);
        if let Ok(text) = std::str::from_utf8(line) {
            let attribute = text.trim();
            if attribute.starts_with('@') && !matches!(attribute, "@image" | "@actor") {
                diagnostics.push(
                    Diagnostic::new(
                        "semantic.unknown_attribute",
                        SourceRange::new(file.path(), offset, offset + line.len()),
                        RecoveryAction::None,
                    )
                    .with_parameter("attribute", attribute.to_owned()),
                );
            }
        }
        offset += physical.len();
    }
}

fn reachable_functions(
    root: &str,
    functions: &BTreeMap<String, Function>,
) -> BTreeSet<SourceRange> {
    let mut reachable = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut pending = BTreeSet::from([root.to_owned()]);
    while let Some(lookup_name) = pending.pop_first() {
        if !visited.insert(lookup_name.clone()) {
            continue;
        }
        if let Some(function) = functions.get(&lookup_name) {
            reachable.insert(function.source.clone());
            for called in called_functions(&function_body(function)) {
                let imported = format!("{}|{called}", function.module);
                let local = format!("{}.{}", function.module, called);
                if functions.contains_key(&imported) {
                    pending.insert(imported);
                } else if functions.contains_key(&local) {
                    pending.insert(local);
                }
            }
        }
    }
    reachable
}

fn called_functions(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut calls = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if matches!(bytes[index], b'A'..=b'Z' | b'a'..=b'z' | b'_') {
            let start = index;
            index += 1;
            while bytes
                .get(index)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.'))
            {
                index += 1;
            }
            let after_name = source[index..].trim_start();
            if after_name.starts_with('(') {
                calls.push(source[start..index].to_owned());
            }
        } else {
            index += 1;
        }
    }
    calls
}

fn unproven_recursive_cycles(functions: &BTreeMap<String, Function>) -> Vec<SourceRange> {
    let unique = functions
        .values()
        .map(|function| (function.source.clone(), function))
        .collect::<BTreeMap<_, _>>();
    let graph = unique
        .iter()
        .map(|(source, function)| {
            let dependencies = called_functions(&function_body(function))
                .into_iter()
                .filter_map(|called| {
                    let imported = format!("{}|{called}", function.module);
                    let local = format!("{}.{}", function.module, called);
                    functions
                        .get(&imported)
                        .or_else(|| functions.get(&local))
                        .map(|target| target.source.clone())
                })
                .collect::<BTreeSet<_>>();
            (source.clone(), dependencies)
        })
        .collect::<BTreeMap<_, _>>();
    graph
        .keys()
        .filter(|root| reaches(root, root, &graph, &mut BTreeSet::new()))
        .cloned()
        .collect()
}

fn reaches(
    root: &SourceRange,
    current: &SourceRange,
    graph: &BTreeMap<SourceRange, BTreeSet<SourceRange>>,
    visited: &mut BTreeSet<SourceRange>,
) -> bool {
    if !visited.insert(current.clone()) {
        return false;
    }
    graph.get(current).is_some_and(|dependencies| {
        dependencies.contains(root)
            || dependencies
                .iter()
                .any(|dependency| reaches(root, dependency, graph, visited))
    })
}

fn partial_result_return(header: &str) -> Option<&str> {
    let start = header.find("Result[")? + "Result[".len();
    let end = header[start..].find(']')? + start;
    (!header[start..end].contains(',')).then_some(&header[start..end])
}

fn contains_type_name(header: &str, type_name: &str) -> bool {
    header
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .any(|word| word == type_name)
}

fn split_commas(source: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (index, character) in source.char_indices() {
        match character {
            '[' | '(' => depth += 1,
            ']' | ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(source[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(source[start..].trim());
    parts
}

fn first_line(source: &str) -> &str {
    source.lines().next().unwrap_or(source)
}

fn qualify(path: &str, name: &str) -> String {
    format!("{}.{}", module_name(path), name)
}

fn module_name(path: &str) -> String {
    path.strip_prefix("src/")
        .and_then(|path| path.strip_suffix(".wr"))
        .unwrap_or(path)
        .replace('/', ".")
}

fn cancelled() -> Analysis {
    Analysis {
        diagnostics: Vec::new(),
        function_facts: Vec::new(),
        inferred_errors: Vec::new(),
        evaluations: Vec::new(),
        constructions: Vec::new(),
        test_plan: Vec::new(),
        defect: None,
        cancelled: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_partiality_distinguishes_one_and_two_arguments() {
        assert_eq!(
            partial_result_return("fn read() -> Result[i64]:"),
            Some("i64")
        );
        assert_eq!(
            partial_result_return("fn read() -> Result[i64, Error]:"),
            None
        );
    }

    #[test]
    fn construction_keys_are_domain_separated() {
        assert_ne!(
            xxhash_rust::xxh3::xxh3_128(b"wrela.construction.v1|Image.new|0"),
            xxhash_rust::xxh3::xxh3_128(b"wrela.identity.v1|Image.new|0")
        );
    }
}
