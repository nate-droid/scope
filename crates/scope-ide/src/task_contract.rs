//! Host-owned task intent and acceptance policy. Expectations remain opaque to Scope.

use std::collections::BTreeMap;

pub use scope_language::ExpectationOrigin;
use scope_language::ValidationContract;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum AssurancePolicy {
    Exploratory,
    #[default]
    Guided,
    Strict,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum Acceptance {
    #[default]
    Provisional,
    Accepted {
        consumer: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCriterion {
    pub expectation: serde_json::Value,
    pub origin: ExpectationOrigin,
    pub acceptance: Acceptance,
    pub mandatory: bool,
}

impl TaskCriterion {
    /// Acceptance endorses an expectation without rewriting its authorship.
    pub fn accept(&mut self, consumer: impl Into<String>) {
        self.acceptance = Acceptance::Accepted {
            consumer: consumer.into(),
        };
    }

    pub fn is_accepted(&self) -> bool {
        matches!(self.acceptance, Acceptance::Accepted { .. })
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TaskContract {
    pub original_request: String,
    pub goal: String,
    /// Map keys are unique criterion IDs, also used as validation check IDs.
    pub criteria: BTreeMap<String, TaskCriterion>,
    pub policy: AssurancePolicy,
}

impl TaskContract {
    /// Structured intent for model context; provenance is never collapsed here.
    pub fn context(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": "task_contract",
            "original_request": self.original_request,
            "goal": self.goal,
            "assurance": format!("{:?}", self.policy),
            "criteria": self.criteria.iter().map(|(id, criterion)| {
                serde_json::json!({
                    "id": id,
                    "expectation": criterion.expectation,
                    "origin": format!("{:?}", criterion.origin),
                    "acceptance": format!("{:?}", criterion.acceptance),
                    "mandatory": criterion.mandatory,
                })
            }).collect::<Vec<_>>(),
            "limitations": "Passing specified checks is not universal correctness. Provisional criteria are not accepted requirements."
        })
    }

    /// Host aggregate only: original provenance remains on each task criterion.
    /// Include provisional and optional criteria so unresolved observations survive.
    pub fn validation_contract(&self) -> ValidationContract {
        ValidationContract {
            id: "task-contract".into(),
            origin: ExpectationOrigin::Project("scope.task-contract.host-aggregate".into()),
            checks: self
                .criteria
                .iter()
                .map(|(id, criterion)| (id.clone(), criterion.expectation.clone()))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acceptance_preserves_agent_provenance_and_opaque_expectation() {
        let mut criterion = TaskCriterion {
            expectation: serde_json::json!({"language-specific": [1, null]}),
            origin: ExpectationOrigin::Agent("author".into()),
            acceptance: Acceptance::Provisional,
            mandatory: true,
        };
        criterion.accept("reviewer");
        assert_eq!(criterion.origin, ExpectationOrigin::Agent("author".into()));
        assert_eq!(
            criterion.acceptance,
            Acceptance::Accepted {
                consumer: "reviewer".into()
            }
        );
        let task = TaskContract {
            criteria: BTreeMap::from([("behavior".into(), criterion.clone())]),
            ..Default::default()
        };
        assert_eq!(task.policy, AssurancePolicy::Guided);
        let flattened = task.validation_contract();
        assert!(matches!(flattened.origin, ExpectationOrigin::Project(_)));
        assert_eq!(flattened.checks["behavior"], criterion.expectation);
        assert_eq!(task.criteria["behavior"].origin, criterion.origin);
    }
}
