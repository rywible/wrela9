#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use xxhash_rust::xxh3::Xxh3;

use crate::architecture_planning::{ArchitecturePlanningModule, ContractContext};
use crate::compiler::{CompilerInstallation, OpenError, ProjectFile, Root};
use crate::identity::{self, IdentityCatalog, IdentityFailure};
use crate::image_planning::ImagePlanningModule;
use crate::model::BuildKind;
use crate::semantic;
use crate::syntax::{DeclarationKind, DeclarationSyntax, FunctionModifier, TypeSyntax};
use crate::typed_hir::{AuthorityContext, BuildAuthority, PoolAuthority};
use crate::{Cancellation, syntax};

const DISTRIBUTION_VERSION: &str = "wrela9-layer2-architecture-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ModuleRole {
    Standard,
    Runtime,
    Driver,
    Authenticated,
}

impl ModuleRole {
    const fn tag(self) -> u8 {
        match self {
            Self::Standard => 1,
            Self::Runtime => 2,
            Self::Driver => 3,
            Self::Authenticated => 4,
        }
    }
}

#[derive(Clone, Debug)]
struct SealedModule {
    source: ProjectFile,
    role: ModuleRole,
    content_digest: u128,
}

#[derive(Clone, Debug)]
pub(crate) struct CompilerDistribution {
    modules: Arc<[SealedModule]>,
    build_authority: BuildAuthority,
    pool_authority: PoolAuthority,
    architecture_planning: ArchitecturePlanningModule,
    image_planning: ImagePlanningModule,
    digest: u128,
}

impl CompilerDistribution {
    pub(crate) fn seal(installation: CompilerInstallation) -> Result<Self, OpenError> {
        let mut paths = BTreeSet::new();
        let mut parsed = BTreeMap::new();
        let cancellation = Cancellation::new();
        for module in &*installation.authenticated_modules {
            if !crate::project_closure::valid_module_path(
                module.path(),
                crate::project_closure::ModuleOrigin::Authenticated,
            ) {
                return Err(OpenError::InvalidAuthenticatedModulePath {
                    path: module.path_arc().clone(),
                });
            }
            if !paths.insert(module.path_arc().clone()) {
                return Err(OpenError::DuplicateAuthenticatedModule {
                    path: module.path_arc().clone(),
                });
            }
            let source = syntax::parse(module, &cancellation);
            if let Some(diagnostic) = source.diagnostics.first() {
                return Err(OpenError::MalformedAuthenticatedModule {
                    path: module.path_arc().clone(),
                    code: Arc::from(diagnostic.code()),
                });
            }
            parsed.insert(module.path().to_owned(), source);
        }
        for (path, source) in &parsed {
            for import in &source.imports {
                if !paths.contains(import.target_path.as_str()) {
                    return Err(OpenError::MissingAuthenticatedDependency {
                        path: Arc::from(path.as_str()),
                        dependency: import.target_path.clone().into(),
                    });
                }
            }
        }
        if let Some(cycle) = crate::project_closure::first_import_cycle(&parsed) {
            return Err(OpenError::AuthenticatedImportCycle { path: cycle.path });
        }

        let files = installation
            .authenticated_modules
            .iter()
            .map(|module| (module.path(), module))
            .collect::<BTreeMap<_, _>>();
        let authenticated_paths = files.keys().copied().collect::<BTreeSet<_>>();
        let mut identities =
            match identity::catalog(&parsed, &files, &authenticated_paths, &cancellation) {
                Ok(identities) => identities,
                Err(IdentityFailure::Collision(collision)) => {
                    return Err(OpenError::AuthenticatedIdentityCollision {
                        digest: collision.digest,
                    });
                }
                Err(IdentityFailure::Cancelled) => {
                    unreachable!("fresh cancellation is not cancelled")
                }
            };
        let build_authority = authenticated_build_authority(&parsed, &identities);
        let pool_authority = authenticated_pool_authority(&parsed, &identities);
        let revision = semantic::analyze(
            &parsed,
            &files,
            &mut identities,
            Root::Image,
            &cancellation,
            false,
            semantic::AnalysisContext::new(
                AuthorityContext::new(&build_authority, &pool_authority),
                None,
            ),
        );
        let finalized = match revision.finalize(false, false, false, false) {
            Ok(revision) => revision,
            Err(semantic::SemanticFailure::Defect(defect)) => {
                return Err(OpenError::AuthenticatedModuleDefect {
                    phase: Arc::from(defect.phase()),
                    evidence: Arc::from(defect.evidence()),
                });
            }
            Err(semantic::SemanticFailure::Cancelled) => {
                unreachable!("fresh cancellation is not cancelled")
            }
        };
        if let Some(diagnostic) = finalized.diagnostics.first() {
            return Err(OpenError::InvalidAuthenticatedModule {
                path: Arc::from(diagnostic.primary().path()),
                code: Arc::from(diagnostic.code()),
            });
        }

        let mut modules = installation
            .authenticated_modules
            .iter()
            .cloned()
            .map(|source| SealedModule {
                role: role_for(source.path()),
                content_digest: content_digest(&source),
                source,
            })
            .collect::<Vec<_>>();
        modules.sort_by(|left, right| left.source.path().cmp(right.source.path()));
        let digest = distribution_digest(&modules, &build_authority, pool_authority);
        Ok(Self {
            modules: modules.into(),
            build_authority,
            pool_authority,
            architecture_planning: ArchitecturePlanningModule::new(ContractContext::new(
                DISTRIBUTION_VERSION,
                digest,
            )),
            image_planning: ImagePlanningModule,
            digest,
        })
    }

