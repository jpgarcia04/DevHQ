import { sameSubject } from '@/domain/evidence'
import type { EntityRef } from '@/domain/models'
import { formatTime, timeUntil } from '@/shared/time'
import type { MonitorCheckStatus, MonitorStatus } from './inventory-api'

export const monitorStatusOf = (monitor: MonitorStatus | null, subject: EntityRef) =>
  monitor?.checks.find((item) => sameSubject(item.subject, subject)) ?? null

/** When a supervised check runs next, or that it runs now. */
export function nextRunText(status: MonitorCheckStatus | null, now: number) {
  if (status?.running) return 'Running now'
  if (!status?.nextRunAt) return null
  return `Next run ${timeUntil(status.nextRunAt, now)} (${formatTime(status.nextRunAt)})`
}
