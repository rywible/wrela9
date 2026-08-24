#![forbid(unsafe_code)]

use std::sync::Arc;

use crate::syntax::NameSyntax;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ModuleId(pub(crate) u128);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct DefinitionId(pub(crate) u128);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct TypeId(pub(crate) u128);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PoolId(pub(crate) u128);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct TestId {
    pub(crate) suite: DefinitionId,
    pub(crate) test: DefinitionId,
    pub(crate) identity: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct VariantId {
    pub(crate) owner: DefinitionId,
    pub(crate) definition: DefinitionId,
    pub(crate) variant: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct TypeParameterId(pub(crate) u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct SpecializationId(pub(crate) u128);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum IntegerType {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FloatType {
    F16,
    F32,
    F64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BuiltinType {
    Image,
    Test,
    TestApplication,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BuiltinVariant {
    ResultOk,
    ResultErr,
    OptionSome,
    OptionNone,
}

impl BuiltinVariant {
    pub(crate) const fn canonical_tag(self) -> u8 {
        match self {
            Self::ResultOk => 0x01,
            Self::ResultErr => 0x02,
            Self::OptionSome => 0x11,
            Self::OptionNone => 0x12,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BuildKind {
    Image,
    Test,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Type {
    Unit,
    Bool,
    Integer(IntegerType),
    Float(FloatType),
    Text,
    Bytes,
    Array(Arc<Type>),
    Tuple(Arc<[Type]>),
    Result {
        success: Arc<Type>,
        error: Option<Arc<Type>>,
    },
    Option(Arc<Type>),
    Builtin(BuiltinType),
    Nominal {
        definition: DefinitionId,
        display: Arc<str>,
    },
    Parameter {
        owner: DefinitionId,
        id: TypeParameterId,
        display: Arc<str>,
    },
    Infer,
}

impl IntegerType {
    pub(crate) const fn canonical_tag(self) -> u8 {
        match self {
            Self::U8 => 0x01,
            Self::U16 => 0x02,
            Self::U32 => 0x03,
            Self::U64 => 0x04,
            Self::I8 => 0x11,
            Self::I16 => 0x12,
            Self::I32 => 0x13,
            Self::I64 => 0x14,
        }
    }
    pub(crate) const fn is_signed(self) -> bool {
        matches!(self, Self::I8 | Self::I16 | Self::I32 | Self::I64)
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
        }
    }

    pub(crate) fn fits(self, value: i128) -> bool {
        match self {
            Self::U8 => u8::try_from(value).is_ok(),
            Self::U16 => u16::try_from(value).is_ok(),
            Self::U32 => u32::try_from(value).is_ok(),
            Self::U64 => u64::try_from(value).is_ok(),
            Self::I8 => i8::try_from(value).is_ok(),
            Self::I16 => i16::try_from(value).is_ok(),
            Self::I32 => i32::try_from(value).is_ok(),
            Self::I64 => i64::try_from(value).is_ok(),
        }
    }
}

impl FloatType {
    pub(crate) const fn canonical_tag(self) -> u8 {
        match self {
            Self::F16 => 0x21,
            Self::F32 => 0x22,
            Self::F64 => 0x23,
        }
    }
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::F16 => "f16",
            Self::F32 => "f32",
            Self::F64 => "f64",
        }
    }
}

impl BuiltinType {
    pub(crate) const fn canonical_tag(self) -> u8 {
        match self {
            Self::Image => 0x01,
            Self::Test => 0x02,
            Self::TestApplication => 0x03,
        }
    }
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Image => "Image",
            Self::Test => "Test",
            Self::TestApplication => "TestApplication",
        }
    }
}

impl BuildKind {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Image => "Image",
            Self::Test => "Test",
        }
    }
}

impl Type {
    pub(crate) fn display(&self) -> String {
        match self {
            Self::Unit => "()".to_owned(),
            Self::Bool => "bool".to_owned(),
            Self::Integer(kind) => kind.name().to_owned(),
            Self::Float(kind) => kind.name().to_owned(),
            Self::Text => "Text".to_owned(),
            Self::Bytes => "Bytes".to_owned(),
            Self::Array(element) => format!("[{}]", element.display()),
            Self::Tuple(members) => format!(
                "({})",
                members
                    .iter()
                    .map(Type::display)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Result { success, error } => error.as_ref().map_or_else(
                || format!("Result[{}]", success.display()),
                |error| format!("Result[{}, {}]", success.display(), error.display()),
            ),
            Self::Option(value) => format!("Option[{}]", value.display()),
            Self::Builtin(kind) => kind.name().to_owned(),
            Self::Nominal { display, .. } | Self::Parameter { display, .. } => display.to_string(),
            Self::Infer => "_".to_owned(),
        }
    }

    pub(crate) const fn is_numeric(&self) -> bool {
        matches!(self, Self::Integer(_) | Self::Float(_))
    }

    pub(crate) fn canonical_key(&self) -> Arc<[u8]> {
        let mut bytes = Vec::new();
        self.append_canonical_key(&mut bytes);
        bytes.into()
    }

    fn append_canonical_key(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::Unit => bytes.push(0),
            Self::Bool => bytes.push(1),
            Self::Integer(kind) => {
                bytes.push(2);
                bytes.push(kind.canonical_tag());
            }
            Self::Float(kind) => {
                bytes.push(3);
                bytes.push(kind.canonical_tag());
            }
            Self::Text => bytes.push(4),
            Self::Bytes => bytes.push(5),
            Self::Array(element) => {
                bytes.push(6);
                element.append_canonical_key(bytes);
            }
            Self::Tuple(members) => {
                bytes.push(7);
                bytes.extend_from_slice(
                    &u64::try_from(members.len())
                        .unwrap_or(u64::MAX)
                        .to_be_bytes(),
                );
                for member in &**members {
                    member.append_canonical_key(bytes);
                }
            }
            Self::Result { success, error } => {
                bytes.push(8);
                success.append_canonical_key(bytes);
                if let Some(error) = error {
                    bytes.push(1);
                    error.append_canonical_key(bytes);
                } else {
                    bytes.push(0);
                }
            }
            Self::Option(value) => {
                bytes.push(9);
                value.append_canonical_key(bytes);
            }
            Self::Builtin(kind) => {
                bytes.push(10);
                bytes.push(kind.canonical_tag());
            }
            Self::Nominal { definition, .. } => {
                bytes.push(11);
                bytes.extend_from_slice(&definition.0.to_be_bytes());
            }
            Self::Parameter { owner, id, .. } => {
                bytes.push(12);
                bytes.extend_from_slice(&owner.0.to_be_bytes());
                bytes.extend_from_slice(&id.0.to_be_bytes());
            }
            Self::Infer => bytes.push(13),
        }
    }
}

pub(crate) fn resolve_builtin_type(name: &NameSyntax) -> Option<Type> {
    let [name] = name.segments.as_slice() else {
        return None;
    };
    Some(match name.as_str() {
        "bool" => Type::Bool,
        "u8" => Type::Integer(IntegerType::U8),
        "u16" => Type::Integer(IntegerType::U16),
        "u32" => Type::Integer(IntegerType::U32),
        "u64" => Type::Integer(IntegerType::U64),
        "i8" => Type::Integer(IntegerType::I8),
        "i16" => Type::Integer(IntegerType::I16),
        "i32" => Type::Integer(IntegerType::I32),
        "i64" => Type::Integer(IntegerType::I64),
        "f16" => Type::Float(FloatType::F16),
        "f32" => Type::Float(FloatType::F32),
        "f64" => Type::Float(FloatType::F64),
        "Text" => Type::Text,
        "Bytes" => Type::Bytes,
        "Image" => Type::Builtin(BuiltinType::Image),
        "Test" => Type::Builtin(BuiltinType::Test),
        "TestApplication" => Type::Builtin(BuiltinType::TestApplication),
        _ => return None,
    })
}

pub(crate) fn resolve_builtin_variant(name: &NameSyntax) -> Option<BuiltinVariant> {
    match name
        .segments
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["Result", "Ok"] => Some(BuiltinVariant::ResultOk),
        ["Result", "Err"] => Some(BuiltinVariant::ResultErr),
        ["Option", "Some"] => Some(BuiltinVariant::OptionSome),
        ["Option", "None"] => Some(BuiltinVariant::OptionNone),
        _ => None,
    }
}
