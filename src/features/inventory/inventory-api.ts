import { Channel, invoke } from '@tauri-apps/api/core'
import type {
  ComponentKind,
  EntityRef,
  EventOutcome,
  IncidentSeverity,
  IncidentStatus,
  Lifecycle,
  ManualEventKind,
  Observation,
  ObservationResult,
  RecordedEvent,
  RecordedIncident,
  RecordSource,
  ReferenceKind,
} from '@/domain/models'

/**
 * Where a system is, as the person or an agent declared it: inventory, not evidence, so it has no
 * source; the change log is its trace. DevHQ never opens or checks it.
 */
export interface InventoryReference {
  id: string
  kind: ReferenceKind
  /** An http(s) URL or an absolute path, as typed. */
  target: string
  /** A name for people that tells two references of one kind apart; never a locator. */
  label: string | null
  /** The resource a path is on; null for a URL. */
  machineId: string | null
}

export interface InventoryResource {
  id: string
  name: string
  kind: 'server' | 'workstation'
  description: string
  operatingSystem: string
  references: InventoryReference[]
}

export interface InventoryComponent {
  id: string
  name: string
  kind: ComponentKind
  resourceId: string | null
}

export interface InventoryApplication {
  id: string
  name: string
  description: string
  lifecycle: Lifecycle
  version: string | null
  components: InventoryComponent[]
  references: InventoryReference[]
}

/**
 * A repository URL more than one system declares, as the parts of a monorepo do: derived by the
 * native process at each read, by exact equality of the URL, and never stored.
 */
export interface SharedRepository {
  target: string
  /** Applications first, then resources, in the inventory's order. */
  systems: EntityRef[]
}

/** A remote of a local copy, by name; its URL only in a form DevHQ keeps without credentials. */
export interface CopyRemote {
  name: string
  url: string | null
}

/** What DevHQ found at a declared path: a copy it read, or why it could not. */
export type ReadingOutcome = 'read' | 'missing' | 'not_a_copy' | 'unreadable'

/** An operation a copy was left halfway through. */
export type CopyOperation =
  'merge' | 'rebase' | 'cherry_pick' | 'revert' | 'bisect' | 'apply_mailbox'

/**
 * What DevHQ read in a local copy, when someone asked: evidence of its own, never health, with no
 * validity. It says what the copy held at `observedAt`, which may no longer be so. It copies the
 * reference, machine and path it read, so it outlives them. Facts only with outcome `read`.
 */
export interface CopyReading {
  id: string
  subject: EntityRef
  referenceId: string
  machineId: string
  path: string
  source: 'read'
  /** The library and its version («libgit2 1.9.7»). */
  reader: string
  /** The agent that asked for it through the gate; null when the person asked. */
  agent: string | null
  observedAt: string
  recordedAt: string
  outcome: ReadingOutcome
  /** Why it was not read; only then. */
  detail: string | null
  /** The branch HEAD was on; null when detached. */
  branch: string | null
  /** HEAD's commit; null before the first commit. */
  head: string | null
  /** The newest commit date of HEAD and the local branches, as the commits say. */
  lastCommitAt: string | null
  /** Commits on no remote branch of the copy, on HEAD and on other local branches; 1001 is 1000+. */
  unpublishedOnHead: number | null
  unpublishedElsewhere: number | null
  stashes: number | null
  /** When the copy last talked to a remote, by its own files: DevHQ never fetches. */
  syncedAt: string | null
  remotes: CopyRemote[] | null
  operation: CopyOperation | null
  /** Tracked files changed, new entries, and files DevHQ could not verify… */
  changed: number | null
  untracked: number | null
  unverifiable: number | null
  /** …or why the working tree is unknown. */
  worktreeUnknown: string | null
}

/**
 * A local copy a system declares now, with its newest reading and what the native process derives
 * from it at each read of the inventory. Never stored.
 */
export interface LocalCopy {
  referenceId: string
  subject: EntityRef
  machineId: string
  path: string
  latest: CopyReading | null
  /** The newest reading was made at another path or machine than declared now. */
  movedSinceRead: boolean
  /** How its remotes compared with the repositories the system declares, normalized. */
  repository: 'matches' | 'differs' | 'undeclared' | null
  /** When read, it held commits on no remote, stashes, changes, new or unverifiable files, or an operation. */
  localWork: boolean
}

