import { BrainCircuit, Clapperboard, House, ShieldCheck } from 'lucide-react'
import type { Application } from '@/domain/models'
import { cn } from '@/shared/utils'

const marks = { shield: ShieldCheck, brain: BrainCircuit, home: House, clapperboard: Clapperboard }
export function AppLogo({ app, large = false }: { app: Application; large?: boolean }) {
  const Icon = marks[app.mark]
  return (
    <span
      className={cn(
        'app-art flex shrink-0 items-center justify-center rounded-xl border border-current/15',
        `art-${app.artwork}`,
        large ? 'size-16' : 'size-10',
      )}
    >
      {app.logoUrl ? (
        <img src={app.logoUrl} alt="" className="size-full rounded-[inherit] object-cover" />
      ) : (
        <Icon aria-hidden="true" className={large ? 'size-8' : 'size-5'} strokeWidth={1.5} />
      )}
    </span>
  )
}
