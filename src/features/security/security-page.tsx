import {
  FileKey2,
  Fingerprint,
  KeyRound,
  Network,
  PackageSearch,
  Settings2,
  ShieldCheck,
  TerminalSquare,
} from 'lucide-react'
import { PageHeader } from '@/components/page-header'

const areas = [
  {
    title: 'Secrets',
    icon: KeyRound,
    description:
      'Credential references, ownership and rotation context. Secret values will stay in secure native storage.',
  },
  {
    title: 'SSH keys',
    icon: TerminalSquare,
    description: 'Key inventory and the resources each key can access.',
  },
  {
    title: 'Certificates',
    icon: FileKey2,
    description: 'Certificate validity, expiry dates and affected services.',
  },
  {
    title: 'Dependencies',
    icon: PackageSearch,
    description: 'Vulnerability findings connected to application versions.',
  },
  {
    title: 'Exposed ports',
    icon: Network,
    description: 'Expected network exposure compared with observed services.',
  },
  {
    title: 'Login attempts',
    icon: Fingerprint,
    description: 'Failed authentication events and patterns worth investigating.',
  },
  {
    title: 'Security checks',
    icon: ShieldCheck,
    description: 'Evidence, outcomes and the history behind each check.',
  },
  {
    title: 'Configuration',
    icon: Settings2,
    description: 'Configuration warnings with context and affected resources.',
  },
]

export function SecurityPage() {
  return (
    <div className="page-stack">
      <PageHeader
        eyebrow="Awareness before assurance"
        title="Security"
        description="A future home for security context across your applications and infrastructure."
      />
      <section className="panel flex items-start gap-4 p-6">
        <span className="rounded-xl border p-3 text-primary">
          <ShieldCheck className="size-6" aria-hidden="true" />
        </span>
        <div>
          <h2 className="text-base font-medium">Visibility starts with evidence.</h2>
          <p className="mt-2 max-w-2xl text-xs leading-6 text-muted-foreground">
            No security sources are connected. An empty finding list does not mean a secure system.
            These areas describe the intended scope; no secrets are stored and no scans are
            performed.
          </p>
          <span className="mt-3 inline-block rounded border px-2 py-1 text-[10px] text-muted-foreground">
            Coverage not assessed
          </span>
        </div>
      </section>
      <section aria-labelledby="security-areas">
        <h2 id="security-areas" className="section-heading mb-5">
          Planned areas
        </h2>
        <div className="grid grid-cols-1 gap-x-8 min-[1060px]:grid-cols-2">
          {areas.map(({ title, icon: Icon, description }) => (
            <div key={title} className="flex gap-4 border-t py-5">
              <Icon className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
              <div>
                <h3 className="text-[13px] font-medium">{title}</h3>
                <p className="mt-2 text-xs leading-6 text-muted-foreground">{description}</p>
              </div>
            </div>
          ))}
        </div>
      </section>
    </div>
  )
}
