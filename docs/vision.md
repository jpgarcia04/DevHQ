# Visión a largo plazo

> «DevHQ no es un launcher de proyectos; es la capa de contexto y control de mi ecosistema de
> desarrollo.»

Este documento guarda hacia dónde quiere ir DevHQ y con qué criterios se valoran las funcionalidades
y las decisiones de arquitectura que lo acerquen. **No es una especificación ni un backlog**: el
[README](../README.md) describe lo que DevHQ hace y manda mientras no cambie, y nada de lo que se
lee aquí existe hasta que el README lo diga. Muchas ideas cambiarán al avanzar el proyecto. Cuando
una se construye, su comportamiento pasa al README en el mismo commit; cuando se corrige o se
descarta, se cambia aquí, con su motivo.

Nació el 6 oct 2026 de un planteamiento que se conserva íntegro al final (ver «Planteamiento
original»), revisado contra lo que DevHQ era ese día: en `main`, el inventario en SQLite (esquema
v6) con observaciones, incidencias, eventos y comprobaciones HTTP(S) supervisadas mientras la
ventana está abierta; en la rama `claude/practical-hamilton-2tkqys`, aún sin fusionar, la puerta
para agentes (`devhq --mcp`, esquema v9).

## La idea

DevHQ pasa, poco a poco, de inventario con evidencia a capa de **contexto, control y memoria** de
todo tu ecosistema de desarrollo: una **representación viva** de ese ecosistema, no un inventario
estático. Sabe qué existe, dónde está, cómo se relaciona, qué estado tiene, desde cuándo y según
quién, y qué necesita tu atención; y se lo cuenta igual a ti, en la ventana, que a cualquier agente
al que le pidas trabajar en él. No reemplaza a las herramientas especializadas que hacen el trabajo
(git, GitHub, Docker, systemd, SSH, Nginx, una base de datos, tu editor, un agente) ni guarda lo que
ya guardan ellas: las integra, las pone en contexto y ofrece una visión común sobre ellas.

La prueba de que va bien: con 50 proyectos, DevHQ sigue respondiendo de un vistazo qué necesita
atención, qué está desplegado y dónde, qué lleva meses sin actividad, qué depende de un servidor o
de una base de datos, qué cambios no se han desplegado, dónde están los logs de lo que falla y qué
debe saber un agente que entra a un proyecto. Hoy responde la primera y, en parte, la de las
dependencias; el resto es la dirección (ver «Preguntas que debe poder responder»).

## Principios

No son requisitos rígidos sino criterios: sirven para decidir qué entra, cómo y en qué orden.

### Qué es DevHQ

1. **Una representación viva, no un inventario estático.** Lo que se observa se renueva solo, lo que
   caduca se ve caducado y lo que se deriva (la atención, lo que depende de qué, lo que lleva meses
   parado) se calcula al leer. Inventario, evidencia y estado derivado no se mezclan (ver «El modelo
   a largo plazo»).
2. **Entender antes que acumular botones.** El valor está en entender bien el ecosistema y las
   relaciones entre sus partes, no en sumar acciones o accesos directos. Las acciones, las
   integraciones y los agentes son adaptadores sobre el modelo.
3. **Conceptos, no proveedores.** El núcleo gira alrededor de aplicaciones, recursos, relaciones,
   estado, historial y contexto. Claude, Codex, GitHub, Docker, Oracle o cualquier otro proveedor
   son integraciones sustituibles: aparecen como procedencia (`source`), como evidencia o como
   referencia, nunca como la forma del modelo. Si cambias de agente, de plataforma, de tecnología o
   de base de datos, cambia un adaptador y DevHQ sigue teniendo sentido.
4. **Integrar, no reemplazar.** DevHQ no sustituye sin necesidad a las herramientas especializadas:
   las integra, las pone en contexto y ofrece una visión común sobre ellas. Guarda identidad,
   relaciones y evidencia, y sabe dónde está lo demás: el código en su repositorio, los logs en su
   servidor, las métricas en su herramienta, los secretos en el almacén del sistema.
5. **Lo que el repositorio no sabe.** A ti y a los agentes, DevHQ les aporta sobre todo el contexto
   que el repositorio no puede conocer por sí solo: dónde corre algo, qué depende de qué, qué
   versión está desplegada, qué estado tiene, qué incidencias hay abiertas y qué recursos tiene
   asociados.

### Cómo se valora lo que entra

6. **La atención manda.** Cada vista responde «¿Qué necesita mi atención?», y a escala eso significa
   priorizar y agregar, no listar.
7. **Pensado para escala.** Toda funcionalidad pasa por la pregunta «¿esto sigue siendo útil si hay
   50 aplicaciones, varios servidores y muchos recursos asociados?». Abrir una carpeta puede ser
   cómodo; responder qué está desplegado, qué depende de qué o qué falla en todo el ecosistema es lo
   que vale.
