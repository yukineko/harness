use crate::config::Config;
use crate::hypothesis::Hypothesis;
use anyhow::Result;

pub struct Store {
    hypotheses: Vec<Hypothesis>,
}

impl Store {
    pub fn load(_cfg: &Config) -> Result<Self> {
        Ok(Self { hypotheses: vec![] })
    }

    pub fn add(&mut self, _text: String, _goal: Option<String>) -> Result<String> {
        Ok(String::new())
    }

    pub fn validate(&mut self, _id: &str, _evidence: Vec<String>) -> Result<()> {
        Ok(())
    }

    pub fn reject(&mut self, _id: &str, _reason: Option<String>) -> Result<()> {
        Ok(())
    }

    pub fn list(&self, _status: Option<&str>) -> &[Hypothesis] {
        &self.hypotheses
    }
}
