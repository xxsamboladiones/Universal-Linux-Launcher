# Arquitetura

O fluxo é `React UI → comandos Tauri → application services → core/database → providers/Linux`. Componentes nunca acessam diretamente arquivos, processos ou SQLite.

## Providers

`LibraryProvider` expõe disponibilidade e scan. Steam e Desktop Entry são implementações independentes; uma falha vira diagnóstico e não interrompe as demais. `CustomProvider` representa registros controlados pelo usuário. Novas integrações implementam o mesmo contrato.

## Banco

SQLite guarda itens, preferências e sessões. Migrations são aplicadas em transações e numeradas por `PRAGMA user_version`. Cada resultado de provider é aplicado em uma transação: itens presentes são inseridos/atualizados e ausentes passam a `installed=false`, preservando favoritos, ocultação e histórico.

## Launch pipeline

`LibraryItem → provider resolution → LaunchSpec → validation → Command::spawn → session → ProcessManager`. `LaunchSpec` separa executável, lista de argumentos, ambiente e diretório. O `ProcessManager` acompanha o filho iniciado, registra duração/exit code e atualiza o tempo total. O frontend não recebe um endpoint arbitrário de shell.

## Descoberta local

Steam, Desktop Entry, Flatpak e AppImage implementam `LibraryProvider`. Scanners executam em threads independentes e publicam `scan-progress` por eventos Tauri. Escritas no banco ocorrem depois do scan, evitando manter o lock SQLite durante I/O externo.

Wrappers futuros transformarão um `LaunchSpec` em outro, permitindo GameMode, Gamescope e MangoHud compostos sem combinações hardcoded.

## Store control plane

O launcher agora separa scanners locais de `GameProvider`. Um provider de loja possui autenticação, biblioteca e geração de especificações estruturadas para instalar, atualizar, verificar e iniciar. A UI chama a mesma operação independentemente da loja.

```text
StoreProvider ── DependencyManager ── ProviderCommand
       │                  │                    │
       └── account        └── manifest         └── operation queue
                              verificado
```

- Steam é híbrido: SteamCMD administra SteamPipe, mas jogos podem exigir Steam Desktop por DRM/Steamworks.
- Epic usa um adaptador Legendary e autenticação externa; nenhuma senha passa pelo Orbit.
- Battle.net é modelado como cliente oficial gerenciado dentro de prefixo Wine.
- GOG possui contrato e estado, mas nenhuma falsa integração de download foi habilitada.

`DependencyManager` só reconhece binários do sistema ou do diretório privado da aplicação. Uma instalação gerenciada exige manifesto com origem e integridade. Nesta versão, a ausência do manifesto bloqueia a ação com erro tipado; não há fallback para script ou shell.

SQLite possui tabelas independentes para contas, dependências e transferências. Segredos não pertencem a essas tabelas: sessões futuras devem usar Secret Service, normalmente fornecido pelo KWallet no KDE.

## Runtime manager

Runtimes são identificados por família, versão, origem e estado. O layout reservado é:

```text
$XDG_DATA_HOME/io.orbit.launcher/
  providers/<dependency>/bin/
  runtimes/proton/<version>/
  runtimes/wine/<version>/
  prefixes/<provider-or-game>/
  games/<provider>/
  manifests/<dependency>.json
```

O próximo passo para downloads reais é implementar verificação criptográfica, retomada atômica e consentimento de espaço em disco antes de habilitar os manifestos.

## Limite frontend/backend

O frontend invoca somente comandos específicos: listar, escanear, iniciar, editar, favoritar e ocultar. Capabilities Tauri concedem apenas permissões core à janela principal.
