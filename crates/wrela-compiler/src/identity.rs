use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use xxhash_rust::xxh3::{Xxh3, xxh3_128};

use crate::compiler::{
    Cancellation, IdentityDomain, IdentityObservation, IdentityOrigin, ProjectFile,
};
use crate::model::{
    DefinitionId, ModuleId, PoolId, SpecializationId, TestId, Type, TypeId, VariantId,
};
use crate::syntax::{DeclarationKind, DeclarationSyntax, ParsedSource};

#[derive(Clone, Debug)]
pub(crate) struct IdentityCollision {
    pub(crate) digest: u128,
    pub(crate) first_key: Arc<[u8]>,
    pub(crate) second_key: Arc<[u8]>,
}

#[derive(Clone, Debug)]
pub(crate) enum IdentityFailure {
    Collision(IdentityCollision),
    Cancelled,
}

#[derive(Clone, Debug)]
pub(crate) struct IdentityCatalog {
    records: Vec<IdentityRecord>,
    specialization_records: BTreeMap<SpecializationId, usize>,
    observation_keys: BTreeSet<Arc<[u8]>>,
    modules: BTreeMap<String, ModuleId>,
    definitions: BTreeMap<(String, DeclarationKind, String), DefinitionId>,
    associated_functions: BTreeMap<(DefinitionId, String), DefinitionId>,
    tests: BTreeMap<(String, String, String), TestId>,
    types: BTreeMap<DefinitionId, TypeId>,
    pools: BTreeMap<DefinitionId, PoolId>,
    interned_types: BTreeMap<Arc<[u8]>, TypeId>,
    variants: BTreeMap<(DefinitionId, String), VariantId>,
    full_keys: BTreeMap<u128, Arc<[u8]>>,
}

#[derive(Clone, Debug)]
struct IdentityRecord {
    observation: IdentityObservation,
    canonical_key: Arc<[u8]>,
}

impl IdentityRecord {
    fn digest(&self) -> u128 {
        self.observation.digest()
    }
}

