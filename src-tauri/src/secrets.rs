use anyhow::Result;
use serde_json::{Map, Value};
use std::fs;
use std::path::PathBuf;

#[derive(Clone)]
pub struct Secrets {
    path: PathBuf,
}

impl Secrets {
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self {
            path: app_data_dir.join("secrets.json"),
        }
    }

    fn load(&self) -> Result<Map<String, Value>> {
        if !self.path.exists() {
            return Ok(Map::new());
        }
        let s = fs::read_to_string(&self.path)?;
        Ok(serde_json::from_str::<Map<String, Value>>(&s).unwrap_or_default())
    }

    fn save(&self, map: &Map<String, Value>) -> Result<()> {
        fs::create_dir_all(self.path.parent().unwrap())?;
        fs::write(&self.path, serde_json::to_string_pretty(map)?)?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<Option<String>> {
        let map = self.load()?;
        Ok(map.get(key).and_then(|v| v.as_str()).map(|s| s.to_string()))
    }

    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        let mut map = self.load()?;
        map.insert(key.to_string(), Value::String(value.to_string()));
        self.save(&map)
    }

    pub fn delete(&self, key: &str) -> Result<()> {
        let mut map = self.load()?;
        map.remove(key);
        self.save(&map)
    }
}
