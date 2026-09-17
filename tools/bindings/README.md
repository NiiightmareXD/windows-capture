# Windows bindings

The library follows [Microsoft's library guidance](https://github.com/microsoft/windows-rs):
use focused `windows-*` crates for shared types and generate the remaining APIs privately.
`windows-core`, `windows-collections`, `windows-future`, and `windows-time` use version
0.100.0. The library and Python wrapper have no direct `windows` or `windows-sys`
dependency. Some example/development dependencies still use `windows-sys` transitively.

`src/bindings.rs` is checked in and shipped in the library package. Normal builds
do not run a generator or depend on Windows metadata. This separate, unpublished
Cargo workspace pins `windows-bindgen` and locks its metadata dependencies.

## Regeneration

Use Rust **1.98.1**, including its rustfmt component (the version pinned in CI):

```sh
cargo +1.98.1 run --locked --manifest-path tools/bindings/Cargo.toml
cargo +1.98.1 run --locked --manifest-path tools/bindings/Cargo.toml -- --check
cargo test --locked -p windows-capture
cargo check --locked --workspace --all-targets
```

`--check` generates a temporary copy and fails if the checked-in file is missing
or differs. It does not overwrite the checked-in bindings. The Windows bindings
workflow runs this check and compiles all workspace targets on every pull request.
Update its Rust version deliberately when upgrading the generator/formatter.

Edit `filter.txt` to add APIs. Select individual methods instead of entire namespaces.
WinRT setters use metadata names such as `put_Width`; event accessors use `add_` and
`remove_`. Empty `Type::{}` entries include only a type's identity/layout. Factory
and static interface shells are explicit because 0.100.0 emits factory helpers
even for unselected class methods. Do not edit `src/bindings.rs` by hand.

`src/events.rs` preserves fallible callbacks and owns their registrations with
`windows_core::EventRevoker`. `src/bindings_impl.rs` preserves the previous
Direct3D/DXGI reference threading traits; unsafe context/DXGI calls still require
the caller's synchronization.

## Public API migration

The generated module is private. Types needed by public APIs are re-exported from
`windows_capture::interop`, including D3D11 interfaces, DXGI formats, and random
access streams. `GraphicsCaptureItem` also remains available at the crate root.

Replace `windows::core` imports with `windows_core`. Replace stream and D3D imports
with the corresponding `windows_capture::interop` imports. These generated types
are distinct Rust types from the old umbrella projections. For COM interop, use
`windows_core::Interface::cast` with projections using the same windows-core version;
older core versions require an explicit owned/borrowed raw COM boundary.

`windows_time::TimeSpan` has a lowercase `duration` field and `from_ticks`/
`from_millis` constructors. Conversion from `std::time::Duration` is now fallible
(`TimeSpan::try_from`). Native enums such as `DXGI_FORMAT` are integer aliases;
their constants no longer have a `.0` field. Native methods may return `HRESULT`
or `BOOL` directly; call `.ok()?` when checking their result.

These changes affect callers that use the exposed Windows types and require a
breaking release of the library.
