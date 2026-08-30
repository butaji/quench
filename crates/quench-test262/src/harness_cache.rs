use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
};

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
            let path = harness_path(&self.root, name)?;
            let source = std::fs::read_to_string(path)
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

    pub(crate) fn sources<'cache, 'name, I>(
        &'cache mut self,
        names: I,
    ) -> Result<Vec<&'cache str>, String>
    where
        I: IntoIterator<Item = &'name str>,
        I::IntoIter: Clone,
    {
        let names = names.into_iter();
        for name in names.clone() {
            self.ensure(name)?;
        }
        names.map(|name| self.get(name)).collect()
    }
}

fn harness_path(root: &Path, name: &str) -> Result<PathBuf, String> {
    let relative = Path::new(name);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!("harness path escapes root: {name}"));
    }
    Ok(root.join(relative))
}
