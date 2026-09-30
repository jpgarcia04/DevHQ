import type {
  EventOutcome,
  HealthStatus,
  IncidentSeverity,
  IncidentStatus,
  Lifecycle,
  RecordSource,
  RecordedEventKind,
} from './models'

export const healthLabels: Record<HealthStatus, string> = {
  healthy: 'Healthy',
  degraded: 'Attention required',
  unhealthy: 'Unhealthy',
  unknown: 'Not observed',
}
export const lifecycleLabels: Record<Lifecycle, string> = {
  development: 'Development',
  testing: 'Testing',
  production: 'Production',
  archived: 'Archived',
}
export const severityLabels: Record<IncidentSeverity, string> = {
  info: 'Info',
  warning: 'Warning',
  critical: 'Critical',
}
export const incidentStatusLabels: Record<IncidentStatus, string> = {
  open: 'Open',
  investigating: 'Investigating',
  resolved: 'Resolved',
}
export const sourceLabels: Record<RecordSource, string> = {
  manual: 'Manual entry',
}
export const eventKindLabels: Record<RecordedEventKind, string> = {
  observation: 'Observation',
  incident: 'Incident',
  deployment: 'Deployment',
  backup: 'Backup',
  maintenance: 'Maintenance',
  note: 'Note',
}
export const eventOutcomeLabels: Record<EventOutcome, string> = {
  success: 'Success',
  warning: 'Warning',
  failure: 'Problem',
  info: 'Info',
}
