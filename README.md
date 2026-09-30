# DevHQ

Centro de operaciones **nativo de escritorio** para un ecosistema personal de software.
Su prioridad es responder **«¿Qué necesita mi atención?»**: observar aplicaciones,
recordar eventos, relacionar infraestructura y verificar resultados de automatizaciones.

> Si una función solo ahorra escribir un comando sencillo, probablemente no pertenece
> a DevHQ. Si observa, recuerda, relaciona, detecta problemas o automatiza un proceso
> completo, sí puede pertenecer.

DevHQ no reemplaza VS Code, una terminal, GitHub Desktop, Docker Desktop, Codex ni
Claude Code. Ya conserva inventario local, pero **aún no observa ni opera sistemas reales**.

## Qué incluye

- Overview centrado en atención, incidencias, actividad y contexto operativo.
- Applications con búsqueda, filtros, identidad visual y detalle con seis tabs.
  Solo Overview tiene contenido completo; las restantes explican su futura función.
- Infrastructure compartida con enlaces a las aplicaciones que dependen de ella,
  incidencias del recurso y enlaces desde los componentes de cada aplicación.
- Incidents con severidad, estado, contexto desplegable y filtros; Activity global.
- Previews de Automations (Trigger → Condition → Action → Verification), Security
  y Settings informativos.

El modo **Demo** utiliza un escenario local fijo del **24/09/2026, 16:40 Europe/Madrid**.
Los tiempos relativos se calculan frente a ese instante, no frente al reloj real.
Los estados unknown/no observado no implican salud ni seguridad. Lifecycle y health
son independientes. Una incidencia de un recurso también aparece en las aplicaciones
que lo utilizan; su actividad también aparece en el contexto de esas aplicaciones.
La copia de ejemplo ocurre a las 08:40 y se verifica a las 08:45; los resultados
de automatización son parte del escenario, no ejecuciones de DevHQ. Los logos y
thumbnails pueden sustituirse mediante campos del modelo. En Tauri se abre por defecto
**Mi inventario**, que usa SQLite; la demo se elige explícitamente en la barra lateral.
La preview web solo muestra la demo porque no dispone de IPC nativo. Los mocks nunca
se importan a SQLite. En el inventario real, aplicaciones, componentes y recursos
aparecen como **no observados**; lifecycle no expresa health.

Desde Mi inventario se crean y editan recursos y aplicaciones. Cada aplicación
contiene componentes que pueden apuntar a un recurso existente. Al guardar una
aplicación se reemplaza su lista de componentes de forma atómica: quitar un
componente también quita su vínculo. La eliminación de aplicaciones o recursos
completos queda fuera de este incremento. Un recurso inexistente se rechaza antes
de escribir la aplicación. Un fallo de SQLite/IPC muestra un error y permite reintentar.
No se conservan observaciones, incidencias ni actividad reales todavía.

## Ejecutar

Requisitos: Node.js 24 LTS, npm, Rust estable y los
[prerrequisitos de Tauri 2](https://v2.tauri.app/start/prerequisites/).
En Windows: Visual Studio Build Tools con C++ y Windows SDK, además de WebView2.

```sh
npm ci
npm run tauri dev          # escritorio nativo
npm run dev               # preview web: http://127.0.0.1:1420
npm run check             # ESLint, tests, TypeScript, Vite y cargo check
cargo test --manifest-path src-tauri/Cargo.toml --lib # persistencia y migraciones
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
npm run tauri build       # ejecutable e instalador NSIS para Windows
```

El instalador se genera en `src-tauri/target/release/bundle/nsis/`. El target de
empaquetado está configurado para Windows; otros sistemas necesitan sus prerrequisitos
y seleccionar un target de bundle apropiado. Los lockfiles de npm y Cargo se versionan.

## Arquitectura

```text
src/
  app/             Shell, rutas y navegación
  features/        Vistas y componentes por área del producto
  components/ui/   Primitivas shadcn/ui basadas en Radix
  components/      Presentación compartida: estados, logos, enlaces, encabezados
  domain/          Modelos, vocabulario y selectores puros
  data/mock/       Escenario tipado y relaciones por ID
  shared/          Utilidades de estilos y tiempo
  test/            Configuración de pruebas
src-tauri/         Host Rust/Tauri 2, SQLite, migraciones, IPC, ventana y CSP
```

React + TypeScript estricto + Vite + Tailwind CSS 4. El estado de filtros es local;
las rutas y tabs usan React Router con hash, compatible con el protocolo empaquetado
de Tauri. No hay servidor HTTP de aplicación, store global ni capa de repositorios
artificial. Los selectores de dominio no dependen de React ni de los mocks. Las vistas
consumen explícitamente el escenario durante esta etapa.

Rust expone solo `list_inventory`, `save_application` y `save_resource`. Abre
`inventory.sqlite3` en el directorio de datos de la aplicación que proporciona
Tauri; las migraciones transaccionales v1→v2 usan `PRAGMA user_version` y las
relaciones aplican claves foráneas. La UI invoca esos casos de uso con contratos
tipados. Se conserva la barra nativa de ventana. La CSP limita recursos y las
capabilities no conceden permisos adicionales. No se implementan autenticación,
cuentas, cloud, sync, telemetría, scanners, automation engine, Runner ni background service.

## Evolución prevista

1. Observaciones, eventos e incidencias persistentes, distinguiendo salud desconocida,
   observaciones caducadas y fallos observados.
2. Una primera integración acotada y verificable. Solo después, añadir supervisión
   cancelable, automatizaciones con resultado e historial, y seguridad con almacén
   nativo de credenciales y evidencias contextualizadas.

Estos son límites conceptuales, **no módulos vacíos ni integraciones implementadas**.
La dirección es UI → IPC/casos de uso → dominio. SQLite conserva contexto; las credenciales no deben
vivir en componentes React, localStorage ni texto plano en SQLite.

Las pruebas cubren navegación, tabs/teclado, filtros, rutas desconocidas, relaciones
compartidas, cronología de la demo, salud desconocida, persistencia tras reapertura,
rechazo de referencias inválidas y actualización de esquema.
