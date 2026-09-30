import { ChevronRight, Workflow } from 'lucide-react'
import { Link } from 'react-router-dom'
import { automations } from '@/data/mock/scenario'
export function AutomationsSummary() {
  return (
    <section aria-labelledby="automations-heading">
      <div className="mb-3 flex items-center justify-between">
        <h2 id="automations-heading" className="section-heading">
          Automations
        </h2>
        <Link className="quiet-link" to="/automations">
          View all <ChevronRight className="size-3" />
        </Link>
      </div>
      <div className="panel p-5">
        <div className="mb-5 flex items-center gap-3">
          <span className="rounded-lg bg-primary/10 p-2 text-primary">
            <Workflow className="size-4" />
          </span>
          <div>
            <p className="text-[13px] font-medium">
              {automations.filter((item) => item.status === 'enabled').length} defined workflows
            </p>
            <p className="mt-1 text-[11px] text-muted-foreground">Preview only · not executing</p>
          </div>
        </div>
        <div className="space-y-4">
          {automations.map((automation) => (
            <div key={automation.id} className="flex items-start justify-between gap-3">
              <div>
                <p className="text-xs">{automation.name}</p>
                <p className="mt-1 text-[10px] text-muted-foreground">{automation.trigger}</p>
              </div>
              <span
                className={
                  automation.lastRun?.outcome === 'failure'
                    ? 'text-[10px] text-rose-300'
                    : automation.lastRun
                      ? 'text-[10px] text-emerald-300'
                      : 'text-[10px] text-muted-foreground'
                }
              >
                {automation.lastRun?.outcome === 'failure'
                  ? 'Failed'
                  : automation.lastRun
                    ? 'Verified'
                    : 'Not run'}
              </span>
            </div>
          ))}
        </div>
        <p className="mt-5 border-t pt-3 text-[10px] text-muted-foreground">
          Last results from the example scenario
        </p>
      </div>
    </section>
  )
}
