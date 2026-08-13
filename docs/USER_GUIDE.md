# Guia do usuário — Orbit Launcher

## Instalação no CachyOS/Arch

O pacote nativo é a opção recomendada porque integra o Orbit ao KDE, ao Secret Service/KWallet e ao menu de aplicativos. Para construir localmente, copie `packaging/arch/PKGBUILD` para uma pasta vazia, substitua `sha256sums=('SKIP')` pelo checksum da release publicada e execute `makepkg -si` como usuário comum.

O AppImage é portátil. Marque-o como executável e execute-o:

```bash
chmod +x Orbit_Launcher-*.AppImage
./Orbit_Launcher-*.AppImage
```

## Primeira execução

Use o botão de atualização da biblioteca para detectar Steam, arquivos `.desktop`, Flatpaks e AppImages. Aplicativos personalizados podem ser cadastrados com “Adicionar”. Configurações de Proton, Wine, GameMode, MangoHud, Gamescope e Steam Overlay ficam no editor de cada jogo.

Fechar a janela mantém o Orbit na bandeja. Use “Sair” no menu da bandeja para encerrar. Se uma segunda instância for iniciada, ela apenas mostra e focaliza a primeira.

## Autostart

Em **Configurações → Geral**, ative “Iniciar com o sistema”. O Orbit cria um arquivo XDG em `~/.config/autostart` e inicia oculto na bandeja. Desativar a opção remove somente o arquivo criado pelo Orbit.

## Backup e restauração

Em **Configurações → Geral**:

1. “Exportar” cria um snapshot `.orbitbackup` consistente.
2. “Importar” verifica formato, origem e integridade SQLite antes da restauração.
3. Reinicie o Orbit após importar.

O backup contém biblioteca, configurações, histórico e fila. Jogos instalados, prefixes, caches, senhas e tokens não são incluídos. Credenciais permanecem no Secret Service/KWallet.

## Atualizações

AppImages podem receber atualização atômica quando o distribuidor configura `ORBIT_UPDATE_URL` e `ORBIT_UPDATE_PUBLIC_KEY`. Manifesto, assinatura e SHA-256 são verificados antes da substituição, e a versão anterior fica como `.old`.

Instalações por PKGBUILD devem ser atualizadas com o gerenciador de pacotes/AUR. O Orbit nunca sobrescreve `/usr/bin` nem chama `sudo`.

## Solução de problemas

- Logs por jogo: editor do item → “Abrir log de compatibilidade”.
- Steam Overlay: use Steam nativa, marque “Steam Overlay” e selecione Proton.
- Ícones locais: confirme que o arquivo ainda existe e faça um novo scan.
- Loja indisponível: confira o componente e o manifesto assinado em “Lojas e contas”.
- Wayland/Gamescope: consulte **Configurações → Compatibilidade**.

## Dados

O Orbit usa os diretórios XDG da aplicação. Não há telemetria. Senhas não são salvas no SQLite.
