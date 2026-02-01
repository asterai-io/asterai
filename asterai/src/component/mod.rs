use crate::checksum::Checksum;
use crate::component::interface::{ComponentBinary, PackageNameRegistry};
use crate::error::AsteraiError;
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
pub mod wit;

pub type ComponentModuleId = Checksum;

#[derive(Debug, Clone, Getters, Eq, PartialEq, Hash)]
pub struct Component {
    /// This is the component ID and includes:
    /// - component namespace (user or team slug)
    /// - component name (WASM package name).
    /// - version (semver).
    ///
    /// Although the version in `PackageName` is optional,
    /// it is required in asterai components and is therefore
    /// guaranteed to be present.
    package_name: PackageName,
}

impl Component {
    pub fn new(package_name: PackageName) -> eyre::Result<Self> {
        if package_name.name.ends_with("-component") {
            return Err(eyre!("component name cannot end with -component"));
        }
        if package_name.version.is_none() {
            return Err(eyre!("version is required for component"));
        }
        Ok(Self { package_name })
    }

    pub async fn fetch_interface(
        &self,
        wkg_client: &wasm_pkg_client::Client,
    ) -> eyre::Result<ComponentBinary> {
        ComponentBinary::fetch(self.clone(), wkg_client).await
    }

    pub fn namespace(&self) -> &str {
        &self.package_name.namespace
    }

    pub fn name(&self) -> &str {
        &self.package_name.name
    }

    pub fn id(&self) -> ComponentId {
        let mut package_name_without_version = self.package_name.clone();
        package_name_without_version.version = None;
        ComponentId::new(package_name_without_version).unwrap()
    }

    pub fn version(&self) -> &Version {
        self.package_name.version.as_ref().unwrap()
    }
}

/// The component ID is similar to `Component` except it does not
/// contain a version string.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ComponentId {
    package_name: PackageName,
}

impl ComponentId {
    pub fn new(package_name: PackageName) -> eyre::Result<Self> {
        if package_name.name.ends_with("-component") {
            bail!("component name cannot end with -component");
        }
        if package_name.version.is_some() {
            bail!("cannot create ComponentId with version");
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

impl Display for Component {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.package_name)
    }
}

impl FromStr for Component {
    type Err = AsteraiError;

    fn from_str(str: &str) -> Result<Self, Self::Err> {
        let Some((component_id_registry, component_version)) = str.split_once('@') else {
            return AsteraiError::InputMissingSemVerString.into();
        };
        let package_name_registry = PackageNameRegistry::new(component_id_registry)
            .map_err(AsteraiError::BadRequest.map())?;
        let version =
            Version::from_str(component_version).map_err(AsteraiError::BadRequest.map())?;
        let component = Self {
            package_name: PackageName {
                namespace: package_name_registry.namespace().to_owned(),
                name: package_name_registry.name().to_owned(),
                version: Some(version),
            },
        };
        Ok(component)
    }
}

impl<'de> Deserialize<'de> for Component {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize the component as a string.
        let s = String::deserialize(deserializer)?;
        Component::from_str(s.as_str())
            .map_err(|_| serde::de::Error::custom(format!("invalid component value: {s}")))
    }
}

impl Serialize for Component {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl Display for ComponentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.package_name)
    }
}

impl FromStr for ComponentId {
    type Err = eyre::Report;

    fn from_str(str: &str) -> Result<Self, Self::Err> {
        let package_name_registry = PackageNameRegistry::new(str).map_err(|e| eyre!(e))?;
        let component = Self {
            package_name: PackageName {
                namespace: package_name_registry.namespace().to_owned(),
                name: package_name_registry.name().to_owned(),
                version: None,
            },
        };
        Ok(component)
    }
}

impl<'de> Deserialize<'de> for ComponentId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize the component as a string.
        let s = String::deserialize(deserializer)?;
        ComponentId::from_str(s.as_str())
            .map_err(|_| serde::de::Error::custom(format!("invalid component ID value: {s}")))
    }
}

impl Serialize for ComponentId {
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
    fn test_component_display() {
        let component = Component::from_str("asterai:test@0.1.0").unwrap();
        let stringified = component.to_string();
        assert_eq!(stringified, "asterai:test@0.1.0");
    }

    #[test]
    fn test_component_id_display() {
        let component_id = ComponentId::from_str("asterai:test").unwrap();
        let stringified = component_id.to_string();
        assert_eq!(stringified, "asterai:test");
    }
}
