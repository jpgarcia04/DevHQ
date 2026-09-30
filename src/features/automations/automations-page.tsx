import { Check, ChevronRight, Clock3, GitBranch, ShieldCheck, Workflow } from 'lucide-react'
import { PageHeader } from '@/components/page-header'
import { EntityLink } from '@/components/entity-link'
import { automations, scenario } from '@/data/mock/scenario'
import { relativeTime } from '@/shared/time'

export function AutomationsPage() {
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Intent, execution, evidence"
        title="Automations"
        description="Complete workflows with a verified outcome. More than a shortcut to a command."
      />
      <div className="flex items-start gap-3 rounded-lg border border-primary/20 bg-primary/5 px-4 py-3.5">
        <Workflow className="mt-0.5 size-4 shrink-0 text-primary" aria-hidden="true" />
        <p className="text-xs leading-5 text-muted-foreground">
          <span className="font-medium text-primary">Workflow previews.</span> These definitions and
          run results are examples. No scheduler or execution engine is active.
        </p>
      </div>
      <div className="space-y-6">
        {automations.map((automation) => {
          const stages = [
            { label: 'Trigger', icon: Clock3, lines: [automation.trigger] },
            { label: 'Condition', icon: GitBranch, lines: [automation.condition] },
            { label: 'Action', icon: Workflow, lines: automation.actions },
            { label: 'Verification', icon: ShieldCheck, lines: [automation.verification] },
          ]
          return (
            <article key={automation.id} className="panel overflow-hidden">
              <header className="flex flex-wrap items-start justify-between gap-4 p-6">
                <div>
                  <h2 className="text-base font-semibold">{automation.name}</h2>
                  <p className="mt-1.5 text-xs text-muted-foreground">{automation.description}</p>
                  <div className="mt-3">
                    <EntityLink
                      subject={{ type: 'application', id: automation.applicationId }}
                      className="text-xs"
                    />
                  </div>
                </div>
                <span className="rounded border px-2 py-1 text-[10px] text-muted-foreground">
                  {automation.status === 'enabled' ? 'Enabled in sample' : 'Paused in sample'}
                </span>
              </header>
              <ol className="grid gap-3 px-6 pb-6 min-[1100px]:grid-cols-4">
                {stages.map((stage, index) => (
                  <li key={stage.label} className="relative rounded-lg border bg-background/50 p-4">
                    <p className="eyebrow flex items-center gap-2">
                      <stage.icon className="size-3.5" aria-hidden="true" />
                      {stage.label}
                    </p>
                    <div className="mt-3 space-y-2">
                      {stage.lines.map((line) => (
                        <p key={line} className="text-xs leading-5">
                          {line}
                        </p>
                      ))}
                    </div>
                    {index < 3 && (
                      <ChevronRight
                        className="absolute -right-2.5 top-1/2 z-10 hidden size-4 bg-card text-muted-foreground min-[1100px]:block"
                        aria-hidden="true"
                      />
                    )}
                  </li>
                ))}
              </ol>
              <div className="flex flex-wrap items-center justify-between gap-3 border-t bg-black/10 px-6 py-4">
                <p className="text-xs text-muted-foreground">
                  <span className="text-foreground">On failure</span>
                  <span className="mx-2">→</span>
                  {automation.onFailure}
                </p>
                <p
                  className={`flex items-center gap-1.5 text-[11px] ${automation.lastRun?.outcome === 'success' ? 'text-emerald-300' : 'text-rose-300'}`}
                >
                  {automation.lastRun?.outcome === 'success' && (
                    <Check className="size-3" aria-hidden="true" />
                  )}
                  Last sample run: {automation.lastRun?.outcome ?? 'Not run'}
                  {automation.lastRun && (
                    <span className="ml-1 text-muted-foreground">
                      · {relativeTime(automation.lastRun.occurredAt, scenario.observedAt)}
                    </span>
                  )}
                </p>
              </div>
            </article>
          )
        })}
      </div>
    </div>
  )
}
