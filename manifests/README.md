# Manifestos de dependências do Orbit

Cada dependência exige três elementos no diretório de dados do aplicativo em `manifests/`:

- `orbit-dependencies.pem`: chave pública da release do Orbit;
- `<id>.json`: manifesto canônico;
- `<id>.json.sig`: assinatura RSA/SHA-256 binária do manifesto.

Formato:

```json
{
  "id": "legendary",
  "version": "0.20.x",
  "url": "https://origem-oficial/artefato",
  "sha256": "64 caracteres hexadecimais",
  "executable": "bin/legendary",
  "archive": "tar.gz"
}
```

O Orbit verifica a assinatura antes da rede, retoma em `.part`, valida SHA-256, extrai em `.staging-*` e só então troca `current` atomicamente. A versão anterior permanece em `rollback`.

As chaves privadas nunca devem fazer parte do repositório ou do pacote do aplicativo.
