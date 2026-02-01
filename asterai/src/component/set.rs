use crate::component::Component;
use crate::error::AsteraiError;
use std::collections::HashSet;
use std::str::FromStr;

pub struct ComponentSet(HashSet<Component>);

impl ComponentSet {
    pub fn inner(&self) -> &HashSet<Component> {
        &self.0
    }

    pub fn take(self) -> HashSet<Component> {
        self.0
    }
}

impl FromStr for ComponentSet {
    type Err = AsteraiError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let set = s
            .split(",")
            .map(Component::from_str)
            .collect::<Result<HashSet<Component>, Self::Err>>()?;
        Ok(Self(set))
    }
}
