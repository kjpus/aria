use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("{0}")]
    Message(String),
}

impl Serialize for CommandError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

impl From<aria_app_core::AppCoreError> for CommandError {
    fn from(value: aria_app_core::AppCoreError) -> Self {
        Self::Message(value.to_string())
    }
}
