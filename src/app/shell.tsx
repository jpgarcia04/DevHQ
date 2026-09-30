import { useEffect, useRef } from 'react'
import { ChevronRight, Circle, Command, PanelLeft, Sparkles } from 'lucide-react'
import { NavLink, Outlet, Link, useLocation } from 'react-router-dom'
import { navigation } from './navigation'
import { applications, incidents, scenario } from '@/data/mock/scenario'
import { activeIncidents } from '@/domain/selectors'
import { cn } from '@/shared/utils'
import { formatTime } from '@/shared/time'
import { useInventory } from '@/features/inventory/inventory-context'

export function Shell() {
  const { mode, setMode, nativeAvailable, snapshot, loadState } = useInventory()
  const { pathname } = useLocation()
  const mainRef = useRef<HTMLElement>(null)
  const previousPath = useRef(pathname)
  const section = navigation.find((item) =>
    item.path === '/'
      ? pathname === '/'
      : pathname === item.path || pathname.startsWith(`${item.path}/`),
  )
  const app = (mode === 'demo' ? applications : (snapshot?.applications ?? [])).find(
    (item) => pathname === `/applications/${item.id}`,
  )
  // Real counts appear only after a successful load; a failed load must not read as zero.
  const activeCount =
    mode === 'demo'
      ? activeIncidents(incidents).length
      : loadState === 'ready' && snapshot
        ? activeIncidents(snapshot.incidents).length
        : null
  const unavailable = mode === 'real' && loadState === 'error'
  useEffect(() => {
    document.title = `${app?.name ?? section?.label ?? 'Not found'} · DevHQ`
    if (previousPath.current !== pathname) {
      mainRef.current?.scrollTo?.(0, 0)
      mainRef.current?.focus({ preventScroll: true })
      previousPath.current = pathname
    }
  }, [pathname, app?.name, section?.label])

  return (
    <div className="flex h-dvh min-h-[540px] overflow-hidden">
      <a
        href="#main-content"
        onClick={(event) => {
          event.preventDefault()
          mainRef.current?.focus()
        }}
        className="fixed left-4 top-3 z-50 -translate-y-24 rounded bg-primary px-4 py-2 text-primary-foreground focus:translate-y-0"
      >
        Skip to content
      </a>
      <aside className="flex w-[210px] shrink-0 flex-col overflow-y-auto border-r bg-[#14161b] xl:w-[232px]">
        <Link
          to="/"
          aria-label="DevHQ overview"
          className="mx-5 mb-8 mt-6 flex w-fit items-center gap-2.5 rounded-md"
        >
          <span className="flex size-8 items-center justify-center rounded-lg bg-primary/12 text-primary">
            <Command className="size-4.5" strokeWidth={2} aria-hidden="true" />
          </span>
          <span className="text-xl font-semibold tracking-[-0.05em]">
            DevHQ<span className="text-primary">.</span>
          </span>
          <span className="ml-1 rounded border px-1 py-0.5 font-mono text-[9px] text-muted-foreground">
            0.1
          </span>
        </Link>
        <div className="mx-4 mb-7 rounded-lg border bg-background/30 px-3 py-3">
          <p className="flex items-center gap-2 text-xs font-medium">
            <span className="flex size-5 items-center justify-center rounded bg-[#2b2935] font-mono text-[10px] text-primary">
              P
            </span>
            {mode === 'demo' ? scenario.name : 'Local inventory'}
          </p>
          <p className="ml-7 mt-1 text-[10px] text-muted-foreground">
            {mode === 'demo' ? 'Fixed example · not live' : 'Stored on this device'}
          </p>
        </div>
        <div className="mx-4 mb-5 flex rounded-md border p-1 text-[11px]" aria-label="Data source">
          {nativeAvailable && (
            <button
              type="button"
              className={`flex-1 rounded px-2 py-1.5 ${mode === 'real' ? 'bg-primary/15 text-primary' : 'text-muted-foreground'}`}
              aria-pressed={mode === 'real'}
              onClick={() => setMode('real')}
            >
              My inventory
            </button>
          )}
          <button
            type="button"
            className={`flex-1 rounded px-2 py-1.5 ${mode === 'demo' ? 'bg-primary/15 text-primary' : 'text-muted-foreground'}`}
            aria-pressed={mode === 'demo'}
            onClick={() => setMode('demo')}
          >
            Demo
          </button>
        </div>
        <p className="eyebrow px-6 pb-3">Workspace</p>
        <nav aria-label="Main navigation" className="space-y-1 px-3">
          {navigation
            .filter((item) => item.path !== '/settings')
            .map(({ path, label, icon: Icon }) => (
              <NavLink
                key={path}
                to={path}
                aria-label={label}
                end={path === '/'}
                className={({ isActive }) =>
                  cn(
                    'flex items-center gap-3 rounded-md px-3 py-2.5 text-[13px] transition-colors',
                    isActive
                      ? 'bg-[#2a2636] font-medium text-[#d7cafa]'
                      : 'text-muted-foreground hover:bg-white/4 hover:text-foreground',
                  )
                }
              >
                <Icon className="size-4" strokeWidth={1.65} aria-hidden="true" />
                {label}
                {path === '/incidents' && activeCount !== null && (
                  <span className="ml-auto rounded bg-amber-300/10 px-1.5 py-0.5 font-mono text-[10px] text-amber-200">
                    {activeCount}
                  </span>
                )}
              </NavLink>
            ))}
        </nav>
        <div className="sidebar-settings mt-auto px-3 pb-4 pt-8">
          <div className="sidebar-preview mb-4 border-b px-3 pb-5">
            <p className="flex items-center gap-2 text-xs text-muted-foreground">
              <Sparkles className="size-3.5 text-primary" aria-hidden="true" />
              Foundation preview
            </p>
            <p className="mt-2 text-[11px] leading-5 text-muted-foreground">
              A connected view.
              <br />A calmer way to operate.
            </p>
          </div>
          <NavLink
            to="/settings"
            className={({ isActive }) =>
              cn(
                'flex items-center gap-3 rounded-md px-3 py-2.5 text-[13px]',
                isActive ? 'bg-accent text-primary' : 'text-muted-foreground hover:text-foreground',
              )
            }
          >
            <PanelLeft className="size-4" aria-hidden="true" />
            Settings
          </NavLink>
        </div>
      </aside>
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex h-14 shrink-0 items-center justify-between border-b px-7 text-xs">
          <div className="flex min-w-0 items-center gap-2.5 text-muted-foreground">
            <span>Workspace</span>
            <ChevronRight className="size-3" aria-hidden="true" />
            <Link className="shrink-0 rounded-sm text-foreground" to={section?.path ?? '/'}>
              {section?.label ?? 'Not found'}
            </Link>
            {app && (
              <>
                <ChevronRight className="size-3 shrink-0" aria-hidden="true" />
                <span className="truncate text-foreground">{app.name}</span>
              </>
            )}
          </div>
          <span
            className={cn(
              'ml-4 flex shrink-0 items-center gap-2 rounded-full border px-2.5 py-1 text-[10px]',
              unavailable ? 'border-rose-300/30 text-rose-300' : 'text-muted-foreground',
            )}
          >
            <Circle
              className={cn(
                'size-1.5',
                unavailable ? 'fill-rose-300' : 'fill-primary text-primary',
              )}
              aria-hidden="true"
            />
            {mode === 'demo'
              ? 'Demo · example data'
              : unavailable
                ? 'Local inventory · unavailable'
                : 'Local inventory · manual evidence'}
          </span>
        </div>
        <main
          id="main-content"
          tabIndex={-1}
          ref={mainRef}
          className="flex-1 overflow-y-auto outline-none"
        >
          <div className="mx-auto max-w-[1500px] p-7 pb-10 xl:px-10 xl:pt-9">
            <Outlet />
          </div>
        </main>
        <footer className="flex h-7 shrink-0 items-center justify-between border-t px-5 font-mono text-[9px] text-muted-foreground">
          <span className="flex items-center gap-2">
            <span className="size-1 rounded-full bg-primary" />
            {mode === 'demo'
              ? 'DEMO DATA · NO LIVE CONNECTIONS'
              : 'LOCAL INVENTORY · MANUAL ENTRIES · NO AUTOMATED CHECKS'}
          </span>
          <span>
            {mode === 'demo'
              ? `Snapshot · 24 Sep 2026 · ${formatTime(scenario.observedAt, scenario.timeZone)} CEST`
              : 'SQLite · device only'}
          </span>
        </footer>
      </div>
    </div>
  )
}
