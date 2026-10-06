//! Reviewed suite membership is data; runner selections are projections of it.
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

const INPUT_INVENTORY_SCHEMA: u64 = 2;
const FROZEN_MEMBERSHIP_STATE: &str = "implemented_frozen";

pub struct NodeInventory {
    document: InventoryDocument,
    sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ObservationInput {
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Deserialize)]
struct InventoryDocument {
    schema: u64,
    state: String,
    entries: Vec<Input>,
}

#[derive(Deserialize)]
struct Input {
    path: PathBuf,
    sha256: String,
    role: Role,
    implementation: Option<Implementation>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Role {
    AssertionCase,
    ObservationCase,
    Module,
    PackageMetadata,
    ChildProcess,
    Documentation,
    InventoryManifest,
}

impl Role {
    fn is_case(self) -> bool {
        matches!(self, Self::AssertionCase | Self::ObservationCase)
    }
}

#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Implementation {
    Included {
        owners: Vec<PathBuf>,
        obligations: Vec<String>,
    },
    Excluded {
        missing_capabilities: Vec<String>,
    },
}

impl NodeInventory {
    /// Validate every input before selecting cases, including support-file hashes.
    pub fn read(path: &Path, repository: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let inventory: InventoryDocument = serde_json::from_slice(&bytes)
            .map_err(|error| format!("parse {}: {error}", path.display()))?;
        if inventory.schema != INPUT_INVENTORY_SCHEMA || inventory.entries.is_empty() {
            return Err("unsupported or empty Node input inventory".into());
        }
        let mut paths = BTreeSet::new();
        for input in &inventory.entries {
            if !paths.insert(&input.path) {
                return Err(format!("duplicate Node input: {}", input.path.display()));
            }
            let path = repository_path(repository, &input.path)?;
            let bytes =
                fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
            if format!("{:x}", Sha256::digest(bytes)) != input.sha256 {
                return Err(format!("changed Node input: {}", input.path.display()));
            }
            match (&input.implementation, input.role.is_case()) {
                (Some(_), false) => {
                    return Err(format!(
                        "support input has case membership: {}",
                        input.path.display()
                    ));
                }
                (
                    Some(Implementation::Included {
                        owners,
                        obligations,
                    }),
                    true,
                ) => {
                    if owners.is_empty()
                        || obligations.is_empty()
                        || obligations.iter().any(|item| item.trim().is_empty())
                    {
                        return Err(format!(
                            "included case has no implementation obligations: {}",
                            input.path.display()
                        ));
                    }
                    for owner in owners {
                        if !repository_path(repository, owner)?.is_file() {
                            return Err(format!(
                                "missing implementation owner: {}",
                                owner.display()
                            ));
                        }
                    }
                }
                (
                    Some(Implementation::Excluded {
                        missing_capabilities,
                    }),
                    true,
                ) if missing_capabilities.is_empty()
                    || missing_capabilities
                        .iter()
                        .any(|item| item.trim().is_empty()) =>
                {
                    return Err(format!(
                        "excluded case has no missing capability: {}",
                        input.path.display()
                    ));
                }
                _ => {}
            }
        }
        Ok(Self {
            document: inventory,
            sha256,
        })
    }

    /// Discovery order belongs to the inventory, not to filesystem enumeration.
    pub fn included_paths(&self, repository: &Path) -> Vec<PathBuf> {
        self.document
            .entries
            .iter()
            .filter(|input| matches!(&input.implementation, Some(Implementation::Included { .. })))
            .map(|input| repository.join(&input.path))
            .collect()
    }

    pub(crate) fn included_observations(&self) -> Vec<ObservationInput> {
        self.document
            .entries
            .iter()
            .filter(|input| {
                matches!(input.role, Role::ObservationCase)
                    && matches!(&input.implementation, Some(Implementation::Included { .. }))
            })
            .map(|input| ObservationInput {
                path: input.path.clone(),
                sha256: input.sha256.clone(),
            })
            .collect()
    }

    pub(crate) fn sha256(&self) -> &str {
        &self.sha256
    }

    pub fn validate_execution(
        &self,
        repository: &Path,
        selected: &[PathBuf],
    ) -> Result<(), String> {
        self.validate_frozen_membership()?;
        let included = self.included_paths(repository);
        if selected != included {
            return Err(format!(
                "complete Node inventory execution requires all {} included paths in inventory order; selected {} (use --subset --filter for a diagnostic subset)",
                included.len(),
                selected.len()
            ));
        }
        self.validate_observations(repository, selected)
    }

    pub fn validate_subset_execution(
        &self,
        repository: &Path,
        selected: &[PathBuf],
    ) -> Result<(), String> {
        self.validate_frozen_membership()?;
        let included = self.included_paths(repository);
        let mut seen = std::collections::HashSet::new();
        if selected.is_empty()
            || selected
                .iter()
                .any(|path| !included.contains(path) || !seen.insert(path))
        {
            return Err(
                "diagnostic subset must contain distinct paths from the included Node inventory"
                    .into(),
            );
        }
        self.validate_observations(repository, selected)
    }

