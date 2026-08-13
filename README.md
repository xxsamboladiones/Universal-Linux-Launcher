# Orbit Launcher

Launcher universal de jogos e aplicativos para CachyOS, Arch Linux, KDE Plasma e Wayland. O Orbit reúne biblioteca local, compatibilidade Wine/Proton, ferramentas de desempenho e provedores de lojas em uma interface única.

Versão atual: **0.1.2**

## Novidades da **v0.1.2-alpha**

Grande atualização de estabilidade e infraestrutura: melhorias na integração Epic/SteamCMD, fila de downloads e instalações, progresso em tempo real, desinstalação, sincronização assíncrona, SQLite, suporte a JAR, ícones Windows/Freedesktop e diversos ajustes de concorrência, segurança e testes.


## Novidades da 0.1.1

- correção automática da janela branca do WebKitGTK em GPUs NVIDIA;
- detecção de NVIDIA antes da criação da webview, com fallback seguro para o renderizador DMA-BUF;
- instalação automática de SteamCMD, Legendary e GOGDL a partir de receitas confiáveis embutidas;
- downloads retomáveis, checksum SHA-256, staging atômico e rollback;
- progresso por eventos durante download, verificação e instalação;
- preparação de provedores em segundo plano, sem bloquear a interface;
- correção do SteamCMD que permanecia no prompt interativo durante a leitura da versão;
- estado Steam conectado vinculado à sessão SteamCMD gerenciada pelo Orbit.

## Recursos

- biblioteca unificada para Steam, arquivos `.desktop`, Flatpak, AppImage e itens personalizados;
- scan paralelo e incremental que preserva favoritos, itens ocultos e histórico;
- edição de executável, argumentos, ambiente e diretório de trabalho;
- seleção de Proton/Wine e prefixo por jogo;
- GameMode, MangoHud, Gamescope, DXVK e VKD3D;
- suporte a jogos não Steam com Steam Overlay;
- painel para Steam, Epic Games, GOG e Battle.net;
- fila persistente de instalações e atualizações;
- Secret Service/KWallet para credenciais e sessões;
- bandeja, instância única, autostart, backup e restauração;
- pacotes AppImage, DEB e PKGBUILD para Arch/CachyOS.

## NVIDIA e AppImage

O Tauri usa WebKitGTK no Linux. Em algumas combinações de WebKitGTK e driver NVIDIA, a importação do framebuffer DMA-BUF falha e a janela fica branca mesmo com o frontend carregado. O Orbit detecta NVIDIA pelo driver, módulo do kernel ou identificador DRM e aplica `WEBKIT_DISABLE_DMABUF_RENDERER=1` antes de inicializar GTK/WebKit.

A mitigação é seletiva e não desativa a aceleração gráfica dos jogos. Para diagnóstico:

```bash
# Reativa o caminho DMA-BUF do WebKitGTK
ORBIT_ENABLE_DMABUF_RENDERER=1 ./orbit-launcher.AppImage

# Último recurso: desativa a composição acelerada da webview
ORBIT_WEBKIT_SOFTWARE=1 ./orbit-launcher.AppImage
```

Referências upstream: [Tauri — Linux Graphics Issues](https://v2.tauri.app/develop/debug/linux-graphics/) e [WebKitGTK bug 281279](https://bugs.webkit.org/show_bug.cgi?id=281279).

## Lojas gerenciadas

O botão **Preparar suporte** baixa e instala automaticamente os componentes suportados no diretório privado do Orbit:

- Steam: SteamCMD, autenticação interativa e Steam Guard;
- Epic Games: Legendary;
- GOG: GOGDL;
- Battle.net: cliente oficial encapsulado em Wine quando os componentes verificáveis estiverem disponíveis.

O SteamCMD não substitui o Steam Desktop em todos os jogos. Steamworks, DRM e Steam Overlay ainda podem exigir o cliente oficial.

## Desenvolvimento

No Arch/CachyOS, instale `base-devel`, `rust`, `pnpm`, `webkit2gtk-4.1`, `libappindicator-gtk3` e `librsvg`. O projeto nunca executa `sudo`.

```bash
pnpm install
pnpm tauri dev
```

Validação:

```bash
pnpm lint
pnpm typecheck
pnpm test
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
pnpm tauri build
```

Validação dos artefatos:

```bash
scripts/validate-appimage.sh src-tauri/target/release/bundle/appimage/*.AppImage
scripts/clean-machine-smoke.sh src-tauri/target/release/orbit-launcher
```

## Distribuição

- PKGBUILD: [`packaging/arch/PKGBUILD`](packaging/arch/PKGBUILD)
- desktop entry: [`packaging/linux/io.orbit.launcher.desktop`](packaging/linux/io.orbit.launcher.desktop)
- AppImage e DEB: `pnpm tauri build`
- CI: lint, typecheck, testes e bundles
- atualização AppImage por manifesto assinado

## Segurança e dados

Componentes gerenciados possuem URL HTTPS e SHA-256 conhecidos. Manifestos externos somente podem substituir as receitas internas quando acompanhados de assinatura válida. Arquivos são preparados em staging antes da troca atômica, e a versão anterior permanece disponível para rollback.

Senhas não são armazenadas no SQLite. Tokens usam Secret Service/KWallet. O Orbit não possui telemetria e mantém seus dados nos diretórios XDG da aplicação.

Consulte o [guia do usuário](docs/USER_GUIDE.md), a [arquitetura](ARCHITECTURE.md), o [guia de desenvolvimento](DEVELOPMENT.md) e o [processo de release](docs/RELEASE.md).
