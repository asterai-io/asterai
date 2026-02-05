wasmtime::component::bindgen!({
    world: "host",
    path: "wit/asterai_host.wit",
    ownership: Owning,
    additional_derives: [PartialEq, Clone],
});
