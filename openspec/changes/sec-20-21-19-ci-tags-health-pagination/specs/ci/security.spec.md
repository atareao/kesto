# Spec Delta — SEC-20: Acciones de GitHub Actions fijadas a SHA (pin inmutable)

## Contexto

Parte del change `sec-20-21-19-ci-tags-health-pagination` (refs #168).
Hoy los workflows en `.github/workflows/*.yml` referencian acciones por tag mutable (`@v4`, `@main`, `@master`, etc.).
Un mantenedor malintencionado o un compromiso de la cuenta de la acción puede retaguear el tag a código malicioso (supply-chain attack).

## Contrato (POST-change)

Cada directiva `uses:` en **todos** los workflows del repositorio (`ci.yml`, `release.yml`, `security.yml`, etc.) debe cumplir:

```
uses: owner/repo@<sha40>  # <tag-original>
```

- `<sha40>`: hash de commit completo de 40 caracteres hexadecimales (lowercase), apuntando al commit exacto que se quiere ejecutar.
- `# <tag-original>`: comentario YAML en la misma línea con el tag original (p.ej. `v4`, `v3.1.0`, `main`) para legibilidad y auditoría.
- **Prohibido** cualquier `uses:` que no contenga un SHA de 40 hex seguido opcionalmente de comentario.

### Ejemplos

```yaml
# Antes (inseguro)
uses: actions/checkout@v4
uses: docker/login-action@v3
uses: rust-lang/rustup-action@master

# Después (seguro)
uses: actions/checkout@11bd71901bbe5b1630ceea73d27597364c9af683  # v4
uses: docker/login-action@74a5d13c0a8f1d2c4b5e6f7a8b9c0d1e2f3a4b5c  # v3
uses: rust-lang/rustup-action@f3e2d1c0b9a8f7e6d5c4b3a2f1e0d9c8b7a6f5e4  # main (2025-06-15)
```

## Alcance

- Todos los archivos bajo `.github/workflows/*.yml` (y `.yaml`).
- Incluye actions oficiales (`actions/*`), de terceros (`docker/*`, `rust-lang/*`, `cargo/*`, etc.) y actions locales (`./.github/actions/...` si las hubiera).
- **No** incluye actions referenciadas desde `uses:` dentro de actions compuestas del propio repo (si las hubiera, también se pintan).

## Escenarios BDD

### Escenario 1 — Happy path: workflow con todas las actions pinedas a SHA
**Dado** un archivo `.github/workflows/ci.yml`  
**Cuando** se inspecciona cada paso `uses:`  
**Entonces** cada uno tiene la forma `owner/repo@<sha40> # <tag>`  
**Y** `<sha40>` coincide con `^[0-9a-f]{40}$`  
**Y** el comentario `# <tag>` está presente y refleja el tag original usado antes del pin

### Escenario 2 — Edge case: action referenciada por SHA ya en el origen
**Dado** un `uses:` que ya venía como `owner/repo@abcdef1234567890...` (40 hex)  
**Cuando** se revisa el workflow  
**Entonces** se **conserva** el SHA; se **añade** el comentario `# <tag>` si se conoce, o `# <sha corto>` si no  
**Y** no se modifica el SHA

### Escenario 3 — Edge case: action local (composite) referenciada por path
**Dado** `uses: ./.github/actions/minha-action`  
**Cuando** se revisa  
**Entonces** **no** se modifica (las actions locales por path no usan tag/SHA remoto)  
**Y** se excluye de la comprobación de pin

### Escenario 4 — Error: `uses:` sin SHA (tag mutable)
**Dado** un workflow con `uses: actions/checkout@v4` (sin pin)  
**Cuando** se ejecuta la comprobación automática (test de `scripts/` o job de CI)  
**Entonces** la comprobación **falla** y reporta el archivo, la línea y el `uses:` ofensivo  
**Y** el mensaje indica "uses must be pinned to a 40-char commit SHA"

### Escenario 5 — Error: `uses:` con SHA truncado (short SHA)
**Dado** `uses: actions/checkout@abc123` (6-7 hex)  
**Cuando** se ejecuta la comprobación  
**Entonces** la comprobación **falla** (short SHA es mutable igual que tag)

### Escenario 6 — Edge case: `uses:` con `@` en el nombre de la action (raro)
**Dado** `uses: owner/repo-with@symbol@v1` (sintaxis válida pero confusa)  
**Cuando** se parsea  
**Entonces** la comprobación identifica correctamente la parte después del último `@` como ref y exige SHA de 40 hex

### Escenario 7 — Comportamiento funcional inalterado
**Dado** el pin a SHA aplicado  
**Cuando** se ejecuta el workflow en CI  
**Entonces** los pasos se comportan **exactamente igual** que con el tag original (mismo código, misma versión)  
**Y** no hay regresiones en tests, build, release, security scans

## Comprobación automática (gate)

- Añadir/activar un script/job (p.ej. `scripts/check-action-pins.sh` o step en `ci.yml`) que:
  1. Recorre `.github/workflows/*.yml`.
  2. Extrae cada `uses:` (excluyendo paths locales `./...`).
  3. Verifica que la ref después del último `@` matchea `^[0-9a-f]{40}$`.
  4. Falla con código ≠ 0 y lista de ofensores si hay incumplimientos.
- El gate corre en `just check-all` y en CI (`check-action-pins` job).

## Procedimiento de actualización de pins (documentación viva)

Cuando se quiera actualizar una action a una versión nueva:
1. Ir al repo de la action, elegir el tag/commit objetivo.
2. Copiar el SHA completo del commit (40 hex).
3. Sustituir el SHA en el workflow y actualizar el comentario `# <nuevo-tag>`.
4. Ejecutar `just check-all` y verificar que el gate pasa.
5. Commit con mensaje `🔧 ci: pin <action> to <sha> (# <tag>)`.

## Notas de implementación

- No cambia el comportamiento de runtime; solo endurece la cadena de suministro.
- Mantener el comentario `# <tag>` alinea con la recomendación de GitHub y facilita Dependabot/Renovate futuros.
- Revisar también `release.yml`, `security.yml` y cualquier workflow adicional.
- Actualizar `GIT_FLOW.md` con la sección "Actualización de pins de actions".
- El spec delta no toca código de aplicación ni tests unitarios de Rust/TS; solo los workflows y su gate.