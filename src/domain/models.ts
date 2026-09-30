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
