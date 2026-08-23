use std::collections::BTreeMap;
use std::sync::Arc;

use xxhash_rust::xxh3::xxh3_128;

use crate::compiler::{IdentityDomain, IdentityObservation, ProjectFile};
use crate::syntax::ParsedSource;

#[derive(Clone, Debug)]
pub(crate) struct IdentityCollision {
    pub(crate) digest: u128,
    pub(crate) first_key: Arc<str>,
    pub(crate) second_key: Arc<str>,
}

pub(crate) fn catalog<'a>(
    parsed_sources: &BTreeMap<String, ParsedSource>,
    files: &BTreeMap<&'a str, &'a ProjectFile>,
) -> Result<Vec<IdentityObservation>, IdentityCollision> {
    catalog_with_hasher(parsed_sources, files, xxh3_128)
}

fn catalog_with_hasher<'a, F>(
    parsed_sources: &BTreeMap<String, ParsedSource>,
    files: &BTreeMap<&'a str, &'a ProjectFile>,
    hasher: F,
) -> Result<Vec<IdentityObservation>, IdentityCollision>
where
    F: Fn(&[u8]) -> u128,
{
    let mut observations = Vec::new();
    let mut digests: BTreeMap<u128, Arc<str>> = BTreeMap::new();
    for (path, parsed) in parsed_sources {
        let module = module_name(path);
        let module_key: Arc<str> = format!("wrela.identity.v1|module|project|{module}").into();
        observations.push(intern(
            IdentityDomain::Module,
            module.clone(),
            Arc::clone(&module_key),
            fingerprint(files[path.as_str()].bytes()),
            &hasher,
            &mut digests,
        )?);

        for declaration in &parsed.declarations {
            let key: Arc<str> = format!(
                "wrela.identity.v1|definition|project|{module}|{}|{}",
                declaration.kind, declaration.name
            )
            .into();
            let bytes = &files[path.as_str()].bytes()[declaration.start..declaration.end];
            observations.push(intern(
                IdentityDomain::Definition,
                declaration.name.clone(),
                Arc::clone(&key),
                fingerprint(bytes),
                &hasher,
                &mut digests,
            )?);
            if matches!(
                declaration.kind,
                "struct" | "resource_struct" | "enum" | "interface" | "type_alias"
            ) {
                let type_key: Arc<str> = format!(
                    "wrela.identity.v1|type|project|{module}|{}|{}",
                    declaration.kind, declaration.name
                )
                .into();
                observations.push(intern(
                    IdentityDomain::Type,
                    declaration.name.clone(),
                    type_key,
                    fingerprint(bytes),
                    &hasher,
                    &mut digests,
                )?);
            } else if declaration.kind == "pool" {
                let pool_key: Arc<str> = format!(
                    "wrela.identity.v1|pool|project|{module}|{}",
                    declaration.name
                )
                .into();
                observations.push(intern(
                    IdentityDomain::Pool,
                    declaration.name.clone(),
                    pool_key,
                    fingerprint(bytes),
                    &hasher,
                    &mut digests,
                )?);
            } else if declaration.kind == "function" {
                let specialization_key: Arc<str> =
                    format!("wrela.identity.v1|specialization|{key}|[]").into();
                observations.push(intern(
                    IdentityDomain::Specialization,
                    format!("{}[]", declaration.name),
                    specialization_key,
                    fingerprint(bytes),
                    &hasher,
                    &mut digests,
                )?);
            }
        }
    }
    observations.sort_by(|left, right| left.canonical_key().cmp(right.canonical_key()));
    Ok(observations)
}

fn intern<F>(
    domain: IdentityDomain,
    name: String,
    canonical_key: Arc<str>,
    fingerprint: u128,
    hasher: &F,
    digests: &mut BTreeMap<u128, Arc<str>>,
) -> Result<IdentityObservation, IdentityCollision>
where
    F: Fn(&[u8]) -> u128,
{
    let digest = hasher(canonical_key.as_bytes());
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
        name,
        canonical_key,
        digest,
        fingerprint,
    ))
}

fn fingerprint(bytes: &[u8]) -> u128 {
    let mut canonical = b"wrela.fingerprint.v1\0".to_vec();
    canonical.extend_from_slice(bytes);
    xxh3_128(&canonical)
}

fn module_name(path: &str) -> String {
    path.strip_prefix("src/")
        .and_then(|path| path.strip_suffix(".wr"))
        .unwrap_or(path)
        .replace('/', ".")
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

        let collision = catalog_with_hasher(&parsed, &files, |_| 7).expect_err("must collide");
        assert_eq!(collision.digest, 7);
        assert_ne!(collision.first_key, collision.second_key);
    }
}
