import { ArrowLeft } from 'lucide-react'
import { Link, useParams, useSearchParams } from 'react-router-dom'
import { AppLogo } from '@/components/app-logo'
import { EmptyState } from '@/components/empty-state'
import { HealthBadge, LifecycleBadge } from '@/components/status-badge'
import { Button } from '@/components/ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { applications } from '@/data/mock/scenario'
import { ApplicationOverview } from './application-overview'

const tabs = [
  { id: 'overview', label: 'Overview' },
  {
    id: 'health',
    label: 'Health',
    title: 'A deeper view of application health',
    description:
      'Component observations, check history and service health will live here. The Overview already contains this application’s sample health.',
  },
  {
    id: 'infrastructure',
    label: 'Infrastructure',
    title: 'The resources behind this application',
    description:
      'Service topology and infrastructure relationships will live here. Explore the shared resources in the workspace view.',
    link: '/infrastructure',
    linkLabel: 'View infrastructure',
  },
  {
    id: 'activity',
    label: 'Activity',
    title: 'An operational history, in context',
    description:
      'This view will bring together deployments, health changes and important events for this application. Sample events are available in Overview and the global timeline.',
    link: '/activity',
    linkLabel: 'View global activity',
  },
  {
    id: 'security',
    label: 'Security',
    title: 'Security with application context',
    description:
      'Findings, credential references and configuration checks will be linked here. No security scan is running.',
    link: '/security',
    linkLabel: 'View security center',
  },
  {
    id: 'automations',
    label: 'Automations',
    title: 'From a trigger to a verified outcome',
    description:
      'Application workflows and their run history will live here. Preview the workflow model in the workspace view.',
    link: '/automations',
    linkLabel: 'View automation previews',
  },
] as const

export function ApplicationDetailPage() {
  const { applicationId } = useParams()
  const [params, setParams] = useSearchParams()
  const app = applications.find((item) => item.id === applicationId)
  if (!app)
    return (
      <EmptyState
        page
        title="Application not found"
        description="This application is not part of the current workspace."
        action={
          <Button asChild variant="outline">
            <Link to="/applications">Back to applications</Link>
          </Button>
        }
      />
    )
  const tab = tabs.find((item) => item.id === params.get('tab'))?.id ?? 'overview'
  return (
    <div className="page-stack">
      <Link to="/applications" className="quiet-link">
        <ArrowLeft className="size-3.5" />
        All applications
      </Link>
      <header className="flex flex-wrap items-center justify-between gap-5">
        <div className="flex items-center gap-5">
          <AppLogo app={app} large />
          <div>
            <h1 className="text-[28px] font-semibold tracking-[-0.04em]">{app.name}</h1>
            <p className="mt-1.5 text-[13px] text-muted-foreground">{app.description}</p>
            <div className="mt-3 flex items-center gap-2">
              <LifecycleBadge lifecycle={app.lifecycle} />
              <span className="font-mono text-[11px] text-muted-foreground">
                {app.version ? `v${app.version}` : 'Pre-release'}
              </span>
            </div>
          </div>
        </div>
        <HealthBadge health={app.health} />
      </header>
      <Tabs
        value={tab}
        onValueChange={(value) => setParams(value === 'overview' ? {} : { tab: value })}
        className="gap-7"
      >
        <div className="overflow-x-auto border-b pb-1">
          <TabsList variant="line" aria-label="Application sections" className="gap-4">
            {tabs.map((item) => (
              <TabsTrigger
                value={item.id}
                key={item.id}
                className="px-1 pb-3 text-xs after:bg-primary"
              >
                {item.label}
              </TabsTrigger>
            ))}
          </TabsList>
        </div>
        <TabsContent value="overview">
          <ApplicationOverview app={app} />
        </TabsContent>
        {tabs
          .filter((item) => item.id !== 'overview')
          .map((item) => (
            <TabsContent key={item.id} value={item.id}>
              <p className="eyebrow mb-4">{app.name} · planned capability</p>
              <EmptyState
                title={item.title}
                description={item.description}
                action={
                  'link' in item ? (
                    <Button asChild variant="outline">
                      <Link to={item.link}>{item.linkLabel}</Link>
                    </Button>
                  ) : undefined
                }
              />
            </TabsContent>
          ))}
      </Tabs>
    </div>
  )
}
