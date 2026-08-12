use std::{collections::HashMap, path::PathBuf};

/// Cached exact test262 harness sources rooted at one harness directory.
pub struct HarnessCache {
    root: PathBuf,
    sources: HashMap<String, String>,
}

impl HarnessCache {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            sources: HashMap::new(),
        }
    }

    pub fn load(&mut self, name: &str) -> Result<String, String> {
        self.ensure(name)?;
        Ok(self.get(name)?.to_string())
    }

    fn ensure(&mut self, name: &str) -> Result<(), String> {
        if !self.sources.contains_key(name) {
            let source = std::fs::read_to_string(self.root.join(name))
                .map_err(|error| format!("harness {name}: {error}"))?;
            self.sources.insert(name.to_string(), source);
        }
        Ok(())
    }

    fn get(&self, name: &str) -> Result<&str, String> {
        self.sources
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| format!("harness cache missing {name}"))
    }

    pub(crate) fn sources(&mut self, names: &[&str]) -> Result<Vec<&str>, String> {
        for name in names {
            self.ensure(name)?;
        }
        names.iter().map(|name| self.get(name)).collect()
    }
}