    pub(crate) fn modules(&self) -> impl Iterator<Item = &ProjectFile> {
        self.modules.iter().map(|module| &module.source)
    }

    pub(crate) const fn build_authority(&self) -> &BuildAuthority {
        &self.build_authority
    }

    pub(crate) const fn pool_authority(&self) -> &PoolAuthority {
        &self.pool_authority
    }

    pub(crate) const fn architecture_planning(&self) -> &ArchitecturePlanningModule {
        &self.architecture_planning
    }

    pub(crate) const fn image_planning(&self) -> &ImagePlanningModule {
        &self.image_planning
    }

    pub(crate) const fn digest(&self) -> u128 {
        self.digest
    }

    pub(crate) const fn version(&self) -> &'static str {
        DISTRIBUTION_VERSION
    }
}

fn authenticated_pool_authority(
    parsed: &BTreeMap<String, syntax::ParsedSource>,
    identities: &IdentityCatalog,
) -> PoolAuthority {
    let scoped_factory = parsed
        .get("src/core/pool.wr")
        .and_then(|source| {
            source.declarations.iter().find(|declaration| {
                declaration.public
                    && declaration.kind == DeclarationKind::Function
                    && declaration.name == "scoped"
                    && matches!(
                        declaration.syntax.as_ref(),
                        Some(DeclarationSyntax::Function(function))
                            if function.modifier == FunctionModifier::Pure
                                && function.type_parameters.is_empty()
                                && function.parameters.len() == 1
                                && function.parameters[0].name == "capacity"
                                && matches!(
                                    &function.parameters[0].type_syntax,
                                    TypeSyntax::Named(name)
                                        if name.segments.as_slice() == ["u64"]
                                )
                                && matches!(
                                    &function.return_type,
                                    TypeSyntax::Named(name)
                                        if name.segments.as_slice() == ["Scope"]
                                )
                    )
            })
        })
        .and_then(|_| {
            identities.definition("src/core/pool.wr", DeclarationKind::Function, "scoped")
        });
    PoolAuthority::from_authenticated_scoped_factory(scoped_factory)
}

fn authenticated_build_authority(
    parsed: &BTreeMap<String, syntax::ParsedSource>,
    identities: &IdentityCatalog,
) -> BuildAuthority {
    let mut grants = Vec::new();
    for (path, source) in parsed {
        for declaration in &source.declarations {
            if !declaration.public {
                continue;
            }
            let struct_ = match declaration.syntax.as_ref() {
                Some(DeclarationSyntax::Struct(struct_))
                | Some(DeclarationSyntax::ResourceStruct(struct_)) => struct_,
                _ => continue,
            };
            let Some(owner) = identities.definition(path, declaration.kind, &declaration.name)
            else {
                continue;
            };
            collect_build_constructor(&mut grants, identities, owner, declaration, struct_);
        }
    }
    BuildAuthority::from_authenticated_declarations(grants)
}

