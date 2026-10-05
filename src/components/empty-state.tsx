import { CircleDashed } from 'lucide-react'
import type { ReactNode } from 'react'

export function EmptyState({
  title,
  description,
  action,
  page = false,
}: {
  title: string
  description: string
  action?: ReactNode
  /** The state is the whole page (a view or record that does not exist): it titles the page. */
  page?: boolean
}) {
  const Heading = page ? 'h1' : 'h2'
  return (
    <div className="flex flex-col items-center rounded-xl border border-dashed px-6 py-14 text-center">
      <CircleDashed className="mb-4 size-7 text-muted-foreground" aria-hidden="true" />
      <Heading className="font-medium">{title}</Heading>
      <p className="mt-2 max-w-md text-sm leading-relaxed text-muted-foreground">{description}</p>
      {action && <div className="mt-5">{action}</div>}
    </div>
  )
}
