use eyre::eyre;
use wit_parser::decoding::DecodedWasm;
use wit_parser::{Resolve, Results, Type, TypeDefKind, World, WorldId, WorldItem};

/// Lightweight read-only wrapper around parsed WIT data.
#[derive(Clone)]
pub struct ComponentWit {
    resolve: Resolve,
    world_id: WorldId,
}

impl ComponentWit {
    /// Construct from an already-decoded Resolve and WorldId.
    pub fn new(resolve: Resolve, world_id: WorldId) -> Self {
        Self { resolve, world_id }
    }

    /// Parse from raw bytes
    /// (handles both compiled components and WIT-only packages).
    pub fn from_bytes(bytes: &[u8]) -> eyre::Result<Self> {
        let decoded = wit_parser::decoding::decode(bytes).map_err(|e| eyre!(e))?;
        match decoded {
            DecodedWasm::Component(resolve, world_id) => Ok(Self { resolve, world_id }),
            DecodedWasm::WitPackage(resolve, pkg_id) => {
                let world_id = *resolve.packages[pkg_id]
                    .worlds
                    .values()
                    .next()
                    .ok_or_else(|| eyre!("WIT package defines no worlds"))?;
                Ok(Self { resolve, world_id })
            }
        }
    }

    pub fn resolve(&self) -> &Resolve {
        &self.resolve
    }

    pub fn world(&self) -> &World {
        &self.resolve.worlds[self.world_id]
    }

    pub fn world_docs(&self) -> Option<String> {
        self.world().docs.contents.clone()
    }
}

/// Uniform interface for querying WIT imports and exports.
pub trait ComponentInterface {
    fn imported_interfaces(&self) -> Vec<ImportedInterface>;
    fn exported_interfaces(&self) -> Vec<ExportedInterface>;
    fn world_functions(&self) -> Vec<ComponentFunction>;
}

pub struct ImportedInterface {
    /// Fully qualified name, e.g. "wasi:http/outgoing-handler@0.2.0".
    pub name: String,
}

pub struct ExportedInterface {
    /// Fully qualified name, e.g. "asterai:host/api@0.1.0".
    pub name: String,
    pub docs: Option<String>,
    pub functions: Vec<ComponentFunction>,
}

pub struct ComponentFunction {
    pub name: String,
    pub docs: Option<String>,
    pub params: Vec<FunctionParam>,
    /// Display string for return type. None if no return.
    pub return_type: Option<String>,
}

pub struct FunctionParam {
    pub name: String,
    /// Display string, e.g. "string", "option<config>".
    pub type_name: String,
}

impl ComponentInterface for ComponentWit {
    fn imported_interfaces(&self) -> Vec<ImportedInterface> {
        let world = self.world();
        world
            .imports
            .iter()
            .filter_map(|(_, item)| match item {
                WorldItem::Interface { id, .. } => Some(ImportedInterface {
                    name: format_interface_name(&self.resolve, *id),
                }),
                _ => None,
            })
            .collect()
    }

    fn exported_interfaces(&self) -> Vec<ExportedInterface> {
        let world = self.world();
        world
            .exports
            .iter()
            .filter_map(|(_, item)| match item {
                WorldItem::Interface { id, .. } => {
                    let iface = &self.resolve.interfaces[*id];
                    let functions = iface
                        .functions
                        .iter()
                        .map(|(_, func)| build_exported_function(&self.resolve, func))
                        .collect();
                    Some(ExportedInterface {
                        name: format_interface_name(&self.resolve, *id),
                        docs: iface.docs.contents.clone(),
                        functions,
                    })
                }
                _ => None,
            })
            .collect()
    }

    /// Functions exported at the world level and therefore not composable
    /// with other components (only callable by the host).
    /// See <https://component-model.bytecodealliance.org/composing-and-distributing/composing.html#what-is-composition>.
    fn world_functions(&self) -> Vec<ComponentFunction> {
        let world = self.world();
        world
            .exports
            .iter()
            .filter_map(|(_, item)| match item {
                WorldItem::Function(func) => Some(build_exported_function(&self.resolve, func)),
                _ => None,
            })
            .collect()
    }
}

