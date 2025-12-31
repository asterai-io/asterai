use crate::auth::Auth;
use asterai_runtime::plugin::Version;
use std::convert::Infallible;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct EnvResourceOrId {
    namespace: Option<String>,
    name: String,
    version: Option<Version>,
}

impl EnvResourceOrId {
    pub fn with_local_namespace_fallback(&self) -> String {
        let namespace = self
            .namespace
            .clone()
            .unwrap_or_else(|| Auth::read_user_or_fallback_namespace());
        let version = self
            .version
            .as_ref()
            .map(|v| format!("@{v}"))
            .unwrap_or_default();
        format!("{namespace}:{}{version}", self.name)
    }
}

impl FromStr for EnvResourceOrId {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (name_or_id, version_opt) = match s.split_once('@') {
            None => (s, None),
            Some((p, v)) => {
                let version_opt = Version::from_str(v).ok();
                (p, version_opt)
            }
        };
        let v = match name_or_id.split_once(':') {
            Some((namespace, name)) => Self {
                namespace: Some(namespace.to_owned()),
                name: name.to_owned(),
                version: version_opt,
            },
            None => Self {
                namespace: None,
                name: name_or_id.to_owned(),
                version: version_opt,
            },
        };
        Ok(v)
    }
}

impl Display for EnvResourceOrId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let namespace = self
            .namespace
            .as_ref()
            .map(|n| format!("{n}:"))
            .unwrap_or_default();
        write!(f, "{namespace}{}", self.name)
    }
}