8. **Observable antes que tecleado.** Siempre que se pueda, la información que se observa o se
   verifica automáticamente gana a la que depende de mantenimiento manual: un dato que nadie
   mantiene se desfasa y, con 50 aplicaciones, nadie los mantiene todos.
9. **Crecer por preguntas.** Una entidad, una relación o una fuente nueva llega cuando una pregunta
   concreta la necesita, y esa pregunta es su prueba.

### Qué se garantiza

10. **Nada desconocido parece sano.** Procedencia, momentos y vigencia siempre visibles; lo que
    reporta alguien no se confunde con lo que DevHQ mide; una lista recortada dice dónde y por qué.
11. **La historia no se reescribe.** Las observaciones son inmutables, lo que tiene historial no se
    borra y un cambio de esquema conserva cada fila.
12. **Trazabilidad, igual para todos.** Es una propiedad del sistema, no de la puerta: cada cambio
    significativo deja rastro, lo haga un agente o lo hagas tú desde la ventana. Qué cambió, cuándo,
    sobre qué aplicación o recurso, el valor anterior y el que quedó cuando aplica, quién lo hizo
    (tú, o qué agente) y con qué resultado. Significativo no es cada clic: crear, editar o borrar
    algo del inventario, cambiar una relación o una comprobación, abrir o cerrar una incidencia, sí;
    abrir una vista o cambiar un filtro, no. Hoy lo cumplen la procedencia de cada registro y, con
    la puerta, `agent_log` para los agentes; tus cambios de configuración todavía no dejan rastro
    (ver «Estado actual e historia»).
13. **La observabilidad es la base.** Las capacidades operativas, si se incorporan, se construyen
    poco a poco sobre esa base y son explícitas, seguras, controlables y auditables, las uses tú
    directamente o un agente al que hayas autorizado. «DevHQ observa, no opera» es una regla del
    estado actual, no un límite permanente de la visión (ver «Centro de control»).
14. **La IA es una capa, no el fundamento.** DevHQ tiene valor entero sin agentes y lo conserva
    aunque cambien por completo los agentes o los proveedores. Los agentes trabajan con el mismo
    estado y contexto operativo que ve DevHQ, por los mismos casos de uso que la ventana, pero sus
    capacidades son explícitas, auditables y sujetas al acceso que les concedes.
15. **Personal y local.** Un archivo SQLite que es tuyo, sin cuentas ni nube. Que DevHQ trabaje por
    su cuenta con la ventana cerrada exige decidirlo de forma explícita.

## Revisión del planteamiento

Qué se conserva, qué se adapta a la arquitectura actual y qué se pospone, y por qué.

### Centro de control: primero saber, después actuar

En infraestructura, un _control plane_ decide y aplica el estado deseado: Kubernetes reconcilia,
Terraform aplica. Hoy DevHQ observa y no opera, y para el estado actual la regla tiene sentido:
puedes confiar en él porque no puede romper nada, y no necesita credenciales. Pero es una regla del
presente, no un límite de la visión: a largo plazo DevHQ puede actuar sobre el ecosistema, por ti
directamente o por agentes que hayas autorizado. Lo que importa es el orden. Primero, la mitad del
_control plane_ que sabe: el único lugar que conoce el estado de todo y sus relaciones. Después,
poco a poco, la mitad que actúa, apoyada en la primera, porque una acción solo es segura si DevHQ
sabe sobre qué actúa, qué depende de ello y puede observar qué resultó.

Cada capacidad operativa que se incorpore cumple cuatro condiciones:

- **Explícita**: tiene nombre y alcance (reiniciar este servicio, desplegar esta versión en este
  recurso), nunca una terminal remota ni comandos arbitrarios, y no ocurre sin que alguien la pida.
- **Segura**: la ejecuta la herramienta que corresponde (systemd, Docker, git, la plataforma de
  despliegue), que DevHQ no reimplementa, con las credenciales en el almacén del sistema, y una
  observación posterior verifica el resultado: el Trigger → Condition → Action → Verification que ya
  anticipa la preview de Automations.
- **Controlable**: tú decides quién puede usarla, tú o un agente, y hasta dónde, y puedes retirar
  ese permiso al instante.
- **Auditable**: queda en el mismo registro que cualquier otro cambio, con quién la pidió, qué se
  hizo y qué resultó, y no se puede borrar ni reescribir.

La puerta para agentes ya sigue este patrón en pequeño, dentro del propio inventario: un agente
cambia datos con capacidades parecidas a las tuyas, solo con el acceso que eliges en Settings, por
los mismos casos de uso que la ventana, y cada cambio queda en `agent_log` en la misma transacción.
Es el precedente natural para lo que algún día actúe fuera de DevHQ. Mientras tanto, la regla actual
se mantiene (ver «Tensiones con las reglas actuales»).

### El modelo antes que los botones: se conserva, ya es el diseño

