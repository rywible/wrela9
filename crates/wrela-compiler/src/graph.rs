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

pub(crate) fn reachable_from<N>(root: N, graph: &BTreeMap<N, BTreeSet<N>>) -> BTreeSet<N>
where
    N: Copy + Ord,
{
    let mut reachable = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(current) = pending.pop() {
        if !reachable.insert(current) {
            continue;
        }
        if let Some(edges) = graph.get(&current) {
            pending.extend(edges.iter().rev().copied());
        }
    }
    reachable
}

pub(crate) fn propagate_monotone<N, V>(
    graph: &BTreeMap<N, BTreeSet<N>>,
    base: &BTreeMap<N, V>,
    mut merge: impl FnMut(&mut V, &V) -> bool,
    mut cancelled: impl FnMut() -> bool,
) -> Option<BTreeMap<N, V>>
where
    N: Copy + Ord,
    V: Clone,
{
    let mut dependents = BTreeMap::<N, BTreeSet<N>>::new();
    for (caller, callees) in graph {
        for callee in callees {
            if base.contains_key(caller) && base.contains_key(callee) {
                dependents.entry(*callee).or_default().insert(*caller);
            }
        }
    }

    let mut facts = base.clone();
    let mut pending = base.keys().copied().collect::<BTreeSet<_>>();
    while let Some(callee) = pending.pop_first() {
        if cancelled() {
            return None;
        }
        let callee_facts = facts.remove(&callee).expect("queued fact exists");
        for caller in dependents.get(&callee).into_iter().flatten() {
            if *caller == callee {
                continue;
            }
            let caller_facts = facts.get_mut(caller).expect("caller fact exists");
            if merge(caller_facts, &callee_facts) {
                pending.insert(*caller);
            }
        }
        facts.insert(callee, callee_facts);
    }
    Some(facts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

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

    #[test]
    fn monotone_facts_propagate_through_cycles_and_callers() {
        let graph = BTreeMap::from([
            (1_u32, BTreeSet::from([2])),
            (2, BTreeSet::from([1, 3])),
            (3, BTreeSet::new()),
        ]);
        let base = BTreeMap::from([
            (1_u32, BTreeSet::from(["one"])),
            (2, BTreeSet::from(["two"])),
            (3, BTreeSet::from(["three"])),
        ]);
        let solved = propagate_monotone(
            &graph,
            &base,
            |caller, callee| {
                let previous = caller.len();
                caller.extend(callee.iter().copied());
                caller.len() != previous
            },
            || false,
        )
        .expect("not cancelled");
        assert_eq!(solved[&1], BTreeSet::from(["one", "three", "two"]));
        assert_eq!(solved[&2], BTreeSet::from(["one", "three", "two"]));
        assert_eq!(solved[&3], BTreeSet::from(["three"]));
    }

    #[test]
    fn propagation_does_not_clone_each_queued_fact() {
        #[derive(Debug)]
        struct CountingFacts {
            values: BTreeSet<u32>,
            clone_count: Arc<AtomicUsize>,
        }

        impl Clone for CountingFacts {
            fn clone(&self) -> Self {
                self.clone_count.fetch_add(1, Ordering::Relaxed);
                Self {
                    values: self.values.clone(),
                    clone_count: Arc::clone(&self.clone_count),
                }
            }
        }

        let clone_count = Arc::new(AtomicUsize::new(0));
        let graph = BTreeMap::from([
            (1_u32, BTreeSet::from([2])),
            (2, BTreeSet::from([3])),
            (3, BTreeSet::new()),
        ]);
        let base = (1..=3)
            .map(|value| {
                (
                    value,
                    CountingFacts {
                        values: BTreeSet::from([value]),
                        clone_count: Arc::clone(&clone_count),
                    },
                )
            })
            .collect();
        let solved = propagate_monotone(
            &graph,
            &base,
            |caller, callee| {
                let previous = caller.values.len();
                caller.values.extend(callee.values.iter().copied());
                caller.values.len() != previous
            },
            || false,
        )
        .expect("not cancelled");
        assert_eq!(solved[&1].values, BTreeSet::from([1, 2, 3]));
        assert_eq!(clone_count.load(Ordering::Relaxed), base.len());
    }
}
