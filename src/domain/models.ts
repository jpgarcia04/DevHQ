export type HealthStatus = 'healthy' | 'degraded' | 'unhealthy' | 'unknown'
export type Lifecycle = 'development' | 'testing' | 'production' | 'archived'
export type IncidentSeverity = 'info' | 'warning' | 'critical'
export type IncidentStatus = 'open' | 'investigating' | 'resolved'
export type ComponentKind = 'backend' | 'database' | 'docker' | 'frontend'

export interface ApplicationComponent {
  id: string
  name: string
  kind: ComponentKind
  health: HealthStatus
  resourceId?: string
}

export interface Application {
  id: string
  name: string
  description: string
  mark: 'shield' | 'brain' | 'home' | 'clapperboard'
  artwork: 'guardian' | 'brain' | 'rentia' | 'clipops'
  logoUrl?: string
  thumbnailUrl?: string
  lifecycle: Lifecycle
  version?: string
  health: HealthStatus
  healthSummary: string
  components: ApplicationComponent[]
  resourceIds: string[]
  lastVerifiedDeploymentAt?: string
  lastBackupAt?: string
}

export interface Resource {
  id: string
  name: string
  kind: 'server' | 'workstation'
  description: string
  health: HealthStatus
  operatingSystem: string
  services: string[]
  healthSummary: string
}

export type EntityRef = { type: 'application'; id: string } | { type: 'resource'; id: string }

export interface Incident {
  id: string
  title: string
  description: string
  severity: IncidentSeverity
  status: IncidentStatus
  subject: EntityRef
  occurredAt: string
}

export interface ActivityEvent {
  id: string
  title: string
  description: string
  kind: 'backup' | 'build' | 'health' | 'deployment' | 'security' | 'automation'
  outcome: 'success' | 'warning' | 'failure'
  subject: EntityRef
  occurredAt: string
}

// Records kept by DevHQ for the real inventory. A person types manual entries; an HTTP
// check DevHQ runs produces automated observations; an agent the person asks to act through
// the agent gate reports records of its own. Only observations and their events are automated.
export type RecordSource = 'manual' | 'http' | 'agent'
export type ObservationResult = Exclude<HealthStatus, 'unknown'>
// `change` is a change to the configuration, by the person or by an agent through the agent gate,
// shown in timelines among the events (see `timelineEvents`); the native process never stores an
// event of that kind.
export type RecordedEventKind =
  'observation' | 'incident' | 'deployment' | 'backup' | 'maintenance' | 'note' | 'change'
export type ManualEventKind = Exclude<RecordedEventKind, 'observation' | 'incident' | 'change'>
export type EventOutcome = 'success' | 'warning' | 'failure' | 'info'

export interface Observation {
  id: string
  subject: EntityRef
  result: ObservationResult
  summary: string
  source: RecordSource
  observedAt: string
  validUntil: string
  recordedAt: string
  /** Technical evidence of an HTTP check; null for every other source. */
  http: HttpEvidence | null
  /** The agent that reported it, as its client named itself; null unless the source is agent. */
  agent: string | null
}

/** A redirect the check followed: the status that sent it and where it pointed. */
export interface HttpRedirect {
  status: number
  location: string
}

/**
 * What an HTTP check requested and what came back. `status` is the last status received
 * (null when no response arrived); `error` explains a failure of the checked system;
 * `latencyMs` is the time until the answer or the failure.
 */
export interface HttpEvidence {
  url: string
  expectedStatus: number
  status: number | null
  latencyMs: number
  error: string | null
  redirects: HttpRedirect[]
}

export interface RecordedIncident {
  id: string
  subject: EntityRef
  title: string
  description: string
  severity: IncidentSeverity
  status: IncidentStatus
  /** Who opened it; each later change of status is an event with its own source. */
  source: Exclude<RecordSource, 'http'>
  openedAt: string
  updatedAt: string
  resolvedAt: string | null
  agent: string | null
}

export interface RecordedEvent {
  id: string
  subject: EntityRef
  kind: RecordedEventKind
  outcome: EventOutcome
  title: string
  detail: string
  source: RecordSource
  observationId: string | null
  incidentId: string | null
  occurredAt: string
  recordedAt: string
  agent: string | null
  /** For a change: the system's name when it was made, which may since have gone. */
  subjectName?: string
}

export interface Automation {
  id: string
  name: string
  description: string
  applicationId: string
  trigger: string
  condition: string
  actions: string[]
  verification: string
  onFailure: string
  status: 'enabled' | 'paused'
  lastRun?: { occurredAt: string; outcome: 'success' | 'failure' }
  nextRunAt?: string
}