DevHQ ya está construido así: sujetos con ID, relaciones por ID con claves foráneas, registros
inmutables con procedencia, momentos y vigencia, y la atención calculada a partir de ellos (en la
ventana y, para la puerta, en Rust, atadas por `contract/attention/`). Lo que la visión pide es que
ese modelo crezca sin romperse (ver «Cómo crecer sin romperlo»).

### «¿Sigue siendo útil con 50 proyectos?»: se adopta como criterio

Se suma a «¿Qué necesita mi atención?» y a «si solo ahorra escribir un comando sencillo, no
pertenece». Tiene tres consecuencias prácticas:

- Las vistas priorizan y agregan. Con 50 proyectos nadie recorre 50 tarjetas: lo que no necesita
  atención se resume (cuántos, con qué cobertura) y lo que sí, se ordena.
- El coste de una consulta no crece con el historial, y lo que crece con el número de sistemas se
  mide. Ya es regla (índices y pruebas de plan de consulta) y ya se mide con 5, 20 y 50 sistemas.
- El dato tiene que poder mantenerse solo, o casi (ver «El inventario envejece»).

Las comodidades pequeñas, como abrir la carpeta de un proyecto, se adaptan: caben como subproducto
de algo que el modelo ya sabe (si DevHQ conoce la ruta local de un proyecto, ofrecer abrirla al lado
es barato), nunca como motivo de una funcionalidad ni como una colección de lanzadores y atajos.

### Proyectos: la aplicación ya es el proyecto

No hace falta una entidad «proyecto» paralela. La unidad que mantienes en DevHQ es la aplicación:
tiene lifecycle, versión, componentes, evidencia, incidencias y actividad. La visión la enriquece en
vez de envolverla. Si llega un proyecto que una aplicación no representa bien (una librería, un
monorepo con varias aplicaciones, unos dotfiles), primero se intenta con relaciones (el mismo
repositorio referido desde varias aplicaciones) y, si hace falta, con otro nombre; un nivel nuevo,
solo si eso no basta.

### Servidores y el VPS: sí, por escalones de lo que exigen

Un servidor ya es un recurso, con su propia comprobación HTTP(S), cada aplicación que corre en él
tiene la suya y la demo ya muestra un servidor compartido con Docker, Nginx, SSH y Fail2ban. Lo que
propone el planteamiento (servicios, contenedores, CPU, RAM, disco, Nginx, despliegues, health
checks, logs y operaciones básicas) se adapta por escalones, según lo que cada uno exige de las
reglas:

1. **Sin credenciales, ya posible.** La comprobación HTTP(S) del servidor y la de cada aplicación
   que corre en él (hoy, una por sujeto), observaciones y eventos a mano y, con la puerta para
   agentes, un agente que ya tiene tu acceso SSH en tu terminal mira el servidor y registra lo que
   vio como observación `agent`. DevHQ no abre conexiones ni guarda nada secreto, y la etiqueta dice
   que es algo reportado, no medido.
2. **Lectura con credenciales.** Un adaptador de solo lectura (por ejemplo SSH con comandos fijos
   que define DevHQ: estado de servicios, contenedores, disco) que produce observaciones con su
   propio `source`. Exige el almacén nativo de credenciales que el README ya prevé, verificar la
   clave del servidor y revisar la regla «no guarda credenciales». Nunca una terminal remota dentro
   de DevHQ ni comandos arbitrarios.
3. **Operaciones**, como reiniciar un servicio o volver a desplegar: el último escalón, construido
   sobre los anteriores, porque una acción necesita saber sobre qué actúa y poder observar qué
   resultó. Las lanzas tú o un agente autorizado por la puerta, y exigen cambiar la regla «no opera
   sistemas» y cumplir las cuatro condiciones de «Centro de control».

Las métricas entran como evidencia, no como series: «disco al 91 % en /, umbral 85 %» es una
observación con sus valores, su momento y su vigencia, y eso es lo que responde qué necesita
atención. Las gráficas siguen en la herramienta que las tenga, y DevHQ sabe dónde está. Lo mismo con
los logs: DevHQ sabe dónde están (una unidad de systemd, un contenedor, un archivo, una URL) y, como
evidencia de una observación, guarda como mucho el fragmento acotado que la justifica; nunca el
flujo, que además puede llevar secretos.

### La capa de IA: se conserva, y es la dirección de la puerta

La puerta para agentes ya sigue esta idea: el mismo ejecutable habla MCP, un protocolo abierto, así
que Claude Code, Codex u otro cliente MCP leen y mantienen el inventario con el acceso que eliges en
Settings, por los mismos casos de uso que la ventana, y lo que registran lleva su procedencia. Para
que siga así:

- Los agentes vienen a DevHQ. DevHQ no llama a modelos, no guarda claves de API y funciona entero
  sin ningún agente. Ya se decidió que la supervisión con la ventana cerrada, si llega, la hará
  DevHQ mismo y no un agente.
