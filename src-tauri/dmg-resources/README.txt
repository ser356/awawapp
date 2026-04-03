# awawapp - Instalación

## Contenido del DMG

- **awawapp.app** - La aplicación principal
- **fix-quarantine.sh** - Script para arreglar el error de "app dañada"
- **README.txt** - Este archivo

## Instalación

1. Arrastra `awawapp.app` a tu carpeta `/Applications`

2. Intenta abrir la app haciendo doble clic

## Si aparece el error "está dañada y no se puede abrir"

macOS bloquea aplicaciones descargadas de internet que no están firmadas 
con un certificado de Apple Developer (cuesta $99/año).

Para solucionarlo:

### Opción 1: Usar el script incluido

1. Abre Terminal (búscalo en Spotlight con Cmd+Space)

2. Arrastra el archivo `fix-quarantine.sh` a la ventana de Terminal

3. Presiona Enter

4. Ya puedes abrir awawapp normalmente

### Opción 2: Comando manual

Abre Terminal y ejecuta:

```
xattr -cr /Applications/awawapp.app
```

### Opción 3: Click derecho

1. Click derecho sobre awawapp.app
2. Selecciona "Abrir"
3. En el diálogo, click en "Abrir" de nuevo

(Esta opción puede no funcionar en todas las versiones de macOS)

## ¿Qué hace el script fix-quarantine.sh?

El script ejecuta el comando `xattr -cr` que elimina los atributos extendidos
de cuarentena que macOS añade a los archivos descargados de internet.

Específicamente, elimina el atributo `com.apple.quarantine` que es el que
causa que Gatekeeper bloquee la aplicación.

El comando es seguro y solo afecta a awawapp.app.

## Más información

- Repositorio: https://github.com/user/awawapp
- Reportar problemas: https://github.com/user/awawapp/issues