impl IdentityCatalog {
    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self {
            records: Vec::new(),
            specialization_records: BTreeMap::new(),
            observation_keys: BTreeSet::new(),
            modules: BTreeMap::new(),
            definitions: BTreeMap::new(),
            associated_functions: BTreeMap::new(),
            tests: BTreeMap::new(),
            types: BTreeMap::new(),
            pools: BTreeMap::new(),
            interned_types: BTreeMap::new(),
            variants: BTreeMap::new(),
            full_keys: BTreeMap::new(),
        }
    }

    pub(crate) fn project_observations(&self) -> Vec<IdentityObservation> {
        self.records
            .iter()
            .map(|record| record.observation.clone())
            .collect()
    }

    pub(crate) fn revision_fingerprint(&self) -> u128 {
        let mut hasher = Xxh3::new();
        hasher.update(b"wrela.identity-catalog-revision\0\x01");
        for record in &self.records {
            hash_part(&mut hasher, &record.canonical_key);
            hasher.update(&record.observation.fingerprint().to_be_bytes());
        }
        hasher.digest128()
    }

    pub(crate) fn finalize(&mut self) {
        self.records
            .sort_by(|left, right| left.canonical_key.cmp(&right.canonical_key));
        self.rebuild_specialization_index();
    }

    pub(crate) fn set_specialization_fingerprint(
        &mut self,
        id: SpecializationId,
        fingerprint: u128,
    ) -> bool {
        let Some(index) = self.specialization_records.get(&id).copied() else {
            return false;
        };
        let record = &mut self.records[index];
        record.observation.replace_fingerprint(fingerprint);
        true
    }

    fn push_record(&mut self, record: IdentityRecord) {
        if self
            .observation_keys
            .insert(Arc::clone(&record.canonical_key))
        {
            if record.observation.domain() == IdentityDomain::Specialization {
                self.specialization_records.insert(
                    SpecializationId(record.observation.digest()),
                    self.records.len(),
                );
            }
            self.records.push(record);
        }
    }

    fn rebuild_specialization_index(&mut self) {
        self.specialization_records.clear();
        self.specialization_records.extend(
            self.records
                .iter()
                .enumerate()
                .filter(|(_, record)| record.observation.domain() == IdentityDomain::Specialization)
                .map(|(index, record)| (SpecializationId(record.observation.digest()), index)),
        );
    }

    pub(crate) fn module(&self, path: &str) -> Option<ModuleId> {
        self.modules.get(path).copied()
    }

    pub(crate) fn definition(
        &self,
        path: &str,
        kind: DeclarationKind,
        name: &str,
    ) -> Option<DefinitionId> {
        self.definitions
            .get(&(path.to_owned(), kind, name.to_owned()))
            .copied()
    }

    pub(crate) fn test(&self, path: &str, suite: &str, test: &str) -> Option<TestId> {
        self.tests
            .get(&(path.to_owned(), suite.to_owned(), test.to_owned()))
            .copied()
    }

    pub(crate) fn associated_function(
        &self,
        owner: DefinitionId,
        name: &str,
    ) -> Option<DefinitionId> {
        self.associated_functions
            .get(&(owner, name.to_owned()))
            .copied()
    }

    pub(crate) fn type_for_definition(&self, definition: DefinitionId) -> Option<TypeId> {
        self.types.get(&definition).copied()
    }

    pub(crate) fn intern_type(&mut self, type_: &Type) -> Result<TypeId, IdentityCollision> {
        match type_ {
            Type::Array(element)
            | Type::FixedArray { element, .. }
            | Type::Own { value: element, .. }
            | Type::Option(element) => {
                self.intern_type(element)?;
            }
            Type::Tuple(members) => {
                for member in &**members {
                    self.intern_type(member)?;
                }
            }
            Type::Result { success, error } => {
                self.intern_type(success)?;
                if let Some(error) = error {
                    self.intern_type(error)?;
                }
            }
            Type::Function {
                parameters,
                return_type,
            } => {
                for parameter in &**parameters {
                    self.intern_type(parameter)?;
                }
                self.intern_type(return_type)?;
            }
            Type::Nominal { arguments, .. } => {
                for argument in &**arguments {
                    self.intern_type(argument)?;
                }
            }
            Type::Unit
            | Type::Bool
            | Type::Integer(_)
            | Type::Float(_)
            | Type::Text
            | Type::Scalar
            | Type::Bytes
            | Type::Builtin(_)
            | Type::Any { .. }
            | Type::Parameter { .. }
            | Type::Infer => {}
        }
        let type_key = type_.canonical_key();
        if let Some(id) = self.interned_types.get(&type_key) {
            return Ok(*id);
        }
        if let Type::Nominal {
            definition,
            arguments,
            ..
        } = type_
            && arguments.is_empty()
        {
            let id = self
                .types
                .get(definition)
                .copied()
                .ok_or_else(|| IdentityCollision {
                    digest: 0,
                    first_key: Arc::from([]),
                    second_key: Arc::clone(&type_key),
                })?;
            self.interned_types.insert(type_key, id);
            return Ok(id);
        }
        let canonical_key = key(
            IdentityDomain::Type,
            IdentityOrigin::Generated,
            &[&type_key],
        );
        let observation = intern(
            IdentityDomain::Type,
            IdentityOrigin::Generated,
            type_.display(),
            canonical_key.clone(),
            fingerprint(&canonical_key),
            &xxh3_128,
            &mut self.full_keys,
        )?;
        let id = TypeId(observation.digest());
        self.interned_types.insert(type_key, id);
        self.push_record(observation);
        Ok(id)
    }

    pub(crate) fn type_matches(&self, id: TypeId, type_: &Type) -> bool {
        self.interned_types.get(&type_.canonical_key()) == Some(&id)
    }

    pub(crate) fn variant(&self, owner: DefinitionId, name: &str) -> Option<VariantId> {
        self.variants.get(&(owner, name.to_owned())).copied()
    }

    #[allow(dead_code)]
    pub(crate) fn pool_for_definition(&self, definition: DefinitionId) -> Option<PoolId> {
        self.pools.get(&definition).copied()
    }

    pub(crate) fn specialization(
        &mut self,
        definition: DefinitionId,
        argument_types: &[Type],
    ) -> Result<SpecializationId, IdentityCollision> {
        let definition_bytes = definition.0.to_be_bytes();
        let type_keys = argument_types
            .iter()
            .map(Type::canonical_key)
            .collect::<Vec<_>>();
        let mut parts = vec![definition_bytes.as_slice()];
        parts.extend(type_keys.iter().map(AsRef::as_ref));
        let canonical_key = key(
            IdentityDomain::Specialization,
            IdentityOrigin::Generated,
            &parts,
        );
        let observation = intern(
            IdentityDomain::Specialization,
            IdentityOrigin::Generated,
            format!(
                "{:032x}[{}]",
                definition.0,
                argument_types
                    .iter()
                    .map(Type::display)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            canonical_key.clone(),
            fingerprint(&canonical_key),
            &xxh3_128,
            &mut self.full_keys,
        )?;
        let id = SpecializationId(observation.digest());
        self.push_record(observation);
        Ok(id)
    }
}

