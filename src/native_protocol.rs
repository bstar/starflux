//! Durable native source identities shared by models and the Linux owner.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub id: String,
    pub epoch: String,
    pub boot: String,
    pub pid: u32,
    pub start_ticks: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct Identity {
    pub instance: String,
    pub boot: String,
    pub pid: u32,
    pub start_ticks: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct Binding {
    pub version: u8,
    pub project: String,
    pub pane: usize,
    pub terminal_epoch: String,
    pub owner: Identity,
    pub source_id: String,
    pub source_epoch: String,
    pub launch_digest: String,
    /// None means the intent was persisted but its delivery/outcome is unknown.
    pub source: Option<Reference>,
}
