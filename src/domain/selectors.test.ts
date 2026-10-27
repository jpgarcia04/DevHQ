import { describe, expect, it } from 'vitest'
import {
  applications,
  automations,
  events,
  incidents,
  resources,
  scenario,
} from '@/data/mock/scenario'
import {
  activeIncidents,
  applicationEvents,
  applicationIncidents,
  ecosystemHealth,
  resourceApplications,
  resourceIncidents,
} from './selectors'

describe('operational relationships', () => {
  it('includes shared infrastructure incidents in an application context', () => {
    const app = applications.find((item) => item.id === 'canvas-media')!
    expect(applicationIncidents(incidents, app).map((item) => item.id)).toEqual([
      'INC-024',
      'INC-023',
    ])
    expect(resourceApplications(applications, 'orbit-vps').map((item) => item.id)).toEqual([
      'orbit-planner',
      'canvas-media',
    ])
    expect(resourceIncidents(incidents, 'orbit-vps').map((item) => item.id)).toEqual(['INC-023'])
    expect(applicationEvents(events, app).map((item) => item.id)).toContain('evt-2')
    expect(applicationEvents(events, app).map((item) => item.id)).not.toContain('evt-7')
  })
  it('does not count resolved incidents as active', () => {
    expect(
      activeIncidents(incidents.map((item) => ({ ...item, status: 'resolved' }))),
    ).toHaveLength(0)
  })
  it('does not report unknown or empty coverage as healthy', () => {
    expect(ecosystemHealth([], [])).toBe('unknown')
    expect(
      ecosystemHealth(
        applications.filter((item) => item.id === 'rivet-rooms'),
        [],
      ),
    ).toBe('unknown')
    expect(ecosystemHealth(applications, resources)).toBe('degraded')
    expect(ecosystemHealth([], [{ ...resources[0]!, health: 'unhealthy' }])).toBe('unhealthy')
  })
  it('keeps all mock references resolvable', () => {
    for (const app of applications) {
      for (const id of app.resourceIds)
        expect(resources.some((resource) => resource.id === id)).toBe(true)
      for (const component of app.components)
        if (component.resourceId) expect(app.resourceIds).toContain(component.resourceId)
    }
    for (const item of [...incidents, ...events])
      expect(
        (item.subject.type === 'application' ? applications : resources).some(
          (entity) => entity.id === item.subject.id,
        ),
      ).toBe(true)
    for (const automation of automations)
      expect(applications.some((app) => app.id === automation.applicationId)).toBe(true)
  })
  it('keeps sample events and automation results in chronological order', () => {
    const timestamps = events.map((event) => Date.parse(event.occurredAt))
    expect(timestamps).toEqual([...timestamps].sort((a, b) => b - a))
    expect(timestamps.every((timestamp) => timestamp <= Date.parse(scenario.observedAt))).toBe(true)
    const backup = applications.find((app) => app.id === 'pine-vault')!
    const backupRun = automations.find((item) => item.id === 'daily-backup')!
    expect(Date.parse(backupRun.lastRun!.occurredAt)).toBeGreaterThan(
      Date.parse(backup.lastBackupAt!),
    )
    expect(Date.parse(backupRun.nextRunAt!)).toBeGreaterThan(Date.parse(scenario.observedAt))
    expect(backupRun.lastRun!.occurredAt).toBe(
      events.find((event) => event.id === 'evt-1')!.occurredAt,
    )
    const canvasMedia = applications.find((app) => app.id === 'canvas-media')!
    const failedRelease = events.find((event) => event.id === 'evt-3')!
    expect(Date.parse(canvasMedia.lastVerifiedDeploymentAt!)).toBeLessThan(
      Date.parse(failedRelease.occurredAt),
    )
    expect(events.find((event) => event.id === 'evt-5')!.kind).toBe('build')
  })
})