fn build_exported_function(resolve: &Resolve, func: &wit_parser::Function) -> ComponentFunction {
    let params = func
        .params
        .iter()
        .map(|(name, ty)| FunctionParam {
            name: name.clone(),
            type_name: type_display(resolve, *ty),
        })
        .collect();
    let return_type = match &func.results {
        results if results.len() == 0 => None,
        Results::Anon(ty) => Some(type_display(resolve, *ty)),
        Results::Named(named) => {
            let parts: Vec<String> = named
                .iter()
                .map(|(n, ty)| format!("{n}: {}", type_display(resolve, *ty)))
                .collect();
            Some(format!("({})", parts.join(", ")))
        }
    };
    ComponentFunction {
        name: func.name.clone(),
        docs: func.docs.contents.clone(),
        params,
        return_type,
    }
}

/// Formats a fully qualified interface name, e.g.
/// `wasi:http/outgoing-handler@0.2.0`.
fn format_interface_name(resolve: &Resolve, id: wit_parser::InterfaceId) -> String {
    let interface = &resolve.interfaces[id];
    let interface_name = interface.name.as_deref().unwrap_or("unknown");
    let Some(pkg_id) = interface.package else {
        return interface_name.to_owned();
    };
    let pkg = &resolve.packages[pkg_id];
    let version_suffix = pkg
        .name
        .version
        .as_ref()
        .map(|v| format!("@{v}"))
        .unwrap_or_default();
    format!(
        "{}:{}/{interface_name}{version_suffix}",
        pkg.name.namespace, pkg.name.name
    )
}

/// Converts a WIT type to a human-readable display string.
pub fn type_display(resolve: &Resolve, ty: Type) -> String {
    match ty {
        Type::Bool => "bool".to_owned(),
        Type::U8 => "u8".to_owned(),
        Type::U16 => "u16".to_owned(),
        Type::U32 => "u32".to_owned(),
        Type::U64 => "u64".to_owned(),
        Type::S8 => "s8".to_owned(),
        Type::S16 => "s16".to_owned(),
        Type::S32 => "s32".to_owned(),
        Type::S64 => "s64".to_owned(),
        Type::F32 => "f32".to_owned(),
        Type::F64 => "f64".to_owned(),
        Type::Char => "char".to_owned(),
        Type::String => "string".to_owned(),
        Type::Id(id) => {
            let type_def = &resolve.types[id];
            if let Some(name) = &type_def.name {
                return name.clone();
            }
            type_kind_display(resolve, &type_def.kind)
        }
    }
}

/// Converts a TypeDefKind to a display string when no name is available.
fn type_kind_display(resolve: &Resolve, kind: &TypeDefKind) -> String {
    match kind {
        TypeDefKind::Option(ty) => {
            format!("option<{}>", type_display(resolve, *ty))
        }
        TypeDefKind::List(ty) => {
            format!("list<{}>", type_display(resolve, *ty))
        }
        TypeDefKind::Result(r) => {
            let ok =
                r.ok.map(|t| type_display(resolve, t))
                    .unwrap_or_else(|| "_".to_owned());
            let err = r
                .err
                .map(|t| type_display(resolve, t))
                .unwrap_or_else(|| "_".to_owned());
            format!("result<{ok}, {err}>")
        }
        TypeDefKind::Tuple(t) => {
            let parts: Vec<String> = t
                .types
                .iter()
                .map(|ty| type_display(resolve, *ty))
                .collect();
            format!("tuple<{}>", parts.join(", "))
        }
        TypeDefKind::Record(_) => "record".to_owned(),
        TypeDefKind::Variant(_) => "variant".to_owned(),
        TypeDefKind::Enum(_) => "enum".to_owned(),
        TypeDefKind::Flags(_) => "flags".to_owned(),
        TypeDefKind::Type(ty) => type_display(resolve, *ty),
        _ => "unknown".to_owned(),
    }
}
