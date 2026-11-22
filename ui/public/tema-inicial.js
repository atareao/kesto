/*
 * Tema antes de pintar: oscuro por defecto, o la elección guardada. Ajusta ya
 * el fondo del documento y el `theme-color` para que no haya destello ni
 * barra del navegador desalineada. No sigue la preferencia del sistema.
 *
 * Se carga como script clásico síncrono desde el `<head>` (no `defer` ni
 * `type="module"`), antes del primer pintado. Vive en un fichero propio —y no
 * en línea— para que el servidor pueda servir `script-src 'self'`.
 */
;(function () {
  var tema = 'oscuro'
  try {
    if (window.localStorage.getItem('kesto-tema') === 'claro') tema = 'claro'
  } catch (error) {
    // Sin almacenamiento, el defecto oscuro.
  }
  document.documentElement.dataset.tema = tema
  var colorDelTema = tema === 'claro' ? '#F8FAFC' : '#0D1B2A'
  var meta = document.querySelector('meta[name="theme-color"]')
  if (meta) meta.setAttribute('content', colorDelTema)
})()