- La puerta expone el modelo de DevHQ (sistemas, evidencia, atención), no conceptos de un proveedor.
  Si MCP se sustituye, se sustituye el adaptador, no los casos de uso.
- Lo que un agente reporta no es lo que DevHQ mide, y ninguna vista los confunde.

### Contexto para agentes: el objetivo sí, la forma se adapta

Que un agente entre a un proyecto sin empezar de cero es un buen objetivo, pero DevHQ no debe
convertirse en otra copia de la documentación. La documentación, las convenciones y las
instrucciones para agentes (README, `CLAUDE.md`, `AGENTS.md`) viven en el repositorio, versionadas
con el código: una copia en DevHQ se desfasaría, y escribirlas en el repositorio por su cuenta, sin
que nadie lo pida, no sería ni explícito ni controlable. Es la misma razón por la que la puerta
muestra el comando de registro para copiarlo y nunca toca la configuración de Claude Code.

Lo que DevHQ aporta es lo que el repositorio no puede saber: dónde corre, qué depende de él y de qué
depende, qué estado tiene y desde cuándo, qué incidencias siguen abiertas, qué se desplegó por
última vez y con qué versión, qué otras aplicaciones comparten su servidor, dónde están su
repositorio, su documentación y sus logs, y qué no se sabe. Cuando toque, la forma natural es un
resumen por sistema en la puerta (una extensión de `get_system`, o un recurso o un prompt de MCP),
compacto, con momentos y vigencia, que además señale las instrucciones del propio repositorio para
que el agente las lea allí. Un resumen de arquitectura que escriba un agente puede guardarse como lo
que es: algo reportado por ese agente, con su fecha.

## El modelo a largo plazo

Tres capas que no se mezclan:

- **Inventario**: qué existe y cómo se relaciona. Aplicaciones, recursos, componentes y, más
  adelante, referencias y más relaciones. Cambia poco y lo declaras tú o un agente. Guarda cómo está
  cada cosa ahora; su historia vive en el registro de cambios (ver «Estado actual e historia»).
- **Evidencia**: qué pasó y qué se vio. Observaciones, incidencias y eventos, inmutables, con
  procedencia, momento observado y registrado, y vigencia. Los despliegues y la actividad de un
  repositorio también son evidencia.
- **Estado derivado**: lo que se calcula al leer y nunca se guarda como verdad. La atención, la
  cobertura, «sin actividad desde hace cuatro meses», «hay commits sin desplegar», «si cae este
  servidor, caen estas tres aplicaciones». Se calcula con las mismas reglas en la ventana y en la
  puerta.

Así, «abandonado» no es un estado que haya que mantener a mano: es la ausencia de actividad
observada, derivada, y no dice nada de la salud, igual que lifecycle no expresa health. `Archived`
sigue siendo una decisión tuya; «sin actividad» es algo que se observa.

### Estado actual e historia

Historial no es lo mismo que estado actual. DevHQ debe poder decir cómo está algo ahora y, cuando
importa, cómo llegó ahí: qué configuración tenía una aplicación antes del último cambio, qué se
desplegó antes de lo que corre hoy, cómo evolucionó una incidencia, desde cuándo un componente corre
en otro servidor, qué cambió un agente y qué quedó después.

La evidencia ya es historia: una observación, una incidencia con el historial de sus estados o un
evento no se reescriben, y el estado actual se deriva de ellos. El inventario no: guarda solo cómo
está cada cosa ahora, y editarlo pisa el valor anterior. Su historia es un **registro de cambios**
que, en la misma transacción que cada cambio significativo, guarda qué cambió, el valor anterior y
el posterior cuando aplica, cuándo y quién: tú o un agente, con su nombre. Lo que ya es evidencia no
se repite en él, porque ya dice quién lo escribió.

Ese registro es **uno solo para ti y para los agentes**, con el actor como un dato más, no dos
mecanismos separados. `agent_log`, en la rama de la puerta, ya cubre lo que cambian los agentes: si
se diseña un sistema de eventos o de auditoría, crece desde él hacia un registro común en vez de
levantar otro al lado. Hoy el valor anterior y el posterior solo forman parte de su resumen en texto
(«version «1.3.0» → «1.4.0»»); el común los necesita estructurados para responder «cómo llegó a
esto» y no solo «quién tocó esto». Como todo historial, se lee por índice, para que su coste no
crezca con los años, y se muestra en Activity mezclado con lo demás, con la etiqueta de su actor.

### Dónde encaja cada cosa

