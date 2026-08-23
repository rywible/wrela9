use std::collections::BTreeMap;
use std::sync::Arc;

use xxhash_rust::xxh3::xxh3_128;

use crate::evaluator::Function;

#[derive(Clone, Debug)]
pub(crate) struct VerifiedProgram {
    functions: BTreeMap<String, Function>,
    fingerprint: u128,
    _verified: Verified,
}

#[derive(Clone, Debug)]
struct Verified;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VerificationDefect {
    pub(crate) evidence: Arc<str>,
}

impl VerifiedProgram {
    pub(crate) fn functions(&self) -> &BTreeMap<String, Function> {
        &self.functions
    }

    pub(crate) const fn fingerprint(&self) -> u128 {
        self.fingerprint
    }
}

pub(crate) fn verify(
    functions: BTreeMap<String, Function>,
) -> Result<VerifiedProgram, VerificationDefect> {
    let mut canonical = b"wrela.typed-hir.v1\0".to_vec();
    for (lookup_name, function) in &functions {
        if lookup_name.is_empty() || function.name.is_empty() {
            return Err(defect("empty resolved function name"));
        }
        if function.source.start() > function.source.end() {
            return Err(defect("reversed function provenance"));
        }
        let mut parameters = std::collections::BTreeSet::new();
        for (name, type_name) in &function.parameters {
            if name.is_empty() || type_name.is_empty() {
                return Err(defect("unresolved parameter in concrete function"));
            }
            if !parameters.insert(name) {
                return Err(defect("duplicate parameter in concrete function"));
            }
        }
        if function
            .body
            .iter()
            .any(|(offset, _)| u64::try_from(*offset).unwrap_or(u64::MAX) < function.source.start())
        {
            return Err(defect("statement provenance escapes its declaration"));
        }
        canonical.extend_from_slice(lookup_name.as_bytes());
        canonical.push(0);
        canonical.extend_from_slice(function.return_type.as_bytes());
        canonical.push(0xff);
    }
    Ok(VerifiedProgram {
        fingerprint: xxh3_128(&canonical),
        functions,
        _verified: Verified,
    })
}

fn defect(evidence: &'static str) -> VerificationDefect {
    VerificationDefect {
        evidence: Arc::from(evidence),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourceRange;

    #[test]
    fn malformed_compiler_artifact_is_contained_as_a_verification_defect() {
        let malformed = Function {
            name: String::new(),
            public: false,
            parameters: Vec::new(),
            return_type: "i64".to_owned(),
            body: vec![(0, "return 1".to_owned())],
            source: SourceRange::new("src/image.wr", 0, 1),
        };
        let defect = verify(BTreeMap::from([(String::new(), malformed)]))
            .expect_err("malformed artifact must not receive verified marker");
        assert_eq!(defect.evidence.as_ref(), "empty resolved function name");
    }
}