/** A system's readings, a page at a time, newest first. */
export interface ReadingPage {
  readings: CopyReading[]
  more: boolean
}

/**
 * Which workstation this computer is, from DevHQ's local data folder, never the inventory; or why
 * it is unknown.
 */
export interface ThisWorkstation {
  resourceId: string | null
  chosenAt: string | null
  problem:
    'not_chosen' | 'not_in_inventory' | 'not_a_workstation' | 'no_machine_id' | 'unreadable' | null
  message: string | null
  /** Where the association is kept, and where the system really keeps it when that differs. */
  file: string
  storedAt: string | null
}

/** A copy DevHQ could not read: a failure of DevHQ, kept nowhere. */
export interface ReadFailure {
  referenceId: string
  subject: EntityRef
  path: string
  message: string
}

export interface ReadOutcome {
  readings: CopyReading[]
  failures: ReadFailure[]
  thisWorkstation: ThisWorkstation
}

/**
 * `off`: runs only when asked. `active`: also every interval while DevHQ is open.
 * `paused`: keeps its interval but runs only when asked until resumed.
 */
export type MonitorState = 'off' | 'active' | 'paused'

/** The one HTTP check an application or resource can have. */
export interface HttpCheck {
  subject: EntityRef
  url: string
  expectedStatus: number
  timeoutSeconds: number
  validForMinutes: number
  updatedAt: string
  monitor: MonitorState
  monitorIntervalMinutes: number | null
}

/** What only the running DevHQ process knows about a check's supervision. */
export interface MonitorCheckStatus {
  subject: EntityRef
  running: boolean
  lastRunAt: string | null
  /** Only while supervision is active and no run is in flight. */
  nextRunAt: string | null
  /** The last run failed inside DevHQ and recorded nothing. */
  lastError: { message: string; at: string } | null
}

export interface MonitorStatus {
  checks: MonitorCheckStatus[]
  /** DevHQ could not read the checks' configuration. */
  error: string | null
  /** Observations recorded by runs since launch; grows when there is new evidence. */
  recorded: number
  /**
   * Commits by other connections the native process noticed since launch (it looks every 2 s):
   * an agent's through the agent gate, another program's, and the window's own writes. Grows
   * when the inventory may have changed.
   */
  changes: number
}

/**
 * Supervision can add an automated result every 5 minutes, so a snapshot carries only the
 * latest `limit` per application or resource; older ones stay in SQLite, untouched. Manual and
 * agent records are always complete.
 */
export interface AutomatedHistory {
  limit: number
  /**
   * Subjects with older automated results than the snapshot carries. From `since` on, their
   * automated observations and events are complete; before it, only manual and agent records
   * are loaded.
   */
  truncated: { subject: EntityRef; since: string }[]
}

/** Automated observations a snapshot left out, a page at a time, newest first. */
export interface ObservationPage {
  observations: Observation[]
  /** Older automated observations remain after this page. */
  more: boolean
}

/** Automated events a snapshot left out of a timeline, a page at a time, newest first. */
export interface EventPage {
  events: RecordedEvent[]
  /** Older automated events of these systems remain after this page. */
  more: boolean
}

/** One consistent read of SQLite. Records are newest first; times are UTC RFC 3339. */
export interface InventorySnapshot {
  applications: InventoryApplication[]
  resources: InventoryResource[]
  sharedRepositories: SharedRepository[]
  /** Each local copy the systems declare, with its newest reading; older ones are paged. */
  localCopies: LocalCopy[]
  observations: Observation[]
  incidents: RecordedIncident[]
  events: RecordedEvent[]
  httpChecks: HttpCheck[]
  /**
   * Every change to the configuration, by the person in the window or by an agent through the
   * agent gate, newest first: all of it.
   */
  changes: ConfigurationChange[]
  /**
   * From when the person's changes are in `changes`: DevHQ started logging them when it upgraded
   * an inventory that already held systems. Null when the log was always complete.
   */
  changesSince: string | null
  automatedHistory: AutomatedHistory
}

