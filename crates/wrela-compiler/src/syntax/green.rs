use super::*;

#[derive(Clone, Debug)]
pub(crate) struct ComptimeSelection {
    pub(crate) branches: Vec<ComptimeBranch>,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct ComptimeBranch {
    pub(crate) condition: Option<ExpressionSyntax>,
    pub(crate) declarations: Vec<Declaration>,
    pub(crate) range: SourceRange,
}

#[derive(Clone, Debug)]
pub(super) struct GreenNode {
    pub(super) kind: SyntaxNodeKind,
    pub(super) range: SourceRange,
    pub(super) children: std::sync::Arc<[GreenChild]>,
}

#[derive(Clone, Debug)]
pub(super) enum GreenChild {
    Node(GreenNode),
    Leaf(SyntaxElement),
}

enum Event {
    Start(SyntaxNodeKind, SourceRange),
    Token(usize),
    Missing(usize),
    Error(usize),
    Finish,
}

struct SyntaxRegion {
    kind: SyntaxNodeKind,
    range: SourceRange,
    children: Vec<SyntaxRegion>,
}

impl GreenNode {
    pub(super) fn project(&self, depth: u16, output: &mut Vec<SyntaxNodeObservation>) {
        output.push(SyntaxNodeObservation::new(
            self.kind,
            self.range.clone(),
            depth,
        ));
        for child in &*self.children {
            if let GreenChild::Node(node) = child {
                node.project(depth.saturating_add(1), output);
            }
        }
    }

