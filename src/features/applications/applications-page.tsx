import { useState } from 'react'
import { Search, X } from 'lucide-react'
import { PageHeader } from '@/components/page-header'
import { EmptyState } from '@/components/empty-state'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import { applications } from '@/data/mock/scenario'
import { lifecycleLabels } from '@/domain/labels'
import { needsAttention } from '@/domain/selectors'
import { counted } from '@/shared/count'
import { ApplicationCard } from './application-card'

export function ApplicationsPage() {
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState('all')
  const filtered = applications.filter(
    (app) =>
      `${app.name} ${app.description}`.toLowerCase().includes(query.toLowerCase().trim()) &&
      (filter === 'all' ||
        (filter === 'attention' ? needsAttention(app.health) : app.lifecycle === filter)),
  )
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Your ecosystem"
        title="Applications"
        description="Every application, with its operational context in one place."
        action={
          <span className="text-xs text-muted-foreground">
            {counted(applications.length, 'application')}
          </span>
        }
      />
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="relative w-80 max-w-full">
          <Search
            className="absolute left-3 top-2.5 size-4 text-muted-foreground"
            aria-hidden="true"
          />
          <Input
            aria-label="Search applications"
            spellCheck={false}
            placeholder="Find an application…"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            className="h-9 bg-card pl-9 pr-9 text-xs"
          />
          {query && (
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label="Clear search"
              className="absolute right-1.5 top-1.5"
              onClick={() => setQuery('')}
            >
              <X />
            </Button>
          )}
        </div>
        <select
          aria-label="Filter applications"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
          className="filter-select"
        >
          <option value="all">All applications</option>
          <option value="attention">Needs attention</option>
          {Object.entries(lifecycleLabels).map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>
      </div>
      <p className="sr-only" aria-live="polite">
        {counted(filtered.length, 'application')} shown
      </p>
      {filtered.length ? (
        <div className="grid grid-cols-1 gap-5 min-[1060px]:grid-cols-2 min-[1700px]:grid-cols-3">
          {filtered.map((app) => (
            <ApplicationCard app={app} key={app.id} />
          ))}
        </div>
      ) : (
        <EmptyState
          title="No applications found"
          description="Try a different name or lifecycle."
          action={
            <Button
              variant="outline"
              onClick={() => {
                setQuery('')
                setFilter('all')
              }}
            >
              Clear filters
            </Button>
          }
        />
      )}
    </div>
  )
}