| Cosa                            | Dónde encajaría                                                                                                       | Por qué                                                                                                   |
| ------------------------------- | --------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| Proyecto                        | La aplicación (existe)                                                                                                | Es la unidad que ya mantienes                                                                             |
| Repositorio                     | Referencia de la aplicación y fuente de evidencia (commits, actividad)                                                | No es dónde corre sino dónde está el código; un monorepo es la misma referencia desde varias aplicaciones |
| Copia local de trabajo          | Referencia ligada a una estación de trabajo: la ruta en esa máquina                                                   | El mismo proyecto puede estar clonado en varias máquinas                                                  |
| Servidor, VPS, laptop           | Recurso `server` o `workstation` (existe)                                                                             | Es donde corren los componentes                                                                           |
| Base de datos                   | Componente si es de una sola aplicación (existe); recurso si la comparten varias                                      | «Qué aplicaciones usan esta base de datos» exige que sea un recurso                                       |
| Contenedor                      | Componente, el papel estable (existe, como `docker`); los contenedores concretos, como evidencia                      | Van y vienen: inventariarlos uno a uno envejecería en horas                                               |
| Despliegue                      | Evento `deployment` (existe) con versión o commit, entorno y recurso de destino                                       | Es historia inmutable; lo desplegado se deriva del último                                                 |
| Entorno                         | Hoy, el lifecycle de la aplicación, uno solo; varios, cuando hagan falta, como dimensión de componentes y despliegues | Una aplicación con producción y staging a la vez no se puede expresar hoy                                 |
| Dominio                         | Recurso de un tipo nuevo, con su comprobación                                                                         | La caducidad del dominio y de su certificado es atención pura                                             |
| API                             | Lo que expone una aplicación: una relación «usa» entre aplicaciones y la comprobación del endpoint                    | Responde qué se rompe si cambia o cae                                                                     |
| Documentación                   | Referencia                                                                                                            | Vive en el repositorio o en la web                                                                        |
| Logs                            | Referencia; un fragmento acotado como evidencia de una observación                                                    | Nunca el flujo                                                                                            |
| CPU, RAM, disco                 | Evidencia: una observación con sus valores                                                                            | Nunca series                                                                                              |
| Agentes de IA                   | Procedencia (`agent`) y clientes de la puerta                                                                         | No son inventario                                                                                         |
| CI, paneles, otras herramientas | Fuentes de evidencia con su `source`, y referencias                                                                   | DevHQ las relaciona, no las sustituye                                                                     |

### Cómo crecer sin romperlo

Cada tabla que cuelga de un sujeto (`observations`, `incidents`, `events`, `http_checks` y, con la
puerta, `agent_log`) lo señala con dos columnas, `application_id` y `resource_id`, y un CHECK que
exige exactamente una. Un tercer tipo de sujeto añadiría una columna a cada una y obligaría a
reconstruirlas, porque SQLite no cambia un CHECK con ALTER, como hicieron la v4 y la v7. Por eso lo
nuevo que se observa entra, mientras quepa, como **tipo de recurso** (una base de datos compartida,
un dominio…): hereda sin cambios las observaciones, las incidencias, la comprobación HTTP(S),
Infrastructure y la atención. Una tabla genérica de sujetos solo se plantea cuando algo no quepa ni
como aplicación ni como recurso, y entonces con una migración que conserve cada fila, como exigen
las reglas del esquema.

Las relaciones crecen igual: un tipo de relación nuevo (un recurso que corre en otro, una aplicación
que usa otra) llega cuando una pregunta concreta lo necesita, con sus claves foráneas y su
`RESTRICT` o `CASCADE` explícitos; no un motor genérico de grafos. Lo que nombra una tecnología en
vez de un concepto, como el tipo de componente `docker`, se generaliza cuando se toque esa parte del
modelo, no antes.

### El inventario envejece

Es el mayor riesgo a 50 proyectos: un mapa rico solo sirve si sigue siendo cierto, y uno desfasado
es peor que ninguno, porque parece fiable. Nadie va a teclear a mano cada relación de 50 proyectos
ni a mantenerla al día. Por eso:

- Se prefiere lo que se puede observar a lo que hay que teclear.
- Un agente puede hacer el tecleo cuando se lo pides, por la puerta, y sus cambios quedan en
  Activity.
- Si algún día DevHQ descubre cosas (repositorios en una carpeta, servicios en un servidor), propone
  y tú aceptas; nunca cambia el inventario en silencio.
- Cada cambio significativo, tuyo o de un agente, queda en el registro de cambios común (ver «Estado
  actual e historia»), así que el mapa puede decir cuándo y quién tocó cada cosa por última vez.

## Preguntas que debe poder responder

