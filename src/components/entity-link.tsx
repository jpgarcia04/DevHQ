import { Link } from 'react-router-dom'
import { applications, resources } from '@/data/mock/scenario'
import type { EntityRef } from '@/domain/models'
import { cn } from '@/shared/utils'

function entityName(subject: EntityRef) {
  return (
    (subject.type === 'application' ? applications : resources).find(
      (item) => item.id === subject.id,
    )?.name ?? 'Unknown entity'
  )
}

export function EntityLink({ subject, className }: { subject: EntityRef; className?: string }) {
  return (
    <Link
      className={cn(
        'rounded-sm text-muted-foreground hover:text-primary hover:underline',
        className,
      )}
      to={
        subject.type === 'application'
          ? `/applications/${subject.id}`
          : `/infrastructure?resource=${subject.id}`
      }
    >
      {entityName(subject)}
    </Link>
  )
}
