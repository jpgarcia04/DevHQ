# Siguientes pasos

Nota de trabajo, no especificación: dice qué está en marcha y qué toca después para acercar DevHQ a
[docs/vision.md](vision.md), de modo que una sesión nueva (de Claude Code, de Codex o de quien sea)
pueda seguir sin la conversación que la escribió. El [README](../README.md) sigue mandando. Cuando un
paso se termina, sale de aquí en el mismo commit; cuando no quede ninguno, esta nota se borra.

Escrita el 6 oct 2026 desde una sesión local en la laptop con Windows, puesta al día esa noche, al
fusionar el registro de cambios y proponer el diseño de las referencias, y el 7 oct, al implementar
las referencias con las decisiones que la persona aprobó y al ver pasar su CI; y el 11 oct, al probar
cómo se puede leer una copia local sin que leerla ejecute nada, proponer el diseño de la etapa 3 y,
tras fusionarse las referencias, ponerlo al día sobre `main`.

## El plan, en corto

Revisado `main` contra la visión, el siguiente paso estructural era anclar cada sistema a lo que es
en el mundo real. La visión, la puerta para agentes (esquema v9), el registro de cambios común (v10)
y las referencias (v11) están en `main`: cada aplicación y recurso dice dónde está su repositorio,
sus copias locales, su documentación, sus logs y su panel. Ahora:

1. **Primera fuente observada sobre esas anclas** (etapa 3, diseño propuesto abajo): la actividad de
   las copias locales, leída cuando se pide y sin que leerla ejecute nada de lo que se lee.

Después, según lo que pidan preguntas concretas: bases de datos compartidas y dominios como tipos de
recurso, relaciones nuevas, despliegues con versión, commit y destino, credenciales, operaciones y
supervisión con la ventana cerrada, en el orden de «Líneas de evolución posibles» de la visión.

No conviene hacer todavía: una tabla genérica de entidades, un motor de grafos, una entidad
«proyecto», una capa de «integraciones» antes de tener una segunda fuente real, event sourcing,
permisos por agente ni descubrir repositorios por su cuenta.

## Estado

