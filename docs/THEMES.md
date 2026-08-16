# Temas do Orbit

Um tema é um pacote declarativo `.orbit-theme` (ZIP). Ele não pode incluir CSS, JavaScript, HTML, binários ou scripts; o Orbit aceita somente metadados, tokens, imagens e fontes permitidas.

Veja também [Temas automáticos](AUTOMATIC-THEMES.md): Pywal é opcional e o Orbit possui um gerador nativo de paletas.

## Estrutura

```text
meu-tema.orbit-theme
├── manifest.json
├── theme.json
├── preview.png            # opcional: PNG, WebP, JPG
├── assets/                # imagens PNG, WebP, JPG opcionais
└── fonts/                 # WOFF, WOFF2 ou TTF opcionais
```

`manifest.json` usa `schemaVersion: 1` e deve conter `id`, `name`, `version` (SemVer), `author`, `description`, `type` (`dark` ou `light`), `orbitVersion`, `entry` (`theme.json`) e, opcionalmente, `preview`.

`theme.json` contém somente os grupos `colors`, `radius`, `spacing`, `typography` e `effects`. Eles são convertidos para variáveis como `--orbit-color-background`, `--orbit-color-primary`, `--orbit-color-accent`, `--orbit-color-on-primary`, `--orbit-radius-medium` e `--orbit-font-family`. Cores aceitam hexadecimal; medidas aceitam `px`, `rem`, `em` e `%`.

Os campos opcionais `accent`, `primaryForeground`, `secondaryForeground` e `accentForeground` foram adicionados de forma retrocompatível. Temas `schemaVersion: 1` existentes continuam válidos: quando esses campos não existem, o Orbit usa `primary` ou `text` como fallback. O gerador automático sempre produz as cores de foreground normalizadas para contraste.

## Instalação e exportação

Em **Configurações → Aparência**, use **Importar tema** para escolher um `.orbit-theme`. Temas externos ficam em `$XDG_DATA_HOME/orbit-launcher/themes/installed` (ou `~/.local/share/orbit-launcher/themes/installed`). Eles podem ser exportados ou removidos nessa mesma tela. Temas internos não são removíveis nem exportáveis.

## Segurança e compatibilidade

Antes da instalação, o Orbit limita o pacote a 20 MB descompactados e 128 arquivos; imagens têm limite de 8 MB e fontes 4 MB. Caminhos absolutos, `..`, symlinks, arquivos executáveis e extensões não permitidas são rejeitados. O manifesto também é validado contra a versão atual do Orbit. A seleção persistida guarda apenas o ID do tema; se ele não existir mais, o Orbit restaura `Orbit Dark`.

Versões futuras poderão introduzir `schemaVersion` 2 ou 3. Uma versão desconhecida é rejeitada, em vez de ser interpretada de modo inseguro ou quebrar temas existentes.