pub(crate) fn catalog<'a>(
    parsed_sources: &BTreeMap<String, ParsedSource>,
    files: &BTreeMap<&'a str, &'a ProjectFile>,
    authenticated_paths: &BTreeSet<&str>,
    cancellation: &Cancellation,
) -> Result<IdentityCatalog, IdentityFailure> {
    catalog_with_hasher(
        parsed_sources,
        files,
        authenticated_paths,
        xxh3_128,
        cancellation,
    )
}

fn catalog_with_hasher<'a, F>(
    parsed_sources: &BTreeMap<String, ParsedSource>,
    files: &BTreeMap<&'a str, &'a ProjectFile>,
    authenticated_paths: &BTreeSet<&str>,
    hasher: F,
    cancellation: &Cancellation,
) -> Result<IdentityCatalog, IdentityFailure>
where
    F: Fn(&[u8]) -> u128,
{
    let mut records = Vec::new();
    let mut modules = BTreeMap::new();
    let mut definitions = BTreeMap::new();
    let mut associated_functions = BTreeMap::new();
    let mut tests = BTreeMap::new();
    let mut types = BTreeMap::new();
    let mut pools = BTreeMap::new();
    let mut variants = BTreeMap::new();
    let mut digests: BTreeMap<u128, Arc<[u8]>> = BTreeMap::new();
    for (path, parsed) in parsed_sources {
        if cancellation.is_cancelled() {
            return Err(IdentityFailure::Cancelled);
        }
        let module = module_name(path);
        let origin = if authenticated_paths.contains(path.as_str()) {
            IdentityOrigin::Authenticated
        } else {
            IdentityOrigin::Project
        };
        let module_observation = intern(
            IdentityDomain::Module,
            origin,
            module.clone(),
            key(IdentityDomain::Module, origin, &[module.as_bytes()]),
            fingerprint(files[path.as_str()].bytes()),
            &hasher,
            &mut digests,
        )
        .map_err(IdentityFailure::Collision)?;
        let module_id = ModuleId(module_observation.digest());
        modules.insert(path.clone(), module_id);
        records.push(module_observation);

        for declaration in &parsed.declarations {
            if cancellation.is_cancelled() {
                return Err(IdentityFailure::Cancelled);
            }
            if !declaration.structurally_valid {
                continue;
            }
            let declaration_key = key(
                IdentityDomain::Definition,
                origin,
                &[
                    &module_id.0.to_be_bytes(),
                    declaration.kind.name().as_bytes(),
                    declaration.name.as_bytes(),
                ],
            );
            let bytes = parsed.declaration_bytes(files[path.as_str()], declaration);
            let definition_observation = intern(
                IdentityDomain::Definition,
                origin,
                declaration.name.clone(),
                Arc::clone(&declaration_key),
                fingerprint(bytes),
                &hasher,
                &mut digests,
            )
            .map_err(IdentityFailure::Collision)?;
            let definition_id = DefinitionId(definition_observation.digest());
            definitions.insert(
                (path.clone(), declaration.kind, declaration.name.clone()),
                definition_id,
            );
            records.push(definition_observation);
            records.push(
                intern(
                    IdentityDomain::SourceSite,
                    origin,
                    format!("{}:{}@{}", module, declaration.name, declaration.start),
                    key(
                        IdentityDomain::SourceSite,
                        origin,
                        &[
                            &definition_id.0.to_be_bytes(),
                            &declaration.start.to_be_bytes(),
                            &declaration.end.to_be_bytes(),
                        ],
                    ),
                    fingerprint(bytes),
                    &hasher,
                    &mut digests,
                )
                .map_err(IdentityFailure::Collision)?,
            );
            let extra_domain = match declaration.kind {
                DeclarationKind::Struct
                | DeclarationKind::ResourceStruct
                | DeclarationKind::Enum
                | DeclarationKind::Interface => Some(IdentityDomain::Type),
                DeclarationKind::Pool => Some(IdentityDomain::Pool),
                _ => None,
            };
            if let Some(domain) = extra_domain {
                let observation = intern(
                    domain,
                    origin,
                    declaration.name.clone(),
                    key(domain, origin, &[&definition_id.0.to_be_bytes()]),
                    fingerprint(bytes),
                    &hasher,
                    &mut digests,
                )
                .map_err(IdentityFailure::Collision)?;
                match domain {
                    IdentityDomain::Type => {
                        types.insert(definition_id, TypeId(observation.digest()));
                    }
                    IdentityDomain::Pool => {
                        pools.insert(definition_id, PoolId(observation.digest()));
                    }
                    _ => unreachable!("declaration extra identity domain is closed"),
                }
                records.push(observation);
            }
            if let Some(DeclarationSyntax::Suite(suite)) = &declaration.syntax {
                for test in &suite.tests {
                    let test_bytes = usize::try_from(test.range.start())
                        .ok()
                        .zip(usize::try_from(test.range.end()).ok())
                        .and_then(|(start, end)| files[path.as_str()].bytes().get(start..end))
                        .unwrap_or_default();
                    let nested_definition = intern(
                        IdentityDomain::Definition,
                        origin,
                        test.name.clone(),
                        key(
                            IdentityDomain::Definition,
                            origin,
                            &[
                                &definition_id.0.to_be_bytes(),
                                b"test",
                                test.name.as_bytes(),
                            ],
                        ),
                        fingerprint(test_bytes),
                        &hasher,
                        &mut digests,
                    )
                    .map_err(IdentityFailure::Collision)?;
                    let test_definition_id = DefinitionId(nested_definition.digest());
                    records.push(nested_definition);
                    let observation = intern(
                        IdentityDomain::Test,
                        origin,
                        format!("{}.{}", declaration.name, test.name),
                        key(
                            IdentityDomain::Test,
                            origin,
                            &[
                                &definition_id.0.to_be_bytes(),
                                &test_definition_id.0.to_be_bytes(),
                            ],
                        ),
                        fingerprint(test_bytes),
                        &hasher,
                        &mut digests,
                    )
                    .map_err(IdentityFailure::Collision)?;
                    tests.insert(
                        (path.clone(), declaration.name.clone(), test.name.clone()),
                        TestId {
                            suite: definition_id,
                            test: test_definition_id,
                            identity: observation.digest(),
                        },
                    );
                    records.push(observation);
                }
            }
            if let Some(DeclarationSyntax::Enum(enum_)) = &declaration.syntax {
                for variant in &enum_.variants {
                    let variant_bytes = usize::try_from(variant.range.start())
                        .ok()
                        .zip(usize::try_from(variant.range.end()).ok())
                        .and_then(|(start, end)| files[path.as_str()].bytes().get(start..end))
                        .unwrap_or_default();
                    let nested_definition = intern(
                        IdentityDomain::Definition,
                        origin,
                        variant.name.clone(),
                        key(
                            IdentityDomain::Definition,
                            origin,
                            &[
                                &definition_id.0.to_be_bytes(),
                                b"variant",
                                variant.name.as_bytes(),
                            ],
                        ),
                        fingerprint(variant_bytes),
                        &hasher,
                        &mut digests,
                    )
                    .map_err(IdentityFailure::Collision)?;
                    let variant_definition = DefinitionId(nested_definition.digest());
                    records.push(nested_definition);
                    let observation = intern(
                        IdentityDomain::Generated,
                        origin,
                        format!("{}.{}", declaration.name, variant.name),
                        key(
                            IdentityDomain::Generated,
                            origin,
                            &[&variant_definition.0.to_be_bytes()],
                        ),
                        fingerprint(variant_bytes),
                        &hasher,
                        &mut digests,
                    )
                    .map_err(IdentityFailure::Collision)?;
                    variants.insert(
                        (definition_id, variant.name.clone()),
                        VariantId {
                            owner: definition_id,
                            definition: variant_definition,
                            variant: observation.digest(),
                        },
                    );
                    records.push(observation);
                }
            }
            let member_functions: &[crate::syntax::MemberFunctionSyntax] = match &declaration.syntax
            {
                Some(
                    DeclarationSyntax::Struct(struct_) | DeclarationSyntax::ResourceStruct(struct_),
                ) => &struct_.functions,
                Some(DeclarationSyntax::Enum(enum_)) => &enum_.functions,
                _ => &[],
            };
            for member in member_functions {
                let canonical_key = key(
                    IdentityDomain::Definition,
                    origin,
                    &[
                        &definition_id.0.to_be_bytes(),
                        b"associated_function",
                        member.name.as_bytes(),
                    ],
                );
                let start = usize::try_from(member.range.start()).ok();
                let end = usize::try_from(member.range.end()).ok();
                let member_bytes = start
                    .zip(end)
                    .and_then(|(start, end)| files[path.as_str()].bytes().get(start..end))
                    .unwrap_or_default();
                let observation = intern(
                    IdentityDomain::Definition,
                    origin,
                    format!("{}.{}", declaration.name, member.name),
                    canonical_key,
                    fingerprint(member_bytes),
                    &hasher,
                    &mut digests,
                )
                .map_err(IdentityFailure::Collision)?;
                associated_functions.insert(
                    (definition_id, member.name.clone()),
                    DefinitionId(observation.digest()),
                );
                records.push(observation);
            }
        }
    }
    records.sort_by(|left, right| left.canonical_key.cmp(&right.canonical_key));
    records.dedup_by(|left, right| left.canonical_key == right.canonical_key);
    Ok(IdentityCatalog {
        observation_keys: records
            .iter()
            .map(|record| Arc::clone(&record.canonical_key))
            .collect(),
        records,
        specialization_records: BTreeMap::new(),
        modules,
        definitions,
        associated_functions,
        tests,
        types,
        pools,
        interned_types: BTreeMap::new(),
        variants,
        full_keys: digests,
    })
}

