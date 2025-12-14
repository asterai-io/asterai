use crate::checksum::Checksum;
use crate::error::AsteraiError;
use crate::plugin::interface::{PackageNameRegistry, PluginInterface};
use derive_getters::Getters;
use eyre::{bail, eyre};
pub use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::str::FromStr;
pub use wit_parser::PackageName;

pub mod function_name;
pub mod interface;
pub mod log;
pub mod pkg;
pub mod set;

pub type PluginModuleId = Checksum;

#[derive(Debug, Clone, Getters, Eq, PartialEq, Hash)]
pub struct Plugin {
    /// This is the plugin ID and includes:
    /// - plugin namespace (user or team slug)
    /// - plugin name (WASM package name).
    /// - version (semver).
    /// Although the version in `PackageName` is optional,
    /// it is required in asterai plugins and is therefore
    /// guaranteed to be present.
    package_name: PackageName,
}

impl Plugin {
    pub fn new(package_name: PackageName) -> eyre::Result<Self> {
        if package_name.name.ends_with("-component") {
            return Err(eyre!("plugin name cannot end with -component"));
        }
        if package_name.version.is_none() {
            return Err(eyre!("version is required for plugin"));
        }
        Ok(Self { package_name })
    }

    pub async fn fetch_interface(
        &self,
        wkg_client: &wasm_pkg_client::Client,
    ) -> eyre::Result<PluginInterface> {
        PluginInterface::fetch(self.clone(), wkg_client).await
    }

    pub fn namespace(&self) -> &str {
        &self.package_name.namespace
    }

    pub fn name(&self) -> &str {
        &self.package_name.name
    }

    pub fn id(&self) -> PluginId {
        let mut package_name_without_version = self.package_name.clone();
        package_name_without_version.version = None;
        PluginId::new(package_name_without_version).unwrap()
    }

    pub fn version(&self) -> &Version {
        self.package_name.version.as_ref().unwrap()
    }
}

/// The plugin ID is similar to `Plugin` except it does not
/// contain a version string.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct PluginId {
    package_name: PackageName,
}

impl PluginId {
    pub fn new(package_name: PackageName) -> eyre::Result<Self> {
        if package_name.name.ends_with("-component") {
            bail!("plugin name cannot end with -component");
        }
        if package_name.version.is_some() {
            bail!("cannot create PluginId with version");
        }
        Ok(Self { package_name })
    }

    pub fn namespace(&self) -> &str {
        &self.package_name.namespace
    }

    pub fn name(&self) -> &str {
        &self.package_name.name
    }
}

impl Display for Plugin {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.package_name)
    }
}

impl FromStr for Plugin {
    type Err = AsteraiError;

    fn from_str(str: &str) -> Result<Self, Self::Err> {
        let Some((plugin_id_registry, plugin_version)) = str.split_once('@') else {
            return AsteraiError::InputMissingSemVerString.into();
        };
        let package_name_registry =
            PackageNameRegistry::new(plugin_id_registry).map_err(AsteraiError::BadRequest.map())?;
        let version = Version::from_str(&plugin_version).map_err(AsteraiError::BadRequest.map())?;
        let plugin = Self {
            package_name: PackageName {
                namespace: package_name_registry.namespace().to_owned(),
                name: package_name_registry.name().to_owned(),
                version: Some(version),
            },
        };
        Ok(plugin)
    }
}

impl<'de> Deserialize<'de> for Plugin {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize the plugin as a string.
        let s = String::deserialize(deserializer)?;
        Plugin::from_str(s.as_str())
            .map_err(|_| serde::de::Error::custom(format!("invalid plugin value: {s}")))
    }
}

impl Serialize for Plugin {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl Display for PluginId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.package_name)
    }
}

impl FromStr for PluginId {
    type Err = eyre::Report;

    fn from_str(str: &str) -> Result<Self, Self::Err> {
        let package_name_registry = PackageNameRegistry::new(str).map_err(|e| eyre!(e))?;
        let plugin = Self {
            package_name: PackageName {
                namespace: package_name_registry.namespace().to_owned(),
                name: package_name_registry.name().to_owned(),
                version: None,
            },
        };
        Ok(plugin)
    }
}

impl<'de> Deserialize<'de> for PluginId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize the plugin as a string.
        let s = String::deserialize(deserializer)?;
        PluginId::from_str(s.as_str())
            .map_err(|_| serde::de::Error::custom(format!("invalid plugin ID value: {s}")))
    }
}

impl Serialize for PluginId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_plugin_display() {
        let plugin = Plugin::from_str("asterai:test@0.1.0").unwrap();
        let stringified = plugin.to_string();
        assert_eq!(stringified, "asterai:test@0.1.0");
    }

    #[test]
    fn test_plugin_id_display() {
        let plugin_id = PluginId::from_str("asterai:test").unwrap();
        let stringified = plugin_id.to_string();
        assert_eq!(stringified, "asterai:test");
    }
}
