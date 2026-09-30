import { Link, Route, Routes } from 'react-router-dom'
import { Shell } from './shell'
import { OverviewPage } from '@/features/overview/overview-page'
import { ApplicationsPage } from '@/features/applications/applications-page'
import { ApplicationDetailPage } from '@/features/applications/application-detail-page'
import { InfrastructurePage } from '@/features/infrastructure/infrastructure-page'
import { AutomationsPage } from '@/features/automations/automations-page'
import { IncidentsPage } from '@/features/incidents/incidents-page'
import { ActivityPage } from '@/features/activity/activity-page'
import { SecurityPage } from '@/features/security/security-page'
import { SettingsPage } from '@/features/settings/settings-page'
import { EmptyState } from '@/components/empty-state'
import { Button } from '@/components/ui/button'
import { useInventory } from '@/features/inventory/inventory-context'
import { InventoryProvider } from '@/features/inventory/inventory-provider'
import {
  RealApplicationDetailPage,
  RealApplicationsPage,
  RealInfrastructurePage,
  RealOverviewPage,
  RealSettingsPage,
  RealUnavailablePage,
} from '@/features/inventory/real-pages'
import { InventoryState } from '@/features/inventory/inventory-state'

export function App() {
  return (
    <InventoryProvider>
      <AppRoutes />
    </InventoryProvider>
  )
}

function AppRoutes() {
  const { mode } = useInventory()
  const demo = mode === 'demo'
  return (
    <Routes>
      <Route element={<Shell />}>
        <Route index element={demo ? <OverviewPage /> : <RealOverviewPage />} />
        <Route
          path="applications"
          element={demo ? <ApplicationsPage /> : <RealApplicationsPage />}
        />
        <Route
          path="applications/:applicationId"
          element={demo ? <ApplicationDetailPage /> : <RealApplicationDetailPage />}
        />
        <Route
          path="infrastructure"
          element={demo ? <InfrastructurePage /> : <RealInfrastructurePage />}
        />
        <Route
          path="automations"
          element={demo ? <AutomationsPage /> : <RealUnavailablePage title="Automations" />}
        />
        <Route
          path="incidents"
          element={demo ? <IncidentsPage /> : <RealUnavailablePage title="Incidents" />}
        />
        <Route
          path="activity"
          element={demo ? <ActivityPage /> : <RealUnavailablePage title="Activity" />}
        />
        <Route
          path="security"
          element={
            demo ? (
              <SecurityPage />
            ) : (
              <InventoryState>
                <SecurityPage />
              </InventoryState>
            )
          }
        />
        <Route path="settings" element={demo ? <SettingsPage /> : <RealSettingsPage />} />
        <Route
          path="*"
          element={
            <EmptyState
              title="View not found"
              description="This view is not part of the current workspace."
              action={
                <Button variant="outline" asChild>
                  <Link to="/">Back to overview</Link>
                </Button>
              }
            />
          }
        />
      </Route>
    </Routes>
  )
}
