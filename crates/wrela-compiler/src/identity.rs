use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use xxhash_rust::xxh3::xxh3_128;

use crate::compiler::{IdentityDomain, IdentityObservation, IdentityOrigin, ProjectFile};
use crate::syntax::ParsedSource;

#[derive(Clone, Debug)]
pub(crate) struct IdentityCollision {
    pub(crate) digest: u128,
    pub(crate) first_key: Arc<[u8]>,
    pub(crate) second_key: Arc<[u8]>,
}

pub(crate) fn catalog<'a>(
    parsed_sources: &BTreeMap<String, ParsedSource>,
    files: &BTreeMap<&'a str, &'a ProjectFile>,
    authenticated_paths: &BTreeSet<&str>,
) -> Result<Vec<IdentityObservation>, IdentityCollision> {
    catalog_with_hasher(parsed_sources, files, authenticated_paths, xxh3_128)
}

fn catalog_with_hasher<'a, F>(
    parsed_sources: &BTreeMap<String, ParsedSource>,
    files: &BTreeMap<&'a str, &'a ProjectFile>,
    authenticated_paths: &BTreeSet<&str>,
    hasher: F,
) -> Result<Vec<IdentityObservation>, IdentityCollision>
where
    F: Fn(&[u8]) -> u128,
{
    let mut observations = Vec::new();
    let mut digests: BTreeMap<u128, Arc<[u8]>> = BTreeMap::new();
    for (path, parsed) in parsed_sources {
        let module = module_name(path);
        let origin = if authenticated_paths.contains(path.as_str()) {
            IdentityOrigin::Authenticated
        } else {
            IdentityOrigin::Project
        };
        observations.push(intern(
            IdentityDomain::Module,
            origin,
            module.clone(),
            key(IdentityDomain::Module, origin, &[module.as_bytes()]),
            fingerprint(files[path.as_str()].bytes()),
            &hasher,
            &mut digests,
        )?);

        for declaration in &parsed.declarations {
            let declaration_key = key(
                IdentityDomain::Definition,
                origin,
                &[
                    module.as_bytes(),
                    declaration.kind.as_bytes(),
                    declaration.name.as_bytes(),
                ],
            );
            let bytes = parsed.declaration_bytes(files[path.as_str()], declaration);
            observations.push(intern(
                IdentityDomain::Definition,
                origin,
                declaration.name.clone(),
                Arc::clone(&declaration_key),
                fingerprint(bytes),
                &hasher,
                &mut digests,
            )?);
            let extra_domain = match declaration.kind {
                "struct" | "resource_struct" | "enum" | "interface" | "type_alias" => {
                    Some(IdentityDomain::Type)
                }
                "pool" => Some(IdentityDomain::Pool),
                "function" => Some(IdentityDomain::Specialization),
                _ => None,
            };
            if let Some(domain) = extra_domain {
                let name = if domain == IdentityDomain::Specialization {
                    format!("{}[]", declaration.name)
                } else {
                    declaration.name.clone()
                };
                observations.push(intern(
                    domain,
                    origin,
                    name,
                    key(
                        domain,
                        origin,
                        &[
                            module.as_bytes(),
                            declaration.kind.as_bytes(),
                            declaration.name.as_bytes(),
                            b"[]",
                        ],
                    ),
                    fingerprint(bytes),
                    &hasher,
                    &mut digests,
                )?);
            }
        }
    }
    observations.sort_by(|left, right| left.canonical_key_bytes().cmp(right.canonical_key_bytes()));
    Ok(observations)
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
) -> Result<IdentityObservation, IdentityCollision>
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
    Ok(IdentityObservation::new(
        domain,
        origin,
        name,
        canonical_key,
        digest,
        fingerprint,
    ))
}

fn fingerprint(bytes: &[u8]) -> u128 {
    let mut canonical = b"wrela.fingerprint\0\x01".to_vec();
    push_part(&mut canonical, bytes);
    xxh3_128(&canonical)
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

        let collision = catalog_with_hasher(&parsed, &files, &BTreeSet::new(), |_| 7)
            .expect_err("must collide");
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
