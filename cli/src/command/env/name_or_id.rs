use crate::auth::Auth;
use std::convert::Infallible;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EnvNameOrId {
    namespace: Option<String>,
    name: String,
}

impl EnvNameOrId {
    pub fn id_with_local_namespace_fallback(&self) -> String {
        let namespace = self
            .namespace
            .clone()
            .unwrap_or_else(|| Auth::read_user_or_fallback_namespace());
        format!("{namespace}:{}", self.name)
    }
}

impl FromStr for EnvNameOrId {
    type Err = Infallible;

    fn from_str(name_or_id: &str) -> Result<Self, Self::Err> {
        let v = match name_or_id.split_once(':') {
            Some((namespace, name)) => Self {
                namespace: Some(namespace.to_owned()),
                name: name.to_owned(),
            },
            None => Self {
                namespace: None,
                name: name_or_id.to_owned(),
            },
        };
        Ok(v)
    }
}

impl Display for EnvNameOrId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let namespace = self
            .namespace
            .as_ref()
            .map(|n| format!("{n}:"))
            .unwrap_or_default();
        write!(f, "{namespace}{}", self.name)
    }
}
