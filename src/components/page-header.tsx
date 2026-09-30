import type { ReactNode } from 'react'

export function PageHeader({
  eyebrow,
  title,
  description,
  action,
}: {
  eyebrow?: string
  title: string
  description: string
  action?: ReactNode
}) {
  return (
    <header className="flex flex-wrap items-end justify-between gap-4">
      <div>
        {eyebrow && <p className="eyebrow mb-2.5">{eyebrow}</p>}
        <h1 className="text-[28px] font-semibold leading-tight tracking-[-0.045em]">{title}</h1>
        <p className="mt-2 text-[13px] leading-relaxed text-muted-foreground">{description}</p>
      </div>
      {action}
    </header>
  )
}
