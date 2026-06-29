use crate::context::Meta;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Idea {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub created_at: String,
    pub source: String,
    pub tags: Vec<String>,
    pub related_tasks: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IdeasFile {
    /// Schema version metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,

    pub ideas: Vec<Idea>,
}
