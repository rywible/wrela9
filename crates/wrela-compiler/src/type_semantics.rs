#![forbid(unsafe_code)]

use crate::model::{DefinitionId, Type};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LabelMode {
    Optional,
    Required,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CallableParameter<'a> {
    pub(crate) name: &'a str,
    pub(crate) type_: &'a Type,
}

#[derive(Clone, Debug)]
pub(crate) struct CallableSignature<'a> {
    pub(crate) parameters: Vec<CallableParameter<'a>>,
    pub(crate) label_mode: LabelMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CheckedCall {
    /// For each source-order argument, the declaration-order parameter it binds.
    pub(crate) source_to_parameter: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CallError {
    Count,
    Label,
    Type,
}

impl<'a> CallableSignature<'a> {
    pub(crate) fn check(
        &self,
        arguments: &[Type],
        labels: &[Option<String>],
    ) -> Result<CheckedCall, CallError> {
        if arguments.len() != self.parameters.len() || labels.len() != arguments.len() {
            return Err(CallError::Count);
        }
        let mut bound = vec![false; self.parameters.len()];
        let mut source_to_parameter = Vec::with_capacity(arguments.len());
        for (source_index, (argument, label)) in arguments.iter().zip(labels).enumerate() {
            let parameter_index = match label {
                Some(label) => self
                    .parameters
                    .iter()
                    .position(|parameter| parameter.name == label)
                    .ok_or(CallError::Label)?,
                None if self.label_mode == LabelMode::Required => {
                    return Err(CallError::Label);
                }
                None => source_index,
            };
            if parameter_index >= self.parameters.len() || bound[parameter_index] {
                return Err(CallError::Label);
            }
            if !can_pass(argument, self.parameters[parameter_index].type_) {
                return Err(CallError::Type);
            }
            bound[parameter_index] = true;
            source_to_parameter.push(parameter_index);
        }
        if bound.iter().any(|bound| !bound) {
            return Err(CallError::Count);
        }
        Ok(CheckedCall {
            source_to_parameter,
        })
    }
}

/// Whether two type terms can describe the same type while resolving compiler-owned
/// inference variables. This never performs a numeric conversion or Result lifting.
pub(crate) fn can_unify(actual: &Type, expected: &Type) -> bool {
    if actual == expected
        || matches!(actual, Type::Infer | Type::Parameter { .. })
        || matches!(expected, Type::Infer | Type::Parameter { .. })
    {
        return true;
    }
    match (actual, expected) {
        (
            Type::Result {
                success: actual_success,
                error: actual_error,
            },
            Type::Result {
                success: expected_success,
                error: expected_error,
            },
        ) => {
            can_unify(actual_success, expected_success)
                && match (actual_error, expected_error) {
                    (_, None) => true,
                    (Some(actual), Some(expected)) => can_unify(actual, expected),
                    (None, Some(_)) => false,
                }
        }
        (Type::Array(actual), Type::Array(expected))
        | (Type::Option(actual), Type::Option(expected)) => can_unify(actual, expected),
        (
            Type::FixedArray {
                element: actual, ..
            },
            Type::Array(expected),
        )
        | (
            Type::Array(actual),
            Type::FixedArray {
                element: expected, ..
            },
        ) => can_unify(actual, expected),
        (
            Type::FixedArray {
                element: actual,
                length: actual_length,
            },
            Type::FixedArray {
                element: expected,
                length: expected_length,
            },
        ) => actual_length == expected_length && can_unify(actual, expected),
        (
            Type::Function {
                parameters: actual_parameters,
                return_type: actual_return,
            },
            Type::Function {
                parameters: expected_parameters,
                return_type: expected_return,
            },
        ) => {
            actual_parameters.len() == expected_parameters.len()
                && actual_parameters
                    .iter()
                    .zip(expected_parameters.iter())
                    .all(|(actual, expected)| can_unify(actual, expected))
                && can_unify(actual_return, expected_return)
        }
        (
            Type::Own {
                pool: actual_pool,
                value: actual,
            },
            Type::Own {
                pool: expected_pool,
                value: expected,
            },
        ) => actual_pool == expected_pool && can_unify(actual, expected),
        (
            Type::Any {
                interface: actual, ..
            },
            Type::Any {
                interface: expected,
                ..
            },
        ) => actual == expected,
        (
            Type::Nominal {
                definition: actual_definition,
                arguments: actual_arguments,
                ..
            },
            Type::Nominal {
                definition: expected_definition,
                arguments: expected_arguments,
                ..
            },
        ) => {
            actual_definition == expected_definition
                && actual_arguments.len() == expected_arguments.len()
                && actual_arguments
                    .iter()
                    .zip(expected_arguments.iter())
                    .all(|(actual, expected)| can_unify(actual, expected))
        }
        (Type::Tuple(actual), Type::Tuple(expected)) => {
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected.iter())
                    .all(|(actual, expected)| can_unify(actual, expected))
        }
        _ => false,
    }
}

/// Whether a value may initialize a place with the expected type.
pub(crate) fn can_initialize(actual: &Type, expected: &Type) -> bool {
    can_unify(actual, expected)
}

/// Whether an argument may bind a callable parameter without an explicit conversion.
pub(crate) fn can_pass(actual: &Type, expected: &Type) -> bool {
    can_unify(actual, expected)
}

/// Whether an expression may complete a function return. A success value produced by
/// postfix propagation is lifted into the enclosing exact Result; its error alternative
/// is handled as control flow by the evaluator.
pub(crate) fn can_return(actual: &Type, expected: &Type) -> bool {
    can_unify(actual, expected)
        || matches!(expected, Type::Result { success, .. } if can_unify(actual, success))
}

/// Whether a value of this type transitively owns a Resource. Nominal
/// classification remains catalog-owned; structural recursion has one meaning.
pub(crate) fn contains_resource(
    type_: &Type,
    nominal_is_resource: &impl Fn(DefinitionId) -> bool,
) -> bool {
    match type_ {
        Type::Own { .. } => true,
        Type::Nominal {
            definition,
            arguments,
            ..
        } => {
            nominal_is_resource(*definition)
                || arguments
                    .iter()
                    .any(|argument| contains_resource(argument, nominal_is_resource))
        }
        Type::Array(value) | Type::FixedArray { element: value, .. } | Type::Option(value) => {
            contains_resource(value, nominal_is_resource)
        }
        Type::Tuple(values) => values
            .iter()
            .any(|value| contains_resource(value, nominal_is_resource)),
        Type::Result { success, error } => {
            contains_resource(success, nominal_is_resource)
                || error
                    .as_ref()
                    .is_some_and(|error| contains_resource(error, nominal_is_resource))
        }
        Type::Function { .. }
        | Type::Any { .. }
        | Type::Unit
        | Type::Bool
        | Type::Integer(_)
        | Type::Float(_)
        | Type::Text
        | Type::Scalar
        | Type::Bytes
        | Type::Builtin(_)
        | Type::Parameter { .. }
        | Type::Infer => false,
    }
}
