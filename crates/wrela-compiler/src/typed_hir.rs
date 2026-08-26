#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use xxhash_rust::xxh3::{Xxh3, xxh3_128};

use crate::control_flow::{syntax_statements_fall_through, verified_statements_fall_through};
use crate::identity::IdentityCatalog;
use crate::model::{
    ArrayLength, BuildKind, BuiltinType, BuiltinVariant, DefinitionId, FloatType, IntegerType,
    ModuleId, PoolId, PoolTerm, SpecializationId, TestId, Type, TypeId, TypeParameterId, VariantId,
    resolve_builtin_type, resolve_builtin_variant,
};
use crate::syntax::{
    BinaryOperatorSyntax, ExpressionSyntax, ExpressionSyntaxKind, NameSyntax, OwnershipSyntax,
    PatternSyntaxKind, PlaceProjectionSyntax, PlaceSyntax, StatementSyntax,
};
use crate::type_semantics::{
    CallError, CallableParameter, CallableSignature, LabelMode, can_initialize, can_pass,
    can_return, can_unify,
};
use crate::{Cancellation, SourceRange};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LocalId(pub(crate) u32);

#[derive(Clone, Debug)]
pub(crate) struct Place {
    pub(crate) local: LocalId,
    pub(crate) projections: Arc<[PlaceProjection]>,
}

impl Place {
    fn local(local: LocalId) -> Self {
        Self {
            local,
            projections: Arc::new([]),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PlaceKey {
    local: LocalId,
    projections: Arc<[ProjectionKey]>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ProjectionKey {
    Field(DefinitionId, Arc<str>),
    Index(Option<i128>),
}

fn projections_overlap(left: &ProjectionKey, right: &ProjectionKey) -> bool {
    match (left, right) {
        (ProjectionKey::Field(left_owner, left), ProjectionKey::Field(right_owner, right)) => {
            left_owner == right_owner && left == right
        }
        (ProjectionKey::Index(Some(left)), ProjectionKey::Index(Some(right))) => left == right,
        (ProjectionKey::Index(_), ProjectionKey::Index(_)) => true,
        _ => false,
    }
}

impl PlaceKey {
    fn from_place(place: &Place) -> Self {
        Self {
            local: place.local,
            projections: place
                .projections
                .iter()
                .map(|projection| match projection {
                    PlaceProjection::Field {
                        definition, name, ..
                    } => ProjectionKey::Field(*definition, name.clone()),
                    PlaceProjection::Index { index, .. } => {
                        ProjectionKey::Index(expression_integer_value(index))
                    }
                })
                .collect(),
        }
    }

    fn conflicts(&self, other: &Self) -> bool {
        if self.local != other.local {
            return false;
        }
        self.projections
            .iter()
            .zip(other.projections.iter())
            .all(|(left, right)| projections_overlap(left, right))
    }

    fn covers(&self, other: &Self) -> bool {
        self.local == other.local
            && self.projections.len() <= other.projections.len()
            && self
                .projections
                .iter()
                .zip(other.projections.iter())
                .all(|(left, right)| projections_overlap(left, right))
    }
}

#[derive(Clone, Debug, Default)]
struct MoveState {
    places: BTreeSet<PlaceKey>,
    restored: BTreeSet<PlaceKey>,
    origins: BTreeMap<PlaceKey, SourceRange>,
}

impl MoveState {
    fn move_key(&mut self, place: PlaceKey) -> bool {
        if self.is_key_unreadable(&place) {
            return false;
        }
        self.restored.retain(|restored| !place.conflicts(restored));
        self.places.insert(place)
    }

    fn move_key_at(&mut self, place: PlaceKey, source: &SourceRange) -> bool {
        if !self.move_key(place.clone()) {
            return false;
        }
        self.origins.insert(place, source.clone());
        true
    }

    fn is_key_fully_moved(&self, place: &PlaceKey) -> bool {
        self.places.iter().any(|moved| moved.covers(place))
            && !self.restored.iter().any(|restored| place.covers(restored))
    }

    fn is_key_unreadable(&self, place: &PlaceKey) -> bool {
        let moved_ancestor = self
            .places
            .iter()
            .filter(|moved| moved.covers(place))
            .map(|moved| moved.projections.len())
            .max();
        let restored_ancestor = self
            .restored
            .iter()
            .filter(|restored| restored.covers(place))
            .map(|restored| restored.projections.len())
            .max();
        moved_ancestor
            .is_some_and(|moved| restored_ancestor.is_none_or(|restored| moved >= restored))
            || self.places.iter().any(|moved| place.covers(moved))
    }

    fn is_unreadable(&self, place: &Place) -> bool {
        let place = PlaceKey::from_place(place);
        self.is_key_unreadable(&place)
    }

    fn move_place(&mut self, place: &Place) -> bool {
        self.move_key(PlaceKey::from_place(place))
    }

    fn move_place_at(&mut self, place: &Place, source: &SourceRange) -> bool {
        self.move_key_at(PlaceKey::from_place(place), source)
    }

    fn origin(&self, place: &Place) -> Option<&SourceRange> {
        let place = PlaceKey::from_place(place);
        self.origins
            .iter()
            .find_map(|(moved, source)| moved.conflicts(&place).then_some(source))
    }

    fn is_uninitialized(&self, place: &Place) -> bool {
        self.is_unreadable(place)
    }

    fn restore_place(&mut self, place: &Place) -> bool {
        let place = PlaceKey::from_place(place);
        let before = (self.places.len(), self.restored.len());
        self.places.retain(|moved| !place.covers(moved));
        self.origins.retain(|moved, _| !place.covers(moved));
        self.restored.retain(|restored| !place.covers(restored));
        if self.places.iter().any(|moved| moved.covers(&place)) {
            self.restored.insert(place);
        }
        before != (self.places.len(), self.restored.len())
    }

    fn contains_local(&self, local: LocalId) -> bool {
        self.is_unreadable(&Place::local(local))
    }

    fn restore_local(&mut self, local: LocalId) -> bool {
        self.restore_place(&Place::local(local))
    }

    fn finish_aggregate_restoration(&mut self, aggregate: &Place, children: &[Place]) {
        if self.is_unreadable(aggregate) && children.iter().all(|child| !self.is_unreadable(child))
        {
            self.restore_place(aggregate);
        }
    }

    fn mismatch(&self, other: &Self) -> Option<(PlaceKey, Option<SourceRange>)> {
        let place = self
            .places
            .symmetric_difference(&other.places)
            .next()
            .cloned()
            .or_else(|| {
                self.restored
                    .symmetric_difference(&other.restored)
                    .next()
                    .cloned()
            })?;
        let origin = self
            .origins
            .get(&place)
            .or_else(|| other.origins.get(&place))
            .cloned();
        Some((place, origin))
    }
}

#[derive(Clone, Debug)]
pub(crate) enum PlaceProjection {
    Field {
        definition: DefinitionId,
        name: Arc<str>,
        type_: Type,
        mutable: bool,
    },
    Index {
        index: Box<Expression>,
        type_: Type,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AccessMode {
    Copy,
    Read,
    Mut,
    Move,
}

impl AccessMode {
    const fn canonical_tag(self) -> u8 {
        match self {
            Self::Copy => 0x01,
            Self::Read => 0x02,
            Self::Mut => 0x03,
            Self::Move => 0x04,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedParameter {
    pub(crate) definition: DefinitionId,
    pub(crate) name: String,
    pub(crate) ownership: OwnershipSyntax,
    pub(crate) type_: Type,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedFunction {
    pub(crate) id: DefinitionId,
    pub(crate) module: ModuleId,
    pub(crate) module_display: String,
    pub(crate) name: String,
    pub(crate) modifier: crate::syntax::FunctionModifier,
    pub(crate) type_parameters: Arc<[crate::model::TypeParameterId]>,
    pub(crate) generic_parameter_names: Arc<[String]>,
    pub(crate) generic_constraints: Arc<[GenericConstraint]>,
    pub(crate) parameters: Vec<ResolvedParameter>,
    pub(crate) return_type: Type,
    pub(crate) body: Vec<StatementSyntax>,
    pub(crate) source: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedConstant {
    pub(crate) id: DefinitionId,
    pub(crate) module: ModuleId,
    pub(crate) name: String,
    pub(crate) type_: Type,
    pub(crate) value: ExpressionSyntax,
    pub(crate) source: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedTest {
    pub(crate) id: TestId,
    pub(crate) suite: String,
    pub(crate) test: String,
    pub(crate) asynchronous: bool,
    pub(crate) parameters: Vec<ResolvedParameter>,
    pub(crate) module: ModuleId,
    pub(crate) body: Vec<StatementSyntax>,
    pub(crate) source: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedVariant {
    pub(crate) order: u32,
    pub(crate) type_parameters: Vec<crate::model::TypeParameterId>,
    pub(crate) generic_constraints: Arc<[GenericConstraint]>,
    pub(crate) parameters: Vec<ResolvedParameter>,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedStruct {
    pub(crate) definition: DefinitionId,
    pub(crate) module: ModuleId,
    pub(crate) display: Arc<str>,
    pub(crate) resource: bool,
    pub(crate) type_parameters: Vec<crate::model::TypeParameterId>,
    pub(crate) generic_parameter_names: Arc<[String]>,
    pub(crate) generic_constraints: Arc<[GenericConstraint]>,
    pub(crate) fields: Vec<ResolvedField>,
    pub(crate) field_selections: Arc<[ResolvedFieldSelection]>,
    pub(crate) applied_fields: AppliedFieldTable,
}

type AppliedFieldTable = RefCell<BTreeMap<Arc<[Type]>, Arc<[ResolvedField]>>>;

#[derive(Clone, Debug)]
pub(crate) struct ResolvedFieldSelection {
    pub(crate) branches: Arc<[ResolvedFieldBranch]>,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedFieldBranch {
    pub(crate) condition: Option<ExpressionSyntax>,
    pub(crate) fields: Arc<[ResolvedField]>,
}

#[derive(Clone, Debug)]
pub(crate) enum GenericConstraint {
    Type { interface: Option<DefinitionId> },
    Const { type_: Type },
    Pool,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedField {
    pub(crate) definition: DefinitionId,
    pub(crate) name: String,
    pub(crate) public: bool,
    pub(crate) mutable: bool,
    pub(crate) type_: Type,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedInterface {
    pub(crate) requirements: BTreeMap<String, ResolvedInterfaceRequirement>,
    pub(crate) implementations: BTreeMap<DefinitionId, BTreeMap<String, DefinitionId>>,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedInterfaceRequirement {
    pub(crate) parameters: Vec<ResolvedParameter>,
    pub(crate) return_type: Type,
}

/// The closed vocabulary by which one Resource Type satisfies its recoverable-exit
/// obligation. This remains private to Typed HIR: source operations transfer or
/// discharge custody without learning how the compiler represents the proof.
#[derive(Clone, Debug, PartialEq, Eq)]
enum DischargeLaw {
    Explicit,
    CompilerReclaim(AuthenticatedReclaim),
    Structural(Arc<[DischargeComponent]>),
    ExistentialWitness(Arc<[WitnessDischarge]>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthenticatedReclaim {
    PoolOwnership,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DischargeComponent {
    identity: DischargeComponentIdentity,
    law: DischargeLaw,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DischargeComponentIdentity {
    Field(DefinitionId),
    Element,
    Tuple(u32),
    Success,
    Error,
    Present,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WitnessDischarge {
    witness: DefinitionId,
    law: DischargeLaw,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RawDischargeLaw {
    type_id: TypeId,
    type_: Type,
    law: DischargeLaw,
}

#[derive(Clone, Debug, Default)]
struct DischargeLawBuilder {
    observed: BTreeMap<TypeId, Type>,
    authenticated_reclaims: BTreeSet<TypeId>,
}

impl DischargeLawBuilder {
    fn observe(&mut self, type_id: TypeId, type_: &Type) -> Result<(), VerificationFailure> {
        match self.observed.entry(type_id) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(type_.clone());
                Ok(())
            }
            std::collections::btree_map::Entry::Occupied(entry) if entry.get() == type_ => Ok(()),
            std::collections::btree_map::Entry::Occupied(_) => Err(VerificationFailure::Defect {
                evidence: Arc::from(format!("discharge-law TypeId collision {:032x}", type_id.0)),
            }),
        }
    }

    fn authenticate_reclaim(&mut self, type_id: TypeId) -> Result<(), VerificationFailure> {
        if !self.observed.contains_key(&type_id) {
            return defect("authenticated reclaim names an unobserved TypeId");
        }
        self.authenticated_reclaims.insert(type_id);
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct VerifiedDischargeLaws {
    laws: BTreeMap<TypeId, DischargeLaw>,
    _verified: Verified,
}

#[derive(Clone, Debug)]
struct LiveDischargeObligation {
    place: PlaceKey,
    subject: Arc<str>,
    subject_identity: Option<DefinitionId>,
}

impl DischargeLaw {
    fn requires_explicit_discharge(&self) -> bool {
        match self {
            Self::Explicit => true,
            Self::CompilerReclaim(_) => false,
            Self::Structural(components) => components
                .iter()
                .any(|component| component.law.requires_explicit_discharge()),
            Self::ExistentialWitness(witnesses) => witnesses
                .iter()
                .any(|witness| witness.law.requires_explicit_discharge()),
        }
    }
}

fn derive_discharge_law(
    type_: &Type,
    structs: &BTreeMap<DefinitionId, ResolvedStruct>,
    interfaces: &BTreeMap<DefinitionId, ResolvedInterface>,
    nominal_displays: &BTreeMap<DefinitionId, Arc<str>>,
) -> Option<DischargeLaw> {
    match type_ {
        Type::Own { .. } => Some(DischargeLaw::CompilerReclaim(
            AuthenticatedReclaim::PoolOwnership,
        )),
        Type::Nominal {
            definition,
            arguments,
            ..
        } => {
            let struct_ = structs.get(definition)?;
            if !struct_.resource {
                return arguments.iter().find_map(|argument| {
                    derive_discharge_law(argument, structs, interfaces, nominal_displays)
                });
            }
            let substitutions = struct_
                .type_parameters
                .iter()
                .copied()
                .zip(arguments.iter().cloned())
                .collect::<BTreeMap<_, _>>();
            let fields = if struct_.field_selections.is_empty()
                || arguments.iter().any(type_has_placeholder)
            {
                Arc::from(struct_.fields.clone())
            } else {
                struct_
                    .applied_fields
                    .borrow()
                    .get(arguments)
                    .cloned()
                    .unwrap_or_else(|| Arc::from(struct_.fields.clone()))
            };
            let components = fields
                .iter()
                .filter_map(|field| {
                    derive_discharge_law(
                        &substitute(&field.type_, &substitutions),
                        structs,
                        interfaces,
                        nominal_displays,
                    )
                    .map(|law| DischargeComponent {
                        identity: DischargeComponentIdentity::Field(field.definition),
                        law,
                    })
                })
                .collect::<Vec<_>>();
            Some(if components.is_empty() {
                DischargeLaw::Explicit
            } else {
                DischargeLaw::Structural(components.into())
            })
        }
        Type::Array(element) | Type::FixedArray { element, .. } => {
            derive_discharge_law(element, structs, interfaces, nominal_displays).map(|law| {
                DischargeLaw::Structural(Arc::from([DischargeComponent {
                    identity: DischargeComponentIdentity::Element,
                    law,
                }]))
            })
        }
        Type::Tuple(members) => {
            let components = members
                .iter()
                .enumerate()
                .filter_map(|(index, member)| {
                    derive_discharge_law(member, structs, interfaces, nominal_displays).map(|law| {
                        DischargeComponent {
                            identity: DischargeComponentIdentity::Tuple(
                                u32::try_from(index).unwrap_or(u32::MAX),
                            ),
                            law,
                        }
                    })
                })
                .collect::<Vec<_>>();
            (!components.is_empty()).then(|| DischargeLaw::Structural(components.into()))
        }
        Type::Result { success, error } => {
            let components = derive_discharge_law(success, structs, interfaces, nominal_displays)
                .map(|law| DischargeComponent {
                    identity: DischargeComponentIdentity::Success,
                    law,
                })
                .into_iter()
                .chain(error.as_deref().and_then(|error| {
                    derive_discharge_law(error, structs, interfaces, nominal_displays).map(|law| {
                        DischargeComponent {
                            identity: DischargeComponentIdentity::Error,
                            law,
                        }
                    })
                }))
                .collect::<Vec<_>>();
            (!components.is_empty()).then(|| DischargeLaw::Structural(components.into()))
        }
        Type::Option(value) => derive_discharge_law(value, structs, interfaces, nominal_displays)
            .map(|law| {
                DischargeLaw::Structural(Arc::from([DischargeComponent {
                    identity: DischargeComponentIdentity::Present,
                    law,
                }]))
            }),
        Type::Any { interface, .. } => {
            let witnesses = interfaces
                .get(interface)?
                .implementations
                .keys()
                .filter_map(|definition| {
                    let display = nominal_displays.get(definition)?.clone();
                    derive_discharge_law(
                        &Type::Nominal {
                            definition: *definition,
                            display,
                            arguments: Arc::from([]),
                        },
                        structs,
                        interfaces,
                        nominal_displays,
                    )
                    .map(|law| WitnessDischarge {
                        witness: *definition,
                        law,
                    })
                })
                .collect::<Vec<_>>();
            (!witnesses.is_empty()).then(|| DischargeLaw::ExistentialWitness(witnesses.into()))
        }
        Type::Unit
        | Type::Bool
        | Type::Integer(_)
        | Type::Float(_)
        | Type::Text
        | Type::Scalar
        | Type::Bytes
        | Type::ConstU64(_)
        | Type::PoolArgument(_)
        | Type::Function { .. }
        | Type::Builtin(_)
        | Type::Parameter { .. }
        | Type::Infer => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolvedName {
    Function(DefinitionId),
    Constant(DefinitionId),
    Nominal(DefinitionId),
    Alias(DefinitionId),
    Pool(PoolId),
    Test(TestId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NamespaceEntry {
    name: ResolvedName,
    public: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct NamespaceCatalog {
    declarations: BTreeMap<ModuleId, BTreeMap<Arc<[String]>, NamespaceEntry>>,
    bindings: BTreeMap<ModuleId, BTreeMap<String, ModuleId>>,
    members: BTreeMap<DefinitionId, BTreeMap<String, (ModuleId, NamespaceEntry)>>,
    nominal_arities: BTreeMap<DefinitionId, usize>,
}

impl NamespaceCatalog {
    pub(crate) fn declare(
        &mut self,
        module: ModuleId,
        segments: impl Into<Arc<[String]>>,
        name: ResolvedName,
        public: bool,
    ) {
        self.declarations
            .entry(module)
            .or_default()
            .insert(segments.into(), NamespaceEntry { name, public });
    }

    pub(crate) fn bind(&mut self, module: ModuleId, alias: String, target: ModuleId) {
        self.bindings
            .entry(module)
            .or_default()
            .insert(alias, target);
    }

    pub(crate) fn set_nominal_arity(&mut self, definition: DefinitionId, arity: usize) {
        self.nominal_arities.insert(definition, arity);
    }

    pub(crate) fn nominal_arity(&self, definition: DefinitionId) -> Option<usize> {
        self.nominal_arities.get(&definition).copied()
    }

    pub(crate) fn declare_member(
        &mut self,
        owner: DefinitionId,
        module: ModuleId,
        name: String,
        resolved: ResolvedName,
        public: bool,
    ) {
        self.members.entry(owner).or_default().insert(
            name,
            (
                module,
                NamespaceEntry {
                    name: resolved,
                    public,
                },
            ),
        );
    }

    pub(crate) fn resolve_member(
        &self,
        requester: ModuleId,
        owner: DefinitionId,
        name: &str,
    ) -> Option<ResolvedName> {
        let (defining_module, entry) = self.members.get(&owner)?.get(name)?;
        (*defining_module == requester || entry.public).then_some(entry.name)
    }

    pub(crate) fn resolve(&self, module: ModuleId, segments: &[String]) -> Option<ResolvedName> {
        let (target, member_segments, imported) = match segments {
            [alias, rest @ ..] if !rest.is_empty() => self
                .bindings
                .get(&module)
                .and_then(|bindings| bindings.get(alias.as_str()))
                .map_or((module, segments, false), |target| (*target, rest, true)),
            _ => (module, segments, false),
        };
        let entry = self.declarations.get(&target)?.get(member_segments)?;
        (!imported || entry.public).then_some(entry.name)
    }

    fn has_binding(&self, module: ModuleId, alias: &str) -> bool {
        self.bindings
            .get(&module)
            .is_some_and(|bindings| bindings.contains_key(alias))
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ProgramInput {
    pub(crate) functions: BTreeMap<DefinitionId, ResolvedFunction>,
    pub(crate) constants: BTreeMap<DefinitionId, ResolvedConstant>,
    pub(crate) tests: BTreeMap<TestId, ResolvedTest>,
    pub(crate) variants: BTreeMap<VariantId, ResolvedVariant>,
    pub(crate) structs: BTreeMap<DefinitionId, ResolvedStruct>,
    pub(crate) interfaces: BTreeMap<DefinitionId, ResolvedInterface>,
    pub(crate) aliases: BTreeMap<DefinitionId, Type>,
    pub(crate) namespace: NamespaceCatalog,
    pub(crate) nominal_displays: BTreeMap<DefinitionId, Arc<str>>,
    pub(crate) comptime_roots: Vec<(ModuleId, ExpressionSyntax)>,
    pub(crate) inferred_error_definitions: BTreeSet<DefinitionId>,
}

#[derive(Clone, Debug)]
pub(crate) struct VerifiedProgram {
    functions: BTreeMap<DefinitionId, Arc<HirFunction>>,
    specialized_functions: BTreeMap<SpecializationId, Arc<HirFunction>>,
    default_specializations: BTreeMap<DefinitionId, SpecializationId>,
    constants: BTreeMap<DefinitionId, HirConstant>,
    tests: BTreeMap<TestId, ResolvedTest>,
    _test_bodies: BTreeMap<TestId, HirTest>,
    specializations: BTreeMap<SpecializationId, SpecializationRecord>,
    comptime_expressions: BTreeMap<SourceRange, ComptimeExpression>,
    closures: BTreeMap<ClosureId, Arc<HirClosure>>,
    inferred_error_definitions: BTreeSet<DefinitionId>,
    _discharge_laws: VerifiedDischargeLaws,
    fingerprint: u128,
    identity_catalog_revision: u128,
    _verified: Verified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ConditionSiteId(pub(crate) u128);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum EvaluationRoot {
    Constant(DefinitionId),
    Condition(ConditionSiteId),
    Image(SpecializationId),
}

impl EvaluationRoot {
    pub(crate) const fn identity(self) -> u128 {
        match self {
            Self::Constant(identity) => identity.0,
            Self::Condition(identity) => identity.0,
            Self::Image(identity) => identity.0,
        }
    }
}

#[derive(Clone, Debug)]
struct ComptimeExpression {
    root: ConditionSiteId,
    expression: Expression,
}

#[derive(Clone, Debug)]
struct Verified;

#[derive(Clone, Debug)]
pub(crate) struct HirFunction {
    pub(crate) id: DefinitionId,
    pub(crate) name: String,
    pub(crate) module_display: String,
    pub(crate) modifier: crate::syntax::FunctionModifier,
    pub(crate) parameters: Vec<(LocalId, Type, AccessMode)>,
    parameter_type_ids: Arc<[TypeId]>,
    pub(crate) parameter_definitions: Arc<[DefinitionId]>,
    pub(crate) return_type: Type,
    pub(crate) body: Arc<[Statement]>,
    pub(crate) source: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) struct HirConstant {
    pub(crate) id: DefinitionId,
    pub(crate) module: ModuleId,
    pub(crate) name: String,
    pub(crate) type_: Type,
    pub(crate) expression: Expression,
    pub(crate) source: SourceRange,
}

#[derive(Clone, Debug)]
struct HirTest {
    parameters: Vec<(LocalId, Type, AccessMode)>,
    parameter_type_ids: Arc<[TypeId]>,
    body: Arc<[Statement]>,
    source: SourceRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ClosureId(pub(crate) u128);

#[derive(Clone, Debug)]
pub(crate) struct HirClosure {
    pub(crate) id: ClosureId,
    pub(crate) parameters: Arc<[(LocalId, Type)]>,
    parameter_type_ids: Arc<[TypeId]>,
    pub(crate) captures: Arc<[(LocalId, Type)]>,
    capture_type_ids: Arc<[TypeId]>,
    pub(crate) return_type: Type,
    pub(crate) body: Expression,
    pub(crate) source: SourceRange,
    identity_key: Arc<[u8]>,
}

#[derive(Clone, Debug)]
pub(crate) enum Statement {
    Return {
        value: Option<Expression>,
        source: SourceRange,
    },
    Panic {
        value: Expression,
        source: SourceRange,
    },
    Assert {
        condition: Expression,
        source: SourceRange,
    },
    Expect {
        condition: Expression,
        source: SourceRange,
    },
    Initialize {
        place: Place,
        value: Expression,
        source: SourceRange,
    },
    Assign {
        place: Place,
        value: Expression,
        source: SourceRange,
    },
    Evaluate(Expression),
    If {
        condition: Expression,
        then_branch: Arc<[Statement]>,
        else_branch: Arc<[Statement]>,
        source: SourceRange,
    },
    IfPattern {
        value: Expression,
        pattern: HirMatchPattern,
        then_branch: Arc<[Statement]>,
        else_branch: Arc<[Statement]>,
        source: SourceRange,
    },
    For {
        pattern: HirMatchPattern,
        iterable: Expression,
        body: Arc<[Statement]>,
        source: SourceRange,
    },
    While {
        condition: Expression,
        body: Arc<[Statement]>,
        max_iterations: u64,
        source: SourceRange,
    },
    Break(SourceRange),
    Continue(SourceRange),
    Match {
        value: Expression,
        cases: Arc<[HirMatchCase]>,
        source: SourceRange,
    },
    Defer {
        action: CleanupAction,
        source: SourceRange,
    },
    WithPool {
        binding: Place,
        scope: Expression,
        body: Arc<[Statement]>,
        source: SourceRange,
    },
    Pass(SourceRange),
}

#[derive(Clone, Debug)]
pub(crate) struct CleanupAction {
    pub(crate) expression: Expression,
    pub(crate) captures: Arc<[CleanupCapture]>,
}

impl CleanupAction {
    pub(crate) fn expression(&self) -> &Expression {
        &self.expression
    }

    pub(crate) fn visit_expressions(&self, visitor: &mut impl FnMut(&Expression)) {
        for capture in self.captures.iter() {
            visitor(&capture.expression);
        }
        visitor(&self.expression);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CleanupCapture {
    pub(crate) ordinal: CleanupCaptureOrdinal,
    pub(crate) expression: Expression,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct CleanupCaptureOrdinal(pub(crate) u32);

#[derive(Clone, Debug)]
pub(crate) struct HirMatchCase {
    pub(crate) pattern: Option<HirMatchPattern>,
    pub(crate) guard: Option<Expression>,
    pub(crate) body: Arc<[Statement]>,
    pub(crate) source: SourceRange,
}

#[derive(Clone, Debug)]
pub(crate) enum HirMatchPattern {
    Wildcard,
    Literal(Literal),
    Variant {
        id: VariantId,
        payload: Arc<[HirMatchPattern]>,
    },
    Struct {
        definition: DefinitionId,
        fields: Arc<[HirMatchPattern]>,
    },
    Tuple(Arc<[HirMatchPattern]>),
    FixedArray(Arc<[HirMatchPattern]>),
    Or(Arc<[HirMatchPattern]>),
    Binding {
        local: LocalId,
        type_id: TypeId,
        type_: Type,
        access: AccessMode,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct Expression {
    pub(crate) kind: ExpressionKind,
    pub(crate) type_id: TypeId,
    pub(crate) type_: Type,
    pub(crate) coerced_from: Option<Type>,
    pub(crate) access: AccessMode,
    pub(crate) source: SourceRange,
}

impl Expression {
    pub(crate) fn visit_children(&self, visitor: &mut impl FnMut(&Expression)) {
        match &self.kind {
            ExpressionKind::Call { target, arguments } => {
                if let CallTarget::Callable { value } = target {
                    visitor(value);
                }
                arguments.iter().for_each(visitor);
            }
            ExpressionKind::Array(arguments) | ExpressionKind::Tuple(arguments) => {
                arguments.iter().for_each(visitor);
            }
            ExpressionKind::RepeatedArray { value, .. }
            | ExpressionKind::Positive(value)
            | ExpressionKind::Negate(value)
            | ExpressionKind::BitNot(value)
            | ExpressionKind::Not(value)
            | ExpressionKind::Await(value)
            | ExpressionKind::Propagate(value) => visitor(value),
            ExpressionKind::Index { value, index }
            | ExpressionKind::Binary {
                left: value,
                right: index,
                ..
            } => {
                visitor(value);
                visitor(index);
            }
            ExpressionKind::Read(place) => {
                for projection in place.projections.iter() {
                    if let PlaceProjection::Index { index, .. } = projection {
                        visitor(index);
                    }
                }
            }
            ExpressionKind::Literal(_)
            | ExpressionKind::Constant(_)
            | ExpressionKind::FunctionValue { .. }
            | ExpressionKind::CleanupCapture(_) => {}
            ExpressionKind::Is { value, .. } => visitor(value),
            ExpressionKind::Closure(closure) => visitor(&closure.body),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum ExpressionKind {
    Literal(Literal),
    Read(Place),
    Constant(DefinitionId),
    FunctionValue {
        definition: DefinitionId,
        specialization: Option<SpecializationId>,
    },
    Closure(Arc<HirClosure>),
    CleanupCapture(CleanupCaptureOrdinal),
    Call {
        target: CallTarget,
        arguments: Arc<[Expression]>,
    },
    Array(Arc<[Expression]>),
    RepeatedArray {
        value: Box<Expression>,
        length: u64,
    },
    Tuple(Arc<[Expression]>),
    Index {
        value: Box<Expression>,
        index: Box<Expression>,
    },
    Positive(Box<Expression>),
    Negate(Box<Expression>),
    BitNot(Box<Expression>),
    Not(Box<Expression>),
    Await(Box<Expression>),
    Propagate(Box<Expression>),
    Is {
        value: Box<Expression>,
        pattern: Box<HirMatchPattern>,
    },
    Binary {
        operator: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum Literal {
    Unit,
    Bool(bool),
    Integer { kind: IntegerType, value: i128 },
    Float { kind: FloatType, bits: u64 },
    Text(Arc<str>),
    Scalar(char),
    Bytes(Arc<[u8]>),
}

#[derive(Clone, Debug)]
pub(crate) enum CallTarget {
    Callable {
        value: Box<Expression>,
    },
    TemplateFunction {
        definition: DefinitionId,
        argument_order: Arc<[u16]>,
        argument_parameters: Arc<[DefinitionId]>,
    },
    Function {
        definition: DefinitionId,
        specialization: SpecializationId,
        argument_order: Arc<[u16]>,
        argument_parameters: Arc<[DefinitionId]>,
    },
    Build {
        primitive: BuildPrimitive,
        labels: Arc<[Arc<str>]>,
    },
    BuiltinVariant(BuiltinVariant),
    UserVariant {
        id: VariantId,
        variant_order: u32,
        type_display: Arc<str>,
        variant_display: Arc<str>,
        argument_order: Arc<[u16]>,
        argument_parameters: Arc<[DefinitionId]>,
    },
    Interface {
        interface: DefinitionId,
        alternatives: Arc<[(DefinitionId, DefinitionId, SpecializationId)]>,
        argument_order: Arc<[u16]>,
        argument_parameters: Arc<[DefinitionId]>,
    },
    Struct {
        definition: DefinitionId,
        type_display: Arc<str>,
        field_order: Arc<[Arc<str>]>,
        argument_fields: Arc<[Arc<str>]>,
        argument_field_definitions: Arc<[DefinitionId]>,
    },
    Test {
        id: TestId,
        argument_order: Arc<[u16]>,
        argument_parameters: Arc<[DefinitionId]>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct BuildPrimitive {
    pub(crate) identity: u128,
    pub(crate) kind: BuildKind,
    pub(crate) definition: DefinitionId,
}

#[derive(Clone, Debug)]
pub(crate) struct SpecializationRecord {
    pub(crate) id: SpecializationId,
    pub(crate) definition: DefinitionId,
    pub(crate) type_arguments: Arc<[Type]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinaryOperator {
    Range,
    RangeInclusive,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
    And,
    Or,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

impl BinaryOperator {
    const fn canonical_tag(self) -> u8 {
        match self {
            Self::Range => 0x20,
            Self::RangeInclusive => 0x21,
            Self::Add => 0x01,
            Self::Subtract => 0x02,
            Self::Multiply => 0x03,
            Self::Divide => 0x04,
            Self::Remainder => 0x05,
            Self::BitAnd => 0x06,
            Self::BitOr => 0x07,
            Self::BitXor => 0x08,
            Self::ShiftLeft => 0x09,
            Self::ShiftRight => 0x0a,
            Self::And => 0x0b,
            Self::Or => 0x0c,
            Self::Equal => 0x11,
            Self::NotEqual => 0x12,
            Self::Less => 0x13,
            Self::LessEqual => 0x14,
            Self::Greater => 0x15,
            Self::GreaterEqual => 0x16,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CreatorFailureKind {
    EmptyName,
    DuplicateParameter,
    ConstantTypeMismatch,
    ReturnTypeMismatch,
    IfConditionRequiresBool,
    UnresolvedName,
    UnresolvedCall,
    UnresolvedNominalType,
    ArgumentCount,
    ArgumentTypeMismatch,
    ArgumentOwnershipMismatch,
    ArgumentLabelMismatch,
    GenericArgumentConflict,
    PropagationRequiresResult,
    BinaryTypeMismatch,
    ArrayElementTypeMismatch,
    InvalidUnaryOperand,
    InvalidIntegerLiteral,
    InvalidFloatLiteral,
    ReadAfterMove,
    LoanConflict,
    LoanAcrossSuspension,
    CustodyJoinMismatch,
    ImmutableReassignment,
    DuplicateLocal,
    AwaitRequiresAsync,
    ExpectRequiresBool,
    TestApplicationOutsideCases,
    InvalidComptimeExpression,
    UnsupportedLayerOneSyntax,
    InvalidMatchPattern,
    NonExhaustiveMatch,
    UnboundedWhile,
    LoopControlOutsideLoop,
    TakeRequiresResourcePlace,
    InvalidFunctionValue,
    ClosureCaptureRequiresData,
    DeferReturnsRecoverableError,
    LoanAcrossCleanup,
    CleanupResourceCaptureRequiresCall,
    UnreachableMatchCase,
    UnresolvedType,
    ResourceRequiresTake,
    ResourceReplacementRequiresDischarge,
    ResourceNotDischarged,
    MustUseValue,
}

impl CreatorFailureKind {
    pub(crate) const fn diagnostic_code(self) -> &'static str {
        match self {
            Self::EmptyName => "semantic.empty_name",
            Self::DuplicateParameter => "semantic.duplicate_parameter",
            Self::ConstantTypeMismatch => "semantic.constant_type_mismatch",
            Self::ReturnTypeMismatch => "semantic.return_type_mismatch",
            Self::IfConditionRequiresBool => "semantic.if_condition_requires_bool",
            Self::UnresolvedName => "semantic.unresolved_name",
            Self::UnresolvedCall => "semantic.unresolved_call",
            Self::UnresolvedNominalType => "semantic.unresolved_nominal_type",
            Self::ArgumentCount => "semantic.argument_count",
            Self::ArgumentTypeMismatch => "semantic.argument_type_mismatch",
            Self::ArgumentOwnershipMismatch => "semantic.argument_ownership_mismatch",
            Self::ArgumentLabelMismatch => "semantic.argument_label_mismatch",
            Self::GenericArgumentConflict => "semantic.generic_argument_conflict",
            Self::PropagationRequiresResult => "semantic.propagation_requires_result",
            Self::BinaryTypeMismatch => "semantic.binary_type_mismatch",
            Self::ArrayElementTypeMismatch => "semantic.array_element_type_mismatch",
            Self::InvalidUnaryOperand => "semantic.invalid_unary_operand",
            Self::InvalidIntegerLiteral => "semantic.invalid_integer_literal",
            Self::InvalidFloatLiteral => "semantic.invalid_float_literal",
            Self::ReadAfterMove => "semantic.read_after_move",
            Self::LoanConflict => "semantic.loan_conflict",
            Self::LoanAcrossSuspension => "semantic.loan_across_suspension",
            Self::CustodyJoinMismatch => "semantic.custody_join_mismatch",
            Self::ImmutableReassignment => "semantic.immutable_reassignment",
            Self::DuplicateLocal => "semantic.duplicate_local",
            Self::AwaitRequiresAsync => "semantic.await_requires_async",
            Self::ExpectRequiresBool => "semantic.expect_requires_bool",
            Self::TestApplicationOutsideCases => "semantic.test_application_outside_cases",
            Self::InvalidComptimeExpression => "semantic.invalid_comptime_expression",
            Self::UnsupportedLayerOneSyntax => "semantic.unsupported_layer_one_syntax",
            Self::InvalidMatchPattern => "semantic.invalid_match_pattern",
            Self::NonExhaustiveMatch => "semantic.non_exhaustive_match",
            Self::UnboundedWhile => "semantic.unbounded_while",
            Self::LoopControlOutsideLoop => "semantic.loop_control_outside_loop",
            Self::TakeRequiresResourcePlace => "semantic.take_requires_resource_place",
            Self::InvalidFunctionValue => "semantic.invalid_function_value",
            Self::ClosureCaptureRequiresData => "semantic.closure_capture_requires_data",
            Self::DeferReturnsRecoverableError => "semantic.defer_returns_recoverable_error",
            Self::LoanAcrossCleanup => "semantic.loan_across_cleanup",
            Self::CleanupResourceCaptureRequiresCall => {
                "semantic.cleanup_resource_capture_requires_call"
            }
            Self::UnreachableMatchCase => "semantic.unreachable_match_case",
            Self::UnresolvedType => "semantic.unresolved_type",
            Self::ResourceRequiresTake => "semantic.resource_requires_take",
            Self::ResourceReplacementRequiresDischarge => {
                "semantic.resource_replacement_requires_discharge"
            }
            Self::ResourceNotDischarged => "semantic.resource_not_discharged",
            Self::MustUseValue => "semantic.must_use_value",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CustodyDiagnosticState {
    Moved,
    Initialized,
    Loaned,
    ConflictingLoan,
    PathDependent,
    LiveUndischarged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum VerificationFailure {
    Creator {
        kind: CreatorFailureKind,
        site: SourceRange,
    },
    Custody {
        kind: CreatorFailureKind,
        site: SourceRange,
        subject: Arc<str>,
        state: CustodyDiagnosticState,
        identities: Box<CustodyIdentities>,
        related: Option<SourceRange>,
    },
    Defect {
        evidence: Arc<str>,
    },
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CustodyIdentities {
    pub(crate) subject: Option<DefinitionId>,
    pub(crate) owner: Option<DefinitionId>,
}

#[derive(Clone, Debug)]
pub(crate) struct BuildAuthority {
    ambient_grants: BTreeMap<Arc<[String]>, BuildPrimitive>,
    grants_by_definition: BTreeMap<DefinitionId, BuildPrimitive>,
    _sealed: SealedAuthority,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PoolAuthority {
    scoped_factory: Option<DefinitionId>,
    _sealed: SealedPoolAuthority,
}

#[derive(Clone, Copy)]
pub(crate) struct AuthorityContext<'a> {
    build: &'a BuildAuthority,
    pool: &'a PoolAuthority,
}

impl<'a> AuthorityContext<'a> {
    pub(crate) const fn new(build: &'a BuildAuthority, pool: &'a PoolAuthority) -> Self {
        Self { build, pool }
    }

    pub(crate) const fn build(self) -> &'a BuildAuthority {
        self.build
    }

    pub(crate) const fn pool(self) -> &'a PoolAuthority {
        self.pool
    }
}

#[derive(Clone, Copy, Debug)]
struct SealedPoolAuthority;

impl PoolAuthority {
    pub(crate) const fn from_authenticated_scoped_factory(
        scoped_factory: Option<DefinitionId>,
    ) -> Self {
        Self {
            scoped_factory,
            _sealed: SealedPoolAuthority,
        }
    }

    pub(crate) const fn is_scoped_factory(self, definition: DefinitionId) -> bool {
        matches!(self.scoped_factory, Some(factory) if factory.0 == definition.0)
    }

    pub(crate) fn canonical_grants(self) -> impl Iterator<Item = DefinitionId> {
        self.scoped_factory.into_iter()
    }
}

#[derive(Clone, Debug)]
struct SealedAuthority;

impl BuildAuthority {
    pub(crate) fn from_authenticated_declarations(
        declarations: impl IntoIterator<Item = (Arc<[String]>, BuildKind, DefinitionId)>,
    ) -> Self {
        let mut ambient_grants = BTreeMap::new();
        let mut grants_by_definition = BTreeMap::new();
        for (name, kind, definition) in declarations {
            let mut key = b"wrela.authenticated-build-primitive\0\x02".to_vec();
            key.extend_from_slice(&definition.0.to_be_bytes());
            key.push(kind.canonical_tag());
            if let BuildKind::Node {
                definition,
                type_identity,
            } = kind
            {
                key.extend_from_slice(&definition.0.to_be_bytes());
                key.extend_from_slice(&type_identity.0.to_be_bytes());
            }
            let primitive = BuildPrimitive {
                identity: xxh3_128(&key),
                kind,
                definition,
            };
            if matches!(kind, BuildKind::Image | BuildKind::Test) {
                ambient_grants.insert(name, primitive);
            }
            grants_by_definition.insert(definition, primitive);
        }
        Self {
            ambient_grants,
            grants_by_definition,
            _sealed: SealedAuthority,
        }
    }

    fn resolve_name(&self, name: &NameSyntax) -> Option<BuildPrimitive> {
        self.ambient_grants.get(name.segments.as_slice()).copied()
    }

    fn resolve_definition(&self, definition: DefinitionId) -> Option<BuildPrimitive> {
        self.grants_by_definition.get(&definition).copied()
    }

    pub(crate) fn canonical_grants(
        &self,
    ) -> impl Iterator<Item = (DefinitionId, BuildKind, u128)> + '_ {
        self.grants_by_definition
            .iter()
            .map(|(definition, primitive)| (*definition, primitive.kind, primitive.identity))
    }

    #[cfg(test)]
    pub(crate) fn test_compiler_distribution() -> Self {
        Self::from_authenticated_declarations([
            (
                Arc::from(["Image".to_owned(), "new".to_owned()]),
                BuildKind::Image,
                DefinitionId(1),
            ),
            (
                Arc::from(["Test".to_owned(), "new".to_owned()]),
                BuildKind::Test,
                DefinitionId(2),
            ),
        ])
    }
}

impl VerifiedProgram {
    pub(crate) fn functions(&self) -> &BTreeMap<DefinitionId, Arc<HirFunction>> {
        &self.functions
    }
    pub(crate) fn constants(&self) -> &BTreeMap<DefinitionId, HirConstant> {
        &self.constants
    }
    pub(crate) fn specialization_function(&self, id: SpecializationId) -> Option<&HirFunction> {
        self.specialized_functions.get(&id).map(AsRef::as_ref)
    }
    pub(crate) fn specialized_functions(&self) -> &BTreeMap<SpecializationId, Arc<HirFunction>> {
        &self.specialized_functions
    }
    pub(crate) fn default_specialization(&self, id: DefinitionId) -> Option<SpecializationId> {
        self.default_specializations.get(&id).copied()
    }
    pub(crate) fn test(&self, id: TestId) -> Option<&ResolvedTest> {
        let test = self.tests.get(&id)?;
        debug_assert_eq!(test.id, id);
        Some(test)
    }
    pub(crate) fn specializations(&self) -> &BTreeMap<SpecializationId, SpecializationRecord> {
        &self.specializations
    }
    pub(crate) fn closure(&self, id: ClosureId) -> Option<&HirClosure> {
        self.closures.get(&id).map(AsRef::as_ref)
    }
    pub(crate) fn specialization_closures(&self, id: SpecializationId) -> BTreeSet<ClosureId> {
        let mut closures = BTreeMap::new();
        if let Some(function) = self.specialization_function(id) {
            collect_statement_closures(&function.body, &mut closures)
                .expect("verified specialization has unique Closure identities");
        }
        closures.into_keys().collect()
    }
    pub(crate) fn test_closures(&self, id: TestId) -> BTreeSet<ClosureId> {
        let mut closures = BTreeMap::new();
        if let Some(body) = self.test_body(id) {
            collect_statement_closures(body, &mut closures)
                .expect("verified Test body has unique Closure identities");
        }
        closures.into_keys().collect()
    }
    pub(crate) fn inferred_error_definitions(&self) -> &BTreeSet<DefinitionId> {
        &self.inferred_error_definitions
    }
    pub(crate) const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }

    pub(crate) const fn identity_catalog_revision(&self) -> u128 {
        self.identity_catalog_revision
    }

    pub(crate) fn test_body(&self, id: TestId) -> Option<&[Statement]> {
        self._test_bodies.get(&id).map(|test| test.body.as_ref())
    }

    pub(crate) fn expected_evaluation_roots(
        &self,
        image: SpecializationId,
    ) -> BTreeSet<EvaluationRoot> {
        let mut roots = self
            .constants
            .keys()
            .copied()
            .map(EvaluationRoot::Constant)
            .collect::<BTreeSet<_>>();
        roots.extend(
            self.comptime_expressions
                .values()
                .map(|expression| EvaluationRoot::Condition(expression.root)),
        );
        roots.insert(EvaluationRoot::Image(image));
        roots
    }

    pub(crate) fn comptime_root(&self, expression: &Expression) -> Option<ConditionSiteId> {
        self.comptime_expressions
            .get(&expression.source)
            .map(|root| root.root)
    }
    pub(crate) fn comptime_expression(&self, identity: ConditionSiteId) -> Option<&Expression> {
        self.comptime_expressions
            .values()
            .find(|expression| expression.root == identity)
            .map(|expression| &expression.expression)
    }

    pub(crate) fn custody_fingerprint(&self) -> u128 {
        let mut canonical = Xxh3::new();
        canonical.extend_from_slice(b"wrela.resource-custody-authority\0\x01");
        for (type_id, law) in &self._discharge_laws.laws {
            canonical.extend_from_slice(&type_id.0.to_be_bytes());
            append_discharge_law(&mut canonical, law);
        }
        canonical.digest128()
    }

    pub(crate) fn verify_expression(
        &self,
        syntax: &ExpressionSyntax,
    ) -> Result<Expression, VerificationFailure> {
        self.comptime_expressions
            .get(&syntax.range)
            .map(|root| root.expression.clone())
            .ok_or_else(|| VerificationFailure::Defect {
                evidence: Arc::from("comptime expression was not included in concrete demand"),
            })
    }
}

fn condition_site_identity(module: ModuleId, source: &SourceRange) -> ConditionSiteId {
    let mut canonical = Xxh3::new();
    canonical.extend_from_slice(b"wrela.condition-site\0\x01");
    canonical.extend_from_slice(&module.0.to_be_bytes());
    canonical.extend_from_slice(&source.start().to_be_bytes());
    canonical.extend_from_slice(&source.end().to_be_bytes());
    ConditionSiteId(canonical.digest128())
}

pub(crate) fn verify(
    input: ProgramInput,
    build_authority: &BuildAuthority,
    pool_authority: &PoolAuthority,
    identity_catalog: &mut IdentityCatalog,
    cancellation: &Cancellation,
) -> Result<VerifiedProgram, VerificationFailure> {
    validate_input(&input)?;
    let mut discharge_laws = DischargeLawBuilder::default();
    intern_input_types(&input, identity_catalog, &mut discharge_laws)?;
    let mut specializations = BTreeMap::new();
    let mut pending_specializations = BTreeSet::new();
    let mut comptime_expressions = BTreeMap::new();
    let mut constants = BTreeMap::new();
    for constant in input.constants.values() {
        if cancellation.is_cancelled() {
            return Err(VerificationFailure::Cancelled);
        }
        let mut lowerer = Lowerer::new(
            constant.module,
            &input,
            AuthorityContext::new(build_authority, pool_authority),
            identity_catalog,
            &mut discharge_laws,
            cancellation,
            SpecializationDemands {
                records: &mut specializations,
                pending: &mut pending_specializations,
            },
            true,
        );
        let expression = lowerer.expression_expected(&constant.value, &constant.type_)?;
        if !can_initialize(&expression.type_, &constant.type_) {
            return creator(CreatorFailureKind::ConstantTypeMismatch, &constant.source);
        }
        constants.insert(
            constant.id,
            HirConstant {
                id: constant.id,
                module: constant.module,
                name: constant.name.clone(),
                type_: constant.type_.clone(),
                expression,
                source: constant.source.clone(),
            },
        );
    }
    for (module, root) in &input.comptime_roots {
        if cancellation.is_cancelled() {
            return Err(VerificationFailure::Cancelled);
        }
        let expression = Lowerer::new(
            *module,
            &input,
            AuthorityContext::new(build_authority, pool_authority),
            identity_catalog,
            &mut discharge_laws,
            cancellation,
            SpecializationDemands {
                records: &mut specializations,
                pending: &mut pending_specializations,
            },
            true,
        )
        .expression(root)?;
        comptime_expressions.insert(
            root.range.clone(),
            ComptimeExpression {
                root: condition_site_identity(*module, &root.range),
                expression,
            },
        );
    }

    let mut functions = BTreeMap::new();
    for function in input.functions.values() {
        if cancellation.is_cancelled() {
            return Err(VerificationFailure::Cancelled);
        }
        let mut lowerer = Lowerer::new(
            function.module,
            &input,
            AuthorityContext::new(build_authority, pool_authority),
            identity_catalog,
            &mut discharge_laws,
            cancellation,
            SpecializationDemands {
                records: &mut specializations,
                pending: &mut pending_specializations,
            },
            function.type_parameters.is_empty(),
        );
        lowerer.set_generic_context(function, None);
        let mut parameters = Vec::new();
        let mut parameter_type_ids = Vec::new();
        for parameter in &function.parameters {
            let local = lowerer.bind_parameter(
                &parameter.name,
                parameter.type_.clone(),
                parameter.ownership,
            )?;
            parameters.push((
                local,
                parameter.type_.clone(),
                match parameter.ownership {
                    OwnershipSyntax::Value => AccessMode::Copy,
                    OwnershipSyntax::Read => AccessMode::Read,
                    OwnershipSyntax::Mut => AccessMode::Mut,
                    OwnershipSyntax::Take => AccessMode::Move,
                },
            ));
            parameter_type_ids.push(observe_type_tree(
                &parameter.type_,
                lowerer.identity_catalog,
                lowerer.discharge_laws,
            )?);
        }
        let body = lowerer.statements(&function.body, &function.return_type)?;
        if syntax_statements_fall_through(&function.body) {
            lowerer.check_recoverable_exit()?;
        }
        functions.insert(
            function.id,
            Arc::new(HirFunction {
                id: function.id,
                name: function.name.clone(),
                module_display: function.module_display.clone(),
                modifier: function.modifier,
                parameters,
                parameter_type_ids: parameter_type_ids.into(),
                parameter_definitions: function
                    .parameters
                    .iter()
                    .map(|parameter| parameter.definition)
                    .collect(),
                return_type: function.return_type.clone(),
                body: body.into(),
                source: function.source.clone(),
            }),
        );
    }

    let mut default_specializations = BTreeMap::new();
    for function in input
        .functions
        .values()
        .filter(|function| function.type_parameters.is_empty())
    {
        let id = identity_catalog
            .specialization(function.id, &[])
            .map_err(|collision| VerificationFailure::Defect {
                evidence: Arc::from(format!(
                    "specialization identity collision {:032x}",
                    collision.digest
                )),
            })?;
        default_specializations.insert(function.id, id);
        if let std::collections::btree_map::Entry::Vacant(entry) = specializations.entry(id) {
            entry.insert(SpecializationRecord {
                id,
                definition: function.id,
                type_arguments: Arc::from([]),
            });
            pending_specializations.insert(id);
        }
    }

    let mut specialized_functions = BTreeMap::new();
    materialize_missing_specializations(
        &input,
        AuthorityContext::new(build_authority, pool_authority),
        identity_catalog,
        &mut discharge_laws,
        cancellation,
        &functions,
        &mut SpecializationDemands {
            records: &mut specializations,
            pending: &mut pending_specializations,
        },
        &mut specialized_functions,
    )?;

    let mut test_bodies = BTreeMap::new();
    for test in input.tests.values() {
        let mut lowerer = Lowerer::new(
            test.module,
            &input,
            AuthorityContext::new(build_authority, pool_authority),
            identity_catalog,
            &mut discharge_laws,
            cancellation,
            SpecializationDemands {
                records: &mut specializations,
                pending: &mut pending_specializations,
            },
            true,
        );
        let mut parameters = Vec::new();
        let mut parameter_type_ids = Vec::new();
        for parameter in &test.parameters {
            let local = lowerer.bind_parameter(
                &parameter.name,
                parameter.type_.clone(),
                parameter.ownership,
            )?;
            parameters.push((
                local,
                parameter.type_.clone(),
                match parameter.ownership {
                    OwnershipSyntax::Value => AccessMode::Copy,
                    OwnershipSyntax::Read => AccessMode::Read,
                    OwnershipSyntax::Mut => AccessMode::Mut,
                    OwnershipSyntax::Take => AccessMode::Move,
                },
            ));
            parameter_type_ids.push(observe_type_tree(
                &parameter.type_,
                lowerer.identity_catalog,
                lowerer.discharge_laws,
            )?);
        }
        let body: Arc<[Statement]> = lowerer.statements(&test.body, &Type::Unit)?.into();
        if syntax_statements_fall_through(&test.body) {
            lowerer.check_recoverable_exit()?;
        }
        if !test.asynchronous && statements_suspend(&body) {
            return creator(CreatorFailureKind::AwaitRequiresAsync, &test.source);
        }
        test_bodies.insert(
            test.id,
            HirTest {
                parameters,
                parameter_type_ids: parameter_type_ids.into(),
                body,
                source: test.source.clone(),
            },
        );
    }

    materialize_missing_specializations(
        &input,
        AuthorityContext::new(build_authority, pool_authority),
        identity_catalog,
        &mut discharge_laws,
        cancellation,
        &functions,
        &mut SpecializationDemands {
            records: &mut specializations,
            pending: &mut pending_specializations,
        },
        &mut specialized_functions,
    )?;

    for (id, function) in &specialized_functions {
        let mut implementation = Xxh3::new();
        implementation.extend_from_slice(b"wrela.specialization-implementation\0\x01");
        append_function(&mut implementation, function);
        if !identity_catalog.set_specialization_fingerprint(*id, implementation.digest128()) {
            return defect("specialized body has no identity-catalog observation");
        }
    }
    let discharge_laws = construct_and_verify_discharge_laws(&input, &discharge_laws)?;
    identity_catalog.finalize();

    let mut closures = BTreeMap::new();
    for function in functions.values().chain(specialized_functions.values()) {
        collect_statement_closures(&function.body, &mut closures)?;
    }
    for constant in constants.values() {
        collect_expression_closures(&constant.expression, &mut closures)?;
    }
    for test in test_bodies.values() {
        collect_statement_closures(&test.body, &mut closures)?;
    }
    for expression in comptime_expressions.values() {
        collect_expression_closures(&expression.expression, &mut closures)?;
    }

    let mut canonical = Xxh3::new();
    canonical.extend_from_slice(b"wrela.typed-hir\0\x04");
    canonical.push(0);
    canonical.extend_from_slice(&identity_catalog.revision_fingerprint().to_be_bytes());
    append_collection_header(&mut canonical, 1, functions.len());
    for (id, function) in &functions {
        append_part(&mut canonical, &id.0.to_be_bytes());
        append_function(&mut canonical, function);
    }
    append_collection_header(&mut canonical, 2, constants.len());
    for (id, constant) in &constants {
        append_part(&mut canonical, &id.0.to_be_bytes());
        append_part(&mut canonical, &constant.type_.canonical_key());
        append_expression(&mut canonical, &constant.expression);
    }
    append_collection_header(&mut canonical, 3, specialized_functions.len());
    for (id, function) in &specialized_functions {
        append_part(&mut canonical, &id.0.to_be_bytes());
        append_function(&mut canonical, function);
    }
    append_collection_header(&mut canonical, 4, default_specializations.len());
    for (definition, specialization) in &default_specializations {
        canonical.extend_from_slice(&definition.0.to_be_bytes());
        canonical.extend_from_slice(&specialization.0.to_be_bytes());
    }
    append_collection_header(&mut canonical, 5, specializations.len());
    for (id, specialization) in &specializations {
        canonical.extend_from_slice(&id.0.to_be_bytes());
        canonical.extend_from_slice(&specialization.definition.0.to_be_bytes());
        canonical.extend_from_slice(
            &u64::try_from(specialization.type_arguments.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        for argument in &*specialization.type_arguments {
            append_part(&mut canonical, &argument.canonical_key());
        }
    }
    append_collection_header(&mut canonical, 6, input.tests.len());
    for (id, test) in &input.tests {
        canonical.extend_from_slice(&id.suite.0.to_be_bytes());
        canonical.extend_from_slice(&id.test.0.to_be_bytes());
        canonical.extend_from_slice(&id.identity.to_be_bytes());
        append_part(&mut canonical, test.suite.as_bytes());
        append_part(&mut canonical, test.test.as_bytes());
        canonical.push(u8::from(test.asynchronous));
        canonical.extend_from_slice(
            &u64::try_from(test.parameters.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        for parameter in &test.parameters {
            append_part(&mut canonical, parameter.name.as_bytes());
            canonical.push(match parameter.ownership {
                OwnershipSyntax::Value => 0,
                OwnershipSyntax::Read => 1,
                OwnershipSyntax::Mut => 2,
                OwnershipSyntax::Take => 3,
            });
            append_part(&mut canonical, &parameter.type_.canonical_key());
        }
        let body = &test_bodies[id];
        append_statements(&mut canonical, &body.body);
    }
    append_collection_header(&mut canonical, 7, comptime_expressions.len());
    for (source, expression) in &comptime_expressions {
        append_range(&mut canonical, source);
        canonical.extend_from_slice(&expression.root.0.to_be_bytes());
        append_expression(&mut canonical, &expression.expression);
    }
    append_collection_header(&mut canonical, 8, input.variants.len());
    for (id, variant) in &input.variants {
        canonical.extend_from_slice(&id.owner.0.to_be_bytes());
        canonical.extend_from_slice(&id.definition.0.to_be_bytes());
        canonical.extend_from_slice(&id.variant.to_be_bytes());
        for parameter in &variant.parameters {
            append_part(&mut canonical, parameter.name.as_bytes());
            append_part(&mut canonical, &parameter.type_.canonical_key());
        }
    }
    append_collection_header(&mut canonical, 9, discharge_laws.laws.len());
    for (type_id, law) in &discharge_laws.laws {
        canonical.extend_from_slice(&type_id.0.to_be_bytes());
        append_discharge_law(&mut canonical, law);
    }
    append_collection_header(&mut canonical, 10, input.inferred_error_definitions.len());
    for definition in &input.inferred_error_definitions {
        canonical.extend_from_slice(&definition.0.to_be_bytes());
    }
    let artifact_catalog = ArtifactCatalog {
        templates: &functions,
        specialized: &specialized_functions,
        constants: &constants,
        specializations: &specializations,
        identities: identity_catalog,
        variants: &input.variants,
        structs: &input.structs,
        interfaces: &input.interfaces,
        discharge_laws: Some(&discharge_laws),
    };
    verify_lowered_artifact(&artifact_catalog, &test_bodies)?;
    Ok(VerifiedProgram {
        functions,
        specialized_functions,
        default_specializations,
        constants,
        tests: input.tests,
        _test_bodies: test_bodies,
        specializations,
        comptime_expressions,
        closures,
        inferred_error_definitions: input.inferred_error_definitions,
        _discharge_laws: discharge_laws,
        fingerprint: canonical.digest128(),
        identity_catalog_revision: identity_catalog.revision_fingerprint(),
        _verified: Verified,
    })
}

/// Lowers only the transitive pure program demanded by a compile-time
/// selection condition. Unrelated bodies remain outside semantic admission so
/// a declaration selected later cannot make the selector circular.
pub(crate) fn verify_comptime_condition(
    input: &ProgramInput,
    module: ModuleId,
    condition: &ExpressionSyntax,
    build_authority: &BuildAuthority,
    pool_authority: &PoolAuthority,
    identity_catalog: &mut IdentityCatalog,
    cancellation: &Cancellation,
) -> Result<(VerifiedProgram, Expression), VerificationFailure> {
    verify_comptime_condition_with_values(
        input,
        module,
        condition,
        build_authority,
        pool_authority,
        identity_catalog,
        cancellation,
        &BTreeMap::new(),
    )
}

#[allow(clippy::too_many_arguments)]
fn verify_comptime_condition_with_values(
    input: &ProgramInput,
    module: ModuleId,
    condition: &ExpressionSyntax,
    build_authority: &BuildAuthority,
    pool_authority: &PoolAuthority,
    identity_catalog: &mut IdentityCatalog,
    cancellation: &Cancellation,
    generic_const_values: &BTreeMap<String, (IntegerType, u64)>,
) -> Result<(VerifiedProgram, Expression), VerificationFailure> {
    validate_input(input)?;
    let mut discharge_laws = DischargeLawBuilder::default();
    intern_input_types(input, identity_catalog, &mut discharge_laws)?;
    let mut specializations = BTreeMap::new();
    let mut pending_specializations = BTreeSet::new();
    let mut lowerer = Lowerer::new(
        module,
        input,
        AuthorityContext::new(build_authority, pool_authority),
        identity_catalog,
        &mut discharge_laws,
        cancellation,
        SpecializationDemands {
            records: &mut specializations,
            pending: &mut pending_specializations,
        },
        true,
    );
    lowerer
        .generic_const_values
        .clone_from(generic_const_values);
    let expression = lowerer.expression_expected(condition, &Type::Bool)?;
    if expression.type_ != Type::Bool {
        return creator(
            CreatorFailureKind::IfConditionRequiresBool,
            &condition.range,
        );
    }

    let mut demanded_constants = BTreeSet::new();
    let mut demanded_functions = BTreeSet::new();
    collect_expression_demands(
        &expression,
        &mut demanded_constants,
        &mut demanded_functions,
    );
    let mut constants = BTreeMap::new();
    let mut functions = BTreeMap::new();
    let mut specialized_functions = BTreeMap::new();
    loop {
        if cancellation.is_cancelled() {
            return Err(VerificationFailure::Cancelled);
        }
        let pending_constants = demanded_constants
            .iter()
            .filter(|id| !constants.contains_key(*id))
            .copied()
            .collect::<Vec<_>>();
        for id in pending_constants {
            let constant = input
                .constants
                .get(&id)
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from("compile-time condition references an unknown constant"),
                })?;
            let value = Lowerer::new(
                constant.module,
                input,
                AuthorityContext::new(build_authority, pool_authority),
                identity_catalog,
                &mut discharge_laws,
                cancellation,
                SpecializationDemands {
                    records: &mut specializations,
                    pending: &mut pending_specializations,
                },
                true,
            )
            .expression_expected(&constant.value, &constant.type_)?;
            if !can_initialize(&value.type_, &constant.type_) {
                return creator(CreatorFailureKind::ConstantTypeMismatch, &constant.source);
            }
            collect_expression_demands(&value, &mut demanded_constants, &mut demanded_functions);
            constants.insert(
                id,
                HirConstant {
                    id,
                    module: constant.module,
                    name: constant.name.clone(),
                    type_: constant.type_.clone(),
                    expression: value,
                    source: constant.source.clone(),
                },
            );
        }

        let pending_functions = demanded_functions
            .iter()
            .filter(|id| {
                input.functions[*id].type_parameters.is_empty() && !functions.contains_key(*id)
            })
            .copied()
            .collect::<Vec<_>>();
        for id in pending_functions {
            let function = &input.functions[&id];
            let lowered = lower_concrete_function(
                function,
                input,
                AuthorityContext::new(build_authority, pool_authority),
                identity_catalog,
                &mut discharge_laws,
                cancellation,
                &mut specializations,
                &mut pending_specializations,
            )?;
            collect_statement_demands(
                &lowered.body,
                &mut demanded_constants,
                &mut demanded_functions,
            );
            functions.insert(id, Arc::new(lowered));
        }

        materialize_missing_specializations(
            input,
            AuthorityContext::new(build_authority, pool_authority),
            identity_catalog,
            &mut discharge_laws,
            cancellation,
            &functions,
            &mut SpecializationDemands {
                records: &mut specializations,
                pending: &mut pending_specializations,
            },
            &mut specialized_functions,
        )?;
        for function in specialized_functions.values() {
            collect_statement_demands(
                &function.body,
                &mut demanded_constants,
                &mut demanded_functions,
            );
        }
        let complete = demanded_constants
            .iter()
            .all(|id| constants.contains_key(id))
            && demanded_functions.iter().all(|id| {
                !input.functions[id].type_parameters.is_empty() || functions.contains_key(id)
            })
            && pending_specializations.is_empty();
        if complete {
            break;
        }
    }

    let default_specializations = specializations
        .values()
        .filter(|record| {
            input.functions[&record.definition]
                .type_parameters
                .is_empty()
        })
        .map(|record| (record.definition, record.id))
        .collect::<BTreeMap<_, _>>();
    let mut closures = BTreeMap::new();
    collect_expression_closures(&expression, &mut closures)?;
    for function in functions.values().chain(specialized_functions.values()) {
        collect_statement_closures(&function.body, &mut closures)?;
    }
    for constant in constants.values() {
        collect_expression_closures(&constant.expression, &mut closures)?;
    }
    let discharge_laws = construct_and_verify_discharge_laws(input, &discharge_laws)?;
    let program = VerifiedProgram {
        functions,
        specialized_functions,
        default_specializations,
        constants,
        tests: BTreeMap::new(),
        _test_bodies: BTreeMap::new(),
        specializations,
        comptime_expressions: BTreeMap::from([(
            condition.range.clone(),
            ComptimeExpression {
                root: condition_site_identity(module, &condition.range),
                expression: expression.clone(),
            },
        )]),
        closures,
        inferred_error_definitions: input.inferred_error_definitions.clone(),
        _discharge_laws: discharge_laws,
        fingerprint: 0,
        identity_catalog_revision: identity_catalog.revision_fingerprint(),
        _verified: Verified,
    };
    Ok((program, expression))
}

#[allow(clippy::too_many_arguments)]
fn lower_concrete_function(
    function: &ResolvedFunction,
    input: &ProgramInput,
    authorities: AuthorityContext<'_>,
    identity_catalog: &mut IdentityCatalog,
    discharge_laws: &mut DischargeLawBuilder,
    cancellation: &Cancellation,
    specializations: &mut BTreeMap<SpecializationId, SpecializationRecord>,
    pending_specializations: &mut BTreeSet<SpecializationId>,
) -> Result<HirFunction, VerificationFailure> {
    let mut lowerer = Lowerer::new(
        function.module,
        input,
        authorities,
        identity_catalog,
        discharge_laws,
        cancellation,
        SpecializationDemands {
            records: specializations,
            pending: pending_specializations,
        },
        true,
    );
    lowerer.set_generic_context(function, None);
    let mut parameters = Vec::new();
    let mut parameter_type_ids = Vec::new();
    for parameter in &function.parameters {
        let local = lowerer.bind_parameter(
            &parameter.name,
            parameter.type_.clone(),
            parameter.ownership,
        )?;
        parameters.push((
            local,
            parameter.type_.clone(),
            match parameter.ownership {
                OwnershipSyntax::Value => AccessMode::Copy,
                OwnershipSyntax::Read => AccessMode::Read,
                OwnershipSyntax::Mut => AccessMode::Mut,
                OwnershipSyntax::Take => AccessMode::Move,
            },
        ));
        parameter_type_ids.push(observe_type_tree(
            &parameter.type_,
            lowerer.identity_catalog,
            lowerer.discharge_laws,
        )?);
    }
    let body = lowerer.statements(&function.body, &function.return_type)?;
    if syntax_statements_fall_through(&function.body) {
        lowerer.check_recoverable_exit()?;
    }
    Ok(HirFunction {
        id: function.id,
        name: function.name.clone(),
        module_display: function.module_display.clone(),
        modifier: function.modifier,
        parameters,
        parameter_type_ids: parameter_type_ids.into(),
        parameter_definitions: function
            .parameters
            .iter()
            .map(|parameter| parameter.definition)
            .collect(),
        return_type: function.return_type.clone(),
        body: body.into(),
        source: function.source.clone(),
    })
}

fn collect_statement_demands(
    statements: &[Statement],
    constants: &mut BTreeSet<DefinitionId>,
    functions: &mut BTreeSet<DefinitionId>,
) {
    for statement in statements {
        match statement {
            Statement::Return { value, .. } => {
                if let Some(value) = value {
                    collect_expression_demands(value, constants, functions);
                }
            }
            Statement::Panic { value, .. }
            | Statement::Initialize { value, .. }
            | Statement::Assign { value, .. }
            | Statement::Evaluate(value) => {
                collect_expression_demands(value, constants, functions);
            }
            Statement::Defer { action, .. } => {
                collect_expression_demands(action.expression(), constants, functions);
                for capture in action.captures.iter() {
                    collect_expression_demands(&capture.expression, constants, functions);
                }
            }
            Statement::Assert { condition, .. } | Statement::Expect { condition, .. } => {
                collect_expression_demands(condition, constants, functions);
            }
            Statement::If {
                condition: value,
                then_branch,
                else_branch,
                ..
            }
            | Statement::IfPattern {
                value,
                then_branch,
                else_branch,
                ..
            } => {
                collect_expression_demands(value, constants, functions);
                collect_statement_demands(then_branch, constants, functions);
                collect_statement_demands(else_branch, constants, functions);
            }
            Statement::For { iterable, body, .. } => {
                collect_expression_demands(iterable, constants, functions);
                collect_statement_demands(body, constants, functions);
            }
            Statement::While {
                condition, body, ..
            } => {
                collect_expression_demands(condition, constants, functions);
                collect_statement_demands(body, constants, functions);
            }
            Statement::Match { value, cases, .. } => {
                collect_expression_demands(value, constants, functions);
                for case in cases.iter() {
                    collect_statement_demands(&case.body, constants, functions);
                }
            }
            Statement::WithPool { scope, body, .. } => {
                collect_expression_demands(scope, constants, functions);
                collect_statement_demands(body, constants, functions);
            }
            Statement::Break(_) | Statement::Continue(_) | Statement::Pass(_) => {}
        }
    }
}

fn collect_expression_demands(
    expression: &Expression,
    constants: &mut BTreeSet<DefinitionId>,
    functions: &mut BTreeSet<DefinitionId>,
) {
    match &expression.kind {
        ExpressionKind::Constant(id) => {
            constants.insert(*id);
        }
        ExpressionKind::FunctionValue { definition, .. } => {
            functions.insert(*definition);
        }
        ExpressionKind::Call { target, arguments } => {
            match target {
                CallTarget::Callable { value } => {
                    collect_expression_demands(value, constants, functions);
                }
                CallTarget::TemplateFunction { definition, .. }
                | CallTarget::Function { definition, .. } => {
                    functions.insert(*definition);
                }
                CallTarget::Interface { alternatives, .. } => {
                    functions.extend(alternatives.iter().map(|(_, definition, _)| *definition));
                }
                CallTarget::Build { .. }
                | CallTarget::BuiltinVariant(_)
                | CallTarget::UserVariant { .. }
                | CallTarget::Struct { .. }
                | CallTarget::Test { .. } => {}
            }
            for argument in arguments.iter() {
                collect_expression_demands(argument, constants, functions);
            }
        }
        ExpressionKind::Array(values) | ExpressionKind::Tuple(values) => {
            for value in values.iter() {
                collect_expression_demands(value, constants, functions);
            }
        }
        ExpressionKind::RepeatedArray { value, .. }
        | ExpressionKind::Positive(value)
        | ExpressionKind::Negate(value)
        | ExpressionKind::BitNot(value)
        | ExpressionKind::Not(value)
        | ExpressionKind::Await(value)
        | ExpressionKind::Propagate(value) => {
            collect_expression_demands(value, constants, functions);
        }
        ExpressionKind::Index { value, index }
        | ExpressionKind::Binary {
            left: value,
            right: index,
            ..
        } => {
            collect_expression_demands(value, constants, functions);
            collect_expression_demands(index, constants, functions);
        }
        ExpressionKind::Is { value, .. } => {
            collect_expression_demands(value, constants, functions);
        }
        ExpressionKind::Closure(closure) => {
            collect_expression_demands(&closure.body, constants, functions);
        }
        ExpressionKind::Literal(_)
        | ExpressionKind::Read(_)
        | ExpressionKind::CleanupCapture(_) => {}
    }
}

pub(crate) fn lower_functions_for_error_inference(
    input: &ProgramInput,
    build_authority: &BuildAuthority,
    pool_authority: &PoolAuthority,
    identity_catalog: &mut IdentityCatalog,
    cancellation: &Cancellation,
) -> Result<BTreeMap<DefinitionId, Arc<HirFunction>>, VerificationFailure> {
    validate_input(input)?;
    let mut discharge_laws = DischargeLawBuilder::default();
    intern_input_types(input, identity_catalog, &mut discharge_laws)?;
    let mut specializations = BTreeMap::new();
    let mut pending_specializations = BTreeSet::new();
    let mut functions = BTreeMap::new();
    for function in input.functions.values() {
        if cancellation.is_cancelled() {
            return Err(VerificationFailure::Cancelled);
        }
        let mut lowerer = Lowerer::new(
            function.module,
            input,
            AuthorityContext::new(build_authority, pool_authority),
            identity_catalog,
            &mut discharge_laws,
            cancellation,
            SpecializationDemands {
                records: &mut specializations,
                pending: &mut pending_specializations,
            },
            false,
        );
        lowerer.set_generic_context(function, None);
        let mut parameters = Vec::new();
        let mut parameter_type_ids = Vec::new();
        for parameter in &function.parameters {
            let local = lowerer.bind_parameter(
                &parameter.name,
                parameter.type_.clone(),
                parameter.ownership,
            )?;
            parameters.push((
                local,
                parameter.type_.clone(),
                match parameter.ownership {
                    OwnershipSyntax::Value => AccessMode::Copy,
                    OwnershipSyntax::Read => AccessMode::Read,
                    OwnershipSyntax::Mut => AccessMode::Mut,
                    OwnershipSyntax::Take => AccessMode::Move,
                },
            ));
            parameter_type_ids.push(observe_type_tree(
                &parameter.type_,
                lowerer.identity_catalog,
                lowerer.discharge_laws,
            )?);
        }
        let body = lowerer.statements(&function.body, &function.return_type)?;
        if syntax_statements_fall_through(&function.body) {
            lowerer.check_recoverable_exit()?;
        }
        functions.insert(
            function.id,
            Arc::new(HirFunction {
                id: function.id,
                name: function.name.clone(),
                module_display: function.module_display.clone(),
                modifier: function.modifier,
                parameters,
                parameter_type_ids: parameter_type_ids.into(),
                parameter_definitions: function
                    .parameters
                    .iter()
                    .map(|parameter| parameter.definition)
                    .collect(),
                return_type: function.return_type.clone(),
                body: body.into(),
                source: function.source.clone(),
            }),
        );
    }
    Ok(functions)
}

#[allow(clippy::too_many_arguments)]
fn materialize_missing_specializations(
    input: &ProgramInput,
    authorities: AuthorityContext<'_>,
    identity_catalog: &mut IdentityCatalog,
    discharge_laws: &mut DischargeLawBuilder,
    cancellation: &Cancellation,
    functions: &BTreeMap<DefinitionId, Arc<HirFunction>>,
    demands: &mut SpecializationDemands<'_>,
    specialized_functions: &mut BTreeMap<SpecializationId, Arc<HirFunction>>,
) -> Result<(), VerificationFailure> {
    while let Some(id) = demands.pending.pop_first() {
        if specialized_functions.contains_key(&id) {
            continue;
        }
        let record =
            demands
                .records
                .get(&id)
                .cloned()
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from("queued specialization has no demand record"),
                })?;
        if cancellation.is_cancelled() {
            return Err(VerificationFailure::Cancelled);
        }
        let function =
            input
                .functions
                .get(&record.definition)
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from("SpecializationId references an unknown function"),
                })?;
        if function.type_parameters.is_empty() {
            let concrete = functions.get(&record.definition).cloned().ok_or_else(|| {
                VerificationFailure::Defect {
                    evidence: Arc::from("non-generic specialization has no template body"),
                }
            })?;
            specialized_functions.insert(record.id, concrete);
            continue;
        }
        if function.type_parameters.len() != record.type_arguments.len()
            || record.type_arguments.iter().any(type_has_placeholder)
        {
            return defect("SpecializationId does not describe a fully concrete application");
        }
        let substitutions = function
            .type_parameters
            .iter()
            .copied()
            .zip(record.type_arguments.iter().cloned())
            .collect::<BTreeMap<_, _>>();
        let mut lowerer = Lowerer::new(
            function.module,
            input,
            authorities,
            identity_catalog,
            &mut *discharge_laws,
            cancellation,
            SpecializationDemands {
                records: &mut *demands.records,
                pending: &mut *demands.pending,
            },
            true,
        );
        lowerer.set_generic_context(function, Some(&substitutions));
        let mut parameters = Vec::new();
        let mut parameter_type_ids = Vec::new();
        for parameter in &function.parameters {
            let type_ = substitute(&parameter.type_, &substitutions);
            let local =
                lowerer.bind_parameter(&parameter.name, type_.clone(), parameter.ownership)?;
            parameters.push((
                local,
                type_.clone(),
                match parameter.ownership {
                    OwnershipSyntax::Value => AccessMode::Copy,
                    OwnershipSyntax::Read => AccessMode::Read,
                    OwnershipSyntax::Mut => AccessMode::Mut,
                    OwnershipSyntax::Take => AccessMode::Move,
                },
            ));
            parameter_type_ids.push(observe_type_tree(
                &type_,
                lowerer.identity_catalog,
                lowerer.discharge_laws,
            )?);
        }
        let return_type = substitute(&function.return_type, &substitutions);
        let body = lowerer.statements(&function.body, &return_type)?;
        if syntax_statements_fall_through(&function.body) {
            lowerer.check_recoverable_exit()?;
        }
        specialized_functions.insert(
            record.id,
            Arc::new(HirFunction {
                id: function.id,
                name: function.name.clone(),
                module_display: function.module_display.clone(),
                modifier: function.modifier,
                parameters,
                parameter_type_ids: parameter_type_ids.into(),
                parameter_definitions: function
                    .parameters
                    .iter()
                    .map(|parameter| parameter.definition)
                    .collect(),
                return_type,
                body: body.into(),
                source: function.source.clone(),
            }),
        );
    }
    Ok(())
}

fn append_collection_header(bytes: &mut impl ByteSink, tag: u8, length: usize) {
    bytes.push(tag);
    bytes.extend_from_slice(&u64::try_from(length).unwrap_or(u64::MAX).to_be_bytes());
}

fn append_argument_order(bytes: &mut impl ByteSink, order: &[u16]) {
    bytes.extend_from_slice(&u64::try_from(order.len()).unwrap_or(u64::MAX).to_be_bytes());
    for parameter in order {
        bytes.extend_from_slice(&parameter.to_be_bytes());
    }
}

fn append_discharge_law(bytes: &mut impl ByteSink, law: &DischargeLaw) {
    match law {
        DischargeLaw::Explicit => bytes.push(0),
        DischargeLaw::CompilerReclaim(AuthenticatedReclaim::PoolOwnership) => bytes.push(1),
        DischargeLaw::Structural(components) => {
            bytes.push(2);
            append_collection_header(bytes, 0, components.len());
            for component in &**components {
                match component.identity {
                    DischargeComponentIdentity::Field(definition) => {
                        bytes.push(0);
                        bytes.extend_from_slice(&definition.0.to_be_bytes());
                    }
                    DischargeComponentIdentity::Element => bytes.push(1),
                    DischargeComponentIdentity::Tuple(index) => {
                        bytes.push(2);
                        bytes.extend_from_slice(&index.to_be_bytes());
                    }
                    DischargeComponentIdentity::Success => bytes.push(3),
                    DischargeComponentIdentity::Error => bytes.push(4),
                    DischargeComponentIdentity::Present => bytes.push(5),
                }
                append_discharge_law(bytes, &component.law);
            }
        }
        DischargeLaw::ExistentialWitness(witnesses) => {
            bytes.push(3);
            append_collection_header(bytes, 0, witnesses.len());
            for witness in &**witnesses {
                bytes.extend_from_slice(&witness.witness.0.to_be_bytes());
                append_discharge_law(bytes, &witness.law);
            }
        }
    }
}

fn intern_input_types(
    input: &ProgramInput,
    identities: &mut IdentityCatalog,
    laws: &mut DischargeLawBuilder,
) -> Result<(), VerificationFailure> {
    let mut intern = |type_: &Type| observe_type_tree(type_, identities, laws).map(|_| ());
    for function in input.functions.values() {
        for parameter in &function.parameters {
            intern(&parameter.type_)?;
        }
        intern(&function.return_type)?;
    }
    for constant in input.constants.values() {
        intern(&constant.type_)?;
    }
    for test in input.tests.values() {
        for parameter in &test.parameters {
            intern(&parameter.type_)?;
        }
    }
    for struct_ in input.structs.values() {
        for field in &struct_.fields {
            intern(&field.type_)?;
        }
        if struct_.type_parameters.is_empty() {
            intern(&Type::Nominal {
                definition: struct_.definition,
                display: struct_.display.clone(),
                arguments: Arc::from([]),
            })?;
        }
    }
    for variant in input.variants.values() {
        for parameter in &variant.parameters {
            intern(&parameter.type_)?;
        }
    }
    for type_ in input.aliases.values() {
        intern(type_)?;
    }
    Ok(())
}

fn observe_type_tree(
    type_: &Type,
    identities: &mut IdentityCatalog,
    laws: &mut DischargeLawBuilder,
) -> Result<TypeId, VerificationFailure> {
    match type_ {
        Type::Array(element)
        | Type::FixedArray { element, .. }
        | Type::Own { value: element, .. }
        | Type::Option(element) => {
            observe_type_tree(element, identities, laws)?;
        }
        Type::Tuple(members) => {
            for member in &**members {
                observe_type_tree(member, identities, laws)?;
            }
        }
        Type::Function {
            parameters,
            return_type,
        } => {
            for parameter in &**parameters {
                observe_type_tree(parameter, identities, laws)?;
            }
            observe_type_tree(return_type, identities, laws)?;
        }
        Type::Result { success, error } => {
            observe_type_tree(success, identities, laws)?;
            if let Some(error) = error {
                observe_type_tree(error, identities, laws)?;
            }
        }
        Type::Nominal { arguments, .. } => {
            for argument in &**arguments {
                observe_type_tree(argument, identities, laws)?;
            }
        }
        Type::Unit
        | Type::Bool
        | Type::Integer(_)
        | Type::Float(_)
        | Type::Text
        | Type::Scalar
        | Type::Bytes
        | Type::ConstU64(_)
        | Type::PoolArgument(_)
        | Type::Any { .. }
        | Type::Builtin(_)
        | Type::Parameter { .. }
        | Type::Infer => {}
    }
    let type_id =
        identities
            .intern_type(type_)
            .map_err(|collision| VerificationFailure::Defect {
                evidence: Arc::from(format!("type identity collision {:032x}", collision.digest)),
            })?;
    laws.observe(type_id, type_)?;
    Ok(type_id)
}

fn construct_discharge_laws(
    observed: &BTreeMap<TypeId, Type>,
    authenticated_reclaims: &BTreeSet<TypeId>,
    input: &ProgramInput,
) -> Vec<RawDischargeLaw> {
    observed
        .iter()
        .filter_map(|(type_id, type_)| {
            authenticated_reclaims
                .contains(type_id)
                .then_some(DischargeLaw::CompilerReclaim(
                    AuthenticatedReclaim::PoolOwnership,
                ))
                .or_else(|| {
                    derive_discharge_law(
                        type_,
                        &input.structs,
                        &input.interfaces,
                        &input.nominal_displays,
                    )
                })
                .map(|law| RawDischargeLaw {
                    type_id: *type_id,
                    type_: type_.clone(),
                    law,
                })
        })
        .collect()
}

/// Independently reconstructs the law shape while auditing a frozen artifact.
/// This intentionally does not share construction's `derive_discharge_law`
/// traversal: a construction bug must not be able to certify itself.
fn audit_expected_discharge_law(
    type_: &Type,
    structs: &BTreeMap<DefinitionId, ResolvedStruct>,
    interfaces: &BTreeMap<DefinitionId, ResolvedInterface>,
    nominal_displays: &BTreeMap<DefinitionId, Arc<str>>,
) -> Option<DischargeLaw> {
    match type_ {
        Type::Own { .. } => Some(DischargeLaw::CompilerReclaim(
            AuthenticatedReclaim::PoolOwnership,
        )),
        Type::Nominal {
            definition,
            arguments,
            ..
        } => {
            let struct_ = structs.get(definition)?;
            if !struct_.resource {
                for argument in arguments.iter() {
                    if let Some(law) = audit_expected_discharge_law(
                        argument,
                        structs,
                        interfaces,
                        nominal_displays,
                    ) {
                        return Some(law);
                    }
                }
                return None;
            }
            let substitutions = struct_
                .type_parameters
                .iter()
                .copied()
                .zip(arguments.iter().cloned())
                .collect::<BTreeMap<_, _>>();
            let fields = if struct_.field_selections.is_empty()
                || arguments.iter().any(type_has_placeholder)
            {
                Arc::from(struct_.fields.clone())
            } else {
                struct_
                    .applied_fields
                    .borrow()
                    .get(arguments)
                    .cloned()
                    .unwrap_or_else(|| Arc::from(struct_.fields.clone()))
            };
            let mut components = Vec::new();
            for field in fields.iter() {
                let field_type = substitute(&field.type_, &substitutions);
                if let Some(law) =
                    audit_expected_discharge_law(&field_type, structs, interfaces, nominal_displays)
                {
                    components.push(DischargeComponent {
                        identity: DischargeComponentIdentity::Field(field.definition),
                        law,
                    });
                }
            }
            Some(if components.is_empty() {
                DischargeLaw::Explicit
            } else {
                DischargeLaw::Structural(components.into())
            })
        }
        Type::Array(element) | Type::FixedArray { element, .. } => {
            let law = audit_expected_discharge_law(element, structs, interfaces, nominal_displays)?;
            Some(DischargeLaw::Structural(Arc::from([DischargeComponent {
                identity: DischargeComponentIdentity::Element,
                law,
            }])))
        }
        Type::Tuple(members) => {
            let mut components = Vec::new();
            for (index, member) in members.iter().enumerate() {
                if let Some(law) =
                    audit_expected_discharge_law(member, structs, interfaces, nominal_displays)
                {
                    components.push(DischargeComponent {
                        identity: DischargeComponentIdentity::Tuple(
                            u32::try_from(index).unwrap_or(u32::MAX),
                        ),
                        law,
                    });
                }
            }
            (!components.is_empty()).then(|| DischargeLaw::Structural(components.into()))
        }
        Type::Result { success, error } => {
            let mut components = Vec::new();
            if let Some(law) =
                audit_expected_discharge_law(success, structs, interfaces, nominal_displays)
            {
                components.push(DischargeComponent {
                    identity: DischargeComponentIdentity::Success,
                    law,
                });
            }
            if let Some(error) = error
                && let Some(law) =
                    audit_expected_discharge_law(error, structs, interfaces, nominal_displays)
            {
                components.push(DischargeComponent {
                    identity: DischargeComponentIdentity::Error,
                    law,
                });
            }
            (!components.is_empty()).then(|| DischargeLaw::Structural(components.into()))
        }
        Type::Option(value) => {
            audit_expected_discharge_law(value, structs, interfaces, nominal_displays).map(|law| {
                DischargeLaw::Structural(Arc::from([DischargeComponent {
                    identity: DischargeComponentIdentity::Present,
                    law,
                }]))
            })
        }
        Type::Any { interface, .. } => {
            let mut witnesses = Vec::new();
            for definition in interfaces.get(interface)?.implementations.keys() {
                let display = nominal_displays.get(definition)?.clone();
                let witness_type = Type::Nominal {
                    definition: *definition,
                    display,
                    arguments: Arc::from([]),
                };
                if let Some(law) = audit_expected_discharge_law(
                    &witness_type,
                    structs,
                    interfaces,
                    nominal_displays,
                ) {
                    witnesses.push(WitnessDischarge {
                        witness: *definition,
                        law,
                    });
                }
            }
            (!witnesses.is_empty()).then(|| DischargeLaw::ExistentialWitness(witnesses.into()))
        }
        Type::Unit
        | Type::Bool
        | Type::Integer(_)
        | Type::Float(_)
        | Type::Text
        | Type::Scalar
        | Type::Bytes
        | Type::ConstU64(_)
        | Type::PoolArgument(_)
        | Type::Function { .. }
        | Type::Builtin(_)
        | Type::Parameter { .. }
        | Type::Infer => None,
    }
}

fn verify_discharge_laws(
    observed: &BTreeMap<TypeId, Type>,
    claimed: &[RawDischargeLaw],
    authenticated_reclaims: &BTreeSet<TypeId>,
    structs: &BTreeMap<DefinitionId, ResolvedStruct>,
    interfaces: &BTreeMap<DefinitionId, ResolvedInterface>,
    nominal_displays: &BTreeMap<DefinitionId, Arc<str>>,
) -> Result<VerifiedDischargeLaws, VerificationFailure> {
    let mut laws = BTreeMap::new();
    for claim in claimed {
        if observed.get(&claim.type_id) != Some(&claim.type_) {
            return defect("discharge law names an unobserved or mismatched TypeId");
        }
        let structural_law =
            audit_expected_discharge_law(&claim.type_, structs, interfaces, nominal_displays);
        if authenticated_reclaims.contains(&claim.type_id) && structural_law.is_none() {
            return defect("authenticated reclaim attached to a Data TypeId");
        }
        let expected = authenticated_reclaims
            .contains(&claim.type_id)
            .then_some(DischargeLaw::CompilerReclaim(
                AuthenticatedReclaim::PoolOwnership,
            ))
            .or(structural_law);
        let Some(expected) = expected else {
            return defect("discharge law attached to a Data TypeId");
        };
        if claim.law != expected {
            return defect("discharge law contradicts its Resource Type structure");
        }
        if laws.insert(claim.type_id, claim.law.clone()).is_some() {
            return defect("Resource TypeId has more than one Discharge Law");
        }
    }
    for (type_id, type_) in observed {
        if (authenticated_reclaims.contains(type_id)
            || audit_expected_discharge_law(type_, structs, interfaces, nominal_displays).is_some())
            && !laws.contains_key(type_id)
        {
            return defect("Resource TypeId has no Discharge Law");
        }
    }
    Ok(VerifiedDischargeLaws {
        laws,
        _verified: Verified,
    })
}

fn construct_and_verify_discharge_laws(
    input: &ProgramInput,
    builder: &DischargeLawBuilder,
) -> Result<VerifiedDischargeLaws, VerificationFailure> {
    let claimed =
        construct_discharge_laws(&builder.observed, &builder.authenticated_reclaims, input);
    verify_discharge_laws(
        &builder.observed,
        &claimed,
        &builder.authenticated_reclaims,
        &input.structs,
        &input.interfaces,
        &input.nominal_displays,
    )
}

struct ArtifactCatalog<'a> {
    templates: &'a BTreeMap<DefinitionId, Arc<HirFunction>>,
    specialized: &'a BTreeMap<SpecializationId, Arc<HirFunction>>,
    constants: &'a BTreeMap<DefinitionId, HirConstant>,
    specializations: &'a BTreeMap<SpecializationId, SpecializationRecord>,
    identities: &'a IdentityCatalog,
    variants: &'a BTreeMap<VariantId, ResolvedVariant>,
    structs: &'a BTreeMap<DefinitionId, ResolvedStruct>,
    interfaces: &'a BTreeMap<DefinitionId, ResolvedInterface>,
    discharge_laws: Option<&'a VerifiedDischargeLaws>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ArtifactLocalType {
    type_id: TypeId,
    type_: Type,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ArtifactParameterCustody {
    discharge_boundaries: BTreeSet<LocalId>,
    non_custodial_loans: BTreeSet<LocalId>,
    invalid_value_parameters: BTreeSet<LocalId>,
}

impl ArtifactParameterCustody {
    fn from_parameters(parameters: &[(LocalId, Type, AccessMode)]) -> Self {
        let mut roles = Self::default();
        for (local, _, access) in parameters {
            match access {
                AccessMode::Move => {
                    roles.discharge_boundaries.insert(*local);
                }
                AccessMode::Read | AccessMode::Mut => {
                    roles.non_custodial_loans.insert(*local);
                }
                AccessMode::Copy => {
                    // Resource-valued `Value` parameters are rejected at the source
                    // seam; keep their recovery artifact non-custodial so that the
                    // independent verifier does not replace that Creator diagnostic.
                    roles.invalid_value_parameters.insert(*local);
                }
            }
        }
        roles
    }

    fn scope_exclusions(&self) -> BTreeSet<LocalId> {
        self.discharge_boundaries
            .union(&self.non_custodial_loans)
            .copied()
            .chain(self.invalid_value_parameters.iter().copied())
            .collect()
    }
}

fn artifact_struct_fields(
    struct_: &ResolvedStruct,
    arguments: &[Type],
) -> Result<Arc<[ResolvedField]>, VerificationFailure> {
    if struct_.field_selections.is_empty() || arguments.iter().any(type_has_placeholder) {
        return Ok(struct_.fields.clone().into());
    }
    struct_
        .applied_fields
        .borrow()
        .get(arguments)
        .cloned()
        .ok_or_else(|| VerificationFailure::Defect {
            evidence: Arc::from("applied nominal has no selected member table"),
        })
}

fn artifact_place_type(
    place: &Place,
    locals: &BTreeMap<LocalId, ArtifactLocalType>,
) -> Option<Type> {
    place
        .projections
        .last()
        .map(|projection| match projection {
            PlaceProjection::Field { type_, .. } | PlaceProjection::Index { type_, .. } => {
                type_.clone()
            }
        })
        .or_else(|| locals.get(&place.local).map(|local| local.type_.clone()))
}

fn restore_artifact_place_custody(
    moved: &mut MoveState,
    place: &Place,
    locals: &BTreeMap<LocalId, ArtifactLocalType>,
    catalog: &ArtifactCatalog<'_>,
) -> Result<(), VerificationFailure> {
    moved.restore_place(place);
    for projection_count in (0..place.projections.len()).rev() {
        let aggregate = Place {
            local: place.local,
            projections: place.projections[..projection_count].into(),
        };
        let Some(Type::Nominal {
            definition,
            arguments,
            ..
        }) = artifact_place_type(&aggregate, locals)
        else {
            continue;
        };
        let struct_ =
            catalog
                .structs
                .get(&definition)
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from("lowered place names an unknown aggregate type"),
                })?;
        let substitutions = struct_
            .type_parameters
            .iter()
            .copied()
            .zip(arguments.iter().cloned())
            .collect::<BTreeMap<_, _>>();
        let fields = artifact_struct_fields(struct_, &arguments)?;
        let children = fields
            .iter()
            .map(|field| {
                let mut projections = aggregate.projections.to_vec();
                projections.push(PlaceProjection::Field {
                    definition,
                    name: Arc::from(field.name.as_str()),
                    type_: substitute(&field.type_, &substitutions),
                    mutable: field.mutable,
                });
                Place {
                    local: aggregate.local,
                    projections: projections.into(),
                }
            })
            .collect::<Vec<_>>();
        moved.finish_aggregate_restoration(&aggregate, &children);
    }
    Ok(())
}

fn type_has_placeholder(type_: &Type) -> bool {
    match type_ {
        Type::Parameter { .. } | Type::Infer => true,
        Type::Array(element)
        | Type::FixedArray { element, .. }
        | Type::Own { value: element, .. }
        | Type::Option(element) => type_has_placeholder(element),
        Type::Function {
            parameters,
            return_type,
        } => parameters.iter().any(type_has_placeholder) || type_has_placeholder(return_type),
        Type::Tuple(members) => members.iter().any(type_has_placeholder),
        Type::Result { success, error } => {
            type_has_placeholder(success)
                || error
                    .as_ref()
                    .is_some_and(|error| type_has_placeholder(error))
        }
        Type::Nominal { arguments, .. } => arguments.iter().any(type_has_placeholder),
        Type::Unit
        | Type::Bool
        | Type::Integer(_)
        | Type::Float(_)
        | Type::Text
        | Type::Scalar
        | Type::Bytes
        | Type::ConstU64(_)
        | Type::PoolArgument(_)
        | Type::Builtin(_)
        | Type::Any { .. } => false,
    }
}

fn verify_specialized_artifact(
    specialized: &BTreeMap<SpecializationId, Arc<HirFunction>>,
    catalog: &ArtifactCatalog<'_>,
) -> Result<(), VerificationFailure> {
    for (id, function) in specialized {
        let record =
            catalog
                .specializations
                .get(id)
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from("concrete body has no Specialization record"),
                })?;
        let template = catalog.templates.get(&record.definition).ok_or_else(|| {
            VerificationFailure::Defect {
                evidence: Arc::from("concrete body has no definition template"),
            }
        })?;
        if record.definition != function.id
            || function.parameters.len() != function.parameter_definitions.len()
            || function.parameter_definitions != template.parameter_definitions
            || function
                .parameters
                .iter()
                .any(|(_, type_, _)| type_has_placeholder(type_))
            || type_has_placeholder(&function.return_type)
        {
            return defect("concrete Specialization body contains template facts");
        }
        if function.parameters.len() != function.parameter_type_ids.len() {
            return defect("concrete Specialization parameter identities are malformed");
        }
        let mut locals = function
            .parameters
            .iter()
            .zip(function.parameter_type_ids.iter())
            .map(|((local, type_, _), type_id)| {
                (
                    *local,
                    ArtifactLocalType {
                        type_id: *type_id,
                        type_: type_.clone(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let discharge_exempt =
            ArtifactParameterCustody::from_parameters(&function.parameters).scope_exclusions();
        let mut moved = MoveState::default();
        let mut previous_source_start = function.source.start();
        verify_loop_control(&function.body, 0)?;
        verify_statements(
            &function.body,
            &function.return_type,
            &mut locals,
            &mut moved,
            &mut Vec::new(),
            &ArtifactVerificationAuthority {
                catalog,
                provenance_owner: &function.source,
                discharge_exempt: &discharge_exempt,
            },
            &mut previous_source_start,
        )?;
        if verified_statements_fall_through(&function.body) {
            verify_artifact_scope_discharged(&locals, &moved, &discharge_exempt, catalog)?;
        }
        if statements_have_placeholder(&function.body) {
            return defect("concrete Specialization operation contains a placeholder type");
        }
    }
    Ok(())
}

fn statements_have_placeholder(statements: &[Statement]) -> bool {
    fn expression_has_placeholder(expression: &Expression) -> bool {
        if type_has_placeholder(&expression.type_) {
            return true;
        }
        let mut found = false;
        expression.visit_children(&mut |child| found |= expression_has_placeholder(child));
        found
    }
    statements.iter().any(|statement| match statement {
        Statement::Return { value, .. } => value.as_ref().is_some_and(expression_has_placeholder),
        Statement::Panic { value, .. }
        | Statement::Assert {
            condition: value, ..
        }
        | Statement::Expect {
            condition: value, ..
        }
        | Statement::Initialize { value, .. }
        | Statement::Assign { value, .. }
        | Statement::Evaluate(value) => expression_has_placeholder(value),
        Statement::Defer { action, .. } => {
            let mut found = false;
            action.visit_expressions(&mut |expression| {
                found |= expression_has_placeholder(expression);
            });
            found
        }
        Statement::If {
            condition: value,
            then_branch,
            else_branch,
            ..
        }
        | Statement::IfPattern {
            value,
            then_branch,
            else_branch,
            ..
        } => {
            expression_has_placeholder(value)
                || statements_have_placeholder(then_branch)
                || statements_have_placeholder(else_branch)
        }
        Statement::For { iterable, body, .. } => {
            expression_has_placeholder(iterable) || statements_have_placeholder(body)
        }
        Statement::While {
            condition, body, ..
        } => expression_has_placeholder(condition) || statements_have_placeholder(body),
        Statement::Break(_) | Statement::Continue(_) => false,
        Statement::Match { value, cases, .. } => {
            expression_has_placeholder(value)
                || cases
                    .iter()
                    .any(|case| statements_have_placeholder(&case.body))
        }
        Statement::WithPool { scope, body, .. } => {
            expression_has_placeholder(scope) || statements_have_placeholder(body)
        }
        Statement::Pass(_) => false,
    })
}

fn collect_statement_closures(
    statements: &[Statement],
    closures: &mut BTreeMap<ClosureId, Arc<HirClosure>>,
) -> Result<(), VerificationFailure> {
    for statement in statements {
        match statement {
            Statement::Return { value, .. } => {
                if let Some(value) = value {
                    collect_expression_closures(value, closures)?;
                }
            }
            Statement::Panic { value, .. }
            | Statement::Assert {
                condition: value, ..
            }
            | Statement::Expect {
                condition: value, ..
            }
            | Statement::Initialize { value, .. }
            | Statement::Assign { value, .. }
            | Statement::Evaluate(value) => collect_expression_closures(value, closures)?,
            Statement::Defer { action, .. } => {
                let mut result = Ok(());
                action.visit_expressions(&mut |expression| {
                    if result.is_ok() {
                        result = collect_expression_closures(expression, closures);
                    }
                });
                result?
            }
            Statement::If {
                condition: value,
                then_branch,
                else_branch,
                ..
            }
            | Statement::IfPattern {
                value,
                then_branch,
                else_branch,
                ..
            } => {
                collect_expression_closures(value, closures)?;
                collect_statement_closures(then_branch, closures)?;
                collect_statement_closures(else_branch, closures)?;
            }
            Statement::For { iterable, body, .. } => {
                collect_expression_closures(iterable, closures)?;
                collect_statement_closures(body, closures)?;
            }
            Statement::While {
                condition, body, ..
            } => {
                collect_expression_closures(condition, closures)?;
                collect_statement_closures(body, closures)?;
            }
            Statement::Match { value, cases, .. } => {
                collect_expression_closures(value, closures)?;
                for case in &**cases {
                    collect_statement_closures(&case.body, closures)?;
                }
            }
            Statement::WithPool { scope, body, .. } => {
                collect_expression_closures(scope, closures)?;
                collect_statement_closures(body, closures)?;
            }
            Statement::Break(_) | Statement::Continue(_) | Statement::Pass(_) => {}
        }
    }
    Ok(())
}

fn collect_expression_closures(
    expression: &Expression,
    closures: &mut BTreeMap<ClosureId, Arc<HirClosure>>,
) -> Result<(), VerificationFailure> {
    if let ExpressionKind::Closure(closure) = &expression.kind {
        if let Some(previous) = closures.insert(closure.id, Arc::clone(closure))
            && previous.identity_key != closure.identity_key
        {
            return defect("closure identity digest collision");
        }
        collect_expression_closures(&closure.body, closures)?;
        return Ok(());
    }
    let mut result = Ok(());
    expression.visit_children(&mut |child| {
        if result.is_ok() {
            result = collect_expression_closures(child, closures);
        }
    });
    result
}

fn verify_loop_control(
    statements: &[Statement],
    loop_depth: usize,
) -> Result<(), VerificationFailure> {
    for statement in statements {
        match statement {
            Statement::Break(_) | Statement::Continue(_) if loop_depth == 0 => {
                return defect("lowered loop control is outside a loop");
            }
            Statement::If {
                then_branch,
                else_branch,
                ..
            }
            | Statement::IfPattern {
                then_branch,
                else_branch,
                ..
            } => {
                verify_loop_control(then_branch, loop_depth)?;
                verify_loop_control(else_branch, loop_depth)?;
            }
            Statement::For { body, .. } | Statement::While { body, .. } => {
                verify_loop_control(body, loop_depth.saturating_add(1))?;
            }
            Statement::Match { cases, .. } => {
                for case in &**cases {
                    verify_loop_control(&case.body, loop_depth)?;
                }
            }
            Statement::WithPool { body, .. } => verify_loop_control(body, loop_depth)?,
            Statement::Return { .. }
            | Statement::Panic { .. }
            | Statement::Assert { .. }
            | Statement::Expect { .. }
            | Statement::Initialize { .. }
            | Statement::Assign { .. }
            | Statement::Evaluate(_)
            | Statement::Defer { .. }
            | Statement::Break(_)
            | Statement::Continue(_)
            | Statement::Pass(_) => {}
        }
    }
    Ok(())
}

fn statements_suspend(statements: &[Statement]) -> bool {
    fn expression_suspends(expression: &Expression) -> bool {
        if matches!(expression.kind, ExpressionKind::Await(_)) {
            return true;
        }
        let mut found = false;
        expression.visit_children(&mut |child| found |= expression_suspends(child));
        found
    }
    statements.iter().any(|statement| match statement {
        Statement::Return { value, .. } => value.as_ref().is_some_and(expression_suspends),
        Statement::Panic { value, .. }
        | Statement::Assert {
            condition: value, ..
        }
        | Statement::Expect {
            condition: value, ..
        }
        | Statement::Initialize { value, .. }
        | Statement::Assign { value, .. }
        | Statement::Evaluate(value) => expression_suspends(value),
        Statement::Defer { action, .. } => {
            let mut suspends = false;
            action.visit_expressions(&mut |expression| {
                suspends |= expression_suspends(expression);
            });
            suspends
        }
        Statement::If {
            condition: value,
            then_branch,
            else_branch,
            ..
        }
        | Statement::IfPattern {
            value,
            then_branch,
            else_branch,
            ..
        } => {
            expression_suspends(value)
                || statements_suspend(then_branch)
                || statements_suspend(else_branch)
        }
        Statement::For { iterable, body, .. } => {
            expression_suspends(iterable) || statements_suspend(body)
        }
        Statement::While {
            condition, body, ..
        } => expression_suspends(condition) || statements_suspend(body),
        Statement::Break(_) | Statement::Continue(_) => false,
        Statement::Match { value, cases, .. } => {
            expression_suspends(value) || cases.iter().any(|case| statements_suspend(&case.body))
        }
        Statement::WithPool { scope, body, .. } => {
            expression_suspends(scope) || statements_suspend(body)
        }
        Statement::Pass(_) => false,
    })
}

fn verify_lowered_artifact(
    catalog: &ArtifactCatalog<'_>,
    tests: &BTreeMap<TestId, HirTest>,
) -> Result<(), VerificationFailure> {
    for (key, function) in catalog.templates {
        if key != &function.id {
            return defect("lowered function key disagrees with its DefinitionId");
        }
        if function.parameters.len() != function.parameter_definitions.len()
            || function.parameters.len() != function.parameter_type_ids.len()
            || function
                .parameter_definitions
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != function.parameter_definitions.len()
        {
            return defect("lowered function parameter identities are malformed");
        }
        let mut locals = BTreeMap::new();
        for ((local, type_, _), type_id) in function
            .parameters
            .iter()
            .zip(function.parameter_type_ids.iter())
        {
            if !catalog.identities.type_matches(*type_id, type_)
                || locals
                    .insert(
                        *local,
                        ArtifactLocalType {
                            type_id: *type_id,
                            type_: type_.clone(),
                        },
                    )
                    .is_some()
            {
                return defect("lowered function repeats a parameter LocalId");
            }
        }
        let discharge_exempt =
            ArtifactParameterCustody::from_parameters(&function.parameters).scope_exclusions();
        let mut moved = MoveState::default();
        let mut previous_source_start = function.source.start();
        verify_loop_control(&function.body, 0)?;
        verify_statements(
            &function.body,
            &function.return_type,
            &mut locals,
            &mut moved,
            &mut Vec::new(),
            &ArtifactVerificationAuthority {
                catalog,
                provenance_owner: &function.source,
                discharge_exempt: &discharge_exempt,
            },
            &mut previous_source_start,
        )?;
        if verified_statements_fall_through(&function.body) {
            verify_artifact_scope_discharged(&locals, &moved, &discharge_exempt, catalog)?;
        }
    }
    for (key, constant) in catalog.constants {
        if key != &constant.id {
            return defect("lowered constant key disagrees with its DefinitionId");
        }
        let actual = verify_expression_artifact(
            &constant.expression,
            &BTreeMap::new(),
            &mut MoveState::default(),
            catalog,
            &constant.source,
        )?;
        if !can_initialize(&actual, &constant.type_) {
            return defect("lowered constant expression disagrees with its resolved type");
        }
    }
    for test in tests.values() {
        if test.parameters.len() != test.parameter_type_ids.len() {
            return defect("lowered Test parameter identities are malformed");
        }
        let mut locals = test
            .parameters
            .iter()
            .zip(test.parameter_type_ids.iter())
            .map(|((local, type_, _), type_id)| {
                (
                    *local,
                    ArtifactLocalType {
                        type_id: *type_id,
                        type_: type_.clone(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        if test
            .parameters
            .iter()
            .zip(test.parameter_type_ids.iter())
            .any(|((_, type_, _), type_id)| !catalog.identities.type_matches(*type_id, type_))
        {
            return defect("lowered Test parameter TypeId disagrees with its canonical type");
        }
        let discharge_exempt =
            ArtifactParameterCustody::from_parameters(&test.parameters).scope_exclusions();
        let mut moved = MoveState::default();
        let mut previous_source_start = test.source.start();
        verify_loop_control(&test.body, 0)?;
        verify_statements(
            &test.body,
            &Type::Unit,
            &mut locals,
            &mut moved,
            &mut Vec::new(),
            &ArtifactVerificationAuthority {
                catalog,
                provenance_owner: &test.source,
                discharge_exempt: &discharge_exempt,
            },
            &mut previous_source_start,
        )?;
        if verified_statements_fall_through(&test.body) {
            verify_artifact_scope_discharged(&locals, &moved, &discharge_exempt, catalog)?;
        }
        if statements_have_placeholder(&test.body) {
            return defect("verified Test body contains a placeholder type");
        }
    }
    verify_specialized_artifact(catalog.specialized, catalog)
}

struct ArtifactVerificationAuthority<'a, 'catalog> {
    catalog: &'a ArtifactCatalog<'catalog>,
    provenance_owner: &'a SourceRange,
    discharge_exempt: &'a BTreeSet<LocalId>,
}

fn artifact_expression_contains_resource_move(
    expression: &Expression,
    catalog: &ArtifactCatalog<'_>,
) -> bool {
    if matches!(expression.kind, ExpressionKind::CleanupCapture(_)) {
        return false;
    }
    if expression.access == AccessMode::Move
        && artifact_type_owns_resource(expression.type_id, &expression.type_, catalog)
    {
        return true;
    }
    let mut found = false;
    expression.visit_children(&mut |child| {
        found |= artifact_expression_contains_resource_move(child, catalog);
    });
    found
}

fn artifact_expression_contains_resource_loan(
    expression: &Expression,
    catalog: &ArtifactCatalog<'_>,
) -> bool {
    if matches!(expression.access, AccessMode::Read | AccessMode::Mut)
        && artifact_type_owns_resource(expression.type_id, &expression.type_, catalog)
    {
        return true;
    }
    if matches!(expression.kind, ExpressionKind::Closure(_)) {
        return false;
    }
    let mut found = false;
    expression.visit_children(&mut |child| {
        found |= artifact_expression_contains_resource_loan(child, catalog);
    });
    found
}

fn collect_cleanup_capture_references(expression: &Expression, references: &mut Vec<Expression>) {
    if matches!(expression.kind, ExpressionKind::CleanupCapture(_)) {
        references.push(expression.clone());
        return;
    }
    if matches!(expression.kind, ExpressionKind::Closure(_)) {
        return;
    }
    expression.visit_children(&mut |child| {
        collect_cleanup_capture_references(child, references);
    });
}

fn cleanup_has_conditional_capture_reference(expression: &Expression) -> bool {
    if let ExpressionKind::Binary {
        operator: BinaryOperator::And | BinaryOperator::Or,
        left,
        right,
    } = &expression.kind
    {
        let mut right_references = Vec::new();
        collect_cleanup_capture_references(right, &mut right_references);
        return !right_references.is_empty()
            || cleanup_has_conditional_capture_reference(left)
            || cleanup_has_conditional_capture_reference(right);
    }
    if matches!(expression.kind, ExpressionKind::Closure(_)) {
        return false;
    }
    let mut found = false;
    expression.visit_children(&mut |child| {
        found |= cleanup_has_conditional_capture_reference(child);
    });
    found
}

fn verify_statements(
    statements: &[Statement],
    return_type: &Type,
    locals: &mut BTreeMap<LocalId, ArtifactLocalType>,
    moved: &mut MoveState,
    loop_custody: &mut Vec<LoopCustodyEdges>,
    authority: &ArtifactVerificationAuthority<'_, '_>,
    previous_source_start: &mut u64,
) -> Result<(), VerificationFailure> {
    let catalog = authority.catalog;
    let provenance_owner = authority.provenance_owner;
    let mut reachable = true;
    for statement in statements {
        let unreachable_state = (!reachable).then(|| (moved.clone(), loop_custody.clone()));
        let source = statement_source(statement);
        if source.path() != provenance_owner.path()
            || source.start() > source.end()
            || source.start() < provenance_owner.start()
            || source.end() > provenance_owner.end()
            || source.start() < *previous_source_start
        {
            return defect("lowered statement provenance is outside source order or ownership");
        }
        *previous_source_start = source.start();
        match statement {
            Statement::Return { value, source: _ } => {
                let actual = if let Some(value) = value {
                    let actual = verify_expression_artifact(value, locals, moved, catalog, source)?;
                    if artifact_type_owns_resource(value.type_id, &actual, catalog)
                        && root_place(value).is_some()
                        && value.access != AccessMode::Move
                    {
                        return defect("lowered Resource return omits an explicit move");
                    }
                    actual
                } else {
                    Type::Unit
                };
                if !can_return(&actual, return_type) {
                    return defect("lowered return expression disagrees with function type");
                }
                verify_artifact_scope_discharged(
                    locals,
                    moved,
                    authority.discharge_exempt,
                    catalog,
                )?;
            }
            Statement::Panic { value, .. } | Statement::Evaluate(value) => {
                verify_expression_artifact(value, locals, moved, catalog, source)?;
            }
            Statement::Defer { action, .. } => {
                let expression = action.expression();
                let mut captures = BTreeMap::new();
                for (expected, capture) in action.captures.iter().enumerate() {
                    if usize::try_from(capture.ordinal.0).ok() != Some(expected) {
                        return defect("lowered cleanup capture ordinals are not canonical");
                    }
                    if captures.insert(capture.ordinal, capture).is_some() {
                        return defect("lowered cleanup capture ordinal appears more than once");
                    }
                    let captured_type = verify_expression_artifact(
                        &capture.expression,
                        locals,
                        moved,
                        catalog,
                        source,
                    )?;
                    if capture.expression.access != AccessMode::Move
                        || !artifact_type_owns_resource(
                            capture.expression.type_id,
                            &captured_type,
                            catalog,
                        )
                    {
                        return defect(
                            "lowered cleanup registration captures a non-Resource expression",
                        );
                    }
                }
                let mut references = Vec::new();
                collect_cleanup_capture_references(expression, &mut references);
                if references.len() != action.captures.len() {
                    return defect(
                        "lowered cleanup capture coverage has missing or extra references",
                    );
                }
                for (expected, (reference, capture)) in
                    references.iter().zip(action.captures.iter()).enumerate()
                {
                    let ExpressionKind::CleanupCapture(ordinal) = reference.kind else {
                        unreachable!("collector returns only cleanup capture references");
                    };
                    if usize::try_from(ordinal.0).ok() != Some(expected)
                        || ordinal != capture.ordinal
                    {
                        return defect(
                            "lowered cleanup capture references are not in canonical order",
                        );
                    }
                    if reference.type_id != capture.expression.type_id
                        || reference.type_ != capture.expression.type_
                        || reference.coerced_from != capture.expression.coerced_from
                        || reference.access != capture.expression.access
                        || reference.source != capture.expression.source
                    {
                        return defect("cleanup capture substitution metadata is inconsistent");
                    }
                }
                if cleanup_has_conditional_capture_reference(expression) {
                    return defect(
                        "lowered cleanup capture reference is not unconditionally evaluated",
                    );
                }
                let mut has_uncaptured_move = false;
                expression.visit_children(&mut |child| {
                    has_uncaptured_move |=
                        artifact_expression_contains_resource_move(child, catalog);
                });
                if has_uncaptured_move {
                    return defect(
                        "lowered cleanup expression retains an uncaptured Resource move",
                    );
                }
                let type_ = verify_expression_artifact_with_cleanup(
                    expression,
                    locals,
                    moved,
                    catalog,
                    source,
                    Some(&captures),
                )?;
                if matches!(type_, Type::Result { .. })
                    || expression_contains_propagation(expression)
                    || action
                        .captures
                        .iter()
                        .any(|capture| expression_contains_propagation(&capture.expression))
                {
                    return defect("lowered defer may return a recoverable error");
                }
                if artifact_type_owns_resource(expression.type_id, &type_, catalog) {
                    return defect("lowered cleanup action leaves a Resource result undisposed");
                }
                if !matches!(expression.kind, ExpressionKind::Call { .. })
                    && !action.captures.is_empty()
                {
                    return defect("lowered non-call cleanup expression captures Resource custody");
                }
                if artifact_expression_contains_resource_loan(expression, catalog) {
                    return defect("lowered cleanup action retains a Resource loan");
                }
            }
            Statement::Assert { condition, .. } | Statement::Expect { condition, .. } => {
                if verify_expression_artifact(condition, locals, moved, catalog, source)?
                    != Type::Bool
                {
                    return defect("lowered expectation condition is not Bool");
                }
            }
            Statement::Initialize { place, value, .. } => {
                let type_ = verify_expression_artifact(value, locals, moved, catalog, source)?;
                if artifact_type_owns_resource(value.type_id, &type_, catalog)
                    && root_place(value).is_some()
                    && value.access != AccessMode::Move
                {
                    return defect("lowered Resource initialization omits an explicit move");
                }
                if locals
                    .insert(
                        place.local,
                        ArtifactLocalType {
                            type_id: value.type_id,
                            type_,
                        },
                    )
                    .is_some()
                {
                    return defect("lowered initialization repeats a LocalId");
                }
            }
            Statement::Assign { place, value, .. } => {
                let type_ = verify_expression_artifact(value, locals, moved, catalog, source)?;
                if artifact_type_owns_resource(value.type_id, &type_, catalog)
                    && root_place(value).is_some()
                    && value.access != AccessMode::Move
                {
                    return defect("lowered Resource assignment omits an explicit move");
                }
                let expected = verify_place_artifact(place, locals, moved, catalog, source)?;
                if !can_initialize(&type_, &expected) {
                    return defect("lowered assignment changes the local type");
                }
                if artifact_place_owns_resource(place, locals, catalog)
                    && !moved.is_uninitialized(place)
                {
                    return defect("lowered assignment replaces live Resource custody");
                }
                restore_artifact_place_custody(moved, place, locals, catalog)?;
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                if verify_expression_artifact(condition, locals, moved, catalog, source)?
                    != Type::Bool
                {
                    return defect("lowered if condition is not Bool");
                }
                let visible = locals.keys().copied().collect::<BTreeSet<_>>();
                let mut then_locals = locals.clone();
                let mut then_moved = moved.clone();
                verify_statements(
                    then_branch,
                    return_type,
                    &mut then_locals,
                    &mut then_moved,
                    loop_custody,
                    authority,
                    previous_source_start,
                )?;
                if verified_statements_fall_through(then_branch) {
                    verify_artifact_scope_discharged(&then_locals, &then_moved, &visible, catalog)?;
                }
                let mut else_locals = locals.clone();
                let mut else_moved = moved.clone();
                verify_statements(
                    else_branch,
                    return_type,
                    &mut else_locals,
                    &mut else_moved,
                    loop_custody,
                    authority,
                    previous_source_start,
                )?;
                if verified_statements_fall_through(else_branch) {
                    verify_artifact_scope_discharged(&else_locals, &else_moved, &visible, catalog)?;
                }
                *moved = match (
                    verified_statements_fall_through(then_branch),
                    verified_statements_fall_through(else_branch),
                ) {
                    (true, true) => verify_custody_join(then_moved, else_moved)?,
                    (true, false) => then_moved,
                    (false, true) => else_moved,
                    (false, false) => moved.clone(),
                };
            }
            Statement::IfPattern {
                value,
                pattern,
                then_branch,
                else_branch,
                ..
            } => {
                let matched = verify_expression_artifact(value, locals, moved, catalog, source)?;
                let visible = locals.keys().copied().collect::<BTreeSet<_>>();
                let mut then_locals = locals.clone();
                verify_match_pattern_artifact(pattern, &matched, &mut then_locals, catalog)?;
                let mut then_excluded = visible.clone();
                then_excluded.extend(pattern_non_custodial_locals(pattern));
                let mut then_moved = moved.clone();
                if let Some(place) = root_place(value) {
                    for moved_place in artifact_pattern_move_keys(
                        pattern,
                        &matched,
                        &PlaceKey::from_place(&place),
                        catalog,
                    )? {
                        if !then_moved.move_key(moved_place) {
                            return defect(
                                "lowered conditional take pattern moves an unreadable component",
                            );
                        }
                    }
                }
                verify_statements(
                    then_branch,
                    return_type,
                    &mut then_locals,
                    &mut then_moved,
                    loop_custody,
                    authority,
                    previous_source_start,
                )?;
                if verified_statements_fall_through(then_branch) {
                    verify_artifact_scope_discharged(
                        &then_locals,
                        &then_moved,
                        &then_excluded,
                        catalog,
                    )?;
                }
                let mut else_locals = locals.clone();
                let mut else_moved = moved.clone();
                verify_statements(
                    else_branch,
                    return_type,
                    &mut else_locals,
                    &mut else_moved,
                    loop_custody,
                    authority,
                    previous_source_start,
                )?;
                if verified_statements_fall_through(else_branch) {
                    verify_artifact_scope_discharged(&else_locals, &else_moved, &visible, catalog)?;
                }
                *moved = match (
                    verified_statements_fall_through(then_branch),
                    verified_statements_fall_through(else_branch),
                ) {
                    (true, true) => verify_custody_join(then_moved, else_moved)?,
                    (true, false) => then_moved,
                    (false, true) => else_moved,
                    (false, false) => moved.clone(),
                };
            }
            Statement::For {
                pattern,
                iterable,
                body,
                ..
            } => {
                let iterable_type =
                    verify_expression_artifact(iterable, locals, moved, catalog, source)?;
                let exact_iterations = match &iterable_type {
                    Type::FixedArray { length, .. } => length.value(),
                    _ => None,
                };
                let element = match iterable_type {
                    Type::Array(element) | Type::FixedArray { element, .. } => element,
                    _ => return defect("lowered for iterable is not a bounded array"),
                };
                if !artifact_pattern_irrefutable(pattern, &element, catalog) {
                    return defect("lowered for pattern is refutable");
                }
                let moved_before = moved.clone();
                let body_falls_through = verified_statements_fall_through(body);
                let visible = locals.keys().copied().collect::<BTreeSet<_>>();
                verify_match_pattern_artifact(pattern, &element, locals, catalog)?;
                let iteration_transfers = match (exact_iterations, root_place(iterable)) {
                    (Some(iterations), Some(place)) => (0..iterations)
                        .map(|index| {
                            let mut projections = PlaceKey::from_place(&place).projections.to_vec();
                            projections.push(ProjectionKey::Index(Some(i128::from(index))));
                            artifact_pattern_move_keys(
                                pattern,
                                &element,
                                &PlaceKey {
                                    local: place.local,
                                    projections: projections.into(),
                                },
                                catalog,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => Vec::new(),
                };
                if let Some(first) = iteration_transfers.first() {
                    for moved_place in first {
                        if !moved.move_key(moved_place.clone()) {
                            return defect(
                                "lowered for take pattern moves an unreadable component",
                            );
                        }
                    }
                }
                let iteration_entry_moved = moved.clone();
                let mut scope_exclusions = visible.clone();
                scope_exclusions.extend(pattern_non_custodial_locals(pattern));
                loop_custody.push(LoopCustodyEdges {
                    artifact_scope_exclusions: scope_exclusions.clone(),
                    ..LoopCustodyEdges::default()
                });
                verify_statements(
                    body,
                    return_type,
                    locals,
                    moved,
                    loop_custody,
                    authority,
                    previous_source_start,
                )?;
                if body_falls_through {
                    verify_artifact_scope_discharged(locals, moved, &scope_exclusions, catalog)?;
                }
                let mut edges = loop_custody
                    .pop()
                    .expect("verified for loop custody scope was pushed");
                let bindings = match_pattern_binding_signature(pattern);
                locals.retain(|local, _| visible.contains(local));
                for local in bindings.keys() {
                    let binding = Place::local(*local);
                    moved.restore_place(&binding);
                    for state in edges.breaks.iter_mut().chain(edges.continues.iter_mut()) {
                        state.restore_place(&binding);
                    }
                }
                let mut continuing = edges.continues;
                if body_falls_through {
                    continuing.push(moved.clone());
                }
                if exact_iterations.is_none_or(|iterations| iterations > 1) {
                    for state in &continuing {
                        verify_custody_join(iteration_entry_moved.clone(), state.clone())?;
                    }
                }
                for state in &mut continuing {
                    for moved_place in iteration_transfers.iter().skip(1).flatten() {
                        if !state.move_key(moved_place.clone()) {
                            return defect(
                                "lowered exact for repeats a Resource component transfer",
                            );
                        }
                    }
                }
                let mut exits = edges.breaks;
                match exact_iterations {
                    Some(0) => exits.push(moved_before.clone()),
                    Some(1) => exits.extend(continuing),
                    Some(_) => exits.extend(continuing),
                    None => exits.push(moved_before.clone()),
                }
                *moved = verify_reachable_custody(exits)?.unwrap_or(moved_before);
            }
            Statement::While {
                condition, body, ..
            } => {
                if verify_expression_artifact(condition, locals, moved, catalog, source)?
                    != Type::Bool
                {
                    return defect("lowered while condition is not Bool");
                }
                let moved_before = moved.clone();
                let body_falls_through = verified_statements_fall_through(body);
                let visible = locals.keys().copied().collect::<BTreeSet<_>>();
                loop_custody.push(LoopCustodyEdges {
                    artifact_scope_exclusions: visible.clone(),
                    ..LoopCustodyEdges::default()
                });
                verify_statements(
                    body,
                    return_type,
                    locals,
                    moved,
                    loop_custody,
                    authority,
                    previous_source_start,
                )?;
                if body_falls_through {
                    verify_artifact_scope_discharged(locals, moved, &visible, catalog)?;
                }
                let edges = loop_custody
                    .pop()
                    .expect("verified while loop custody scope was pushed");
                locals.retain(|local, _| visible.contains(local));
                let mut continuing = edges.continues;
                if body_falls_through {
                    continuing.push(moved.clone());
                }
                for state in continuing {
                    verify_custody_join(moved_before.clone(), state)?;
                }
                let mut exits = edges.breaks;
                exits.push(moved_before.clone());
                *moved = verify_reachable_custody(exits)?.unwrap_or(moved_before);
            }
            Statement::Break(_) => {
                let exclusions = loop_custody
                    .last()
                    .ok_or_else(|| VerificationFailure::Defect {
                        evidence: Arc::from("lowered break has no custody loop scope"),
                    })?
                    .artifact_scope_exclusions
                    .clone();
                verify_artifact_scope_discharged(locals, moved, &exclusions, catalog)?;
                loop_custody
                    .last_mut()
                    .expect("custody scope was checked")
                    .breaks
                    .push(moved.clone());
            }
            Statement::Continue(_) => {
                let exclusions = loop_custody
                    .last()
                    .ok_or_else(|| VerificationFailure::Defect {
                        evidence: Arc::from("lowered continue has no custody loop scope"),
                    })?
                    .artifact_scope_exclusions
                    .clone();
                verify_artifact_scope_discharged(locals, moved, &exclusions, catalog)?;
                loop_custody
                    .last_mut()
                    .expect("custody scope was checked")
                    .continues
                    .push(moved.clone());
            }
            Statement::Match { value, cases, .. } => {
                let matched = verify_expression_artifact(value, locals, moved, catalog, source)?;
                let visible = locals.keys().copied().collect::<BTreeSet<_>>();
                let mut pattern_keys = BTreeSet::new();
                if !cases
                    .iter()
                    .filter(|case| case.guard.is_none())
                    .filter_map(|case| case.pattern.as_ref())
                    .all(|pattern| pattern_keys.insert(match_pattern_key(pattern)))
                {
                    return defect("lowered match repeats an unreachable pattern");
                }
                if !hir_match_exhaustive_for_type(&matched, cases, catalog) {
                    return defect("lowered match is not exhaustive");
                }
                let mut branch_moved = Vec::with_capacity(cases.len());
                for (index, case) in cases.iter().enumerate() {
                    if case.guard.is_none()
                        && case.pattern.as_ref().is_none_or(|pattern| {
                            artifact_pattern_irrefutable(pattern, &matched, catalog)
                        })
                        && index + 1 != cases.len()
                    {
                        return defect("lowered irrefutable match case is not last");
                    }
                    let mut case_locals = locals.clone();
                    let mut case_moved = moved.clone();
                    let mut case_excluded = visible.clone();
                    if let Some(pattern) = &case.pattern {
                        verify_match_pattern_artifact(
                            pattern,
                            &matched,
                            &mut case_locals,
                            catalog,
                        )?;
                        if let Some(place) = root_place(value) {
                            for moved_place in artifact_pattern_move_keys(
                                pattern,
                                &matched,
                                &PlaceKey::from_place(&place),
                                catalog,
                            )? {
                                if !case_moved.move_key(moved_place) {
                                    return defect(
                                        "lowered take pattern moves an unreadable component",
                                    );
                                }
                            }
                        }
                        case_excluded.extend(pattern_non_custodial_locals(pattern));
                    }
                    if let Some(guard) = &case.guard
                        && verify_expression_artifact(
                            guard,
                            &case_locals,
                            &mut case_moved,
                            catalog,
                            &case.source,
                        )? != Type::Bool
                    {
                        return defect("lowered match guard is not Bool");
                    }
                    verify_statements(
                        &case.body,
                        return_type,
                        &mut case_locals,
                        &mut case_moved,
                        loop_custody,
                        authority,
                        previous_source_start,
                    )?;
                    if verified_statements_fall_through(&case.body) {
                        verify_artifact_scope_discharged(
                            &case_locals,
                            &case_moved,
                            &case_excluded,
                            catalog,
                        )?;
                        branch_moved.push(case_moved);
                    }
                }
                if let Some(joined) = verify_reachable_custody(branch_moved)? {
                    *moved = joined;
                }
            }
            Statement::WithPool {
                binding,
                scope,
                body,
                ..
            } => {
                let mut scope_exclusions = locals.keys().copied().collect::<BTreeSet<_>>();
                let scope_type = verify_expression_artifact(scope, locals, moved, catalog, source)?;
                if !catalog.identities.type_matches(scope.type_id, &scope_type)
                    || !matches!(
                        catalog
                            .discharge_laws
                            .and_then(|laws| laws.laws.get(&scope.type_id)),
                        Some(DischargeLaw::CompilerReclaim(
                            AuthenticatedReclaim::PoolOwnership
                        ))
                    )
                {
                    return defect("lowered Pool scope lacks an authenticated reclaim law");
                }
                if locals
                    .insert(
                        binding.local,
                        ArtifactLocalType {
                            type_id: scope.type_id,
                            type_: scope_type,
                        },
                    )
                    .is_some()
                {
                    return defect("lowered Pool scope repeats a LocalId");
                }
                scope_exclusions.insert(binding.local);
                verify_statements(
                    body,
                    return_type,
                    locals,
                    moved,
                    loop_custody,
                    authority,
                    previous_source_start,
                )?;
                if verified_statements_fall_through(body) {
                    verify_artifact_scope_discharged(locals, moved, &scope_exclusions, catalog)?;
                }
                locals.remove(&binding.local);
                moved.restore_place(binding);
            }
            Statement::Pass(_) => {}
        }
        if let Some((state, loop_state)) = unreachable_state {
            *moved = state;
            *loop_custody = loop_state;
        } else {
            reachable = verified_statements_fall_through(std::slice::from_ref(statement));
        }
    }
    Ok(())
}

fn statement_source(statement: &Statement) -> &SourceRange {
    match statement {
        Statement::Return { source, .. }
        | Statement::Panic { source, .. }
        | Statement::Assert { source, .. }
        | Statement::Expect { source, .. }
        | Statement::Initialize { source, .. }
        | Statement::Assign { source, .. }
        | Statement::If { source, .. }
        | Statement::IfPattern { source, .. }
        | Statement::For { source, .. }
        | Statement::While { source, .. }
        | Statement::Match { source, .. }
        | Statement::Defer { source, .. }
        | Statement::WithPool { source, .. }
        | Statement::Break(source)
        | Statement::Continue(source)
        | Statement::Pass(source) => source,
        Statement::Evaluate(expression) => &expression.source,
    }
}

fn literal_type(literal: &Literal) -> Type {
    match literal {
        Literal::Unit => Type::Unit,
        Literal::Bool(_) => Type::Bool,
        Literal::Integer { kind, .. } => Type::Integer(*kind),
        Literal::Float { kind, .. } => Type::Float(*kind),
        Literal::Text(_) => Type::Text,
        Literal::Scalar(_) => Type::Scalar,
        Literal::Bytes(_) => Type::Bytes,
    }
}

fn artifact_type_owns_resource(
    type_id: TypeId,
    type_: &Type,
    catalog: &ArtifactCatalog<'_>,
) -> bool {
    if let Some(laws) = catalog.discharge_laws {
        return catalog.identities.type_matches(type_id, type_) && laws.laws.contains_key(&type_id);
    }
    let displays = catalog
        .structs
        .iter()
        .map(|(definition, struct_)| (*definition, struct_.display.clone()))
        .collect::<BTreeMap<_, _>>();
    audit_expected_discharge_law(type_, catalog.structs, catalog.interfaces, &displays).is_some()
}

fn artifact_place_owns_resource(
    place: &Place,
    locals: &BTreeMap<LocalId, ArtifactLocalType>,
    catalog: &ArtifactCatalog<'_>,
) -> bool {
    let Some(local) = locals.get(&place.local) else {
        return false;
    };
    let Some(laws) = catalog.discharge_laws else {
        return artifact_place_type(place, locals).is_some_and(|type_| {
            let displays = catalog
                .structs
                .iter()
                .map(|(definition, struct_)| (*definition, struct_.display.clone()))
                .collect::<BTreeMap<_, _>>();
            audit_expected_discharge_law(&type_, catalog.structs, catalog.interfaces, &displays)
                .is_some()
        });
    };
    if !catalog.identities.type_matches(local.type_id, &local.type_) {
        return false;
    }
    let mut law = laws.laws.get(&local.type_id);
    let mut type_ = local.type_.clone();
    for projection in place.projections.iter() {
        let Some(current_law) = law else {
            return false;
        };
        match projection {
            PlaceProjection::Field {
                definition, name, ..
            } => {
                let Type::Nominal {
                    definition: actual_definition,
                    arguments,
                    ..
                } = &type_
                else {
                    return false;
                };
                if actual_definition != definition {
                    return false;
                }
                let Some(struct_) = catalog.structs.get(definition) else {
                    return false;
                };
                let Ok(fields) = artifact_struct_fields(struct_, arguments) else {
                    return false;
                };
                let Some(field) = fields.iter().find(|field| field.name == **name) else {
                    return false;
                };
                law = match current_law {
                    DischargeLaw::Structural(components) => components
                        .iter()
                        .find(|component| {
                            component.identity
                                == DischargeComponentIdentity::Field(field.definition)
                        })
                        .map(|component| &component.law),
                    _ => None,
                };
                let substitutions = struct_
                    .type_parameters
                    .iter()
                    .copied()
                    .zip(arguments.iter().cloned())
                    .collect::<BTreeMap<_, _>>();
                type_ = substitute(&field.type_, &substitutions);
            }
            PlaceProjection::Index { .. } => {
                law = match current_law {
                    DischargeLaw::Structural(components) => components
                        .iter()
                        .find(|component| component.identity == DischargeComponentIdentity::Element)
                        .map(|component| &component.law),
                    _ => None,
                };
                type_ = match type_ {
                    Type::Array(element) | Type::FixedArray { element, .. } => (*element).clone(),
                    _ => return false,
                };
            }
        }
    }
    law.is_some()
}

fn artifact_has_live_discharge_obligation(
    type_id: TypeId,
    type_: &Type,
    place: &PlaceKey,
    moved: &MoveState,
    catalog: &ArtifactCatalog<'_>,
) -> bool {
    let displays = catalog
        .structs
        .iter()
        .map(|(definition, struct_)| (*definition, struct_.display.clone()))
        .collect::<BTreeMap<_, _>>();
    let law = if let Some(laws) = catalog.discharge_laws {
        catalog
            .identities
            .type_matches(type_id, type_)
            .then(|| laws.laws.get(&type_id).cloned())
            .flatten()
    } else {
        audit_expected_discharge_law(type_, catalog.structs, catalog.interfaces, &displays)
    };
    law.as_ref().is_some_and(|law| {
        artifact_law_has_live_discharge_obligation(type_, law, place, moved, catalog)
    })
}

fn artifact_law_has_live_discharge_obligation(
    type_: &Type,
    law: &DischargeLaw,
    place: &PlaceKey,
    moved: &MoveState,
    catalog: &ArtifactCatalog<'_>,
) -> bool {
    if !law.requires_explicit_discharge() {
        return false;
    }
    if let Type::Nominal {
        definition,
        arguments,
        ..
    } = type_
        && let Some(struct_) = catalog.structs.get(definition)
        && struct_.resource
        && let DischargeLaw::Structural(components) = law
    {
        let substitutions = struct_
            .type_parameters
            .iter()
            .copied()
            .zip(arguments.iter().cloned())
            .collect::<BTreeMap<_, _>>();
        let fields = artifact_struct_fields(struct_, arguments).unwrap_or_default();
        return fields.iter().any(|field| {
            let Some(field_law) = components
                .iter()
                .find(|component| {
                    component.identity == DischargeComponentIdentity::Field(field.definition)
                })
                .map(|component| &component.law)
            else {
                return false;
            };
            let mut projections = place.projections.to_vec();
            projections.push(ProjectionKey::Field(
                *definition,
                Arc::from(field.name.as_str()),
            ));
            artifact_law_has_live_discharge_obligation(
                &substitute(&field.type_, &substitutions),
                field_law,
                &PlaceKey {
                    local: place.local,
                    projections: projections.into(),
                },
                moved,
                catalog,
            )
        });
    }
    if let Type::FixedArray {
        element,
        length: ArrayLength::Value(length),
    } = type_
        && let DischargeLaw::Structural(components) = law
        && let Some(element_law) = components
            .iter()
            .find(|component| component.identity == DischargeComponentIdentity::Element)
            .map(|component| &component.law)
    {
        return (0..*length).any(|index| {
            let mut projections = place.projections.to_vec();
            projections.push(ProjectionKey::Index(Some(i128::from(index))));
            artifact_law_has_live_discharge_obligation(
                element,
                element_law,
                &PlaceKey {
                    local: place.local,
                    projections: projections.into(),
                },
                moved,
                catalog,
            )
        });
    }
    if let Type::Tuple(members) = type_
        && let DischargeLaw::Structural(components) = law
    {
        return members.iter().enumerate().any(|(index, member)| {
            let identity =
                DischargeComponentIdentity::Tuple(u32::try_from(index).unwrap_or(u32::MAX));
            let Some(member_law) = components
                .iter()
                .find(|component| component.identity == identity)
                .map(|component| &component.law)
            else {
                return false;
            };
            let mut projections = place.projections.to_vec();
            projections.push(ProjectionKey::Index(Some(
                i128::try_from(index).unwrap_or(i128::MAX),
            )));
            artifact_law_has_live_discharge_obligation(
                member,
                member_law,
                &PlaceKey {
                    local: place.local,
                    projections: projections.into(),
                },
                moved,
                catalog,
            )
        });
    }
    if matches!(type_, Type::Array(_)) {
        return !moved.is_key_fully_moved(place);
    }
    !moved.is_key_unreadable(place)
}

fn verify_artifact_scope_discharged(
    locals: &BTreeMap<LocalId, ArtifactLocalType>,
    moved: &MoveState,
    excluded: &BTreeSet<LocalId>,
    catalog: &ArtifactCatalog<'_>,
) -> Result<(), VerificationFailure> {
    if locals.iter().any(|(local, type_)| {
        !excluded.contains(local)
            && artifact_has_live_discharge_obligation(
                type_.type_id,
                &type_.type_,
                &PlaceKey {
                    local: *local,
                    projections: Arc::from([]),
                },
                moved,
                catalog,
            )
    }) {
        return defect("lowered recoverable exit abandons live Resource custody");
    }
    Ok(())
}

fn match_pattern_key(pattern: &HirMatchPattern) -> Vec<u8> {
    let mut key = Vec::new();
    append_match_pattern(&mut key, pattern);
    key
}

fn verify_place_artifact(
    place: &Place,
    locals: &BTreeMap<LocalId, ArtifactLocalType>,
    moved: &mut MoveState,
    catalog: &ArtifactCatalog<'_>,
    source: &SourceRange,
) -> Result<Type, VerificationFailure> {
    verify_place_artifact_with_cleanup(place, locals, moved, catalog, source, None)
}

fn verify_place_artifact_with_cleanup(
    place: &Place,
    locals: &BTreeMap<LocalId, ArtifactLocalType>,
    moved: &mut MoveState,
    catalog: &ArtifactCatalog<'_>,
    source: &SourceRange,
    cleanup_captures: Option<&BTreeMap<CleanupCaptureOrdinal, &CleanupCapture>>,
) -> Result<Type, VerificationFailure> {
    let mut type_ = locals
        .get(&place.local)
        .map(|local| local.type_.clone())
        .ok_or_else(|| VerificationFailure::Defect {
            evidence: Arc::from("lowered place references an unknown LocalId"),
        })?;
    for projection in place.projections.iter() {
        match projection {
            PlaceProjection::Field {
                definition,
                name,
                type_: recorded_type,
                mutable,
            } => {
                let Type::Nominal {
                    definition: actual_definition,
                    arguments,
                    ..
                } = &type_
                else {
                    return defect("lowered field place projects a non-struct type");
                };
                if actual_definition != definition {
                    return defect("lowered field place records the wrong owner");
                }
                let Some(struct_) = catalog.structs.get(definition) else {
                    return defect("lowered field place references an unknown struct");
                };
                let fields = artifact_struct_fields(struct_, arguments)?;
                let Some(field) = fields.iter().find(|field| field.name == **name) else {
                    return defect("lowered field place references an unknown field");
                };
                let substitutions = struct_
                    .type_parameters
                    .iter()
                    .copied()
                    .zip(arguments.iter().cloned())
                    .collect::<BTreeMap<_, _>>();
                let actual_type = substitute(&field.type_, &substitutions);
                if actual_type != *recorded_type || field.mutable != *mutable {
                    return defect("lowered field place metadata disagrees with its declaration");
                }
                type_ = actual_type;
            }
            PlaceProjection::Index {
                index,
                type_: recorded_type,
            } => {
                let index_type = verify_expression_artifact_with_cleanup(
                    index,
                    locals,
                    moved,
                    catalog,
                    source,
                    cleanup_captures,
                )?;
                if !matches!(index_type, Type::Integer(_)) {
                    return defect("lowered index place uses a non-integer index");
                }
                let element = match &type_ {
                    Type::Array(element) | Type::FixedArray { element, .. } => (**element).clone(),
                    _ => return defect("lowered index place projects a non-array type"),
                };
                if element != *recorded_type {
                    return defect("lowered index place records the wrong element type");
                }
                type_ = element;
            }
        }
    }
    Ok(type_)
}

fn verify_expression_artifact(
    expression: &Expression,
    locals: &BTreeMap<LocalId, ArtifactLocalType>,
    moved: &mut MoveState,
    catalog: &ArtifactCatalog<'_>,
    provenance_owner: &SourceRange,
) -> Result<Type, VerificationFailure> {
    verify_expression_artifact_with_cleanup(
        expression,
        locals,
        moved,
        catalog,
        provenance_owner,
        None,
    )
}

fn verify_expression_artifact_with_cleanup(
    expression: &Expression,
    locals: &BTreeMap<LocalId, ArtifactLocalType>,
    moved: &mut MoveState,
    catalog: &ArtifactCatalog<'_>,
    provenance_owner: &SourceRange,
    cleanup_captures: Option<&BTreeMap<CleanupCaptureOrdinal, &CleanupCapture>>,
) -> Result<Type, VerificationFailure> {
    if expression.source.start() > expression.source.end()
        || expression.source.path() != provenance_owner.path()
        || expression.source.start() < provenance_owner.start()
        || expression.source.end() > provenance_owner.end()
    {
        return defect("lowered expression has reversed or foreign provenance");
    }
    if !catalog
        .identities
        .type_matches(expression.type_id, &expression.type_)
    {
        return defect("lowered expression TypeId disagrees with its canonical type");
    }
    let mut previous_child_start = expression.source.start();
    let mut invalid_child_provenance = false;
    expression.visit_children(&mut |child| {
        if child.source.path() != provenance_owner.path()
            || child.source.start() < expression.source.start()
            || child.source.end() > expression.source.end()
            || child.source.start() < previous_child_start
        {
            invalid_child_provenance = true;
        }
        previous_child_start = child.source.start();
    });
    if invalid_child_provenance {
        return defect("lowered child expression provenance escapes its owner or source order");
    }
    let actual = match &expression.kind {
        ExpressionKind::Literal(literal) => match literal {
            Literal::Unit => Type::Unit,
            Literal::Bool(_) => Type::Bool,
            Literal::Integer { kind, .. } => Type::Integer(*kind),
            Literal::Float { kind, .. } => Type::Float(*kind),
            Literal::Text(_) => Type::Text,
            Literal::Scalar(_) => Type::Scalar,
            Literal::Bytes(_) => Type::Bytes,
        },
        ExpressionKind::Read(place) => {
            let type_ = verify_place_artifact_with_cleanup(
                place,
                locals,
                moved,
                catalog,
                &expression.source,
                cleanup_captures,
            )?;
            if moved.is_unreadable(place) {
                return defect("lowered read uses a LocalId after it was moved");
            }
            if expression.access == AccessMode::Move {
                if !artifact_place_owns_resource(place, locals, catalog) {
                    return defect("lowered move does not transfer a Resource-owning place");
                }
                if !moved.move_place(place) {
                    return defect("lowered move repeats an unavailable Resource transfer");
                }
            }
            type_
        }
        ExpressionKind::Constant(id) => catalog
            .constants
            .get(id)
            .map(|constant| constant.type_.clone())
            .ok_or_else(|| VerificationFailure::Defect {
                evidence: Arc::from("lowered expression references an unknown constant"),
            })?,
        ExpressionKind::FunctionValue {
            definition,
            specialization,
        } => {
            let specialization = specialization.ok_or_else(|| VerificationFailure::Defect {
                evidence: Arc::from("template function value reached a concrete artifact"),
            })?;
            let record = catalog
                .specializations
                .get(&specialization)
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from("function value references an unknown specialization"),
                })?;
            let function = catalog.specialized.get(&specialization).ok_or_else(|| {
                VerificationFailure::Defect {
                    evidence: Arc::from("function value has no concrete specialization body"),
                }
            })?;
            if record.definition != *definition || function.id != *definition {
                return defect("function value mixes definition and specialization identities");
            }
            if function
                .parameters
                .iter()
                .any(|(_, _, access)| *access != AccessMode::Copy)
            {
                return defect("function value exposes a non-value parameter mode");
            }
            Type::Function {
                parameters: function
                    .parameters
                    .iter()
                    .map(|(_, type_, _)| type_.clone())
                    .collect::<Vec<_>>()
                    .into(),
                return_type: Arc::new(function.return_type.clone()),
            }
        }
        ExpressionKind::Closure(closure) => {
            let Type::Function {
                parameters,
                return_type,
            } = &expression.type_
            else {
                return defect("lowered closure does not have a function type");
            };
            if parameters.len() != closure.parameters.len()
                || closure.parameters.len() != closure.parameter_type_ids.len()
                || closure.captures.len() != closure.capture_type_ids.len()
                || parameters
                    .iter()
                    .zip(closure.parameters.iter())
                    .any(|(expected, (_, actual))| expected != actual)
                || **return_type != closure.return_type
                || closure.id.0 != xxh3_128(&closure.identity_key)
            {
                return defect("lowered closure signature or identity is inconsistent");
            }
            let mut closure_locals = BTreeMap::new();
            for ((local, type_), type_id) in
                closure.captures.iter().zip(closure.capture_type_ids.iter())
            {
                let Some(local_type) = locals.get(local) else {
                    return defect("lowered closure capture is invalid");
                };
                if local_type.type_ != *type_
                    || local_type.type_id != *type_id
                    || !catalog.identities.type_matches(*type_id, type_)
                    || artifact_type_owns_resource(local_type.type_id, type_, catalog)
                    || closure_locals.insert(*local, local_type.clone()).is_some()
                {
                    return defect("lowered closure capture is invalid");
                }
            }
            for ((local, type_), type_id) in closure
                .parameters
                .iter()
                .zip(closure.parameter_type_ids.iter())
            {
                if !catalog.identities.type_matches(*type_id, type_) {
                    return defect("lowered closure parameter TypeId disagrees with its type");
                }
                if closure_locals
                    .insert(
                        *local,
                        ArtifactLocalType {
                            type_id: *type_id,
                            type_: type_.clone(),
                        },
                    )
                    .is_some()
                {
                    return defect("lowered closure repeats a LocalId");
                }
            }
            let actual = verify_expression_artifact(
                &closure.body,
                &closure_locals,
                &mut MoveState::default(),
                catalog,
                &closure.source,
            )?;
            if !can_return(&actual, &closure.return_type) {
                return defect("lowered closure body disagrees with its return type");
            }
            expression.type_.clone()
        }
        ExpressionKind::CleanupCapture(ordinal) => {
            let capture = cleanup_captures
                .and_then(|captures| captures.get(ordinal))
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from(
                        "cleanup capture reference appears outside its verified action",
                    ),
                })?;
            if expression.type_id != capture.expression.type_id
                || expression.type_ != capture.expression.type_
                || expression.coerced_from != capture.expression.coerced_from
                || expression.access != capture.expression.access
                || expression.source != capture.expression.source
            {
                return defect("cleanup capture substitution metadata is inconsistent");
            }
            capture.expression.type_.clone()
        }
        ExpressionKind::Call { target, arguments } => {
            verify_artifact_call_custody(arguments)?;
            let callable_type = if let CallTarget::Callable { value } = target {
                Some(verify_expression_artifact_with_cleanup(
                    value,
                    locals,
                    moved,
                    catalog,
                    &expression.source,
                    cleanup_captures,
                )?)
            } else {
                None
            };
            let argument_types = arguments
                .iter()
                .map(|argument| {
                    verify_expression_artifact_with_cleanup(
                        argument,
                        locals,
                        moved,
                        catalog,
                        &expression.source,
                        cleanup_captures,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            match target {
                CallTarget::Callable { .. } => {
                    let Some(Type::Function {
                        parameters,
                        return_type,
                    }) = callable_type
                    else {
                        return defect("callable target is not a function value");
                    };
                    if parameters.len() != argument_types.len()
                        || arguments
                            .iter()
                            .any(|argument| argument.access != AccessMode::Copy)
                        || argument_types
                            .iter()
                            .zip(parameters.iter())
                            .any(|(actual, expected)| !can_pass(actual, expected))
                    {
                        return defect("callable operands disagree with its function type");
                    }
                    (*return_type).clone()
                }
                CallTarget::TemplateFunction {
                    definition,
                    argument_order,
                    argument_parameters,
                } => catalog
                    .templates
                    .get(definition)
                    .map(|function| {
                        let expected_parameters = argument_order
                            .iter()
                            .map(|index| function.parameter_definitions[usize::from(*index)])
                            .collect::<Vec<_>>();
                        if argument_parameters.as_ref() != expected_parameters
                            || !argument_accesses_match(
                                arguments,
                                argument_order,
                                &function.parameters,
                            )
                        {
                            return defect("template call parameter identities disagree");
                        }
                        let ordered = reorder_types(&argument_types, argument_order)?;
                        if !arguments_match(&ordered, &function.parameters) {
                            return defect("template call operands disagree with parameters");
                        }
                        Ok(function.return_type.clone())
                    })
                    .transpose()?
                    .ok_or_else(|| VerificationFailure::Defect {
                        evidence: Arc::from("template call references an unknown function"),
                    })?,
                CallTarget::Function {
                    definition,
                    specialization,
                    argument_order,
                    argument_parameters,
                } => {
                    let record = catalog.specializations.get(specialization).ok_or_else(|| {
                        VerificationFailure::Defect {
                            evidence: Arc::from(
                                "lowered call references an unknown SpecializationId",
                            ),
                        }
                    })?;
                    if record.definition != *definition {
                        return defect("lowered call mixes DefinitionId and SpecializationId");
                    }
                    let function = catalog.specialized.get(specialization).ok_or_else(|| {
                        VerificationFailure::Defect {
                            evidence: Arc::from("lowered call has no concrete specialization body"),
                        }
                    })?;
                    let expected_parameters = argument_order
                        .iter()
                        .map(|index| function.parameter_definitions[usize::from(*index)])
                        .collect::<Vec<_>>();
                    let ordered = reorder_types(&argument_types, argument_order)?;
                    if function.id != *definition
                        || argument_parameters.as_ref() != expected_parameters
                        || !arguments_match(&ordered, &function.parameters)
                        || !argument_accesses_match(arguments, argument_order, &function.parameters)
                    {
                        return defect("concrete call operands disagree with specialization");
                    }
                    function.return_type.clone()
                }
                CallTarget::Interface {
                    interface,
                    alternatives,
                    argument_order,
                    argument_parameters,
                } => {
                    let declaration = catalog.interfaces.get(interface).ok_or_else(|| {
                        VerificationFailure::Defect {
                            evidence: Arc::from("interface call references an unknown interface"),
                        }
                    })?;
                    let requirement = declaration
                        .requirements
                        .values()
                        .find(|requirement| {
                            let expected = argument_order
                                .iter()
                                .map(|index| requirement.parameters[usize::from(*index)].definition)
                                .collect::<Vec<_>>();
                            argument_parameters.as_ref() == expected
                        })
                        .ok_or_else(|| VerificationFailure::Defect {
                            evidence: Arc::from(
                                "interface call parameter identities match no requirement",
                            ),
                        })?;
                    if requirement.parameters.len() != argument_order.len() {
                        return defect("interface call requirement arity is malformed");
                    }
                    if alternatives.is_empty() {
                        if !argument_types.iter().any(type_has_placeholder) {
                            return defect("concrete interface call has no closed alternatives");
                        }
                        return Ok(expression.type_.clone());
                    }
                    if declaration.implementations.is_empty() {
                        return defect("interface call has no declared implementations");
                    }
                    for (nominal, definition, specialization) in alternatives.iter() {
                        if declaration
                            .implementations
                            .get(nominal)
                            .is_none_or(|members| !members.values().any(|id| id == definition))
                        {
                            return defect("interface call alternative is not an implementation");
                        }
                        let function =
                            catalog.specialized.get(specialization).ok_or_else(|| {
                                VerificationFailure::Defect {
                                    evidence: Arc::from(
                                        "interface call alternative has no specialization",
                                    ),
                                }
                            })?;
                        if function.id != *definition
                            || function.parameters.len() != argument_types.len()
                            || argument_order.len() != argument_types.len()
                            || !argument_accesses_match(
                                arguments,
                                argument_order,
                                &function.parameters,
                            )
                        {
                            return defect("interface call alternative signature is malformed");
                        }
                        for (source, parameter) in argument_order.iter().enumerate().skip(1) {
                            if !can_pass(
                                &argument_types[source],
                                &function.parameters[usize::from(*parameter)].1,
                            ) {
                                return defect(
                                    "interface call operand disagrees with an alternative",
                                );
                            }
                        }
                        if !can_unify(&function.return_type, &expression.type_) {
                            return defect("interface alternatives disagree on return type");
                        }
                    }
                    expression.type_.clone()
                }
                CallTarget::Build { primitive, .. } => match primitive.kind {
                    BuildKind::Image => Type::Builtin(BuiltinType::Image),
                    BuildKind::Test => Type::Builtin(BuiltinType::Test),
                    BuildKind::Node { definition, .. } => {
                        if !matches!(
                            expression.type_,
                            Type::Nominal { definition: actual, .. } if actual == definition
                        ) {
                            return defect(
                                "symbolic node result annotation disagrees with its constructor",
                            );
                        }
                        expression.type_.clone()
                    }
                },
                CallTarget::BuiltinVariant(variant) => {
                    let inferred = builtin_variant_type(*variant, arguments, &expression.source)
                        .map_err(|_| VerificationFailure::Defect {
                            evidence: Arc::from("built-in variant operands are malformed"),
                        })?;
                    if !can_unify(&inferred, &expression.type_) {
                        return defect("built-in variant result annotation is inconsistent");
                    }
                    expression.type_.clone()
                }
                CallTarget::UserVariant {
                    id, argument_order, ..
                } => {
                    let Type::Nominal {
                        definition,
                        arguments: type_arguments,
                        ..
                    } = expression
                        .coerced_from
                        .as_ref()
                        .unwrap_or(&expression.type_)
                    else {
                        return defect("user variant result is not its nominal type");
                    };
                    if *definition != id.owner {
                        return defect("user variant owner disagrees with result type");
                    }
                    let signature =
                        catalog
                            .variants
                            .get(id)
                            .ok_or_else(|| VerificationFailure::Defect {
                                evidence: Arc::from("user variant has no semantic signature"),
                            })?;
                    let ordered = reorder_types(&argument_types, argument_order)?;
                    if type_arguments.len() != signature.type_parameters.len() {
                        return defect("user variant result has the wrong generic arity");
                    }
                    let substitutions = signature
                        .type_parameters
                        .iter()
                        .copied()
                        .zip(type_arguments.iter().cloned())
                        .collect::<BTreeMap<_, _>>();
                    if ordered.len() != signature.parameters.len()
                        || ordered
                            .iter()
                            .zip(&signature.parameters)
                            .any(|(argument, parameter)| {
                                !can_pass(argument, &substitute(&parameter.type_, &substitutions))
                            })
                    {
                        return defect("user variant operands disagree with its payload");
                    }
                    expression.type_.clone()
                }
                CallTarget::Struct {
                    definition,
                    field_order,
                    argument_fields,
                    ..
                } => {
                    let Type::Nominal {
                        definition: result_definition,
                        arguments: type_arguments,
                        ..
                    } = expression
                        .coerced_from
                        .as_ref()
                        .unwrap_or(&expression.type_)
                    else {
                        return defect("struct construction result is not its nominal type");
                    };
                    if result_definition != definition {
                        return defect("struct construction identity disagrees with result type");
                    }
                    let signature = catalog.structs.get(definition).ok_or_else(|| {
                        VerificationFailure::Defect {
                            evidence: Arc::from("struct construction has no semantic signature"),
                        }
                    })?;
                    if type_arguments.len() != signature.type_parameters.len() {
                        return defect("struct construction result has the wrong generic arity");
                    }
                    let substitutions = signature
                        .type_parameters
                        .iter()
                        .copied()
                        .zip(type_arguments.iter().cloned())
                        .collect::<BTreeMap<_, _>>();
                    let fields = artifact_struct_fields(signature, type_arguments)?;
                    let declared_order = fields
                        .iter()
                        .map(|field| field.name.as_str())
                        .collect::<Vec<_>>();
                    if field_order.iter().map(AsRef::as_ref).collect::<Vec<_>>() != declared_order {
                        return defect(
                            "struct construction field order disagrees with declaration",
                        );
                    }
                    if argument_fields.len() != argument_types.len()
                        || argument_fields.len() != fields.len()
                    {
                        return defect("struct construction does not initialize every field");
                    }
                    let mut seen = BTreeSet::new();
                    for (field_name, argument_type) in argument_fields.iter().zip(&argument_types) {
                        if !seen.insert(field_name.as_ref()) {
                            return defect("struct construction initializes a field twice");
                        }
                        let Some(field) = fields
                            .iter()
                            .find(|field| field.name == field_name.as_ref())
                        else {
                            return defect("struct construction initializes an unknown field");
                        };
                        if !can_pass(argument_type, &substitute(&field.type_, &substitutions)) {
                            return defect("struct construction operand disagrees with field type");
                        }
                    }
                    expression.type_.clone()
                }
                CallTarget::Test { argument_order, .. } => {
                    let _ = reorder_types(&argument_types, argument_order)?;
                    Type::Builtin(BuiltinType::TestApplication)
                }
            }
        }
        ExpressionKind::Array(values) => {
            let Type::FixedArray { element, length } = &expression.type_ else {
                return defect("array literal result is not a fixed-array type");
            };
            if length.value() != Some(u64::try_from(values.len()).unwrap_or(u64::MAX)) {
                return defect("array literal length disagrees with its fixed-array type");
            }
            for value in &**values {
                let actual = verify_expression_artifact_with_cleanup(
                    value,
                    locals,
                    moved,
                    catalog,
                    &expression.source,
                    cleanup_captures,
                )?;
                if actual != **element {
                    return defect("array element disagrees with aggregate type");
                }
            }
            expression.type_.clone()
        }
        ExpressionKind::RepeatedArray { value, length } => {
            let Type::FixedArray {
                element,
                length: type_length,
            } = &expression.type_
            else {
                return defect("repeated array result is not a fixed-array type");
            };
            if Some(*length) != type_length.value() {
                return defect("repeated array length disagrees with its fixed-array type");
            }
            let actual = verify_expression_artifact_with_cleanup(
                value,
                locals,
                moved,
                catalog,
                &expression.source,
                cleanup_captures,
            )?;
            if actual != **element {
                return defect("repeated array element disagrees with aggregate type");
            }
            expression.type_.clone()
        }
        ExpressionKind::Tuple(values) => Type::Tuple(
            values
                .iter()
                .map(|value| {
                    verify_expression_artifact_with_cleanup(
                        value,
                        locals,
                        moved,
                        catalog,
                        &expression.source,
                        cleanup_captures,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?
                .into(),
        ),
        ExpressionKind::Index { value, index } => {
            let value_type = verify_expression_artifact_with_cleanup(
                value,
                locals,
                moved,
                catalog,
                &expression.source,
                cleanup_captures,
            )?;
            let index_type = verify_expression_artifact_with_cleanup(
                index,
                locals,
                moved,
                catalog,
                &expression.source,
                cleanup_captures,
            )?;
            if !matches!(index_type, Type::Integer(_)) {
                return defect("array index is not an integer");
            }
            let element = match value_type {
                Type::Array(element) | Type::FixedArray { element, .. } => element,
                _ => return defect("indexed value is not an array"),
            };
            (*element).clone()
        }
        ExpressionKind::Positive(value) => {
            let type_ = verify_expression_artifact_with_cleanup(
                value,
                locals,
                moved,
                catalog,
                &expression.source,
                cleanup_captures,
            )?;
            if !type_.is_numeric() {
                return defect("positive operand is not numeric");
            }
            type_
        }
        ExpressionKind::Negate(value) => {
            let type_ = verify_expression_artifact_with_cleanup(
                value,
                locals,
                moved,
                catalog,
                &expression.source,
                cleanup_captures,
            )?;
            if !matches!(
                &type_,
                Type::Integer(kind) if kind.is_signed()
            ) && !matches!(&type_, Type::Float(_))
            {
                return defect("negate operand is not numeric");
            }
            type_
        }
        ExpressionKind::BitNot(value) => {
            let type_ = verify_expression_artifact_with_cleanup(
                value,
                locals,
                moved,
                catalog,
                &expression.source,
                cleanup_captures,
            )?;
            if !matches!(type_, Type::Integer(_)) {
                return defect("bitwise-not operand is not an integer");
            }
            type_
        }
        ExpressionKind::Not(value) => {
            let type_ = verify_expression_artifact_with_cleanup(
                value,
                locals,
                moved,
                catalog,
                &expression.source,
                cleanup_captures,
            )?;
            if type_ != Type::Bool {
                return defect("not operand is not Bool");
            }
            Type::Bool
        }
        ExpressionKind::Await(value) => {
            if call_loan(value).is_some() {
                return defect("lowered await holds a Resource loan across suspension");
            }
            verify_expression_artifact_with_cleanup(
                value,
                locals,
                moved,
                catalog,
                &expression.source,
                cleanup_captures,
            )?
        }
        ExpressionKind::Propagate(value) => {
            let propagated = verify_expression_artifact_with_cleanup(
                value,
                locals,
                moved,
                catalog,
                &expression.source,
                cleanup_captures,
            )?;
            let success = match propagated {
                Type::Result { success, .. } | Type::Option(success) => success,
                _ => return defect("lowered propagation operand is not Result or Option"),
            };
            (*success).clone()
        }
        ExpressionKind::Is { value, pattern } => {
            let value = verify_expression_artifact_with_cleanup(
                value,
                locals,
                moved,
                catalog,
                &expression.source,
                cleanup_captures,
            )?;
            let mut pattern_locals = locals.clone();
            verify_match_pattern_artifact(pattern, &value, &mut pattern_locals, catalog)?;
            if !match_pattern_binding_signature(pattern).is_empty() {
                return defect("lowered expression-form is pattern binds locals");
            }
            Type::Bool
        }
        ExpressionKind::Binary {
            operator,
            left,
            right,
        } => {
            let left = verify_expression_artifact_with_cleanup(
                left,
                locals,
                moved,
                catalog,
                &expression.source,
                cleanup_captures,
            )?;
            let right = verify_expression_artifact_with_cleanup(
                right,
                locals,
                moved,
                catalog,
                &expression.source,
                cleanup_captures,
            )?;
            binary_type(*operator, &left, &right).ok_or_else(|| VerificationFailure::Defect {
                evidence: Arc::from("binary operation operands are not well typed"),
            })?
        }
    };
    if expression.access == AccessMode::Move
        && matches!(expression.kind, ExpressionKind::Index { .. })
        && let Some(place) = root_place(expression)
        && !moved.move_place(&place)
    {
        return defect("lowered move reads a containing LocalId after it was moved");
    }
    let valid_existential_coercion = expression.coerced_from.as_ref() == Some(&actual)
        && matches!(
            (&actual, &expression.type_),
            (
                Type::Nominal { definition, .. },
                Type::Any { interface, .. }
            ) if catalog.interfaces.get(interface).is_some_and(|interface| {
                interface.implementations.contains_key(definition)
            })
        );
    if actual != expression.type_ && !valid_existential_coercion {
        return defect("lowered expression type annotation is inconsistent");
    }
    Ok(expression.type_.clone())
}

fn arguments_match(argument_types: &[Type], parameters: &[(LocalId, Type, AccessMode)]) -> bool {
    argument_types.len() == parameters.len()
        && argument_types
            .iter()
            .zip(parameters)
            .all(|(argument, (_, parameter, _))| can_pass(argument, parameter))
}

fn argument_accesses_match(
    arguments: &[Expression],
    source_to_parameter: &[u16],
    parameters: &[(LocalId, Type, AccessMode)],
) -> bool {
    arguments.len() == source_to_parameter.len()
        && arguments
            .iter()
            .zip(source_to_parameter)
            .all(|(argument, parameter)| {
                parameters
                    .get(usize::from(*parameter))
                    .is_some_and(|(_, _, access)| argument.access == *access)
            })
}

fn verify_artifact_call_custody(arguments: &[Expression]) -> Result<(), VerificationFailure> {
    let mut active_places: Vec<(PlaceKey, AccessMode)> = Vec::new();
    for argument in arguments {
        let Some(place) = root_place(argument).map(|place| PlaceKey::from_place(&place)) else {
            continue;
        };
        if matches!(
            argument.access,
            AccessMode::Read | AccessMode::Mut | AccessMode::Move
        ) && active_places.iter().any(|(other, other_access)| {
            other.conflicts(&place)
                && !matches!(
                    (*other_access, argument.access),
                    (AccessMode::Read, AccessMode::Read)
                )
        }) {
            return defect("lowered call contains overlapping Resource loans");
        }
        active_places.push((place, argument.access));
    }
    Ok(())
}

fn reorder_types(
    source: &[Type],
    source_to_parameter: &[u16],
) -> Result<Vec<Type>, VerificationFailure> {
    if source.len() != source_to_parameter.len() {
        return defect("call argument binding length disagrees with operands");
    }
    let mut ordered = vec![None; source.len()];
    for (value, parameter) in source.iter().zip(source_to_parameter) {
        let slot = ordered.get_mut(usize::from(*parameter)).ok_or_else(|| {
            VerificationFailure::Defect {
                evidence: Arc::from("call argument binding names an invalid parameter"),
            }
        })?;
        if slot.replace(value.clone()).is_some() {
            return defect("call argument binding initializes a parameter twice");
        }
    }
    ordered
        .into_iter()
        .map(|value| {
            value.ok_or_else(|| VerificationFailure::Defect {
                evidence: Arc::from("call argument binding omits a parameter"),
            })
        })
        .collect()
}

fn validate_input(input: &ProgramInput) -> Result<(), VerificationFailure> {
    for (lookup, function) in &input.functions {
        if lookup != &function.id {
            return defect("function catalog key disagrees with typed identity");
        }
        if function.name.is_empty() {
            return creator(CreatorFailureKind::EmptyName, &function.source);
        }
        let mut names = BTreeSet::new();
        if function
            .parameters
            .iter()
            .any(|parameter| !names.insert(&parameter.name))
        {
            return creator(CreatorFailureKind::DuplicateParameter, &function.source);
        }
        if function.source.start() > function.source.end() {
            return defect("reversed function provenance");
        }
    }
    Ok(())
}

fn verify_custody_join(
    left: MoveState,
    right: MoveState,
) -> Result<MoveState, VerificationFailure> {
    if left.mismatch(&right).is_some() {
        return defect("lowered control flow has path-dependent Resource custody");
    }
    Ok(left)
}

fn verify_reachable_custody(
    states: impl IntoIterator<Item = MoveState>,
) -> Result<Option<MoveState>, VerificationFailure> {
    let mut states = states.into_iter();
    let Some(mut joined) = states.next() else {
        return Ok(None);
    };
    for state in states {
        joined = verify_custody_join(joined, state)?;
    }
    Ok(Some(joined))
}

#[derive(Clone, Default)]
struct LoopCustodyEdges {
    breaks: Vec<MoveState>,
    continues: Vec<MoveState>,
    artifact_scope_exclusions: BTreeSet<LocalId>,
}

struct Lowerer<'a> {
    module: ModuleId,
    input: &'a ProgramInput,
    functions: &'a BTreeMap<DefinitionId, ResolvedFunction>,
    constants: &'a BTreeMap<DefinitionId, ResolvedConstant>,
    tests: &'a BTreeMap<TestId, ResolvedTest>,
    variants: &'a BTreeMap<VariantId, ResolvedVariant>,
    structs: &'a BTreeMap<DefinitionId, ResolvedStruct>,
    interfaces: &'a BTreeMap<DefinitionId, ResolvedInterface>,
    aliases: &'a BTreeMap<DefinitionId, Type>,
    namespace: &'a NamespaceCatalog,
    nominal_displays: &'a BTreeMap<DefinitionId, Arc<str>>,
    locals: BTreeMap<String, (LocalId, Type, bool)>,
    local_origins: BTreeMap<LocalId, SourceRange>,
    parameter_locals: BTreeSet<LocalId>,
    non_custodial_locals: BTreeSet<LocalId>,
    moved: MoveState,
    known_integers: BTreeMap<LocalId, i128>,
    loop_depth: usize,
    loop_custody: Vec<LoopCustodyEdges>,
    loop_scope_exclusions: Vec<BTreeSet<LocalId>>,
    next_local: u32,
    build_authority: &'a BuildAuthority,
    pool_authority: &'a PoolAuthority,
    identity_catalog: &'a mut IdentityCatalog,
    discharge_laws: &'a mut DischargeLawBuilder,
    cancellation: &'a Cancellation,
    specialization_demands: SpecializationDemands<'a>,
    concrete_context: bool,
    test_application_context: u16,
    expected_expression_type: Option<Type>,
    owner_identity: Option<DefinitionId>,
    generic_interface_bounds: BTreeMap<TypeParameterId, DefinitionId>,
    generic_const_values: BTreeMap<String, (IntegerType, u64)>,
}

struct SpecializationDemands<'a> {
    records: &'a mut BTreeMap<SpecializationId, SpecializationRecord>,
    pending: &'a mut BTreeSet<SpecializationId>,
}

impl<'a> Lowerer<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        module: ModuleId,
        input: &'a ProgramInput,
        authorities: AuthorityContext<'a>,
        identity_catalog: &'a mut IdentityCatalog,
        discharge_laws: &'a mut DischargeLawBuilder,
        cancellation: &'a Cancellation,
        specialization_demands: SpecializationDemands<'a>,
        concrete_context: bool,
    ) -> Self {
        Self {
            module,
            input,
            functions: &input.functions,
            constants: &input.constants,
            tests: &input.tests,
            variants: &input.variants,
            structs: &input.structs,
            interfaces: &input.interfaces,
            aliases: &input.aliases,
            namespace: &input.namespace,
            nominal_displays: &input.nominal_displays,
            locals: BTreeMap::new(),
            local_origins: BTreeMap::new(),
            parameter_locals: BTreeSet::new(),
            non_custodial_locals: BTreeSet::new(),
            moved: MoveState::default(),
            known_integers: BTreeMap::new(),
            loop_depth: 0,
            loop_custody: Vec::new(),
            loop_scope_exclusions: Vec::new(),
            next_local: 0,
            build_authority: authorities.build(),
            pool_authority: authorities.pool(),
            identity_catalog,
            discharge_laws,
            cancellation,
            specialization_demands,
            concrete_context,
            test_application_context: 0,
            expected_expression_type: None,
            owner_identity: None,
            generic_interface_bounds: BTreeMap::new(),
            generic_const_values: BTreeMap::new(),
        }
    }

    fn set_generic_context(
        &mut self,
        function: &ResolvedFunction,
        substitutions: Option<&BTreeMap<TypeParameterId, Type>>,
    ) {
        self.owner_identity = Some(function.id);
        self.generic_interface_bounds = function
            .type_parameters
            .iter()
            .copied()
            .zip(function.generic_constraints.iter())
            .filter_map(|(parameter, constraint)| match constraint {
                GenericConstraint::Type {
                    interface: Some(interface),
                } => Some((parameter, *interface)),
                _ => None,
            })
            .collect();
        self.generic_const_values = function
            .generic_parameter_names
            .iter()
            .zip(function.type_parameters.iter())
            .zip(function.generic_constraints.iter())
            .filter_map(|((name, parameter), constraint)| {
                let GenericConstraint::Const {
                    type_: Type::Integer(kind),
                } = constraint
                else {
                    return None;
                };
                let Type::ConstU64(value) = substitutions?.get(parameter)? else {
                    return None;
                };
                Some((name.clone(), (*kind, *value)))
            })
            .collect();
    }

    fn struct_fields(
        &mut self,
        definition: DefinitionId,
        arguments: &[Type],
        site: &SourceRange,
    ) -> Result<Arc<[ResolvedField]>, VerificationFailure> {
        let key = Arc::<[Type]>::from(arguments.to_vec());
        let (mut fields, selections, parameter_names, constraints) = {
            let struct_ = self
                .structs
                .get(&definition)
                .ok_or_else(|| creator_value(CreatorFailureKind::UnresolvedName, site))?;
            if struct_.field_selections.is_empty() || arguments.iter().any(type_has_placeholder) {
                return Ok(struct_.fields.clone().into());
            }
            if let Some(fields) = struct_.applied_fields.borrow().get(&key) {
                return Ok(fields.clone());
            }
            (
                struct_.fields.clone(),
                struct_.field_selections.clone(),
                struct_.generic_parameter_names.clone(),
                struct_.generic_constraints.clone(),
            )
        };
        let values = parameter_names
            .iter()
            .zip(constraints.iter())
            .zip(arguments.iter())
            .filter_map(|((name, constraint), argument)| {
                let GenericConstraint::Const {
                    type_: Type::Integer(kind),
                } = constraint
                else {
                    return None;
                };
                let Type::ConstU64(value) = argument else {
                    return None;
                };
                Some((name.clone(), (*kind, *value)))
            })
            .collect::<BTreeMap<_, _>>();
        for selection in selections.iter() {
            for branch in selection.branches.iter() {
                let selected = if let Some(condition) = &branch.condition {
                    self.evaluate_comptime_condition(condition, &values)?
                } else {
                    true
                };
                if selected {
                    fields.extend(branch.fields.iter().cloned());
                    break;
                }
            }
        }
        let fields = Arc::<[ResolvedField]>::from(fields);
        self.structs
            .get(&definition)
            .expect("selected struct remains catalogued")
            .applied_fields
            .borrow_mut()
            .insert(key, fields.clone());
        Ok(fields)
    }

    fn evaluate_comptime_condition(
        &mut self,
        condition: &ExpressionSyntax,
        generic_const_values: &BTreeMap<String, (IntegerType, u64)>,
    ) -> Result<bool, VerificationFailure> {
        let (program, expression) = verify_comptime_condition_with_values(
            self.input,
            self.module,
            condition,
            self.build_authority,
            self.pool_authority,
            self.identity_catalog,
            self.cancellation,
            generic_const_values,
        )?;
        let run = crate::evaluator::Engine::new(&program, self.cancellation)
            .evaluate_expression(&expression);
        match run.outcome {
            crate::EvaluationOutcome::Completed(crate::CanonicalValue::Bool(value)) => Ok(value),
            crate::EvaluationOutcome::Cancelled => Err(VerificationFailure::Cancelled),
            crate::EvaluationOutcome::Defect { evidence } => {
                Err(VerificationFailure::Defect { evidence })
            }
            _ => creator(
                CreatorFailureKind::InvalidComptimeExpression,
                &condition.range,
            ),
        }
    }

    fn place_type(&self, place: &Place) -> Option<Type> {
        place
            .projections
            .last()
            .map(|projection| match projection {
                PlaceProjection::Field { type_, .. } | PlaceProjection::Index { type_, .. } => {
                    type_.clone()
                }
            })
            .or_else(|| {
                self.locals
                    .values()
                    .find_map(|(local, type_, _)| (*local == place.local).then(|| type_.clone()))
            })
    }

    fn restore_place_custody(
        &mut self,
        place: &Place,
        site: &SourceRange,
    ) -> Result<(), VerificationFailure> {
        self.moved.restore_place(place);
        for projection_count in (0..place.projections.len()).rev() {
            let aggregate = Place {
                local: place.local,
                projections: place.projections[..projection_count].into(),
            };
            let Some(Type::Nominal {
                definition,
                arguments,
                ..
            }) = self.place_type(&aggregate)
            else {
                continue;
            };
            let parameters = self.structs[&definition]
                .type_parameters
                .iter()
                .copied()
                .zip(arguments.iter().cloned())
                .collect::<BTreeMap<_, _>>();
            let fields = self.struct_fields(definition, &arguments, site)?;
            let children = fields
                .iter()
                .map(|field| {
                    let mut projections = aggregate.projections.to_vec();
                    projections.push(PlaceProjection::Field {
                        definition,
                        name: Arc::from(field.name.as_str()),
                        type_: substitute(&field.type_, &parameters),
                        mutable: field.mutable,
                    });
                    Place {
                        local: aggregate.local,
                        projections: projections.into(),
                    }
                })
                .collect::<Vec<_>>();
            self.moved
                .finish_aggregate_restoration(&aggregate, &children);
        }
        Ok(())
    }

    fn bind_local(&mut self, name: &str, type_: Type) -> Result<LocalId, VerificationFailure> {
        let id = LocalId(self.next_local);
        self.next_local =
            self.next_local
                .checked_add(1)
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from("local identity overflow"),
                })?;
        self.locals.insert(name.to_owned(), (id, type_, false));
        Ok(id)
    }

    fn bind_source_local(
        &mut self,
        name: &str,
        type_: Type,
        mutable: bool,
        site: &SourceRange,
    ) -> Result<LocalId, VerificationFailure> {
        if self.locals.contains_key(name) {
            return creator(CreatorFailureKind::DuplicateLocal, site);
        }
        let id = self.bind_local(name, type_)?;
        self.locals.get_mut(name).expect("new local").2 = mutable;
        self.local_origins.insert(id, site.clone());
        Ok(id)
    }

    fn bind_parameter(
        &mut self,
        name: &str,
        type_: Type,
        ownership: OwnershipSyntax,
    ) -> Result<LocalId, VerificationFailure> {
        let id = self.bind_local(name, type_)?;
        match ownership {
            OwnershipSyntax::Take => {
                self.parameter_locals.insert(id);
            }
            OwnershipSyntax::Read | OwnershipSyntax::Mut => {
                self.non_custodial_locals.insert(id);
            }
            OwnershipSyntax::Value => {}
        }
        self.locals.get_mut(name).expect("new parameter").2 =
            matches!(ownership, OwnershipSyntax::Mut | OwnershipSyntax::Take);
        Ok(id)
    }

    fn lower_place(
        &mut self,
        syntax: &PlaceSyntax,
    ) -> Result<(Place, Type, bool), VerificationFailure> {
        let Some((local, mut type_, mut writable)) = self.locals.get(&syntax.root).cloned() else {
            return creator(CreatorFailureKind::UnresolvedName, &syntax.range);
        };
        let mut projections = Vec::with_capacity(syntax.projections.len());
        for projection in &syntax.projections {
            match projection {
                PlaceProjectionSyntax::Field { name, range } => {
                    let Type::Nominal {
                        definition,
                        arguments,
                        ..
                    } = &type_
                    else {
                        return creator(CreatorFailureKind::UnresolvedName, range);
                    };
                    let struct_module = self
                        .structs
                        .get(definition)
                        .map(|struct_| struct_.module)
                        .ok_or_else(|| creator_value(CreatorFailureKind::UnresolvedName, range))?;
                    let fields = self.struct_fields(*definition, arguments, range)?;
                    let Some(field) = fields.iter().find(|field| field.name == *name) else {
                        return creator(CreatorFailureKind::UnresolvedName, range);
                    };
                    if !field.public && struct_module != self.module {
                        return creator(CreatorFailureKind::UnresolvedName, range);
                    }
                    let substitutions = self.structs[definition]
                        .type_parameters
                        .iter()
                        .copied()
                        .zip(arguments.iter().cloned())
                        .collect::<BTreeMap<_, _>>();
                    let field_type = substitute(&field.type_, &substitutions);
                    writable &= field.mutable;
                    projections.push(PlaceProjection::Field {
                        definition: *definition,
                        name: Arc::from(name.as_str()),
                        type_: field_type.clone(),
                        mutable: field.mutable,
                    });
                    type_ = field_type;
                }
                PlaceProjectionSyntax::Index(index) => {
                    let index = self.expression(index)?;
                    if !matches!(index.type_, Type::Integer(_)) {
                        return creator(CreatorFailureKind::ArgumentTypeMismatch, &index.source);
                    }
                    let element = match &type_ {
                        Type::Array(element) | Type::FixedArray { element, .. } => {
                            (**element).clone()
                        }
                        _ => {
                            return creator(
                                CreatorFailureKind::ArgumentTypeMismatch,
                                &index.source,
                            );
                        }
                    };
                    projections.push(PlaceProjection::Index {
                        index: Box::new(index),
                        type_: element.clone(),
                    });
                    type_ = element;
                }
            }
        }
        Ok((
            Place {
                local,
                projections: projections.into(),
            },
            type_,
            writable,
        ))
    }

    fn while_iteration_bound(
        &self,
        condition: &ExpressionSyntax,
        body: &[StatementSyntax],
    ) -> Option<u64> {
        let ExpressionSyntaxKind::Binary {
            operator: condition_operator,
            left,
            right,
        } = &condition.kind
        else {
            return None;
        };
        let ExpressionSyntaxKind::Name(NameSyntax { segments }) = &left.kind else {
            return None;
        };
        let [name] = segments.as_slice() else {
            return None;
        };
        let limit = syntax_integer_value(right)?;
        let (local, _, mutable) = self.locals.get(name)?;
        if !mutable {
            return None;
        }
        let initial = *self.known_integers.get(local)?;
        let StatementSyntax::Assign {
            place,
            mutable_binding: false,
            declared_type: None,
            operator: update_operator,
            value,
            ..
        } = body.first()?
        else {
            return None;
        };
        if place.root != *name || !place.projections.is_empty() {
            return None;
        }
        let (update, step) = if let Some(update) = update_operator {
            (*update, syntax_integer_value(value)?)
        } else {
            let ExpressionSyntaxKind::Binary {
                operator,
                left,
                right,
            } = &value.kind
            else {
                return None;
            };
            let ExpressionSyntaxKind::Name(NameSyntax { segments }) = &left.kind else {
                return None;
            };
            if segments.as_slice() != [name.as_str()] {
                return None;
            }
            (*operator, syntax_integer_value(right)?)
        };
        if step <= 0 {
            return None;
        }
        let iterations = match (*condition_operator, update) {
            (BinaryOperatorSyntax::Less, BinaryOperatorSyntax::Add) => {
                if initial >= limit {
                    0
                } else {
                    (limit - initial + step - 1) / step
                }
            }
            (BinaryOperatorSyntax::LessEqual, BinaryOperatorSyntax::Add) => {
                if initial > limit {
                    0
                } else {
                    (limit - initial) / step + 1
                }
            }
            (BinaryOperatorSyntax::Greater, BinaryOperatorSyntax::Subtract) => {
                if initial <= limit {
                    0
                } else {
                    (initial - limit + step - 1) / step
                }
            }
            (BinaryOperatorSyntax::GreaterEqual, BinaryOperatorSyntax::Subtract) => {
                if initial < limit {
                    0
                } else {
                    (initial - limit) / step + 1
                }
            }
            _ => return None,
        };
        u64::try_from(iterations).ok()
    }

    fn statements(
        &mut self,
        syntax: &[StatementSyntax],
        return_type: &Type,
    ) -> Result<Vec<Statement>, VerificationFailure> {
        let mut statements = Vec::new();
        let mut reachable = true;
        for statement in syntax {
            if self.cancellation.is_cancelled() {
                return Err(VerificationFailure::Cancelled);
            }
            let unreachable_state = (!reachable).then(|| {
                (
                    self.moved.clone(),
                    self.known_integers.clone(),
                    self.loop_custody.clone(),
                )
            });
            if let StatementSyntax::Comptime { branches, range } = statement {
                if self.concrete_context {
                    let selected = self.select_comptime_statements(branches)?;
                    let selected_falls_through = syntax_statements_fall_through(&selected);
                    statements.extend(self.statements(&selected, return_type)?);
                    if reachable {
                        reachable = selected_falls_through;
                    }
                } else {
                    statements.push(Statement::Pass(range.clone()));
                }
                if let Some((moved, known_integers, loop_custody)) = unreachable_state {
                    self.moved = moved;
                    self.known_integers = known_integers;
                    self.loop_custody = loop_custody;
                }
                continue;
            }
            statements.push(match statement {
                StatementSyntax::Return { value, range } => {
                    let mut value = value
                        .as_ref()
                        .map(|value| self.expression_expected(value, return_type))
                        .transpose()?;
                    let actual = value.as_ref().map_or(&Type::Unit, |value| &value.type_);
                    if !can_return(actual, return_type) {
                        return creator(CreatorFailureKind::ReturnTypeMismatch, range);
                    }
                    if let Some(value) = &mut value
                        && matches!(
                            value.kind,
                            ExpressionKind::Call {
                                target: CallTarget::BuiltinVariant(
                                    BuiltinVariant::ResultOk | BuiltinVariant::ResultErr
                                ),
                                ..
                            }
                        )
                    {
                        value.type_ = return_type.clone();
                        value.type_id = self.identity_catalog.intern_type(return_type).map_err(
                            |collision| VerificationFailure::Defect {
                                evidence: Arc::from(format!(
                                    "type identity collision {:032x}",
                                    collision.digest
                                )),
                            },
                        )?;
                    }
                    if let Some(value) = &value {
                        self.require_owned_transfer(value)?;
                    }
                    self.check_recoverable_exit()?;
                    Statement::Return {
                        value,
                        source: range.clone(),
                    }
                }
                StatementSyntax::Panic { value, range } => Statement::Panic {
                    value: self.expression(value)?,
                    source: range.clone(),
                },
                StatementSyntax::Assert { condition, range } => {
                    let condition = self.expression(condition)?;
                    if condition.type_ != Type::Bool {
                        return creator(CreatorFailureKind::IfConditionRequiresBool, range);
                    }
                    Statement::Assert {
                        condition,
                        source: range.clone(),
                    }
                }
                StatementSyntax::Expect { condition, range } => {
                    let condition = self.expression(condition)?;
                    if condition.type_ != Type::Bool {
                        return creator(CreatorFailureKind::ExpectRequiresBool, range);
                    }
                    Statement::Expect {
                        condition,
                        source: range.clone(),
                    }
                }
                StatementSyntax::Assign {
                    place: place_syntax,
                    mutable_binding,
                    declared_type,
                    operator,
                    value,
                    range,
                } => {
                    let name = &place_syntax.root;
                    let declared_type = declared_type
                        .as_ref()
                        .map(|type_| {
                            self.resolve_local_type(type_).ok_or_else(|| {
                                creator_value(CreatorFailureKind::UnresolvedType, range)
                            })
                        })
                        .transpose()?;
                    let existing = self.locals.contains_key(name);
                    let lowered_place = if existing {
                        Some(self.lower_place(place_syntax)?)
                    } else {
                        None
                    };
                    let expected = declared_type
                        .as_ref()
                        .or_else(|| lowered_place.as_ref().map(|(_, type_, _)| type_));
                    let mut value = if let Some(expected) = expected.cloned() {
                        self.expression_expected(value, &expected)?
                    } else {
                        self.expression(value)?
                    };
                    if let (Some(operator), Some((place, place_type, _))) =
                        (operator, lowered_place.as_ref())
                    {
                        if self.moved.is_unreadable(place) {
                            return Err(self.custody_failure(
                                CreatorFailureKind::ReadAfterMove,
                                range,
                                &PlaceKey::from_place(place),
                                CustodyDiagnosticState::Moved,
                                self.moved.origin(place).cloned(),
                            ));
                        }
                        let left = self.finish_expression(
                            ExpressionKind::Read(place.clone()),
                            place_type.clone(),
                            place_syntax.range.clone(),
                        )?;
                        let operator = BinaryOperator::from(*operator);
                        self.validate_binary_capability(operator, &left.type_, range)?;
                        let result_type = binary_type(operator, &left.type_, &value.type_)
                            .ok_or_else(|| {
                                creator_value(CreatorFailureKind::BinaryTypeMismatch, range)
                            })?;
                        value = self.finish_expression(
                            ExpressionKind::Binary {
                                operator,
                                left: Box::new(left),
                                right: Box::new(value),
                            },
                            result_type,
                            range.clone(),
                        )?;
                    }
                    if let Some(declared_type) = &declared_type
                        && !can_initialize(&value.type_, declared_type)
                    {
                        return creator(CreatorFailureKind::ArgumentTypeMismatch, range);
                    }
                    self.require_owned_transfer(&value)?;
                    if name == "_" {
                        if !place_syntax.projections.is_empty()
                            || operator.is_some()
                            || declared_type.is_some()
                            || *mutable_binding
                        {
                            return creator(CreatorFailureKind::UnresolvedName, range);
                        }
                        if self.is_must_use(&value.type_) {
                            return creator(CreatorFailureKind::MustUseValue, range);
                        }
                        Statement::Evaluate(value)
                    } else if let Some((place, type_, writable)) = lowered_place {
                        let known_integer = expression_integer_value(&value);
                        if *mutable_binding || declared_type.is_some() {
                            return creator(CreatorFailureKind::DuplicateLocal, range);
                        }
                        if !can_initialize(&value.type_, &type_) {
                            return creator(CreatorFailureKind::ArgumentTypeMismatch, range);
                        }
                        let uninitialized = self.moved.is_uninitialized(&place);
                        if self.type_owns_resource(&type_) && !uninitialized {
                            return Err(self.custody_failure(
                                CreatorFailureKind::ResourceReplacementRequiresDischarge,
                                range,
                                &PlaceKey::from_place(&place),
                                CustodyDiagnosticState::Initialized,
                                None,
                            ));
                        }
                        if !writable && !uninitialized {
                            return creator(CreatorFailureKind::ImmutableReassignment, range);
                        }
                        self.restore_place_custody(&place, range)?;
                        if place.projections.is_empty() {
                            if let Some(value) = known_integer {
                                self.known_integers.insert(place.local, value);
                            } else {
                                self.known_integers.remove(&place.local);
                            }
                        }
                        Statement::Assign {
                            place,
                            value,
                            source: range.clone(),
                        }
                    } else {
                        let known_integer = expression_integer_value(&value);
                        if !place_syntax.projections.is_empty() || operator.is_some() {
                            return creator(CreatorFailureKind::UnresolvedName, range);
                        }
                        let place = self.bind_source_local(
                            name,
                            declared_type.unwrap_or_else(|| value.type_.clone()),
                            *mutable_binding,
                            &place_syntax.range,
                        )?;
                        if let Some(value) = known_integer {
                            self.known_integers.insert(place, value);
                        }
                        Statement::Initialize {
                            place: Place::local(place),
                            value,
                            source: range.clone(),
                        }
                    }
                }
                StatementSyntax::Evaluate(expression) => {
                    let expression = self.expression(expression)?;
                    if self.is_must_use(&expression.type_) {
                        return creator(CreatorFailureKind::MustUseValue, &expression.source);
                    }
                    Statement::Evaluate(expression)
                }
                StatementSyntax::If {
                    condition,
                    then_branch,
                    else_branch,
                    range,
                } => {
                    if let ExpressionSyntaxKind::Is { value, pattern } = &condition.kind {
                        let value = self.expression(value)?;
                        let before = self.locals.clone();
                        let visible_before = before
                            .values()
                            .map(|(local, _, _)| *local)
                            .collect::<BTreeSet<_>>();
                        let moved_before = self.moved.clone();
                        let known_before = self.known_integers.clone();
                        let pattern = self
                            .lower_match_pattern(pattern, &value.type_)?
                            .ok_or_else(|| {
                                creator_value(CreatorFailureKind::InvalidMatchPattern, range)
                            })?;
                        if let Some(place) = root_place(&value) {
                            for moved in self.pattern_move_keys(
                                &pattern,
                                &value.type_,
                                &PlaceKey::from_place(&place),
                            )? {
                                self.moved.move_key_at(moved, range);
                            }
                        }
                        let then_terminates = !syntax_statements_fall_through(then_branch);
                        let else_terminates = !syntax_statements_fall_through(else_branch);
                        let then_branch = self.statements(then_branch, return_type)?;
                        if !then_terminates {
                            self.check_recoverable_exit_excluding(&visible_before)?;
                        }
                        let then_moved = self.moved.clone();
                        let then_known = self.known_integers.clone();
                        self.locals.clone_from(&before);
                        self.moved.clone_from(&moved_before);
                        self.known_integers.clone_from(&known_before);
                        let else_branch = self.statements(else_branch, return_type)?;
                        if !else_terminates {
                            self.check_recoverable_exit_excluding(&visible_before)?;
                        }
                        let else_moved = self.moved.clone();
                        let else_known = self.known_integers.clone();
                        self.locals = before;
                        self.known_integers = match (then_terminates, else_terminates) {
                            (false, false) => join_known_integers(&[then_known, else_known]),
                            (false, true) => then_known,
                            (true, false) => else_known,
                            (true, true) => known_before,
                        };
                        self.moved = self.select_branch_custody(
                            moved_before,
                            then_moved,
                            else_moved,
                            then_terminates,
                            else_terminates,
                            range,
                        )?;
                        Statement::IfPattern {
                            value,
                            pattern,
                            then_branch: then_branch.into(),
                            else_branch: else_branch.into(),
                            source: range.clone(),
                        }
                    } else {
                        let condition = self.expression(condition)?;
                        if condition.type_ != Type::Bool {
                            return creator(CreatorFailureKind::IfConditionRequiresBool, range);
                        }
                        let before = self.locals.clone();
                        let visible_before = before
                            .values()
                            .map(|(local, _, _)| *local)
                            .collect::<BTreeSet<_>>();
                        let moved_before = self.moved.clone();
                        let known_before = self.known_integers.clone();
                        let then_terminates = !syntax_statements_fall_through(then_branch);
                        let else_terminates = !syntax_statements_fall_through(else_branch);
                        let then_branch = self.statements(then_branch, return_type)?;
                        if !then_terminates {
                            self.check_recoverable_exit_excluding(&visible_before)?;
                        }
                        let then_moved = self.moved.clone();
                        let then_known = self.known_integers.clone();
                        self.locals.clone_from(&before);
                        self.moved.clone_from(&moved_before);
                        self.known_integers.clone_from(&known_before);
                        let else_branch = self.statements(else_branch, return_type)?;
                        if !else_terminates {
                            self.check_recoverable_exit_excluding(&visible_before)?;
                        }
                        let else_moved = self.moved.clone();
                        let else_known = self.known_integers.clone();
                        self.locals = before;
                        self.known_integers = match (then_terminates, else_terminates) {
                            (false, false) => join_known_integers(&[then_known, else_known]),
                            (false, true) => then_known,
                            (true, false) => else_known,
                            (true, true) => known_before,
                        };
                        self.moved = self.select_branch_custody(
                            moved_before,
                            then_moved,
                            else_moved,
                            then_terminates,
                            else_terminates,
                            range,
                        )?;
                        Statement::If {
                            condition,
                            then_branch: then_branch.into(),
                            else_branch: else_branch.into(),
                            source: range.clone(),
                        }
                    }
                }
                StatementSyntax::For {
                    pattern,
                    iterable,
                    body,
                    range,
                } => {
                    let iterable = self.expression(iterable)?;
                    let moved_before = self.moved.clone();
                    let exact_iterations = match &iterable.type_ {
                        Type::FixedArray { length, .. } => length.value(),
                        _ => None,
                    };
                    let element = match &iterable.type_ {
                        Type::Array(element) | Type::FixedArray { element, .. } => element,
                        _ => return creator(CreatorFailureKind::ArgumentTypeMismatch, range),
                    };
                    let visible = self.locals.keys().cloned().collect::<BTreeSet<_>>();
                    let visible_locals = self
                        .locals
                        .values()
                        .map(|(local, _, _)| *local)
                        .collect::<BTreeSet<_>>();
                    let known_before = self.known_integers.clone();
                    let pattern = self.lower_match_pattern(pattern, element)?.ok_or_else(|| {
                        creator_value(CreatorFailureKind::InvalidMatchPattern, range)
                    })?;
                    if !self.pattern_irrefutable(&pattern, element) {
                        return creator(CreatorFailureKind::InvalidMatchPattern, range);
                    }
                    let iteration_transfers = match (exact_iterations, root_place(&iterable)) {
                        (Some(iterations), Some(place)) => (0..iterations)
                            .map(|index| {
                                let mut projections =
                                    PlaceKey::from_place(&place).projections.to_vec();
                                projections.push(ProjectionKey::Index(Some(i128::from(index))));
                                self.pattern_move_keys(
                                    &pattern,
                                    element,
                                    &PlaceKey {
                                        local: place.local,
                                        projections: projections.into(),
                                    },
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                        _ => Vec::new(),
                    };
                    if let Some(first) = iteration_transfers.first() {
                        for moved in first {
                            if !self.moved.move_key_at(moved.clone(), range) {
                                return Err(self.custody_failure(
                                    CreatorFailureKind::ReadAfterMove,
                                    range,
                                    moved,
                                    CustodyDiagnosticState::Moved,
                                    None,
                                ));
                            }
                        }
                    }
                    let iteration_entry_moved = self.moved.clone();
                    let binding_places = match_pattern_binding_signature(&pattern)
                        .keys()
                        .copied()
                        .map(Place::local)
                        .collect::<Vec<_>>();
                    let body_falls_through = syntax_statements_fall_through(body);
                    self.loop_depth += 1;
                    self.loop_custody.push(LoopCustodyEdges::default());
                    self.loop_scope_exclusions.push(visible_locals.clone());
                    let body = self.statements(body, return_type);
                    if body.is_ok() && body_falls_through {
                        self.check_recoverable_exit_excluding(&visible_locals)?;
                    }
                    self.loop_scope_exclusions
                        .pop()
                        .expect("for loop discharge scope was pushed");
                    let mut edges = self
                        .loop_custody
                        .pop()
                        .expect("for loop custody scope was pushed");
                    self.loop_depth -= 1;
                    let body = body?;
                    self.locals.retain(|name, _| visible.contains(name));
                    for binding in &binding_places {
                        self.moved.restore_place(binding);
                        for state in edges.breaks.iter_mut().chain(edges.continues.iter_mut()) {
                            state.restore_place(binding);
                        }
                    }
                    let mut continuing = edges.continues;
                    if body_falls_through {
                        continuing.push(self.moved.clone());
                    }
                    if exact_iterations.is_none_or(|iterations| iterations > 1) {
                        for state in &continuing {
                            self.join_custody(iteration_entry_moved.clone(), state.clone(), range)?;
                        }
                    }
                    for state in &mut continuing {
                        for moved in iteration_transfers.iter().skip(1).flatten() {
                            if !state.move_key(moved.clone()) {
                                return defect(
                                    "exact for transfer repeats an unavailable Resource place",
                                );
                            }
                        }
                    }
                    let mut exits = edges.breaks;
                    match exact_iterations {
                        Some(0) => exits.push(moved_before.clone()),
                        Some(1) => exits.extend(continuing),
                        Some(_) => exits.extend(continuing),
                        None => exits.push(moved_before.clone()),
                    }
                    self.moved = self
                        .join_reachable_custody(exits, range)?
                        .unwrap_or(moved_before);
                    self.known_integers =
                        join_known_integers(&[known_before, self.known_integers.clone()]);
                    for binding in &binding_places {
                        self.known_integers.remove(&binding.local);
                    }
                    Statement::For {
                        pattern,
                        iterable,
                        body: body.into(),
                        source: range.clone(),
                    }
                }
                StatementSyntax::While {
                    condition,
                    body,
                    range,
                } => {
                    let max_iterations = self
                        .while_iteration_bound(condition, body)
                        .ok_or_else(|| creator_value(CreatorFailureKind::UnboundedWhile, range))?;
                    let condition = self.expression(condition)?;
                    if condition.type_ != Type::Bool {
                        return creator(CreatorFailureKind::IfConditionRequiresBool, range);
                    }
                    let moved_before = self.moved.clone();
                    let visible_locals = self
                        .locals
                        .values()
                        .map(|(local, _, _)| *local)
                        .collect::<BTreeSet<_>>();
                    let known_before = self.known_integers.clone();
                    let body_falls_through = syntax_statements_fall_through(body);
                    self.loop_depth += 1;
                    self.loop_custody.push(LoopCustodyEdges::default());
                    self.loop_scope_exclusions.push(visible_locals.clone());
                    let body = self.statements(body, return_type);
                    if body.is_ok() && body_falls_through {
                        self.check_recoverable_exit_excluding(&visible_locals)?;
                    }
                    self.loop_scope_exclusions
                        .pop()
                        .expect("while loop discharge scope was pushed");
                    let edges = self
                        .loop_custody
                        .pop()
                        .expect("while loop custody scope was pushed");
                    self.loop_depth -= 1;
                    let body = body?;
                    let mut continuing = edges.continues;
                    if body_falls_through {
                        continuing.push(self.moved.clone());
                    }
                    for state in continuing {
                        self.join_custody(moved_before.clone(), state, range)?;
                    }
                    let mut exits = edges.breaks;
                    exits.push(moved_before.clone());
                    self.moved = self
                        .join_reachable_custody(exits, range)?
                        .unwrap_or(moved_before);
                    self.known_integers =
                        join_known_integers(&[known_before, self.known_integers.clone()]);
                    Statement::While {
                        condition,
                        body: body.into(),
                        max_iterations,
                        source: range.clone(),
                    }
                }
                StatementSyntax::Break(range) => {
                    if self.loop_depth == 0 {
                        return creator(CreatorFailureKind::LoopControlOutsideLoop, range);
                    }
                    let excluded = self
                        .loop_scope_exclusions
                        .last()
                        .cloned()
                        .expect("loop depth and discharge scopes agree");
                    self.check_recoverable_exit_excluding(&excluded)?;
                    self.loop_custody
                        .last_mut()
                        .expect("loop depth and custody scopes agree")
                        .breaks
                        .push(self.moved.clone());
                    Statement::Break(range.clone())
                }
                StatementSyntax::Continue(range) => {
                    if self.loop_depth == 0 {
                        return creator(CreatorFailureKind::LoopControlOutsideLoop, range);
                    }
                    let excluded = self
                        .loop_scope_exclusions
                        .last()
                        .cloned()
                        .expect("loop depth and discharge scopes agree");
                    self.check_recoverable_exit_excluding(&excluded)?;
                    self.loop_custody
                        .last_mut()
                        .expect("loop depth and custody scopes agree")
                        .continues
                        .push(self.moved.clone());
                    Statement::Continue(range.clone())
                }
                StatementSyntax::Match {
                    value,
                    cases,
                    range,
                } => {
                    let value = self.expression(value)?;
                    let before = self.locals.clone();
                    let visible_before = before
                        .values()
                        .map(|(local, _, _)| *local)
                        .collect::<BTreeSet<_>>();
                    let moved_before = self.moved.clone();
                    let known_before = self.known_integers.clone();
                    let mut lowered = Vec::new();
                    let mut continuing_custody = Vec::new();
                    let mut continuing_known = Vec::new();
                    let mut pattern_keys = BTreeSet::new();
                    for (index, case) in cases.iter().enumerate() {
                        if matches!(
                            case.pattern.kind,
                            PatternSyntaxKind::Wildcard | PatternSyntaxKind::Binding(_)
                        ) && case.guard.is_none()
                            && index + 1 != cases.len()
                        {
                            return creator(CreatorFailureKind::NonExhaustiveMatch, &case.range);
                        }
                        self.locals.clone_from(&before);
                        self.moved.clone_from(&moved_before);
                        self.known_integers.clone_from(&known_before);
                        let pattern = self.lower_match_pattern(&case.pattern, &value.type_)?;
                        if let (Some(pattern), Some(place)) = (&pattern, root_place(&value)) {
                            for moved in self.pattern_move_keys(
                                pattern,
                                &value.type_,
                                &PlaceKey::from_place(&place),
                            )? {
                                self.moved.move_key_at(moved, &case.pattern.range);
                            }
                        }
                        if let Some(pattern) = &pattern
                            && case.guard.is_none()
                            && !pattern_keys.insert(match_pattern_key(pattern))
                        {
                            return creator(CreatorFailureKind::UnreachableMatchCase, &case.range);
                        }
                        let guard = case
                            .guard
                            .as_ref()
                            .map(|guard| self.expression(guard))
                            .transpose()?;
                        if guard
                            .as_ref()
                            .is_some_and(|guard| guard.type_ != Type::Bool)
                        {
                            return creator(
                                CreatorFailureKind::IfConditionRequiresBool,
                                &case.range,
                            );
                        }
                        let body = self.statements(&case.body, return_type)?;
                        if syntax_statements_fall_through(&case.body) {
                            self.check_recoverable_exit_excluding(&visible_before)?;
                            continuing_custody.push(self.moved.clone());
                            continuing_known.push(self.known_integers.clone());
                        }
                        lowered.push(HirMatchCase {
                            pattern,
                            guard,
                            body: body.into(),
                            source: case.range.clone(),
                        });
                    }
                    let exhaustive = self.match_is_exhaustive(&value.type_, &lowered);
                    if !exhaustive {
                        return creator(CreatorFailureKind::NonExhaustiveMatch, range);
                    }
                    self.locals = before;
                    self.moved = if let Some(first) = continuing_custody.first().cloned() {
                        continuing_custody
                            .into_iter()
                            .skip(1)
                            .try_fold(first, |joined, branch| {
                                self.join_custody(joined, branch, range)
                            })?
                    } else {
                        moved_before
                    };
                    self.known_integers = if continuing_known.is_empty() {
                        known_before
                    } else {
                        join_known_integers(&continuing_known)
                    };
                    Statement::Match {
                        value,
                        cases: lowered.into(),
                        source: range.clone(),
                    }
                }
                StatementSyntax::Defer { expression, range } => {
                    let expression = self.expression(expression)?;
                    if matches!(expression.type_, Type::Result { .. })
                        || expression_contains_propagation(&expression)
                    {
                        return creator(CreatorFailureKind::DeferReturnsRecoverableError, range);
                    }
                    if self.type_owns_resource(&expression.type_) {
                        return creator(CreatorFailureKind::MustUseValue, range);
                    }
                    if !matches!(expression.kind, ExpressionKind::Call { .. })
                        && self.expression_contains_resource_move(&expression)
                    {
                        return creator(
                            CreatorFailureKind::CleanupResourceCaptureRequiresCall,
                            range,
                        );
                    }
                    if self.cleanup_has_conditional_resource_move(&expression) {
                        return creator(
                            CreatorFailureKind::CleanupResourceCaptureRequiresCall,
                            range,
                        );
                    }
                    if let Some(argument) = self.cleanup_resource_loan(&expression) {
                        let place = root_place(&argument).ok_or_else(|| {
                            creator_value(CreatorFailureKind::LoanAcrossCleanup, &argument.source)
                        })?;
                        return Err(self.custody_failure(
                            CreatorFailureKind::LoanAcrossCleanup,
                            range,
                            &PlaceKey::from_place(&place),
                            CustodyDiagnosticState::Loaned,
                            Some(argument.source.clone()),
                        ));
                    }
                    let mut captures = Vec::new();
                    let expression =
                        self.normalize_cleanup_expression(expression, &mut captures)?;
                    let action = CleanupAction {
                        expression,
                        captures: captures.into(),
                    };
                    Statement::Defer {
                        action,
                        source: range.clone(),
                    }
                }
                StatementSyntax::With {
                    scope,
                    binding,
                    body,
                    range,
                } => {
                    let Some(binding) = binding else {
                        return creator(CreatorFailureKind::UnsupportedLayerOneSyntax, range);
                    };
                    let scope = self.expression(scope)?;
                    let definition = match &scope.kind {
                        ExpressionKind::Call {
                            target:
                                CallTarget::Function { definition, .. }
                                | CallTarget::TemplateFunction { definition, .. },
                            ..
                        } => *definition,
                        _ => {
                            return creator(CreatorFailureKind::UnsupportedLayerOneSyntax, range);
                        }
                    };
                    if !self.pool_authority.is_scoped_factory(definition) {
                        return creator(CreatorFailureKind::UnsupportedLayerOneSyntax, range);
                    }
                    self.discharge_laws.authenticate_reclaim(scope.type_id)?;
                    let visible = self.locals.keys().cloned().collect::<BTreeSet<_>>();
                    let mut scope_exclusions = self
                        .locals
                        .values()
                        .map(|(local, _, _)| *local)
                        .collect::<BTreeSet<_>>();
                    let binding_place =
                        self.bind_source_local(binding, scope.type_.clone(), true, range)?;
                    scope_exclusions.insert(binding_place);
                    let body_falls_through = syntax_statements_fall_through(body);
                    let body = self.statements(body, return_type)?;
                    if body_falls_through {
                        self.check_recoverable_exit_excluding(&scope_exclusions)?;
                    }
                    self.locals.retain(|name, _| visible.contains(name));
                    self.moved.restore_local(binding_place);
                    self.known_integers.remove(&binding_place);
                    Statement::WithPool {
                        binding: Place::local(binding_place),
                        scope,
                        body: body.into(),
                        source: range.clone(),
                    }
                }
                StatementSyntax::Unsupported { range, .. } => {
                    return creator(CreatorFailureKind::UnsupportedLayerOneSyntax, range);
                }
                StatementSyntax::Comptime { range, .. } => Statement::Pass(range.clone()),
                StatementSyntax::Pass(range) => Statement::Pass(range.clone()),
            });
            if let Some((moved, known_integers, loop_custody)) = unreachable_state {
                self.moved = moved;
                self.known_integers = known_integers;
                self.loop_custody = loop_custody;
            } else {
                reachable = syntax_statements_fall_through(std::slice::from_ref(statement));
            }
        }
        Ok(statements)
    }

    fn select_comptime_statements(
        &mut self,
        branches: &[crate::syntax::ComptimeStatementBranch],
    ) -> Result<Vec<StatementSyntax>, VerificationFailure> {
        for branch in branches {
            let matches = if let Some(condition) = &branch.condition {
                let (program, expression) = verify_comptime_condition_with_values(
                    self.input,
                    self.module,
                    condition,
                    self.build_authority,
                    self.pool_authority,
                    self.identity_catalog,
                    self.cancellation,
                    &self.generic_const_values,
                )?;
                let run = crate::evaluator::Engine::new(&program, self.cancellation)
                    .evaluate_expression(&expression);
                match run.outcome {
                    crate::EvaluationOutcome::Completed(crate::CanonicalValue::Bool(value)) => {
                        value
                    }
                    crate::EvaluationOutcome::Cancelled => {
                        return Err(VerificationFailure::Cancelled);
                    }
                    crate::EvaluationOutcome::Defect { evidence } => {
                        return Err(VerificationFailure::Defect { evidence });
                    }
                    _ => {
                        return creator(
                            CreatorFailureKind::InvalidComptimeExpression,
                            &condition.range,
                        );
                    }
                }
            } else {
                true
            };
            if matches {
                return Ok(branch.statements.clone());
            }
        }
        Ok(Vec::new())
    }

    fn expression_expected(
        &mut self,
        syntax: &ExpressionSyntax,
        expected: &Type,
    ) -> Result<Expression, VerificationFailure> {
        let previous = self.expected_expression_type.replace(expected.clone());
        let mut expression = self.expression(syntax);
        self.expected_expression_type = previous;
        if let Ok(expression) = &mut expression {
            self.apply_expected_type(expression, expected)?;
            let observed = observe_type_tree(
                &expression.type_,
                self.identity_catalog,
                self.discharge_laws,
            )?;
            if observed != expression.type_id {
                return defect("coerced expression TypeId disagrees with its discharge type");
            }
        }
        expression
    }

    fn expression(&mut self, syntax: &ExpressionSyntax) -> Result<Expression, VerificationFailure> {
        if self.cancellation.is_cancelled() {
            return Err(VerificationFailure::Cancelled);
        }
        let (kind, type_) = match &syntax.kind {
            ExpressionSyntaxKind::Integer(authored) => {
                let (value, mut integer) = parse_integer_literal(authored).map_err(|()| {
                    creator_value(CreatorFailureKind::InvalidIntegerLiteral, &syntax.range)
                })?;
                if !integer_literal_has_suffix(authored)
                    && let Some(Type::Integer(expected)) = self.expected_expression_type
                    && expected.fits(value)
                {
                    integer = expected;
                }
                (
                    ExpressionKind::Literal(Literal::Integer {
                        kind: integer,
                        value,
                    }),
                    Type::Integer(integer),
                )
            }
            ExpressionSyntaxKind::Float(authored) => {
                let (value, float) = parse_float_literal(authored).map_err(|()| {
                    creator_value(CreatorFailureKind::InvalidFloatLiteral, &syntax.range)
                })?;
                (
                    ExpressionKind::Literal(Literal::Float {
                        kind: float,
                        bits: encode_float(float, value),
                    }),
                    Type::Float(float),
                )
            }
            ExpressionSyntaxKind::Text(value) => (
                ExpressionKind::Literal(Literal::Text(Arc::from(value.as_str()))),
                Type::Text,
            ),
            ExpressionSyntaxKind::Scalar(value) => (
                ExpressionKind::Literal(Literal::Scalar(*value)),
                Type::Scalar,
            ),
            ExpressionSyntaxKind::Bytes(value) => (
                ExpressionKind::Literal(Literal::Bytes(Arc::from(value.clone()))),
                Type::Bytes,
            ),
            ExpressionSyntaxKind::Bool(value) => {
                (ExpressionKind::Literal(Literal::Bool(*value)), Type::Bool)
            }
            ExpressionSyntaxKind::Unit => (ExpressionKind::Literal(Literal::Unit), Type::Unit),
            ExpressionSyntaxKind::Name(name) => return self.value(name, syntax),
            ExpressionSyntaxKind::Call { callee, arguments } => {
                let callable = match callee.segments.as_slice() {
                    [name]
                        if self.locals.get(name).is_some_and(|(_, type_, _)| {
                            matches!(type_, Type::Function { .. })
                        }) =>
                    {
                        Some(self.value(callee, syntax)?)
                    }
                    _ if matches!(
                        self.namespace.resolve(self.module, &callee.segments),
                        Some(ResolvedName::Constant(id))
                            if self.constants.get(&id).is_some_and(|constant| {
                                matches!(constant.type_, Type::Function { .. })
                            })
                    ) =>
                    {
                        Some(self.value(callee, syntax)?)
                    }
                    _ => None,
                };
                let receiver = match callee.segments.as_slice() {
                    [receiver, member] if self.locals.contains_key(receiver) => {
                        let receiver_expression = self.value(
                            &NameSyntax {
                                segments: vec![receiver.clone()],
                            },
                            syntax,
                        )?;
                        match receiver_expression.type_.clone() {
                            Type::Nominal { definition, .. } => {
                                let Some(ResolvedName::Function(id)) = self
                                    .namespace
                                    .resolve_member(self.module, definition, member)
                                else {
                                    return creator(
                                        CreatorFailureKind::UnresolvedCall,
                                        &syntax.range,
                                    );
                                };
                                Some((receiver_expression, Some(id), None))
                            }
                            Type::Any { interface, .. } => {
                                Some((receiver_expression, None, Some((interface, member.clone()))))
                            }
                            Type::Parameter { id, .. } => {
                                let interface =
                                    self.generic_interface_bounds.get(&id).copied().ok_or_else(
                                        || {
                                            creator_value(
                                                CreatorFailureKind::UnresolvedCall,
                                                &syntax.range,
                                            )
                                        },
                                    )?;
                                Some((receiver_expression, None, Some((interface, member.clone()))))
                            }
                            _ => {
                                return creator(CreatorFailureKind::UnresolvedCall, &syntax.range);
                            }
                        }
                    }
                    _ => None,
                };
                let build_primitive = self.resolve_build_primitive(callee);
                if build_primitive.is_some_and(|primitive| primitive.kind == BuildKind::Test) {
                    if arguments.len() != 1 {
                        return creator(CreatorFailureKind::ArgumentCount, &syntax.range);
                    }
                    if arguments[0].label.as_deref() != Some("cases") {
                        return creator(CreatorFailureKind::ArgumentLabelMismatch, &syntax.range);
                    }
                }
                let mut lowered = Vec::new();
                let mut labels = Vec::new();
                if let Some((receiver, _, _)) = &receiver {
                    lowered.push(receiver.clone());
                    labels.push(None);
                }
                let inside_test_cases =
                    build_primitive.is_some_and(|primitive| primitive.kind == BuildKind::Test);
                let scoped_pool_factory =
                    match self.namespace.resolve(self.module, &callee.segments) {
                        Some(ResolvedName::Function(definition)) => {
                            self.pool_authority.is_scoped_factory(definition)
                        }
                        _ => false,
                    };
                for argument in arguments {
                    let establishes_context =
                        inside_test_cases && argument.label.as_deref() == Some("cases");
                    if establishes_context {
                        self.test_application_context =
                            self.test_application_context.saturating_add(1);
                    }
                    let value = if scoped_pool_factory {
                        self.expression_expected(&argument.value, &Type::Integer(IntegerType::U64))
                    } else {
                        self.expression(&argument.value)
                    };
                    if establishes_context {
                        self.test_application_context -= 1;
                    }
                    lowered.push(value?);
                }
                labels.extend(
                    arguments
                        .iter()
                        .map(|argument| argument.label.clone())
                        .collect::<Vec<_>>(),
                );
                let (target, type_) = if let Some(callable) = callable {
                    if labels.iter().any(Option::is_some) {
                        return creator(CreatorFailureKind::ArgumentLabelMismatch, &syntax.range);
                    }
                    let Type::Function {
                        parameters,
                        return_type,
                    } = &callable.type_
                    else {
                        return defect("callable local lost its function type");
                    };
                    if parameters.len() != lowered.len() {
                        return creator(CreatorFailureKind::ArgumentCount, &syntax.range);
                    }
                    for (argument, expected) in lowered.iter_mut().zip(parameters.iter()) {
                        self.apply_expected_type(argument, expected)?;
                        if !can_pass(&argument.type_, expected) {
                            return creator(
                                CreatorFailureKind::ArgumentTypeMismatch,
                                &argument.source,
                            );
                        }
                    }
                    let return_type = (**return_type).clone();
                    (
                        CallTarget::Callable {
                            value: Box::new(callable),
                        },
                        return_type,
                    )
                } else if let Some((_, Some(id), _)) = &receiver {
                    self.function_call(*id, &lowered, &labels, &syntax.range)?
                } else if let Some((_, _, Some((interface, member)))) = &receiver {
                    self.interface_call(*interface, member, &lowered, &labels, &syntax.range)?
                } else {
                    self.call(callee, &lowered, &labels, &syntax.range)?
                };
                if let CallTarget::Function {
                    definition,
                    specialization,
                    argument_order,
                    ..
                } = &target
                {
                    let function = &self.functions[definition];
                    let substitutions = self
                        .specialization_demands
                        .records
                        .get(specialization)
                        .map(|record| {
                            function
                                .type_parameters
                                .iter()
                                .copied()
                                .zip(record.type_arguments.iter().cloned())
                                .collect::<BTreeMap<_, _>>()
                        })
                        .unwrap_or_default();
                    let parameter_context = argument_order
                        .iter()
                        .map(|parameter| {
                            let parameter = &function.parameters[usize::from(*parameter)];
                            (
                                substitute(&parameter.type_, &substitutions),
                                parameter.ownership,
                            )
                        })
                        .collect::<Vec<_>>();
                    for (source_index, value) in lowered.iter_mut().enumerate() {
                        let (expected, ownership) = &parameter_context[source_index];
                        self.apply_expected_type(value, expected)?;
                        self.apply_authored_ownership(value, *ownership)?;
                    }
                }
                if let CallTarget::Build { primitive, .. } = &target {
                    self.apply_build_argument_types_and_modes(
                        *primitive,
                        &mut lowered,
                        &labels,
                        &syntax.range,
                    )?;
                }
                if let (
                    CallTarget::Struct { definition, .. },
                    Type::Nominal {
                        arguments: type_arguments,
                        ..
                    },
                ) = (&target, &type_)
                {
                    let fields = self.struct_fields(*definition, type_arguments, &syntax.range)?;
                    let resource_fields = labels
                        .iter()
                        .map(|label| {
                            label.as_deref().and_then(|label| {
                                fields
                                    .iter()
                                    .find(|field| field.name == label)
                                    .map(|field| self.type_owns_resource(&field.type_))
                            })
                        })
                        .collect::<Vec<_>>();
                    for (argument, resource) in lowered.iter().zip(resource_fields) {
                        if resource == Some(true) {
                            self.require_owned_transfer(argument)?;
                        }
                    }
                }
                if let CallTarget::Test {
                    id, argument_order, ..
                } = &target
                {
                    let ownership = argument_order
                        .iter()
                        .map(|parameter| {
                            self.tests[id].parameters[usize::from(*parameter)].ownership
                        })
                        .collect::<Vec<_>>();
                    for (argument, ownership) in lowered.iter_mut().zip(ownership) {
                        self.apply_authored_ownership(argument, ownership)?;
                    }
                }
                if let CallTarget::Interface {
                    alternatives,
                    argument_order,
                    argument_parameters,
                    interface,
                    ..
                } = &target
                {
                    let ownership = if let Some((_, definition, _)) = alternatives.first() {
                        argument_order
                            .iter()
                            .map(|parameter| {
                                self.functions[definition].parameters[usize::from(*parameter)]
                                    .ownership
                            })
                            .collect::<Vec<_>>()
                    } else {
                        let requirement = self.interfaces[interface]
                            .requirements
                            .values()
                            .find(|requirement| {
                                argument_order
                                    .iter()
                                    .map(|parameter| {
                                        requirement.parameters[usize::from(*parameter)].definition
                                    })
                                    .eq(argument_parameters.iter().copied())
                            })
                            .ok_or_else(|| {
                                creator_value(CreatorFailureKind::UnresolvedCall, &syntax.range)
                            })?;
                        argument_order
                            .iter()
                            .map(|parameter| {
                                requirement.parameters[usize::from(*parameter)].ownership
                            })
                            .collect::<Vec<_>>()
                    };
                    for (argument, ownership) in lowered.iter_mut().zip(ownership) {
                        self.apply_authored_ownership(argument, ownership)?;
                    }
                }
                if matches!(
                    &target,
                    CallTarget::BuiltinVariant(_) | CallTarget::UserVariant { .. }
                ) {
                    for argument in &lowered {
                        self.require_owned_transfer(argument)?;
                    }
                }
                self.validate_call_custody(&lowered)?;
                if let CallTarget::Test { .. } = &target
                    && self.test_application_context == 0
                {
                    return creator(
                        CreatorFailureKind::TestApplicationOutsideCases,
                        &syntax.range,
                    );
                }
                (
                    ExpressionKind::Call {
                        target,
                        arguments: lowered.into(),
                    },
                    type_,
                )
            }
            ExpressionSyntaxKind::Array(values) => {
                let values = values
                    .iter()
                    .map(|value| self.expression(value))
                    .collect::<Result<Vec<_>, _>>()?;
                for value in &values {
                    self.require_owned_transfer(value)?;
                }
                let element = values
                    .first()
                    .map_or(Type::Infer, |value| value.type_.clone());
                if values
                    .iter()
                    .any(|value| !can_unify(&value.type_, &element))
                {
                    return creator(CreatorFailureKind::ArrayElementTypeMismatch, &syntax.range);
                }
                let length = u64::try_from(values.len()).unwrap_or(u64::MAX);
                (
                    ExpressionKind::Array(values.into()),
                    Type::FixedArray {
                        element: Arc::new(element),
                        length: ArrayLength::Value(length),
                    },
                )
            }
            ExpressionSyntaxKind::RepeatedArray { value, length } => {
                let value = self.expression(value)?;
                if self.type_owns_resource(&value.type_) {
                    return creator(CreatorFailureKind::ResourceRequiresTake, &value.source);
                }
                let type_ = Type::FixedArray {
                    element: Arc::new(value.type_.clone()),
                    length: ArrayLength::Value(*length),
                };
                (
                    ExpressionKind::RepeatedArray {
                        value: Box::new(value),
                        length: *length,
                    },
                    type_,
                )
            }
            ExpressionSyntaxKind::Tuple(values) => {
                let values = values
                    .iter()
                    .map(|value| self.expression(value))
                    .collect::<Result<Vec<_>, _>>()?;
                for value in &values {
                    self.require_owned_transfer(value)?;
                }
                let type_ = Type::Tuple(
                    values
                        .iter()
                        .map(|value| value.type_.clone())
                        .collect::<Vec<_>>()
                        .into(),
                );
                (ExpressionKind::Tuple(values.into()), type_)
            }
            ExpressionSyntaxKind::Index { value, index } => {
                let value = self.expression(value)?;
                let index = self.expression(index)?;
                if !matches!(index.type_, Type::Integer(_)) {
                    return creator(CreatorFailureKind::InvalidUnaryOperand, &syntax.range);
                }
                let element = match &value.type_ {
                    Type::Array(element) | Type::FixedArray { element, .. } => element,
                    _ => return creator(CreatorFailureKind::UnresolvedName, &syntax.range),
                };
                let type_ = (**element).clone();
                (
                    ExpressionKind::Index {
                        value: Box::new(value),
                        index: Box::new(index),
                    },
                    type_,
                )
            }
            ExpressionSyntaxKind::Positive(value) => {
                let value = self.expression(value)?;
                if !value.type_.is_numeric() {
                    return creator(CreatorFailureKind::InvalidUnaryOperand, &syntax.range);
                }
                let type_ = value.type_.clone();
                (ExpressionKind::Positive(Box::new(value)), type_)
            }
            ExpressionSyntaxKind::Negate(value) => {
                if let ExpressionSyntaxKind::Integer(authored) = &value.kind {
                    let (_, kind) = parse_integer_parts(authored).map_err(|()| {
                        creator_value(CreatorFailureKind::InvalidIntegerLiteral, &syntax.range)
                    })?;
                    if !kind.is_signed() {
                        return creator(CreatorFailureKind::InvalidUnaryOperand, &syntax.range);
                    }
                    let (value, kind) = parse_negated_integer_literal(authored).map_err(|()| {
                        creator_value(CreatorFailureKind::InvalidIntegerLiteral, &syntax.range)
                    })?;
                    (
                        ExpressionKind::Literal(Literal::Integer { kind, value }),
                        Type::Integer(kind),
                    )
                } else {
                    let value = self.expression(value)?;
                    let can_negate = match value.type_ {
                        Type::Float(_) => true,
                        Type::Integer(kind) => kind.is_signed(),
                        _ => false,
                    };
                    if !can_negate {
                        return creator(CreatorFailureKind::InvalidUnaryOperand, &syntax.range);
                    }
                    let type_ = value.type_.clone();
                    (ExpressionKind::Negate(Box::new(value)), type_)
                }
            }
            ExpressionSyntaxKind::BitNot(value) => {
                let value = self.expression(value)?;
                if !matches!(value.type_, Type::Integer(_)) {
                    return creator(CreatorFailureKind::InvalidUnaryOperand, &syntax.range);
                }
                let type_ = value.type_.clone();
                (ExpressionKind::BitNot(Box::new(value)), type_)
            }
            ExpressionSyntaxKind::Not(value) => {
                let value = self.expression(value)?;
                if value.type_ != Type::Bool {
                    return creator(CreatorFailureKind::InvalidUnaryOperand, &syntax.range);
                }
                (ExpressionKind::Not(Box::new(value)), Type::Bool)
            }
            ExpressionSyntaxKind::Await(value) => {
                let value = self.expression(value)?;
                if let Some(loan) = call_loan(&value) {
                    let place = root_place(loan).ok_or_else(|| VerificationFailure::Defect {
                        evidence: Arc::from("lowered Resource loan has no source place"),
                    })?;
                    return Err(self.custody_failure(
                        CreatorFailureKind::LoanAcrossSuspension,
                        &syntax.range,
                        &PlaceKey::from_place(&place),
                        CustodyDiagnosticState::Loaned,
                        Some(loan.source.clone()),
                    ));
                }
                let type_ = value.type_.clone();
                (ExpressionKind::Await(Box::new(value)), type_)
            }
            ExpressionSyntaxKind::Mut(value) => {
                let mut value = self.expression(value)?;
                let Some(place) = root_place(&value) else {
                    return creator(CreatorFailureKind::TakeRequiresResourcePlace, &syntax.range);
                };
                let writable = self
                    .locals
                    .values()
                    .find(|(local, _, _)| *local == place.local)
                    .is_some_and(|(_, _, mutable)| {
                        *mutable
                            && place.projections.iter().all(|projection| match projection {
                                PlaceProjection::Field { mutable, .. } => *mutable,
                                PlaceProjection::Index { .. } => true,
                            })
                    });
                if !writable || !self.type_owns_resource(&value.type_) {
                    return creator(CreatorFailureKind::ImmutableReassignment, &syntax.range);
                }
                value.kind = ExpressionKind::Read(place);
                value.access = AccessMode::Mut;
                value.source = syntax.range.clone();
                return Ok(value);
            }
            ExpressionSyntaxKind::Take(value) => {
                let mut value = self.expression(value)?;
                let Some(place) = root_place(&value) else {
                    return creator(CreatorFailureKind::TakeRequiresResourcePlace, &syntax.range);
                };
                if !self.type_owns_resource(&value.type_) {
                    return creator(CreatorFailureKind::TakeRequiresResourcePlace, &syntax.range);
                }
                value.kind = ExpressionKind::Read(place);
                value.access = AccessMode::Move;
                value.source = syntax.range.clone();
                self.record_move(&value)?;
                return Ok(value);
            }
            ExpressionSyntaxKind::Closure { parameters, body } => {
                let contextual = self.expected_expression_type.take();
                let contextual = match contextual.as_ref() {
                    Some(Type::Function {
                        parameters: expected_parameters,
                        return_type,
                    }) if expected_parameters.len() == parameters.len() => {
                        Some((expected_parameters.clone(), return_type.clone()))
                    }
                    Some(_) => {
                        return creator(CreatorFailureKind::InvalidFunctionValue, &syntax.range);
                    }
                    None => None,
                };
                let before_locals = self.locals.clone();
                let before_moved = self.moved.clone();
                let before_known = self.known_integers.clone();
                let mut seen = BTreeSet::new();
                let mut lowered_parameters = Vec::with_capacity(parameters.len());
                for (index, parameter) in parameters.iter().enumerate() {
                    if !seen.insert(parameter.name.as_str()) {
                        return creator(CreatorFailureKind::DuplicateParameter, &parameter.range);
                    }
                    let type_ = if let Some(type_syntax) = &parameter.type_ {
                        let type_ = self.resolve_local_type(type_syntax).ok_or_else(|| {
                            creator_value(CreatorFailureKind::UnresolvedType, &parameter.range)
                        })?;
                        if contextual
                            .as_ref()
                            .is_some_and(|(parameters, _)| !can_unify(&type_, &parameters[index]))
                        {
                            return creator(
                                CreatorFailureKind::InvalidFunctionValue,
                                &parameter.range,
                            );
                        }
                        type_
                    } else if let Some((expected_parameters, _)) = &contextual {
                        expected_parameters[index].clone()
                    } else {
                        return creator(CreatorFailureKind::InvalidFunctionValue, &parameter.range);
                    };
                    let local = self.bind_local(&parameter.name, type_.clone())?;
                    lowered_parameters.push((local, type_));
                }
                let lowered_body = if let Some((_, expected_return)) = &contextual {
                    self.expression_expected(body, expected_return)?
                } else {
                    self.expression(body)?
                };
                let parameter_locals = lowered_parameters
                    .iter()
                    .map(|(local, _)| *local)
                    .collect::<BTreeSet<_>>();
                let mut referenced = BTreeSet::new();
                collect_required_locals(&lowered_body, &mut referenced);
                let mut captures = Vec::new();
                for local in referenced.difference(&parameter_locals) {
                    let Some((_, type_, _)) = before_locals
                        .values()
                        .find(|(candidate, _, _)| *candidate == *local)
                    else {
                        return defect("closure body references a local outside its lexical scope");
                    };
                    if self.type_owns_resource(type_) {
                        return creator(
                            CreatorFailureKind::ClosureCaptureRequiresData,
                            &syntax.range,
                        );
                    }
                    captures.push((*local, type_.clone()));
                }
                let type_ = Type::Function {
                    parameters: lowered_parameters
                        .iter()
                        .map(|(_, type_)| type_.clone())
                        .collect(),
                    return_type: Arc::new(lowered_body.type_.clone()),
                };
                let mut key = Vec::new();
                key.extend_from_slice(b"wrela.closure\0\x01");
                append_range(&mut key, &syntax.range);
                append_part(&mut key, &type_.canonical_key());
                append_expression(&mut key, &lowered_body);
                let id = ClosureId(xxh3_128(&key));
                let parameter_type_ids = lowered_parameters
                    .iter()
                    .map(|(_, type_)| {
                        observe_type_tree(type_, self.identity_catalog, self.discharge_laws)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let capture_type_ids = captures
                    .iter()
                    .map(|(_, type_)| {
                        observe_type_tree(type_, self.identity_catalog, self.discharge_laws)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                self.locals = before_locals;
                self.moved = before_moved;
                self.known_integers = before_known;
                (
                    ExpressionKind::Closure(Arc::new(HirClosure {
                        id,
                        parameters: lowered_parameters.into(),
                        parameter_type_ids: parameter_type_ids.into(),
                        captures: captures.into(),
                        capture_type_ids: capture_type_ids.into(),
                        return_type: lowered_body.type_.clone(),
                        body: lowered_body,
                        source: syntax.range.clone(),
                        identity_key: key.into(),
                    })),
                    type_,
                )
            }
            ExpressionSyntaxKind::Propagate(value) => {
                let value = self.expression(value)?;
                let success = match &value.type_ {
                    Type::Result { success, .. } | Type::Option(success) => success,
                    _ => {
                        return creator(
                            CreatorFailureKind::PropagationRequiresResult,
                            &syntax.range,
                        );
                    }
                };
                self.check_recoverable_exit()?;
                let type_ = (**success).clone();
                (ExpressionKind::Propagate(Box::new(value)), type_)
            }
            ExpressionSyntaxKind::Is { value, pattern } => {
                let value = self.expression(value)?;
                let before_locals = self.locals.clone();
                let before_moved = self.moved.clone();
                let before_known = self.known_integers.clone();
                let pattern = self
                    .lower_match_pattern(pattern, &value.type_)?
                    .ok_or_else(|| {
                        creator_value(CreatorFailureKind::InvalidMatchPattern, &syntax.range)
                    })?;
                let binds = !match_pattern_binding_signature(&pattern).is_empty();
                self.locals = before_locals;
                self.moved = before_moved;
                self.known_integers = before_known;
                if binds {
                    return creator(CreatorFailureKind::InvalidMatchPattern, &syntax.range);
                }
                (
                    ExpressionKind::Is {
                        value: Box::new(value),
                        pattern: Box::new(pattern),
                    },
                    Type::Bool,
                )
            }
            ExpressionSyntaxKind::Binary {
                operator,
                left,
                right,
            } => {
                let left = self.expression(left)?;
                let right = self.expression(right)?;
                let operator = BinaryOperator::from(*operator);
                self.validate_binary_capability(operator, &left.type_, &syntax.range)?;
                let type_ = binary_type(operator, &left.type_, &right.type_).ok_or_else(|| {
                    creator_value(CreatorFailureKind::BinaryTypeMismatch, &syntax.range)
                })?;
                (
                    ExpressionKind::Binary {
                        operator,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    type_,
                )
            }
            ExpressionSyntaxKind::Unsupported(_) => {
                return creator(CreatorFailureKind::UnsupportedLayerOneSyntax, &syntax.range);
            }
        };
        self.finish_expression(kind, type_, syntax.range.clone())
    }

    fn finish_expression(
        &mut self,
        kind: ExpressionKind,
        type_: Type,
        source: SourceRange,
    ) -> Result<Expression, VerificationFailure> {
        let type_id = observe_type_tree(&type_, self.identity_catalog, self.discharge_laws)?;
        Ok(Expression {
            kind,
            type_id,
            type_,
            coerced_from: None,
            access: AccessMode::Copy,
            source,
        })
    }

    fn apply_expected_type(
        &mut self,
        expression: &mut Expression,
        expected: &Type,
    ) -> Result<(), VerificationFailure> {
        if let (Type::Nominal { definition, .. }, Type::Any { interface, .. }) =
            (&expression.type_, expected)
        {
            if self
                .interfaces
                .get(interface)
                .is_none_or(|interface| !interface.implementations.contains_key(definition))
            {
                return creator(CreatorFailureKind::ArgumentTypeMismatch, &expression.source);
            }
            expression.coerced_from = Some(expression.type_.clone());
            expression.type_ = expected.clone();
            expression.type_id =
                self.identity_catalog
                    .intern_type(expected)
                    .map_err(|collision| VerificationFailure::Defect {
                        evidence: Arc::from(format!(
                            "type identity collision {:032x}",
                            collision.digest
                        )),
                    })?;
            return Ok(());
        }
        if expression.type_ == *expected
            || matches!(expected, Type::Infer | Type::Parameter { .. })
            || matches!(
                (&expression.type_, expected),
                (
                    Type::Result { error: Some(_), .. },
                    Type::Result { error: None, .. }
                )
            )
            || !can_unify(&expression.type_, expected)
        {
            return Ok(());
        }
        match (&mut expression.kind, expected) {
            (
                ExpressionKind::Call {
                    target: CallTarget::BuiltinVariant(variant),
                    arguments,
                },
                Type::Result { success, error },
            ) if matches!(
                variant,
                BuiltinVariant::ResultOk | BuiltinVariant::ResultErr
            ) =>
            {
                if let Some(argument) = Arc::make_mut(arguments).first_mut() {
                    let payload = match variant {
                        BuiltinVariant::ResultOk => success.as_ref(),
                        BuiltinVariant::ResultErr => error.as_deref().unwrap_or(&Type::Infer),
                        _ => unreachable!("guarded Result variant"),
                    };
                    self.apply_expected_type(argument, payload)?;
                }
            }
            (
                ExpressionKind::Call {
                    target:
                        CallTarget::BuiltinVariant(
                            BuiltinVariant::OptionSome | BuiltinVariant::OptionNone,
                        ),
                    arguments,
                },
                Type::Option(value),
            ) => {
                if let Some(argument) = Arc::make_mut(arguments).first_mut() {
                    self.apply_expected_type(argument, value)?;
                }
            }
            (ExpressionKind::Array(values), Type::Array(element)) => {
                for value in Arc::make_mut(values) {
                    self.apply_expected_type(value, element)?;
                }
                let concrete = Type::FixedArray {
                    element: element.clone(),
                    length: ArrayLength::Value(u64::try_from(values.len()).unwrap_or(u64::MAX)),
                };
                expression.type_id =
                    self.identity_catalog
                        .intern_type(&concrete)
                        .map_err(|collision| VerificationFailure::Defect {
                            evidence: Arc::from(format!(
                                "type identity collision {:032x}",
                                collision.digest
                            )),
                        })?;
                expression.type_ = concrete;
                return Ok(());
            }
            (ExpressionKind::RepeatedArray { value, length }, Type::Array(element)) => {
                self.apply_expected_type(value, element)?;
                let concrete = Type::FixedArray {
                    element: element.clone(),
                    length: ArrayLength::Value(*length),
                };
                expression.type_id =
                    self.identity_catalog
                        .intern_type(&concrete)
                        .map_err(|collision| VerificationFailure::Defect {
                            evidence: Arc::from(format!(
                                "type identity collision {:032x}",
                                collision.digest
                            )),
                        })?;
                expression.type_ = concrete;
                return Ok(());
            }
            (ExpressionKind::Tuple(values), Type::Tuple(members))
                if values.len() == members.len() =>
            {
                for (value, member) in Arc::make_mut(values).iter_mut().zip(members.iter()) {
                    self.apply_expected_type(value, member)?;
                }
            }
            _ => {}
        }
        expression.type_ = expected.clone();
        expression.type_id = self
            .identity_catalog
            .intern_type(expected)
            .map_err(|collision| VerificationFailure::Defect {
                evidence: Arc::from(format!("type identity collision {:032x}", collision.digest)),
            })?;
        Ok(())
    }

    fn record_move(&mut self, expression: &Expression) -> Result<(), VerificationFailure> {
        if expression.access != AccessMode::Move {
            return Ok(());
        }
        if let Some(place) = root_place(expression)
            && !self.moved.move_place_at(&place, &expression.source)
        {
            return Err(self.custody_failure(
                CreatorFailureKind::ReadAfterMove,
                &expression.source,
                &PlaceKey::from_place(&place),
                CustodyDiagnosticState::Moved,
                self.moved.origin(&place).cloned(),
            ));
        }
        Ok(())
    }

    fn place_key_subject(&self, place: &PlaceKey) -> Arc<str> {
        let root = self
            .locals
            .iter()
            .find_map(|(name, (local, _, _))| (*local == place.local).then_some(name.as_str()))
            .unwrap_or("<local>");
        let mut subject = root.to_owned();
        for projection in place.projections.iter() {
            match projection {
                ProjectionKey::Field(_, name) => {
                    subject.push('.');
                    subject.push_str(name);
                }
                ProjectionKey::Index(_) => subject.push_str("[]"),
            }
        }
        Arc::from(subject)
    }

    fn custody_failure(
        &self,
        kind: CreatorFailureKind,
        site: &SourceRange,
        place: &PlaceKey,
        state: CustodyDiagnosticState,
        related: Option<SourceRange>,
    ) -> VerificationFailure {
        let subject_identity = self.locals.values().find_map(|(local, type_, _)| {
            if *local != place.local {
                return None;
            }
            match type_ {
                Type::Nominal { definition, .. } => Some(*definition),
                _ => None,
            }
        });
        custody_value(
            kind,
            site,
            self.place_key_subject(place),
            state,
            subject_identity,
            self.owner_identity,
            related,
        )
    }

    fn join_custody(
        &self,
        left: MoveState,
        right: MoveState,
        site: &SourceRange,
    ) -> Result<MoveState, VerificationFailure> {
        if let Some((place, related)) = left.mismatch(&right) {
            return Err(self.custody_failure(
                CreatorFailureKind::CustodyJoinMismatch,
                site,
                &place,
                CustodyDiagnosticState::PathDependent,
                related,
            ));
        }
        Ok(left)
    }

    fn select_branch_custody(
        &self,
        before: MoveState,
        then_state: MoveState,
        else_state: MoveState,
        then_exits: bool,
        else_exits: bool,
        site: &SourceRange,
    ) -> Result<MoveState, VerificationFailure> {
        match (then_exits, else_exits) {
            (false, false) => self.join_custody(then_state, else_state, site),
            (false, true) => Ok(then_state),
            (true, false) => Ok(else_state),
            (true, true) => Ok(before),
        }
    }

    fn join_reachable_custody(
        &self,
        states: impl IntoIterator<Item = MoveState>,
        site: &SourceRange,
    ) -> Result<Option<MoveState>, VerificationFailure> {
        let mut states = states.into_iter();
        let Some(mut joined) = states.next() else {
            return Ok(None);
        };
        for state in states {
            joined = self.join_custody(joined, state, site)?;
        }
        Ok(Some(joined))
    }

    fn type_owns_resource(&self, type_: &Type) -> bool {
        self.discharge_law(type_).is_some()
    }

    fn discharge_law(&self, type_: &Type) -> Option<DischargeLaw> {
        derive_discharge_law(type_, self.structs, self.interfaces, self.nominal_displays)
    }

    fn collect_live_discharge_obligations(
        &self,
        type_: &Type,
        place: &PlaceKey,
        subject: &str,
        obligations: &mut Vec<LiveDischargeObligation>,
    ) {
        let Some(law) = self.discharge_law(type_) else {
            return;
        };
        if !law.requires_explicit_discharge() {
            return;
        }
        if let Type::Nominal {
            definition,
            arguments,
            ..
        } = type_
            && let Some(struct_) = self.structs.get(definition)
            && struct_.resource
        {
            let substitutions = struct_
                .type_parameters
                .iter()
                .copied()
                .zip(arguments.iter().cloned())
                .collect::<BTreeMap<_, _>>();
            let fields = if struct_.field_selections.is_empty()
                || arguments.iter().any(type_has_placeholder)
            {
                Arc::from(struct_.fields.clone())
            } else {
                struct_
                    .applied_fields
                    .borrow()
                    .get(arguments)
                    .cloned()
                    .unwrap_or_else(|| Arc::from(struct_.fields.clone()))
            };
            let resource_fields = fields
                .iter()
                .filter_map(|field| {
                    let type_ = substitute(&field.type_, &substitutions);
                    self.discharge_law(&type_)
                        .is_some()
                        .then_some((field, type_))
                })
                .collect::<Vec<_>>();
            if !resource_fields.is_empty() {
                for (field, field_type) in resource_fields {
                    let mut projections = place.projections.to_vec();
                    projections.push(ProjectionKey::Field(
                        *definition,
                        Arc::from(field.name.as_str()),
                    ));
                    let field_place = PlaceKey {
                        local: place.local,
                        projections: projections.into(),
                    };
                    self.collect_live_discharge_obligations(
                        &field_type,
                        &field_place,
                        &format!("{subject}.{}", field.name),
                        obligations,
                    );
                }
                return;
            }
        }
        if let Type::FixedArray {
            element,
            length: ArrayLength::Value(length),
        } = type_
            && self.discharge_law(element).is_some()
        {
            for index in 0..*length {
                let mut projections = place.projections.to_vec();
                projections.push(ProjectionKey::Index(Some(i128::from(index))));
                self.collect_live_discharge_obligations(
                    element,
                    &PlaceKey {
                        local: place.local,
                        projections: projections.into(),
                    },
                    &format!("{subject}[{index}]"),
                    obligations,
                );
            }
            return;
        }
        if let Type::Tuple(members) = type_ {
            for (index, member) in members.iter().enumerate() {
                if self.discharge_law(member).is_none() {
                    continue;
                }
                let mut projections = place.projections.to_vec();
                projections.push(ProjectionKey::Index(Some(
                    i128::try_from(index).unwrap_or(i128::MAX),
                )));
                self.collect_live_discharge_obligations(
                    member,
                    &PlaceKey {
                        local: place.local,
                        projections: projections.into(),
                    },
                    &format!("{subject}[{index}]"),
                    obligations,
                );
            }
            return;
        }
        if matches!(type_, Type::Array(_)) {
            if !self.moved.is_key_fully_moved(place) {
                obligations.push(LiveDischargeObligation {
                    place: place.clone(),
                    subject: Arc::from(subject),
                    subject_identity: None,
                });
            }
            return;
        }
        if !self.moved.is_key_unreadable(place) {
            obligations.push(LiveDischargeObligation {
                place: place.clone(),
                subject: Arc::from(subject),
                subject_identity: match type_ {
                    Type::Nominal { definition, .. } => Some(*definition),
                    Type::Any { interface, .. } => Some(*interface),
                    _ => None,
                },
            });
        }
    }

    fn check_recoverable_exit(&self) -> Result<(), VerificationFailure> {
        self.check_recoverable_exit_excluding(&self.parameter_locals)
    }

    fn check_recoverable_exit_excluding(
        &self,
        excluded: &BTreeSet<LocalId>,
    ) -> Result<(), VerificationFailure> {
        let mut candidates = self
            .locals
            .iter()
            .filter(|(_, (local, _, _))| {
                !excluded.contains(local) && !self.non_custodial_locals.contains(local)
            })
            .flat_map(|(name, (local, type_, _))| {
                let mut obligations = Vec::new();
                self.collect_live_discharge_obligations(
                    type_,
                    &PlaceKey {
                        local: *local,
                        projections: Arc::from([]),
                    },
                    name,
                    &mut obligations,
                );
                obligations
                    .into_iter()
                    .map(|obligation| (self.local_origins.get(local), obligation))
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            left.0
                .map(SourceRange::start)
                .cmp(&right.0.map(SourceRange::start))
                .then(left.1.subject.cmp(&right.1.subject))
        });
        let Some((Some(site), obligation)) = candidates.first() else {
            return Ok(());
        };
        Err(custody_value(
            CreatorFailureKind::ResourceNotDischarged,
            site,
            Arc::clone(&obligation.subject),
            CustodyDiagnosticState::LiveUndischarged,
            obligation.subject_identity,
            self.owner_identity,
            self.moved.origins.get(&obligation.place).cloned(),
        ))
    }

    fn require_owned_transfer(&self, expression: &Expression) -> Result<(), VerificationFailure> {
        if self.type_owns_resource(&expression.type_)
            && root_place(expression).is_some()
            && expression.access != AccessMode::Move
        {
            return creator(CreatorFailureKind::ResourceRequiresTake, &expression.source);
        }
        Ok(())
    }

    fn is_must_use(&self, type_: &Type) -> bool {
        self.type_owns_resource(type_) || matches!(type_, Type::Result { .. })
    }

    fn expression_contains_resource_move(&self, expression: &Expression) -> bool {
        if expression.access == AccessMode::Move && self.type_owns_resource(&expression.type_) {
            return true;
        }
        let mut found = false;
        expression.visit_children(&mut |child| {
            found |= self.expression_contains_resource_move(child);
        });
        found
    }

    fn cleanup_resource_loan(&self, expression: &Expression) -> Option<Expression> {
        if matches!(expression.access, AccessMode::Read | AccessMode::Mut)
            && self.type_owns_resource(&expression.type_)
        {
            return Some(expression.clone());
        }
        if matches!(expression.kind, ExpressionKind::Closure(_)) {
            return None;
        }
        let mut found = None;
        expression.visit_children(&mut |child| {
            if found.is_none() {
                found = self.cleanup_resource_loan(child);
            }
        });
        found
    }

    fn cleanup_has_conditional_resource_move(&self, expression: &Expression) -> bool {
        if let ExpressionKind::Binary {
            operator: BinaryOperator::And | BinaryOperator::Or,
            left,
            right,
        } = &expression.kind
        {
            return self.expression_contains_resource_move(right)
                || self.cleanup_has_conditional_resource_move(left)
                || self.cleanup_has_conditional_resource_move(right);
        }
        if matches!(expression.kind, ExpressionKind::Closure(_)) {
            return false;
        }
        let mut found = false;
        expression.visit_children(&mut |child| {
            found |= self.cleanup_has_conditional_resource_move(child);
        });
        found
    }

    fn normalize_cleanup_expression(
        &self,
        expression: Expression,
        captures: &mut Vec<CleanupCapture>,
    ) -> Result<Expression, VerificationFailure> {
        if expression.access == AccessMode::Move && self.type_owns_resource(&expression.type_) {
            let ordinal = CleanupCaptureOrdinal(u32::try_from(captures.len()).map_err(|_| {
                VerificationFailure::Defect {
                    evidence: Arc::from("cleanup capture ordinal exceeds artifact limits"),
                }
            })?);
            let replacement = Expression {
                kind: ExpressionKind::CleanupCapture(ordinal),
                type_id: expression.type_id,
                type_: expression.type_.clone(),
                coerced_from: expression.coerced_from.clone(),
                access: expression.access,
                source: expression.source.clone(),
            };
            captures.push(CleanupCapture {
                ordinal,
                expression,
            });
            return Ok(replacement);
        }

        let Expression {
            kind,
            type_id,
            type_,
            coerced_from,
            access,
            source,
        } = expression;
        let kind = match kind {
            ExpressionKind::Read(mut place) => {
                place.projections = place
                    .projections
                    .iter()
                    .cloned()
                    .map(|projection| match projection {
                        PlaceProjection::Index { index, type_ } => Ok(PlaceProjection::Index {
                            index: Box::new(self.normalize_cleanup_expression(*index, captures)?),
                            type_,
                        }),
                        projection => Ok(projection),
                    })
                    .collect::<Result<Vec<_>, VerificationFailure>>()?
                    .into();
                ExpressionKind::Read(place)
            }
            ExpressionKind::Call { target, arguments } => {
                let target = match target {
                    CallTarget::Callable { value } => CallTarget::Callable {
                        value: Box::new(self.normalize_cleanup_expression(*value, captures)?),
                    },
                    target => target,
                };
                let arguments = arguments
                    .iter()
                    .cloned()
                    .map(|argument| self.normalize_cleanup_expression(argument, captures))
                    .collect::<Result<Vec<_>, _>>()?
                    .into();
                ExpressionKind::Call { target, arguments }
            }
            ExpressionKind::Array(values) => ExpressionKind::Array(
                values
                    .iter()
                    .cloned()
                    .map(|value| self.normalize_cleanup_expression(value, captures))
                    .collect::<Result<Vec<_>, _>>()?
                    .into(),
            ),
            ExpressionKind::Tuple(values) => ExpressionKind::Tuple(
                values
                    .iter()
                    .cloned()
                    .map(|value| self.normalize_cleanup_expression(value, captures))
                    .collect::<Result<Vec<_>, _>>()?
                    .into(),
            ),
            ExpressionKind::RepeatedArray { value, length } => ExpressionKind::RepeatedArray {
                value: Box::new(self.normalize_cleanup_expression(*value, captures)?),
                length,
            },
            ExpressionKind::Index { value, index } => ExpressionKind::Index {
                value: Box::new(self.normalize_cleanup_expression(*value, captures)?),
                index: Box::new(self.normalize_cleanup_expression(*index, captures)?),
            },
            ExpressionKind::Positive(value) => ExpressionKind::Positive(Box::new(
                self.normalize_cleanup_expression(*value, captures)?,
            )),
            ExpressionKind::Negate(value) => ExpressionKind::Negate(Box::new(
                self.normalize_cleanup_expression(*value, captures)?,
            )),
            ExpressionKind::BitNot(value) => ExpressionKind::BitNot(Box::new(
                self.normalize_cleanup_expression(*value, captures)?,
            )),
            ExpressionKind::Not(value) => ExpressionKind::Not(Box::new(
                self.normalize_cleanup_expression(*value, captures)?,
            )),
            ExpressionKind::Await(value) => ExpressionKind::Await(Box::new(
                self.normalize_cleanup_expression(*value, captures)?,
            )),
            ExpressionKind::Propagate(value) => ExpressionKind::Propagate(Box::new(
                self.normalize_cleanup_expression(*value, captures)?,
            )),
            ExpressionKind::Is { value, pattern } => ExpressionKind::Is {
                value: Box::new(self.normalize_cleanup_expression(*value, captures)?),
                pattern,
            },
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => ExpressionKind::Binary {
                operator,
                left: Box::new(self.normalize_cleanup_expression(*left, captures)?),
                right: Box::new(self.normalize_cleanup_expression(*right, captures)?),
            },
            kind @ (ExpressionKind::Literal(_)
            | ExpressionKind::Constant(_)
            | ExpressionKind::FunctionValue { .. }
            | ExpressionKind::Closure(_)
            | ExpressionKind::CleanupCapture(_)) => kind,
        };
        Ok(Expression {
            kind,
            type_id,
            type_,
            coerced_from,
            access,
            source,
        })
    }

    fn validate_binary_capability(
        &self,
        operator: BinaryOperator,
        type_: &Type,
        site: &SourceRange,
    ) -> Result<(), VerificationFailure> {
        let supported = if matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
            self.type_supports_comparison(type_, false, &mut BTreeSet::new())
        } else if matches!(
            operator,
            BinaryOperator::Less
                | BinaryOperator::LessEqual
                | BinaryOperator::Greater
                | BinaryOperator::GreaterEqual
        ) {
            self.type_supports_comparison(type_, true, &mut BTreeSet::new())
        } else {
            true
        };
        if !supported {
            return creator(CreatorFailureKind::BinaryTypeMismatch, site);
        }
        Ok(())
    }

    fn type_supports_comparison(
        &self,
        type_: &Type,
        ordered: bool,
        visiting: &mut BTreeSet<DefinitionId>,
    ) -> bool {
        match type_ {
            Type::Unit | Type::Bool => !ordered,
            Type::Integer(_) | Type::Float(_) | Type::Text | Type::Scalar | Type::Bytes => true,
            Type::Array(value) | Type::FixedArray { element: value, .. } | Type::Option(value) => {
                self.type_supports_comparison(value, ordered, visiting)
            }
            Type::Tuple(values) => values
                .iter()
                .all(|value| self.type_supports_comparison(value, ordered, visiting)),
            Type::Result { success, error } => {
                self.type_supports_comparison(success, ordered, visiting)
                    && error
                        .as_ref()
                        .is_none_or(|error| self.type_supports_comparison(error, ordered, visiting))
            }
            Type::Nominal {
                definition,
                arguments,
                ..
            } => {
                if !visiting.insert(*definition) {
                    return true;
                }
                let result = if let Some(struct_) = self.structs.get(definition) {
                    !struct_.resource
                        && struct_.fields.iter().all(|field| {
                            let substitutions = struct_
                                .type_parameters
                                .iter()
                                .copied()
                                .zip(arguments.iter().cloned())
                                .collect::<BTreeMap<_, _>>();
                            self.type_supports_comparison(
                                &substitute(&field.type_, &substitutions),
                                ordered,
                                visiting,
                            )
                        })
                } else {
                    self.variants
                        .iter()
                        .filter(|(id, _)| id.owner == *definition)
                        .all(|(_, variant)| {
                            let substitutions = variant
                                .type_parameters
                                .iter()
                                .copied()
                                .zip(arguments.iter().cloned())
                                .collect::<BTreeMap<_, _>>();
                            variant.parameters.iter().all(|parameter| {
                                self.type_supports_comparison(
                                    &substitute(&parameter.type_, &substitutions),
                                    ordered,
                                    visiting,
                                )
                            })
                        })
                };
                visiting.remove(definition);
                result
            }
            Type::Function { .. }
            | Type::Own { .. }
            | Type::Any { .. }
            | Type::Builtin(_)
            | Type::ConstU64(_)
            | Type::PoolArgument(_)
            | Type::Parameter { .. }
            | Type::Infer => false,
        }
    }

    fn resolve_local_type(&self, syntax: &crate::syntax::TypeSyntax) -> Option<Type> {
        use crate::syntax::TypeSyntax;
        match syntax {
            TypeSyntax::Unit => Some(Type::Unit),
            TypeSyntax::ConstU64(value) => Some(Type::ConstU64(*value)),
            TypeSyntax::Infer => None,
            TypeSyntax::Named(name) => {
                if let Some(type_) = resolve_builtin_type(name) {
                    return Some(type_);
                }
                match self.namespace.resolve(self.module, &name.segments)? {
                    ResolvedName::Pool(pool) => Some(Type::PoolArgument(pool)),
                    ResolvedName::Nominal(definition)
                        if self.namespace.nominal_arity(definition) == Some(0) =>
                    {
                        Some(Type::Nominal {
                            definition,
                            display: self.nominal_displays.get(&definition)?.clone(),
                            arguments: Arc::from([]),
                        })
                    }
                    ResolvedName::Alias(definition) => self.aliases.get(&definition).cloned(),
                    _ => None,
                }
            }
            TypeSyntax::Apply { base, arguments } => {
                let arguments = arguments
                    .iter()
                    .map(|argument| self.resolve_local_type(argument))
                    .collect::<Option<Vec<_>>>()?;
                match base
                    .segments
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .as_slice()
                {
                    ["Option"] if arguments.len() == 1 => {
                        Some(Type::Option(Arc::new(arguments[0].clone())))
                    }
                    ["Result"] if matches!(arguments.as_slice(), [_] | [_, _]) => {
                        Some(Type::Result {
                            success: Arc::new(arguments[0].clone()),
                            error: arguments.get(1).cloned().map(Arc::new),
                        })
                    }
                    _ => match self.namespace.resolve(self.module, &base.segments)? {
                        ResolvedName::Nominal(definition)
                            if self.namespace.nominal_arity(definition)
                                == Some(arguments.len()) =>
                        {
                            Some(Type::Nominal {
                                definition,
                                display: self.nominal_displays.get(&definition)?.clone(),
                                arguments: arguments.into(),
                            })
                        }
                        _ => None,
                    },
                }
            }
            TypeSyntax::Array(element) => {
                Some(Type::Array(Arc::new(self.resolve_local_type(element)?)))
            }
            TypeSyntax::Tuple(members) => Some(Type::Tuple(
                members
                    .iter()
                    .map(|member| self.resolve_local_type(member))
                    .collect::<Option<Vec<_>>>()?
                    .into(),
            )),
            TypeSyntax::FixedArray { element, length } => Some(Type::FixedArray {
                element: Arc::new(self.resolve_local_type(element)?),
                length: match length {
                    crate::syntax::FixedArrayLengthSyntax::Literal(value) => {
                        ArrayLength::Value(*value)
                    }
                    crate::syntax::FixedArrayLengthSyntax::Parameter(name) => {
                        let _ = name;
                        return None;
                    }
                },
            }),
            TypeSyntax::Function {
                parameters,
                return_type,
            } => Some(Type::Function {
                parameters: parameters
                    .iter()
                    .map(|parameter| self.resolve_local_type(parameter))
                    .collect::<Option<Vec<_>>>()?
                    .into(),
                return_type: Arc::new(self.resolve_local_type(return_type)?),
            }),
            TypeSyntax::Own { pool, value } => {
                let ResolvedName::Pool(pool) =
                    self.namespace.resolve(self.module, &pool.segments)?
                else {
                    return None;
                };
                Some(Type::Own {
                    pool: PoolTerm::Concrete(pool),
                    value: Arc::new(self.resolve_local_type(value)?),
                })
            }
            TypeSyntax::Any(interface) => {
                let ResolvedName::Nominal(interface) =
                    self.namespace.resolve(self.module, &interface.segments)?
                else {
                    return None;
                };
                Some(Type::Any {
                    interface,
                    display: self.nominal_displays.get(&interface)?.clone(),
                })
            }
        }
    }

    fn pattern_move_keys(
        &self,
        pattern: &HirMatchPattern,
        expected: &Type,
        root: &PlaceKey,
    ) -> Result<BTreeSet<PlaceKey>, VerificationFailure> {
        let project = |projection: ProjectionKey| {
            let mut projections = root.projections.to_vec();
            projections.push(projection);
            PlaceKey {
                local: root.local,
                projections: projections.into(),
            }
        };
        match pattern {
            HirMatchPattern::Binding { access, .. } => Ok((*access == AccessMode::Move)
                .then(|| root.clone())
                .into_iter()
                .collect()),
            HirMatchPattern::Wildcard | HirMatchPattern::Literal(_) => Ok(BTreeSet::new()),
            HirMatchPattern::Tuple(patterns) => {
                let Type::Tuple(types) = expected else {
                    return defect("typed tuple pattern lost its tuple type");
                };
                let mut moved = BTreeSet::new();
                for (index, (pattern, type_)) in patterns.iter().zip(types.iter()).enumerate() {
                    moved.extend(self.pattern_move_keys(
                        pattern,
                        type_,
                        &project(ProjectionKey::Index(Some(
                            i128::try_from(index).unwrap_or(i128::MAX),
                        ))),
                    )?);
                }
                Ok(moved)
            }
            HirMatchPattern::FixedArray(patterns) => {
                let element = match expected {
                    Type::Array(element) | Type::FixedArray { element, .. } => element,
                    _ => return defect("typed array pattern lost its array type"),
                };
                let mut moved = BTreeSet::new();
                for (index, pattern) in patterns.iter().enumerate() {
                    moved.extend(self.pattern_move_keys(
                        pattern,
                        element,
                        &project(ProjectionKey::Index(Some(
                            i128::try_from(index).unwrap_or(i128::MAX),
                        ))),
                    )?);
                }
                Ok(moved)
            }
            HirMatchPattern::Struct { definition, fields } => {
                let Type::Nominal { arguments, .. } = expected else {
                    return defect("typed struct pattern lost its nominal type");
                };
                let struct_ = &self.structs[definition];
                let substitutions = struct_
                    .type_parameters
                    .iter()
                    .copied()
                    .zip(arguments.iter().cloned())
                    .collect::<BTreeMap<_, _>>();
                let selected = if struct_.field_selections.is_empty()
                    || arguments.iter().any(type_has_placeholder)
                {
                    Arc::from(struct_.fields.clone())
                } else {
                    struct_
                        .applied_fields
                        .borrow()
                        .get(arguments)
                        .cloned()
                        .unwrap_or_else(|| Arc::from(struct_.fields.clone()))
                };
                let mut moved = BTreeSet::new();
                for (pattern, field) in fields.iter().zip(selected.iter()) {
                    moved.extend(self.pattern_move_keys(
                        pattern,
                        &substitute(&field.type_, &substitutions),
                        &project(ProjectionKey::Field(
                            *definition,
                            Arc::from(field.name.as_str()),
                        )),
                    )?);
                }
                Ok(moved)
            }
            HirMatchPattern::Variant { id, payload } => {
                let Type::Nominal { arguments, .. } = expected else {
                    return defect("typed variant pattern lost its nominal type");
                };
                let variant = &self.variants[id];
                let substitutions = variant
                    .type_parameters
                    .iter()
                    .copied()
                    .zip(arguments.iter().cloned())
                    .collect::<BTreeMap<_, _>>();
                let mut moved = BTreeSet::new();
                for (index, (pattern, parameter)) in
                    payload.iter().zip(variant.parameters.iter()).enumerate()
                {
                    moved.extend(self.pattern_move_keys(
                        pattern,
                        &substitute(&parameter.type_, &substitutions),
                        &project(ProjectionKey::Index(Some(
                            i128::try_from(index).unwrap_or(i128::MAX),
                        ))),
                    )?);
                }
                Ok(moved)
            }
            HirMatchPattern::Or(alternatives) => {
                let mut alternatives = alternatives.iter();
                let Some(first) = alternatives.next() else {
                    return defect("typed or-pattern has no alternatives");
                };
                let mut guaranteed = self.pattern_move_keys(first, expected, root)?;
                for alternative in alternatives {
                    let moved = self.pattern_move_keys(alternative, expected, root)?;
                    guaranteed.retain(|place| moved.contains(place));
                }
                Ok(guaranteed)
            }
        }
    }

    fn match_is_exhaustive(&self, type_: &Type, cases: &[HirMatchCase]) -> bool {
        if cases.iter().any(|case| {
            case.guard.is_none()
                && case
                    .pattern
                    .as_ref()
                    .is_none_or(|pattern| self.pattern_irrefutable(pattern, type_))
        }) {
            return true;
        }
        match type_ {
            Type::Bool => {
                let patterns = cases
                    .iter()
                    .filter_map(|case| case.pattern.as_ref())
                    .map(match_pattern_key)
                    .collect::<BTreeSet<_>>();
                patterns.contains(&[0, 1, 0][..]) && patterns.contains(&[0, 1, 1][..])
            }
            Type::Nominal { definition, .. } => {
                let declared = self
                    .variants
                    .keys()
                    .filter(|variant| variant.owner == *definition)
                    .copied()
                    .collect::<BTreeSet<_>>();
                !declared.is_empty()
                    && declared.iter().all(|id| {
                        cases.iter().any(|case| {
                            case.guard.is_none()
                                && case.pattern.as_ref().is_some_and(|pattern| {
                                    self.pattern_covers_variant(pattern, type_, *id)
                                })
                        })
                    })
            }
            _ => false,
        }
    }

    fn pattern_irrefutable(&self, pattern: &HirMatchPattern, type_: &Type) -> bool {
        match pattern {
            HirMatchPattern::Wildcard | HirMatchPattern::Binding { .. } => true,
            HirMatchPattern::Literal(_) | HirMatchPattern::Variant { .. } => false,
            HirMatchPattern::Or(alternatives) => alternatives
                .iter()
                .any(|pattern| self.pattern_irrefutable(pattern, type_)),
            HirMatchPattern::Tuple(patterns) => {
                let Type::Tuple(types) = type_ else {
                    return false;
                };
                patterns.len() == types.len()
                    && patterns
                        .iter()
                        .zip(types.iter())
                        .all(|(pattern, type_)| self.pattern_irrefutable(pattern, type_))
            }
            HirMatchPattern::FixedArray(patterns) => {
                let (element, length) = match type_ {
                    Type::Array(element) => (element.as_ref(), patterns.len() as u64),
                    Type::FixedArray { element, length } => {
                        (element.as_ref(), length.value().unwrap_or(u64::MAX))
                    }
                    _ => return false,
                };
                patterns.len() as u64 == length
                    && patterns
                        .iter()
                        .all(|pattern| self.pattern_irrefutable(pattern, element))
            }
            HirMatchPattern::Struct { definition, fields } => {
                let Type::Nominal {
                    definition: expected,
                    arguments,
                    ..
                } = type_
                else {
                    return false;
                };
                let Some(struct_) = self.structs.get(definition) else {
                    return false;
                };
                let selected_fields = if struct_.field_selections.is_empty() {
                    Arc::from(struct_.fields.clone())
                } else {
                    let Some(fields) = struct_.applied_fields.borrow().get(arguments).cloned()
                    else {
                        return false;
                    };
                    fields
                };
                if definition != expected
                    || fields.len() != selected_fields.len()
                    || arguments.len() != struct_.type_parameters.len()
                {
                    return false;
                }
                let substitutions = struct_
                    .type_parameters
                    .iter()
                    .copied()
                    .zip(arguments.iter().cloned())
                    .collect::<BTreeMap<_, _>>();
                fields
                    .iter()
                    .zip(selected_fields.iter())
                    .all(|(pattern, field)| {
                        self.pattern_irrefutable(pattern, &substitute(&field.type_, &substitutions))
                    })
            }
        }
    }

    fn pattern_covers_variant(
        &self,
        pattern: &HirMatchPattern,
        enum_type: &Type,
        expected_id: VariantId,
    ) -> bool {
        if let HirMatchPattern::Or(alternatives) = pattern {
            return alternatives
                .iter()
                .any(|pattern| self.pattern_covers_variant(pattern, enum_type, expected_id));
        }
        let HirMatchPattern::Variant { id, payload } = pattern else {
            return false;
        };
        if *id != expected_id {
            return false;
        }
        let Type::Nominal { arguments, .. } = enum_type else {
            return false;
        };
        let Some(variant) = self.variants.get(id) else {
            return false;
        };
        if arguments.len() != variant.type_parameters.len()
            || payload.len() != variant.parameters.len()
        {
            return false;
        }
        let substitutions = variant
            .type_parameters
            .iter()
            .copied()
            .zip(arguments.iter().cloned())
            .collect::<BTreeMap<_, _>>();
        payload
            .iter()
            .zip(&variant.parameters)
            .all(|(pattern, parameter)| {
                self.pattern_irrefutable(pattern, &substitute(&parameter.type_, &substitutions))
            })
    }

    fn lower_match_pattern(
        &mut self,
        pattern: &crate::syntax::PatternSyntax,
        expected: &Type,
    ) -> Result<Option<HirMatchPattern>, VerificationFailure> {
        self.lower_match_pattern_with_reuse(pattern, expected, None)
            .map(Some)
    }

    fn lower_match_pattern_with_reuse(
        &mut self,
        pattern: &crate::syntax::PatternSyntax,
        expected: &Type,
        reuse: Option<&BTreeMap<String, (LocalId, Type, bool)>>,
    ) -> Result<HirMatchPattern, VerificationFailure> {
        match &pattern.kind {
            PatternSyntaxKind::Wildcard => Ok(HirMatchPattern::Wildcard),
            PatternSyntaxKind::Binding(name) => {
                let local = if let Some((local, type_, _)) = reuse.and_then(|reuse| reuse.get(name))
                {
                    if !can_initialize(type_, expected) || !can_initialize(expected, type_) {
                        return creator(CreatorFailureKind::InvalidMatchPattern, &pattern.range);
                    }
                    *local
                } else {
                    self.bind_source_local(name, expected.clone(), false, &pattern.range)?
                };
                let access = if self.type_owns_resource(expected) {
                    self.non_custodial_locals.insert(local);
                    AccessMode::Read
                } else {
                    AccessMode::Copy
                };
                Ok(HirMatchPattern::Binding {
                    local,
                    type_id: observe_type_tree(
                        expected,
                        self.identity_catalog,
                        self.discharge_laws,
                    )?,
                    type_: expected.clone(),
                    access,
                })
            }
            PatternSyntaxKind::Take(inner) => {
                let PatternSyntaxKind::Binding(name) = &inner.kind else {
                    return creator(CreatorFailureKind::InvalidMatchPattern, &pattern.range);
                };
                if !self.type_owns_resource(expected) {
                    return creator(CreatorFailureKind::InvalidMatchPattern, &pattern.range);
                }
                let local = if let Some((local, type_, _)) = reuse.and_then(|reuse| reuse.get(name))
                {
                    if !can_initialize(type_, expected) || !can_initialize(expected, type_) {
                        return creator(CreatorFailureKind::InvalidMatchPattern, &pattern.range);
                    }
                    *local
                } else {
                    self.bind_source_local(name, expected.clone(), false, &inner.range)?
                };
                self.non_custodial_locals.remove(&local);
                Ok(HirMatchPattern::Binding {
                    local,
                    type_id: observe_type_tree(
                        expected,
                        self.identity_catalog,
                        self.discharge_laws,
                    )?,
                    type_: expected.clone(),
                    access: AccessMode::Move,
                })
            }
            PatternSyntaxKind::Literal(literal) => {
                let expression = self.expression(literal)?;
                if !can_unify(&expression.type_, expected) {
                    return creator(CreatorFailureKind::BinaryTypeMismatch, &pattern.range);
                }
                match expression.kind {
                    ExpressionKind::Literal(literal) => Ok(HirMatchPattern::Literal(literal)),
                    _ => creator(CreatorFailureKind::InvalidMatchPattern, &pattern.range),
                }
            }
            PatternSyntaxKind::Tuple(members) => {
                let Type::Tuple(expected_members) = expected else {
                    return creator(CreatorFailureKind::BinaryTypeMismatch, &pattern.range);
                };
                if members.len() != expected_members.len() {
                    return creator(CreatorFailureKind::InvalidMatchPattern, &pattern.range);
                }
                members
                    .iter()
                    .zip(expected_members.iter())
                    .map(|(member, expected)| {
                        self.lower_match_pattern_with_reuse(member, expected, reuse)
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map(|members| HirMatchPattern::Tuple(members.into()))
            }
            PatternSyntaxKind::FixedArray(members) => {
                let (element, length) = match expected {
                    Type::FixedArray { element, length } => {
                        (element.as_ref(), length.value().unwrap_or(u64::MAX))
                    }
                    Type::Array(element) => (element.as_ref(), members.len() as u64),
                    _ => return creator(CreatorFailureKind::BinaryTypeMismatch, &pattern.range),
                };
                if members.len() as u64 != length {
                    return creator(CreatorFailureKind::InvalidMatchPattern, &pattern.range);
                }
                members
                    .iter()
                    .map(|member| self.lower_match_pattern_with_reuse(member, element, reuse))
                    .collect::<Result<Vec<_>, _>>()
                    .map(|members| HirMatchPattern::FixedArray(members.into()))
            }
            PatternSyntaxKind::Or(alternatives) => {
                let before = self.locals.clone();
                let first = alternatives.first().ok_or_else(|| {
                    creator_value(CreatorFailureKind::InvalidMatchPattern, &pattern.range)
                })?;
                let first = self.lower_match_pattern_with_reuse(first, expected, reuse)?;
                let bindings = self
                    .locals
                    .iter()
                    .filter(|(name, _)| !before.contains_key(*name))
                    .map(|(name, binding)| (name.clone(), binding.clone()))
                    .collect::<BTreeMap<_, _>>();
                let signature = match_pattern_binding_signature(&first);
                let mut lowered = vec![first];
                for alternative in alternatives.iter().skip(1) {
                    let alternative = self.lower_match_pattern_with_reuse(
                        alternative,
                        expected,
                        Some(&bindings),
                    )?;
                    if match_pattern_binding_signature(&alternative) != signature {
                        return creator(CreatorFailureKind::InvalidMatchPattern, &pattern.range);
                    }
                    lowered.push(alternative);
                }
                Ok(HirMatchPattern::Or(lowered.into()))
            }
            PatternSyntaxKind::Constructor { name, arguments } => {
                let Type::Nominal {
                    definition,
                    arguments: type_arguments,
                    ..
                } = expected
                else {
                    return creator(CreatorFailureKind::BinaryTypeMismatch, &pattern.range);
                };
                if let [struct_name] = name.segments.as_slice() {
                    let Some(ResolvedName::Nominal(resolved)) = self
                        .namespace
                        .resolve(self.module, std::slice::from_ref(struct_name))
                    else {
                        return creator(CreatorFailureKind::InvalidMatchPattern, &pattern.range);
                    };
                    if resolved != *definition {
                        return creator(CreatorFailureKind::BinaryTypeMismatch, &pattern.range);
                    }
                    let Some(struct_) = self.structs.get(&resolved).cloned() else {
                        return creator(CreatorFailureKind::InvalidMatchPattern, &pattern.range);
                    };
                    let fields = self.struct_fields(resolved, type_arguments, &pattern.range)?;
                    if arguments.len() != fields.len()
                        || type_arguments.len() != struct_.type_parameters.len()
                    {
                        return creator(CreatorFailureKind::InvalidMatchPattern, &pattern.range);
                    }
                    let substitutions = struct_
                        .type_parameters
                        .iter()
                        .copied()
                        .zip(type_arguments.iter().cloned())
                        .collect::<BTreeMap<_, _>>();
                    let mut ordered = vec![None; fields.len()];
                    for argument in arguments {
                        let Some(label) = &argument.label else {
                            return creator(
                                CreatorFailureKind::InvalidMatchPattern,
                                &argument.pattern.range,
                            );
                        };
                        let index = fields
                            .iter()
                            .position(|field| field.name == *label)
                            .ok_or_else(|| {
                                creator_value(
                                    CreatorFailureKind::InvalidMatchPattern,
                                    &argument.pattern.range,
                                )
                            })?;
                        if ordered[index].is_some() {
                            return creator(
                                CreatorFailureKind::InvalidMatchPattern,
                                &argument.pattern.range,
                            );
                        }
                        ordered[index] = Some(self.lower_match_pattern_with_reuse(
                            &argument.pattern,
                            &substitute(&fields[index].type_, &substitutions),
                            reuse,
                        )?);
                    }
                    return Ok(HirMatchPattern::Struct {
                        definition: resolved,
                        fields: ordered
                            .into_iter()
                            .map(|field| field.unwrap_or(HirMatchPattern::Wildcard))
                            .collect::<Vec<_>>()
                            .into(),
                    });
                }
                let [owner_name, variant_name] = name.segments.as_slice() else {
                    return creator(CreatorFailureKind::InvalidMatchPattern, &pattern.range);
                };
                let Some(ResolvedName::Nominal(owner)) = self
                    .namespace
                    .resolve(self.module, std::slice::from_ref(owner_name))
                else {
                    return creator(CreatorFailureKind::InvalidMatchPattern, &pattern.range);
                };
                if *definition != owner {
                    return creator(CreatorFailureKind::BinaryTypeMismatch, &pattern.range);
                }
                let id = self
                    .identity_catalog
                    .variant(owner, variant_name)
                    .ok_or_else(|| {
                        creator_value(CreatorFailureKind::InvalidMatchPattern, &pattern.range)
                    })?;
                let Some(variant) = self.variants.get(&id).cloned() else {
                    return creator(CreatorFailureKind::InvalidMatchPattern, &pattern.range);
                };
                if variant.parameters.len() != arguments.len()
                    || variant.type_parameters.len() != type_arguments.len()
                {
                    return creator(CreatorFailureKind::InvalidMatchPattern, &pattern.range);
                }
                let substitutions = variant
                    .type_parameters
                    .iter()
                    .copied()
                    .zip(type_arguments.iter().cloned())
                    .collect::<BTreeMap<_, _>>();
                let mut ordered = vec![None; variant.parameters.len()];
                let mut saw_named = false;
                for (source_index, argument) in arguments.iter().enumerate() {
                    let parameter_index = if let Some(label) = &argument.label {
                        saw_named = true;
                        variant
                            .parameters
                            .iter()
                            .position(|parameter| parameter.name == *label)
                            .ok_or_else(|| {
                                creator_value(
                                    CreatorFailureKind::InvalidMatchPattern,
                                    &argument.pattern.range,
                                )
                            })?
                    } else {
                        if saw_named {
                            return creator(
                                CreatorFailureKind::InvalidMatchPattern,
                                &argument.pattern.range,
                            );
                        }
                        source_index
                    };
                    if ordered[parameter_index].is_some() {
                        return creator(
                            CreatorFailureKind::InvalidMatchPattern,
                            &argument.pattern.range,
                        );
                    }
                    let expected =
                        substitute(&variant.parameters[parameter_index].type_, &substitutions);
                    ordered[parameter_index] = Some(self.lower_match_pattern_with_reuse(
                        &argument.pattern,
                        &expected,
                        reuse,
                    )?);
                }
                let payload = ordered
                    .into_iter()
                    .map(|pattern| pattern.unwrap_or(HirMatchPattern::Wildcard))
                    .collect::<Vec<_>>();
                Ok(HirMatchPattern::Variant {
                    id,
                    payload: payload.into(),
                })
            }
        }
    }

    fn value(
        &mut self,
        name: &NameSyntax,
        syntax: &ExpressionSyntax,
    ) -> Result<Expression, VerificationFailure> {
        if let [local, fields @ ..] = name.segments.as_slice()
            && !fields.is_empty()
            && self.locals.contains_key(local)
        {
            let (id, mut type_, _) = self.locals.get(local).cloned().expect("checked local");
            let mut projections = Vec::with_capacity(fields.len());
            for field_name in fields {
                let Type::Nominal {
                    definition,
                    ref arguments,
                    ..
                } = type_
                else {
                    return creator(CreatorFailureKind::UnresolvedName, &syntax.range);
                };
                let (struct_module, type_parameters) = self
                    .structs
                    .get(&definition)
                    .map(|struct_| (struct_.module, struct_.type_parameters.clone()))
                    .ok_or_else(|| {
                        creator_value(CreatorFailureKind::UnresolvedName, &syntax.range)
                    })?;
                let selected_fields = self.struct_fields(definition, arguments, &syntax.range)?;
                let field = selected_fields
                    .iter()
                    .find(|field| field.name == *field_name)
                    .ok_or_else(|| {
                        creator_value(CreatorFailureKind::UnresolvedName, &syntax.range)
                    })?;
                if struct_module != self.module && !field.public {
                    return creator(CreatorFailureKind::UnresolvedName, &syntax.range);
                }
                if arguments.len() != type_parameters.len() {
                    return creator(CreatorFailureKind::GenericArgumentConflict, &syntax.range);
                }
                let substitutions = type_parameters
                    .iter()
                    .copied()
                    .zip(arguments.iter().cloned())
                    .collect::<BTreeMap<_, _>>();
                let field_type = substitute(&field.type_, &substitutions);
                projections.push(PlaceProjection::Field {
                    definition,
                    name: Arc::from(field.name.as_str()),
                    type_: field_type.clone(),
                    mutable: field.mutable,
                });
                type_ = field_type;
            }
            let place = Place {
                local: id,
                projections: projections.into(),
            };
            if self.moved.is_unreadable(&place) {
                return Err(self.custody_failure(
                    CreatorFailureKind::ReadAfterMove,
                    &syntax.range,
                    &PlaceKey::from_place(&place),
                    CustodyDiagnosticState::Moved,
                    self.moved.origin(&place).cloned(),
                ));
            }
            return self.finish_expression(
                ExpressionKind::Read(place),
                type_,
                syntax.range.clone(),
            );
        }
        if let [local] = name.segments.as_slice()
            && let Some((id, type_, _)) = self.locals.get(local)
        {
            if self.moved.contains_local(*id) {
                let place = Place::local(*id);
                return Err(self.custody_failure(
                    CreatorFailureKind::ReadAfterMove,
                    &syntax.range,
                    &PlaceKey::from_place(&place),
                    CustodyDiagnosticState::Moved,
                    self.moved.origin(&place).cloned(),
                ));
            }
            return self.finish_expression(
                ExpressionKind::Read(Place::local(*id)),
                type_.clone(),
                syntax.range.clone(),
            );
        }
        if let [name] = name.segments.as_slice()
            && let Some((kind, value)) = self.generic_const_values.get(name).copied()
        {
            return self.finish_expression(
                ExpressionKind::Literal(Literal::Integer {
                    kind,
                    value: i128::from(value),
                }),
                Type::Integer(kind),
                syntax.range.clone(),
            );
        }
        if let Some(ResolvedName::Constant(id)) =
            self.namespace.resolve(self.module, &name.segments)
        {
            let constant = self
                .constants
                .get(&id)
                .ok_or_else(|| creator_value(CreatorFailureKind::UnresolvedName, &syntax.range))?;
            return self.finish_expression(
                ExpressionKind::Constant(id),
                constant.type_.clone(),
                syntax.range.clone(),
            );
        }
        if let Some(ResolvedName::Function(id)) =
            self.namespace.resolve(self.module, &name.segments)
        {
            let (specialization, type_) = self.function_value(id, &syntax.range)?;
            return self.finish_expression(
                ExpressionKind::FunctionValue {
                    definition: id,
                    specialization,
                },
                type_,
                syntax.range.clone(),
            );
        }
        if resolve_builtin_variant(name).is_some() || name.segments.len() == 2 {
            let (target, type_) = self.call(name, &[], &[], &syntax.range)?;
            return self.finish_expression(
                ExpressionKind::Call {
                    target,
                    arguments: Arc::from([]),
                },
                type_,
                syntax.range.clone(),
            );
        }
        creator(CreatorFailureKind::UnresolvedName, &syntax.range)
    }

    fn resolve_build_primitive(&self, name: &NameSyntax) -> Option<BuildPrimitive> {
        self.build_authority.resolve_name(name).or_else(|| {
            let ResolvedName::Function(definition) =
                self.namespace.resolve(self.module, &name.segments)?
            else {
                return None;
            };
            self.build_authority.resolve_definition(definition)
        })
    }

    fn build_call(
        &self,
        primitive: BuildPrimitive,
        arguments: &[Expression],
        labels: &[Option<String>],
        site: &SourceRange,
    ) -> Result<(CallTarget, Type), VerificationFailure> {
        let return_type = match primitive.kind {
            BuildKind::Image => {
                let mut seen = BTreeSet::new();
                if labels.iter().any(|label| {
                    label
                        .as_ref()
                        .is_none_or(|label| !seen.insert(label.clone()))
                }) {
                    return creator(CreatorFailureKind::ArgumentLabelMismatch, site);
                }
                Type::Builtin(BuiltinType::Image)
            }
            BuildKind::Test => {
                let cases_type = Type::Array(Arc::new(Type::Builtin(BuiltinType::TestApplication)));
                let signature = CallableSignature {
                    parameters: vec![CallableParameter {
                        name: "cases",
                        type_: &cases_type,
                    }],
                    label_mode: LabelMode::Required,
                };
                check_callable_signature(&signature, arguments, labels, site)?;
                Type::Builtin(BuiltinType::Test)
            }
            BuildKind::Node {
                definition: owner, ..
            } => {
                let function = self.functions.get(&primitive.definition).ok_or_else(|| {
                    VerificationFailure::Defect {
                        evidence: Arc::from(
                            "sealed Build Constructor is absent from the function catalog",
                        ),
                    }
                })?;
                check_callable(
                    &function.parameters,
                    arguments,
                    labels,
                    LabelMode::Required,
                    site,
                )?;
                if !matches!(
                    function.return_type,
                    Type::Nominal { definition, .. } if definition == owner
                ) {
                    return defect(
                        "sealed Build Constructor return type disagrees with its registry entry",
                    );
                }
                function.return_type.clone()
            }
        };
        let labels = labels
            .iter()
            .map(|label| {
                label
                    .as_deref()
                    .map(Arc::from)
                    .ok_or_else(|| creator_value(CreatorFailureKind::ArgumentLabelMismatch, site))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((
            CallTarget::Build {
                primitive,
                labels: labels.into(),
            },
            return_type,
        ))
    }

    fn apply_build_argument_types_and_modes(
        &mut self,
        primitive: BuildPrimitive,
        arguments: &mut [Expression],
        labels: &[Option<String>],
        site: &SourceRange,
    ) -> Result<(), VerificationFailure> {
        let BuildKind::Node { .. } = primitive.kind else {
            return Ok(());
        };
        let function = self.functions.get(&primitive.definition).ok_or_else(|| {
            VerificationFailure::Defect {
                evidence: Arc::from("sealed Build Constructor is absent from the function catalog"),
            }
        })?;
        let argument_order = check_callable(
            &function.parameters,
            arguments,
            labels,
            LabelMode::Required,
            site,
        )?;
        let parameter_context = argument_order
            .iter()
            .map(|parameter| {
                let parameter = &function.parameters[usize::from(*parameter)];
                (parameter.type_.clone(), parameter.ownership)
            })
            .collect::<Vec<_>>();
        for (argument, (expected, ownership)) in arguments.iter_mut().zip(parameter_context) {
            self.apply_expected_type(argument, &expected)?;
            if !can_pass(&argument.type_, &expected) {
                return creator(CreatorFailureKind::ArgumentTypeMismatch, &argument.source);
            }
            self.apply_authored_ownership(argument, ownership)?;
        }
        Ok(())
    }

    fn apply_authored_ownership(
        &mut self,
        argument: &mut Expression,
        ownership: OwnershipSyntax,
    ) -> Result<(), VerificationFailure> {
        match ownership {
            OwnershipSyntax::Value if argument.access == AccessMode::Copy => Ok(()),
            OwnershipSyntax::Read if argument.access == AccessMode::Copy => {
                argument.access = AccessMode::Read;
                Ok(())
            }
            OwnershipSyntax::Mut if argument.access == AccessMode::Mut => Ok(()),
            OwnershipSyntax::Take
                if argument.access == AccessMode::Move || root_place(argument).is_none() =>
            {
                argument.access = AccessMode::Move;
                Ok(())
            }
            _ => creator(
                CreatorFailureKind::ArgumentOwnershipMismatch,
                &argument.source,
            ),
        }
    }

    fn validate_call_custody(&self, arguments: &[Expression]) -> Result<(), VerificationFailure> {
        let mut active: Vec<(PlaceKey, AccessMode, SourceRange)> = Vec::new();
        for argument in arguments {
            let Some(place) = root_place(argument).map(|place| PlaceKey::from_place(&place)) else {
                continue;
            };
            let access = argument.access;
            if !matches!(
                access,
                AccessMode::Read | AccessMode::Mut | AccessMode::Move
            ) {
                continue;
            }
            if let Some((_, _, related)) = active.iter().find(|(other, other_access, _)| {
                other.conflicts(&place)
                    && !matches!((other_access, access), (AccessMode::Read, AccessMode::Read))
            }) {
                let place = root_place(argument).expect("loan place was resolved above");
                return Err(self.custody_failure(
                    CreatorFailureKind::LoanConflict,
                    &argument.source,
                    &PlaceKey::from_place(&place),
                    CustodyDiagnosticState::ConflictingLoan,
                    Some(related.clone()),
                ));
            }
            active.push((place, access, argument.source.clone()));
        }
        Ok(())
    }

    fn call(
        &mut self,
        name: &NameSyntax,
        arguments: &[Expression],
        labels: &[Option<String>],
        site: &SourceRange,
    ) -> Result<(CallTarget, Type), VerificationFailure> {
        if let Some(ResolvedName::Function(id)) =
            self.namespace.resolve(self.module, &name.segments)
        {
            if let Some(primitive) = self.build_authority.resolve_definition(id) {
                return self.build_call(primitive, arguments, labels, site);
            }
            return self.function_call(id, arguments, labels, site);
        }
        if let Some(ResolvedName::Nominal(id)) = self.namespace.resolve(self.module, &name.segments)
            && let Some(struct_) = self.structs.get(&id).cloned()
        {
            if struct_.definition != id {
                return defect("struct catalog key disagrees with DefinitionId");
            }
            let mut substitutions = BTreeMap::new();
            if let Some(Type::Nominal {
                definition,
                arguments: expected_arguments,
                ..
            }) = self.expected_expression_type.as_ref()
                && *definition == id
                && expected_arguments.len() == struct_.type_parameters.len()
            {
                substitutions.extend(
                    struct_
                        .type_parameters
                        .iter()
                        .copied()
                        .zip(expected_arguments.iter().cloned()),
                );
            }
            let known_arguments = struct_
                .type_parameters
                .iter()
                .map(|parameter| substitutions.get(parameter).cloned())
                .collect::<Option<Vec<_>>>();
            let binding_fields = if let Some(arguments) = &known_arguments {
                self.struct_fields(id, arguments, site)?
            } else {
                struct_.fields.clone().into()
            };
            for (argument, label) in arguments.iter().zip(labels) {
                let label = label.as_deref().ok_or_else(|| {
                    creator_value(CreatorFailureKind::ArgumentLabelMismatch, site)
                })?;
                let field = binding_fields
                    .iter()
                    .find(|field| field.name == label)
                    .ok_or_else(|| {
                        creator_value(CreatorFailureKind::ArgumentLabelMismatch, site)
                    })?;
                bind_type(&field.type_, &argument.type_, &mut substitutions, site)?;
            }
            let type_arguments = struct_
                .type_parameters
                .iter()
                .map(|parameter| {
                    substitutions.get(parameter).cloned().ok_or_else(|| {
                        creator_value(CreatorFailureKind::GenericArgumentConflict, site)
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            self.validate_generic_arguments(&struct_.generic_constraints, &type_arguments, site)?;
            let fields = self.struct_fields(id, &type_arguments, site)?;
            if struct_.module != self.module && fields.iter().any(|field| !field.public) {
                return creator(CreatorFailureKind::UnresolvedCall, site);
            }
            let signature = CallableSignature {
                parameters: fields
                    .iter()
                    .map(|field| CallableParameter {
                        name: &field.name,
                        type_: &field.type_,
                    })
                    .collect(),
                label_mode: LabelMode::Required,
            };
            check_callable_signature(&signature, arguments, labels, site)?;
            let argument_fields = labels
                .iter()
                .map(|label| {
                    label.as_deref().map(Arc::from).ok_or_else(|| {
                        creator_value(CreatorFailureKind::ArgumentLabelMismatch, site)
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let argument_field_definitions = labels
                .iter()
                .map(|label| {
                    let label = label.as_deref().ok_or_else(|| {
                        creator_value(CreatorFailureKind::ArgumentLabelMismatch, site)
                    })?;
                    fields
                        .iter()
                        .find(|field| field.name == label)
                        .map(|field| field.definition)
                        .ok_or_else(|| {
                            creator_value(CreatorFailureKind::ArgumentLabelMismatch, site)
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            return Ok((
                CallTarget::Struct {
                    definition: id,
                    type_display: struct_.display.clone(),
                    field_order: fields
                        .iter()
                        .map(|field| Arc::from(field.name.as_str()))
                        .collect(),
                    argument_fields: argument_fields.into(),
                    argument_field_definitions: argument_field_definitions.into(),
                },
                Type::Nominal {
                    definition: id,
                    display: struct_.display.clone(),
                    arguments: type_arguments.into(),
                },
            ));
        }
        if let Some(primitive) = self.build_authority.resolve_name(name) {
            return self.build_call(primitive, arguments, labels, site);
        }
        if let Some(ResolvedName::Test(id)) = self.namespace.resolve(self.module, &name.segments) {
            let test = self
                .tests
                .get(&id)
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from("resolved test absent from catalog"),
                })?;
            let argument_order = check_callable(
                &test.parameters,
                arguments,
                labels,
                LabelMode::Optional,
                site,
            )?;
            let argument_parameters = argument_order
                .iter()
                .map(|parameter| test.parameters[usize::from(*parameter)].definition)
                .collect();
            return Ok((
                CallTarget::Test {
                    id,
                    argument_order,
                    argument_parameters,
                },
                Type::Builtin(BuiltinType::TestApplication),
            ));
        }
        if let Some(variant) = resolve_builtin_variant(name) {
            let inferred = Type::Infer;
            let parameters = match variant {
                BuiltinVariant::OptionNone => Vec::new(),
                BuiltinVariant::ResultOk | BuiltinVariant::OptionSome => {
                    vec![CallableParameter {
                        name: "value",
                        type_: &inferred,
                    }]
                }
                BuiltinVariant::ResultErr => vec![CallableParameter {
                    name: "error",
                    type_: &inferred,
                }],
            };
            check_callable_signature(
                &CallableSignature {
                    parameters,
                    label_mode: LabelMode::Optional,
                },
                arguments,
                labels,
                site,
            )?;
            return Ok((
                CallTarget::BuiltinVariant(variant),
                builtin_variant_type(variant, arguments, site)?,
            ));
        }
        if name.segments.len() == 2 {
            if self.namespace.has_binding(self.module, &name.segments[0]) {
                return creator(CreatorFailureKind::UnresolvedCall, site);
            }
            let Some(ResolvedName::Nominal(owner)) =
                self.namespace.resolve(self.module, &name.segments[..1])
            else {
                return creator(CreatorFailureKind::UnresolvedNominalType, site);
            };
            let display = self.nominal_displays.get(&owner).cloned().ok_or_else(|| {
                VerificationFailure::Defect {
                    evidence: Arc::from("nominal display absent from catalog"),
                }
            })?;
            let Some(variant_id) = self.identity_catalog.variant(owner, &name.segments[1]) else {
                return creator(CreatorFailureKind::UnresolvedCall, site);
            };
            let variant =
                self.variants
                    .get(&variant_id)
                    .ok_or_else(|| VerificationFailure::Defect {
                        evidence: Arc::from("resolved variant absent from semantic catalog"),
                    })?;
            let argument_order = check_callable(
                &variant.parameters,
                arguments,
                labels,
                LabelMode::Optional,
                site,
            )?;
            let mut substitutions = BTreeMap::new();
            if let Some(Type::Nominal {
                definition,
                arguments: expected_arguments,
                ..
            }) = self.expected_expression_type.as_ref()
                && *definition == owner
                && expected_arguments.len() == variant.type_parameters.len()
            {
                substitutions.extend(
                    variant
                        .type_parameters
                        .iter()
                        .copied()
                        .zip(expected_arguments.iter().cloned()),
                );
            }
            for (source_index, parameter_index) in argument_order.iter().enumerate() {
                bind_type(
                    &variant.parameters[usize::from(*parameter_index)].type_,
                    &arguments[source_index].type_,
                    &mut substitutions,
                    site,
                )?;
            }
            let type_arguments = variant
                .type_parameters
                .iter()
                .map(|parameter| {
                    substitutions.get(parameter).cloned().ok_or_else(|| {
                        creator_value(CreatorFailureKind::GenericArgumentConflict, site)
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            self.validate_generic_arguments(&variant.generic_constraints, &type_arguments, site)?;
            let argument_parameters = argument_order
                .iter()
                .map(|parameter| variant.parameters[usize::from(*parameter)].definition)
                .collect();
            return Ok((
                CallTarget::UserVariant {
                    id: variant_id,
                    variant_order: variant.order,
                    type_display: display.clone(),
                    variant_display: Arc::from(name.segments[1].as_str()),
                    argument_order,
                    argument_parameters,
                },
                Type::Nominal {
                    definition: owner,
                    display,
                    arguments: type_arguments.into(),
                },
            ));
        }
        creator(CreatorFailureKind::UnresolvedCall, site)
    }

    fn function_call(
        &mut self,
        id: DefinitionId,
        arguments: &[Expression],
        labels: &[Option<String>],
        site: &SourceRange,
    ) -> Result<(CallTarget, Type), VerificationFailure> {
        let function = self
            .functions
            .get(&id)
            .ok_or_else(|| VerificationFailure::Defect {
                evidence: Arc::from(format!(
                    "resolved function {:032x} absent from catalog; available: {}",
                    id.0,
                    self.functions
                        .values()
                        .map(|function| function.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            })?;
        let argument_order = check_callable(
            &function.parameters,
            arguments,
            labels,
            LabelMode::Optional,
            site,
        )?;
        let mut substitutions = BTreeMap::new();
        for (source_index, parameter_index) in argument_order.iter().enumerate() {
            bind_type(
                &function.parameters[usize::from(*parameter_index)].type_,
                &arguments[source_index].type_,
                &mut substitutions,
                site,
            )?;
        }
        let return_type = substitute(&function.return_type, &substitutions);
        if !self.concrete_result_type_is_well_formed(&return_type)
            || function.parameters.iter().any(|parameter| {
                !self.concrete_result_type_is_well_formed(&substitute(
                    &parameter.type_,
                    &substitutions,
                ))
            })
        {
            return creator(CreatorFailureKind::GenericArgumentConflict, site);
        }
        let argument_parameters = argument_order
            .iter()
            .map(|parameter| function.parameters[usize::from(*parameter)].definition)
            .collect::<Arc<[_]>>();
        if !self.concrete_context {
            return Ok((
                CallTarget::TemplateFunction {
                    definition: id,
                    argument_order,
                    argument_parameters,
                },
                return_type,
            ));
        }
        let type_arguments = function
            .type_parameters
            .iter()
            .map(|parameter| {
                substitutions
                    .get(parameter)
                    .cloned()
                    .ok_or_else(|| VerificationFailure::Creator {
                        kind: CreatorFailureKind::GenericArgumentConflict,
                        site: site.clone(),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.validate_generic_arguments(&function.generic_constraints, &type_arguments, site)?;
        let specialization_id = self
            .identity_catalog
            .specialization(id, &type_arguments)
            .map_err(|collision| VerificationFailure::Defect {
                evidence: Arc::from(format!(
                    "specialization identity collision {:032x}",
                    collision.digest
                )),
            })?;
        let specialization = SpecializationRecord {
            id: specialization_id,
            definition: id,
            type_arguments: type_arguments.into(),
        };
        if let std::collections::btree_map::Entry::Vacant(entry) =
            self.specialization_demands.records.entry(specialization.id)
        {
            entry.insert(specialization.clone());
            self.specialization_demands
                .pending
                .insert(specialization.id);
        }
        Ok((
            CallTarget::Function {
                definition: id,
                specialization: specialization.id,
                argument_order,
                argument_parameters,
            },
            return_type,
        ))
    }

    fn interface_call(
        &mut self,
        interface_id: DefinitionId,
        member: &str,
        arguments: &[Expression],
        labels: &[Option<String>],
        site: &SourceRange,
    ) -> Result<(CallTarget, Type), VerificationFailure> {
        let interface = self
            .interfaces
            .get(&interface_id)
            .ok_or_else(|| creator_value(CreatorFailureKind::UnresolvedCall, site))?;
        let requirement = interface
            .requirements
            .get(member)
            .cloned()
            .ok_or_else(|| creator_value(CreatorFailureKind::UnresolvedCall, site))?;
        let argument_order = check_callable(
            &requirement.parameters,
            arguments,
            labels,
            LabelMode::Optional,
            site,
        )?;
        let argument_parameters = argument_order
            .iter()
            .map(|parameter| requirement.parameters[usize::from(*parameter)].definition)
            .collect();
        if !self.concrete_context {
            return Ok((
                CallTarget::Interface {
                    interface: interface_id,
                    alternatives: Arc::from([]),
                    argument_order,
                    argument_parameters,
                },
                requirement.return_type,
            ));
        }
        let implementation_ids = interface
            .implementations
            .iter()
            .map(|(nominal, members)| {
                members
                    .get(member)
                    .copied()
                    .map(|implementation| (*nominal, implementation))
                    .ok_or_else(|| creator_value(CreatorFailureKind::UnresolvedCall, site))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if implementation_ids.is_empty() {
            return creator(CreatorFailureKind::UnresolvedCall, site);
        }
        let mut alternatives = Vec::with_capacity(implementation_ids.len());
        for (nominal, implementation) in implementation_ids {
            let function = &self.functions[&implementation];
            if !function.type_parameters.is_empty() {
                return creator(CreatorFailureKind::GenericArgumentConflict, site);
            }
            let specialization_id = self
                .identity_catalog
                .specialization(implementation, &[])
                .map_err(|collision| VerificationFailure::Defect {
                    evidence: Arc::from(format!(
                        "specialization identity collision {:032x}",
                        collision.digest
                    )),
                })?;
            if let std::collections::btree_map::Entry::Vacant(entry) =
                self.specialization_demands.records.entry(specialization_id)
            {
                entry.insert(SpecializationRecord {
                    id: specialization_id,
                    definition: implementation,
                    type_arguments: Arc::from([]),
                });
                self.specialization_demands
                    .pending
                    .insert(specialization_id);
            }
            alternatives.push((nominal, implementation, specialization_id));
        }
        Ok((
            CallTarget::Interface {
                interface: interface_id,
                alternatives: alternatives.into(),
                argument_order,
                argument_parameters,
            },
            requirement.return_type.clone(),
        ))
    }

    fn validate_generic_arguments(
        &self,
        constraints: &[GenericConstraint],
        arguments: &[Type],
        site: &SourceRange,
    ) -> Result<(), VerificationFailure> {
        if constraints.len() != arguments.len() {
            return creator(CreatorFailureKind::GenericArgumentConflict, site);
        }
        for (constraint, argument) in constraints.iter().zip(arguments) {
            let valid = match constraint {
                GenericConstraint::Type { interface: None } => !matches!(
                    argument,
                    Type::ConstU64(_)
                        | Type::PoolArgument(_)
                        | Type::Infer
                        | Type::Parameter { .. }
                ),
                GenericConstraint::Type {
                    interface: Some(interface),
                } => {
                    let Type::Nominal { definition, .. } = argument else {
                        return creator(CreatorFailureKind::GenericArgumentConflict, site);
                    };
                    self.interfaces
                        .get(interface)
                        .is_some_and(|interface| interface.implementations.contains_key(definition))
                }
                GenericConstraint::Const { type_ } => match (type_, argument) {
                    (Type::Integer(kind), Type::ConstU64(value)) => kind.fits(i128::from(*value)),
                    _ => false,
                },
                GenericConstraint::Pool => matches!(argument, Type::PoolArgument(_)),
            };
            if !valid {
                return creator(CreatorFailureKind::GenericArgumentConflict, site);
            }
        }
        Ok(())
    }

    fn concrete_result_type_is_well_formed(&self, type_: &Type) -> bool {
        match type_ {
            Type::Result { success, error } => {
                self.concrete_result_type_is_well_formed(success)
                    && error.as_ref().is_none_or(|error| {
                        matches!(
                            error.as_ref(),
                            Type::Nominal { definition, .. }
                                if !self.interfaces.contains_key(definition)
                        ) && self.concrete_result_type_is_well_formed(error)
                    })
            }
            Type::Array(value)
            | Type::FixedArray { element: value, .. }
            | Type::Own { value, .. }
            | Type::Option(value) => self.concrete_result_type_is_well_formed(value),
            Type::Tuple(values) => values
                .iter()
                .all(|value| self.concrete_result_type_is_well_formed(value)),
            Type::Function {
                parameters,
                return_type,
            } => {
                parameters
                    .iter()
                    .all(|value| self.concrete_result_type_is_well_formed(value))
                    && self.concrete_result_type_is_well_formed(return_type)
            }
            Type::Nominal { arguments, .. } => arguments
                .iter()
                .all(|value| self.concrete_result_type_is_well_formed(value)),
            _ => true,
        }
    }

    fn function_value(
        &mut self,
        id: DefinitionId,
        site: &SourceRange,
    ) -> Result<(Option<SpecializationId>, Type), VerificationFailure> {
        let function = self
            .functions
            .get(&id)
            .ok_or_else(|| VerificationFailure::Defect {
                evidence: Arc::from(format!(
                    "resolved function {:032x} absent from catalog; available: {}",
                    id.0,
                    self.functions
                        .values()
                        .map(|function| function.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            })?;
        if !function.type_parameters.is_empty()
            || function
                .parameters
                .iter()
                .any(|parameter| parameter.ownership != OwnershipSyntax::Value)
        {
            return creator(CreatorFailureKind::InvalidFunctionValue, site);
        }
        let type_ = Type::Function {
            parameters: function
                .parameters
                .iter()
                .map(|parameter| parameter.type_.clone())
                .collect::<Vec<_>>()
                .into(),
            return_type: Arc::new(function.return_type.clone()),
        };
        if !self.concrete_context {
            return Ok((None, type_));
        }
        let specialization_id =
            self.identity_catalog
                .specialization(id, &[])
                .map_err(|collision| VerificationFailure::Defect {
                    evidence: Arc::from(format!(
                        "specialization identity collision {:032x}",
                        collision.digest
                    )),
                })?;
        let specialization = SpecializationRecord {
            id: specialization_id,
            definition: id,
            type_arguments: Arc::from([]),
        };
        if let std::collections::btree_map::Entry::Vacant(entry) =
            self.specialization_demands.records.entry(specialization.id)
        {
            entry.insert(specialization.clone());
            self.specialization_demands
                .pending
                .insert(specialization.id);
        }
        Ok((Some(specialization.id), type_))
    }
}

fn syntax_integer_value(expression: &ExpressionSyntax) -> Option<i128> {
    match &expression.kind {
        ExpressionSyntaxKind::Integer(authored) => {
            parse_integer_literal(authored).ok().map(|v| v.0)
        }
        ExpressionSyntaxKind::Negate(value) => syntax_integer_value(value)?.checked_neg(),
        ExpressionSyntaxKind::Positive(value) => syntax_integer_value(value),
        _ => None,
    }
}

fn join_known_integers(states: &[BTreeMap<LocalId, i128>]) -> BTreeMap<LocalId, i128> {
    let Some(first) = states.first() else {
        return BTreeMap::new();
    };
    first
        .iter()
        .filter(|(local, value)| {
            states
                .iter()
                .skip(1)
                .all(|state| state.get(local) == Some(*value))
        })
        .map(|(local, value)| (*local, *value))
        .collect()
}

fn expression_integer_value(expression: &Expression) -> Option<i128> {
    match expression.kind {
        ExpressionKind::Literal(Literal::Integer { value, .. }) => Some(value),
        _ => None,
    }
}

pub(crate) fn root_place(expression: &Expression) -> Option<Place> {
    match &expression.kind {
        ExpressionKind::Read(place) => Some(place.clone()),
        ExpressionKind::Index { value, index } => {
            let mut place = root_place(value)?;
            let mut projections = place.projections.to_vec();
            projections.push(PlaceProjection::Index {
                index: index.clone(),
                type_: expression.type_.clone(),
            });
            place.projections = projections.into();
            Some(place)
        }
        _ => None,
    }
}

fn call_loan(expression: &Expression) -> Option<&Expression> {
    let ExpressionKind::Call { arguments, .. } = &expression.kind else {
        return None;
    };
    arguments
        .iter()
        .find(|argument| matches!(argument.access, AccessMode::Read | AccessMode::Mut))
}

fn collect_required_locals(expression: &Expression, output: &mut BTreeSet<LocalId>) {
    match &expression.kind {
        ExpressionKind::Read(place) => {
            output.insert(place.local);
        }
        ExpressionKind::Closure(closure) => {
            output.extend(closure.captures.iter().map(|(local, _)| *local));
        }
        _ => expression.visit_children(&mut |child| collect_required_locals(child, output)),
    }
}

fn bind_type(
    expected: &Type,
    actual: &Type,
    substitutions: &mut BTreeMap<crate::model::TypeParameterId, Type>,
    site: &SourceRange,
) -> Result<(), VerificationFailure> {
    match (expected, actual) {
        (Type::Parameter { id, .. }, actual) => {
            if let Some(previous) = substitutions.insert(*id, actual.clone())
                && previous != *actual
            {
                return creator(CreatorFailureKind::GenericArgumentConflict, site);
            }
            Ok(())
        }
        (Type::Array(expected), Type::Array(actual))
        | (
            Type::Array(expected),
            Type::FixedArray {
                element: actual, ..
            },
        )
        | (Type::Option(expected), Type::Option(actual)) => {
            bind_type(expected, actual, substitutions, site)
        }
        (
            Type::FixedArray {
                element: expected,
                length: expected_length,
            },
            Type::FixedArray {
                element: actual,
                length: actual_length,
            },
        ) => {
            match (expected_length, actual_length) {
                (ArrayLength::Value(expected), ArrayLength::Value(actual))
                    if expected == actual => {}
                (ArrayLength::Parameter { id, .. }, ArrayLength::Value(actual)) => {
                    if let Some(previous) = substitutions.insert(*id, Type::ConstU64(*actual))
                        && previous != Type::ConstU64(*actual)
                    {
                        return creator(CreatorFailureKind::GenericArgumentConflict, site);
                    }
                }
                _ => return creator(CreatorFailureKind::GenericArgumentConflict, site),
            }
            bind_type(expected, actual, substitutions, site)
        }
        (
            Type::Function {
                parameters: expected_parameters,
                return_type: expected_return,
            },
            Type::Function {
                parameters: actual_parameters,
                return_type: actual_return,
            },
        ) if expected_parameters.len() == actual_parameters.len() => {
            for (expected, actual) in expected_parameters.iter().zip(actual_parameters.iter()) {
                bind_type(expected, actual, substitutions, site)?;
            }
            bind_type(expected_return, actual_return, substitutions, site)
        }
        (
            Type::Own {
                pool: expected_pool,
                value: expected,
            },
            Type::Own {
                pool: actual_pool,
                value: actual,
            },
        ) => {
            match (expected_pool, actual_pool) {
                (PoolTerm::Concrete(expected), PoolTerm::Concrete(actual))
                    if expected == actual => {}
                (PoolTerm::Parameter { id, .. }, PoolTerm::Concrete(actual)) => {
                    if let Some(previous) = substitutions.insert(*id, Type::PoolArgument(*actual))
                        && previous != Type::PoolArgument(*actual)
                    {
                        return creator(CreatorFailureKind::GenericArgumentConflict, site);
                    }
                }
                _ => return creator(CreatorFailureKind::GenericArgumentConflict, site),
            }
            bind_type(expected, actual, substitutions, site)
        }
        (Type::Tuple(expected), Type::Tuple(actual)) if expected.len() == actual.len() => {
            for (expected, actual) in expected.iter().zip(actual.iter()) {
                bind_type(expected, actual, substitutions, site)?;
            }
            Ok(())
        }
        (
            Type::Nominal {
                definition: expected_definition,
                arguments: expected_arguments,
                ..
            },
            Type::Nominal {
                definition: actual_definition,
                arguments: actual_arguments,
                ..
            },
        ) if expected_definition == actual_definition
            && expected_arguments.len() == actual_arguments.len() =>
        {
            for (expected, actual) in expected_arguments.iter().zip(actual_arguments.iter()) {
                bind_type(expected, actual, substitutions, site)?;
            }
            Ok(())
        }
        (
            Type::Result {
                success: expected_success,
                error: expected_error,
            },
            Type::Result {
                success: actual_success,
                error: actual_error,
            },
        ) => {
            bind_type(expected_success, actual_success, substitutions, site)?;
            match (expected_error, actual_error) {
                (Some(expected), Some(actual)) => bind_type(expected, actual, substitutions, site),
                (None, _) => Ok(()),
                (Some(_), None) => creator(CreatorFailureKind::ArgumentTypeMismatch, site),
            }
        }
        _ if can_pass(actual, expected) => Ok(()),
        _ => creator(CreatorFailureKind::ArgumentTypeMismatch, site),
    }
}

fn expression_contains_propagation(expression: &Expression) -> bool {
    if matches!(expression.kind, ExpressionKind::Propagate(_)) {
        return true;
    }
    let mut found = false;
    expression.visit_children(&mut |child| found |= expression_contains_propagation(child));
    found
}

fn match_pattern_binding_signature(
    pattern: &HirMatchPattern,
) -> BTreeMap<LocalId, (Type, AccessMode)> {
    fn collect(pattern: &HirMatchPattern, bindings: &mut BTreeMap<LocalId, (Type, AccessMode)>) {
        match pattern {
            HirMatchPattern::Binding {
                local,
                type_,
                access,
                ..
            } => {
                bindings.insert(*local, (type_.clone(), *access));
            }
            HirMatchPattern::Variant { payload, .. }
            | HirMatchPattern::Struct {
                fields: payload, ..
            }
            | HirMatchPattern::Tuple(payload)
            | HirMatchPattern::FixedArray(payload)
            | HirMatchPattern::Or(payload) => {
                payload
                    .iter()
                    .for_each(|pattern| collect(pattern, bindings));
            }
            HirMatchPattern::Wildcard | HirMatchPattern::Literal(_) => {}
        }
    }

    let mut bindings = BTreeMap::new();
    collect(pattern, &mut bindings);
    bindings
}

fn pattern_non_custodial_locals(pattern: &HirMatchPattern) -> BTreeSet<LocalId> {
    match_pattern_binding_signature(pattern)
        .into_iter()
        .filter_map(|(local, (_, access))| {
            matches!(access, AccessMode::Read | AccessMode::Mut).then_some(local)
        })
        .collect()
}

fn artifact_pattern_move_keys(
    pattern: &HirMatchPattern,
    expected: &Type,
    root: &PlaceKey,
    catalog: &ArtifactCatalog<'_>,
) -> Result<BTreeSet<PlaceKey>, VerificationFailure> {
    let project = |projection: ProjectionKey| {
        let mut projections = root.projections.to_vec();
        projections.push(projection);
        PlaceKey {
            local: root.local,
            projections: projections.into(),
        }
    };
    match pattern {
        HirMatchPattern::Binding { access, .. } => Ok((*access == AccessMode::Move)
            .then(|| root.clone())
            .into_iter()
            .collect()),
        HirMatchPattern::Wildcard | HirMatchPattern::Literal(_) => Ok(BTreeSet::new()),
        HirMatchPattern::Tuple(patterns) => {
            let Type::Tuple(types) = expected else {
                return defect("lowered tuple pattern lost its tuple type");
            };
            let mut moved = BTreeSet::new();
            for (index, (pattern, type_)) in patterns.iter().zip(types.iter()).enumerate() {
                moved.extend(artifact_pattern_move_keys(
                    pattern,
                    type_,
                    &project(ProjectionKey::Index(Some(
                        i128::try_from(index).unwrap_or(i128::MAX),
                    ))),
                    catalog,
                )?);
            }
            Ok(moved)
        }
        HirMatchPattern::FixedArray(patterns) => {
            let element = match expected {
                Type::Array(element) | Type::FixedArray { element, .. } => element,
                _ => return defect("lowered array pattern lost its array type"),
            };
            let mut moved = BTreeSet::new();
            for (index, pattern) in patterns.iter().enumerate() {
                moved.extend(artifact_pattern_move_keys(
                    pattern,
                    element,
                    &project(ProjectionKey::Index(Some(
                        i128::try_from(index).unwrap_or(i128::MAX),
                    ))),
                    catalog,
                )?);
            }
            Ok(moved)
        }
        HirMatchPattern::Struct { definition, fields } => {
            let Type::Nominal { arguments, .. } = expected else {
                return defect("lowered struct pattern lost its nominal type");
            };
            let struct_ =
                catalog
                    .structs
                    .get(definition)
                    .ok_or_else(|| VerificationFailure::Defect {
                        evidence: Arc::from("lowered struct pattern has no nominal definition"),
                    })?;
            let selected = artifact_struct_fields(struct_, arguments)?;
            let substitutions = struct_
                .type_parameters
                .iter()
                .copied()
                .zip(arguments.iter().cloned())
                .collect::<BTreeMap<_, _>>();
            let mut moved = BTreeSet::new();
            for (pattern, field) in fields.iter().zip(selected.iter()) {
                moved.extend(artifact_pattern_move_keys(
                    pattern,
                    &substitute(&field.type_, &substitutions),
                    &project(ProjectionKey::Field(
                        *definition,
                        Arc::from(field.name.as_str()),
                    )),
                    catalog,
                )?);
            }
            Ok(moved)
        }
        HirMatchPattern::Variant { id, payload } => {
            let Type::Nominal { arguments, .. } = expected else {
                return defect("lowered variant pattern lost its nominal type");
            };
            let variant = catalog
                .variants
                .get(id)
                .ok_or_else(|| VerificationFailure::Defect {
                    evidence: Arc::from("lowered variant pattern has no variant definition"),
                })?;
            let substitutions = variant
                .type_parameters
                .iter()
                .copied()
                .zip(arguments.iter().cloned())
                .collect::<BTreeMap<_, _>>();
            let mut moved = BTreeSet::new();
            for (index, (pattern, parameter)) in
                payload.iter().zip(variant.parameters.iter()).enumerate()
            {
                moved.extend(artifact_pattern_move_keys(
                    pattern,
                    &substitute(&parameter.type_, &substitutions),
                    &project(ProjectionKey::Index(Some(
                        i128::try_from(index).unwrap_or(i128::MAX),
                    ))),
                    catalog,
                )?);
            }
            Ok(moved)
        }
        HirMatchPattern::Or(alternatives) => {
            let mut alternatives = alternatives.iter();
            let Some(first) = alternatives.next() else {
                return defect("lowered or-pattern has no alternatives");
            };
            let mut guaranteed = artifact_pattern_move_keys(first, expected, root, catalog)?;
            for alternative in alternatives {
                let moved = artifact_pattern_move_keys(alternative, expected, root, catalog)?;
                guaranteed.retain(|place| moved.contains(place));
            }
            Ok(guaranteed)
        }
    }
}

fn verify_match_pattern_artifact(
    pattern: &HirMatchPattern,
    expected: &Type,
    locals: &mut BTreeMap<LocalId, ArtifactLocalType>,
    catalog: &ArtifactCatalog<'_>,
) -> Result<(), VerificationFailure> {
    match pattern {
        HirMatchPattern::Wildcard => Ok(()),
        HirMatchPattern::Literal(literal) => {
            if can_initialize(&literal_type(literal), expected) {
                Ok(())
            } else {
                defect("lowered match literal disagrees with matched type")
            }
        }
        HirMatchPattern::Binding {
            local,
            type_id,
            type_,
            ..
        } => {
            if !can_initialize(type_, expected) {
                return defect("lowered match binding disagrees with matched type");
            }
            if !catalog.identities.type_matches(*type_id, type_)
                || locals
                    .insert(
                        *local,
                        ArtifactLocalType {
                            type_id: *type_id,
                            type_: type_.clone(),
                        },
                    )
                    .is_some()
            {
                return defect("lowered match binding repeats a LocalId");
            }
            Ok(())
        }
        HirMatchPattern::Variant { id, payload } => {
            let Type::Nominal {
                definition,
                arguments,
                ..
            } = expected
            else {
                return defect("lowered variant pattern targets a non-nominal value");
            };
            if *definition != id.owner {
                return defect("lowered variant pattern has the wrong owner");
            }
            let Some(variant) = catalog.variants.get(id) else {
                return defect("lowered variant pattern references an unknown variant");
            };
            if variant.type_parameters.len() != arguments.len()
                || variant.parameters.len() != payload.len()
            {
                return defect("lowered variant pattern has the wrong payload shape");
            }
            let substitutions = variant
                .type_parameters
                .iter()
                .copied()
                .zip(arguments.iter().cloned())
                .collect::<BTreeMap<_, _>>();
            for (pattern, parameter) in payload.iter().zip(&variant.parameters) {
                verify_match_pattern_artifact(
                    pattern,
                    &substitute(&parameter.type_, &substitutions),
                    locals,
                    catalog,
                )?;
            }
            Ok(())
        }
        HirMatchPattern::Struct { definition, fields } => {
            let Type::Nominal {
                definition: expected_definition,
                arguments,
                ..
            } = expected
            else {
                return defect("lowered struct pattern targets a non-nominal value");
            };
            if definition != expected_definition {
                return defect("lowered struct pattern has the wrong nominal type");
            }
            let Some(struct_) = catalog.structs.get(definition) else {
                return defect("lowered struct pattern references an unknown struct");
            };
            let selected_fields = artifact_struct_fields(struct_, arguments)?;
            if struct_.type_parameters.len() != arguments.len()
                || selected_fields.len() != fields.len()
            {
                return defect("lowered struct pattern has the wrong field shape");
            }
            let substitutions = struct_
                .type_parameters
                .iter()
                .copied()
                .zip(arguments.iter().cloned())
                .collect::<BTreeMap<_, _>>();
            for (pattern, field) in fields.iter().zip(selected_fields.iter()) {
                verify_match_pattern_artifact(
                    pattern,
                    &substitute(&field.type_, &substitutions),
                    locals,
                    catalog,
                )?;
            }
            Ok(())
        }
        HirMatchPattern::Tuple(patterns) => {
            let Type::Tuple(types) = expected else {
                return defect("lowered tuple pattern targets a non-tuple value");
            };
            if patterns.len() != types.len() {
                return defect("lowered tuple pattern has the wrong arity");
            }
            for (pattern, expected) in patterns.iter().zip(types.iter()) {
                verify_match_pattern_artifact(pattern, expected, locals, catalog)?;
            }
            Ok(())
        }
        HirMatchPattern::FixedArray(patterns) => {
            let (element, length) = match expected {
                Type::Array(element) => (element.as_ref(), patterns.len() as u64),
                Type::FixedArray { element, length } => {
                    (element.as_ref(), length.value().unwrap_or(u64::MAX))
                }
                _ => return defect("lowered fixed-array pattern targets a non-array value"),
            };
            if patterns.len() as u64 != length {
                return defect("lowered fixed-array pattern has the wrong length");
            }
            for pattern in patterns.iter() {
                verify_match_pattern_artifact(pattern, element, locals, catalog)?;
            }
            Ok(())
        }
        HirMatchPattern::Or(alternatives) => {
            let Some(first) = alternatives.first() else {
                return defect("lowered or-pattern has no alternatives");
            };
            let base = locals.clone();
            let mut first_locals = base.clone();
            verify_match_pattern_artifact(first, expected, &mut first_locals, catalog)?;
            let first_bindings = match_pattern_binding_signature(first);
            for alternative in alternatives.iter().skip(1) {
                let mut alternative_locals = base.clone();
                verify_match_pattern_artifact(
                    alternative,
                    expected,
                    &mut alternative_locals,
                    catalog,
                )?;
                if match_pattern_binding_signature(alternative) != first_bindings {
                    return defect("lowered or-pattern alternatives bind different locals");
                }
            }
            *locals = first_locals;
            Ok(())
        }
    }
}

fn hir_match_exhaustive_for_type(
    type_: &Type,
    cases: &[HirMatchCase],
    catalog: &ArtifactCatalog<'_>,
) -> bool {
    if cases.iter().any(|case| {
        case.guard.is_none()
            && case
                .pattern
                .as_ref()
                .is_none_or(|pattern| artifact_pattern_irrefutable(pattern, type_, catalog))
    }) {
        return true;
    }
    match type_ {
        Type::Bool => crate::control_flow::verified_match_exhaustive(cases),
        Type::Nominal { definition, .. } => {
            let declared = catalog
                .variants
                .keys()
                .filter(|variant| variant.owner == *definition)
                .copied()
                .collect::<BTreeSet<_>>();
            !declared.is_empty()
                && declared.iter().all(|id| {
                    cases.iter().any(|case| {
                        case.guard.is_none()
                            && case.pattern.as_ref().is_some_and(|pattern| {
                                artifact_pattern_covers_variant(pattern, type_, *id, catalog)
                            })
                    })
                })
        }
        _ => false,
    }
}

fn artifact_pattern_irrefutable(
    pattern: &HirMatchPattern,
    type_: &Type,
    catalog: &ArtifactCatalog<'_>,
) -> bool {
    match pattern {
        HirMatchPattern::Wildcard | HirMatchPattern::Binding { .. } => true,
        HirMatchPattern::Literal(_) | HirMatchPattern::Variant { .. } => false,
        HirMatchPattern::Or(alternatives) => alternatives
            .iter()
            .any(|pattern| artifact_pattern_irrefutable(pattern, type_, catalog)),
        HirMatchPattern::Tuple(patterns) => {
            let Type::Tuple(types) = type_ else {
                return false;
            };
            patterns.len() == types.len()
                && patterns
                    .iter()
                    .zip(types.iter())
                    .all(|(pattern, type_)| artifact_pattern_irrefutable(pattern, type_, catalog))
        }
        HirMatchPattern::FixedArray(patterns) => {
            let (element, length) = match type_ {
                Type::Array(element) => (element.as_ref(), patterns.len() as u64),
                Type::FixedArray { element, length } => {
                    (element.as_ref(), length.value().unwrap_or(u64::MAX))
                }
                _ => return false,
            };
            patterns.len() as u64 == length
                && patterns
                    .iter()
                    .all(|pattern| artifact_pattern_irrefutable(pattern, element, catalog))
        }
        HirMatchPattern::Struct { definition, fields } => {
            let Type::Nominal {
                definition: expected,
                arguments,
                ..
            } = type_
            else {
                return false;
            };
            let Some(struct_) = catalog.structs.get(definition) else {
                return false;
            };
            let Ok(selected_fields) = artifact_struct_fields(struct_, arguments) else {
                return false;
            };
            if definition != expected
                || fields.len() != selected_fields.len()
                || arguments.len() != struct_.type_parameters.len()
            {
                return false;
            }
            let substitutions = struct_
                .type_parameters
                .iter()
                .copied()
                .zip(arguments.iter().cloned())
                .collect::<BTreeMap<_, _>>();
            fields
                .iter()
                .zip(selected_fields.iter())
                .all(|(pattern, field)| {
                    artifact_pattern_irrefutable(
                        pattern,
                        &substitute(&field.type_, &substitutions),
                        catalog,
                    )
                })
        }
    }
}

fn artifact_pattern_covers_variant(
    pattern: &HirMatchPattern,
    enum_type: &Type,
    expected_id: VariantId,
    catalog: &ArtifactCatalog<'_>,
) -> bool {
    if let HirMatchPattern::Or(alternatives) = pattern {
        return alternatives.iter().any(|pattern| {
            artifact_pattern_covers_variant(pattern, enum_type, expected_id, catalog)
        });
    }
    let HirMatchPattern::Variant { id, payload } = pattern else {
        return false;
    };
    if *id != expected_id {
        return false;
    }
    let Type::Nominal { arguments, .. } = enum_type else {
        return false;
    };
    let Some(variant) = catalog.variants.get(id) else {
        return false;
    };
    if arguments.len() != variant.type_parameters.len() || payload.len() != variant.parameters.len()
    {
        return false;
    }
    let substitutions = variant
        .type_parameters
        .iter()
        .copied()
        .zip(arguments.iter().cloned())
        .collect::<BTreeMap<_, _>>();
    payload
        .iter()
        .zip(&variant.parameters)
        .all(|(pattern, parameter)| {
            artifact_pattern_irrefutable(
                pattern,
                &substitute(&parameter.type_, &substitutions),
                catalog,
            )
        })
}

fn check_callable(
    parameters: &[ResolvedParameter],
    arguments: &[Expression],
    labels: &[Option<String>],
    label_mode: LabelMode,
    site: &SourceRange,
) -> Result<Arc<[u16]>, VerificationFailure> {
    let signature = CallableSignature {
        parameters: parameters
            .iter()
            .map(|parameter| CallableParameter {
                name: &parameter.name,
                type_: &parameter.type_,
            })
            .collect(),
        label_mode,
    };
    check_callable_signature(&signature, arguments, labels, site)
}

fn check_callable_signature(
    signature: &CallableSignature<'_>,
    arguments: &[Expression],
    labels: &[Option<String>],
    site: &SourceRange,
) -> Result<Arc<[u16]>, VerificationFailure> {
    signature
        .check(
            &arguments
                .iter()
                .map(|argument| argument.type_.clone())
                .collect::<Vec<_>>(),
            labels,
        )
        .map_err(|error| creator_value(call_error_kind(error), site))?
        .source_to_parameter
        .into_iter()
        .map(|index| {
            u16::try_from(index).map_err(|_| VerificationFailure::Defect {
                evidence: Arc::from("callable parameter index exceeds the HIR representation"),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Into::into)
}

const fn call_error_kind(error: CallError) -> CreatorFailureKind {
    match error {
        CallError::Count => CreatorFailureKind::ArgumentCount,
        CallError::Label => CreatorFailureKind::ArgumentLabelMismatch,
        CallError::Type => CreatorFailureKind::ArgumentTypeMismatch,
    }
}

fn substitute(type_: &Type, substitutions: &BTreeMap<crate::model::TypeParameterId, Type>) -> Type {
    match type_ {
        Type::Parameter { id, .. } => substitutions
            .get(id)
            .cloned()
            .unwrap_or_else(|| type_.clone()),
        Type::Array(element) => Type::Array(Arc::new(substitute(element, substitutions))),
        Type::FixedArray { element, length } => Type::FixedArray {
            element: Arc::new(substitute(element, substitutions)),
            length: match length {
                ArrayLength::Value(value) => ArrayLength::Value(*value),
                ArrayLength::Parameter { id, .. } => substitutions
                    .get(id)
                    .and_then(|value| match value {
                        Type::ConstU64(value) => Some(ArrayLength::Value(*value)),
                        _ => None,
                    })
                    .unwrap_or_else(|| length.clone()),
            },
        },
        Type::Tuple(members) => Type::Tuple(
            members
                .iter()
                .map(|member| substitute(member, substitutions))
                .collect(),
        ),
        Type::Result { success, error } => Type::Result {
            success: Arc::new(substitute(success, substitutions)),
            error: error
                .as_ref()
                .map(|error| Arc::new(substitute(error, substitutions))),
        },
        Type::Option(value) => Type::Option(Arc::new(substitute(value, substitutions))),
        Type::Function {
            parameters,
            return_type,
        } => Type::Function {
            parameters: parameters
                .iter()
                .map(|parameter| substitute(parameter, substitutions))
                .collect(),
            return_type: Arc::new(substitute(return_type, substitutions)),
        },
        Type::Own { pool, value } => Type::Own {
            pool: match pool {
                PoolTerm::Concrete(pool) => PoolTerm::Concrete(*pool),
                PoolTerm::Parameter { id, .. } => substitutions
                    .get(id)
                    .and_then(|value| match value {
                        Type::PoolArgument(pool) => Some(PoolTerm::Concrete(*pool)),
                        _ => None,
                    })
                    .unwrap_or_else(|| pool.clone()),
            },
            value: Arc::new(substitute(value, substitutions)),
        },
        Type::Nominal {
            definition,
            display,
            arguments,
        } => Type::Nominal {
            definition: *definition,
            display: display.clone(),
            arguments: arguments
                .iter()
                .map(|argument| substitute(argument, substitutions))
                .collect(),
        },
        _ => type_.clone(),
    }
}

fn builtin_variant_type(
    variant: BuiltinVariant,
    arguments: &[Expression],
    site: &SourceRange,
) -> Result<Type, VerificationFailure> {
    Ok(match variant {
        BuiltinVariant::ResultOk => Type::Result {
            success: Arc::new(
                arguments
                    .first()
                    .ok_or_else(|| creator_value(CreatorFailureKind::ArgumentCount, site))?
                    .type_
                    .clone(),
            ),
            error: Some(Arc::new(Type::Infer)),
        },
        BuiltinVariant::ResultErr => Type::Result {
            success: Arc::new(Type::Infer),
            error: Some(Arc::new(
                arguments
                    .first()
                    .ok_or_else(|| creator_value(CreatorFailureKind::ArgumentCount, site))?
                    .type_
                    .clone(),
            )),
        },
        BuiltinVariant::OptionSome => Type::Option(Arc::new(
            arguments
                .first()
                .ok_or_else(|| creator_value(CreatorFailureKind::ArgumentCount, site))?
                .type_
                .clone(),
        )),
        BuiltinVariant::OptionNone => Type::Option(Arc::new(Type::Infer)),
    })
}

fn binary_type(operator: BinaryOperator, left: &Type, right: &Type) -> Option<Type> {
    if matches!(
        operator,
        BinaryOperator::Range | BinaryOperator::RangeInclusive
    ) {
        return (left == right && matches!(left, Type::Integer(_)))
            .then(|| Type::Array(Arc::new(left.clone())));
    }
    if matches!(operator, BinaryOperator::And | BinaryOperator::Or) {
        return (*left == Type::Bool && *right == Type::Bool).then_some(Type::Bool);
    }
    if !can_unify(left, right) {
        return None;
    }
    if matches!(
        operator,
        BinaryOperator::BitAnd
            | BinaryOperator::BitOr
            | BinaryOperator::BitXor
            | BinaryOperator::ShiftLeft
            | BinaryOperator::ShiftRight
    ) {
        return matches!(left, Type::Integer(_)).then(|| left.clone());
    }
    if matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
        return comparison_shape_supported(left, false).then_some(Type::Bool);
    }
    if matches!(
        operator,
        BinaryOperator::Less
            | BinaryOperator::LessEqual
            | BinaryOperator::Greater
            | BinaryOperator::GreaterEqual
    ) {
        return comparison_shape_supported(left, true).then_some(Type::Bool);
    }
    left.is_numeric().then(|| left.clone())
}

fn comparison_shape_supported(type_: &Type, ordered: bool) -> bool {
    match type_ {
        Type::Unit | Type::Bool => !ordered,
        Type::Integer(_) | Type::Float(_) | Type::Text | Type::Scalar | Type::Bytes => true,
        Type::Array(value) | Type::FixedArray { element: value, .. } | Type::Option(value) => {
            comparison_shape_supported(value, ordered)
        }
        Type::Tuple(values) => values
            .iter()
            .all(|value| comparison_shape_supported(value, ordered)),
        Type::Result { success, error } => {
            comparison_shape_supported(success, ordered)
                && error
                    .as_ref()
                    .is_none_or(|error| comparison_shape_supported(error, ordered))
        }
        // Nominal capability is validated against its declaration while lowering.
        Type::Nominal { .. } => true,
        Type::Function { .. }
        | Type::Own { .. }
        | Type::Any { .. }
        | Type::Builtin(_)
        | Type::ConstU64(_)
        | Type::PoolArgument(_)
        | Type::Parameter { .. }
        | Type::Infer => false,
    }
}

impl From<BinaryOperatorSyntax> for BinaryOperator {
    fn from(value: BinaryOperatorSyntax) -> Self {
        match value {
            BinaryOperatorSyntax::Range => Self::Range,
            BinaryOperatorSyntax::RangeInclusive => Self::RangeInclusive,
            BinaryOperatorSyntax::Add => Self::Add,
            BinaryOperatorSyntax::Subtract => Self::Subtract,
            BinaryOperatorSyntax::Multiply => Self::Multiply,
            BinaryOperatorSyntax::Divide => Self::Divide,
            BinaryOperatorSyntax::Remainder => Self::Remainder,
            BinaryOperatorSyntax::BitAnd => Self::BitAnd,
            BinaryOperatorSyntax::BitOr => Self::BitOr,
            BinaryOperatorSyntax::BitXor => Self::BitXor,
            BinaryOperatorSyntax::ShiftLeft => Self::ShiftLeft,
            BinaryOperatorSyntax::ShiftRight => Self::ShiftRight,
            BinaryOperatorSyntax::And => Self::And,
            BinaryOperatorSyntax::Or => Self::Or,
            BinaryOperatorSyntax::Equal => Self::Equal,
            BinaryOperatorSyntax::NotEqual => Self::NotEqual,
            BinaryOperatorSyntax::Less => Self::Less,
            BinaryOperatorSyntax::LessEqual => Self::LessEqual,
            BinaryOperatorSyntax::Greater => Self::Greater,
            BinaryOperatorSyntax::GreaterEqual => Self::GreaterEqual,
        }
    }
}

fn parse_integer_literal(source: &str) -> Result<(i128, IntegerType), ()> {
    let (magnitude, kind) = parse_integer_parts(source)?;
    let value = i128::try_from(magnitude).map_err(|_| ())?;
    kind.fits(value).then_some((value, kind)).ok_or(())
}

fn integer_literal_has_suffix(source: &str) -> bool {
    ["u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64"]
        .iter()
        .any(|suffix| source.ends_with(suffix))
}

fn parse_negated_integer_literal(source: &str) -> Result<(i128, IntegerType), ()> {
    let (magnitude, kind) = parse_integer_parts(source)?;
    if !kind.is_signed() {
        return Err(());
    }
    let magnitude = i128::try_from(magnitude).map_err(|_| ())?;
    let value = magnitude.checked_neg().ok_or(())?;
    kind.fits(value).then_some((value, kind)).ok_or(())
}

fn parse_integer_parts(source: &str) -> Result<(u128, IntegerType), ()> {
    let kinds = [
        ("u8", IntegerType::U8),
        ("u16", IntegerType::U16),
        ("u32", IntegerType::U32),
        ("u64", IntegerType::U64),
        ("i8", IntegerType::I8),
        ("i16", IntegerType::I16),
        ("i32", IntegerType::I32),
        ("i64", IntegerType::I64),
    ];
    let (digits, kind) = kinds
        .iter()
        .find_map(|(suffix, kind)| source.strip_suffix(suffix).map(|digits| (digits, *kind)))
        .unwrap_or((source, IntegerType::I64));
    let digits = digits.replace('_', "");
    let (radix, digits) = if let Some(digits) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        (16, digits)
    } else if let Some(digits) = digits
        .strip_prefix("0b")
        .or_else(|| digits.strip_prefix("0B"))
    {
        (2, digits)
    } else if let Some(digits) = digits
        .strip_prefix("0o")
        .or_else(|| digits.strip_prefix("0O"))
    {
        (8, digits)
    } else {
        (10, digits.as_str())
    };
    let value = u128::from_str_radix(digits, radix).map_err(|_| ())?;
    Ok((value, kind))
}

fn parse_float_literal(source: &str) -> Result<(f64, FloatType), ()> {
    let kinds = [
        ("f16", FloatType::F16),
        ("f32", FloatType::F32),
        ("f64", FloatType::F64),
    ];
    let (digits, kind) = kinds
        .iter()
        .find_map(|(suffix, kind)| source.strip_suffix(suffix).map(|digits| (digits, *kind)))
        .unwrap_or((source, FloatType::F64));
    Ok((
        digits.replace('_', "").parse::<f64>().map_err(|_| ())?,
        kind,
    ))
}

fn encode_float(kind: FloatType, value: f64) -> u64 {
    if value.is_nan() {
        return match kind {
            FloatType::F16 => 0x7e00,
            FloatType::F32 => 0x7fc0_0000,
            FloatType::F64 => 0x7ff8_0000_0000_0000,
        };
    }
    match kind {
        FloatType::F16 => u64::from(half::f16::from_f64(value).to_bits()),
        FloatType::F32 => u64::from((value as f32).to_bits()),
        FloatType::F64 => value.to_bits(),
    }
}

fn creator<T>(kind: CreatorFailureKind, site: &SourceRange) -> Result<T, VerificationFailure> {
    Err(creator_value(kind, site))
}
fn creator_value(kind: CreatorFailureKind, site: &SourceRange) -> VerificationFailure {
    VerificationFailure::Creator {
        kind,
        site: site.clone(),
    }
}
fn custody_value(
    kind: CreatorFailureKind,
    site: &SourceRange,
    subject: Arc<str>,
    state: CustodyDiagnosticState,
    subject_identity: Option<DefinitionId>,
    owner_identity: Option<DefinitionId>,
    related: Option<SourceRange>,
) -> VerificationFailure {
    VerificationFailure::Custody {
        kind,
        site: site.clone(),
        subject,
        state,
        identities: Box::new(CustodyIdentities {
            subject: subject_identity,
            owner: owner_identity,
        }),
        related,
    }
}
fn defect<T>(evidence: &'static str) -> Result<T, VerificationFailure> {
    Err(VerificationFailure::Defect {
        evidence: Arc::from(evidence),
    })
}

trait ByteSink {
    fn push(&mut self, byte: u8);
    fn extend_from_slice(&mut self, bytes: &[u8]);
}

impl ByteSink for Vec<u8> {
    fn push(&mut self, byte: u8) {
        Vec::push(self, byte);
    }

    fn extend_from_slice(&mut self, bytes: &[u8]) {
        Vec::extend_from_slice(self, bytes);
    }
}

impl ByteSink for Xxh3 {
    fn push(&mut self, byte: u8) {
        self.update(&[byte]);
    }

    fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.update(bytes);
    }
}

fn append_part(bytes: &mut impl ByteSink, part: &[u8]) {
    bytes.extend_from_slice(&u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes());
    bytes.extend_from_slice(part);
}

fn append_function(bytes: &mut impl ByteSink, function: &HirFunction) {
    bytes.push(function.modifier.canonical_tag());
    bytes.extend_from_slice(
        &u64::try_from(function.parameters.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for (((local, type_, access), type_id), definition) in function
        .parameters
        .iter()
        .zip(function.parameter_type_ids.iter())
        .zip(function.parameter_definitions.iter())
    {
        bytes.extend_from_slice(&local.0.to_be_bytes());
        bytes.extend_from_slice(&type_id.0.to_be_bytes());
        bytes.extend_from_slice(&definition.0.to_be_bytes());
        append_part(bytes, &type_.canonical_key());
        bytes.push(access.canonical_tag());
    }
    append_part(bytes, &function.return_type.canonical_key());
    append_statements(bytes, &function.body);
}

fn append_statements(bytes: &mut impl ByteSink, statements: &[Statement]) {
    bytes.extend_from_slice(
        &u64::try_from(statements.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for statement in statements {
        match statement {
            Statement::Return { value, source } => {
                bytes.push(1);
                append_range(bytes, source);
                if let Some(value) = value {
                    bytes.push(1);
                    append_expression(bytes, value);
                } else {
                    bytes.push(0);
                }
            }
            Statement::Panic { value, source } => {
                bytes.push(2);
                append_range(bytes, source);
                append_expression(bytes, value);
            }
            Statement::Assert { condition, source } => {
                bytes.push(9);
                append_range(bytes, source);
                append_expression(bytes, condition);
            }
            Statement::Expect { condition, source } => {
                bytes.push(8);
                append_range(bytes, source);
                append_expression(bytes, condition);
            }
            Statement::Initialize {
                place,
                value,
                source,
            } => {
                bytes.push(3);
                append_place(bytes, place);
                append_range(bytes, source);
                append_expression(bytes, value);
            }
            Statement::Assign {
                place,
                value,
                source,
            } => {
                bytes.push(7);
                append_place(bytes, place);
                append_range(bytes, source);
                append_expression(bytes, value);
            }
            Statement::Evaluate(value) => {
                bytes.push(4);
                append_expression(bytes, value);
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                source,
            } => {
                bytes.push(5);
                append_range(bytes, source);
                append_expression(bytes, condition);
                append_statements(bytes, then_branch);
                append_statements(bytes, else_branch);
            }
            Statement::IfPattern {
                value,
                pattern,
                then_branch,
                else_branch,
                source,
            } => {
                bytes.push(17);
                append_range(bytes, source);
                append_expression(bytes, value);
                append_match_pattern(bytes, pattern);
                append_statements(bytes, then_branch);
                append_statements(bytes, else_branch);
            }
            Statement::For {
                pattern,
                iterable,
                body,
                source,
            } => {
                bytes.push(10);
                append_match_pattern(bytes, pattern);
                append_range(bytes, source);
                append_expression(bytes, iterable);
                append_statements(bytes, body);
            }
            Statement::While {
                condition,
                body,
                max_iterations,
                source,
            } => {
                bytes.push(12);
                append_range(bytes, source);
                bytes.extend_from_slice(&max_iterations.to_be_bytes());
                append_expression(bytes, condition);
                append_statements(bytes, body);
            }
            Statement::Break(source) => {
                bytes.push(13);
                append_range(bytes, source);
            }
            Statement::Continue(source) => {
                bytes.push(14);
                append_range(bytes, source);
            }
            Statement::Match {
                value,
                cases,
                source,
            } => {
                bytes.push(11);
                append_range(bytes, source);
                append_expression(bytes, value);
                bytes.extend_from_slice(
                    &u64::try_from(cases.len()).unwrap_or(u64::MAX).to_be_bytes(),
                );
                for case in cases.iter() {
                    append_range(bytes, &case.source);
                    if let Some(pattern) = &case.pattern {
                        bytes.push(1);
                        append_match_pattern(bytes, pattern);
                    } else {
                        bytes.push(0);
                    }
                    if let Some(guard) = &case.guard {
                        bytes.push(1);
                        append_expression(bytes, guard);
                    } else {
                        bytes.push(0);
                    }
                    append_statements(bytes, &case.body);
                }
            }
            Statement::Defer { action, source } => {
                bytes.push(15);
                append_range(bytes, source);
                append_expression(bytes, action.expression());
                bytes.extend_from_slice(
                    &u64::try_from(action.captures.len())
                        .unwrap_or(u64::MAX)
                        .to_be_bytes(),
                );
                for capture in action.captures.iter() {
                    bytes.extend_from_slice(&capture.ordinal.0.to_be_bytes());
                    append_expression(bytes, &capture.expression);
                }
            }
            Statement::WithPool {
                binding,
                scope,
                body,
                source,
            } => {
                bytes.push(16);
                append_place(bytes, binding);
                append_range(bytes, source);
                append_expression(bytes, scope);
                append_statements(bytes, body);
            }
            Statement::Pass(source) => {
                bytes.push(6);
                append_range(bytes, source);
            }
        }
    }
}

fn append_place(bytes: &mut impl ByteSink, place: &Place) {
    bytes.extend_from_slice(&place.local.0.to_be_bytes());
    bytes.extend_from_slice(&(place.projections.len() as u32).to_be_bytes());
    for projection in place.projections.iter() {
        match projection {
            PlaceProjection::Field {
                definition,
                name,
                type_,
                mutable,
            } => {
                bytes.push(0);
                bytes.extend_from_slice(&definition.0.to_be_bytes());
                append_part(bytes, name.as_bytes());
                append_part(bytes, &type_.canonical_key());
                bytes.push(u8::from(*mutable));
            }
            PlaceProjection::Index { index, type_ } => {
                bytes.push(1);
                append_part(bytes, &type_.canonical_key());
                append_expression(bytes, index);
            }
        }
    }
}

fn append_expression(bytes: &mut impl ByteSink, expression: &Expression) {
    append_range(bytes, &expression.source);
    bytes.extend_from_slice(&expression.type_id.0.to_be_bytes());
    if let Some(type_) = &expression.coerced_from {
        bytes.push(1);
        append_part(bytes, &type_.canonical_key());
    } else {
        bytes.push(0);
    }
    bytes.push(match expression.access {
        AccessMode::Copy => 0,
        AccessMode::Read => 1,
        AccessMode::Mut => 2,
        AccessMode::Move => 3,
    });
    append_part(bytes, &expression.type_.canonical_key());
    match &expression.kind {
        ExpressionKind::Literal(literal) => {
            bytes.push(0);
            append_literal(bytes, literal);
        }
        ExpressionKind::Read(place) => {
            bytes.push(1);
            append_place(bytes, place);
        }
        ExpressionKind::Constant(id) => {
            bytes.push(2);
            bytes.extend_from_slice(&id.0.to_be_bytes());
        }
        ExpressionKind::FunctionValue {
            definition,
            specialization,
        } => {
            bytes.push(17);
            bytes.extend_from_slice(&definition.0.to_be_bytes());
            if let Some(specialization) = specialization {
                bytes.push(1);
                bytes.extend_from_slice(&specialization.0.to_be_bytes());
            } else {
                bytes.push(0);
            }
        }
        ExpressionKind::Closure(closure) => {
            bytes.push(18);
            bytes.extend_from_slice(&closure.id.0.to_be_bytes());
            bytes.extend_from_slice(
                &u64::try_from(closure.parameters.len())
                    .unwrap_or(u64::MAX)
                    .to_be_bytes(),
            );
            for ((local, type_), type_id) in closure
                .parameters
                .iter()
                .zip(closure.parameter_type_ids.iter())
            {
                bytes.extend_from_slice(&local.0.to_be_bytes());
                bytes.extend_from_slice(&type_id.0.to_be_bytes());
                append_part(bytes, &type_.canonical_key());
            }
            for ((local, type_), type_id) in
                closure.captures.iter().zip(closure.capture_type_ids.iter())
            {
                bytes.extend_from_slice(&local.0.to_be_bytes());
                bytes.extend_from_slice(&type_id.0.to_be_bytes());
                append_part(bytes, &type_.canonical_key());
            }
            append_part(bytes, &closure.return_type.canonical_key());
            append_expression(bytes, &closure.body);
        }
        ExpressionKind::CleanupCapture(ordinal) => {
            bytes.push(19);
            bytes.extend_from_slice(&ordinal.0.to_be_bytes());
        }
        ExpressionKind::Call { target, arguments } => {
            bytes.push(3);
            match target {
                CallTarget::Callable { value } => {
                    bytes.push(7);
                    append_expression(bytes, value);
                }
                CallTarget::TemplateFunction {
                    definition,
                    argument_order,
                    argument_parameters,
                } => {
                    bytes.push(0);
                    bytes.extend_from_slice(&definition.0.to_be_bytes());
                    append_argument_order(bytes, argument_order);
                    for parameter in argument_parameters.iter() {
                        bytes.extend_from_slice(&parameter.0.to_be_bytes());
                    }
                }
                CallTarget::Function {
                    definition,
                    specialization,
                    argument_order,
                    argument_parameters,
                } => {
                    bytes.push(1);
                    bytes.extend_from_slice(&definition.0.to_be_bytes());
                    bytes.extend_from_slice(&specialization.0.to_be_bytes());
                    append_argument_order(bytes, argument_order);
                    for parameter in argument_parameters.iter() {
                        bytes.extend_from_slice(&parameter.0.to_be_bytes());
                    }
                }
                CallTarget::Interface {
                    interface,
                    alternatives,
                    argument_order,
                    argument_parameters,
                } => {
                    bytes.push(8);
                    bytes.extend_from_slice(&interface.0.to_be_bytes());
                    for (nominal, definition, specialization) in alternatives.iter() {
                        bytes.extend_from_slice(&nominal.0.to_be_bytes());
                        bytes.extend_from_slice(&definition.0.to_be_bytes());
                        bytes.extend_from_slice(&specialization.0.to_be_bytes());
                    }
                    append_argument_order(bytes, argument_order);
                    for parameter in argument_parameters.iter() {
                        bytes.extend_from_slice(&parameter.0.to_be_bytes());
                    }
                }
                CallTarget::Build { primitive, labels } => {
                    bytes.push(2);
                    bytes.extend_from_slice(&primitive.identity.to_be_bytes());
                    bytes.push(primitive.kind.canonical_tag());
                    if let BuildKind::Node {
                        definition,
                        type_identity,
                    } = primitive.kind
                    {
                        bytes.extend_from_slice(&definition.0.to_be_bytes());
                        bytes.extend_from_slice(&type_identity.0.to_be_bytes());
                    }
                    for label in labels.iter() {
                        append_part(bytes, label.as_bytes());
                    }
                }
                CallTarget::BuiltinVariant(variant) => {
                    bytes.push(3);
                    bytes.push(variant.canonical_tag());
                }
                CallTarget::UserVariant {
                    id,
                    argument_order,
                    argument_parameters,
                    ..
                } => {
                    bytes.push(4);
                    bytes.extend_from_slice(&id.owner.0.to_be_bytes());
                    bytes.extend_from_slice(&id.variant.to_be_bytes());
                    append_argument_order(bytes, argument_order);
                    for parameter in argument_parameters.iter() {
                        bytes.extend_from_slice(&parameter.0.to_be_bytes());
                    }
                }
                CallTarget::Test {
                    id,
                    argument_order,
                    argument_parameters,
                } => {
                    bytes.push(5);
                    bytes.extend_from_slice(&id.suite.0.to_be_bytes());
                    bytes.extend_from_slice(&id.test.0.to_be_bytes());
                    bytes.extend_from_slice(&id.identity.to_be_bytes());
                    append_argument_order(bytes, argument_order);
                    for parameter in argument_parameters.iter() {
                        bytes.extend_from_slice(&parameter.0.to_be_bytes());
                    }
                }
                CallTarget::Struct {
                    definition,
                    field_order,
                    argument_fields,
                    argument_field_definitions,
                    ..
                } => {
                    bytes.push(6);
                    bytes.extend_from_slice(&definition.0.to_be_bytes());
                    for field in &**field_order {
                        append_part(bytes, field.as_bytes());
                    }
                    bytes.push(0xff);
                    for field in &**argument_fields {
                        append_part(bytes, field.as_bytes());
                    }
                    for field in argument_field_definitions.iter() {
                        bytes.extend_from_slice(&field.0.to_be_bytes());
                    }
                }
            }
            bytes.extend_from_slice(
                &u64::try_from(arguments.len())
                    .unwrap_or(u64::MAX)
                    .to_be_bytes(),
            );
            for argument in &**arguments {
                append_expression(bytes, argument);
            }
        }
        ExpressionKind::Array(values) => {
            bytes.push(4);
            bytes.extend_from_slice(
                &u64::try_from(values.len())
                    .unwrap_or(u64::MAX)
                    .to_be_bytes(),
            );
            for value in &**values {
                append_expression(bytes, value);
            }
        }
        ExpressionKind::RepeatedArray { value, length } => {
            bytes.push(15);
            bytes.extend_from_slice(&length.to_be_bytes());
            append_expression(bytes, value);
        }
        ExpressionKind::Tuple(values) => {
            bytes.push(10);
            bytes.extend_from_slice(
                &u64::try_from(values.len())
                    .unwrap_or(u64::MAX)
                    .to_be_bytes(),
            );
            for value in &**values {
                append_expression(bytes, value);
            }
        }
        ExpressionKind::Index { value, index } => {
            bytes.push(12);
            append_expression(bytes, value);
            append_expression(bytes, index);
        }
        ExpressionKind::Positive(value) => {
            bytes.push(13);
            append_expression(bytes, value);
        }
        ExpressionKind::Negate(value) => {
            bytes.push(5);
            append_expression(bytes, value);
        }
        ExpressionKind::BitNot(value) => {
            bytes.push(14);
            append_expression(bytes, value);
        }
        ExpressionKind::Not(value) => {
            bytes.push(9);
            append_expression(bytes, value);
        }
        ExpressionKind::Await(value) => {
            bytes.push(6);
            append_expression(bytes, value);
        }
        ExpressionKind::Propagate(value) => {
            bytes.push(7);
            append_expression(bytes, value);
        }
        ExpressionKind::Is { value, pattern } => {
            bytes.push(18);
            append_expression(bytes, value);
            append_match_pattern(bytes, pattern);
        }
        ExpressionKind::Binary {
            operator,
            left,
            right,
        } => {
            bytes.push(8);
            bytes.push(operator.canonical_tag());
            append_expression(bytes, left);
            append_expression(bytes, right);
        }
    }
}

fn append_literal(bytes: &mut impl ByteSink, literal: &Literal) {
    match literal {
        Literal::Unit => bytes.push(0),
        Literal::Bool(value) => {
            bytes.push(1);
            bytes.push(u8::from(*value));
        }
        Literal::Integer { kind, value } => {
            bytes.push(2);
            append_part(bytes, kind.name().as_bytes());
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        Literal::Float { kind, bits } => {
            bytes.push(3);
            append_part(bytes, kind.name().as_bytes());
            bytes.extend_from_slice(&bits.to_be_bytes());
        }
        Literal::Text(value) => {
            bytes.push(4);
            append_part(bytes, value.as_bytes());
        }
        Literal::Scalar(value) => {
            bytes.push(5);
            bytes.extend_from_slice(&u32::from(*value).to_be_bytes());
        }
        Literal::Bytes(value) => {
            bytes.push(6);
            append_part(bytes, value);
        }
    }
}

fn append_match_pattern(bytes: &mut impl ByteSink, pattern: &HirMatchPattern) {
    match pattern {
        HirMatchPattern::Literal(literal) => {
            bytes.push(0);
            append_literal(bytes, literal);
        }
        HirMatchPattern::Variant { id, payload } => {
            bytes.push(1);
            bytes.extend_from_slice(&id.owner.0.to_be_bytes());
            bytes.extend_from_slice(&id.variant.to_be_bytes());
            bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            for pattern in payload.iter() {
                append_match_pattern(bytes, pattern);
            }
        }
        HirMatchPattern::Struct { definition, fields } => {
            bytes.push(4);
            bytes.extend_from_slice(&definition.0.to_be_bytes());
            bytes.extend_from_slice(&(fields.len() as u32).to_be_bytes());
            for pattern in fields.iter() {
                append_match_pattern(bytes, pattern);
            }
        }
        HirMatchPattern::Tuple(patterns) => {
            bytes.push(5);
            bytes.extend_from_slice(&(patterns.len() as u32).to_be_bytes());
            for pattern in patterns.iter() {
                append_match_pattern(bytes, pattern);
            }
        }
        HirMatchPattern::FixedArray(patterns) => {
            bytes.push(6);
            bytes.extend_from_slice(&(patterns.len() as u32).to_be_bytes());
            for pattern in patterns.iter() {
                append_match_pattern(bytes, pattern);
            }
        }
        HirMatchPattern::Or(alternatives) => {
            bytes.push(7);
            bytes.extend_from_slice(&(alternatives.len() as u32).to_be_bytes());
            for pattern in alternatives.iter() {
                append_match_pattern(bytes, pattern);
            }
        }
        HirMatchPattern::Binding {
            local,
            type_id,
            type_,
            access,
        } => {
            bytes.push(2);
            bytes.extend_from_slice(&local.0.to_be_bytes());
            bytes.extend_from_slice(&type_id.0.to_be_bytes());
            append_part(bytes, &type_.canonical_key());
            bytes.push(access.canonical_tag());
        }
        HirMatchPattern::Wildcard => bytes.push(3),
    }
}

fn append_range(bytes: &mut impl ByteSink, range: &SourceRange) {
    append_part(bytes, range.path().as_bytes());
    bytes.extend_from_slice(&range.start().to_be_bytes());
    bytes.extend_from_slice(&range.end().to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool_owned_bool() -> Type {
        Type::Own {
            pool: PoolTerm::Concrete(PoolId(1)),
            value: Arc::new(Type::Bool),
        }
    }

    #[test]
    fn discharge_law_verifier_rejects_missing_and_duplicate_resource_laws() {
        let type_id = TypeId(1);
        let type_ = pool_owned_bool();
        let observed = BTreeMap::from([(type_id, type_.clone())]);
        let structs = BTreeMap::new();
        let interfaces = BTreeMap::new();
        let displays = BTreeMap::new();
        assert!(matches!(
            verify_discharge_laws(
                &observed,
                &[],
                &BTreeSet::new(),
                &structs,
                &interfaces,
                &displays,
            ),
            Err(VerificationFailure::Defect { .. })
        ));
        let claim = RawDischargeLaw {
            type_id,
            type_,
            law: DischargeLaw::CompilerReclaim(AuthenticatedReclaim::PoolOwnership),
        };
        assert!(matches!(
            verify_discharge_laws(
                &observed,
                &[claim.clone(), claim],
                &BTreeSet::new(),
                &structs,
                &interfaces,
                &displays,
            ),
            Err(VerificationFailure::Defect { .. })
        ));
    }

    #[test]
    fn discharge_law_verifier_rejects_an_unauthenticated_claim() {
        let type_id = TypeId(1);
        let type_ = pool_owned_bool();
        let observed = BTreeMap::from([(type_id, type_.clone())]);
        assert!(matches!(
            verify_discharge_laws(
                &observed,
                &[RawDischargeLaw {
                    type_id,
                    type_,
                    law: DischargeLaw::Explicit,
                }],
                &BTreeSet::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
            ),
            Err(VerificationFailure::Defect { .. })
        ));
    }

    #[test]
    fn independent_discharge_law_verifier_rejects_a_single_component_fault() {
        let owner = DefinitionId(10);
        let field = DefinitionId(11);
        let wrong_field = DefinitionId(12);
        let type_id = TypeId(1);
        let type_ = Type::Nominal {
            definition: owner,
            display: Arc::from("Envelope"),
            arguments: Arc::from([]),
        };
        let observed = BTreeMap::from([(type_id, type_.clone())]);
        let structs = BTreeMap::from([(
            owner,
            ResolvedStruct {
                definition: owner,
                module: ModuleId(1),
                display: Arc::from("Envelope"),
                resource: true,
                type_parameters: Vec::new(),
                generic_parameter_names: Arc::from([]),
                generic_constraints: Arc::from([]),
                fields: vec![ResolvedField {
                    definition: field,
                    name: "ticket".into(),
                    public: false,
                    mutable: false,
                    type_: pool_owned_bool(),
                }],
                field_selections: Arc::from([]),
                applied_fields: RefCell::new(BTreeMap::new()),
            },
        )]);
        let claim = RawDischargeLaw {
            type_id,
            type_,
            law: DischargeLaw::Structural(Arc::from([DischargeComponent {
                identity: DischargeComponentIdentity::Field(wrong_field),
                law: DischargeLaw::CompilerReclaim(AuthenticatedReclaim::PoolOwnership),
            }])),
        };
        assert!(matches!(
            verify_discharge_laws(
                &observed,
                &[claim],
                &BTreeSet::new(),
                &structs,
                &BTreeMap::new(),
                &BTreeMap::from([(owner, Arc::from("Envelope"))]),
            ),
            Err(VerificationFailure::Defect { .. })
        ));
    }

    #[test]
    fn discharge_law_verifier_rejects_mixed_type_and_type_id_artifacts() {
        let type_id = TypeId(1);
        let observed = BTreeMap::from([(type_id, pool_owned_bool())]);
        assert!(matches!(
            verify_discharge_laws(
                &observed,
                &[RawDischargeLaw {
                    type_id,
                    type_: Type::Bool,
                    law: DischargeLaw::CompilerReclaim(AuthenticatedReclaim::PoolOwnership,),
                }],
                &BTreeSet::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
            ),
            Err(VerificationFailure::Defect { .. })
        ));
    }

    #[test]
    fn artifact_parameter_custody_separates_take_boundaries_from_loans() {
        let take = LocalId(1);
        let read = LocalId(2);
        let mutate = LocalId(3);
        let value = LocalId(4);
        let roles = ArtifactParameterCustody::from_parameters(&[
            (take, pool_owned_bool(), AccessMode::Move),
            (read, pool_owned_bool(), AccessMode::Read),
            (mutate, pool_owned_bool(), AccessMode::Mut),
            (value, Type::Bool, AccessMode::Copy),
        ]);
        assert_eq!(roles.discharge_boundaries, BTreeSet::from([take]));
        assert_eq!(roles.non_custodial_loans, BTreeSet::from([read, mutate]));
        assert_eq!(roles.invalid_value_parameters, BTreeSet::from([value]));
    }

    #[test]
    fn discharge_law_builder_observes_each_resource_component_type_id() {
        let nested = pool_owned_bool();
        let aggregate = Type::FixedArray {
            element: Arc::new(nested.clone()),
            length: ArrayLength::Value(2),
        };
        let mut builder = DischargeLawBuilder::default();
        let mut identities = crate::identity::IdentityCatalog::empty();
        let aggregate_id = observe_type_tree(&aggregate, &mut identities, &mut builder)
            .expect("aggregate and component types are observed");
        let observed = &builder.observed;
        assert_eq!(observed.len(), 3);
        assert!(observed.values().any(|type_| type_ == &nested));
        assert_eq!(observed.get(&aggregate_id), Some(&aggregate));
    }

    #[test]
    fn post_lowering_discharge_verifier_rejects_a_live_resource_artifact() {
        let local = LocalId(1);
        let definition = DefinitionId(9);
        let resource = Type::Nominal {
            definition,
            display: Arc::from("Ticket"),
            arguments: Arc::from([]),
        };
        let templates = BTreeMap::new();
        let specialized = BTreeMap::new();
        let constants = BTreeMap::new();
        let specializations = BTreeMap::new();
        let variants = BTreeMap::new();
        let structs = BTreeMap::from([(
            definition,
            ResolvedStruct {
                definition,
                module: ModuleId(1),
                display: Arc::from("Ticket"),
                resource: true,
                type_parameters: Vec::new(),
                generic_parameter_names: Arc::from([]),
                generic_constraints: Arc::from([]),
                fields: Vec::new(),
                field_selections: Arc::from([]),
                applied_fields: RefCell::new(BTreeMap::new()),
            },
        )]);
        let interfaces = BTreeMap::new();
        let identities = crate::identity::IdentityCatalog::empty();
        let resource_id = TypeId(1);
        let catalog = ArtifactCatalog {
            templates: &templates,
            specialized: &specialized,
            constants: &constants,
            specializations: &specializations,
            identities: &identities,
            variants: &variants,
            structs: &structs,
            interfaces: &interfaces,
            discharge_laws: None,
        };
        assert!(matches!(
            verify_artifact_scope_discharged(
                &BTreeMap::from([(
                    local,
                    ArtifactLocalType {
                        type_id: resource_id,
                        type_: resource,
                    },
                )]),
                &MoveState::default(),
                &BTreeSet::new(),
                &catalog,
            ),
            Err(VerificationFailure::Defect { .. })
        ));
    }

    #[test]
    fn malformed_compiler_artifact_is_contained_as_a_verification_defect() {
        let mut identities = crate::identity::IdentityCatalog::empty();
        let id = DefinitionId(1);
        let mut input = ProgramInput::default();
        input.functions.insert(
            DefinitionId(2),
            ResolvedFunction {
                id,
                module: ModuleId(1),
                module_display: "image".to_owned(),
                name: "broken".to_owned(),
                modifier: crate::syntax::FunctionModifier::Ordinary,
                type_parameters: Arc::from([]),
                generic_parameter_names: Arc::from([]),
                generic_constraints: Arc::from([]),
                parameters: Vec::new(),
                return_type: Type::Unit,
                body: Vec::new(),
                source: SourceRange::from_u64("src/image.wr", 1, 2),
            },
        );
        assert!(matches!(
            verify(
                input,
                &BuildAuthority::test_compiler_distribution(),
                &PoolAuthority::from_authenticated_scoped_factory(None),
                &mut identities,
                &Cancellation::new()
            ),
            Err(VerificationFailure::Defect { .. })
        ));
    }

    #[test]
    fn post_lowering_verifier_recomputes_operation_types() {
        let range = SourceRange::from_u64("src/image.wr", 0, 1);
        let mut identities = crate::identity::IdentityCatalog::empty();
        let bool_id = identities.intern_type(&Type::Bool).expect("type interns");
        let boolean = || Expression {
            kind: ExpressionKind::Literal(Literal::Bool(true)),
            type_id: bool_id,
            type_: Type::Bool,
            coerced_from: None,
            access: AccessMode::Copy,
            source: range.clone(),
        };
        let malformed = Expression {
            kind: ExpressionKind::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(boolean()),
                right: Box::new(boolean()),
            },
            type_id: bool_id,
            type_: Type::Bool,
            coerced_from: None,
            access: AccessMode::Copy,
            source: range,
        };
        let templates = BTreeMap::new();
        let specialized = BTreeMap::new();
        let constants = BTreeMap::new();
        let specializations = BTreeMap::new();
        let variants = BTreeMap::new();
        let structs = BTreeMap::new();
        let interfaces = BTreeMap::new();
        let catalog = ArtifactCatalog {
            templates: &templates,
            specialized: &specialized,
            constants: &constants,
            specializations: &specializations,
            identities: &identities,
            variants: &variants,
            structs: &structs,
            interfaces: &interfaces,
            discharge_laws: None,
        };
        assert!(matches!(
            verify_expression_artifact(
                &malformed,
                &BTreeMap::new(),
                &mut MoveState::default(),
                &catalog,
                &SourceRange::from_u64("src/image.wr", 0, 1),
            ),
            Err(VerificationFailure::Defect { .. })
        ));

        let owner = SourceRange::from_u64("src/image.wr", 0, 1);
        let mut previous = owner.start();
        assert!(matches!(
            verify_statements(
                &[Statement::Pass(
                    SourceRange::from_u64("src/image.wr", 2, 3,)
                )],
                &Type::Unit,
                &mut BTreeMap::new(),
                &mut MoveState::default(),
                &mut Vec::new(),
                &ArtifactVerificationAuthority {
                    catalog: &catalog,
                    provenance_owner: &owner,
                    discharge_exempt: &BTreeSet::new(),
                },
                &mut previous,
            ),
            Err(VerificationFailure::Defect { .. })
        ));
    }

    #[test]
    fn post_lowering_verifier_rejects_unsigned_negation() {
        let range = SourceRange::from_u64("src/image.wr", 0, 1);
        let mut identities = crate::identity::IdentityCatalog::empty();
        let type_ = Type::Integer(IntegerType::U8);
        let type_id = identities.intern_type(&type_).expect("type interns");
        let operand = Expression {
            kind: ExpressionKind::Literal(Literal::Integer {
                kind: IntegerType::U8,
                value: 1,
            }),
            type_id,
            type_: type_.clone(),
            coerced_from: None,
            access: AccessMode::Copy,
            source: range.clone(),
        };
        let malformed = Expression {
            kind: ExpressionKind::Negate(Box::new(operand)),
            type_id,
            type_: type_.clone(),
            coerced_from: None,
            access: AccessMode::Copy,
            source: range.clone(),
        };
        let templates = BTreeMap::new();
        let specialized = BTreeMap::new();
        let constants = BTreeMap::new();
        let specializations = BTreeMap::new();
        let variants = BTreeMap::new();
        let structs = BTreeMap::new();
        let interfaces = BTreeMap::new();
        let catalog = ArtifactCatalog {
            templates: &templates,
            specialized: &specialized,
            constants: &constants,
            specializations: &specializations,
            identities: &identities,
            variants: &variants,
            structs: &structs,
            interfaces: &interfaces,
            discharge_laws: None,
        };

        assert!(matches!(
            verify_expression_artifact(
                &malformed,
                &BTreeMap::new(),
                &mut MoveState::default(),
                &catalog,
                &range,
            ),
            Err(VerificationFailure::Defect { .. })
        ));
    }

    #[test]
    fn post_lowering_verifier_rejects_loop_control_outside_a_loop() {
        let range = SourceRange::from_u64("src/image.wr", 0, 1);
        assert!(matches!(
            verify_loop_control(&[Statement::Break(range.clone())], 0),
            Err(VerificationFailure::Defect { .. })
        ));
        assert!(
            verify_loop_control(
                &[Statement::While {
                    condition: Expression {
                        kind: ExpressionKind::Literal(Literal::Bool(true)),
                        type_id: TypeId(0),
                        type_: Type::Bool,
                        coerced_from: None,
                        access: AccessMode::Copy,
                        source: range.clone(),
                    },
                    body: Arc::from([Statement::Continue(range.clone())]),
                    max_iterations: 1,
                    source: range,
                }],
                0,
            )
            .is_ok()
        );
    }

    #[test]
    fn post_lowering_verifier_rejects_path_dependent_custody() {
        let owner = SourceRange::from_u64("src/image.wr", 0, 10);
        let condition_range = SourceRange::from_u64("src/image.wr", 1, 2);
        let move_range = SourceRange::from_u64("src/image.wr", 3, 4);
        let local = LocalId(1);
        let owned_type = Type::Own {
            pool: PoolTerm::Concrete(PoolId(1)),
            value: Arc::new(Type::Bool),
        };
        let mut identities = crate::identity::IdentityCatalog::empty();
        let bool_id = identities.intern_type(&Type::Bool).expect("type interns");
        let owned_id = identities
            .intern_type(&owned_type)
            .expect("resource type interns");
        let condition = Expression {
            kind: ExpressionKind::Literal(Literal::Bool(true)),
            type_id: bool_id,
            type_: Type::Bool,
            coerced_from: None,
            access: AccessMode::Copy,
            source: condition_range,
        };
        let moved_value = Expression {
            kind: ExpressionKind::Read(Place::local(local)),
            type_id: owned_id,
            type_: owned_type.clone(),
            coerced_from: None,
            access: AccessMode::Move,
            source: move_range.clone(),
        };
        let statements = [Statement::If {
            condition,
            then_branch: Arc::from([Statement::Evaluate(moved_value)]),
            else_branch: Arc::from([]),
            source: SourceRange::from_u64("src/image.wr", 1, 5),
        }];
        let templates = BTreeMap::new();
        let specialized = BTreeMap::new();
        let constants = BTreeMap::new();
        let specializations = BTreeMap::new();
        let variants = BTreeMap::new();
        let structs = BTreeMap::new();
        let interfaces = BTreeMap::new();
        let catalog = ArtifactCatalog {
            templates: &templates,
            specialized: &specialized,
            constants: &constants,
            specializations: &specializations,
            identities: &identities,
            variants: &variants,
            structs: &structs,
            interfaces: &interfaces,
            discharge_laws: None,
        };
        let mut locals = BTreeMap::from([(
            local,
            ArtifactLocalType {
                type_id: owned_id,
                type_: owned_type,
            },
        )]);
        let mut previous = owner.start();
        assert!(matches!(
            verify_statements(
                &statements,
                &Type::Unit,
                &mut locals,
                &mut MoveState::default(),
                &mut Vec::new(),
                &ArtifactVerificationAuthority {
                    catalog: &catalog,
                    provenance_owner: &owner,
                    discharge_exempt: &BTreeSet::new(),
                },
                &mut previous,
            ),
            Err(VerificationFailure::Defect { .. })
        ));
    }

    #[test]
    fn post_lowering_verifier_rejects_illegal_moves_and_overlapping_loans() {
        let owner = SourceRange::from_u64("src/image.wr", 0, 10);
        let source = SourceRange::from_u64("src/image.wr", 1, 2);
        let local = LocalId(1);
        let mut identities = crate::identity::IdentityCatalog::empty();
        let bool_id = identities.intern_type(&Type::Bool).expect("type interns");
        let owned_type = Type::Own {
            pool: PoolTerm::Concrete(PoolId(1)),
            value: Arc::new(Type::Bool),
        };
        let owned_id = identities
            .intern_type(&owned_type)
            .expect("Resource-owning type interns");
        let moved_data = Expression {
            kind: ExpressionKind::Read(Place::local(local)),
            type_id: bool_id,
            type_: Type::Bool,
            coerced_from: None,
            access: AccessMode::Move,
            source: source.clone(),
        };
        let templates = BTreeMap::new();
        let specialized = BTreeMap::new();
        let constants = BTreeMap::new();
        let specializations = BTreeMap::new();
        let variants = BTreeMap::new();
        let structs = BTreeMap::new();
        let interfaces = BTreeMap::new();
        let catalog = ArtifactCatalog {
            templates: &templates,
            specialized: &specialized,
            constants: &constants,
            specializations: &specializations,
            identities: &identities,
            variants: &variants,
            structs: &structs,
            interfaces: &interfaces,
            discharge_laws: None,
        };
        assert!(matches!(
            verify_expression_artifact(
                &moved_data,
                &BTreeMap::from([(
                    local,
                    ArtifactLocalType {
                        type_id: bool_id,
                        type_: Type::Bool,
                    },
                )]),
                &mut MoveState::default(),
                &catalog,
                &owner,
            ),
            Err(VerificationFailure::Defect { .. })
        ));
        let loan = |access| Expression {
            kind: ExpressionKind::Read(Place::local(local)),
            type_id: owned_id,
            type_: owned_type.clone(),
            coerced_from: None,
            access,
            source: source.clone(),
        };
        assert!(matches!(
            verify_artifact_call_custody(&[loan(AccessMode::Mut), loan(AccessMode::Read)]),
            Err(VerificationFailure::Defect { .. })
        ));
    }

    #[test]
    fn post_lowering_verifier_rejects_live_resource_replacement() {
        let owner = SourceRange::from_u64("src/image.wr", 0, 10);
        let source = SourceRange::from_u64("src/image.wr", 1, 2);
        let destination = LocalId(1);
        let source_local = LocalId(2);
        let owned_type = Type::Own {
            pool: PoolTerm::Concrete(PoolId(1)),
            value: Arc::new(Type::Bool),
        };
        let mut identities = crate::identity::IdentityCatalog::empty();
        let owned_id = identities
            .intern_type(&owned_type)
            .expect("Resource-owning type interns");
        let replacement = Expression {
            kind: ExpressionKind::Read(Place::local(source_local)),
            type_id: owned_id,
            type_: owned_type.clone(),
            coerced_from: None,
            access: AccessMode::Move,
            source: source.clone(),
        };
        let statements = [Statement::Assign {
            place: Place::local(destination),
            value: replacement,
            source,
        }];
        let templates = BTreeMap::new();
        let specialized = BTreeMap::new();
        let constants = BTreeMap::new();
        let specializations = BTreeMap::new();
        let variants = BTreeMap::new();
        let structs = BTreeMap::new();
        let interfaces = BTreeMap::new();
        let catalog = ArtifactCatalog {
            templates: &templates,
            specialized: &specialized,
            constants: &constants,
            specializations: &specializations,
            identities: &identities,
            variants: &variants,
            structs: &structs,
            interfaces: &interfaces,
            discharge_laws: None,
        };
        let mut locals = BTreeMap::from([
            (
                destination,
                ArtifactLocalType {
                    type_id: owned_id,
                    type_: owned_type.clone(),
                },
            ),
            (
                source_local,
                ArtifactLocalType {
                    type_id: owned_id,
                    type_: owned_type,
                },
            ),
        ]);
        let mut previous = owner.start();
        assert!(matches!(
            verify_statements(
                &statements,
                &Type::Unit,
                &mut locals,
                &mut MoveState::default(),
                &mut Vec::new(),
                &ArtifactVerificationAuthority {
                    catalog: &catalog,
                    provenance_owner: &owner,
                    discharge_exempt: &BTreeSet::new(),
                },
                &mut previous,
            ),
            Err(VerificationFailure::Defect { .. })
        ));
    }

    #[test]
    fn post_lowering_verifier_rejects_a_moved_loop_back_edge() {
        let owner = SourceRange::from_u64("src/image.wr", 0, 10);
        let loop_range = SourceRange::from_u64("src/image.wr", 1, 5);
        let move_range = SourceRange::from_u64("src/image.wr", 3, 4);
        let local = LocalId(1);
        let mut identities = crate::identity::IdentityCatalog::empty();
        let bool_id = identities.intern_type(&Type::Bool).expect("type interns");
        let array_type = Type::FixedArray {
            element: Arc::new(Type::Bool),
            length: ArrayLength::Value(2),
        };
        let array_id = identities
            .intern_type(&array_type)
            .expect("array type interns");
        let owned_type = Type::Own {
            pool: PoolTerm::Concrete(PoolId(1)),
            value: Arc::new(Type::Bool),
        };
        let owned_id = identities
            .intern_type(&owned_type)
            .expect("Resource-owning type interns");
        let iterable = Expression {
            kind: ExpressionKind::Array(
                [true, false]
                    .into_iter()
                    .map(|value| Expression {
                        kind: ExpressionKind::Literal(Literal::Bool(value)),
                        type_id: bool_id,
                        type_: Type::Bool,
                        coerced_from: None,
                        access: AccessMode::Copy,
                        source: SourceRange::from_u64("src/image.wr", 1, 2),
                    })
                    .collect(),
            ),
            type_id: array_id,
            type_: array_type,
            coerced_from: None,
            access: AccessMode::Copy,
            source: loop_range.clone(),
        };
        let moved = Expression {
            kind: ExpressionKind::Read(Place::local(local)),
            type_id: owned_id,
            type_: owned_type.clone(),
            coerced_from: None,
            access: AccessMode::Move,
            source: move_range,
        };
        let statements = [Statement::For {
            pattern: HirMatchPattern::Wildcard,
            iterable,
            body: Arc::from([Statement::Evaluate(moved)]),
            source: loop_range,
        }];
        let templates = BTreeMap::new();
        let specialized = BTreeMap::new();
        let constants = BTreeMap::new();
        let specializations = BTreeMap::new();
        let variants = BTreeMap::new();
        let structs = BTreeMap::new();
        let interfaces = BTreeMap::new();
        let catalog = ArtifactCatalog {
            templates: &templates,
            specialized: &specialized,
            constants: &constants,
            specializations: &specializations,
            identities: &identities,
            variants: &variants,
            structs: &structs,
            interfaces: &interfaces,
            discharge_laws: None,
        };
        let mut previous = owner.start();
        assert!(matches!(
            verify_statements(
                &statements,
                &Type::Unit,
                &mut BTreeMap::from([(
                    local,
                    ArtifactLocalType {
                        type_id: owned_id,
                        type_: owned_type,
                    },
                )]),
                &mut MoveState::default(),
                &mut Vec::new(),
                &ArtifactVerificationAuthority {
                    catalog: &catalog,
                    provenance_owner: &owner,
                    discharge_exempt: &BTreeSet::new(),
                },
                &mut previous,
            ),
            Err(VerificationFailure::Defect { .. })
        ));
    }

    #[test]
    fn cleanup_verifier_rejects_each_single_fault_in_its_execution_contract() {
        let owner = SourceRange::from_u64("src/image.wr", 0, 10);
        let source = SourceRange::from_u64("src/image.wr", 1, 2);
        let local = LocalId(1);
        let second_local = LocalId(4);
        let resource = pool_owned_bool();
        let mut identities = crate::identity::IdentityCatalog::empty();
        let resource_id = identities
            .intern_type(&resource)
            .expect("Resource-owning type interns");
        let unit_id = identities.intern_type(&Type::Unit).expect("Unit interns");
        let bool_id = identities.intern_type(&Type::Bool).expect("Bool interns");
        let tuple_type = Type::Tuple(Arc::from([Type::Unit, Type::Unit]));
        let tuple_id = identities.intern_type(&tuple_type).expect("tuple interns");
        let resource_tuple_type = Type::Tuple(Arc::from([resource.clone(), resource.clone()]));
        let resource_tuple_id = identities
            .intern_type(&resource_tuple_type)
            .expect("Resource tuple interns");
        let callable_local = LocalId(3);
        let callable_type = Type::Function {
            parameters: Arc::from([]),
            return_type: Arc::new(Type::Unit),
        };
        let callable_id = identities
            .intern_type(&callable_type)
            .expect("callable type interns");
        let definitions = [DefinitionId(10), DefinitionId(20), DefinitionId(30)];
        let specializations = [
            SpecializationId(11),
            SpecializationId(21),
            SpecializationId(31),
        ];
        let parameter_definitions = [DefinitionId(12), DefinitionId(22), DefinitionId(32)];
        let accesses = [AccessMode::Move, AccessMode::Read, AccessMode::Move];
        let returns = [Type::Unit, Type::Unit, resource.clone()];
        let specialized = definitions
            .iter()
            .zip(specializations)
            .zip(parameter_definitions)
            .zip(accesses)
            .zip(returns.iter())
            .map(
                |((((definition, specialization), parameter), access), return_type)| {
                    (
                        specialization,
                        Arc::new(HirFunction {
                            id: *definition,
                            name: "cleanup".to_owned(),
                            module_display: "image".to_owned(),
                            modifier: crate::syntax::FunctionModifier::Ordinary,
                            parameters: vec![(LocalId(2), resource.clone(), access)],
                            parameter_type_ids: Arc::from([resource_id]),
                            parameter_definitions: Arc::from([parameter]),
                            return_type: return_type.clone(),
                            body: Arc::from([]),
                            source: owner.clone(),
                        }),
                    )
                },
            )
            .collect::<BTreeMap<_, _>>();
        let specialization_records = definitions
            .iter()
            .zip(specializations)
            .map(|(definition, id)| {
                (
                    id,
                    SpecializationRecord {
                        id,
                        definition: *definition,
                        type_arguments: Arc::from([]),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let templates = BTreeMap::new();
        let constants = BTreeMap::new();
        let variants = BTreeMap::new();
        let structs = BTreeMap::new();
        let interfaces = BTreeMap::new();
        let catalog = ArtifactCatalog {
            templates: &templates,
            specialized: &specialized,
            constants: &constants,
            specializations: &specialization_records,
            identities: &identities,
            variants: &variants,
            structs: &structs,
            interfaces: &interfaces,
            discharge_laws: None,
        };
        let read_from = |local, access| Expression {
            kind: ExpressionKind::Read(Place::local(local)),
            type_id: resource_id,
            type_: resource.clone(),
            coerced_from: None,
            access,
            source: source.clone(),
        };
        let read = |access| read_from(local, access);
        let call = |index: usize, argument: Expression| {
            let type_ = returns[index].clone();
            let type_id = if type_ == Type::Unit {
                unit_id
            } else {
                resource_id
            };
            Expression {
                kind: ExpressionKind::Call {
                    target: CallTarget::Function {
                        definition: definitions[index],
                        specialization: specializations[index],
                        argument_order: Arc::from([0]),
                        argument_parameters: Arc::from([parameter_definitions[index]]),
                    },
                    arguments: Arc::from([argument]),
                },
                type_id,
                type_,
                coerced_from: None,
                access: if type_id == resource_id {
                    AccessMode::Move
                } else {
                    AccessMode::Copy
                },
                source: source.clone(),
            }
        };
        let verify_action = |action| {
            let mut previous = owner.start();
            verify_statements(
                &[Statement::Defer {
                    action,
                    source: source.clone(),
                }],
                &Type::Unit,
                &mut BTreeMap::from([
                    (
                        local,
                        ArtifactLocalType {
                            type_id: resource_id,
                            type_: resource.clone(),
                        },
                    ),
                    (
                        callable_local,
                        ArtifactLocalType {
                            type_id: callable_id,
                            type_: callable_type.clone(),
                        },
                    ),
                    (
                        second_local,
                        ArtifactLocalType {
                            type_id: resource_id,
                            type_: resource.clone(),
                        },
                    ),
                ]),
                &mut MoveState::default(),
                &mut Vec::new(),
                &ArtifactVerificationAuthority {
                    catalog: &catalog,
                    provenance_owner: &owner,
                    discharge_exempt: &BTreeSet::new(),
                },
                &mut previous,
            )
        };
        let evidence = |result: Result<(), VerificationFailure>| match result {
            Err(VerificationFailure::Defect { evidence }) => evidence,
            other => panic!("expected a contained verifier defect, got {other:?}"),
        };
        let slot = |capture: &Expression, ordinal| Expression {
            kind: ExpressionKind::CleanupCapture(CleanupCaptureOrdinal(ordinal)),
            type_id: capture.type_id,
            type_: capture.type_.clone(),
            coerced_from: capture.coerced_from.clone(),
            access: capture.access,
            source: capture.source.clone(),
        };
        let capture = |ordinal, expression| CleanupCapture {
            ordinal: CleanupCaptureOrdinal(ordinal),
            expression,
        };

        let resource_result = read(AccessMode::Move);
        assert_eq!(
            evidence(verify_action(CleanupAction {
                expression: call(2, slot(&resource_result, 0)),
                captures: Arc::from([capture(0, resource_result)]),
            }))
            .as_ref(),
            "lowered cleanup action leaves a Resource result undisposed"
        );
        let hidden_resource = read(AccessMode::Move);
        let hidden_call = call(0, slot(&hidden_resource, 0));
        assert_eq!(
            evidence(verify_action(CleanupAction {
                expression: Expression {
                    kind: ExpressionKind::Tuple(Arc::from([
                        hidden_call,
                        Expression {
                            kind: ExpressionKind::Literal(Literal::Unit),
                            type_id: unit_id,
                            type_: Type::Unit,
                            coerced_from: None,
                            access: AccessMode::Copy,
                            source: source.clone(),
                        },
                    ])),
                    type_id: tuple_id,
                    type_: tuple_type.clone(),
                    coerced_from: None,
                    access: AccessMode::Copy,
                    source: source.clone(),
                },
                captures: Arc::from([capture(0, hidden_resource)]),
            }))
            .as_ref(),
            "lowered non-call cleanup expression captures Resource custody"
        );
        assert_eq!(
            evidence(verify_action(CleanupAction {
                expression: call(1, read(AccessMode::Read)),
                captures: Arc::from([]),
            }))
            .as_ref(),
            "lowered cleanup action retains a Resource loan"
        );

        let missing = read(AccessMode::Move);
        assert_eq!(
            evidence(verify_action(CleanupAction {
                expression: Expression {
                    kind: ExpressionKind::Literal(Literal::Unit),
                    type_id: unit_id,
                    type_: Type::Unit,
                    coerced_from: None,
                    access: AccessMode::Copy,
                    source: source.clone(),
                },
                captures: Arc::from([capture(0, missing)]),
            }))
            .as_ref(),
            "lowered cleanup capture coverage has missing or extra references"
        );

        let extra = read(AccessMode::Move);
        assert_eq!(
            evidence(verify_action(CleanupAction {
                expression: slot(&extra, 0),
                captures: Arc::from([]),
            }))
            .as_ref(),
            "lowered cleanup capture coverage has missing or extra references"
        );

        let first = read_from(local, AccessMode::Move);
        let second = read_from(second_local, AccessMode::Move);
        assert_eq!(
            evidence(verify_action(CleanupAction {
                expression: Expression {
                    kind: ExpressionKind::Tuple(Arc::from([slot(&first, 0), slot(&second, 0),])),
                    type_id: resource_tuple_id,
                    type_: resource_tuple_type,
                    coerced_from: None,
                    access: AccessMode::Move,
                    source: source.clone(),
                },
                captures: Arc::from([capture(0, first), capture(1, second)]),
            }))
            .as_ref(),
            "lowered cleanup capture references are not in canonical order"
        );

        let noncanonical = read(AccessMode::Move);
        assert_eq!(
            evidence(verify_action(CleanupAction {
                expression: slot(&noncanonical, 1),
                captures: Arc::from([capture(1, noncanonical)]),
            }))
            .as_ref(),
            "lowered cleanup capture ordinals are not canonical"
        );

        let data_capture = Expression {
            kind: ExpressionKind::Literal(Literal::Unit),
            type_id: unit_id,
            type_: Type::Unit,
            coerced_from: None,
            access: AccessMode::Copy,
            source: source.clone(),
        };
        assert_eq!(
            evidence(verify_action(CleanupAction {
                expression: slot(&data_capture, 0),
                captures: Arc::from([capture(0, data_capture)]),
            }))
            .as_ref(),
            "lowered cleanup registration captures a non-Resource expression"
        );

        let metadata_capture = read(AccessMode::Move);
        let mut wrong_metadata = slot(&metadata_capture, 0);
        wrong_metadata.access = AccessMode::Copy;
        assert_eq!(
            evidence(verify_action(CleanupAction {
                expression: wrong_metadata,
                captures: Arc::from([capture(0, metadata_capture)]),
            }))
            .as_ref(),
            "cleanup capture substitution metadata is inconsistent"
        );

        let type_capture = read(AccessMode::Move);
        let mut wrong_type = slot(&type_capture, 0);
        wrong_type.type_id = bool_id;
        wrong_type.type_ = Type::Bool;
        assert_eq!(
            evidence(verify_action(CleanupAction {
                expression: wrong_type,
                captures: Arc::from([capture(0, type_capture)]),
            }))
            .as_ref(),
            "cleanup capture substitution metadata is inconsistent"
        );

        let provenance_capture = read(AccessMode::Move);
        let mut wrong_provenance = slot(&provenance_capture, 0);
        wrong_provenance.source = SourceRange::from_u64("src/image.wr", 2, 3);
        assert_eq!(
            evidence(verify_action(CleanupAction {
                expression: wrong_provenance,
                captures: Arc::from([capture(0, provenance_capture)]),
            }))
            .as_ref(),
            "cleanup capture substitution metadata is inconsistent"
        );

        assert_eq!(
            evidence(verify_action(CleanupAction {
                expression: call(0, read(AccessMode::Move)),
                captures: Arc::from([]),
            }))
            .as_ref(),
            "lowered cleanup expression retains an uncaptured Resource move"
        );

        let conditional = read(AccessMode::Move);
        assert_eq!(
            evidence(verify_action(CleanupAction {
                expression: Expression {
                    kind: ExpressionKind::Binary {
                        operator: BinaryOperator::And,
                        left: Box::new(Expression {
                            kind: ExpressionKind::Literal(Literal::Bool(false)),
                            type_id: bool_id,
                            type_: Type::Bool,
                            coerced_from: None,
                            access: AccessMode::Copy,
                            source: source.clone(),
                        }),
                        right: Box::new(slot(&conditional, 0)),
                    },
                    type_id: bool_id,
                    type_: Type::Bool,
                    coerced_from: None,
                    access: AccessMode::Copy,
                    source: source.clone(),
                },
                captures: Arc::from([capture(0, conditional)]),
            }))
            .as_ref(),
            "lowered cleanup capture reference is not unconditionally evaluated"
        );
    }
}