fn key(domain: IdentityDomain, origin: IdentityOrigin, parts: &[&[u8]]) -> Arc<[u8]> {
    let mut bytes = b"wrela.identity\0\x01".to_vec();
    push_part(&mut bytes, domain.tag());
    push_part(&mut bytes, origin.tag());
    for part in parts {
        push_part(&mut bytes, part);
    }
    bytes.into()
}

fn push_part(target: &mut Vec<u8>, part: &[u8]) {
    target.extend_from_slice(&u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes());
    target.extend_from_slice(part);
}

fn intern<F>(
    domain: IdentityDomain,
    origin: IdentityOrigin,
    name: String,
    canonical_key: Arc<[u8]>,
    fingerprint: u128,
    hasher: &F,
    digests: &mut BTreeMap<u128, Arc<[u8]>>,
) -> Result<IdentityRecord, IdentityCollision>
where
    F: Fn(&[u8]) -> u128,
{
    let digest = hasher(&canonical_key);
    if let Some(existing) = digests.get(&digest)
        && existing.as_ref() != canonical_key.as_ref()
    {
        return Err(IdentityCollision {
            digest,
            first_key: Arc::clone(existing),
            second_key: canonical_key,
        });
    }
    digests.insert(digest, Arc::clone(&canonical_key));
    Ok(IdentityRecord {
        observation: IdentityObservation::new(domain, origin, name, digest, fingerprint),
        canonical_key,
    })
}