- [#2](https://github.com/jpgarcia04/DevHQ/pull/2) (la visión),
  [#3](https://github.com/jpgarcia04/DevHQ/pull/3) (la puerta, v9),
  [#4](https://github.com/jpgarcia04/DevHQ/pull/4) (el registro de cambios, v10) y
  [#5](https://github.com/jpgarcia04/DevHQ/pull/5) (las referencias, etapa 2, v11) están
  **fusionados** en `main`. #4 entró (`1a40caf`) con todo el CI de Linux y de Windows en verde al
  primer intento, y #5 también (`aff9dd7`, 11 oct): los 5 checks de Linux y los 3 jobs de Windows (el
  previo; «Rust tests · NTFS, SChannel…», 13 min; «Installer, native walk…», 18 min). Lo que la
  persona fijó al aprobar las referencias y queda en el README («Referencias»): se editan con su
  sistema, como los componentes, y el registro de cambios las cubre con un campo `references` en las
  acciones de siempre; la v11 solo copia el destino; `repository` es solo una URL web http(s),
  comparada por igualdad exacta; un destino es una URL http(s) o una ruta absoluta, y la etiqueta es
  un nombre para personas, nunca un localizador. Son inventario declarado, no evidencia: no llevan
  `source`.
- **El diseño de la etapa 3** está en `claude/local-activity-design`, que sale de `main` (`aff9dd7`)
  desde que #5 se fusionó; antes salía de su punta.
- **La primera carga, medida en las dos caminatas de #5.** La caminata dice cuánto tarda la ventana
  en poder manejarse tras arrancar DevHQ y, en el primer paso (y en cualquiera que falle), si la
  página está visible, cuánto lleva cargada y cuándo se pidió y se respondió cada llamada a DevHQ, en
  ms desde que cargó la página:
  - WebView2 («Installer, native walk…», bajo la protección en tiempo real): la ventana se pudo
    manejar 5077 ms después de arrancar DevHQ; visible, con foco y cargada hacía 1413 ms;
    **`list_inventory` 1231 → 1255 ms (24 ms)**, con la actualización desde v6 y el paso a WAL
    incluidos, y `watch_http_monitor` 1230 → 1259 ms. El paso tardó 726 ms. La puerta dejó de ver la
    ventana 319 ms después de que DevHQ terminara. En #4 fueron 4760 ms, 686 ms, `list_inventory`
    532 → 560 ms (28 ms), 407 ms y 322 ms: la llamada sigue tardando lo mismo.
  - WebKitGTK: la ventana se pudo manejar 3036 ms después de arrancar DevHQ; visible, con foco y
    cargada hacía 394 ms; el paso tardó 1373 ms (en #4, 2426, 516 y 1679 ms). Ahí no hay tiempos de
    llamadas: Tauri llama a DevHQ por `ipc://localhost/<comando>`, que no es HTTP, y la página solo
    mide peticiones HTTP(S) (Resource Timing); en Windows llama por `http://ipc.localhost/<comando>`.
    Desde #5 la caminata pregunta a Tauri adónde llama (`convertFileSrc('', 'ipc')`) y lo dice en vez
    de «none recorded»: «calls into DevHQ: not timed here, since Tauri sends them to
    ipc://localhost/, which is not HTTP(S), and the page times only HTTP(S) requests».
- **Un fallo intermitente sin explicar**, visto una vez en el primer intento del job «Installer,
  native walk…» de #3: el primer paso esperó 15 s a «Nothing is registered yet.» y la captura mostraba
  «Loading local inventory...», así que `list_inventory` no había respondido; pasaron 74 s entre el
  arranque de msedgedriver y el fallo, y al relanzarlo el paso pasó en 901 ms. En #4 y en #5 no
  volvió. Si vuelve, leer su «window state»:
  - Si `list_inventory` falta mientras otras llamadas sí aparecen, o se respondió tarde, lo retiene
    Rust: buscar qué (las tres conexiones de la primera carga, la conversión a WAL, el antivirus);
    hacer `list_inventory` asíncrono, como ya son los comandos que esperan, al menos dejaría la
    ventana atendida mientras tanto.
  - Si se respondió pronto (unos 30 ms, como arriba) y la página no lo mostró, el retraso está en la
    página o en el driver: `visibility hidden` apuntaría a que WebView2 no pinta una ventana que nadie
    ve.

## Qué hacer, en orden

1. **Etapa 3, actividad de las copias locales: diseño propuesto, esperando el visto bueno de la
   persona** (ver «Etapa 3: diseño propuesto», abajo). No se escribe código hasta que lo dé.
2. **Abrir una referencia, si la persona lo quiere**, solo como subproducto: un comando Rust
   `open_reference(id)` que lee la referencia de SQLite y abre en el navegador solo URL http(s); la
   ventana nunca pasa una URL y el WebView no gana plugin ni permiso. Una ruta no se abre hasta que
   DevHQ sepa en qué estación corre, y entonces solo se muestra la carpeta en el explorador, nunca se
   abre un archivo: abrir una ruta que escribió un agente podría ejecutar un programa.

## Etapa 3: diseño propuesto

Responde «¿qué lleva meses sin actividad?» y «¿qué trabajo existe solo en una máquina?», y prepara
«¿qué tiene cambios sin desplegar?»: guarda el commit de HEAD, que la etapa de despliegues comparará
con el desplegado. Es la primera fuente que DevHQ lee en una máquina concreta y la primera cuyo
objetivo puede haber escrito un agente, así que empieza por un principio.

### El principio: observar no da poder a lo observado

Leer una fuente nunca le da a esa fuente una forma de ejecutar código ni de cambiar nada: DevHQ no
ejecuta ningún programa que la fuente nombre (hooks, filtros, programas de firma o de monitorización,
helpers de remotos) y no escribe en ella. La ruta de una `local_copy` y el repositorio que contiene
son datos no confiables, los haya declarado la persona o un agente. La garantía no puede depender de
una lista de cosas prohibidas que cada versión de Git alarga: viene de leer con algo que no sabe
ejecutar programas y, además, desde un proceso al que el sistema operativo no deja crearlos.

### Lo comprobado

El 11 oct, en la laptop (Windows 11, Git 2.54, `git2` 0.21 con libgit2 1.9.7, `gix` 0.89), con un
arnés aparte, fuera del repositorio. Construyó repositorios cuya config, cuyos `.gitattributes` y
cuyos hooks nombraban un programa marcador, que solo deja una marca, en cada sitio donde Git lo
permite: unas cuarenta claves de `git help --config` que nombran un programa (de `core.fsmonitor` y
`filter.<driver>.process` a `gpg.ssh.program`, `hook.<nombre>.command` y `gc.recentObjectsHook`),
hooks en `.git/hooks` y en `core.hooksPath`, `include.path`, `config.worktree`, un submódulo y un
repositorio anidado con su propia config hostil, un worktree enlazado, un commit firmado con
`log.showSignature`, un remoto `ext::` con `remote.<nombre>.vcs`, y copias del marcador en el `PATH`
llamadas `git`, `sh`, `gpg`, `ssh`, `git-lfs`… Leyó cada copia, comparó el contenido y el mtime de
cada archivo antes y después, y contó las marcas:

- **El `git` instalado**: `status` ejecutó el hook de fsmonitor y los drivers de filtro y, sin
  `--no-optional-locks`, reescribió `.git/index`; `log -1 --format=%ct` ejecutó
  `gpg.openpgp.program`. Con `-c core.fsmonitor=false`, `-c log.showSignature=false`,
  `--no-optional-locks` e `--ignore-submodules=all` siguió ejecutando drivers de filtro de la
  config. Endurecerlo es una lista de prohibiciones que nunca se cierra.
- **gix**: con las opciones por defecto, cada apertura lanzó el `git` del `PATH`
  (`git config -lz --show-origin` y `git --exec-path`) para encontrar la config del sistema, también
  leyendo solo refs, y `status` ejecutó los `filter.<driver>.process` del repositorio, también los
  que llegaban por `include.path`. Aislado (`open::Options::isolated()`) ya no lanza `git` ni sigue
  includes y leer refs no ejecutó nada, pero `status` siguió ejecutando los drivers de la config
  local; y, sin la config del sistema, no ve el `core.autocrlf=true` que Git for Windows pone en
  `C:\Program Files\Git\etc\gitconfig`, así que dio por cambiado un archivo que `git status` ve
  limpio.
- **libgit2**: no ejecutó nada en ningún caso ni cambió o creó un solo archivo, y en un gemelo benigno
  leyó lo mismo que `git`: rama, HEAD, último commit, commits sin publicar, merge en curso, cambios y
  entradas nuevas, con las carpetas nuevas agrupadas como en `git status`. Solo aplica sus filtros
  internos (crlf, ident), así que un archivo bajo un driver externo con el mtime cambiado sale como
  modificado aunque `git` lo vea limpio (comparó «a» con el blob «Xa»); se reconoce por su atributo
  `filter` y se cuenta como no verificable.
- **Coste**: leer DevHQ (249 archivos seguidos y 99 commits, con `node_modules` y `target`
  ignorados) tardó 18 ms con libgit2 (347 ms en frío) y unos 100 ms con `git status`.
- **Encierro**: un proceso que se mete a sí mismo en un Job Object con `ActiveProcessLimit = 1` ya no
  puede crear procesos (Windows responde 1816, cuota insuficiente). gix encerrado dejó cero marcas;
  libgit2 encerrado leyó lo mismo que antes.

### Cómo se lee

- **Biblioteca**: libgit2, por `git2` sin funciones de red (`default-features = false`,
  `vendored-libgit2`), y solo sus APIs de lectura: abrir sin buscar hacia arriba (`NO_SEARCH`: la
  ruta declarada es la carpeta de la copia; varias aplicaciones de un monorepo declaran la misma),
  refs, commits, reflogs, config, atributos y `statuses` sin `UPDATE_INDEX` y sin submódulos. Nunca
  fetch, checkout ni nada que escriba.
- **Un proceso lector propio**: `devhq --read-copy`, el mismo ejecutable que la puerta, sin ventana.
  Antes de leer nada se encierra: en Windows, un Job Object con `ActiveProcessLimit = 1` y
  `KILL_ON_JOB_CLOSE`; en Linux, `RLIMIT_NPROC = 0`, que a un usuario sin privilegios le impide
  `fork` y también hilos (hay que verificarlo en el CI), o un filtro seccomp que niegue `execve` si el
  lector necesitara hilos. Si no puede encerrarse, no lee. Lo lanzan la ventana, desde un comando
  asíncrono, o la puerta: sin shell y con un entorno mínimo (lo que libgit2 necesita para encontrar
  la config del usuario y la del sistema, sin variables `GIT_*`); recibe las rutas por stdin y
  devuelve una lectura por línea en stdout. Así un fallo de memoria o un bucle de libgit2 ante datos
  hostiles mata a un hijo y no a la ventana, el plazo se cumple matándolo (libgit2 no se puede
  interrumpir) y lo que devuelve se valida como dato no confiable antes de guardarlo. La ventana no
  puede encerrarse así: WebView2 necesita sus propios procesos.
- **Plazo**: primero las refs, con un plazo corto, y después el working tree, con uno mayor (unos
  20 s). Si el working tree no termina, la lectura se guarda con las refs y el working tree
  desconocido.
- **Alternativas descartadas**:
  - El `git` instalado: ejecuta lo que nombra el repositorio y escribe el índice; endurecerlo es una
    lista que nunca se cierra, y depende de que haya `git` y de cuál.
  - gix tal cual: lanza `git` y ejecuta drivers.
  - gix aislado: ejecuta drivers y pierde la config del sistema, con resultados falsos en Windows.
  - gix solo para refs: seguro y en Rust, pero sin working tree, que es la mitad del trabajo que solo
    existe en una copia. Queda como opción si la persona prefiere no tener una biblioteca en C.
  - gix con un pipeline propio sin drivers: posible con su plumbing, pero es más código sobre una
    API 0.x que cambia a menudo, y en Windows la config del sistema sigue exigiendo lanzar `git` o
    buscarla a mano.
  - Leer `.git` a mano: packfiles con deltas, el índice y las reglas de ignorados; sería reimplementar
    Git.
  - libgit2 dentro del proceso de la ventana: más simple, pero sin encierro posible, sin un plazo real
    y con un fallo de C que cerraría DevHQ.

### Qué se lee: una lectura neutral

El lector produce una **lectura** (`CopyReading` en Rust) en conceptos, no en la forma de libgit2.
La tecnología queda aparte, en `reader` («libgit2 1.9.7»), para saber qué lecturas hizo un lector si
un día resulta tener un fallo. Cada campo responde a una pregunta:

| Campo                                       | Qué es                                                                                                                                                                                                                                                                                                        | Para qué                                                                                                                |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `outcome`, `detail`                         | `read`; `missing` (nada en esa ruta); `not_a_copy` (hay algo, pero no una copia de trabajo: una carpeta sin repositorio, un repositorio bare, un archivo); `unreadable` (el sistema niega el acceso, o el repositorio es de otro usuario y libgit2 no confía en él). `detail`, en palabras del sistema        | Una copia declarada que no está es inventario que envejece                                                              |
| `branch`, `head`                            | La rama actual (nula si HEAD está suelto) y el commit de HEAD (nulo si aún no hay commits)                                                                                                                                                                                                                    | Contexto para un agente; después, los cambios sin desplegar                                                             |
| `lastCommitAt`                              | La fecha del commit más nuevo entre HEAD y las ramas locales, según el propio commit                                                                                                                                                                                                                          | La falta de actividad                                                                                                   |
| `unpublishedOnHead`, `unpublishedElsewhere` | Commits alcanzables desde HEAD, y desde las demás ramas locales, que ninguna rama remota de la copia contiene; hasta 1000 («1000+»)                                                                                                                                                                           | Trabajo que solo existe aquí. Separados porque las ramas viejas de un PR fusionado con squash cuentan como sin publicar |
| `syncedAt`                                  | La última vez que la copia habló con un remoto según sus archivos: lo más nuevo entre el mtime de `FETCH_HEAD` y los reflogs de las ramas remotas                                                                                                                                                             | «Sin publicar» se compara con lo que la copia sabía entonces: DevHQ no hace fetch                                       |
| `remotes`                                   | Nombre y URL de cada remoto (hasta 20), sin contraseña ni usuario http(s)                                                                                                                                                                                                                                     | La divergencia con el repositorio declarado                                                                             |
| `operation`                                 | `merge`, `rebase`, `cherry_pick`, `revert`, `bisect` o `apply_mailbox` en curso                                                                                                                                                                                                                               | Algo que se quedó a medias                                                                                              |
| `changed`, `untracked`, `unverifiable`      | Archivos seguidos con cambios (en el índice o en el working tree, conflictos incluidos); entradas nuevas no ignoradas (una carpeta nueva cuenta una vez, como en `git status`); archivos con el mtime cambiado bajo un driver de filtro que la config define y DevHQ no ejecuta. O desconocido, con el motivo | Trabajo sin confirmar; lo no verificable no se cuenta como cambio ni como limpio                                        |

**Fuera, por ruido o por ser datos personales**: autor, correo y mensajes de commit; nombres de
archivo; listas de ramas, tags y stashes; el estado de los submódulos (cada uno es su propia copia,
si se declara); archivos ignorados, tamaños y estadísticas de diff; «behind», que dice más de cuándo
fue el último fetch que del proyecto; la rama por defecto; objetos LFS; hooks y config. Una columna
se añade después si una pregunta concreta la pide.

### Cómo se guarda: evidencia propia, no salud

- **Una tabla nueva**, `copy_readings` (v12), y no una fuente más de `observations`: una lectura no
  es healthy, degraded ni unhealthy, no entra en la atención ni en la cobertura de salud, y meterla en
  `observations` o `events` obligaría a reconstruirlas por sus CHECK de `source` y `result`. Es
  inmutable y lleva las columnas de toda evidencia: el sujeto (`application_id`/`resource_id`, con un
  CHECK de exactamente uno y `RESTRICT`), `source = 'read'` («Automated · Local read»), `reader`,
  `observed_at`, `recorded_at` y `outcome`, más los hechos de arriba, con CHECK que solo admiten
  hechos con `read` y exigen el working tree o el motivo de no tenerlo.
- **Qué copia y dónde**: `reference_id`, `machine_id` y `path` se copian, sin claves foráneas, como la
  URL en una observación HTTP. `references::replace` borra y reinserta en cada guardado: una clave
  foránea con `RESTRICT` lo rompería y con `CASCADE` borraría evidencia. Así una copia se puede quitar
  o mover sin tocar su historia, y una máquina se puede borrar cuando ya no guarda rutas. Si la última
  lectura se hizo en otra ruta o en otra máquina de las que declara hoy la referencia, la ventana lo
  dice, como el aviso de URL cambiada de la comprobación HTTP.
- **Es historia**: `history_of` la cuenta, así que un sistema con lecturas ya no se puede borrar,
  solo archivar.
- **Sin vigencia**: una lectura no afirma nada del presente, vale para su instante, y su antigüedad
  siempre se ve («read 3 days ago on Laptop»); todo lo derivado dice de qué lectura sale. Una vigencia
  fija haría «caducar» algo que nunca dijo seguir siendo cierto.
- **Carga**: `list_inventory` lleva solo la última lectura de cada `local_copy` actual, por un índice
  `(reference_id, observed_at)`; las demás se paginan por sistema, de 200 en 200, por índices
  `(application_id, observed_at)` y `(resource_id, observed_at)`, con pruebas de plan de consulta. Las
  lecturas no escriben eventos ni salen en Activity: leer no es algo que le pase al sistema. Quién la
  pidió no se guarda, como en una ejecución de comprobación, y como nada sale del equipo ni cambia la
  configuración tampoco deja entrada en el registro de cambios.

### En qué estación de trabajo corre DevHQ

- **Dos cosas separadas**: el recurso `workstation` es inventario compartido; «esta instalación corre
  en ese recurso» es una asociación local, fuera de `inventory.sqlite3`, en `this-workstation.json`,
  en la carpeta local de DevHQ (`app_local_data_dir`: `%LOCALAPPDATA%\com.devhq.desktop` en Windows,
  que no viaja con un perfil móvil; la puerta la resuelve con `dirs::data_local_dir()`, y un test ata
  las dos, como hoy la del inventario). Back up now no la copia.
- **Guarda** los ID de los recursos que son esta máquina (uno por inventario), cuándo se eligieron, el
  nombre del equipo (solo para mostrarlo) y una **huella de la máquina**: un HMAC-SHA256 con una clave
  fija de DevHQ sobre el identificador que da el sistema (`MachineGuid` en Windows, `/etc/machine-id`
  en Linux); el identificador en bruto nunca se guarda ni se muestra. Si el archivo llega a otro
  equipo (copiar el home entero es habitual en Linux, donde está junto al inventario), su huella no
  coincide y DevHQ no se cree esa máquina; sin la huella, grabaría como evidencia de la copia de Laptop
  lo que leyó en otro equipo.
- **Vale** solo si la huella coincide, el recurso existe en el inventario abierto y es `workstation`;
  si no, la estación es desconocida y se dice por qué (sin elegir, archivo de otro equipo, recurso
  borrado o que ya no es estación, sistema sin identificador). Sin estación conocida no se lee
  ninguna copia.
- **Solo la ventana la cambia** (Settings › This computer: elegir una estación o crear una con el
  nombre del equipo); la puerta solo la lee, como el modo de acceso. No es inventario, así que no va
  al registro de cambios; cada lectura guarda en qué máquina se hizo.
- **MSIX**: una puerta lanzada desde una app empaquetada ve el archivo real si el paquete no tiene
  otro; una ventana empaquetada escribiría en la copia del paquete. `status` y Settings dicen dónde
  está de verdad (`storedAt`).
- **Alternativas**: la huella como columna del recurso (más simple, con rastro en el registro de
  cambios, y una huella copiada no coincide en otro equipo; pero mete en el inventario compartido una
  identidad que pertenece a una máquina, justo lo que la persona no quiere); la asociación local sin
  huella (un home copiado haría que otro equipo se creyera este); el nombre del equipo (se repite y
  cambia); preguntarlo en cada sesión (la puerta no puede preguntar); deducirlo de qué rutas existen
  (es escanear, y la misma ruta puede existir en dos equipos).

### Cuándo se lee y quién lo pide

- **Solo a petición**: Read en una copia de esta estación, Read all on this computer y la herramienta
  de la puerta. Nunca al abrir, nunca con un temporizador, nunca con la ventana cerrada. Leer escribe
  evidencia, así que la puerta exige Read and write.
- Solo se leen copias de esta estación. Para las de otra máquina, la ventana muestra su última
  lectura y dice que se leen desde el DevHQ de esa máquina.

### Lo derivado: qué vale y qué sería ruido

Se calcula en Rust en cada lectura del inventario, como `sharedRepositories`, para la ventana y la
puerta, y no se guarda. Nada entra en la lista ordenada de «¿Qué necesita mi atención?», que sigue
siendo de salud, así que `contract/attention/` no cambia.

- **Último commit de cada sistema**: el más nuevo entre las últimas lecturas de sus copias, con su
  base («Laptop, read today; Desktop never read»). Es una cota inferior: nunca dice «inactivo» si una
  copia no se ha leído. Applications lo muestra y ordena por él, sin un umbral de meses que mantener;
  lo que no tiene lecturas va aparte.
- **Trabajo que solo existe en una copia**: commits sin publicar, cambios, entradas nuevas o archivos
  no verificables, o una operación a medias, cada cosa con su número y la fecha de su lectura. Va en
  un panel **Local work** de Overview, como Supervision, junto con las copias declaradas que no se
  encontraron y cuántas copias de esta estación nunca se leyeron, para que un panel vacío no
  tranquilice de más.
- **Divergencia con lo declarado**: la copia no está donde dice la referencia; su remoto no parece el
  repositorio declarado; o el sistema no declara repositorio y la copia tiene remoto (la ventana puede
  proponer declararlo cuando el remoto es una URL web, y la persona acepta). La comparación normaliza:
  el host en minúsculas sin usuario ni puerto, y la ruta sin `/` al principio ni al final, sin `.git`
  y en minúsculas, para `https://…`, `ssh://…`, `git://…` y `usuario@host:ruta`; una ruta local o un
  `ext::` no se comparan. Los hosts cuyo SSH y web difieren (Azure DevOps) salen como distintos, así
  que la ventana dice «no parece», y nunca es atención.
- **Ruido, fuera**: que dos copias estén en commits distintos, en una rama que no es la por defecto o
  con HEAD suelto; un umbral fijo de «inactivo»; comparar copias entre sí.

### Ventana y puerta

- **Comandos**: `read_copies(referenceIds | null)`, asíncrono (null son todas las de esta estación),
  responde lecturas y, por copia, los fallos de DevHQ; `older_readings(subject, before)`;
  `set_this_workstation(resourceId | null)`; y `list_inventory` gana `thisWorkstation` y, por cada
  `local_copy`, su última lectura y lo derivado. Cada uno con su fixture en `contract/` y sus
  lectores en `arguments()`.
- **Ventana**: Settings › This computer; en el Overview de cada aplicación y en la tarjeta de cada
  recurso, cada copia con su última lectura, su máquina y Read si es de esta; la estación marcada
  «This computer» en Infrastructure, con Read all; el panel Local work; el último commit en
  Applications. La preview muestra lecturas de ejemplo y rechaza leer, como toda escritura.
- **Puerta**: `status` dice qué estación es esta, o por qué no se sabe; `list_systems` y `get_system`
  dan, por copia, `onThisWorkstation`, su última lectura y lo derivado; `attention` añade, sin
  ordenarlos, `localWork` y `copiesNeverRead`, como ya hace con `notSupervised`; `history` admite
  `records: "readings"`; y una herramienta nueva, `read_copies` (un sistema, una referencia, o todas
  las de esta estación), con Read and write y sin `openWorldHint`, porque nada sale del equipo.

### Reglas que cambian

Texto propuesto; se escribe con la implementación, en el mismo commit.

- **`CLAUDE.md`**, en «Product rules», en lugar de «DevHQ observes; it does not operate systems, run
  in the background or hold credentials.»: «DevHQ observes; it does not operate systems, run in the
  background or hold credentials. It reads only what the inventory names and never searches a disk
  or a network for things to add. Observing never gives the source a way to run code or change
  anything: reading never runs a program the source names (hooks, filters, signing, monitor or
  remote helper programs) and never writes to it, so every declared path, and what it holds, is
  untrusted data.» Y una sección «Local copies»: el lector corre en `devhq --read-copy`, al que el
  sistema no deja crear procesos, y un lector o una biblioteca nuevos pasan antes la batería hostil.
- **README, «Arquitectura»**, en lugar de «No se implementan autenticación, cuentas, cloud, sync,
  telemetría, scanners, automation engine, Runner ni background service.»: «No se implementan
  autenticación, cuentas, cloud, sync, telemetría, automation engine, Runner ni background service.
  DevHQ no busca qué observar: nunca recorre discos, carpetas ni redes en busca de repositorios o
  servicios. Lee solo lo que el inventario nombra (la URL de una comprobación, la ruta de una copia
  local de esta estación de trabajo) cuando se le pide, o mientras la ventana está abierta si es una
  comprobación supervisada. Leer nunca ejecuta un programa que nombre lo leído ni escribe en ello: la
  ruta y lo que contiene se tratan como datos no confiables, aunque los haya declarado la persona.»
  La entradilla dice que también conserva lo que lee, cuando se lo pides, de las copias locales de
  esta estación, y una sección nueva, «Actividad de las copias locales», describe todo lo de arriba.
- **`docs/vision.md`**: un principio 16 en «Qué se garantiza», «Observar no da poder a lo
  observado»; en «Tensiones», la fila «Descubrir repositorios o servicios» pasa a «DevHQ no busca qué
  observar; lee solo lo que el inventario nombra» → «Que lo lances tú, sobre un lugar que eliges, de
  solo lectura y sin ejecutar nada de lo que encuentre, y que proponga altas que aceptas»; la línea de
  evolución 5, hecha; y en «Cómo crecer sin romperlo», el patrón de abajo.

### El patrón para las fuentes siguientes

Docker, GitHub, un VPS o los despliegues no comparten todavía más que esto, y no se escribe una capa
de integraciones hasta que una segunda fuente lo pida:

1. El inventario nombra el objetivo y la fuente lee solo eso, a petición; supervisarla es otra
   decisión, como con HTTP.
2. Si el objetivo vive en una máquina (una copia, el Docker local), se lee solo en la suya, con «esta
   estación».
3. Si la fuente juzga salud (un contenedor caído), escribe observaciones con su `source`; si registra
   hechos que no son salud, tiene su tabla de evidencia con las columnas comunes y copia su objetivo
   en vez de apuntarlo con una clave foránea.
4. Un fallo del objetivo es evidencia; uno de DevHQ es un error que no se guarda.
5. Leer no ejecuta nada de la fuente ni escribe en ella, y si hay credenciales, ninguna pasa por la
   fuente que se lee.
6. Lo derivado se calcula al leer, en Rust, para la ventana y la puerta; si entra en la atención, se
   escribe a los dos lados con casos en `contract/attention/`.

### Riesgos

- **libgit2 es C que lee datos hostiles.** En 2024 corrigió un bucle infinito en
  `git_revparse_single` (CVE-2024-24575) y una corrupción de memoria en `git_index_add`
  (CVE-2024-24577, una escritura que DevHQ no usa). El proceso aparte, encerrado y con plazo, lo
  contiene, y su salida se valida; queda que un exploit lea lo que el usuario puede leer. `git2` se
  mantiene al día.
- **libgit2 va detrás de Git**: un formato que no conoce (`extensions.refStorage = reftable`,
  `objectFormat = sha256`) es un fallo de DevHQ («este formato aún no se puede leer»), no evidencia
  de la copia.
- **La comprobación de propietario** de libgit2, como `safe.directory`, rechazaría copias en FAT,
  exFAT o recursos de red cuyo propietario no reconoce (sin comprobar todavía): saldrían
  `unreadable`, con el motivo.
- **Working tree**: sin su driver, un archivo LFS tocado es no verificable; sin fsmonitor ni untracked
  cache, un árbol enorme tarda más, y el plazo lo acota. Leer mientras la persona ejecuta un `git`
  pesado puede fallar una vez; es un fallo de DevHQ y se vuelve a pedir.
- **Commits sin publicar** en ramas viejas (squash): se ven aparte, pero se ven.
- **La huella de la máquina**: las VMs clonadas sin sysprep comparten `MachineGuid`; un Linux sin
  `machine-id` no puede elegir estación; reinstalar el sistema obliga a elegirla otra vez.
- **Las lecturas hacen imborrable su sistema.**
- **Una dependencia C nueva** (libgit2 y zlib, compiladas con cc como ya se compila SQLite): hay que
  verla compilar en el CI de Linux y de Windows y medir cuánto crece el instalador.
- **Una ruta escrita por un agente** puede apuntar a cualquier carpeta que el usuario pueda leer:
  DevHQ solo dirá si es una copia y cuáles son sus remotos, que un agente con terminal ya ve.

### Pruebas propuestas

1. **Batería hostil**, en Rust y la misma para cualquier lector, en Linux y en Windows. Repositorios
   que la propia prueba construye con cada clave de config que nombra un programa (una lista fija,
   sacada de `git help --config`, con un test que avisa si el `git` del CI conoce claves que la lista
   no tiene), drivers en `.gitattributes`, hooks en `.git/hooks`, `core.hooksPath` y
   `hook.<nombre>.command`, `include.path`, `includeIf`, `config.worktree`, un submódulo y un
   repositorio anidado hostiles, un worktree enlazado, un commit firmado con `log.showSignature`,
   remotos `ext::` y `remote.<nombre>.vcs`, `core.fsmonitor` como hook y como `true`,
   `core.untrackedCache`, `feature.manyFiles` y `filter.<driver>.required`. Cada programa nombrado es
   el propio ejecutable de la prueba, que con una variable de entorno solo deja una marca (como el
   ayudante de `window_lock`), copiado también en el `PATH` como `git`, `sh`, `gpg`, `ssh` y
   `git-lfs`. Se exige: ninguna marca; cada archivo de la copia igual en contenido y mtime; ningún
   archivo nuevo; y la lectura, correcta.
2. **Encierro**: en modo lector, crear cualquier proceso falla, y si el encierro no se puede poner,
   no se lee.
3. **Contra `git`**, donde el CI lo tiene: gemelos benignos (ramas, HEAD suelto, sin commits, merge
   y rebase en curso, sin publicar en HEAD y en otras ramas, carpetas nuevas, el `core.autocrlf` del
   sistema en Windows leído por el lector encerrado y con su entorno mínimo, un driver real que da no
   verificable), comparados con `git status --porcelain` y `git rev-list`.
4. **Resultados**: ruta que no existe, archivo, carpeta vacía, repositorio bare, sin permiso
   (`chmod 000` en Linux), de otro usuario (`sudo chown` en el CI de Linux) y formato desconocido, que
   no guarda nada.
5. **Plazo**: un lector que se bloquea se mata y queda la lectura de refs con el working tree
   desconocido; una salida del hijo malformada o enorme se rechaza.
6. **Remotos**: credenciales que se quitan y normalización, con casos (`https://token@…`,
   `ssh://u:p@…`, `git@host:x.git`, Azure DevOps, rutas locales, `ext::`).
7. **Esquema**: la actualización v11→v12 comparada fila por fila; CHECK que rechazan lecturas
   incoherentes; `history_of` que impide borrar; editar o quitar la referencia no toca sus lecturas; y
   los planes de consulta de la última lectura por copia y de la paginación.
8. **Esta estación**: el archivo de otra máquina se ignora; un recurso borrado o que deja de ser
   estación de trabajo la deja desconocida; la ventana y la puerta resuelven el mismo archivo; la
   puerta nunca lo escribe.
9. **Contratos IPC**, la preview y la **caminata nativa** en Linux y en Windows, con la protección
   en tiempo real: elegir esta estación, declarar una copia hostil con un commit sin publicar y un
   cambio, Read, ver Local work y comprobar que no quedó ninguna marca.

Cada prueba nueva se confirma fallando sin su arreglo: por ejemplo, con el lector cambiado por el
`git` instalado, la batería hostil tiene que fallar.

### Decisiones de la persona

1. El principio «observar no da poder a lo observado», con los textos propuestos para `CLAUDE.md`,
   el README y la visión.
2. El lector: libgit2 en un proceso aparte encerrado (propuesto), libgit2 dentro de DevHQ, o gix solo
   para refs, sin working tree.
3. El working tree en la v12, solo con números (propuesto), o más adelante.
4. Los campos de la lectura y lo que queda fuera.
5. Evidencia propia sin vigencia, con la antigüedad siempre visible (propuesto), o con una vigencia
   fija.
6. Que las lecturas impidan borrar su sistema, como el resto de la historia (propuesto).
7. Esta estación: una asociación local con la huella de la máquina, elegida solo en la ventana
   (propuesto), o la huella como columna del recurso.
8. Solo a petición (propuesto), o también al abrir la ventana.
9. Lo derivado: el último commit en Applications, el panel Local work y las divergencias, sin nada en
   la lista de atención (propuesto).
10. Una sola normalización de URL, también para `sharedRepositories` (propuesto), o mantener allí la
    igualdad exacta de la etapa 2.
11. `read_copies` en la puerta con Read and write y sin entrada en el registro de cambios
    (propuesto).
12. Que el sistema también impida escribir al lector (nivel de integridad bajo en Windows, Landlock
    en Linux): ahora, o más adelante (propuesto), mientras lo garantizan libgit2 y la prueba que mira
    cada archivo.

## Lo que solo se puede hacer en la laptop

- Lo pendiente de la puerta en [docs/puerta-para-agentes.md](puerta-para-agentes.md): un Claude Code
  real contra el `devhq.exe` instalado y una puerta lanzada desde la app de escritorio de Claude
  (MSIX). La persona prefiere no instalar DevHQ hasta que sea usable.
- `npm run e2e:native` en Windows, que necesita apartar antes los datos de `%APPDATA%` y
  `%LOCALAPPDATA%`.

Una sesión en la nube corre todo lo demás, incluido `npm run e2e:native` sobre WebKitGTK en Linux
(ver «Cloud sessions» en `CLAUDE.md`), y el workflow de Windows prueba lo suyo en cada PR.
