use log::error;
use std::fmt::Debug;
use thiserror::Error;

#[derive(Debug, Error, Eq, PartialEq, Hash)]
pub enum AsteraiError {
    #[error("input did not include semver version string (e.g. @0.1.0)")]
    InputMissingSemVerString,
    #[error("bad request (malformed input)")]
    BadRequest,
}

pub type AsteraiResult<T> = Result<T, AsteraiError>;

impl AsteraiError {
    pub fn map<E: Debug>(self) -> impl FnOnce(E) -> Self {
        |e| {
            error!("{e:#?}");
            self
        }
    }
}

impl<T> Into<Result<T, AsteraiError>> for AsteraiError {
    fn into(self) -> Result<T, AsteraiError> {
        Err(self)
    }
}
