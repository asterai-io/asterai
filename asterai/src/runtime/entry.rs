//! Host entry points, called from the plugin module.
use crate::runtime::env::HostEnv;
use crate::runtime::wit_bindings::exports::asterai::host::api::RuntimeInfo;
use eyre::eyre;
use std::future::Future;
use wasmtime::StoreContextMut;
use wasmtime::component::Linker;

pub fn add_asterai_host_to_linker(linker: &mut Linker<HostEnv>) -> eyre::Result<()> {
    let mut exported_instance = linker
        .instance("asterai:host/api@0.1.0")
        .map_err(|e| eyre!("{e:#?}"))?;
    // TODO allow a sink to forward these errors somewhere?
    exported_instance
        .func_wrap_async("get-runtime-info", get_runtime_info)
        .map_err(|e| eyre!("{e:#?}"))?;
    Ok(())
}

fn get_runtime_info<'a>(
    store: StoreContextMut<'a, HostEnv>,
    (_key, _user_id): (String, Option<String>),
) -> Box<dyn Future<Output = eyre::Result<(RuntimeInfo,), wasmtime::Error>> + Send + 'a> {
    Box::new(async move {
        let _runtime_metadata = store.data().runtime_data.as_ref().unwrap();
        let runtime_info = RuntimeInfo {
            // TODO actually get version from metadata.
            version: "todo-set-version".to_owned(),
        };
        Ok((runtime_info,))
    })
}
