import {
  Activity,
  Boxes,
  CircleAlert,
  LayoutDashboard,
  Layers3,
  Settings2,
  ShieldCheck,
  Workflow,
} from 'lucide-react'

export const navigation = [
  { path: '/', label: 'Overview', icon: LayoutDashboard },
  { path: '/applications', label: 'Applications', icon: Layers3 },
  { path: '/infrastructure', label: 'Infrastructure', icon: Boxes },
  { path: '/automations', label: 'Automations', icon: Workflow },
  { path: '/incidents', label: 'Incidents', icon: CircleAlert },
  { path: '/activity', label: 'Activity', icon: Activity },
  { path: '/security', label: 'Security', icon: ShieldCheck },
  { path: '/settings', label: 'Settings', icon: Settings2 },
] as const