| Pregunta                                                 | Qué hace falta                                                                           | Hoy                                                                                                                       |
| -------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| ¿Qué necesita mi atención?                               | Evidencia vigente y las reglas de atención                                               | Sí: Overview y, con la puerta, `attention`                                                                                |
| ¿Qué está fallando?                                      | Observaciones vigentes                                                                   | Sí, en lo que tiene comprobación u observaciones; lo nunca observado se cuenta como hueco, no como sano                   |
| ¿Qué aplicaciones corren en este servidor?               | Componente → recurso                                                                     | Sí: Infrastructure y el detalle de cada recurso                                                                           |
| ¿Qué se cae si cae este servidor?                        | Relaciones entre recursos y entre aplicaciones                                           | En parte: solo lo que corre directamente en él                                                                            |
| ¿Qué aplicaciones usan esta base de datos?               | Bases de datos compartidas como recurso                                                  | No: un recurso solo es servidor o estación de trabajo, y una base de datos es componente de una sola aplicación           |
| ¿Qué está desplegado, dónde y con qué versión?           | Despliegues con versión o commit y destino                                               | En parte: lifecycle, la versión como texto y eventos de despliegue a mano, sin destino ni commit                          |
| ¿Qué tiene cambios sin desplegar?                        | El repositorio de cada aplicación, el commit del último despliegue y leer el repositorio | No                                                                                                                        |
| ¿Qué lleva meses sin actividad?                          | Actividad observada: commits, despliegues                                                | No: solo existe la actividad que se registra en DevHQ                                                                     |
| ¿Dónde están sus logs, su documentación, su repositorio? | Referencias                                                                              | No: solo la descripción libre                                                                                             |
| ¿Qué dominio o certificado caduca pronto?                | Dominios como recurso y la caducidad del certificado como evidencia                      | No                                                                                                                        |
| ¿Cómo llegó esto a su estado actual?                     | La evidencia y un registro de cambios común                                              | En parte: la evidencia y el historial de cada incidencia; de la configuración, solo los cambios de agentes, con la puerta |
| ¿Qué debe saber un agente que entra a este proyecto?     | Un resumen por sistema con referencias, relaciones y estado                              | En parte: `get_system` de la puerta                                                                                       |

## Líneas de evolución posibles

Sin fechas ni compromiso, ordenadas por lo que cada una necesita de las anteriores. Los pasos
concretos y su orden están en «Evolución prevista» del README; este documento no los reordena.

1. **Registro de cambios común**: lo que cambias tú en la ventana deja rastro igual que lo que
   cambia un agente, en un único registro con el actor, el valor anterior y el posterior. Sin
   credenciales ni operaciones; es la base de la trazabilidad que pedirán las capacidades
   operativas.
2. **Referencias**: dónde está cada cosa (repositorio, ruta local en cada estación de trabajo,
   documentación, logs, panel del proveedor). Barato y sin credenciales; hace útiles los resúmenes
   para agentes y comodidades como abrir la carpeta.
3. **Más tipos de recurso y de relación**: bases de datos compartidas y dominios como recursos, un
   recurso que corre en otro, una aplicación que usa otra. Responde qué se cae si cae algo.
4. **Despliegues con estructura**: el evento de despliegue con versión o commit, entorno y recurso
   de destino, registrado por ti, por un agente o, más adelante, por la CI. Responde qué está
   desplegado y dónde.
5. **Actividad local**: leer, cuando lo pides, los repositorios locales que el inventario ya conoce
   (último commit, cambios sin confirmar, commits posteriores al último despliegue) como evidencia
   con su propio `source`. Sin credenciales, pero choca con «no se implementan scanners».
6. **Contexto para agentes**: el resumen por sistema en la puerta, más rico con cada paso anterior.
7. **Fuentes con credenciales**: servidores por SSH de solo lectura, el estado de la CI en GitHub u
   otra plataforma. Exige el almacén nativo de credenciales.
8. **Capacidades operativas**: las automatizaciones con resultado e historial que el README ya
   anticipa y acciones sobre servidores y despliegues, lanzadas por ti o por un agente autorizado,
   cada una explícita, segura, controlable y auditable. Exige cambiar «no opera sistemas».
9. **Supervisión con la ventana cerrada**: exige cambiar «no funciona en segundo plano» y cuidar lo
   que consume.

## Tensiones con las reglas actuales

Cada regla sigue vigente hasta que se decida cambiarla de forma explícita, y entonces el README
cambia en el mismo commit que la funcionalidad que lo necesita.

| Idea                                                  | Regla o límite de hoy                                                               | Qué haría falta                                                                                                                  |
| ----------------------------------------------------- | ----------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| Actuar sobre el ecosistema, tú o un agente autorizado | DevHQ observa; no opera sistemas                                                    | Cambiar la regla con la primera capacidad operativa, construida sobre la observación: explícita, segura, controlable y auditable |
| Leer servidores o plataformas con credenciales        | No guarda credenciales                                                              | El almacén nativo de credenciales que el README ya prevé; nunca en SQLite, React ni localStorage                                 |
| Vigilar con la ventana cerrada                        | No funciona en segundo plano                                                        | Revisar la regla y medir lo que consume; lo hace DevHQ, no un agente                                                             |
| Descubrir repositorios o servicios                    | No se implementan scanners                                                          | Que lo lances tú, de solo lectura, y que proponga cambios que aceptas                                                            |
| Atajos como abrir una carpeta                         | Si solo ahorra un comando sencillo, no pertenece                                    | Solo como subproducto de una referencia que el modelo ya tiene                                                                   |
| Muchos tipos de cosas                                 | Dos tipos de sujeto, con una columna cada uno en cada tabla que cuelga de un sujeto | Crecer por tipos de recurso; una tabla genérica de sujetos solo si algo no cabe, con una migración que conserve cada fila        |