/** One field of an application, resource or HTTP check, before and after a change. */
export interface FieldChange {
  /** The field as the window knows it: `version`, `components`, `url`… */
  field: string
  /** Null where it had no value, or did not exist (before a creation, after a deletion). */
  before: unknown
  after: unknown
}

/**
 * A change to what carries no source of its own (applications, resources, components and HTTP
 * checks), by the person or by an agent through the agent gate. Written in the same transaction
 * as the change, never altered.
 */
export interface ConfigurationChange {
  id: string
  subject: EntityRef
  /** The system's name when the change was made; it may since have been renamed or deleted. */
  subjectName: string
  /** Who made it: the person, in the window, or an agent. */
  source: Exclude<RecordSource, 'http'>
  /** As the agent's client named itself (DevHQ cannot verify it); null for the person. */
  agent: string | null
  action:
    | 'created'
    | 'updated'
    | 'deleted'
    | 'check_saved'
    | 'check_paused'
    | 'check_resumed'
    | 'check_removed'
    | 'check_run'
  summary: string
  /** Field by field; null only for agents' changes logged before DevHQ kept the fields. */
  changes: FieldChange[] | null
  occurredAt: string
}

/** How far agents may act through the agent gate (`devhq --mcp`). Off until the person opens it. */
export type AgentAccessMode = 'off' | 'read' | 'write'

export interface AgentAccess {
  mode: AgentAccessMode
  /** When the person last changed it; null while it was never changed (and is off). */
  changedAt: string | null
  /** This executable, for the command that registers the gate; null if the system cannot say. */
  executable: string | null
}

/** Where the inventory lives on this device, so that it can be backed up. */
export interface DataLocation {
  /** The SQLite file. Once DevHQ is closed it holds everything on its own. */
  database: string
  /** The folder it is in, which also holds its write-ahead log while DevHQ has it open. */
  directory: string
  /**
   * Where the file system really keeps the file, when that is not `database`: through a link, or
   * because Windows redirects AppData for a DevHQ started from a packaged app (the Claude or
   * Codex desktop app, say). Other programs, File Explorer included, find it only there.
   */
  storedAt: string | null
}

/** Rows in a backup, by table. */
export interface BackupCounts {
  applications: number
  components: number
  resources: number
  observations: number
  incidents: number
  events: number
  httpChecks: number
}

/** A finished backup, as it was verified once written. */
export interface Backup {
  name: string
  /** Where the file is, to copy it somewhere safer. */
  path: string
  /** Where the file system really keeps it, when that is not `path` (see `DataLocation`). */
  storedAt: string | null
  /** When the copy was taken (UTC), as its name says. */
  takenAt: string
  sizeBytes: number
  /** What SQLite's quick_check said of the copy. Only an "ok" copy gets its final name. */
  quickCheck: string
  schemaVersion: number
  counts: BackupCounts
}

/**
 * A file in the backups folder, as found there. DevHQ vouches for none of them: a backup is
 * verified once, when it is made, and anything can happen to the file afterwards.
 */
export interface BackupFile {
  name: string
  sizeBytes: number
  /** When the file was last written, if the system says. */
  modifiedAt: string | null
  /** A copy that a backup was still writing, or that an interrupted one left behind. */
  partial: boolean
  /**
   * Where the file system really keeps it, when that is not in the listed folder. Asked file by
   * file: a folder Windows redirects for a packaged app can hold files kept in two places.
   */
  storedAt: string | null
}

export interface BackupFolder {
  directory: string
  /** Newest first. */
  files: BackupFile[]
}

/** How many rows a table holds. */
export interface TableRows {
  table: string
  rows: number
}

/**
 * What the inventory the window does not serve is, read without changing any of its files, and
 * whether DevHQ offers to upgrade it.
 */
