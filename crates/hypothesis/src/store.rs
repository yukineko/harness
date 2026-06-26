use crate::config::Config;
use crate::hypothesis::{Hypothesis, Status};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

// ── TOML file shape ───────────────────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize)]
struct HypothesesFile {
    #[serde(default)]
    hypotheses: Vec<Hypothesis>,
}

// ── Timestamp (for validate/reject updated_at) ────────────────────────────────

fn now_iso() -> String {
    use chrono::Utc;
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

// ── Store ─────────────────────────────────────────────────────────────────────

pub struct Store {
    hypotheses: Vec<Hypothesis>,
    cfg: Config,
}

impl Store {
    pub fn load(cfg: &Config) -> Result<Self> {
        let path = cfg.hypotheses_path();
        let hypotheses = if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            let file: HypothesesFile = toml::from_str(&text)?;
            file.hypotheses
        } else {
            vec![]
        };
        Ok(Self {
            hypotheses,
            cfg: Config {
                enabled: cfg.enabled,
                store_dir: cfg.store_dir.clone(),
                inject_limit: cfg.inject_limit,
            },
        })
    }

    fn save(&self) -> Result<()> {
        let store_dir = &self.cfg.store_dir;
        std::fs::create_dir_all(store_dir)?;

        let file = HypothesesFile {
            hypotheses: self.hypotheses.clone(),
        };
        let content = toml::to_string_pretty(&file)?;

        // Atomic write: write to temp file then rename
        let tmp_name = format!(
            ".hypotheses.tmp.{}.toml",
            {
                let mut h = DefaultHasher::new();
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
                    .hash(&mut h);
                h.finish()
            }
        );
        let tmp_path = store_dir.join(&tmp_name);
        std::fs::write(&tmp_path, content)?;
        std::fs::rename(&tmp_path, self.cfg.hypotheses_path())?;
        Ok(())
    }

    pub fn add(&mut self, text: String, goal: Option<String>) -> Result<String> {
        let h = Hypothesis::new(text, goal);
        let id = h.id.clone();
        self.hypotheses.push(h);
        self.save()?;
        Ok(id)
    }

    pub fn validate(&mut self, id: &str, evidence: Vec<String>, run_id: Option<String>) -> Result<()> {
        let h = self
            .hypotheses
            .iter_mut()
            .find(|h| h.id == id)
            .ok_or_else(|| anyhow::anyhow!("hypothesis not found: {id}"))?;
        h.status = Status::Validated;
        h.evidence.extend(evidence);
        h.condukt_run = run_id;
        h.updated_at = now_iso();
        self.save()
    }

    pub fn reject(&mut self, id: &str, reason: Option<String>, run_id: Option<String>) -> Result<()> {
        let h = self
            .hypotheses
            .iter_mut()
            .find(|h| h.id == id)
            .ok_or_else(|| anyhow::anyhow!("hypothesis not found: {id}"))?;
        h.status = Status::Rejected;
        if let Some(r) = reason {
            h.evidence.push(r);
        }
        h.condukt_run = run_id;
        h.updated_at = now_iso();
        self.save()
    }

    pub fn list(&self, status: Option<&str>) -> &[Hypothesis] {
        match status {
            None => &self.hypotheses,
            Some(_) => &self.hypotheses, // filtered below via iter in callers; return all for now
        }
    }

    #[allow(dead_code)]
    pub fn list_filtered(&self, status: Option<&str>) -> Vec<&Hypothesis> {
        match status {
            None => self.hypotheses.iter().collect(),
            Some(s) => self.hypotheses.iter().filter(|h| h.status.to_string() == s).collect(),
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_cfg(dir: &TempDir) -> Config {
        Config {
            enabled: true,
            store_dir: dir.path().to_path_buf(),
            inject_limit: 2000,
        }
    }

    #[test]
    fn test_add_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let cfg = test_cfg(&dir);

        // add a hypothesis
        let mut st = Store::load(&cfg).unwrap();
        let id = st.add("my hypothesis text".to_string(), None).unwrap();
        assert!(!id.is_empty());

        // reload from disk
        let st2 = Store::load(&cfg).unwrap();
        let hypotheses = st2.list(None);
        assert_eq!(hypotheses.len(), 1);
        assert_eq!(hypotheses[0].id, id);
        assert_eq!(hypotheses[0].text, "my hypothesis text");
        assert!(hypotheses[0].status.is_open());
    }

    #[test]
    fn test_validate() {
        let dir = TempDir::new().unwrap();
        let cfg = test_cfg(&dir);

        let mut st = Store::load(&cfg).unwrap();
        let id = st.add("validate this".to_string(), None).unwrap();

        st.validate(&id, vec!["evidence A".to_string(), "evidence B".to_string()], None)
            .unwrap();

        // reload to verify persistence
        let st2 = Store::load(&cfg).unwrap();
        let h = &st2.list(None)[0];
        assert!(h.status.is_validated());
        assert!(h.evidence.contains(&"evidence A".to_string()));
        assert!(h.evidence.contains(&"evidence B".to_string()));
    }

    #[test]
    fn test_reject() {
        let dir = TempDir::new().unwrap();
        let cfg = test_cfg(&dir);

        let mut st = Store::load(&cfg).unwrap();
        let id = st.add("reject this".to_string(), None).unwrap();

        st.reject(&id, Some("not supported by data".to_string()), None)
            .unwrap();

        let st2 = Store::load(&cfg).unwrap();
        let h = &st2.list(None)[0];
        assert!(h.status.is_rejected());
        assert!(h.evidence.contains(&"not supported by data".to_string()));
    }

    #[test]
    fn test_unknown_id_error() {
        let dir = TempDir::new().unwrap();
        let cfg = test_cfg(&dir);

        let mut st = Store::load(&cfg).unwrap();

        let err = st.validate("deadbeef", vec![], None).unwrap_err();
        assert!(err.to_string().contains("hypothesis not found"));

        let err2 = st.reject("deadbeef", None, None).unwrap_err();
        assert!(err2.to_string().contains("hypothesis not found"));
    }

    #[test]
    fn test_load_empty_when_file_missing() {
        let dir = TempDir::new().unwrap();
        let cfg = test_cfg(&dir);

        // File does not exist — should return empty store, not error
        let st = Store::load(&cfg).unwrap();
        assert_eq!(st.list(None).len(), 0);
    }

    #[test]
    fn test_validate_with_run_id() {
        let dir = TempDir::new().unwrap();
        let cfg = test_cfg(&dir);

        let mut st = Store::load(&cfg).unwrap();
        let id = st.add("validate with run".to_string(), None).unwrap();
        st.validate(&id, vec![], Some("run-abc123".to_string())).unwrap();

        let st2 = Store::load(&cfg).unwrap();
        let h = &st2.list(None)[0];
        assert!(h.status.is_validated());
        assert_eq!(h.condukt_run, Some("run-abc123".to_string()));
    }

    #[test]
    fn test_reject_with_run_id() {
        let dir = TempDir::new().unwrap();
        let cfg = test_cfg(&dir);

        let mut st = Store::load(&cfg).unwrap();
        let id = st.add("reject with run".to_string(), None).unwrap();
        st.reject(&id, None, Some("run-xyz789".to_string())).unwrap();

        let st2 = Store::load(&cfg).unwrap();
        let h = &st2.list(None)[0];
        assert!(h.status.is_rejected());
        assert_eq!(h.condukt_run, Some("run-xyz789".to_string()));
    }

    #[test]
    fn test_linked_goal_preserved() {
        let dir = TempDir::new().unwrap();
        let cfg = test_cfg(&dir);

        let mut st = Store::load(&cfg).unwrap();
        st.add("with goal".to_string(), Some("goal-abc".to_string()))
            .unwrap();

        let st2 = Store::load(&cfg).unwrap();
        let h = &st2.list(None)[0];
        assert_eq!(h.linked_goal, Some("goal-abc".to_string()));
    }
}