## Lo que DevHQ no quiere ser

- Un sustituto de GitHub, Docker, la terminal, el editor o un panel de monitorización: los observa,
  los relaciona y sabe dónde están, y si algún día actúa, actúa a través de ellos.
- Un almacén de métricas o de logs: guarda veredictos con su evidencia, no series ni flujos.
- Un gestor de secretos o un cliente SSH: si alguna vez necesita credenciales, viven en el almacén
  del sistema.
- Un producto de IA: funciona entero sin agentes, y los agentes son intercambiables.
- Una base de datos de configuración que hay que teclear entera: lo que nadie mantiene, se desfasa.
- Un servicio en la nube, multiusuario o con cuentas.
- Una colección de botones y lanzadores.

## Cómo usar este documento

Ante una funcionalidad o un cambio de arquitectura:

1. ¿Qué pregunta responde, y la sigue respondiendo con 50 aplicaciones, varios servidores y muchos
   recursos asociados?
2. ¿Es inventario, evidencia o estado derivado, y en qué fila de «Dónde encaja cada cosa» cae? Si no
   cae en ninguna, se revisa aquí primero.
3. ¿Quién mantiene el dato con 50 proyectos: se observa, lo reporta un agente o hay que teclearlo?
4. ¿Seguiría teniendo sentido si cambiaras de agente, de plataforma o de tecnología? ¿Deja rastro de
   qué se hizo, cuándo, sobre qué, quién y con qué resultado?
5. ¿Qué principio o regla actual toca? Si toca una regla, se decide de forma explícita y el README
   cambia en el mismo commit.
6. ¿Es lo más pequeño que responde la pregunta?

Este documento se corrige cuando cambia la dirección, con una línea en «Historial».

## Historial

- 6 oct 2026: primera versión, a partir del planteamiento original y de su revisión contra `main`
  (esquema v6) y la rama de la puerta para agentes (esquema v9).
- 6 oct 2026: «DevHQ observa, no opera» pasa a leerse como regla del estado actual y no como límite
  de la visión. La observabilidad es la base; las capacidades operativas, tuyas o de agentes
  autorizados, quedan abiertas a largo plazo si se construyen sobre ella y son explícitas, seguras,
  controlables y auditables.
- 6 oct 2026: la frase de rumbo y los principios agrupados: representación viva, conceptos y no
  proveedores, integrar y no reemplazar, lo que el repositorio no sabe, escala, lo observable antes
  que lo tecleado, trazabilidad y la IA como capa.
- 6 oct 2026: la trazabilidad pasa a ser una propiedad del sistema, igual para ti que para los
  agentes, y se distingue el estado actual de la historia de cómo se llegó a él, con un único
  registro de cambios común en vez de dos mecanismos.

## Planteamiento original

El texto con el que se propuso esta visión, el 6 oct 2026, dirigido a la sesión de Claude Code que
escribió este documento. Se conserva sin cambios porque es la fuente de la intención; lo de arriba
es su revisión.

