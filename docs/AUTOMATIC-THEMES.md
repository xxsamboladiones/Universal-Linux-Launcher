# Temas automáticos

O Orbit pode gerar um tema declarativo a partir do wallpaper atual em **Configurações → Aparência → Temas**. Pywal, Pywal16 e `wal` são opcionais: quando disponíveis, o Orbit lê e valida `colors.json` em `$XDG_CACHE_HOME/wal/` (normalmente `~/.cache/wal/`); quando não estão disponíveis, o gerador nativo usa PNG, JPEG ou WebP diretamente.

O gerador limita o arquivo a 30 MB, redimensiona a análise para 96×96 pixels, usa SHA-256 para o cache em `$XDG_CACHE_HOME/orbit-launcher/themes/` e normaliza contraste para texto e controles. Não executa código nem scripts do wallpaper, de temas ou de arquivos Pywal.

Em KDE Plasma, o caminho é lido da configuração local do Plasma. Isso funciona independentemente de Wayland ou X11 porque a análise ocorre no arquivo. Se o Plasma não expuser um arquivo (por exemplo, plugins de slideshow), o Orbit preserva o último tema válido; sem cache, mantém o tema oficial padrão.

## Modos e atualização

- **Manual** aplica o tema escolhido no card e preserva essa escolha enquanto outros modos são usados.
- **Automático** gera e aplica a paleta imediatamente, sem reiniciar o Orbit.
- **Sistema** usa Orbit Light ou Orbit Dark conforme a preferência claro/escuro do desktop, sem apagar a escolha manual.

Ao selecionar **Pywal**, a atualização automática fica implícita. O backend observa somente o diretório `wal`, com debounce de 800 ms, e reage à substituição de `colors.json`; não há polling. A detecção procura `wal`, `pywal` e `pywal16` no `PATH`, mas também reconhece uma paleta válida existente. Isso cobre aplicativos iniciados pelo menu do Plasma, cujo `PATH` pode ser diferente do terminal.

O watcher apenas lê JSON declarativo com limite de 128 KiB. Ele nunca executa o comando Pywal, scripts, conteúdo do wallpaper ou valores vindos do arquivo. Paletas inválidas tentam o gerador nativo; se isso também falhar, a última paleta válida e depois Orbit Dark são os fallbacks.
