# Leer GitHub (propuesta)

Estado: **en construcción** (24 nov 2026). La persona aprobó esta decisión y su implementación. El
primer paso, guardar el token, ya existe y reescribió la regla «DevHQ no guarda credenciales»
([especificación](especificacion.md), «Token de GitHub»); la lectura todavía no. La especificación
manda. Los repositorios
y sistemas de los ejemplos son inventados.

## El problema

El [trabajo pendiente](especificacion.md#trabajo-pendiente) responde «¿qué necesita mi atención?» con
lo que DevHQ ve en este equipo: las copias locales y las instalaciones. Pero en un ecosistema personal
buena parte del trabajo ya no vive en el disco: vive en la plataforma donde está el código. Las ramas
se abren y se fusionan desde sesiones en la nube, la CI corre allí y lo que se olvida es un pull
request abierto o una rama principal que dejó de pasar sus comprobaciones. DevHQ no ve nada de eso, así
que puede decir «nada que hacer» mientras la rama principal de un sistema lleva días en rojo.

Antes de proponer nada se hizo la prueba a mano: se leyó con permisos de solo lectura lo que GitHub
diría hoy de cada repositorio de un ecosistema real de cuatro aplicaciones (los datos no se copian
aquí) y se separó lo que habría servido de lo que sobraba.

- **Sirvió: la rama principal en rojo.** Las comprobaciones programadas de la rama principal de uno de
  los repositorios fallaron durante una semana, por dos avisos de seguridad en dependencias, y nadie lo
  notó hasta que otro cambio lo arregló de rebote. Era exactamente lo que DevHQ tenía que haber dicho
  el primer día.
- **Sirvió: un pull request olvidado.** Uno con una auditoría de seguridad llevaba casi dos semanas
  abierto, con sus comprobaciones en verde y sin fusionar.
- **Sobró: las issues** (el ecosistema no las usa: estaban vacías en los cuatro repositorios) y las
  ramas (una veintena, casi todas ya fusionadas: listarlas sería ruido).
- **No se pudo ver:** las alertas de Dependabot, que piden un permiso aparte. Y lo que hay instalado
  en este equipo no sale en GitHub: eso lo siguen diciendo las instalaciones.

La conclusión es que la señal útil de la plataforma es poca y muy concreta. Esta propuesta lee esas
dos cosas y ninguna más; otras (Dependabot, publicaciones frente a lo instalado) se valorarán después
con el mismo método: primero una prueba a mano que diga si sirven.

## La propuesta

### Qué lee

Solo los repositorios que el inventario ya nombra: la referencia `repository` de una aplicación o un
recurso cuya URL sea `https://github.com/<propietario>/<repositorio>`, sin nada más detrás. DevHQ no
lista los repositorios de una cuenta ni busca otros que añadir. De cada uno lee, con la API REST:

1. **La rama principal** que GitHub declara para el repositorio.
2. **La última ejecución terminada de cada workflow en esa rama**, sea por un push, por una
   programación o lanzada a mano: su nombre, su conclusión, el commit y cuándo terminó. Una ejecución
   cancelada no cuenta como fallo (suele ser una que otra más nueva sustituyó) y se mira la anterior.
   Un repositorio cuyos workflows solo corren en pull requests no tiene ninguna en la rama principal:
   eso se dice como «sin comprobaciones en la rama principal», nunca como «en verde».
3. **Los pull requests abiertos**: número, título, si es borrador y cuándo cambió por última vez.

Nada más: ni el contenido del código, ni las issues, ni las ramas, ni los registros de la CI.

### Qué dice

Lo que lee entra en el trabajo pendiente con dos motivos nuevos, con su momento de lectura como el
resto, y no en «Needs attention» ni en la cobertura: una CI en rojo dice que la rama principal no pasa
sus comprobaciones, no que la aplicación esté caída.

- `main_failing`: la última ejecución terminada de algún workflow de la rama principal falló o agotó su
  tiempo. Va primero, por delante de una operación de git a medias, porque es lo único de la lista que
  puede estar rompiendo algo ahora.
- `pr_waiting`: un pull request abierto sin cambios desde hace más de 7 días (borradores incluidos, que
  lo dicen). Va después de los stashes.

Un repositorio que no se pudo leer (sin red, token caducado o sin permiso, repositorio que ya no
existe) da `not_found`, como una copia que no está donde se declaró: nunca se presenta como en calma.
Los dos motivos y su orden se fijarían en `contract/pending-work/`, como los demás.

### Cuándo

Como las copias locales y las instalaciones: cuando la persona pulsa leer y una vez al abrirse la
ventana. Sin sondeo con la ventana abierta, sin webhooks y sin nada en segundo plano. La puerta para
agentes podría pedir una lectura con «Read and maintain», como `read_copies`, y nunca vería el token.

### Qué guarda

Cada lectura es una observación inmutable con su propio `source`, `github`, en una tabla nueva de una
migración nueva: el repositorio, la rama principal, el estado de cada workflow leído y los pull
requests abiertos con lo justo para mostrarlos. Lo que devuelve GitHub es dato no confiable: se
comprueba su forma, se limitan los tamaños y el número de elementos, y un título se guarda como texto,
nunca como algo que se interprete.

## La credencial

Es lo que rompe una regla, así que se fija aquí entera.

- **Un token de grano fino de GitHub**, que la persona crea en su cuenta, limitado a los repositorios
  que elija y con permisos de solo lectura: `Metadata`, `Actions` y `Pull requests`. Con él GitHub no
  deja escribir nada, ni siquiera por error. Es gratis, no gasta minutos de Actions y permite 5.000
  consultas por hora; una lectura cuesta unas tres por repositorio. Caduca cuando la persona elija (un
  año como mucho), y entonces las lecturas pasan a `not_found` con el motivo.
- **Dónde vive**: en el almacén de credenciales del sistema (en Windows, el Administrador de
  credenciales), con el nombre del identificador de DevHQ. Nunca en SQLite, en React, en
  `localStorage`, en un registro, en un mensaje de error, en una exportación ni en lo que sirve la
  puerta. La ventana lo pide en Settings, no lo vuelve a mostrar y ofrece borrarlo.
- **Por dónde viaja**: solo a `api.github.com`, por HTTPS con el certificado verificado, y ninguna URL
  ni dato que venga de GitHub decide adónde se envía.
- **Sin token**, GitHub responde por los repositorios públicos con un límite de 60 consultas por hora.
  Bastaría para empezar con los públicos sin romper ninguna regla, pero en un ecosistema con
  repositorios privados no responde la pregunta, así que no se propone como solución sino como la forma
  de probar el lector antes de guardar nada.

### Reglas que cambia

El paso que guarda el token reescribió, en el mismo commit que su código, la regla de CLAUDE.md
«DevHQ observes; it does not operate systems, run in the background or hold credentials», la de la
especificación que la repite y la fila «Leer servidores o plataformas con credenciales» de
[vision.md](vision.md) («Tensiones con las reglas actuales»): DevHQ guarda **una** credencial, de
solo lectura, en el almacén del sistema, para leer lo que el inventario nombra. «DevHQ observa, no
opera» y «no funciona en segundo plano» no cambian.

### Lo decidido al guardar el token

- En Linux, que solo sirve para desarrollo y CI, el almacén es el llavero de usuario del núcleo, que
  no sobrevive a un reinicio, en vez del Secret Service, que exigiría D-Bus y otra dependencia. En
  cualquier otro sistema no se guarda nada.
- DevHQ comprueba la forma del token (`github_pat_` y letras, dígitos y guiones bajos) y rechaza los
  clásicos por su nombre, pero no pregunta a GitHub si vale: guardarlo no abre ninguna conexión. Lo
  sabrá la primera lectura.
- No se escribió un almacén propio ni se añadió una dependencia: las llamadas del Administrador de
  credenciales ya están en `windows-sys`, y las del llavero en `libc`.

## Lo que queda fuera

Escribir en GitHub (comentar, fusionar, relanzar la CI), las issues, las ramas, los registros de la
CI, Dependabot, las publicaciones, otras plataformas (GitLab, Gitea) y cualquier lectura sin que la
persona abra la ventana o la pida. Cada una sería otra decisión.

## Cómo se construiría

Dos pull requests y un solo instalador: el primero, el almacén de la credencial y su sitio en Settings,
con el cambio de regla; el segundo, el lector, la migración, los dos motivos en el trabajo pendiente y
en `attention`, y la versión nueva. El lector se prueba contra respuestas inventadas y hostiles
(tamaños enormes, campos que faltan, tipos cambiados, redirecciones), nunca contra la cuenta de la
persona.

## Preguntas abiertas

- ¿7 días es el umbral correcto para un pull request olvidado, o conviene que se elija en Settings?
- ¿Se empieza leyendo solo los repositorios públicos sin token, para ver el lector funcionando antes de
  guardar la credencial?