    fn validate_frozen_membership(&self) -> Result<(), String> {
        if self.document.state != FROZEN_MEMBERSHIP_STATE
            || self
                .document
                .entries
                .iter()
                .any(|input| input.role.is_case() && input.implementation.is_none())
        {
            return Err(
                "implemented Node membership is not frozen; --list is diagnostic only".into(),
            );
        }
        Ok(())
    }

    fn validate_observations(&self, repository: &Path, selected: &[PathBuf]) -> Result<(), String> {
        let observations = self.included_observations();
        if observations
            .iter()
            .any(|input| selected.contains(&repository.join(&input.path)))
        {
            crate::node_observations::validate_evidence(self, repository, selected)?;
        }
        Ok(())
    }
}

fn repository_path(repository: &Path, path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(format!(
            "invalid repository-relative input: {}",
            path.display()
        ));
    }
    Ok(repository.join(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn membership_and_inputs_must_be_valid_before_execution() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("quench-node-inventory-{unique}"));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("case.js"), "inert case input").unwrap();
        fs::write(root.join("case-two.js"), "second inert case input").unwrap();
        fs::write(root.join("helper.js"), "inert support input").unwrap();
        fs::write(root.join("host.rs"), "inert owner input").unwrap();
        let inventory_path = root.join("inventory.json");
        let document = json!({
            "schema":INPUT_INVENTORY_SCHEMA,"state":FROZEN_MEMBERSHIP_STATE,
            "entries":[
                {"path":"case.js","sha256":format!("{:x}",Sha256::digest(b"inert case input")),"role":"assertion_case",
                 "implementation":{"status":"included","owners":["host.rs"],"obligations":["implemented API"]}},
                {"path":"case-two.js","sha256":format!("{:x}",Sha256::digest(b"second inert case input")),"role":"assertion_case",
                 "implementation":{"status":"included","owners":["host.rs"],"obligations":["implemented API"]}},
                {"path":"helper.js","sha256":format!("{:x}",Sha256::digest(b"inert support input")),"role":"module"}
            ]
        });
        fs::write(&inventory_path, document.to_string()).unwrap();
        let inventory = NodeInventory::read(&inventory_path, &root).unwrap();
        let selected = inventory.included_paths(&root);
        assert_eq!(selected, [root.join("case.js"), root.join("case-two.js")]);
        assert!(inventory.validate_execution(&root, &selected).is_ok());
        assert!(inventory.validate_execution(&root, &selected[..1]).is_err());
        assert!(inventory
            .validate_subset_execution(&root, &selected[..1])
            .is_ok());
        assert!(inventory.validate_subset_execution(&root, &[]).is_err());
        assert!(inventory
            .validate_subset_execution(&root, &[root.join("outside.js")])
            .is_err());

        for (label, changed) in [
            ("duplicate", {
                let mut value = document.clone();
                value["entries"][1] = value["entries"][0].clone();
                value
            }),
            ("hash", {
                let mut value = document.clone();
                value["entries"][0]["sha256"] = json!("changed");
                value
            }),
            ("parent path", {
                let mut value = document.clone();
                value["entries"][0]["path"] = json!("../case.js");
                value
            }),
            ("support membership", {
                let mut value = document.clone();
                value["entries"][2]["implementation"] =
                    value["entries"][0]["implementation"].clone();
                value
            }),
            ("unknown role", {
                let mut value = document.clone();
                value["entries"][0]["role"] = json!("unknown");
                value
            }),
            ("missing owner", {
                let mut value = document.clone();
                value["entries"][0]["implementation"]["owners"] = json!(["missing.rs"]);
                value
            }),
            ("empty obligation", {
                let mut value = document.clone();
                value["entries"][0]["implementation"]["obligations"] = json!([""]);
                value
            }),
            ("unsupported exclusion", {
                let mut value = document.clone();
                value["entries"][0]["implementation"] =
                    json!({"status":"excluded","missing_capabilities":[]});
                value
            }),
        ] {
            fs::write(&inventory_path, changed.to_string()).unwrap();
            assert!(
                NodeInventory::read(&inventory_path, &root).is_err(),
                "{label}"
            );
        }
        for role in ["assertion_case", "observation_case"] {
            let mut pending = document.clone();
            pending["state"] = json!("audit_pending");
            pending["entries"][0]["role"] = json!(role);
            fs::write(&inventory_path, pending.to_string()).unwrap();
            let inventory = NodeInventory::read(&inventory_path, &root).unwrap();
            assert!(inventory
                .validate_execution(&root, &inventory.included_paths(&root))
                .is_err());
            pending["state"] = json!(FROZEN_MEMBERSHIP_STATE);
            if role == "assertion_case" {
                pending["entries"][0]["implementation"] = json!(null);
            }
            fs::write(&inventory_path, pending.to_string()).unwrap();
            let inventory = NodeInventory::read(&inventory_path, &root).unwrap();
            assert!(inventory
                .validate_execution(&root, &inventory.included_paths(&root))
                .is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }
}
