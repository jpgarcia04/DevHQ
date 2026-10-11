# Siguientes pasos

Nota de trabajo, no especificación: dice qué está en marcha y qué toca después para acercar DevHQ a
[docs/vision.md](vision.md), de modo que una sesión nueva (de Claude Code, de Codex o de quien sea)
pueda seguir sin la conversación que la escribió. El [README](../README.md) sigue mandando. Cuando un
paso se termina, sale de aquí en el mismo commit; cuando no quede ninguno, esta nota se borra.

Escrita el 6 oct 2026 desde una sesión local en la laptop con Windows, puesta al día esa noche, al
fusionar el registro de cambios y proponer el diseño de las referencias, y el 7 oct, al implementar
las referencias con las decisiones que la persona aprobó y al ver pasar su CI; y el 11 oct, al probar
cómo se puede leer una copia local sin que leerla ejecute nada, proponer el diseño de la etapa 3,
ponerlo al día sobre `main` tras fusionarse las referencias y revisarlo con las cuestiones de la
persona.

## El plan, en corto

Revisado `main` contra la visión, el siguiente paso estructural era anclar cada sistema a lo que es
en el mundo real. La visión, la puerta para agentes (esquema v9), el registro de cambios común (v10)
y las referencias (v11) están en `main`: cada aplicación y recurso dice dónde está su repositorio,
sus copias locales, su documentación, sus logs y su panel. Ahora:

1. **Primera fuente observada sobre esas anclas** (etapa 3, diseño revisado abajo): la actividad de
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
- **La etapa 3** se implementa en `claude/local-activity-design`, que sale de `main` (`aff9dd7`)
  desde que #5 se fusionó; antes salía de su punta. La persona aprobó la dirección del diseño el 11
  oct, y la revisión de ese día no dejó nada pendiente de ella. Hecho en un primer commit: el lector
  (`devhq --read-copy`, libgit2 encerrado, con plazo), la batería hostil, el esquema v12
  (`copy_readings`), esta estación de trabajo (`this-workstation.json` con la huella), los comandos de
  la ventana y la herramienta `read_copies` de la puerta, con el README, `CLAUDE.md` y la visión. Todo
  pasa en la laptop con Windows. El primer CI de Linux
  ([run 38142657773](https://github.com/jpgarcia04/DevHQ/actions/runs/38142657773), lanzado a mano
  sobre `63c8204`) compiló el código solo de Linux, pasó clippy, la batería hostil y el encierro con
  seccomp (`a_confined_reader_cannot_start_any_program`), y falló una prueba: una copia en una carpeta
  `chmod 000` salía `not_a_copy` en vez de `unreadable`. La causa es de libgit2: `find_repo_traverse`
  mira `.git`, `HEAD`, `objects` y `refs` con `stat`, y cualquier fallo de `stat` le parece «no está».
  Pasaba igual en Windows con una ACL que niega leer, solo que ninguna prueba lo miraba. Ahora, cuando
  libgit2 no encuentra repositorio, el lector vuelve a mirar lo mismo con `std::fs` y, si el sistema
  le negó algo, la lectura es `unreadable`; la prueba cubre la carpeta y solo su `.git`, en Linux y en
  Windows, y falla sin el arreglo. La batería encontró un fallo real antes de
  existir el primer commit: la API de reflog de libgit2 (`git_reflog_read`) crea un log vacío cuando una
  ref no tiene, y leía escribiendo en `.git/modules/sub/logs/refs/remotes/origin/main`; ahora esos logs se
  leen como archivos. Hecha después la ventana (Settings › This computer, la última lectura de cada
  copia con Read y sus lecturas anteriores, el panel Local copies con Read all y la última actividad en
  Applications), con la preview y sus pruebas. Esta estación se pide aparte de `list_inventory`, después
  de cada lectura del inventario, para no cambiar lo que `list` devuelve. Y la caminata nativa: elige
  esta estación, declara una copia hostil construida con el `git` instalado y la lee con Read, Read all
  y la puerta; ningún marcador corre, ningún archivo cambia, y `git status` sobre la misma copia sí
  ejecuta su fsmonitor. Pasó en el CI de Linux
  ([run 38150025121](https://github.com/jpgarcia04/DevHQ/actions/runs/38150025121), los 4 jobs) con el
  lector encerrado por seccomp. En el PR [#6](https://github.com/jpgarcia04/DevHQ/pull/6), el workflow
  de Windows ([run 38150395994](https://github.com/jpgarcia04/DevHQ/actions/runs/38150395994)) pasó la
  caminata entera bajo la protección en tiempo real, con el lector encerrado en su Job Object, y midió
  el instalador: `setup.exe` de 3 389 953 bytes en #5 a 4 075 038 (+685 085, un 20 %), y `devhq.exe`
  instalado de 13 136 384 a 15 141 376 (+2 004 992, un 15 %), casi todo libgit2. Sus tests de Rust
  fallaron en el segundo pase, con `TMP` en una carpeta con espacios y acentos: la copia con `.git`
  negado salió `unreadable`, como debe, pero por otro camino: el sistema dejó a libgit2 ver `.git` y le
  negó abrir `.git/config`, un error de clase `Os` que el lector ya respondía como `unreadable` con el
  motivo de libgit2. La prueba exigía la frase de DevHQ; ahora admite las dos (sin mirar las palabras
  del sistema, que salen en su idioma). En la laptop siempre lo detecta DevHQ, también con `TMP`
  acentuado. El pase siguiente ([run 38151813941](https://github.com/jpgarcia04/DevHQ/actions/runs/38151813941)) pasó los tests en los dos pases y la caminata, pero falló la comprobación de que los tests no dejan nada en `TMP`: quedaban la config de Git vacía y el marcador de la batería, creados una vez por ejecución desde el primer commit de la etapa. Ahora el marcador se compila en la carpeta de la batería, que la prueba borra, y `GIT_CONFIG_GLOBAL` apunta a una ruta que no existe (Git se salta una config global ausente); en la laptop, con un `TMP` nuevo, no queda nada. Falta el pase de Windows de ese cambio y fusionar.
- **Fuera de alcance, para después**: el bundle de la ventana pasó de unos 497 a 517 kB y Vite avisa
  de que supera 500 kB; es un aviso, en una app que carga desde disco, y no se cambió la configuración
  por él. Una aplicación que ya no declara ninguna copia local no ofrece «Show every reading» en la
  ventana; sus lecturas siguen guardadas y la puerta las da (`history`).
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

1. **Etapa 3, actividad de las copias locales**: ver pasar el workflow de Windows en el PR desde
   `claude/local-activity-design` (ver «Estado») y fusionarlo cuando la persona lo apruebe.
2. **Abrir una referencia, si la persona lo quiere**, solo como subproducto: un comando Rust
   `open_reference(id)` que lee la referencia de SQLite y abre en el navegador solo URL http(s); la
   ventana nunca pasa una URL y el WebView no gana plugin ni permiso. Una ruta no se abre hasta que
   DevHQ sepa en qué estación corre, y entonces solo se muestra la carpeta en el explorador, nunca se
   abre un archivo: abrir una ruta que escribió un agente podría ejecutar un programa.

## Etapa 3: diseño revisado

Responde «¿qué lleva meses sin actividad?» y «¿qué trabajo existe solo en una máquina?», y prepara
«¿qué tiene cambios sin desplegar?»: guarda el commit de HEAD, que la etapa de despliegues comparará
con el desplegado. Es la primera fuente que DevHQ lee en una máquina concreta y la primera cuyo
objetivo puede haber escrito un agente, así que empieza por un principio. Se propuso el 11 oct y se
revisó ese mismo día con seis cuestiones de la persona (ver «Qué cambió en la revisión», al final).

### El principio: observar no da poder a lo observado

Leer una fuente nunca le da a esa fuente una forma de ejecutar código ni de cambiar nada: DevHQ no
ejecuta ningún programa que la fuente nombre (hooks, filtros, programas de firma o de monitorización,
helpers de remotos) y no escribe en ella. La ruta de una `local_copy` y el repositorio que contiene
son datos no confiables, los haya declarado la persona o un agente. La garantía no puede depender de
una lista de cosas prohibidas que cada versión de Git alarga: viene de leer con algo que no sabe
ejecutar programas, de una batería de repositorios hostiles que lo exige en Linux y en Windows y,
donde el sistema lo permite, de un proceso lector al que el sistema no deja arrancar programas.

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

### Qué garantiza el lector, y qué no

Son tres garantías distintas, y cada una descansa en algo distinto:

| Garantía                            | En qué descansa                                                                                                                                                                                                                                                                                                  | Windows                                                                                                                             | Linux                                                                                                                                                                                                                                                   |
| ----------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Leer no arranca ningún programa     | libgit2 con solo sus APIs de lectura, que no tienen ningún camino que lance un proceso (y `git2` sin `https` ni `ssh`, así que tampoco transportes de red), y la batería hostil, que lo exige en cada cambio de lector o de versión. Además, el sistema impide al proceso lector arrancar programas por sí mismo | Comprobado en la laptop. El lector se mete en un Job Object con `ActiveProcessLimit = 1`: crear un proceso falla (1816), comprobado | Comprobado en el CI de Linux (run 38142657773): la batería hostil no dejó marcas. El lector se pone un filtro seccomp que responde `EPERM` a `execve` y `execveat`, también como root y heredado por cualquier hijo: crear un proceso falla, comprobado |
| Leer no escribe nada                | Solo APIs de lectura (`statuses` sin `UPDATE_INDEX`; nunca checkout, fetch ni escritura de refs) y la batería, que compara el contenido y el mtime de cada archivo antes y después y exige que no aparezca ninguno                                                                                               | Comprobado en la laptop; el sistema no lo impide                                                                                    | Comprobado en el CI de Linux; el sistema no lo impide                                                                                                                                                                                                   |
| Un fallo del lector no daña a DevHQ | Un proceso propio por copia, con plazo: un cuelgue, un bucle o un fallo de memoria de libgit2 ante datos hostiles matan al hijo y nunca a la ventana ni a la puerta, y lo que devuelve se valida como dato no confiable antes de guardarlo                                                                       | Igual                                                                                                                               | Igual                                                                                                                                                                                                                                                   |

Lo que no se garantiza, y se documenta en el README, en `CLAUDE.md` y en la descripción de la
herramienta de la puerta:

- **El encierro no contiene un exploit.** Si unos datos hostiles lograran ejecutar código dentro del
  lector, ese código tendría los permisos de la persona: podría leer y escribir lo que ella puede,
  conectarse a la red y pedirle a otro proceso que arranque un programa (WMI o el Programador de
  tareas en Windows; systemd o D-Bus en Linux), algo que ni el Job Object ni seccomp impiden. Lo que
  sí garantizan es que el lector, libgit2 incluido, no puede lanzar un programa por sí mismo. El
  riesgo se reduce manteniendo `git2` al día y llamando solo a las APIs que la batería cubre.
- **Nada del sistema impide que el lector escriba.** Lo garantizan libgit2 y la batería. Landlock
  (Linux 5.13 o posterior) o un token de integridad baja (Windows) podrían impedirlo; no se han
  probado y quedan para cuando una fuente nueva lo pida.
- **En otros sistemas** (macOS u otros Unix, que hoy no se distribuyen) el lector no sabe encerrarse,
  así que no lee y lo dice.
- **`RLIMIT_NPROC = 0` se descarta en Linux**: impide `fork`, pero no `execve`, que reemplaza al
  propio proceso por otro programa, y no se aplica a root.

### Cómo se lee

- **Biblioteca**: libgit2 por `git2` sin funciones de red (`default-features = false`,
  `vendored-libgit2`), y solo sus APIs de lectura: abrir sin buscar hacia arriba (`NO_SEARCH`: la
  ruta declarada es la raíz de la copia, así que ni una subcarpeta ni un `$HOME` que sea un
  repositorio de dotfiles se toman por ella; varias aplicaciones de un monorepo declaran la misma
  raíz), refs, commits, reflogs, config, atributos y `statuses` sin `UPDATE_INDEX` y sin submódulos.
  La comprobación de propietario de libgit2 (la de `safe.directory`) se queda activa: una copia que
  el propio `git` rechazaría sale `unreadable`, con su motivo.
- **Un proceso por copia**: `devhq --read-copy`, el mismo ejecutable que la ventana y la puerta, sin
  ventana, que vive lo que dura una lectura. Antes de leer nada se encierra (Job Object en Windows,
  seccomp en Linux); si no puede, no lee y responde por qué. Recibe la ruta por stdin y escribe dos
  líneas JSON: primero las refs y después el working tree, de modo que si el working tree no
  termina, lo demás ya llegó.
- **Quién lo lanza**: la ventana, desde un comando asíncrono que espera en un hilo aparte, o la
  puerta. Sin shell; con el entorno de quien lo lanza menos toda variable `GIT_*` (libgit2 necesita
  `PATH` y el registro para encontrar la config de Git for Windows, que pone `core.autocrlf=true`); y,
  en Windows, sin consola (`CREATE_NO_WINDOW`: en las compilaciones de desarrollo se abriría una).
- **Plazo**: 30 s por copia, que se cumplen matando al hijo, porque libgit2 no se puede interrumpir.
  Si las refs llegaron, la lectura se guarda con el working tree desconocido y el motivo; si no, es
  un fallo de DevHQ y no se guarda nada.
- **Lo que vuelve** se valida antes de guardarlo: tamaño de cada línea, tipos, límites de cada
  número, un `head` de 40 dígitos hexadecimales, textos de longitud acotada y sin caracteres de
  control. Lo que no cumple es un fallo de DevHQ.
- **Una copia tras otra**: Read all las lee en serie. Arrancar un proceso cuesta milisegundos, y así
  no hay paralelismo que gestionar ni un plazo vencido que obligue a relanzar un lector compartido.
- **Alternativas descartadas**:
  - El `git` instalado: ejecuta lo que nombra el repositorio y escribe el índice; endurecerlo es una
    lista que nunca se cierra, y depende de que haya `git` y de cuál.
  - gix: tal cual lanza `git` y ejecuta drivers; aislado sigue ejecutándolos en `status` y pierde la
    config del sistema, con resultados falsos en Windows; solo para refs es seguro y está en Rust,
    pero sin working tree, que es la mitad del valor; con un pipeline propio sin drivers, sería más
    código sobre una API 0.x que cambia a menudo.
  - Leer `.git` a mano: packfiles con deltas, el índice y las reglas de ignorados; sería
    reimplementar Git.
  - libgit2 dentro de la ventana: más simple, pero un cuelgue la dejaría esperando para siempre, un
    fallo de C la cerraría, no habría plazo real y no se puede encerrar (WebView2 necesita sus
    propios procesos).

### Qué se lee: una lectura neutral

El lector produce una **lectura** (`CopyReading` en Rust) en conceptos, no en la forma de libgit2.
La tecnología queda aparte, en `reader` («libgit2 1.9.7»), para saber qué lecturas hizo un lector si
un día resulta tener un fallo. Cada campo responde a una pregunta:

| Campo                                       | Qué es                                                                                                                                                                                                                                                                                                              | Para qué                                                                                                                |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `outcome`, `detail`                         | `read`; `missing` (nada en esa ruta); `not_a_copy` (hay algo, pero no la raíz de una copia de trabajo: una carpeta sin repositorio o dentro de uno, un repositorio bare, un archivo); `unreadable` (el sistema niega el acceso, o libgit2 no confía en el propietario). `detail`, el motivo en palabras del sistema | Una copia declarada que no está es inventario que envejece                                                              |
| `branch`, `head`                            | La rama actual (nula si HEAD está suelto) y el commit de HEAD (nulo si aún no hay commits)                                                                                                                                                                                                                          | Contexto para un agente; después, los cambios sin desplegar                                                             |
| `lastCommitAt`                              | La fecha de commit más nueva entre HEAD y las ramas locales, según el propio commit                                                                                                                                                                                                                                 | La falta de actividad                                                                                                   |
| `unpublishedOnHead`, `unpublishedElsewhere` | Commits alcanzables desde HEAD, y desde las demás ramas locales, que ninguna rama remota de la copia contiene; hasta 1000 («1000+»)                                                                                                                                                                                 | Trabajo que solo existe aquí. Separados porque las ramas viejas de un PR fusionado con squash cuentan como sin publicar |
| `stashes`                                   | Cuántos stashes guarda                                                                                                                                                                                                                                                                                              | También es trabajo que solo existe aquí, y el más fácil de olvidar                                                      |
| `syncedAt`                                  | La última vez que la copia habló con un remoto según sus archivos: lo más nuevo entre el mtime de `FETCH_HEAD` y los reflogs de las ramas remotas                                                                                                                                                                   | «Sin publicar» se compara con lo que la copia sabía entonces: DevHQ no hace fetch                                       |
| `remotes`                                   | Nombre y URL de cada remoto, hasta 20, limpia (ver abajo)                                                                                                                                                                                                                                                           | Comprobar que la copia es del repositorio declarado                                                                     |
| `operation`                                 | `merge`, `rebase`, `cherry_pick`, `revert`, `bisect` o `apply_mailbox` en curso                                                                                                                                                                                                                                     | Algo que se quedó a medias                                                                                              |
| `changed`, `untracked`, `unverifiable`      | Archivos seguidos con cambios (en el índice o en el working tree, conflictos incluidos); entradas nuevas no ignoradas (una carpeta nueva cuenta una vez, como en `git status`); archivos que parecen cambiados bajo un driver de filtro que DevHQ no ejecuta. O desconocido, con el motivo                          | Trabajo sin confirmar; lo no verificable no se cuenta como cambio ni como limpio                                        |

- **Remotos sin secretos**: libgit2 devuelve la URL tal cual, con credenciales
  (`https://user:token@…`). Solo se guardan formas conocidas, limpias: http(s) sin usuario,
  contraseña, query ni fragmento; `ssh://` sin contraseña; `usuario@host:ruta`; `git://`, `file://`
  y rutas absolutas. Cualquier otra (`ext::`, que es una línea de comandos, o lo que no se reconozca)
  se guarda solo con su nombre, y la ventana dice que DevHQ no muestra su URL.
- **Fuera, por ruido o por ser datos personales**: autor, correo y mensajes de commit; nombres de
  archivo; listas de ramas, tags y stashes; el estado de los submódulos (cada uno es su propia copia,
  si se declara); archivos ignorados, tamaños y estadísticas de diff; «behind», que dice más de
  cuándo fue el último fetch que del proyecto; la rama por defecto; objetos LFS; hooks y config. Una
  columna se añade después si una pregunta concreta la pide.
- **Siempre en pasado**: cada hecho se muestra con el momento y la máquina de su lectura, y en pasado
  («read 3 days ago on Laptop: 2 changed files, 1 commit on no remote»). No hay vigencia que haga
  caducar una lectura, porque nunca afirmó seguir siendo cierta, y por eso mismo nada la presenta como
  actual: la ventana y la puerta nunca dicen «has» ni «now» de una copia, sino lo que tenía cuando se
  leyó.

### Cómo se guarda: evidencia propia, no salud

- **Una tabla nueva**, `copy_readings` (v12), y no una fuente más de `observations`: una lectura no
  es healthy, degraded ni unhealthy, no entra en la atención ni en la cobertura de salud, y meterla en
  `observations` o `events` obligaría a reconstruirlas por sus CHECK de `source` y `result`. Es
  inmutable y lleva las columnas de toda evidencia: el sujeto (`application_id`/`resource_id`, con un
  CHECK de exactamente uno y `RESTRICT`), `source = 'read'` («Automated · Local read»), `reader`,
  `observed_at`, `recorded_at` y `outcome`, más los hechos de arriba, con CHECK que solo admiten
  hechos con `read` y exigen el working tree o el motivo de no tenerlo. Y `agent`: el agente que la
  pidió por la puerta, o nulo si la pidió la persona (ver «La puerta: leer no es configurar»).
- **Qué copia y dónde**: `reference_id`, `machine_id` y `path` se copian, sin claves foráneas, como la
  URL en una observación HTTP. `references::replace` borra y reinserta en cada guardado: una clave
  foránea con `RESTRICT` lo rompería y con `CASCADE` borraría evidencia. Así una copia se puede quitar
  o mover sin tocar su historia, y una máquina se puede borrar cuando ya no guarda rutas; sus
  lecturas conservan su ID y dicen «a machine no longer in the inventory». Si la última lectura se
  hizo en otra ruta o en otra máquina de las que declara hoy la referencia, la ventana lo dice, como
  el aviso de URL cambiada de la comprobación HTTP. Si la referencia desaparece o cambia mientras se
  lee, no se guarda nada, como una comprobación quitada en vuelo.
- **Es historia**: `history_of` la cuenta, así que un sistema con lecturas ya no se puede borrar.
- **Retirar lo que ya no se usa**: una aplicación pasa a Archived, como hoy; las copias de las
  aplicaciones archivadas salen de Read all, del panel Local copies y de las nunca leídas, y conservan
  sus lecturas. Quitar la referencia de una copia cuya carpeta ya no existe tampoco toca sus
  lecturas. Los recursos no tienen lifecycle: el que declare una copia la retira quitando la
  referencia.
- **Carga**: `list_inventory` lleva solo la última lectura de cada `local_copy` actual, por un índice
  `(reference_id, observed_at)`; las anteriores de cada sistema se paginan de 200 en 200, por índices
  `(application_id, observed_at)` y `(resource_id, observed_at)`, con pruebas de plan de consulta. Las
  lecturas no escriben eventos ni salen en Activity: leer no es algo que le pase al sistema.

### En qué estación de trabajo corre DevHQ

- **Fuera del inventario compartido**: el recurso `workstation` es inventario; «esta instalación
  corre en ese recurso» es una asociación local, en `this-workstation.json`, dentro de la carpeta
  local de DevHQ (`app_local_data_dir`: `%LOCALAPPDATA%\com.devhq.desktop` en Windows, que no viaja
  con un perfil móvil; la puerta la resuelve con `dirs::data_local_dir()`, y un test ata las dos,
  como hoy la del inventario). Back up now no la copia.
- **La huella de la máquina sí hace falta**: en Linux esa carpeta es la misma que la del inventario
  (`~/.local/share/com.devhq.desktop`), y copiar el home entero a otro equipo, o compartirlo entre
  dos, es habitual; en Windows, las herramientas de migración también copian `%LOCALAPPDATA%`. Sin
  huella, otro equipo con el archivo se creería Laptop y grabaría como lecturas de las copias de
  Laptop las de sus propias rutas, que a menudo se llaman igual: lo desconocido presentado como
  sabido.
- **Más simple que lo propuesto**: el archivo guarda, **por huella**, el recurso elegido en cada
  inventario y cuándo. La huella es un SHA-256 de una etiqueta de DevHQ y del identificador del
  sistema (`MachineGuid` en Windows, `/etc/machine-id` en Linux); el identificador en bruto nunca se
  guarda ni se muestra, y `sha2` ya está en el árbol de dependencias (lo trae wry). Sin clave de HMAC
  y sin el nombre del equipo. En otro equipo no hay entrada para su huella, así que simplemente aún no
  ha elegido; y dos equipos que comparten home conviven en el mismo archivo.
- **Vale** solo si el recurso elegido en este equipo existe en el inventario abierto y es
  `workstation`; si no, la estación es desconocida y se dice por qué: sin elegir, el elegido ya no
  está en este inventario, ya no es estación de trabajo, el sistema no da identificador o el archivo
  no se puede leer. Sin estación conocida no se lee ninguna copia.
- **Solo la ventana la cambia**, en Settings › This computer, eligiendo una estación de trabajo que ya
  exista (crearla sigue siendo cosa de Infrastructure); la puerta solo la lee, como el modo de
  acceso. No es inventario, así que no va al registro de cambios; cada lectura guarda en qué máquina
  se hizo.
- **MSIX**: una puerta lanzada desde una app empaquetada ve el archivo real si el paquete no tiene
  otro; una ventana empaquetada escribiría en la copia del paquete. `status` y Settings dicen dónde
  está de verdad (`storedAt`).
- **Alternativas**: la huella como columna del recurso (rastro en el registro de cambios, pero mete
  en el inventario compartido una identidad que pertenece a una máquina, justo lo que la persona no
  quiere); la asociación sin huella (un home copiado haría que otro equipo se creyera este); el nombre
  del equipo (se repite y cambia); preguntarlo en cada sesión (la puerta no puede preguntar);
  deducirlo de qué rutas existen (es escanear, y la misma ruta puede existir en dos equipos).

### Cuándo se lee y quién lo pide

- **Solo a petición**: Read en una copia de esta estación, Read all on this computer y la herramienta
  de la puerta. Nunca al abrir, nunca con un temporizador, nunca con la ventana cerrada.
- Solo se leen copias de esta estación. Para las de otra máquina, la ventana muestra su última
  lectura y dice que se leen desde el DevHQ de esa máquina.
- **Sin alertas**: al principio, una lectura no produce atención ni salud; solo se muestra.

### Lo derivado: qué vale y qué sería ruido

Se calcula en Rust en cada lectura del inventario, para la ventana y la puerta, y no se guarda. Nada
entra en la lista ordenada de «¿Qué necesita mi atención?», que sigue siendo de salud, así que
`contract/attention/` no cambia.

- **Última actividad de cada sistema**: el commit más nuevo entre las últimas lecturas de sus copias,
  con su base («Laptop, read today; Desktop never read»). Es una cota inferior: nunca dice
  «inactivo» si una copia no se ha leído. Applications la muestra en cada aplicación, sin un umbral
  de meses que mantener; lo que no tiene lecturas lo dice.
- **Trabajo que solo existe en una copia**: commits sin publicar, stashes, cambios, entradas nuevas o
  archivos no verificables, o una operación a medias, cada cosa con su número y la fecha de su
  lectura. Va en un panel **Local copies** de Overview, como Supervision, junto con las copias
  declaradas que no se encontraron y cuántas copias de esta estación nunca se leyeron, para que un
  panel vacío no tranquilice de más; ahí está Read all.
- **Divergencia con lo declarado**: la copia no está donde dice la referencia; o ninguno de sus
  remotos parece el repositorio que declara su sistema; o el sistema no declara repositorio y la
  copia tiene remotos, y solo se dice (proponer declararlo queda para cuando haga falta). Para
  comparar un remoto con el repositorio declarado se normalizan los dos: el host en minúsculas sin
  usuario ni puerto, y la ruta sin `/` al principio ni al final, sin `.git` y en minúsculas, para
  `https://`, `ssh://`, `git://` y `usuario@host:ruta`; una ruta local y lo que no se guardó no se
  comparan. Es el caso concreto que la igualdad exacta no resuelve: el remoto real
  `git@github.com:me/shop.git` y el declarado `https://github.com/me/shop` saldrían siempre
  distintos. Los hosts cuyo SSH y web difieren (Azure DevOps) salen como distintos, así que la
  ventana dice «no parece», y nunca es atención.
- **`sharedRepositories` no cambia**: sigue comparando por igualdad exacta, como fijó la etapa 2. La
  normalización existe solo para la comparación nueva; nada de la etapa 2 la necesita hoy.
- **Ruido, fuera**: que dos copias estén en commits distintos, en una rama que no es la por defecto o
  con HEAD suelto; un umbral fijo de «inactivo»; comparar copias entre sí.

### Ventana y puerta

- **Comandos**: `read_copies(referenceIds | null)`, asíncrono (null son todas las de esta estación,
  salvo las de aplicaciones archivadas), que responde las lecturas guardadas y, por copia, los fallos
  de DevHQ; `older_readings(subject, before)`, de 200 en 200; `this_workstation` y
  `set_this_workstation(resourceId | null)`; y `list_inventory` gana `localCopies`: cada copia local
  declarada con su última lectura y lo derivado. Cada uno con su fixture en `contract/` y sus lectores
  en `arguments()`.
- **Ventana**: Settings › This computer; en el Overview de cada aplicación y en la tarjeta de cada
  recurso, cada copia con su última lectura, su máquina, sus lecturas anteriores bajo demanda y Read
  si es de esta; el panel Local copies de Overview, con Read all; la última actividad en
  Applications. La preview muestra lecturas de ejemplo y rechaza leer, como toda escritura.
- **Puerta**: `status` dice qué estación es esta, o por qué no se sabe; `list_systems` y `get_system`
  dan, por copia, `onThisWorkstation`, su última lectura y lo derivado; `attention` añade, sin
  ordenarlos y diciendo que no son salud, `localWork` y `copiesNeverRead`, como ya hace con
  `notSupervised`; `history` admite `records: "readings"`; y una herramienta nueva, `read_copies` (un
  sistema, una referencia, o todas las de esta estación).

### La puerta: leer no es configurar

- **Bastan los tres modos de hoy.** Ver lecturas es leer el inventario: Read only. Pedir una lectura
  guarda evidencia, como `record_observation`, así que exige Read and write; con Read only un agente
  nunca provoca una lectura. No hace falta un modo nuevo ni permisos por agente.
- **No es un cambio de configuración**: `read_copies` no toca aplicaciones, recursos, referencias ni
  comprobaciones, así que no deja entrada en el registro de cambios. La propia lectura guarda qué
  agente la pidió, y la ventana y la puerta lo muestran: que un agente hiciera leer a DevHQ tu disco
  lleva su nombre, como lo lleva una comprobación que un agente hizo salir de tu computadora. Se
  declara sin `readOnlyHint` (escribe evidencia), sin `destructiveHint` (no cambia nada que ya
  exista) y sin `openWorldHint` (nada sale del equipo).
- **Lo que la hace posible sí deja rastro**: declarar o mover una copia local es editar las
  referencias de un sistema, con su entrada en el registro de cambios. Elegir esta estación no es
  inventario y solo puede hacerlo la ventana.
- **Limitaciones que se documentan**: con Read and write, un agente puede declarar cualquier ruta
  absoluta de esta estación como copia local y hacer que DevHQ la lea; DevHQ solo dirá si ahí hay una
  copia y los hechos de la tabla (nunca nombres de archivo, contenidos, autores ni mensajes), que un
  agente con terminal ya ve. Los modos son de intención, no una barrera de seguridad. Una puerta
  lanzada desde una app empaquetada (MSIX) puede ver otro inventario y otro `this-workstation.json`, y
  `status` lo dice.

### Reglas que cambian

Texto propuesto; se escribe con la implementación, en el mismo commit.

- **`CLAUDE.md`**, en «Product rules», detrás de «DevHQ observes; it does not operate systems, run in
  the background or hold credentials.»: «It reads only what the inventory names, when asked, and never
  searches a disk or a network for things to add. Observing never gives the source a way to run code
  or change anything: a reader never runs a program the source names and never writes to it, and every
  declared path, and what it holds, is untrusted data.» Y una sección «Local copies»: el lector corre
  en `devhq --read-copy`, un proceso por copia que se encierra antes de leer; un lector o una
  biblioteca nuevos, y cada versión nueva de libgit2, pasan antes la batería hostil en Linux y en
  Windows; y lo que no se garantiza (escrituras, un exploit).
- **README**: la entradilla y «Qué incluye»; una sección «Copias locales» con todo lo de arriba,
  incluido lo que no se garantiza; en «Arquitectura», «scanners» pasa a «DevHQ no busca qué observar:
  nunca recorre discos, carpetas ni redes en busca de repositorios o servicios, y lee solo lo que el
  inventario nombra»; y «Evolución prevista» gana el paso hecho.
- **`docs/vision.md`**: un principio 16 en «Qué se garantiza», «Observar no da poder a lo observado»;
  en «Tensiones», la fila «Descubrir repositorios o servicios» pasa a «DevHQ no busca qué observar; lee
  solo lo que el inventario nombra» → «Que lo lances tú, sobre un lugar que eliges, de solo lectura y
  sin ejecutar nada de lo que encuentre, y que proponga altas que aceptas»; la línea de evolución 5,
  hecha; y en «Cómo crecer sin romperlo», el patrón de abajo.

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
5. Leer no ejecuta nada de la fuente ni escribe en ella; el lector corre en un proceso propio, con
   plazo y, donde el sistema lo permite, sin poder arrancar programas; lo que no se garantiza se
   documenta; y si hay credenciales, ninguna pasa por la fuente que se lee.
6. Lo derivado se calcula al leer, en Rust, para la ventana y la puerta; si entra en la atención, se
   escribe a los dos lados con casos en `contract/attention/`.

### Riesgos

- **libgit2 es C que lee datos hostiles.** En 2024 corrigió un bucle infinito en
  `git_revparse_single` (CVE-2024-24575) y una corrupción de memoria en `git_index_add`
  (CVE-2024-24577, una escritura que DevHQ no usa). El proceso aparte, con plazo, contiene un
  cuelgue o un fallo, y su salida se valida; un exploit no queda contenido (ver «Qué garantiza el
  lector, y qué no»). `git2` se mantiene al día, y cada versión pasa la batería.
- **libgit2 va detrás de Git**: un formato que no conoce (`extensions.refStorage = reftable`,
  `objectFormat = sha256`, un índice partido con `core.splitIndex`) es un fallo de DevHQ («este
  formato aún no se puede leer»), no evidencia de la copia; si solo afecta al índice, la lectura se
  guarda con el working tree desconocido.
- **La comprobación de propietario** de libgit2, como `safe.directory`, rechazaría copias en FAT,
  exFAT o recursos de red cuyo propietario no reconoce (sin comprobar): saldrían `unreadable`, con
  el motivo.
- **Working tree**: sin su driver, un archivo LFS tocado es no verificable; sin fsmonitor ni untracked
  cache, un árbol enorme tarda más, y el plazo lo acota. Leer mientras la persona ejecuta un `git`
  pesado puede fallar una vez; es un fallo de DevHQ y se vuelve a pedir.
- **Commits sin publicar** en ramas viejas (squash): se ven aparte, pero se ven.
- **La huella de la máquina**: las VMs clonadas sin sysprep comparten `MachineGuid`; un Linux sin
  `machine-id` no puede elegir estación; reinstalar el sistema obliga a elegirla otra vez.
- **Las lecturas hacen imborrable su sistema**; una aplicación se archiva.
- **Una dependencia C nueva** (libgit2 y zlib, compiladas con cc como ya se compila SQLite): hay que
  verla compilar en el CI de Linux y de Windows y medir cuánto crece el instalador.
- **seccomp en otros Linux**: el filtro se puso en el runner de GitHub (Ubuntu); un kernel o un
  contenedor que no lo admitiera dejaría al lector sin leer, y lo diría.
- **Una ruta escrita por un agente** puede apuntar a cualquier carpeta que el usuario pueda leer:
  DevHQ solo dirá si es una copia y los hechos de la tabla.

### Pruebas

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
   `git-lfs`. La config hostil se escribe al final, para que el `git` que construye los repositorios
   no la dispare. Se exige: ninguna marca; cada archivo de la copia igual en contenido y mtime; ningún
   archivo nuevo; y la lectura, correcta. La batería pasa por el lector real, `devhq --read-copy`
   encerrado, y no solo por la función que lee.
2. **Encierro**: en modo lector, arrancar cualquier programa falla en Windows y en Linux, también
   como root donde el CI lo sea; si el encierro no se puede poner, no se lee.
3. **Contra `git`**, donde el CI lo tiene: gemelos benignos (ramas, HEAD suelto, sin commits, merge
   y rebase en curso, sin publicar en HEAD y en otras ramas, stashes, carpetas nuevas, el
   `core.autocrlf` del sistema en Windows leído por el lector encerrado, un driver real que da no
   verificable), comparados con `git status --porcelain`, `git rev-list` y `git stash list`.
4. **Resultados**: ruta que no existe, archivo, carpeta vacía, subcarpeta de una copia, repositorio
   bare, sin permiso (`chmod 000` en Linux) y formato desconocido, que no guarda nada.
5. **Plazo y salida**: un lector que se bloquea se mata y queda la lectura de refs con el working
   tree desconocido; una salida malformada, incoherente o enorme se rechaza.
6. **Remotos**: credenciales que se quitan y normalización, con casos (`https://token@…`,
   `ssh://u:p@…`, `git@host:x.git`, Azure DevOps, rutas locales, `ext::`).
7. **Esquema**: la actualización v11→v12 comparada fila por fila; CHECK que rechazan lecturas
   incoherentes; `history_of` que impide borrar; editar o quitar la referencia no toca sus lecturas;
   una referencia quitada mientras se lee no guarda nada; y los planes de consulta de la última
   lectura por copia y de la paginación.
8. **Esta estación**: otra huella no ve lo elegido; un recurso borrado o que deja de ser estación de
   trabajo la deja desconocida; la ventana y la puerta resuelven el mismo archivo; la puerta nunca lo
   escribe.
9. **Contratos IPC**, la preview y la **caminata nativa** en Linux y en Windows, con la protección
   en tiempo real: elegir esta estación, declarar una copia hostil con un commit sin publicar y un
   cambio, Read, ver Local copies y comprobar que no quedó ninguna marca.

Cada prueba nueva se confirma fallando sin su arreglo: por ejemplo, con el lector cambiado por el
`git` instalado, la batería hostil tiene que fallar.

### Qué cambió en la revisión

El 11 oct la persona leyó el diseño, aceptó la dirección general y pidió revisar seis cuestiones:

1. **Seguridad del lector**: se separan las tres garantías, con lo que está comprobado en cada
   sistema y lo que no (ver la tabla); en Linux, seccomp sobre `execve` en vez de `RLIMIT_NPROC`, que
   no lo impide; y queda escrito que el encierro no contiene un exploit ni impide escribir.
2. **Complejidad**: la lectura completa entra ya, porque el working tree es la mitad del valor y la
   protección es la misma para todo; lo que se simplifica es la maquinaria: un proceso por copia con
   un solo plazo, sin entorno a medida, sin crear estaciones desde Settings y sin proponer el
   repositorio.
3. **Qué se lee**: los mismos campos, más `stashes`; los remotos, solo en formas conocidas y sin
   credenciales; todo en pasado y con su momento.
4. **Esta estación**: sigue fuera del inventario; la huella hace falta (en Linux el archivo vive
   junto al inventario), pero como clave del archivo, un SHA-256 sin clave de HMAC y sin el nombre del
   equipo.
5. **Historia y atención**: archivar una aplicación la retira de las lecturas y los paneles sin
   tocar su historia; ninguna alerta; `sharedRepositories` no cambia y la normalización solo sirve
   para comparar el remoto real con el repositorio declarado.
6. **Puerta**: bastan los tres modos; leer exige Read and write, no deja entrada en el registro de
   cambios y guarda qué agente la pidió; las limitaciones quedan escritas.

No queda ninguna decisión pendiente de la persona: se implementa en un PR desde
`claude/local-activity-design`, y los textos de las reglas que cambian se revisan en ese PR.

## Lo que solo se puede hacer en la laptop

- Lo pendiente de la puerta en [docs/puerta-para-agentes.md](puerta-para-agentes.md): un Claude Code
  real contra el `devhq.exe` instalado y una puerta lanzada desde la app de escritorio de Claude
  (MSIX). La persona prefiere no instalar DevHQ hasta que sea usable.
- `npm run e2e:native` en Windows, que necesita apartar antes los datos de `%APPDATA%` y
  `%LOCALAPPDATA%`.

Una sesión en la nube corre todo lo demás, incluido `npm run e2e:native` sobre WebKitGTK en Linux
(ver «Cloud sessions» en `CLAUDE.md`), y el workflow de Windows prueba lo suyo en cada PR.
