# Desenvolvimento

## Arch e CachyOS

Instale `base-devel`, `rust`, `pnpm`, `webkit2gtk-4.1`, `libappindicator-gtk3` e `librsvg` com o gerenciador do sistema. Rust stable é o target suportado.

## Rotina

1. `pnpm install`
2. `pnpm tauri dev`
3. `pnpm lint && pnpm typecheck && pnpm test`
4. `cargo test --manifest-path src-tauri/Cargo.toml`
5. `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
6. `pnpm tauri build`

O bundle DEB é validado com `pnpm tauri build --bundles deb`. O AppImage depende do `linuxdeploy`; em ambientes onde ele não executa, valide as dependências/FUSE do host ou use o modo de extração do AppImage antes de considerar o release concluído.

Use `RUST_LOG=orbit_launcher=debug` para diagnóstico no terminal. Providers devem ignorar formatos desconhecidos com warning, jamais panic. Novas migrations devem ser aditivas e versionadas antes de alterar schema existente.

## Implementando uma loja

Implemente `GameProvider` e produza apenas `ProviderCommand` com executável e argumentos separados. Não invoque shell. Registre dependências no `DependencyManager`, adicione testes exatos dos argumentos e defina autenticação externa ou por cliente gerenciado. Senhas, cookies e refresh tokens não podem ser serializados em logs, IPC ou SQLite.
