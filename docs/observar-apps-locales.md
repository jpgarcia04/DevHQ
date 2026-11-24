# Observar aplicaciones locales (diseño)

Estado: **el paso 1 está implementado (14 nov 2026, esquema v14)**; los pasos 2 y 3 siguen siendo una
propuesta, sin código. Este documento explica por qué y recoge lo que se decidió; la especificación de
lo que existe es el [README, hoy la especificación](especificacion.md) («Instalaciones»), y cada regla que se cite sigue vigente
hasta que un cambio la reescriba allí. Lo que el paso 1 terminó siendo, y en qué se apartó de lo que
se propone abajo, está en [«Lo decidido al implementar el paso 1»](#lo-decidido-al-implementar-el-paso-1).
Los sistemas y rutas de los ejemplos son inventados.

## El problema

DevHQ solo sabe producir evidencia de una forma: una comprobación HTTP(S). Sirve para un servicio que
está siempre en marcha, pero no para dos tipos de aplicación que son habituales en un ecosistema
personal:

- **Aplicaciones bajo demanda.** Por ejemplo, «Cuentas Claras», una app web local que solo corre cuando
  la persona la abre y se apaga sola al cerrar la última pestaña. Una comprobación periódica registraría
  un fallo la mayor parte del día. DevHQ no distingue «cerrada a propósito» de «abierta y fallando».
- **Aplicaciones de escritorio sin servidor.** Por ejemplo, «Atlas Notes», instalada con MSI, sin
  endpoint que preguntar. Hoy no hay forma de observarla.

El resultado es que una app así queda para siempre en «Not observed», y «¿qué necesita mi atención?»
responde «todo sin observar» aunque todo esté bien. Hay tres preguntas que hoy no tienen respuesta:

| Pregunta                                                  | Qué responde hoy DevHQ                      |
| --------------------------------------------------------- | ------------------------------------------- |
| ¿Está funcionando ahora?                                  | «No se sabe»                                |
| ¿Qué versión tengo instalada y desde cuándo?              | La versión declarada, si alguien la mantuvo |
| ¿Qué hay en el repositorio que todavía no está instalado? | «DevHQ no lo sabe»                          |

La versión declarada es el punto débil: es texto que alguien escribe, y se queda vieja en cuanto nadie
la actualiza tras instalar una build nueva.

## Qué se puede observar en Windows sin ejecutar nada

Comprobado con tres aplicaciones de tipos distintos (una Tauri instalada con MSI, una Spring Boot
empaquetada con jpackage y DevHQ mismo), leyendo solo:

- **El recurso de versión del ejecutable** (`ProductVersion`, `FileVersion`, `ProductName`). Se lee como
  datos del archivo, sin cargarlo para ejecutarlo. Las tres apps lo tienen, pero **dos de las tres
  declaran una versión de relleno (`0.1.0`) que no cambia entre builds**, así que la versión sola no
  identifica qué build está instalada.
- **El contenido del ejecutable**: su SHA-256, tamaño y fecha de modificación. El hash sí identifica la
  build, y cambia en cada instalación.
- **Si hay un proceso cuyo ejecutable es esa ruta.** Se puede saber sin leer la línea de comandos de
  nadie. Pero tiene un límite: «abierto» depende de cada app. Con la ventana de DevHQ cerrada puede
  haber varios procesos con su ruta: son las puertas `--mcp` que lanzan las sesiones de agentes.
- **El estado de un servicio de Windows** por su nombre (por ejemplo, el de una base de datos local:
  `Running`, arranque `Automatic`), con una consulta de solo lectura al administrador de servicios.

Nada de esto exige credenciales, red ni ejecutar el programa observado.

## La propuesta, en tres pasos

Cada paso es útil por sí solo y no obliga a hacer el siguiente.

### Paso 1. La instalación como referencia, y su lectura

**Inventario.** Un tipo de referencia nuevo, `installation`: la ruta absoluta del ejecutable instalado,
en una máquina que sea estación de trabajo, como una `local_copy`. Responde «¿dónde está instalada?» y
es lo único que DevHQ lee: nunca busca instalaciones.

**Evidencia.** Leerla produce una **lectura de instalación**, con el mismo patrón que las de las copias
locales (README, «Copias locales»; vision.md, «Cómo crecer sin romperlo»): un hecho, no un juicio de
salud, sin vigencia, inmutable, que copia la ruta y la máquina que leyó. Guarda:

- si el archivo existe (`read`, `missing`, `unreadable`, `not_an_executable`);
- `ProductVersion`, `FileVersion` y `ProductName`, tal como los declara el archivo;
- SHA-256, tamaño y fecha de modificación;
- si en ese momento había un proceso con esa ruta, y cuántos, sin guardar identificadores de proceso;
- dónde está el archivo de verdad (`storedAt`), por la redirección de AppData (ver «Riesgos»).

**Lo derivado**, calculado al leer, en Rust y para la ventana y la puerta:

- **La versión declarada no es la instalada**: un aviso cuando la última lectura dice una versión y la
  ficha otra. Habría detectado al instante la versión desfasada de DevHQ mismo.
- **Cuándo cambió la instalación**: entre dos lecturas con distinto hash. DevHQ dice «se instaló una
  build nueva entre el 3 y el 5 de noviembre», con los dos momentos, nunca una fecha inventada. Es un
  despliegue observado, no tecleado.
- **Si estaba abierta**, con el momento de la lectura, en pasado como todas las lecturas.

**Cuándo se lee.** Solo a petición, como las copias: un botón Read en la referencia, «Read all on this
computer» en Overview, y en la puerta una herramienta `read_installations` con Read and maintain. Leerlas
también cuando se abre la ventana o con la supervisión es una decisión aparte, como lo fue con HTTP.

**Cómo se lee.** En un proceso propio, como `devhq --read-copy`: sin poder arrancar programas (Job
Object), con plazo, y con una respuesta que se valida como dato no confiable. El recurso de versión se
analiza con un lector de PE en Rust, nunca cargando el ejecutable, porque un archivo manipulado puede
intentar explotar al analizador. Una batería hostil de ejecutables malformados (recursos que se salen
del archivo, tablas circulares, cadenas enormes, un archivo que no es PE) lo prueba.

**Esquema.** Una migración: reconstruir `system_references` para añadir `installation` a su CHECK (SQLite
no cambia un CHECK con ALTER, como pasó en la v4 y la v7, así que `check_upgraded` debe aceptarla con su
prueba) y una tabla nueva de lecturas que empieza vacía, con sus índices y la prueba de plan de consulta.

### Paso 2. Disponibilidad: «siempre» o «bajo demanda»

**Inventario.** Cada aplicación declara cómo se usa: `always` (por defecto, lo de hoy) u `on_demand`.

**Comprobación condicionada.** La comprobación HTTP de una app bajo demanda solo se ejecuta si su
instalación tiene un proceso abierto en ese momento. Si no lo tiene, no se registra nada: ni sano ni
caído. Si lo tiene, la comprobación funciona como hoy, y un fallo es un fallo de verdad: la app está
abierta y no responde.

**Atención.** Para una app bajo demanda, un resultado sano caducado no entra en «Needs attention», y la
cobertura la cuenta como **«vista sana la última vez que estuvo abierta»**, con su fecha, no como hueco.
Un fallo vigente sí entra, como hoy. Es un cambio de reglas en los dos lados, con casos en
`contract/attention/`.

Este paso resuelve las apps web bajo demanda. Las de escritorio sin endpoint se quedan con lo que dé el
paso 1 (instalada, versión, abierta), que no es salud y no se presenta como salud.

### Paso 3. Servicios de Windows

Un tipo de referencia `service` (el nombre del servicio en una máquina) y su lectura: estado y tipo de
arranque. Un servicio de arranque automático que está parado es algo que pide atención. Encaja en el
mismo lector, pero conviene esperar a tener un caso que lo pida además de la base de datos local.

### Fuera de DevHQ: una pauta para las apps propias

El paso 1 identifica la build por su hash, pero no la relaciona con un commit. Para las apps que la
persona construye, la solución más barata no está en DevHQ: que cada build declare su versión de verdad
y escriba el commit en el recurso de versión (por ejemplo `ProductVersion = 1.4.0+3f9c2ab`). Entonces
DevHQ puede decir qué commit está instalado y, con la lectura de la copia local, cuántos commits de la
rama principal faltan por instalar. Contar esos commits exige una API de libgit2 más en el lector de
copias, con su paso por la batería hostil. DevHQ mismo debería ser el primero en cumplir la pauta.

## Reglas que toca

| Regla                                                                  | Qué pasa                                                                                                                                                                   |
| ---------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Lee solo lo que el inventario nombra, cuando se le pide, y nunca busca | Se cumple: la ruta la declara el inventario. Mirar la lista de procesos para saber si alguno tiene esa ruta es leer el sistema, y se escribe así; no se guarda ningún otro |
| Nunca ejecuta lo que la fuente nombra y nunca escribe en ella          | Se cumple: el ejecutable se lee como datos, en un proceso que no puede arrancar programas                                                                                  |
| No funciona en segundo plano                                           | Se cumple: a petición, o con la supervisión mientras la ventana está abierta                                                                                               |
| Nada desconocido parece sano                                           | Se cumple: una lectura de instalación no es salud. El paso 2 cambia qué cuenta como hueco para las apps bajo demanda, y por eso se escribe en el README                    |
| La historia no se reescribe                                            | Se cumple: lecturas inmutables; una app con lecturas no se borra                                                                                                           |

## Riesgos en Windows

- **Redirección de AppData.** Muchas apps se instalan por usuario en `%LOCALAPPDATA%`, y una puerta
  lanzada desde una app empaquetada (Claude, Codex) ve la copia del paquete mezclada con la real. Una
  ruta que solo existe en la carpeta real se lee bien, pero una copia antigua dentro del paquete la
  taparía. La lectura debe guardar dónde está el archivo de verdad (`storedAt`, como hace `location`) y
  decir que no se fía cuando no es la ruta pedida.
- **Ejecutables protegidos o de otro usuario.** Puede no dejar leer la ruta de un proceso elevado:
  entonces «abierta» es desconocido, no «cerrada».
- **«Abierta» depende de la app.** El ejemplo de DevHQ, con sus puertas, muestra que el proceso no
  siempre es la ventana. Una app así podría declarar qué cuenta, o quedarse con «hay procesos», dicho
  tal cual.
- **Una instalación en curso.** El archivo puede estar a medio escribir: el hash se calcula sobre lo
  que había, y la siguiente lectura lo corrige.
- **Las apps de la Tienda** (`WindowsApps`) no dejan leer su carpeta: `unreadable`.
- **Linux.** Un ELF no tiene recurso de versión. El hash, el tamaño, la fecha y el proceso sí se pueden
  leer (`/proc`), así que el paso 1 funcionaría a medias; los servicios serían de systemd.

## Descartado

- **Ejecutar la app con `--version`**: es ejecutar lo que el inventario nombra. Prohibido.
- **Un endpoint HTTP dentro de las apps de escritorio** solo para DevHQ: abre un puerto en apps que no
  lo necesitan y solo respondería mientras están abiertas.
- **Las claves de desinstalación del registro**: dan la misma versión que el ejecutable, no las nombra
  el inventario y cambian con cada instalador.
- **Leer la línea de comandos de los procesos**: puede llevar secretos y no hace falta.
- **WMI**: lento y de más para esto; las APIs de procesos y servicios bastan.

## Las preguntas de vision.md

1. **Qué pregunta responde, y con 50 aplicaciones.** «¿Está funcionando?», «¿qué tengo instalado y desde
   cuándo?» y «¿qué falta por instalar?». Con 50 apps, leer 50 instalaciones es leer 50 archivos y una
   lista de procesos: barato. Es justo lo que no escala a mano.
2. **Inventario, evidencia o derivado.** La instalación es una referencia (inventario); su lectura,
   evidencia; el desfase de versión, el cambio de build y «vista sana la última vez», derivados.
3. **Quién mantiene el dato.** Se observa. Hoy la versión se teclea y se queda vieja.
4. **Si cambia el agente, la plataforma o la tecnología.** No depende de ningún agente. Depende de
   Windows, y en Linux se cumple a medias. Cada lectura deja quién la pidió y cuándo.
5. **Qué regla toca.** Ninguna se rompe. El paso 2 cambia las reglas de atención para un tipo de app, y
   eso se escribe en el README y en los contratos.
6. **Lo más pequeño.** El paso 1 sin procesos ya resuelve la versión desfasada. Con procesos, además dice
   si estaba abierta. Esa es la primera entrega que propongo.

## Primera entrega propuesta

El paso 1 completo: la referencia `installation`, la lectura (versión, hash, tamaño, fecha, procesos y
ruta real), el aviso de versión declarada frente a instalada, el cambio de build entre lecturas, la
herramienta `read_installations` en la puerta y la batería de ejecutables malformados. Y, en DevHQ mismo,
la pauta: que su build declare la versión de verdad y el commit.

## Preguntas abiertas

Las tres del paso 1 se respondieron antes de escribirlo (12 nov 2026):

- **¿Basta «hay N procesos con esa ruta», o una app como DevHQ debe poder decir qué cuenta como
  abierta?** Basta, dicho tal cual: la lectura guarda cuántos procesos corrían desde el archivo y
  cuántos no dejó mirar el sistema, y la ventana y la puerta lo dicen así, nunca como «abierta». Qué
  cuenta como abierta se decide con el paso 2, que es el que lo necesita para condicionar la
  comprobación HTTP.
- **¿Leer las instalaciones al abrir la ventana se considera «a petición»?** No. Solo Read, Read all on
  this computer y `read_installations`, como las copias. Leerlas solas sigue siendo una decisión aparte.
  (24 nov 2026: se decidió. La ventana lee una vez al abrirse las copias y las instalaciones de esta
  estación, para el [trabajo pendiente](especificacion.md#trabajo-pendiente), porque sin lecturas
  recientes «¿qué necesita mi atención?» no tenía nada que decir de aplicaciones que no son servicios.)
- **¿El desfase entre versión declarada e instalada debe entrar en «Needs attention»?** No: va en la
  referencia, en un panel Installations de Overview junto a Local copies y, en la puerta, en
  `attention` como lista aparte y sin ordenar. No es salud, así que no toca las reglas de atención ni
  `contract/attention/`.

Quedan para el paso 2:

- Cómo declara una aplicación qué proceso cuenta como «abierta», cuando su ejecutable también corre sin
  ventana (las puertas `--mcp` de DevHQ).
- Si la comprobación condicionada debe leer la instalación ella misma, o basarse en la última lectura,
  que puede ser vieja.

## Lo decidido al implementar el paso 1

Lo que existe está en el README; aquí, lo que se decidió por el camino y por qué.

- **Qué guarda una lectura.** Además de lo propuesto: por qué no se conoce la versión
  (`versionUnknown`), en vez de dejar los tres textos vacíos, porque nada desconocido debe parecer un
  dato; y, con el número de procesos, cuántos no dejó mirar el sistema (`processesUnexamined`). Sin
  privilegios de administrador son muchos, así que «ninguno» siempre va con esa cifra. Solo se guardan
  `ProductVersion`, `FileVersion` y `ProductName`, de la primera tabla de cadenas: ningún número del
  recurso se convierte en versión.
- **El lector no se cuenta.** Cuando DevHQ lee su propia instalación, el lector es un proceso de ese
  mismo archivo. Es el instrumento, no un uso del programa, y se excluye; la ventana o la puerta que lo
  lanzó sí cuentan.
- **Lo que dice el archivo, no lo que diría Windows.** Para los ejecutables del propio sistema, las
  APIs de versión de Windows leen un archivo de idioma aparte (`.mui`) y pueden dar otra versión que la
  del recurso del ejecutable. DevHQ lee el recurso, y lo dice.
- **Cuándo cambió la build.** Para que leer el inventario no cueste más con los años, la búsqueda
  hacia atrás se detiene en 200 lecturas y lo dice («la misma build al menos desde…»). Una lectura que
  no leyó el archivo no cuenta como otra build, y una hecha en otra ruta corta la búsqueda.
- **La comparación de versiones** ignora una `v` inicial, lo que sigue a un `+` y los ceros finales:
  «1.4.0» es «v1.4.0», «1.4.0+3f9c2ab» y «1.4.0.0». Así la pauta del commit no hace diferir a nadie.
- **La pauta en DevHQ.** Su build lee el commit de `.git` como archivos, también en un worktree, y sin
  ejecutar `git`; por eso no puede saber si el árbol tenía cambios sin confirmar y no lo afirma:
  `.dirty` solo aparece cuando quien compila lo dice. La versión pasó a 0.2.0 y sube cuando se
  construye un instalador para instalarlo, no en cada cambio.
- **Sin capa común todavía.** El lector de instalaciones reutiliza el encierro del de copias locales
  (`confine`) y `inventory::stored_at`, pero repite el arranque del proceso, el plazo y la validación
  de la respuesta, que son cortos y distintos en cada uno (una línea frente a dos fases). Una tercera
  fuente dirá qué tienen de verdad en común.
- **Linux.** El lector lee un ELF (hash, tamaño, fecha, procesos por `/proc`) y dice que no declara
  versión. Sus pruebas se escribieron para los dos sistemas, pero solo se ejecutaron en Windows: en
  Linux las ejecuta el CI.
- **Lo que queda fuera.** Contar los commits de la rama principal que faltan por instalar (exige una
  API más de libgit2 en el lector de copias, con su paso por la batería hostil), y cualquier lectura
  que no se pida.
