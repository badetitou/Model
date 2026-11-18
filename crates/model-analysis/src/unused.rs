//! Entities isolated from the dependency graph (candidate dead code).

use model_core::{EntityId, EntityKind, Model, RelationKind};

use crate::Analysis;

/// What counts as a use, and which entities are candidates.
#[derive(Debug, Clone)]
pub struct UnusedOptions {
    /// Relation kinds connecting entities. Defaults to `Calls`, `References`
    /// and `Implements`: a type only used through an interface is not dead.
    pub kinds: Vec<RelationKind>,
    /// External entities model other projects: never report them.
    pub exclude_external: bool,
    /// Require a source file: namespaces, files and implicit entities are
    /// not candidate dead code.
    pub require_source: bool,
}

impl Default for UnusedOptions {
    fn default() -> Self {
        Self {
            kinds: vec![
                RelationKind::Calls,
                RelationKind::References,
                RelationKind::Implements,
            ],
            exclude_external: true,
            require_source: true,
        }
    }
}

/// Finds entities with neither incoming nor outgoing uses.
#[derive(Debug, Clone, Default)]
pub struct Unused {
    pub options: UnusedOptions,
}

impl Analysis for Unused {
    type Output = Vec<EntityId>;

    fn name(&self) -> &str {
        "unused"
    }

    fn run(&self, model: &Model) -> anyhow::Result<Vec<EntityId>> {
        let mut connected = vec![false; model.entity_count()];
        for rel in model.relations() {
            if !self.options.kinds.contains(rel.kind()) {
                continue;
            }
            // A type whose methods are used is not dead code: propagate the
            // connection to its enclosing entities.
            for e in [rel.from(), rel.to()] {
                if let Some(slot) = connected.get_mut(e.id().index()) {
                    *slot = true;
                }
                for parent in e.ancestors() {
                    if let Some(slot) = connected.get_mut(parent.id().index()) {
                        *slot = true;
                    }
                }
            }
        }
        let mut out = Vec::new();
        for e in model.entities() {
            if self.options.exclude_external && e.external() {
                continue;
            }
            if self.options.require_source && e.file().is_none() {
                continue;
            }
            if matches!(e.kind(), EntityKind::Namespace | EntityKind::File) {
                continue;
            }
            if !connected[e.id().index()] {
                out.push(e.id());
            }
        }
        out.sort_unstable();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{id, sample};

    #[test]
    fn finds_the_isolated_class() {
        let m = sample();
        let out = Unused::default().run(&m).unwrap();
        // A, B, I and their methods take part in a call or an implements
        // relation; C stands alone.
        assert_eq!(out, vec![id(3)]);
    }
}
