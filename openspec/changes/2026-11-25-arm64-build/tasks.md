# Checklist TDD para soporte arm64

- [ ] Verificar que el target `aarch64-unknown-linux-gnu` está instalado.
- [ ] Verificar que el enlazador cruzado está disponible.
- [ ] Añadir receta `build-arm64` al `justfile`.
- [ ] Crear script `scripts/check-arm64.sh` que falle si la compilación no tiene éxito.
- [ ] Añadir matriz arm64 en `.github/workflows/ci.yml`.
- [ ] Parametrizar `runs-on` y `TARGET` en la matriz.
- [ ] Mantener puertas de calidad (`cargo fmt`, `cargo clippy`, `cargo test`, `cargo deny`) para ambos targets.
- [ ] Si no hay runner arm64 nativo, documentar uso de QEMU y marcar job como no bloqueante.
- [ ] Actualizar Dockerfile para usar base oficial arm64 y `buildx`.
- [ ] Verificar que `pnpm install` y `pnpm run build` funcionan en arm64.
- [ ] Documentar pasos en `docs/FEATURE-052-arm64.md` y `docs/ENTORNO.md`.
- [ ] Añadir estado a `docs/ESTADO.md`.
- [ ] Referenciar el issue #129 y este plan en la documentación.
