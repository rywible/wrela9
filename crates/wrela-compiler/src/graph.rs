#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn strongly_connected_components<N>(graph: &BTreeMap<N, BTreeSet<N>>) -> Vec<Vec<N>>
where
    N: Copy + Ord,
{
    let mut nodes = graph.keys().copied().collect::<BTreeSet<_>>();
    nodes.extend(graph.values().flat_map(|edges| edges.iter().copied()));

    let mut visited = BTreeSet::new();
    let mut finish_order = Vec::with_capacity(nodes.len());
    for root in nodes.iter().copied() {
        if visited.contains(&root) {
            continue;
        }
        let mut stack = vec![(root, false)];
        while let Some((node, expanded)) = stack.pop() {
            if expanded {
                finish_order.push(node);
                continue;
            }
            if !visited.insert(node) {
                continue;
            }
            stack.push((node, true));
            if let Some(edges) = graph.get(&node) {
                stack.extend(edges.iter().rev().copied().map(|edge| (edge, false)));
            }
        }
    }

    let mut reverse = nodes
        .iter()
        .copied()
        .map(|node| (node, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for (source, edges) in graph {
        for target in edges {
            reverse.entry(*target).or_default().insert(*source);
        }
    }

    visited.clear();
    let mut components = Vec::new();
    for root in finish_order.into_iter().rev() {
        if !visited.insert(root) {
            continue;
        }
        let mut component = Vec::new();
        let mut pending = vec![root];
        while let Some(node) = pending.pop() {
            component.push(node);
            if let Some(edges) = reverse.get(&node) {
                for edge in edges.iter().rev().copied() {
                    if visited.insert(edge) {
                        pending.push(edge);
                    }
                }
            }
        }
        component.sort();
        components.push(component);
    }
    components.sort_by_key(|component| component.first().copied());
    components
}

pub(crate) fn recursive_nodes<N>(graph: &BTreeMap<N, BTreeSet<N>>) -> BTreeSet<N>
where
    N: Copy + Ord,
{
    strongly_connected_components(graph)
        .into_iter()
        .filter(|component| {
            component.len() > 1
                || component
                    .first()
                    .is_some_and(|node| graph.get(node).is_some_and(|edges| edges.contains(node)))
        })
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iterative_scc_finds_cycles_without_host_recursion() {
        let graph = BTreeMap::from([
            (1_u32, BTreeSet::from([2])),
            (2, BTreeSet::from([1, 3])),
            (3, BTreeSet::new()),
            (4, BTreeSet::from([4])),
        ]);
        assert_eq!(recursive_nodes(&graph), BTreeSet::from([1, 2, 4]));
    }
}
