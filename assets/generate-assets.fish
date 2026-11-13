#!/usr/bin/env fish

# ==============================================================================
# KESTO - Asset Generator Script (Fish Shell)
# Genera todos los recursos gráficos para la Web (Favicons/PWA) y el README.md
# ==============================================================================

function print_step --argument-names msg
    set_color --bold cyan
    echo "==> $msg"
    set_color normal
end

function print_success --argument-names msg
    set_color --bold green
    echo "  ✔ $msg"
    set_color normal
end

function print_error --argument-names msg
    set_color --bold red
    echo "  ✖ $msg"
    set_color normal
end

# 1. Validación de Entrada
set -l INPUT_SVG $argv[1]

if test -z "$INPUT_SVG"
    if test -f "icon.svg"
        set INPUT_SVG "icon.svg"
    else
        print_error "Uso: ./generate-assets.fish <archivo.svg>"
        echo "O bien coloca un archivo 'icon.svg' en el directorio actual."
        exit 1
    end
end

if not test -f "$INPUT_SVG"
    print_error "El archivo '$INPUT_SVG' no existe."
    exit 1
end

# 2. Selección del Motor de Renderizado SVG
set -l RENDERER ""

if type -q resvg
    set RENDERER "resvg"
else if type -q inkscape
    set RENDERER "inkscape"
else if type -q rsvg-convert
    set RENDERER "rsvg-convert"
else if type -q magick
    set RENDERER "magick"
else
    print_error "Se requiere una herramienta para convertir SVG: install resvg, inkscape, rsvg-convert o imagemagick."
    exit 1
end

print_step "Usando motor de renderizado SVG: $RENDERER"

# Función interna de conversión SVG -> PNG
function render_svg --argument-names input_file width output_file engine
    switch $engine
        case "resvg"
            resvg -w $width $input_file $output_file >/dev/null 2>&1
        case "inkscape"
            inkscape --export-type=png -w $width -o $output_file $input_file >/dev/null 2>&1
        case "rsvg-convert"
            rsvg-convert -w $width -h $width $input_file -o $output_file
        case "magick"
            magick -background none -density 300 $input_file -resize {$width}x{$width} $output_file
    end
end

# 3. Creación de Directorios de Salida
set -l WEB_DIR "public/assets"
set -l DOCS_DIR "docs/assets"

mkdir -p $WEB_DIR
mkdir -p $DOCS_DIR

# ==============================================================================
# A. Assets para la Web (Favicons, PWA y Apple Icons)
# ==============================================================================
print_step "Generando assets para la Web (React / Antd Dashboard)..."

set -l WEB_SIZES 16 32 48 180 192 512

for size in $WEB_SIZES
    set -l target_file ""
    switch $size
        case 16
            set target_file "$WEB_DIR/favicon-16x16.png"
        case 32
            set target_file "$WEB_DIR/favicon-32x32.png"
        case 48
            set target_file "$WEB_DIR/favicon-48x48.png"
        case 180
            set target_file "$WEB_DIR/apple-touch-icon.png"
        case 192
            set target_file "$WEB_DIR/android-chrome-192x192.png"
        case 512
            set target_file "$WEB_DIR/android-chrome-512x512.png"
    end

    render_svg $INPUT_SVG $size $target_file $RENDERER
    print_success "Creado: $target_file ({$size}x{$size}px)"
end

# Generación del archivo favicon.ico multi-resolución (16x16, 32x32, 48x48)
if type -q magick
    print_step "Generando favicon.ico multi-resolución..."
    magick $WEB_DIR/favicon-16x16.png $WEB_DIR/favicon-32x32.png $WEB_DIR/favicon-48x48.png $WEB_DIR/favicon.ico
    print_success "Creado: $WEB_DIR/favicon.ico"
else
    # Fallback copiar la versión de 32px como .ico si no está ImageMagick
    cp $WEB_DIR/favicon-32x32.png $WEB_DIR/favicon.ico
end

# Copiar el SVG original optimizado a la carpeta pública
cp $INPUT_SVG $WEB_DIR/icon.svg
print_success "Copiado: $WEB_DIR/icon.svg"

# ==============================================================================
# B. Assets para la Documentación y README.md
# ==============================================================================
print_step "Generando assets para la documentación y README.md..."

# 1. Logo Isotipo grande para Header de GitHub / Redes (512x512)
render_svg $INPUT_SVG 512 "$DOCS_DIR/logo-512.png" $RENDERER
print_success "Creado: $DOCS_DIR/logo-512.png"

# 2. Logo Isotipo mediano para README / Antd Sidebar Header (128x128)
render_svg $INPUT_SVG 128 "$DOCS_DIR/logo-128.png" $RENDERER
print_success "Creado: $DOCS_DIR/logo-128.png"

# 3. Logo Isotipo pequeño para badges / tablas (64x64)
render_svg $INPUT_SVG 64 "$DOCS_DIR/logo-64.png" $RENDERER
print_success "Creado: $DOCS_DIR/logo-64.png"

# Copia del SVG original a la carpeta de docs
cp $INPUT_SVG $DOCS_DIR/logo.svg
print_success "Copiado: $DOCS_DIR/logo.svg"

# ==============================================================================
# C. Plantilla HTML y Markdown para incluir en el proyecto
# ==============================================================================
print_step "Estructura final de archivos generados:"

echo "
📂 public/assets/ (Web / React / PWA)
├── favicon.ico
├── favicon-16x16.png
├── favicon-32x32.png
├── apple-touch-icon.png (180x180)
├── android-chrome-192x192.png
├── android-chrome-512x512.png
└── icon.svg

📂 docs/assets/ (README.md)
├── logo.svg
├── logo-512.png
├── logo-128.png
└── logo-64.png
"

set_color --bold green
echo "¡Proceso completado con éxito para Kesto!"
set_color normal
