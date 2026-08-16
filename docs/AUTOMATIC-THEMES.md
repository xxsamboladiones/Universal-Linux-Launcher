# Temas automáticos

O Orbit pode gerar um tema declarativo a partir do wallpaper atual em **Configurações → Aparência → Temas**. Pywal, Pywal16 e `wal` são opcionais: quando disponíveis, o Orbit lê e valida a paleta em `~/.cache/wal/colors.json`; quando não estão disponíveis, o gerador nativo usa PNG, JPEG ou WebP diretamente.

O gerador limita o arquivo a 30 MB, redimensiona a análise para 96×96 pixels, usa SHA-256 para o cache em `$XDG_CACHE_HOME/orbit-launcher/themes/` e normaliza contraste para texto e controles. Não executa código nem scripts do wallpaper, de temas ou de arquivos Pywal.

Em KDE Plasma, o caminho é lido da configuração local do Plasma. Isso funciona independentemente de Wayland ou X11 porque a análise ocorre no arquivo. Se o Plasma não expuser um arquivo (por exemplo, plugins de slideshow), o Orbit preserva o último tema válido e permite voltar ao tema manual.