    pub(super) fn authored_bytes(&self) -> u64 {
        self.children
            .iter()
            .map(|child| match child {
                GreenChild::Node(node) => node.authored_bytes(),
                GreenChild::Leaf(leaf)
                    if matches!(
                        leaf.kind(),
                        SyntaxElementKind::Token(_)
                            | SyntaxElementKind::Trivia(_)
                            | SyntaxElementKind::Invalid(_)
                    ) =>
                {
                    leaf.range().end().saturating_sub(leaf.range().start())
                }
                GreenChild::Leaf(_) => 0,
            })
            .sum()
    }
}

pub(super) fn build_green_tree(
    file: &ProjectFile,
    declarations: &[Declaration],
    elements: &[SyntaxElement],
    cancellation: &Cancellation,
) -> Option<GreenNode> {
    let mut events = vec![Event::Start(
        SyntaxNodeKind::Source,
        SourceRange::new_shared(file.path_arc(), 0, file.bytes().len()),
    )];
    let mut element_index = 0;
    for declaration in declarations {
        if cancellation.is_cancelled()
            || emit_elements_before(
                declaration.start,
                elements,
                &mut element_index,
                &mut events,
                cancellation,
            )
            || emit_region(
                &declaration_region(file, declaration),
                elements,
                &mut element_index,
                &mut events,
                cancellation,
            )
        {
            return None;
        }
    }
    while element_index < elements.len() {
        if element_index.is_multiple_of(256) && cancellation.is_cancelled() {
            return None;
        }
        emit_element(elements, element_index, &mut events);
        element_index += 1;
    }
    events.push(Event::Finish);

    let mut stack: Vec<(SyntaxNodeKind, SourceRange, Vec<GreenChild>)> = Vec::new();
    let mut root = None;
    for (event_index, event) in events.into_iter().enumerate() {
        if event_index.is_multiple_of(256) && cancellation.is_cancelled() {
            return None;
        }
        match event {
            Event::Start(kind, range) => stack.push((kind, range, Vec::new())),
            Event::Token(index) | Event::Missing(index) | Event::Error(index) => stack
                .last_mut()
                .expect("event parser has an open node")
                .2
                .push(GreenChild::Leaf(elements[index].clone())),
            Event::Finish => {
                let (kind, range, children) = stack.pop().expect("balanced parser events");
                let node = GreenNode {
                    kind,
                    range,
                    children: children.into(),
                };
                if let Some(parent) = stack.last_mut() {
                    parent.2.push(GreenChild::Node(node));
                } else {
                    root = Some(node);
                }
            }
        }
    }
    Some(root.expect("source event produces a root"))
}

pub(super) fn merge_syntax_elements(
    physical: Vec<SyntaxElement>,
    layout: Vec<SyntaxElement>,
) -> Vec<SyntaxElement> {
    let mut physical = physical.into_iter().peekable();
    let mut layout = layout.into_iter().peekable();
    let mut merged = Vec::with_capacity(physical.len() + layout.len());
    loop {
        let take_layout = match (physical.peek(), layout.peek()) {
            (Some(physical), Some(layout)) => {
                (layout.range().start(), layout.range().end())
                    < (physical.range().start(), physical.range().end())
            }
            (None, Some(_)) => true,
            (Some(_), None) => false,
            (None, None) => break,
        };
        if take_layout {
            merged.push(layout.next().expect("peeked layout element"));
        } else {
            merged.push(physical.next().expect("peeked physical element"));
        }
    }
    merged
}

fn declaration_region(file: &ProjectFile, declaration: &Declaration) -> SyntaxRegion {
    let mut children = Vec::new();
    if let Some(syntax) = &declaration.syntax {
        match syntax {
            DeclarationSyntax::Function(function) => {
                children.push(SyntaxRegion {
                    kind: SyntaxNodeKind::FunctionSignature,
                    range: declaration.range.clone(),
                    children: function
                        .parameters
                        .iter()
                        .map(|parameter| SyntaxRegion {
                            kind: SyntaxNodeKind::Parameter,
                            range: parameter.range.clone(),
                            children: Vec::new(),
                        })
                        .collect(),
                });
                let mut statements = Vec::new();
                collect_statement_regions(&function.body, &mut statements);
                children.push(SyntaxRegion {
                    kind: SyntaxNodeKind::Block,
                    range: SourceRange::from_u64_shared(
                        file.path_arc(),
                        declaration.range.end(),
                        declaration.end,
                    ),
                    children: statements,
                });
            }
            DeclarationSyntax::Constant(constant) => children.push(SyntaxRegion {
                kind: SyntaxNodeKind::ConstantValue,
                range: declaration.range.clone(),
                children: vec![expression_region(&constant.value)],
            }),
            DeclarationSyntax::TypeAlias(_) => {}
            DeclarationSyntax::Suite(suite) => {
                children.push(SyntaxRegion {
                    kind: SyntaxNodeKind::SuiteHeader,
                    range: declaration.range.clone(),
                    children: Vec::new(),
                });
                children.extend(suite.tests.iter().map(|test| {
                    let mut test_children = test
                        .parameters
                        .iter()
                        .map(|parameter| SyntaxRegion {
                            kind: SyntaxNodeKind::Parameter,
                            range: parameter.range.clone(),
                            children: Vec::new(),
                        })
                        .collect::<Vec<_>>();
                    collect_statement_regions(&test.body, &mut test_children);
                    SyntaxRegion {
                        kind: if test.asynchronous {
                            SyntaxNodeKind::AsyncTest
                        } else {
                            SyntaxNodeKind::Test
                        },
                        range: test.range.clone(),
                        children: test_children,
                    }
                }));
            }
            DeclarationSyntax::Enum(enum_) => {
                let variants = enum_
                    .variants
                    .iter()
                    .map(|variant| SyntaxRegion {
                        kind: SyntaxNodeKind::Variant,
                        range: variant.range.clone(),
                        children: variant
                            .parameters
                            .iter()
                            .map(|parameter| SyntaxRegion {
                                kind: SyntaxNodeKind::Parameter,
                                range: parameter.range.clone(),
                                children: Vec::new(),
                            })
                            .collect(),
                    })
                    .collect();
                let functions = enum_.functions.iter().map(member_function_region).collect();
                let mut regions = merge_regions(variants, functions);
                regions.extend(
                    enum_
                        .comptime_selections
                        .iter()
                        .map(member_selection_region),
                );
                regions.sort_by_key(|region| region.range.start());
                children.extend(regions);
            }
            DeclarationSyntax::Struct(struct_) | DeclarationSyntax::ResourceStruct(struct_) => {
                let fields = struct_
                    .fields
                    .iter()
                    .map(|field| SyntaxRegion {
                        kind: SyntaxNodeKind::Field,
                        range: field.range.clone(),
                        children: Vec::new(),
                    })
                    .collect();
                let functions = struct_
                    .functions
                    .iter()
                    .map(member_function_region)
                    .collect();
                let mut regions = merge_regions(fields, functions);
                regions.extend(
                    struct_
                        .comptime_selections
                        .iter()
                        .map(member_selection_region),
                );
                regions.sort_by_key(|region| region.range.start());
                children.extend(regions);
            }
            DeclarationSyntax::Interface(interface) => {
                children.extend(interface.requirements.iter().map(|requirement| {
                    SyntaxRegion {
                        kind: SyntaxNodeKind::FunctionRequirement,
                        range: requirement.range.clone(),
                        children: requirement
                            .parameters
                            .iter()
                            .map(|parameter| SyntaxRegion {
                                kind: SyntaxNodeKind::Parameter,
                                range: parameter.range.clone(),
                                children: Vec::new(),
                            })
                            .collect(),
                    }
                }));
            }
            DeclarationSyntax::Pool => {}
        }
    }
    SyntaxRegion {
        kind: declaration.kind.node_kind(),
        range: SourceRange::from_u64_shared(file.path_arc(), declaration.start, declaration.end),
        children,
    }
}

fn member_selection_region(selection: &ComptimeMemberSelection) -> SyntaxRegion {
    SyntaxRegion {
        kind: SyntaxNodeKind::ComptimeSelection,
        range: selection.range.clone(),
        children: selection
            .branches
            .iter()
            .map(|branch| {
                let mut children = branch
                    .condition
                    .iter()
                    .map(expression_region)
                    .collect::<Vec<_>>();
                children.extend(branch.fields.iter().map(|field| SyntaxRegion {
                    kind: SyntaxNodeKind::Field,
                    range: field.range.clone(),
                    children: Vec::new(),
                }));
                children.extend(branch.variants.iter().map(|variant| {
                    SyntaxRegion {
                        kind: SyntaxNodeKind::Variant,
                        range: variant.range.clone(),
                        children: variant
                            .parameters
                            .iter()
                            .map(|parameter| SyntaxRegion {
                                kind: SyntaxNodeKind::Parameter,
                                range: parameter.range.clone(),
                                children: Vec::new(),
                            })
                            .collect(),
                    }
                }));
                children.extend(branch.functions.iter().map(member_function_region));
                children.sort_by_key(|region| region.range.start());
                SyntaxRegion {
                    kind: SyntaxNodeKind::ComptimeBranch,
                    range: branch.range.clone(),
                    children,
                }
            })
            .collect(),
    }
}

fn collect_statement_regions(statements: &[StatementSyntax], output: &mut Vec<SyntaxRegion>) {
    let mut pending = statements.iter().rev().collect::<Vec<_>>();
    while let Some(statement) = pending.pop() {
        if let StatementSyntax::Comptime { branches, range } = statement {
            output.push(SyntaxRegion {
                kind: SyntaxNodeKind::ComptimeSelection,
                range: range.clone(),
                children: branches
                    .iter()
                    .map(|branch| {
                        let mut children = branch
                            .condition
                            .iter()
                            .map(expression_region)
                            .collect::<Vec<_>>();
                        collect_statement_regions(&branch.statements, &mut children);
                        SyntaxRegion {
                            kind: SyntaxNodeKind::ComptimeBranch,
                            range: branch.range.clone(),
                            children,
                        }
                    })
                    .collect(),
            });
            continue;
        }
        let (kind, range, expressions) = match statement {
            StatementSyntax::Return { value, range } => (
                SyntaxNodeKind::ReturnStatement,
                range,
                value.iter().map(expression_region).collect(),
            ),
            StatementSyntax::Panic { value, range } => (
                SyntaxNodeKind::PanicStatement,
                range,
                vec![expression_region(value)],
            ),
            StatementSyntax::Assert { condition, range } => (
                SyntaxNodeKind::AssertStatement,
                range,
                vec![expression_region(condition)],
            ),
            StatementSyntax::Expect { condition, range } => (
                SyntaxNodeKind::ExpectStatement,
                range,
                vec![expression_region(condition)],
            ),
            StatementSyntax::Assign { value, range, .. } => (
                SyntaxNodeKind::InitializeStatement,
                range,
                vec![expression_region(value)],
            ),
            StatementSyntax::Evaluate(value) => (
                SyntaxNodeKind::ExpressionStatement,
                &value.range,
                vec![expression_region(value)],
            ),
            StatementSyntax::If {
                condition, range, ..
            } => (
                SyntaxNodeKind::IfStatement,
                range,
                vec![expression_region(condition)],
            ),
            StatementSyntax::Comptime { .. } => unreachable!("handled before generic statement"),
            StatementSyntax::For {
                pattern,
                iterable,
                range,
                ..
            } => (
                SyntaxNodeKind::ForStatement,
                range,
                vec![pattern_region(pattern), expression_region(iterable)],
            ),
            StatementSyntax::While {
                condition, range, ..
            } => (
                SyntaxNodeKind::WhileStatement,
                range,
                vec![expression_region(condition)],
            ),
            StatementSyntax::Break(range) => (SyntaxNodeKind::BreakStatement, range, Vec::new()),
            StatementSyntax::Continue(range) => {
                (SyntaxNodeKind::ContinueStatement, range, Vec::new())
            }
            StatementSyntax::Match { value, range, .. } => (
                SyntaxNodeKind::MatchStatement,
                range,
                vec![expression_region(value)],
            ),
            StatementSyntax::Defer { expression, range } => (
                SyntaxNodeKind::DeferStatement,
                range,
                vec![expression_region(expression)],
            ),
            StatementSyntax::With { scope, range, .. } => (
                SyntaxNodeKind::WithStatement,
                range,
                vec![expression_region(scope)],
            ),
            StatementSyntax::Unsupported { kind, range } => {
                (unsupported_statement_node(*kind), range, Vec::new())
            }
            StatementSyntax::Pass(range) => (SyntaxNodeKind::PassStatement, range, Vec::new()),
        };
        output.push(SyntaxRegion {
            kind,
            range: range.clone(),
            children: expressions,
        });
        match statement {
            StatementSyntax::If {
                then_branch,
                else_branch,
                ..
            } => {
                pending.extend(else_branch.iter().rev());
                pending.extend(then_branch.iter().rev());
            }
            StatementSyntax::Comptime { .. } => unreachable!("handled before generic statement"),
            StatementSyntax::For { body, .. } => pending.extend(body.iter().rev()),
            StatementSyntax::While { body, .. } => pending.extend(body.iter().rev()),
            StatementSyntax::With { body, .. } => pending.extend(body.iter().rev()),
            StatementSyntax::Match { cases, .. } => {
                for case in cases {
                    output.push(SyntaxRegion {
                        kind: SyntaxNodeKind::MatchCase,
                        range: case.range.clone(),
                        children: std::iter::once(pattern_region(&case.pattern))
                            .chain(case.guard.iter().map(expression_region))
                            .collect(),
                    });
                    collect_statement_regions(&case.body, output);
                }
            }
            _ => {}
        }
    }
}

fn pattern_region(pattern: &PatternSyntax) -> SyntaxRegion {
    let (kind, children) = match &pattern.kind {
        PatternSyntaxKind::Wildcard | PatternSyntaxKind::Binding(_) => {
            (SyntaxNodeKind::NameExpression, Vec::new())
        }
        PatternSyntaxKind::Literal(expression) => return expression_region(expression),
        PatternSyntaxKind::Take(pattern) => (
            SyntaxNodeKind::TakeExpression,
            vec![pattern_region(pattern)],
        ),
        PatternSyntaxKind::Constructor { arguments, .. } => (
            SyntaxNodeKind::CallExpression,
            arguments
                .iter()
                .map(|argument| pattern_region(&argument.pattern))
                .collect(),
        ),
        PatternSyntaxKind::Tuple(patterns) => (
            SyntaxNodeKind::TupleExpression,
            patterns.iter().map(pattern_region).collect(),
        ),
        PatternSyntaxKind::FixedArray(patterns) => (
            SyntaxNodeKind::ArrayExpression,
            patterns.iter().map(pattern_region).collect(),
        ),
        PatternSyntaxKind::Or(patterns) => (
            SyntaxNodeKind::BinaryExpression,
            patterns.iter().map(pattern_region).collect(),
        ),
    };
    SyntaxRegion {
        kind,
        range: pattern.range.clone(),
        children,
    }
}

fn member_function_region(function: &MemberFunctionSyntax) -> SyntaxRegion {
    let mut children = function
        .function
        .parameters
        .iter()
        .map(|parameter| SyntaxRegion {
            kind: SyntaxNodeKind::Parameter,
            range: parameter.range.clone(),
            children: Vec::new(),
        })
        .collect::<Vec<_>>();
    collect_statement_regions(&function.function.body, &mut children);
    SyntaxRegion {
        kind: SyntaxNodeKind::MemberFunction,
        range: function.range.clone(),
        children,
    }
}

fn merge_regions(left: Vec<SyntaxRegion>, right: Vec<SyntaxRegion>) -> Vec<SyntaxRegion> {
    let mut left = left.into_iter().peekable();
    let mut right = right.into_iter().peekable();
    let mut merged = Vec::with_capacity(left.len() + right.len());
    loop {
        let take_right = match (left.peek(), right.peek()) {
            (Some(left), Some(right)) => right.range.start() < left.range.start(),
            (None, Some(_)) => true,
            (Some(_), None) => false,
            (None, None) => break,
        };
        if take_right {
            merged.push(right.next().expect("peeked right region"));
        } else {
            merged.push(left.next().expect("peeked left region"));
        }
    }
    merged
}

fn expression_region(expression: &ExpressionSyntax) -> SyntaxRegion {
    let (kind, children) = match &expression.kind {
        ExpressionSyntaxKind::Integer(_) => (SyntaxNodeKind::IntegerExpression, Vec::new()),
        ExpressionSyntaxKind::Float(_) => (SyntaxNodeKind::FloatExpression, Vec::new()),
        ExpressionSyntaxKind::Text(_) => (SyntaxNodeKind::TextExpression, Vec::new()),
        ExpressionSyntaxKind::Scalar(_) => (SyntaxNodeKind::ScalarExpression, Vec::new()),
        ExpressionSyntaxKind::Bytes(_) => (SyntaxNodeKind::BytesExpression, Vec::new()),
        ExpressionSyntaxKind::Bool(_) => (SyntaxNodeKind::BoolExpression, Vec::new()),
        ExpressionSyntaxKind::Name(_) => (SyntaxNodeKind::NameExpression, Vec::new()),
        ExpressionSyntaxKind::Call { arguments, .. } => (
            SyntaxNodeKind::CallExpression,
            arguments
                .iter()
                .map(|argument| expression_region(&argument.value))
                .collect(),
        ),
        ExpressionSyntaxKind::Array(values) => (
            SyntaxNodeKind::ArrayExpression,
            values.iter().map(expression_region).collect(),
        ),
        ExpressionSyntaxKind::RepeatedArray { value, .. } => (
            SyntaxNodeKind::RepeatedArrayExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::Tuple(values) => (
            SyntaxNodeKind::TupleExpression,
            values.iter().map(expression_region).collect(),
        ),
        ExpressionSyntaxKind::Index { value, index } => (
            SyntaxNodeKind::IndexExpression,
            vec![expression_region(value), expression_region(index)],
        ),
        ExpressionSyntaxKind::Unit => (SyntaxNodeKind::UnitExpression, Vec::new()),
        ExpressionSyntaxKind::Positive(value) => (
            SyntaxNodeKind::PositiveExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::Negate(value) => (
            SyntaxNodeKind::NegateExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::BitNot(value) => (
            SyntaxNodeKind::BitNotExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::Not(value) => (
            SyntaxNodeKind::NotExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::Await(value) => (
            SyntaxNodeKind::AwaitExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::Mut(value) => (
            SyntaxNodeKind::MutExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::Take(value) => (
            SyntaxNodeKind::TakeExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::Closure { body, .. } => (
            SyntaxNodeKind::ClosureExpression,
            vec![expression_region(body)],
        ),
        ExpressionSyntaxKind::Propagate(value) => (
            SyntaxNodeKind::PropagateExpression,
            vec![expression_region(value)],
        ),
        ExpressionSyntaxKind::Is { value, pattern } => (
            SyntaxNodeKind::IsExpression,
            vec![expression_region(value), pattern_region(pattern)],
        ),
        ExpressionSyntaxKind::Binary {
            operator,
            left,
            right,
        } => (
            if matches!(
                operator,
                BinaryOperatorSyntax::Range | BinaryOperatorSyntax::RangeInclusive
            ) {
                SyntaxNodeKind::RangeExpression
            } else {
                SyntaxNodeKind::BinaryExpression
            },
            vec![expression_region(left), expression_region(right)],
        ),
        ExpressionSyntaxKind::Unsupported(kind) => (
            match kind {
                UnsupportedExpressionKind::Send => SyntaxNodeKind::SendExpression,
                UnsupportedExpressionKind::TrySend => SyntaxNodeKind::TrySendExpression,
            },
            Vec::new(),
        ),
    };
    SyntaxRegion {
        kind,
        range: expression.range.clone(),
        children,
    }
}

fn emit_region(
    region: &SyntaxRegion,
    elements: &[SyntaxElement],
    element_index: &mut usize,
    events: &mut Vec<Event>,
    cancellation: &Cancellation,
) -> bool {
    let mut stack = vec![(region, 0_usize, false)];
    while let Some((current, child_index, started)) = stack.last_mut() {
        if cancellation.is_cancelled() {
            return true;
        }
        if !*started {
            events.push(Event::Start(current.kind, current.range.clone()));
            *started = true;
        }
        if let Some(child) = current.children.get(*child_index) {
            *child_index += 1;
            if emit_elements_before(
                child.range.start(),
                elements,
                element_index,
                events,
                cancellation,
            ) {
                return true;
            }
            stack.push((child, 0, false));
            continue;
        }
        if emit_elements_before(
            current.range.end(),
            elements,
            element_index,
            events,
            cancellation,
        ) {
            return true;
        }
        events.push(Event::Finish);
        stack.pop();
    }
    false
}

fn emit_elements_before(
    end: u64,
    elements: &[SyntaxElement],
    element_index: &mut usize,
    events: &mut Vec<Event>,
    cancellation: &Cancellation,
) -> bool {
    while elements
        .get(*element_index)
        .is_some_and(|element| element.range().start() < end)
    {
        if element_index.is_multiple_of(256) && cancellation.is_cancelled() {
            return true;
        }
        emit_element(elements, *element_index, events);
        *element_index += 1;
    }
    false
}

fn emit_element(elements: &[SyntaxElement], index: usize, events: &mut Vec<Event>) {
    events.push(match elements[index].kind() {
        SyntaxElementKind::Missing(_) => Event::Missing(index),
        SyntaxElementKind::Error(_) | SyntaxElementKind::Invalid(_) => Event::Error(index),
        SyntaxElementKind::Token(_)
        | SyntaxElementKind::Trivia(_)
        | SyntaxElementKind::Layout(_) => Event::Token(index),
    });
}

pub(super) fn validate_top_level(
    file: &ProjectFile,
    lexemes: &[Lexeme],
    diagnostics: &mut Vec<Diagnostic>,
    cancellation: &Cancellation,
) -> bool {
    let mut offset = 0;
    let mut token_index = 0;
    let mut delimiter_depth = 0_u32;
    while offset < file.bytes().len() {
        if cancellation.is_cancelled() {
            return true;
        }
        let Some(physical_end) =
            physical_line_end(file.bytes(), offset, file.bytes().len(), cancellation)
        else {
            return true;
        };
        let content_end = physical_content_end(file.bytes(), offset, physical_end);
        let line = &file.bytes()[offset..content_end];
        let end = offset + line.len();
        while lexemes.get(token_index).is_some_and(|lexeme| {
            usize::try_from(lexeme.range.start()).is_ok_and(|start| start < offset)
        }) {
            match lexemes[token_index].kind {
                TokenKind::LeftParen | TokenKind::LeftBracket => {
                    delimiter_depth = delimiter_depth.saturating_add(1);
                }
                TokenKind::RightParen | TokenKind::RightBracket => {
                    delimiter_depth = delimiter_depth.saturating_sub(1);
                }
                _ => {}
            }
            token_index += 1;
        }
        let first = lexemes
            .get(token_index)
            .filter(|lexeme| usize::try_from(lexeme.range.start()).is_ok_and(|start| start < end));
        let accepted = first.is_none()
            || delimiter_depth > 0
            || line.first().is_some_and(u8::is_ascii_whitespace)
            || first.is_some_and(|lexeme| {
                matches!(
                    lexeme.kind,
                    TokenKind::At
                        | TokenKind::From
                        | TokenKind::Comptime
                        | TokenKind::Elif
                        | TokenKind::Else
                        | TokenKind::Pub
                        | TokenKind::Pure
                        | TokenKind::Async
                        | TokenKind::Fn
                        | TokenKind::Const
                        | TokenKind::Pool
                        | TokenKind::Type
                        | TokenKind::Struct
                        | TokenKind::Resource
                        | TokenKind::Enum
                        | TokenKind::Interface
                        | TokenKind::Suite
                )
            });
        if !accepted {
            diagnostics.push(Diagnostic::new(
                "syntax.unexpected_top_level",
                SourceRange::new_shared(file.path_arc(), offset, offset + line.len()),
                RecoveryAction::SkippedToBoundary,
            ));
        }
        offset = physical_end;
    }
    false
}