fn collect_build_constructor(
    grants: &mut Vec<(Arc<[String]>, BuildKind, crate::model::DefinitionId)>,
    identities: &IdentityCatalog,
    owner: crate::model::DefinitionId,
    declaration: &syntax::Declaration,
    struct_: &syntax::StructSyntax,
) {
    if !struct_.type_parameters.is_empty() {
        return;
    }
    let kind = match declaration.name.as_str() {
        "Image" => BuildKind::Image,
        "Test" => BuildKind::Test,
        _ => {
            let Some(type_identity) = identities.type_for_definition(owner) else {
                return;
            };
            BuildKind::Node {
                definition: owner,
                type_identity,
            }
        }
    };
    let Some(member) = struct_.functions.iter().find(|member| {
        member.name == "new"
            && member.public
            && member.function.modifier == FunctionModifier::Pure
            && member.function.type_parameters.is_empty()
            && member
                .function
                .parameters
                .iter()
                .all(|parameter| parameter.name != "self")
            && build_signature_matches(
                kind,
                &declaration.name,
                &member.function.parameters,
                &member.function.return_type,
            )
    }) else {
        return;
    };
    let Some(definition) = identities.associated_function(owner, &member.name) else {
        return;
    };
    grants.push((
        Arc::from([declaration.name.clone(), member.name.clone()]),
        kind,
        definition,
    ));
}

fn build_signature_matches(
    kind: BuildKind,
    owner_name: &str,
    parameters: &[syntax::ParameterSyntax],
    return_type: &TypeSyntax,
) -> bool {
    let named = |type_: &TypeSyntax, expected| matches!(type_, TypeSyntax::Named(name) if name.segments.as_slice() == [expected]);
    match kind {
        BuildKind::Image => parameters.is_empty() && named(return_type, "Image"),
        BuildKind::Test => {
            parameters.len() == 1
                && parameters[0].name == "cases"
                && matches!(
                    &parameters[0].type_syntax,
                    TypeSyntax::Array(element) if named(element, "TestApplication")
                )
                && named(return_type, "Test")
        }
        BuildKind::Node { .. } => named(return_type, owner_name),
    }
}

fn role_for(path: &str) -> ModuleRole {
    if path.starts_with("src/core/") || path.starts_with("src/std/") {
        ModuleRole::Standard
    } else if path.starts_with("src/runtime/") {
        ModuleRole::Runtime
    } else if path.starts_with("src/drivers/") {
        ModuleRole::Driver
    } else {
        ModuleRole::Authenticated
    }
}

fn content_digest(source: &ProjectFile) -> u128 {
    let mut hasher = Xxh3::new();
    hasher.update(b"wrela.distribution-module\0\x01");
    hash_part(&mut hasher, source.path().as_bytes());
    hash_part(&mut hasher, source.bytes());
    hasher.digest128()
}

fn distribution_digest(
    modules: &[SealedModule],
    authority: &BuildAuthority,
    pool_authority: PoolAuthority,
) -> u128 {
    let mut hasher = Xxh3::new();
    hasher.update(b"wrela.compiler-distribution\0\x01");
    hash_part(&mut hasher, DISTRIBUTION_VERSION.as_bytes());
    for module in modules {
        hasher.update(&[module.role.tag()]);
        hash_part(&mut hasher, module.source.path().as_bytes());
        hasher.update(&module.content_digest.to_be_bytes());
    }
    for (definition, kind, identity) in authority.canonical_grants() {
        hasher.update(&definition.0.to_be_bytes());
        hasher.update(&[kind.canonical_tag()]);
        if let BuildKind::Node {
            definition,
            type_identity,
        } = kind
        {
            hasher.update(&definition.0.to_be_bytes());
            hasher.update(&type_identity.0.to_be_bytes());
        }
        hasher.update(&identity.to_be_bytes());
    }
    for definition in pool_authority.canonical_grants() {
        hasher.update(b"pool.scoped\0");
        hasher.update(&definition.0.to_be_bytes());
    }
    hasher.digest128()
}

fn hash_part(hasher: &mut Xxh3, bytes: &[u8]) {
    hasher.update(&u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(bytes);
}
