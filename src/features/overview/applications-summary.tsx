import { ChevronRight } from 'lucide-react'
import { Link } from 'react-router-dom'
import { AppLogo } from '@/components/app-logo'
import { HealthBadge } from '@/components/status-badge'
import { applications } from '@/data/mock/scenario'
import { needsAttention } from '@/domain/selectors'
export function ApplicationsSummary() {
  return (
    <section aria-labelledby="applications-heading">
      <div className="mb-3 flex items-center justify-between">
        <h2 id="applications-heading" className="section-heading">
          Applications
        </h2>
        <Link to="/applications" className="quiet-link">
          View all <ChevronRight className="size-3" />
        </Link>
      </div>
      <div className="panel divide-y">
        {[...applications]
          .sort((a, b) => Number(needsAttention(b.health)) - Number(needsAttention(a.health)))
          .map((app) => (
            <Link
              key={app.id}
              to={`/applications/${app.id}`}
              className="flex items-center gap-3 px-4 py-3.5 transition-colors hover:bg-white/3"
            >
              <AppLogo app={app} />
              <div className="min-w-0 flex-1">
                <p className="text-[13px] font-medium">{app.name}</p>
                <p className="mt-1 truncate text-[11px] text-muted-foreground">
                  {app.health === 'degraded'
                    ? 'Deployment needs review'
                    : app.version
                      ? `Version ${app.version}`
                      : 'Development · no runtime observations'}
                </p>
              </div>
              <HealthBadge health={app.health} compact />
              <ChevronRight className="size-3.5 text-muted-foreground" aria-hidden="true" />
            </Link>
          ))}
      </div>
    </section>
  )
}