fn fingerprint(bytes: &[u8]) -> u128 {
    let mut hasher = Xxh3::new();
    hasher.update(b"wrela.fingerprint\0\x01");
    hash_part(&mut hasher, bytes);
    hasher.digest128()
}

fn hash_part(hasher: &mut Xxh3, part: &[u8]) {
    hasher.update(&u64::try_from(part.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(part);
}

fn module_name(path: &str) -> String {
    path.strip_prefix("src/")
        .and_then(|path| path.strip_suffix(".wr"))
        .unwrap_or(path)
        .replace('/', ".")
}

impl IdentityDomain {
    const fn tag(self) -> &'static [u8] {
        match self {
            Self::Module => b"module",
            Self::Definition => b"definition",
            Self::Type => b"type",
            Self::Pool => b"pool",
            Self::Specialization => b"specialization",
            Self::Generated => b"generated",
            Self::SourceSite => b"source_site",
            Self::Test => b"test",
            Self::Construction => b"construction",
        }
    }
}

impl IdentityOrigin {
    const fn tag(self) -> &'static [u8] {
        match self {
            Self::Project => b"project",
            Self::Authenticated => b"authenticated",
            Self::Generated => b"generated",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::{Cancellation, ProjectFile};
    use crate::syntax;

    #[test]
    fn distinct_full_keys_reject_a_forced_digest_collision() {
        let first = ProjectFile::new("src/image.wr", b"fn one():\n    pass\n");
        let second = ProjectFile::new("src/game/two.wr", b"fn two():\n    pass\n");
        let mut files = BTreeMap::new();
        files.insert(first.path(), &first);
        files.insert(second.path(), &second);
        let cancellation = Cancellation::new();
        let mut parsed = BTreeMap::new();
        parsed.insert(
            first.path().to_owned(),
            syntax::parse(&first, &cancellation),
        );
        parsed.insert(
            second.path().to_owned(),
            syntax::parse(&second, &cancellation),
        );

        let collision =
            catalog_with_hasher(&parsed, &files, &BTreeSet::new(), |_| 7, &cancellation)
                .expect_err("must collide");
        let IdentityFailure::Collision(collision) = collision else {
            panic!("expected collision");
        };
        assert_eq!(collision.digest, 7);
        assert_ne!(collision.first_key, collision.second_key);
    }

    #[test]
    fn length_prefixes_make_delimiter_bytes_unambiguous() {
        assert_ne!(
            key(
                IdentityDomain::Definition,
                IdentityOrigin::Project,
                &[b"a|b", b"c"]
            ),
            key(
                IdentityDomain::Definition,
                IdentityOrigin::Project,
                &[b"a", b"b|c"]
            ),
        );
    }
}