export interface UpgradeInspection {
  database: string
  /** Where the file system really keeps it, when that is not `database`. */
  storedAt: string | null
  /** What was found (`outdated`, `current`, `needs_recovery`…): only `outdated` is upgraded. */
  state: string
  /** The schema version the file declares, when one could be read. */
  storedVersion: number | null
  /** This DevHQ's schema version, the one it upgrades to. */
  version: number
  message: string
  /** Back up and upgrade is offered: an outdated inventory DevHQ can vouch for, read whole. */
  upgradable: boolean
  /** Why DevHQ will not upgrade it; empty when it will. */
  problems: string[]
  tables: TableRows[]
  /** What the person consents to, sent back with the upgrade; only when `upgradable`. */
  fingerprint: string | null
  /** The folder the backup goes to. */
  backups: string
}

/** The verified copy an upgrade made before changing anything, with a manifest beside it. */
export interface BeforeUpgrade {
  name: string
  path: string
  storedAt: string | null
  manifest: string
  /** When the copy was taken (UTC), as its name says. */
  takenAt: string
  sizeBytes: number
  /** SHA-256 of the file's bytes, as the manifest records it. */
  sha256: string
  /** The inventory's schema version before the upgrade. */
  schemaVersion: number
  integrityCheck: string
  /** Each table with its rows, every one compared value by value with the inventory. */
  tables: TableRows[]
}

/** How an upgrade ended, once committed. */
export interface Upgraded {
  from: number
  to: number
  /** Null when the inventory was already in this DevHQ's schema and nothing was written. */
  backup: BeforeUpgrade | null
  /** Tables and rows carried over, each row checked value by value before the commit. */
  tablesKept: number
  rowsKept: number
}

/** Where an upgrade is: each step is sent once the native process reached it. */
export type UpgradeStep =
  | { step: 'checking' }
  | { step: 'backingUp' }
  | { step: 'backedUp'; backup: BeforeUpgrade }
  | { step: 'upgrading' }
  | { step: 'verifying' }

/** A reference to keep (with its ID) or to add. */
export type ReferenceInput = Omit<InventoryReference, 'id'> & { id?: string }
/** `references` is the complete list: the save replaces the stored one. */
export type ResourceInput = Omit<InventoryResource, 'id' | 'references'> & {
  id?: string
  references: ReferenceInput[]
}
/** `components` and `references` are complete lists: the save replaces the stored ones. */
export type ApplicationInput = Omit<InventoryApplication, 'id' | 'components' | 'references'> & {
  id?: string
  components: (Omit<InventoryComponent, 'id'> & { id?: string })[]
  references: ReferenceInput[]
}
export interface ObservationInput {
  subject: EntityRef
  result: ObservationResult
  summary: string
  observedAt: string
  validForMinutes: number
}
export interface IncidentInput {
  subject: EntityRef
  title: string
  description: string
  severity: IncidentSeverity
}
export interface IncidentStatusInput {
  id: string
  status: IncidentStatus
  note: string
}
/**
 * `monitorIntervalMinutes: null` turns supervision off. Turning it on starts it active;
 * editing a supervised check keeps it active or paused as it was.
 */
export type HttpCheckInput = Omit<HttpCheck, 'updatedAt' | 'monitor'>
export interface EventInput {
  subject: EntityRef
  kind: ManualEventKind
  outcome: EventOutcome
  title: string
  detail: string
  occurredAt: string
}

