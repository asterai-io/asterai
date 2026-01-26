//! A publishable item, including the namespace, name, and version.
//! Resources include components, environments, and blueprints.
//! All resources are published to the same registry, and each resource
//! must have a unique name and version.
use crate::component::interface::PackageNameRegistry;
use crate::error::AsteraiError;
use derive_getters::Getters;
use eyre::{bail, eyre};
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use wit_parser::PackageName;

pub mod metadata;

#[derive(Debug, Clone, Getters, Eq, PartialEq, Hash)]
pub struct Resource {
    /// This is the resource ID and includes:
    /// - namespace (user or team slug)
    /// - name (WASM package name).
    /// - version (semver).
    ///
    /// Although the version in `PackageName` is optional,
    /// it is required in `Resource`s and is therefore
    /// guaranteed to be present.
    ///
    /// This accepts either `:` or `/` as a separator between namespace and name,
    /// but is stored in WIT style (namespace:name).
    package_name: PackageName,
}

impl Resource {
    pub fn namespace(&self) -> &str {
        &self.package_name.namespace
    }

    pub fn name(&self) -> &str {
        &self.package_name.name
    }

    pub fn id(&self) -> ResourceId {
        let mut package_name_without_version = self.package_name.clone();
        package_name_without_version.version = None;
        ResourceId::new(package_name_without_version).unwrap()
    }

    pub fn version(&self) -> &Version {
        self.package_name.version.as_ref().unwrap()
    }
}

/// The resource ID is similar to `Resource` except it does not
/// contain a version string.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ResourceId {
    package_name: PackageName,
}

impl ResourceId {
    pub fn new(package_name: PackageName) -> eyre::Result<Self> {
        if package_name.name.ends_with("-component") {
            bail!("resource name cannot end with -component");
        }
        if package_name.version.is_some() {
            bail!("cannot create ResourceId with version");
        }
        Ok(Self { package_name })
    }

    pub fn new_from_parts(namespace: String, name: String) -> eyre::Result<Self> {
        let package_name = PackageName {
            namespace,
            name,
            version: None,
        };
        Self::new(package_name)
    }

    pub fn with_version(mut self, version: &str) -> eyre::Result<Resource> {
        self.package_name.version = Some(Version::from_str(version)?);
        Ok(Resource {
            package_name: self.package_name,
        })
    }

    pub fn namespace(&self) -> &str {
        &self.package_name.namespace
    }

    pub fn name(&self) -> &str {
        &self.package_name.name
    }
}

impl Display for Resource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.package_name)
    }
}

impl FromStr for Resource {
    type Err = AsteraiError;

    fn from_str(str: &str) -> Result<Self, Self::Err> {
        let Some((resource_id_registry, resource_version)) = str.split_once('@') else {
            return AsteraiError::InputMissingSemVerString.into();
        };
        // Support both WIT-style (namespace:name) and OCI-style (namespace/name).
        let normalized_id = resource_id_registry.replace('/', ":");
        let package_name_registry =
            PackageNameRegistry::new(&normalized_id).map_err(AsteraiError::BadRequest.map())?;
        let version =
            Version::from_str(resource_version).map_err(AsteraiError::BadRequest.map())?;
        let resource = Self {
            package_name: PackageName {
                namespace: package_name_registry.namespace().to_owned(),
                name: package_name_registry.name().to_owned(),
                version: Some(version),
            },
        };
        Ok(resource)
    }
}

impl<'de> Deserialize<'de> for Resource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize the resource as a string.
        let s = String::deserialize(deserializer)?;
        Resource::from_str(s.as_str())
            .map_err(|_| serde::de::Error::custom(format!("invalid resource value: {s}")))
    }
}

impl Serialize for Resource {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl Display for ResourceId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.package_name)
    }
}

impl FromStr for ResourceId {
    type Err = eyre::Report;

    fn from_str(str: &str) -> Result<Self, Self::Err> {
        // Support both WIT-style (namespace:name) and OCI-style (namespace/name).
        let normalized_id = str.replace('/', ":");
        let package_name_registry =
            PackageNameRegistry::new(&normalized_id).map_err(|e| eyre!(e))?;
        let resource = Self {
            package_name: PackageName {
                namespace: package_name_registry.namespace().to_owned(),
                name: package_name_registry.name().to_owned(),
                version: None,
            },
        };
        Ok(resource)
    }
}

impl<'de> Deserialize<'de> for ResourceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Deserialize the resource as a string.
        let s = String::deserialize(deserializer)?;
        ResourceId::from_str(s.as_str())
            .map_err(|_| serde::de::Error::custom(format!("invalid resource ID value: {s}")))
    }
}

impl Serialize for ResourceId {
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
    fn test_resource_display() {
        let resource = Resource::from_str("asterai:test@0.1.0").unwrap();
        let stringified = resource.to_string();
        assert_eq!(stringified, "asterai:test@0.1.0");
    }

    #[test]
    fn test_resource_id_display() {
        let resource_id = ResourceId::from_str("asterai:test").unwrap();
        let stringified = resource_id.to_string();
        assert_eq!(stringified, "asterai:test");
    }

    #[test]
    fn test_resource_from_wit_style() {
        let resource = Resource::from_str("asterai:test@0.1.0").unwrap();
        assert_eq!(resource.namespace(), "asterai");
        assert_eq!(resource.name(), "test");
        assert_eq!(resource.version().to_string(), "0.1.0");
    }

    #[test]
    fn test_resource_from_oci_style() {
        let resource = Resource::from_str("asterai/test@0.1.0").unwrap();
        assert_eq!(resource.namespace(), "asterai");
        assert_eq!(resource.name(), "test");
        assert_eq!(resource.version().to_string(), "0.1.0");
        // Should display in WIT style.
        assert_eq!(resource.to_string(), "asterai:test@0.1.0");
    }

    #[test]
    fn test_resource_id_from_wit_style() {
        let resource_id = ResourceId::from_str("asterai:test").unwrap();
        assert_eq!(resource_id.namespace(), "asterai");
        assert_eq!(resource_id.name(), "test");
    }

    #[test]
    fn test_resource_id_from_oci_style() {
        let resource_id = ResourceId::from_str("asterai/test").unwrap();
        assert_eq!(resource_id.namespace(), "asterai");
        assert_eq!(resource_id.name(), "test");
        // Should display in WIT style.
        assert_eq!(resource_id.to_string(), "asterai:test");
    }
}
