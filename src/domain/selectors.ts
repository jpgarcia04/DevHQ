import type { ActivityEvent, Application, HealthStatus, Incident, Resource } from './models'

export const needsAttention = (health: HealthStatus) =>
  health === 'degraded' || health === 'unhealthy'
export const activeIncidents = (items: Incident[]) =>
  items.filter((item) => item.status !== 'resolved')
export const applicationIncidents = (items: Incident[], app: Application) =>
  items.filter((item) =>
    item.subject.type === 'application'
      ? item.subject.id === app.id
      : app.resourceIds.includes(item.subject.id),
  )
export const applicationEvents = (items: ActivityEvent[], app: Application) =>
  items.filter((item) =>
    item.subject.type === 'application'
      ? item.subject.id === app.id
      : app.resourceIds.includes(item.subject.id),
  )
export const resourceIncidents = (items: Incident[], resourceId: Resource['id']) =>
  items.filter((item) => item.subject.type === 'resource' && item.subject.id === resourceId)
export const resourceApplications = (items: Application[], resourceId: Resource['id']) =>
  items.filter((app) => app.resourceIds.includes(resourceId))

export function ecosystemHealth(apps: Application[], resources: Resource[]): HealthStatus {
  const statuses = [...apps, ...resources].map((item) => item.health)
  if (statuses.includes('unhealthy')) return 'unhealthy'
  if (statuses.includes('degraded')) return 'degraded'
  if (statuses.length === 0 || statuses.includes('unknown')) return 'unknown'
  return 'healthy'
}
