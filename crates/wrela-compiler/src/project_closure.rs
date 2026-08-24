//! Project-closure topology shared by project compilation and distribution sealing.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use xxhash_rust::xxh3::Xxh3;

use crate::compiler::{ProjectFile, SourceRange};
use crate::syntax::ParsedSource;

#[derive(Clone, Copy)]
pub(crate) enum ModuleOrigin {
    Project,
    Authenticated,
}

pub(crate) struct ImportCycle {
    pub(crate) path: Arc<str>,
    pub(crate) range: SourceRange,
}

pub(crate) fn valid_module_path(path: &str, origin: ModuleOrigin) -> bool {
    let Some(relative) = path.strip_prefix("src/") else {
        return false;
    };
    let Some(stem) = relative.strip_suffix(".wr") else {
        return false;
    };
    let mut segments = stem.split('/');
    let first = segments.next().unwrap_or_default();
    let rest: Vec<_> = segments.collect();
    if rest.is_empty() {
        match origin {
            ModuleOrigin::Project if matches!(first, "image" | "test") => {}
            ModuleOrigin::Project | ModuleOrigin::Authenticated => return false,
        }
    }
    [first].into_iter().chain(rest).all(valid_path_segment)
}

pub(crate) fn first_import_cycle(
    parsed_sources: &BTreeMap<String, ParsedSource>,
) -> Option<ImportCycle> {
    let paths = parsed_sources.keys().cloned().collect::<Vec<_>>();
    let indexes = paths
        .iter()
        .enumerate()
        .map(|(index, path)| (path.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let graph = parsed_sources
        .iter()
        .map(|(path, parsed)| {
            (
                indexes[path.as_str()],
                parsed
                    .imports
                    .iter()
                    .filter_map(|import| indexes.get(import.target_path.as_str()).copied())
                    .collect(),
            )
        })
        .collect::<BTreeMap<usize, BTreeSet<usize>>>();
    let components = crate::graph::strongly_connected_components(&graph);
    let component_by_path = components
        .iter()
        .enumerate()
        .flat_map(|(index, component)| component.iter().copied().map(move |path| (path, index)))
        .collect::<BTreeMap<_, _>>();

    parsed_sources.iter().find_map(|(path, parsed)| {
        let path_index = indexes[path.as_str()];
        let component = *component_by_path.get(&path_index)?;
        let cyclic = components[component].len() > 1
            || graph
                .get(&path_index)
                .is_some_and(|edges| edges.contains(&path_index));
        let range = cyclic.then(|| {
            parsed.imports.iter().find_map(|import| {
                (indexes
                    .get(import.target_path.as_str())
                    .and_then(|index| component_by_path.get(index))
                    == Some(&component))
                .then(|| import.range.clone())
            })
        })??;
        Some(ImportCycle {
            path: Arc::from(path.as_str()),
            range,
        })
    })
}

pub(crate) fn digest<'a>(
    parsed_sources: &BTreeMap<String, ParsedSource>,
    files: &BTreeMap<&'a str, &'a ProjectFile>,
    authenticated_paths: &BTreeSet<&str>,
    distribution_digest: u128,
) -> u128 {
    let mut hasher = Xxh3::new();
    hasher.update(b"wrela.semantic-closure\0\x01");
    hasher.update(&distribution_digest.to_be_bytes());
    for path in parsed_sources.keys() {
        hasher.update(&[u8::from(authenticated_paths.contains(path.as_str()))]);
        hash_digest_part(&mut hasher, path.as_bytes());
        hash_digest_part(&mut hasher, files[path.as_str()].bytes());
    }
    hasher.digest128()
}

fn hash_digest_part(hasher: &mut Xxh3, bytes: &[u8]) {
    hasher.update(&u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(bytes);
}

fn valid_path_segment(segment: &str) -> bool {
    let mut bytes = segment.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}
