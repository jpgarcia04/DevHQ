import type { HealthStatus, IncidentSeverity, IncidentStatus, Lifecycle } from './models'

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
