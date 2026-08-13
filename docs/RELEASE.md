# Processo de release

1. Atualize a versão em `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` e `packaging/arch/PKGBUILD`.
2. Execute lint, typecheck, testes Rust/TypeScript e builds.
3. Valide o AppImage com `scripts/validate-appimage.sh`.
4. Execute `scripts/clean-machine-smoke.sh` com o binário release.
5. Gere `latest.json`, calcule SHA-256 do AppImage e assine o manifesto fora do repositório.
6. Publique AppImage, `.deb`, source tarball, manifesto e assinatura.
7. Substitua o `SKIP` do PKGBUILD pelo SHA-256 do source tarball antes de publicar no AUR.

Formato de `latest.json`:

```json
{"version":"0.2.0","url":"https://releases.example/Orbit.AppImage","sha256":"..."}
```

Assinatura:

```bash
openssl dgst -sha256 -sign update-private.pem -out latest.json.sig latest.json
```

A chave privada pertence ao cofre do CI e nunca ao repositório.

No Arch/CachyOS, o script `pnpm tauri` define `NO_STRIP=1`. Isso evita que o
`strip` antigo embutido no linuxdeploy rejeite as seções ELF `.relr.dyn` das
bibliotecas atuais da distribuição.