> Quiero dejar registrada una dirección de largo plazo para DevHQ, no como una lista de tareas que
> haya que implementar ahora mismo, sino como una visión de producto que debería influir poco a poco
> en la arquitectura y en las decisiones futuras del proyecto. Tú tienes mucho más contexto que yo
> sobre cómo funciona actualmente DevHQ, qué está implementado, qué limitaciones existen y qué cosas
> serían razonables o no dentro de la arquitectura actual, así que no quiero que asumas que todo lo
> siguiente debe construirse tal cual ni de inmediato. Tómalo como intención, principios y posibles
> líneas de evolución.
>
> La idea central es que DevHQ termine convirtiéndose en el centro de control de todo mi ecosistema
> de desarrollo y, con el tiempo, de buena parte de mi entorno digital relacionado con proyectos.
> Hoy pueden ser pocos proyectos, servicios y máquinas, pero quiero diseñarlo pensando en un futuro
> en el que pueda tener decenas de repositorios, aplicaciones, bases de datos, contenedores,
> servidores, despliegues, dominios, APIs, documentación, agentes de IA y herramientas distintas.
> DevHQ no debería intentar reemplazar todas esas herramientas ni almacenar absolutamente todo
> dentro de sí mismo. Su valor debería estar en saber qué existe, dónde está, cómo se relaciona, qué
> estado tiene y cómo interactuar con ello desde un único lugar.
>
> Me gustaría pensar DevHQ como una especie de “control plane” personal. Cada proyecto debería poder
> convertirse progresivamente en una entidad mucho más rica que simplemente una carpeta o un
> repositorio: podría conocer su repositorio, ubicación local, tecnologías, servicios asociados,
> entornos, bases de datos, despliegues, servidores, documentación, estado de salud, últimas
> actividades y otras relaciones relevantes. La intención es que, si dentro de unos años existen 50
> proyectos, DevHQ permita mantener una representación viva de todo el ecosistema sin perder el
> rastro de qué depende de qué, qué sigue activo, qué está desplegado, qué lleva meses abandonado o
> qué necesita atención.
>
> Un principio importante para valorar futuras funcionalidades podría ser preguntarnos: “¿Esto sigue
> siendo realmente útil si tengo 50 proyectos?”. Funciones pequeñas como abrir una carpeta pueden
> seguir siendo convenientes, pero el verdadero valor a largo plazo estaría en poder responder
> preguntas más amplias: qué proyectos están desplegados, cuáles llevan meses sin actividad, qué
> servicios dependen de cierto servidor, qué aplicaciones utilizan determinada base de datos, qué
> repositorios tienen cambios sin desplegar, qué servicios están fallando, dónde están los logs
> relevantes, qué componentes dependen entre sí, etc.
>
> El VPS también debería poder integrarse en esta visión. No necesariamente ahora ni de una forma
> concreta, pero me gustaría que DevHQ eventualmente pudiera representar servidores como parte
> natural del ecosistema: conocer qué servicios están corriendo, contenedores, uso de CPU/RAM/disco,
> estado de Nginx u otros servicios, despliegues asociados, health checks, logs y operaciones
> básicas. La intención no es reinventar SSH, Docker, systemd, GitHub u otras herramientas, sino que
> DevHQ actúe como una capa de control sobre ellas. Por ejemplo, un proyecto podría estar
> relacionado con un repositorio de GitHub, un workspace local, un contenedor Docker, una base de
> datos y un deployment en un VPS, y DevHQ sería quien entienda y muestre esas relaciones.
>
> La capa de IA también me interesa mucho, especialmente con el trabajo actual para crear una
> “puerta” mediante la cual Codex, Claude u otros agentes puedan acceder a DevHQ con el mismo
> contexto y datos que yo. Pero no quiero que DevHQ dependa estructuralmente de una IA concreta.
> Preferiría que primero exista una capa sólida y estructurada de información sobre mis proyectos,
> servicios e infraestructura, y que encima de ella puedan conectarse distintos agentes. Así, un
> agente podría recibir contexto real de DevHQ y entender algo como: “este proyecto está en este
> repositorio, actualmente corre en este servidor, utiliza estos servicios, este deployment
> corresponde a este commit y estos son sus últimos logs”. La IA debería ser una capa intercambiable
> que aprovecha el conocimiento de DevHQ, no el fundamento del sistema.
>
> A largo plazo incluso me gustaría que DevHQ ayudara a preparar contexto para los propios agentes:
> documentación del proyecto, resúmenes de arquitectura, información sobre dependencias,
> convenciones, instrucciones para agentes, estado actual del sistema, etc. Sería interesante que
> pudiera facilitar que Codex, Claude u otro agente entren a un proyecto sin empezar prácticamente
> desde cero cada vez.
>
> También quiero evitar que DevHQ se convierta únicamente en una colección de botones o accesos
> directos. Las acciones son importantes, pero el corazón del proyecto debería ser el modelo de
> información y las relaciones entre las cosas. Si el día de mañana cambio Claude por otro agente,
> GitHub por otra plataforma, Docker por otra tecnología o una base de datos por otra, DevHQ debería
> seguir teniendo sentido porque está construido alrededor de mis proyectos y mi infraestructura, no
> alrededor de proveedores concretos.
>
> En resumen, la dirección que quiero explorar es que DevHQ evolucione gradualmente desde una
> herramienta para gestionar proyectos hacia una capa de control, contexto y memoria de todo mi
> ecosistema de desarrollo. No hace falta implementar todo esto ahora. De hecho, probablemente
> muchas ideas cambien conforme avance el proyecto. Lo que sí me gustaría es que esta visión quede
> registrada en el README o, preferiblemente, en algún documento de ideas/roadmap/vision a futuro
> para que podamos consultarla cuando haya que tomar decisiones arquitectónicas y valorar nuevas
> funcionalidades.
>
> Con el contexto que ya tienes del proyecto actual, revisa estas ideas críticamente: conserva lo
> que tenga sentido, adapta lo que choque con la arquitectura actual y evita introducir complejidad
> prematura. Si consideras que esta visión merece un archivo específico dentro del repositorio, crea
> o actualiza el documento que mejor encaje, pero trátalo principalmente como una guía de dirección
> futura y no como un backlog inmediato.