export const inventoryApi = {
  list: () => invoke<InventorySnapshot>('list_inventory'),
  dataLocation: () => invoke<DataLocation>('data_location'),
  /**
   * Copies the inventory into the backups folder next to it, verifies the copy and only then
   * names it. Writes nothing to the inventory. One at a time: a second request is refused.
   */
  backupInventory: () => invoke<Backup>('backup_inventory'),
  /** The files in the backups folder, as found there; it reads none of them. */
  listBackups: () => invoke<BackupFolder>('list_backups'),
  agentAccess: () => invoke<AgentAccess>('agent_access'),
  /** The gate reads it on every call, so the change applies at once. */
  setAgentAccess: (mode: AgentAccessMode) => invoke<AgentAccess>('set_agent_access', { mode }),
  saveResource: (input: ResourceInput) => invoke<InventoryResource>('save_resource', { input }),
  saveApplication: (input: ApplicationInput) =>
    invoke<InventoryApplication>('save_application', { input }),
  /** Refused while it has records: DevHQ never erases history. */
  deleteApplication: (id: string) => invoke<null>('delete_application', { id }),
  /** Refused while it has records, a component runs on it or it holds another system's paths. */
  deleteResource: (id: string) => invoke<null>('delete_resource', { id }),
  recordObservation: (input: ObservationInput) =>
    invoke<Observation>('record_observation', { input }),
  /** The subject's automated observations older than `before`, one of its own already shown. */
  olderObservations: (subject: EntityRef, before: string) =>
    invoke<ObservationPage>('older_observations', { subject, before }),
  /**
   * The automated events of every given subject older than `before`, one of their own already
   * shown, newest first across all of them: a merged timeline pages through them together.
   */
  olderEvents: (subjects: EntityRef[], before: string) =>
    invoke<EventPage>('older_events', { subjects, before }),
  openIncident: (input: IncidentInput) => invoke<RecordedIncident>('open_incident', { input }),
  updateIncidentStatus: (input: IncidentStatusInput) =>
    invoke<RecordedIncident>('update_incident_status', { input }),
  recordEvent: (input: EventInput) => invoke<RecordedEvent>('record_event', { input }),
  saveHttpCheck: (input: HttpCheckInput) => invoke<HttpCheck>('save_http_check', { input }),
  removeHttpCheck: (subject: EntityRef) => invoke<null>('remove_http_check', { subject }),
  setHttpCheckPaused: (subject: EntityRef, paused: boolean) =>
    invoke<HttpCheck>('set_http_check_paused', { subject, paused }),
  /**
   * Resolves with the recorded observation, including when the checked system failed. If a
   * run is already in flight, resolves with that run's observation instead of starting another.
   */
  runHttpCheck: (subject: EntityRef) => invoke<Observation>('run_http_check', { subject }),
  /** Receives the supervision status now and after every change, pushed by the native process. */
  watchHttpMonitor: async (onStatus: (status: MonitorStatus) => void) => {
    const channel = new Channel<MonitorStatus>(onStatus)
    await invoke<null>('watch_http_monitor', { channel })
  },
  /** Which workstation this computer is, or why DevHQ does not know. */
  thisWorkstation: () => invoke<ThisWorkstation>('this_workstation'),
  /** Chooses an existing workstation as this computer; null forgets the choice. */
  setThisWorkstation: (resourceId: string | null) =>
    invoke<ThisWorkstation>('set_this_workstation', { resourceId }),
  /**
   * Reads, now, the copies named or (null) every one of this workstation but those of archived
   * applications, each in a confined reader; resolves with what was kept and what DevHQ could not
   * read.
   */
  readCopies: (referenceIds: string[] | null) =>
    invoke<ReadOutcome>('read_copies', { referenceIds }),
  /** A system's readings older than `before`, one already shown, or its newest with null. */
  olderReadings: (subject: EntityRef, before: string | null) =>
    invoke<ReadingPage>('older_readings', { subject, before }),
  /** Looks at the inventory the window does not serve, without changing any of its files. */
  inspectUpgrade: () => invoke<UpgradeInspection>('inspect_upgrade'),
  /**
   * Backs up and upgrades the inventory if it is still exactly what the person was shown
   * (`from` and `fingerprint`); each step reaches `onStep` as the native process reaches it.
   */
  upgradeInventory: async (
    from: number,
    fingerprint: string,
    onStep: (step: UpgradeStep) => void,
  ) => {
    const progress = new Channel<UpgradeStep>(onStep)
    return invoke<Upgraded>('upgrade_inventory', { from, fingerprint, progress })
  },
}

/** What kind of refusal DevHQ answered (`schema_outdated`, `access_denied`…), if it said. */
export function inventoryErrorCode(error: unknown): string | null {
  if (error && typeof error === 'object' && 'code' in error) {
    const code = (error as { code: unknown }).code
    if (typeof code === 'string') return code
  }
  return null
}

export function inventoryError(error: unknown): string {
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message: unknown }).message
    if (typeof message === 'string') return message
  }
  if (typeof error === 'string') return error
  return 'The inventory request failed. Check the local database and try again.'
}
