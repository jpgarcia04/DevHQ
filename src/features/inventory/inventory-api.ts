import { invoke } from '@tauri-apps/api/core'
import type { ComponentKind, Lifecycle } from '@/domain/models'

export interface InventoryResource {
  id: string
  name: string
  kind: 'server' | 'workstation'
  description: string
  operatingSystem: string
}

export interface InventoryComponent {
  id: string
  name: string
  kind: ComponentKind
  resourceId: string | null
}

export interface InventoryApplication {
  id: string
  name: string
  description: string
  lifecycle: Lifecycle
  version: string | null
  components: InventoryComponent[]
}

export interface InventorySnapshot {
  applications: InventoryApplication[]
  resources: InventoryResource[]
}

export type ResourceInput = Omit<InventoryResource, 'id'> & { id?: string }
export type ApplicationInput = Omit<InventoryApplication, 'id' | 'components'> & {
  id?: string
  components: (Omit<InventoryComponent, 'id'> & { id?: string })[]
}

export const inventoryApi = {
  list: () => invoke<InventorySnapshot>('list_inventory'),
  saveResource: (input: ResourceInput) => invoke<InventoryResource>('save_resource', { input }),
  saveApplication: (input: ApplicationInput) =>
    invoke<InventoryApplication>('save_application', { input }),
}

export function inventoryError(error: unknown): string {
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message: unknown }).message
    if (typeof message === 'string') return message
  }
  if (typeof error === 'string') return error
  return 'The inventory request failed. Check the local database and try again.'
}
