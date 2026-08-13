<<<<<<< HEAD
# Universal-Linux-Launcher
Orbit is a modern Linux game and app launcher built for CachyOS and Arch. Unify Steam, Epic, Battle.net, Flatpak, AppImage and local apps in one fast, customizable library with custom launch options, Proton/Wine support, performance tools and managed stores.
=======
# Orbit Launcher

Launcher desktop local e modular para jogos e aplicativos Linux, desenvolvido para CachyOS/Arch, KDE Plasma e Wayland. O Orbit reúne descoberta local, compatibilidade Wine/Proton, lojas gerenciadas e distribuição nativa/AppImage.

## Desenvolvimento

No Arch/CachyOS, instale manualmente as dependências: `base-devel rust pnpm webkit2gtk-4.1 libappindicator-gtk3 librsvg`. O projeto nunca executa `sudo`.

```bash
pnpm install
pnpm tauri dev
```

Validação e build:

```bash
pnpm lint
pnpm typecheck
pnpm test
cargo test --manifest-path src-tauri/Cargo.toml
pnpm tauri build
```

Validação do produto:

```bash
scripts/validate-appimage.sh src-tauri/target/release/bundle/appimage/*.AppImage
scripts/clean-machine-smoke.sh src-tauri/target/release/orbit-launcher
```

Os dados ficam no diretório XDG de dados da aplicação, em SQLite. Não há conta, servidor ou telemetria.

## Distribuição

- PKGBUILD: `packaging/arch/PKGBUILD`
- AppImage e `.deb`: `pnpm tauri build`
- CI: lint, typecheck, testes, bundles e validação do AppImage
- instância única, tray KDE, autostart XDG e backup `.orbitbackup`
- atualizações AppImage por manifesto assinado

Consulte [o guia do usuário](docs/USER_GUIDE.md) e [o processo de release](docs/RELEASE.md).

## Recursos

- IDs estáveis (`steam:730`, `desktop:org.kde.kate`, `custom:<uuid>`)
- Steam Libraries adicionais por `libraryfolders.vdf`
- parsing seguro do `Exec` de arquivos `.desktop`
- argumentos e ambiente estruturados, sem `sh -c`
- rescan incremental que preserva preferências
- isolamento de falhas por provider
- scan paralelo com progresso por provider e diff transacional
- itens desinstalados preservados no histórico com `installed=false`
- sessões com PID, duração, exit code e tempo total
- execução opcional em Konsole/terminal configurado
- resolução offline de ícones Freedesktop
- migrations SQLite numeradas e atualização do banco legado
- painel de Steam, Epic, GOG e Battle.net com estratégias explícitas
- contratos de instalação, atualização, verificação e execução por provider
- inventário de SteamCMD, Legendary, Wine-GE e clientes encapsulados
- schema persistente para contas, componentes e fila de transferências
- bloqueio seguro de downloads sem manifesto de origem e integridade

## Segurança das integrações gerenciadas

Downloads de componentes ficam bloqueados enquanto não houver manifesto versionado com URL, checksum e assinatura confiáveis.

Credenciais nunca são solicitadas ou armazenadas no SQLite. A autenticação será feita pelo fluxo externo de cada provider e tokens de sessão serão entregues ao Secret Service/KWallet.

Consulte [ARCHITECTURE.md](ARCHITECTURE.md) e [DEVELOPMENT.md](DEVELOPMENT.md).
>>>>>>> f0a6795 (feat: initial Orbit launcher)
