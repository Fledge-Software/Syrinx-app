//! Host-side bindings for `syrinx:plugin@0.1.0`, generated from the same WIT plugins build against.

wasmtime::component::bindgen!({
    path: "../../wit/v0_1",
    world: "plugin",
    // Values are compared in the host's and the plugins' contract tests.
    additional_derives: [PartialEq],
});
