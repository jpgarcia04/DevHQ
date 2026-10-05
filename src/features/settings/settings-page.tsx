import { PageHeader } from '@/components/page-header'
import { scenario } from '@/data/mock/scenario'
import { formatDate, formatTime } from '@/shared/time'

export function SettingsPage() {
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="A deliberate starting point"
        title="Settings"
        description="Workspace information and the boundaries of this foundation."
      />
      <section className="panel max-w-4xl p-6">
        <h2 className="section-heading mb-6">Workspace</h2>
        <dl className="divide-y">
          {[
            ['Name', scenario.name],
            ['Data source', 'Local mock scenario'],
            ['Appearance', 'Dark · desktop foundation'],
            ['Time zone', scenario.timeZone],
            [
              'Reference snapshot',
              `${formatDate(scenario.observedAt, scenario.timeZone)} · ${formatTime(scenario.observedAt, scenario.timeZone)} CEST`,
            ],
            ['Persistence', 'None: a fixed example. My inventory, in the desktop app, uses SQLite'],
            ['Connections', 'No external integrations'],
          ].map(([label, value]) => (
            <div
              key={label}
              className="flex items-start justify-between gap-8 py-4 first:pt-0 last:pb-0"
            >
              <dt className="text-xs text-muted-foreground">{label}</dt>
              <dd className="text-right text-xs">{value}</dd>
            </div>
          ))}
        </dl>
      </section>
      <section className="max-w-3xl">
        <h2 className="section-heading">About DevHQ</h2>
        <p className="mt-3 text-[13px] leading-7 text-muted-foreground">
          DevHQ is the operations home for a personal software ecosystem. It is designed to observe,
          remember, connect systems and bring attention to what matters.
        </p>
        <p className="mt-3 text-xs leading-6 text-muted-foreground">
          This preview validates navigation, relationships and operational context. Preferences,
          credential storage and integrations will be introduced when they support real workflows.
        </p>
        <p className="mt-5 font-mono text-[10px] text-muted-foreground">DEVHQ / FOUNDATION 0.1.0</p>
      </section>
    </div>
  )
}
